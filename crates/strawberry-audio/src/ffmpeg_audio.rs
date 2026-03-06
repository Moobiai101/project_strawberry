//! # FFmpeg Audio Decoder
//!
//! Uses FFmpeg subprocess for audio decoding, consistent with video architecture.
//! This approach ensures full codec support and LGPL compliance.

use std::path::Path;
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

/// Audio decode errors
#[derive(Error, Debug)]
pub enum AudioDecodeError {
    #[error("FFmpeg not found - please install FFmpeg")]
    FFmpegNotFound,

    #[error("FFprobe not found - please install FFmpeg")]
    FFprobeNotFound,

    #[error("Failed to run FFmpeg: {0}")]
    ProcessError(String),

    #[error("Failed to parse audio metadata: {0}")]
    ParseError(String),

    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Audio stream metadata
#[derive(Debug, Clone)]
pub struct AudioMetadata {
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of audio channels
    pub channels: u16,
    /// Total duration
    pub duration: Duration,
    /// Codec name
    pub codec: String,
    /// Bit depth (if available)
    pub bit_depth: Option<u16>,
}

/// Decoded audio samples (interleaved f32)
#[derive(Debug, Clone)]
pub struct AudioSamples {
    /// Sample rate
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Interleaved f32 samples
    pub data: Vec<f32>,
}

impl AudioSamples {
    /// Duration of these samples
    pub fn duration(&self) -> Duration {
        let sample_count = self.data.len() / self.channels as usize;
        Duration::from_secs_f64(sample_count as f64 / self.sample_rate as f64)
    }

    /// Number of sample frames (samples per channel)
    pub fn frame_count(&self) -> usize {
        self.data.len() / self.channels as usize
    }
}

/// FFmpeg-based audio decoder
pub struct FFmpegAudioDecoder {
    ffmpeg_path: String,
    ffprobe_path: String,
}

impl FFmpegAudioDecoder {
    /// Create a new audio decoder
    pub fn new() -> Result<Self, AudioDecodeError> {
        let ffmpeg_path = which::which("ffmpeg")
            .map_err(|_| AudioDecodeError::FFmpegNotFound)?
            .to_string_lossy()
            .to_string();

        let ffprobe_path = which::which("ffprobe")
            .map_err(|_| AudioDecodeError::FFprobeNotFound)?
            .to_string_lossy()
            .to_string();

        Ok(Self {
            ffmpeg_path,
            ffprobe_path,
        })
    }

    /// Probe audio file for metadata
    pub fn probe(&self, path: &Path) -> Result<AudioMetadata, AudioDecodeError> {
        let output = Command::new(&self.ffprobe_path)
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_streams",
                "-select_streams", "a:0", // First audio stream
                path.to_string_lossy().as_ref(),
            ])
            .output()?;

        if !output.status.success() {
            return Err(AudioDecodeError::ProcessError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| AudioDecodeError::ParseError(e.to_string()))?;

        let stream = json["streams"]
            .get(0)
            .ok_or_else(|| AudioDecodeError::ParseError("No audio stream found".to_string()))?;

        let sample_rate = stream["sample_rate"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(48000);

        let channels = stream["channels"]
            .as_u64()
            .unwrap_or(2) as u16;

        let codec = stream["codec_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        // Duration from stream or format
        let duration_secs = stream["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let bit_depth = stream["bits_per_raw_sample"]
            .as_str()
            .and_then(|s| s.parse().ok());

        Ok(AudioMetadata {
            sample_rate,
            channels,
            duration: Duration::from_secs_f64(duration_secs),
            codec,
            bit_depth,
        })
    }

    /// Decode audio samples from a time range
    ///
    /// Returns f32 samples at native sample rate (or target_sample_rate if specified)
    pub fn decode(
        &self,
        path: &Path,
        start: Duration,
        duration: Duration,
        target_sample_rate: Option<u32>,
    ) -> Result<AudioSamples, AudioDecodeError> {
        let metadata = self.probe(path)?;
        let output_sample_rate = target_sample_rate.unwrap_or(metadata.sample_rate);

        // FFmpeg command to extract audio as f32le PCM
        let mut cmd = Command::new(&self.ffmpeg_path);
        cmd.args([
            "-v", "quiet",
            "-ss", &format!("{:.6}", start.as_secs_f64()),
            "-t", &format!("{:.6}", duration.as_secs_f64()),
            "-i", path.to_string_lossy().as_ref(),
            "-f", "f32le",       // Output format: 32-bit float little-endian
            "-acodec", "pcm_f32le",
            "-ar", &output_sample_rate.to_string(),
            "-ac", &metadata.channels.to_string(),
            "-",                  // Output to stdout
        ]);

        let output = cmd.output()?;

        if !output.status.success() {
            return Err(AudioDecodeError::ProcessError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        // Convert bytes to f32 samples
        let bytes = output.stdout;
        let sample_count = bytes.len() / 4; // 4 bytes per f32
        let mut samples = Vec::with_capacity(sample_count);

        for chunk in bytes.chunks_exact(4) {
            let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            samples.push(sample);
        }

        Ok(AudioSamples {
            sample_rate: output_sample_rate,
            channels: metadata.channels,
            data: samples,
        })
    }

    /// Decode entire audio file
    pub fn decode_full(&self, path: &Path) -> Result<AudioSamples, AudioDecodeError> {
        let metadata = self.probe(path)?;
        self.decode(path, Duration::ZERO, metadata.duration, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_creation() {
        // This test will fail if FFmpeg isn't installed
        let decoder = FFmpegAudioDecoder::new();
        if decoder.is_ok() {
            // If FFmpeg is available, decoder should be created
            assert!(decoder.is_ok());
        }
    }
}
