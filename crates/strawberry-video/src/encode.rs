//! Video encoder service

use crate::ffmpeg::{FFmpeg, FFmpegError};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use thiserror::Error;

/// Video encode errors
#[derive(Error, Debug)]
pub enum EncodeError {
    #[error("FFmpeg error: {0}")]
    FFmpeg(#[from] FFmpegError),

    #[error("Encoding failed: {0}")]
    EncodingFailed(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Export settings for video encoding
#[derive(Debug, Clone)]
pub struct ExportSettings {
    /// Output width
    pub width: u32,
    /// Output height
    pub height: u32,
    /// Frame rate
    pub frame_rate: f64,
    /// Video codec (e.g., "libx264", "libx265", "prores")
    pub video_codec: String,
    /// Audio codec (e.g., "aac", "pcm_s16le")
    pub audio_codec: String,
    /// Video bitrate (e.g., "10M")
    pub video_bitrate: String,
    /// Audio bitrate (e.g., "320k")
    pub audio_bitrate: String,
    /// Pixel format (e.g., "yuv420p")
    pub pixel_format: String,
    /// Additional FFmpeg arguments
    pub extra_args: Vec<String>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frame_rate: 30.0,
            video_codec: "libx264".to_string(),
            audio_codec: "aac".to_string(),
            video_bitrate: "10M".to_string(),
            audio_bitrate: "320k".to_string(),
            pixel_format: "yuv420p".to_string(),
            extra_args: vec![],
        }
    }
}

impl ExportSettings {
    /// Create H.264 MP4 preset
    pub fn h264_mp4() -> Self {
        Self::default()
    }

    /// Create H.265/HEVC MP4 preset
    pub fn h265_mp4() -> Self {
        Self {
            video_codec: "libx265".to_string(),
            ..Self::default()
        }
    }

    /// Create ProRes preset (for editing)
    pub fn prores() -> Self {
        Self {
            video_codec: "prores_ks".to_string(),
            pixel_format: "yuv422p10le".to_string(),
            video_bitrate: "".to_string(), // ProRes is quality-based
            extra_args: vec!["-profile:v".to_string(), "3".to_string()], // ProRes 422 HQ
            ..Self::default()
        }
    }

    /// Create WebM/VP9 preset
    pub fn webm_vp9() -> Self {
        Self {
            video_codec: "libvpx-vp9".to_string(),
            audio_codec: "libopus".to_string(),
            pixel_format: "yuv420p".to_string(),
            ..Self::default()
        }
    }
}

/// Video encoder - wraps FFmpeg for export
pub struct VideoEncoder {
    ffmpeg: Arc<FFmpeg>,
}

impl VideoEncoder {
    /// Create a new video encoder
    pub fn new() -> Result<Self, EncodeError> {
        Ok(Self {
            ffmpeg: Arc::new(FFmpeg::new()?),
        })
    }

    /// Create with custom FFmpeg instance
    pub fn with_ffmpeg(ffmpeg: Arc<FFmpeg>) -> Self {
        Self { ffmpeg }
    }

    /// Encode raw frames to video file
    ///
    /// Frames should be in RGBA format, sent via stdin
    pub fn encode_frames(
        &self,
        output: &Path,
        settings: &ExportSettings,
        _frame_count: u64,
    ) -> Result<std::process::Child, EncodeError> {
        let mut args = vec![
            "-y".to_string(), // Overwrite output
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pix_fmt".to_string(),
            "rgba".to_string(),
            "-s".to_string(),
            format!("{}x{}", settings.width, settings.height),
            "-r".to_string(),
            format!("{}", settings.frame_rate),
            "-i".to_string(),
            "-".to_string(), // Input from stdin
            "-c:v".to_string(),
            settings.video_codec.clone(),
            "-pix_fmt".to_string(),
            settings.pixel_format.clone(),
        ];

        if !settings.video_bitrate.is_empty() {
            args.extend(["-b:v".to_string(), settings.video_bitrate.clone()]);
        }

        args.extend(settings.extra_args.clone());
        args.push(output.to_string_lossy().to_string());

        let child = Command::new(self.ffmpeg.ffmpeg_path())
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}

impl Default for VideoEncoder {
    fn default() -> Self {
        Self::new().expect("Failed to initialize video encoder")
    }
}
