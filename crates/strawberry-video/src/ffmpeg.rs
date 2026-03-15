//! FFmpeg subprocess wrapper
//!
//! Provides a clean interface to FFmpeg CLI for decode/encode operations.

use crate::hw_accel::{self, HwAccelCapabilities};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

/// FFmpeg-related errors
#[derive(Error, Debug)]
pub enum FFmpegError {
    #[error("FFmpeg not found. Ensure ffmpeg is installed and in PATH.")]
    NotFound,

    #[error("FFmpeg execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid output: {0}")]
    InvalidOutput(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// FFmpeg subprocess wrapper
pub struct FFmpeg {
    /// Path to ffmpeg binary
    ffmpeg_path: PathBuf,
    /// Path to ffprobe binary
    ffprobe_path: PathBuf,
    /// Detected hardware acceleration capabilities (cached at init)
    hw_caps: HwAccelCapabilities,
}

impl FFmpeg {
    /// Create a new FFmpeg wrapper, auto-detecting binary paths
    pub fn new() -> Result<Self, FFmpegError> {
        let ffmpeg_path = Self::find_binary("ffmpeg")?;
        let ffprobe_path = Self::find_binary("ffprobe")?;

        info!("FFmpeg found at: {:?}", ffmpeg_path);
        info!("FFprobe found at: {:?}", ffprobe_path);

        // Detect GPU hardware encoders (runs once, ~50ms)
        let hw_caps: HwAccelCapabilities = hw_accel::detect_hw_encoders(&ffmpeg_path);

        Ok(Self {
            ffmpeg_path,
            ffprobe_path,
            hw_caps,
        })
    }

    /// Create with explicit paths
    pub fn with_paths(ffmpeg: PathBuf, ffprobe: PathBuf) -> Self {
        let hw_caps: HwAccelCapabilities = hw_accel::detect_hw_encoders(&ffmpeg);
        Self {
            ffmpeg_path: ffmpeg,
            ffprobe_path: ffprobe,
            hw_caps,
        }
    }

    /// Find binary in PATH or common locations
    fn find_binary(name: &str) -> Result<PathBuf, FFmpegError> {
        // Try PATH first
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }

        // Try common Windows locations
        let common_paths = [
            format!("C:\\ffmpeg\\bin\\{}.exe", name),
            format!("C:\\Program Files\\ffmpeg\\bin\\{}.exe", name),
            format!(
                "{}\\{}.exe",
                std::env::var("FFMPEG_DIR").unwrap_or_default(),
                name
            ),
        ];

        for path in &common_paths {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
        }

        // On MSYS2/MinGW, try the mingw64 path
        #[cfg(windows)]
        {
            let msys2_path = PathBuf::from(format!("C:\\msys64\\mingw64\\bin\\{}.exe", name));
            if msys2_path.exists() {
                return Ok(msys2_path);
            }
        }

        Err(FFmpegError::NotFound)
    }

    /// Get video metadata using ffprobe
    pub fn probe(&self, path: &Path) -> Result<VideoMetadata, FFmpegError> {
        let output = Command::new(&self.ffprobe_path)
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output()?;

        if !output.status.success() {
            return Err(FFmpegError::ExecutionFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        debug!("ffprobe output: {}", json_str);

        // Parse JSON
        let json: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| FFmpegError::InvalidOutput(format!("JSON parse error: {}", e)))?;

        // Find the video stream
        let streams = json["streams"]
            .as_array()
            .ok_or_else(|| FFmpegError::InvalidOutput("No streams array".to_string()))?;

        let video_stream = streams
            .iter()
            .find(|s| s["codec_type"].as_str() == Some("video"))
            .ok_or_else(|| FFmpegError::InvalidOutput("No video stream found".to_string()))?;

        // Extract video stream properties
        let width = video_stream["width"]
            .as_u64()
            .ok_or_else(|| FFmpegError::InvalidOutput("Missing width".to_string()))?
            as u32;

        let height = video_stream["height"]
            .as_u64()
            .ok_or_else(|| FFmpegError::InvalidOutput("Missing height".to_string()))?
            as u32;

        let codec = video_stream["codec_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        // Parse frame rate from "N/D" format (e.g., "30/1" or "30000/1001")
        let frame_rate = if let Some(r_frame_rate) = video_stream["r_frame_rate"].as_str() {
            parse_frame_rate(r_frame_rate)
        } else if let Some(avg_frame_rate) = video_stream["avg_frame_rate"].as_str() {
            parse_frame_rate(avg_frame_rate)
        } else {
            30.0 // Default fallback
        };

        // Get frame count (may not always be present)
        let frame_count = video_stream["nb_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        // Get duration from stream or format
        let duration_secs = video_stream["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                json["format"]["duration"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .unwrap_or(0.0);

        let duration = Duration::from_secs_f64(duration_secs);

        Ok(VideoMetadata {
            duration,
            width,
            height,
            frame_rate,
            codec,
            frame_count,
        })
    }

    /// Decode a single frame at the given timestamp
    pub fn decode_frame(
        &self,
        path: &Path,
        timestamp: Duration,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, FFmpegError> {
        let output = Command::new(&self.ffmpeg_path)
            .args(["-ss", &format!("{:.3}", timestamp.as_secs_f64()), "-i"])
            .arg(path)
            .args([
                "-vframes",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{}x{}", width, height),
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;

        if !output.status.success() {
            return Err(FFmpegError::ExecutionFailed(
                "Failed to decode frame".to_string(),
            ));
        }

        let expected_size = (width * height * 4) as usize;
        if output.stdout.len() != expected_size {
            return Err(FFmpegError::InvalidOutput(format!(
                "Expected {} bytes, got {}",
                expected_size,
                output.stdout.len()
            )));
        }

        Ok(output.stdout)
    }

    /// Decode multiple frames (for buffering)
    pub fn decode_frames(
        &self,
        path: &Path,
        start: Duration,
        count: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<Vec<u8>>, FFmpegError> {
        let output = Command::new(&self.ffmpeg_path)
            .args(["-ss", &format!("{:.3}", start.as_secs_f64()), "-i"])
            .arg(path)
            .args([
                "-vframes",
                &count.to_string(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{}x{}", width, height),
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;

        if !output.status.success() {
            return Err(FFmpegError::ExecutionFailed(
                "Failed to decode frames".to_string(),
            ));
        }

        let frame_size = (width * height * 4) as usize;
        let frames: Vec<Vec<u8>> = output
            .stdout
            .chunks(frame_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        Ok(frames)
    }

    /// Get the ffmpeg binary path
    pub fn ffmpeg_path(&self) -> &Path {
        &self.ffmpeg_path
    }

    /// Get the ffprobe binary path  
    pub fn ffprobe_path(&self) -> &Path {
        &self.ffprobe_path
    }

    /// Get detected hardware acceleration capabilities
    ///
    /// Use `hw_caps().best_encoder` to get the best available encoder
    /// for proxy generation.
    pub fn hw_caps(&self) -> &HwAccelCapabilities {
        &self.hw_caps
    }

    /// Check whether a media file contains an audio stream.
    ///
    /// Uses ffprobe to inspect the container. Returns `false` if
    /// no audio stream is found or if probing fails.
    pub fn has_audio_stream(&self, path: &Path) -> bool {
        let output = Command::new(&self.ffprobe_path)
            .args([
                "-v",
                "quiet",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();

        match output {
            Ok(out) => {
                let stdout: String = String::from_utf8_lossy(&out.stdout).to_string();
                stdout.trim().contains("audio")
            }
            Err(_) => false,
        }
    }

    /// Extract audio from a media file to WAV (PCM s16le, 44100 Hz, stereo).
    ///
    /// Uses FFmpeg to transcode any audio codec into a universally compatible
    /// format that kira/symphonia can always decode. This is the industry-standard
    /// approach used by DaVinci Resolve, Premiere Pro, and Final Cut Pro.
    ///
    /// ## Arguments
    /// - `source_path`: Path to the source media file (any container/codec)
    /// - `output_wav_path`: Where to write the extracted WAV file
    ///
    /// ## Errors
    /// - Returns `FFmpegError` if the source has no audio or extraction fails
    pub fn extract_audio_to_wav(
        &self,
        source_path: &Path,
        output_wav_path: &Path,
    ) -> Result<(), FFmpegError> {
        // Ensure parent directory exists
        if let Some(parent) = output_wav_path.parent() {
            std::fs::create_dir_all(parent).map_err(FFmpegError::Io)?;
        }

        info!(
            "Extracting audio: {:?} -> {:?}",
            source_path, output_wav_path
        );

        let output = Command::new(&self.ffmpeg_path)
            .args([
                "-y", // Overwrite output
                "-i",
            ])
            .arg(source_path)
            .args([
                "-vn", // No video
                "-acodec",
                "pcm_s16le", // 16-bit PCM (WAV)
                "-ar",
                "44100", // 44.1 kHz sample rate
                "-ac",
                "2", // Stereo
            ])
            .arg(output_wav_path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(FFmpegError::Io)?;

        if !output.status.success() {
            let stderr: String = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(FFmpegError::ExecutionFailed(format!(
                "Audio extraction failed: {}",
                stderr.lines().last().unwrap_or("unknown error")
            )));
        }

        // Verify the output file was created and is non-empty
        let file_size: u64 = std::fs::metadata(output_wav_path)
            .map_err(FFmpegError::Io)?
            .len();

        if file_size < 44 {
            // WAV header alone is 44 bytes — anything less means empty
            std::fs::remove_file(output_wav_path).ok();
            return Err(FFmpegError::ExecutionFailed(
                "Audio extraction produced empty file (source may have no audio)".to_string(),
            ));
        }

        info!("Audio extracted: {:.1} KB", file_size as f64 / 1024.0);

        Ok(())
    }
}

impl Default for FFmpeg {
    fn default() -> Self {
        Self::new().expect("FFmpeg not found")
    }
}

/// Parse frame rate from ffprobe "N/D" format (e.g., "30/1" or "30000/1001")
fn parse_frame_rate(rate_str: &str) -> f64 {
    let parts: Vec<&str> = rate_str.split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].parse::<f64>().unwrap_or(30.0);
        let den = parts[1].parse::<f64>().unwrap_or(1.0);
        if den != 0.0 {
            return num / den;
        }
    }
    // Fallback: try parsing as plain number
    rate_str.parse::<f64>().unwrap_or(30.0)
}

/// Video file metadata
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub codec: String,
    pub frame_count: u64,
}
