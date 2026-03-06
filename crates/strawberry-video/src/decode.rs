//! Video decoder service

use crate::ffmpeg::{FFmpeg, FFmpegError, VideoMetadata};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Video decode errors
#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("FFmpeg error: {0}")]
    FFmpeg(#[from] FFmpegError),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Seek out of bounds: {0}")]
    SeekOutOfBounds(String),

    #[error("No video loaded")]
    NoVideoLoaded,

    #[error("Pipe decoder error: {0}")]
    PipeDecoder(String),
}

/// Decoded video frame
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Raw RGBA pixel data
    pub data: Vec<u8>,
    /// Frame width
    pub width: u32,
    /// Frame height
    pub height: u32,
    /// Timestamp in the video
    pub timestamp: Duration,
    /// Frame number
    pub frame_number: u64,
}

impl VideoFrame {
    /// Create a new frame from raw RGBA data
    pub fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        timestamp: Duration,
        frame_number: u64,
    ) -> Self {
        Self {
            data,
            width,
            height,
            timestamp,
            frame_number,
        }
    }
}

/// Video decoder - wraps FFmpeg for frame extraction
pub struct VideoDecoder {
    ffmpeg: Arc<FFmpeg>,
    /// Default decode resolution
    default_width: u32,
    default_height: u32,
}

impl VideoDecoder {
    /// Create a new video decoder
    pub fn new() -> Result<Self, DecodeError> {
        Ok(Self {
            ffmpeg: Arc::new(FFmpeg::new()?),
            default_width: 1920,
            default_height: 1080,
        })
    }

    /// Create with custom FFmpeg instance
    pub fn with_ffmpeg(ffmpeg: Arc<FFmpeg>) -> Self {
        Self {
            ffmpeg,
            default_width: 1920,
            default_height: 1080,
        }
    }

    /// Set default decode resolution
    pub fn set_default_resolution(&mut self, width: u32, height: u32) {
        self.default_width = width;
        self.default_height = height;
    }

    /// Get video metadata
    pub fn get_metadata(&self, path: &Path) -> Result<VideoMetadata, DecodeError> {
        if !path.exists() {
            return Err(DecodeError::FileNotFound(path.display().to_string()));
        }
        Ok(self.ffmpeg.probe(path)?)
    }

    /// Decode a single frame at the given timestamp
    pub fn decode_frame(
        &self,
        path: &Path,
        timestamp: Duration,
    ) -> Result<VideoFrame, DecodeError> {
        self.decode_frame_at_resolution(path, timestamp, self.default_width, self.default_height)
    }

    /// Decode a frame at specific resolution
    pub fn decode_frame_at_resolution(
        &self,
        path: &Path,
        timestamp: Duration,
        width: u32,
        height: u32,
    ) -> Result<VideoFrame, DecodeError> {
        if !path.exists() {
            return Err(DecodeError::FileNotFound(path.display().to_string()));
        }

        let data = self.ffmpeg.decode_frame(path, timestamp, width, height)?;
        let frame_rate = 30.0; // TODO: Get from metadata
        let frame_number = (timestamp.as_secs_f64() * frame_rate) as u64;

        Ok(VideoFrame::new(
            data,
            width,
            height,
            timestamp,
            frame_number,
        ))
    }

    /// Decode a range of frames (for buffering)
    pub fn decode_frame_range(
        &self,
        path: &Path,
        start: Duration,
        count: u32,
    ) -> Result<Vec<VideoFrame>, DecodeError> {
        self.decode_frame_range_at_resolution(
            path,
            start,
            count,
            self.default_width,
            self.default_height,
        )
    }

    /// Decode a range of frames at specific resolution
    pub fn decode_frame_range_at_resolution(
        &self,
        path: &Path,
        start: Duration,
        count: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<VideoFrame>, DecodeError> {
        if !path.exists() {
            return Err(DecodeError::FileNotFound(path.display().to_string()));
        }

        let raw_frames = self
            .ffmpeg
            .decode_frames(path, start, count, width, height)?;
        let frame_rate = 30.0; // TODO: Get from metadata
        let frame_duration = Duration::from_secs_f64(1.0 / frame_rate);

        let frames = raw_frames
            .into_iter()
            .enumerate()
            .map(|(i, data)| {
                let timestamp = start + frame_duration * i as u32;
                let frame_number = (timestamp.as_secs_f64() * frame_rate) as u64;
                VideoFrame::new(data, width, height, timestamp, frame_number)
            })
            .collect();

        Ok(frames)
    }
}

impl Default for VideoDecoder {
    fn default() -> Self {
        Self::new().expect("Failed to initialize video decoder")
    }
}
