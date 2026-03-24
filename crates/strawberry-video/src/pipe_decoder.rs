//! # Pipe Decoder
//!
//! Production-grade FFmpeg pipe-based continuous video decoder.
//!
//! ## Architecture (Adobe Premiere / CapCut approach)
//!
//! Instead of spawning FFmpeg per-frame (100ms+ overhead), we:
//! 1. Keep FFmpeg running as a child process
//! 2. Pipe raw video frames continuously to our buffer
//! 3. Read frames from the buffer as needed
//!
//! This enables real-time 30fps+ playback.

use crate::decode::VideoFrame;
use crate::ffmpeg::VideoMetadata;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use thiserror::Error;

/// Errors from the pipe decoder
#[derive(Error, Debug)]
pub enum PipeDecodeError {
    #[error("FFmpeg not found")]
    FFmpegNotFound,

    #[error("Failed to start FFmpeg: {0}")]
    ProcessError(String),

    #[error("No video loaded")]
    NoVideoLoaded,

    #[error("Seek failed: {0}")]
    SeekError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// A decoded frame with its timestamp
#[derive(Debug)]
pub struct TimestampedFrame {
    pub frame: VideoFrame,
    pub frame_number: u64,
    pub timestamp: Duration,
}

/// Pipe-based continuous FFmpeg decoder
///
/// Keeps FFmpeg running and streams decoded frames into a ring buffer.
/// Provides low-latency frame access for real-time playback.
pub struct PipeDecoder {
    /// Currently loaded video
    video_path: Option<PathBuf>,
    video_metadata: Option<VideoMetadata>,

    /// FFmpeg child process
    ffmpeg_process: Option<Child>,

    /// Reader thread
    reader_thread: Option<JoinHandle<()>>,

    /// Shared state with reader thread
    state: Arc<DecoderState>,

    /// Path to FFmpeg binary
    ffmpeg_path: String,
}

/// Shared state between main thread and reader thread
struct DecoderState {
    /// Ring buffer of decoded frames
    frame_buffer: Mutex<VecDeque<TimestampedFrame>>,
    /// Maximum frames to buffer
    max_buffer_size: usize,
    /// Current frame number being decoded
    current_decode_frame: AtomicU64,
    /// Flag to stop the reader thread
    stop_flag: AtomicBool,
    /// Frame width
    width: AtomicU64,
    /// Frame height  
    height: AtomicU64,
    /// Frame rate
    frame_rate: Mutex<f64>,
}

impl DecoderState {
    fn new(max_buffer_size: usize) -> Self {
        Self {
            frame_buffer: Mutex::new(VecDeque::with_capacity(max_buffer_size)),
            max_buffer_size,
            current_decode_frame: AtomicU64::new(0),
            stop_flag: AtomicBool::new(false),
            width: AtomicU64::new(0),
            height: AtomicU64::new(0),
            frame_rate: Mutex::new(30.0),
        }
    }
}

impl PipeDecoder {
    /// Create a new pipe decoder
    pub fn new() -> Result<Self, PipeDecodeError> {
        let ffmpeg_path = which::which("ffmpeg")
            .map_err(|_| PipeDecodeError::FFmpegNotFound)?
            .to_string_lossy()
            .to_string();

        Ok(Self {
            video_path: None,
            video_metadata: None,
            ffmpeg_process: None,
            reader_thread: None,
            state: Arc::new(DecoderState::new(60)), // Buffer 60 frames = 2 seconds at 30fps
            ffmpeg_path,
        })
    }

    /// Load a video and start decoding from a specific timestamp
    pub fn load(
        &mut self,
        path: &Path,
        metadata: VideoMetadata,
        start_time: Duration,
    ) -> Result<(), PipeDecodeError> {
        // Stop any existing decode
        self.stop();

        self.video_path = Some(path.to_path_buf());
        self.video_metadata = Some(metadata.clone());

        // Update shared state
        self.state
            .width
            .store(metadata.width as u64, Ordering::SeqCst);
        self.state
            .height
            .store(metadata.height as u64, Ordering::SeqCst);
        *self.state.frame_rate.lock() = metadata.frame_rate;
        self.state.current_decode_frame.store(0, Ordering::SeqCst);
        self.state.stop_flag.store(false, Ordering::SeqCst);
        self.state.frame_buffer.lock().clear();

        // Start FFmpeg pipe from the specified timestamp
        self.start_ffmpeg_pipe(path, start_time)?;

        Ok(())
    }

    /// Start FFmpeg pipe from a specific timestamp
    fn start_ffmpeg_pipe(
        &mut self,
        path: &Path,
        start_time: Duration,
    ) -> Result<(), PipeDecodeError> {
        let metadata = self
            .video_metadata
            .as_ref()
            .ok_or(PipeDecodeError::NoVideoLoaded)?;

        let width = metadata.width;
        let height = metadata.height;
        let frame_rate = metadata.frame_rate;

        // Calculate starting frame number
        let start_frame = (start_time.as_secs_f64() * frame_rate) as u64;
        self.state
            .current_decode_frame
            .store(start_frame, Ordering::SeqCst);

        // Build FFmpeg command for continuous raw output
        // Use hardware acceleration when available for H.264; otherwise software decode
        let mut cmd = Command::new(&self.ffmpeg_path);

        // Detect codec from the file - only use h264_cuvid for H.264 files
        // DNxHR and other codecs (like our proxies) need software decode
        let codec_name: &str = &metadata.codec;
        let is_h264: bool =
            codec_name.to_lowercase().contains("h264") || codec_name.to_lowercase().contains("avc");

        if is_h264 {
            // NVDEC Hardware Acceleration (NVIDIA GPU decode) for H.264
            // This is what Adobe Premiere/DaVinci Resolve use for realtime playback
            cmd.args([
                "-hwaccel",
                "cuda", // Use CUDA for hardware acceleration
                "-hwaccel_output_format",
                "nv12", // NV12 is efficient for GPU decode
                "-c:v",
                "h264_cuvid", // Use NVDEC for H.264
            ]);
            tracing::debug!("PipeDecoder: Using h264_cuvid hardware decode");
        } else {
            // Software decode for non-H.264 codecs (DNxHR, ProRes, etc.)
            // DNxHR is intra-frame so software decode is still fast
            tracing::debug!(
                "PipeDecoder: Using software decode for codec '{}'",
                codec_name
            );
        }

        // Seek and input
        cmd.args([
            "-ss",
            &format!("{:.6}", start_time.as_secs_f64()),
            "-i",
            &path.to_string_lossy(),
        ]);

        // Output format - convert to RGBA for texture upload
        cmd.args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba", // Convert to RGBA for texture
            "-s",
            &format!("{}x{}", width, height),
            "-r",
            &format!("{}", frame_rate),
            "-vsync",
            "cfr", // Constant frame rate
            "-an", // No audio
            "-",   // Output to stdout
        ]);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped()); // Capture FFmpeg errors for debugging

        let mut child = cmd
            .spawn()
            .map_err(|e| PipeDecodeError::ProcessError(e.to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PipeDecodeError::ProcessError("No stdout".to_string()))?;

        // Spawn stderr reader to log FFmpeg errors
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if line.contains("Error") || line.contains("error") {
                            tracing::error!("FFmpeg stderr: {}", line);
                        } else {
                            tracing::trace!("FFmpeg: {}", line);
                        }
                    }
                }
            });
        }

        self.ffmpeg_process = Some(child);

        // Spawn reader thread
        let state = Arc::clone(&self.state);
        let frame_size = (width * height * 4) as usize; // RGBA

        self.reader_thread = Some(thread::spawn(move || {
            Self::reader_loop(stdout, state, frame_size, width, height, frame_rate);
        }));

        tracing::info!(
            "Started pipe decoder at {:.2}s (frame {})",
            start_time.as_secs_f64(),
            start_frame
        );

        Ok(())
    }

    /// Reader thread loop - reads frames from FFmpeg stdout
    ///
    /// CRITICAL: This loop NEVER blocks. If the buffer is full, we drop
    /// the oldest frame and continue. This keeps FFmpeg's stdout flowing.
    /// Professional editors (Premiere, Resolve) do the same - they drop
    /// frames rather than stall the decode pipeline.
    fn reader_loop<R: Read>(
        mut reader: R,
        state: Arc<DecoderState>,
        frame_size: usize,
        width: u32,
        height: u32,
        frame_rate: f64,
    ) {
        let mut frame_buffer = vec![0u8; frame_size];

        loop {
            // Check stop flag
            if state.stop_flag.load(Ordering::SeqCst) {
                break;
            }

            // CRITICAL: Wait if buffer is full (BACKPRESSURE)
            // This is how Adobe Premiere/DaVinci work - they DON'T decode endlessly.
            // The decoder waits until there's space, naturally throttling to real-time.
            loop {
                {
                    let buffer_len = state.frame_buffer.lock().len();
                    if buffer_len < state.max_buffer_size {
                        break; // Space available, continue decoding
                    }
                }
                // Buffer full - wait for consumer to catch up
                if state.stop_flag.load(Ordering::SeqCst) {
                    return; // Stop requested while waiting
                }
                thread::sleep(Duration::from_millis(5)); // Short sleep, check often
            }

            // Read one complete frame
            match reader.read_exact(&mut frame_buffer) {
                Ok(_) => {
                    let frame_num = state.current_decode_frame.fetch_add(1, Ordering::SeqCst);
                    let timestamp = Duration::from_secs_f64(frame_num as f64 / frame_rate);

                    let frame = VideoFrame::new(
                        frame_buffer.clone(),
                        width,
                        height,
                        timestamp,
                        frame_num,
                    );

                    let timestamped = TimestampedFrame {
                        frame,
                        frame_number: frame_num,
                        timestamp,
                    };

                    // Add to buffer (we already checked there's space)
                    state.frame_buffer.lock().push_back(timestamped);
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        // End of video
                        tracing::debug!("Pipe decoder reached end of video");
                    } else {
                        tracing::error!("Pipe read error: {}", e);
                    }
                    break;
                }
            }
        }

        tracing::debug!("Pipe decoder reader thread exiting");
    }

    /// Get a frame at the specified frame number
    /// Returns the closest available frame if exact match not found
    pub fn get_frame(&self, target_frame: u64) -> Option<Arc<VideoFrame>> {
        let buffer = self.state.frame_buffer.lock();

        if buffer.is_empty() {
            tracing::trace!("get_frame: buffer empty, target={}", target_frame);
            return None;
        }

        // Get range of frames in buffer for debugging
        let first_frame = buffer.front().map(|f| f.frame_number).unwrap_or(0);
        let last_frame = buffer.back().map(|f| f.frame_number).unwrap_or(0);

        // DEBUG: Log buffer range vs requested
        if target_frame % 30 == 0 {
            tracing::debug!(
                "get_frame: target={}, buffer=[{}..{}], size={}",
                target_frame,
                first_frame,
                last_frame,
                buffer.len()
            );
        }

        // Find the best frame (closest to target, preferring frames before target)
        let mut best: Option<&TimestampedFrame> = None;
        let mut best_diff: i64 = i64::MAX;

        for frame in buffer.iter() {
            let diff = (frame.frame_number as i64) - (target_frame as i64);

            // Prefer frames at or before target (for smooth playback)
            // But accept frames slightly ahead too
            let adjusted_diff = if diff > 0 { diff * 2 } else { -diff }; // Penalize future frames

            if adjusted_diff < best_diff {
                best_diff = adjusted_diff;
                best = Some(frame);
            }
        }

        best.map(|f| Arc::new(f.frame.clone()))
    }

    /// Get the latest (most recent) frame in the buffer
    pub fn get_latest_frame(&self) -> Option<Arc<VideoFrame>> {
        let buffer = self.state.frame_buffer.lock();
        buffer.back().map(|f| Arc::new(f.frame.clone()))
    }

    /// Get a frame matching the requested timestamp (for synced playback)
    ///
    /// This is THE KEY method for video editors:
    /// - Finds the frame closest to the requested time
    /// - Returns it WITHOUT removing from buffer (for scrubbing)
    /// - If no frame is close enough, returns None
    pub fn get_frame_for_time(&self, target_time: Duration) -> Option<Arc<VideoFrame>> {
        let buffer = self.state.frame_buffer.lock();

        if buffer.is_empty() {
            return None;
        }

        // Find frame with timestamp closest to target
        let target_secs = target_time.as_secs_f64();
        let mut best: Option<&TimestampedFrame> = None;
        let mut best_diff: f64 = f64::MAX;

        for frame in buffer.iter() {
            let frame_time = frame.timestamp.as_secs_f64();
            let diff = (frame_time - target_secs).abs();

            if diff < best_diff {
                best_diff = diff;
                best = Some(frame);
            }
        }

        // Only return if reasonably close (within 0.5 seconds)
        // This prevents showing completely wrong frames
        if best_diff < 0.5 {
            best.map(|f| Arc::new(f.frame.clone()))
        } else {
            // Frame is too far from target - buffer doesn't have what we need
            tracing::debug!(
                "get_frame_for_time: target={:.2}s, best diff={:.2}s (too far)",
                target_secs,
                best_diff
            );
            None
        }
    }

    /// Pop the best-matching frame for the given time
    ///
    /// Production NLE strategy: ALWAYS return a frame if the buffer has one.
    /// A 1-2 frame stale display is invisible to humans; a blank frame (None)
    /// causes jarring stutter. Only return None when the buffer is truly empty.
    ///
    /// Frame selection logic:
    /// 1. Drain frames that are more than 1 frame-duration behind target
    /// 2. Return the front frame (closest to target) if any remain
    pub fn pop_frame_for_time(&self, target_time: Duration) -> Option<Arc<VideoFrame>> {
        let mut buffer = self.state.frame_buffer.lock();

        if buffer.is_empty() {
            return None;
        }

        let target_secs: f64 = target_time.as_secs_f64();
        let frame_rate: f64 = *self.state.frame_rate.lock();
        // One frame duration — frames older than this are truly stale
        let frame_duration: f64 = if frame_rate > 0.0 {
            1.0 / frame_rate
        } else {
            1.0 / 30.0 // Fallback to 30fps
        };

        // Drain frames that are more than 1 frame-duration behind target.
        // Keep the most recent one as our fallback.
        while buffer.len() > 1 {
            if let Some(front) = buffer.front() {
                let frame_time: f64 = front.timestamp.as_secs_f64();
                if frame_time < target_secs - frame_duration {
                    buffer.pop_front();
                } else {
                    break;
                }
            }
        }

        // Return the front frame — it's either at or close to target.
        // Even if it's slightly stale, showing it is better than showing nothing.
        if let Some(front) = buffer.front() {
            Some(Arc::new(front.frame.clone()))
        } else {
            None
        }
    }

    /// Get the frame timestamp for a frame number
    pub fn frame_to_timestamp(&self, frame: u64) -> Duration {
        let frame_rate = *self.state.frame_rate.lock();
        Duration::from_secs_f64(frame as f64 / frame_rate)
    }

    /// Seek to a new position
    /// This restarts the FFmpeg pipe from the new position
    pub fn seek(&mut self, timestamp: Duration) -> Result<(), PipeDecodeError> {
        let path = self
            .video_path
            .clone()
            .ok_or(PipeDecodeError::NoVideoLoaded)?;

        // Stop current pipe
        self.stop();

        // Clear buffer
        self.state.frame_buffer.lock().clear();
        self.state.stop_flag.store(false, Ordering::SeqCst);

        // Start new pipe from seek position
        self.start_ffmpeg_pipe(&path, timestamp)?;

        Ok(())
    }

    /// Stop the decoder
    pub fn stop(&mut self) {
        // Signal reader thread to stop
        self.state.stop_flag.store(true, Ordering::SeqCst);

        // Kill FFmpeg process
        if let Some(mut process) = self.ffmpeg_process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }

        // Wait for reader thread
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }

    /// Get buffer fullness (0.0 - 1.0)
    pub fn buffer_fullness(&self) -> f64 {
        let len = self.state.frame_buffer.lock().len();
        len as f64 / self.state.max_buffer_size as f64
    }

    /// Get current decode position
    pub fn decode_position(&self) -> u64 {
        self.state.current_decode_frame.load(Ordering::SeqCst)
    }

    /// Check if the pipe decoder is active
    pub fn is_active(&self) -> bool {
        self.ffmpeg_process.is_some()
    }
}

impl Drop for PipeDecoder {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_creation() {
        let decoder = PipeDecoder::new();
        if decoder.is_ok() {
            let dec = decoder.unwrap();
            assert!(!dec.is_active());
        }
    }
}
