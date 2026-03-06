//! # Video Service
//!
//! High-level service that coordinates video decoding, caching, and texture management.
//! This is the main entry point for the UI to interact with video files.

use crate::cache::{FrameCache, FrameKey};
use crate::decode::{DecodeError, VideoDecoder, VideoFrame};
use crate::ffmpeg::VideoMetadata;
use crate::pipe_decoder::PipeDecoder;
use crate::worker::{DecodeResult, DecodeWorker};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Status of a frame request
#[derive(Debug, Clone)]
pub enum FrameStatus {
    /// Frame is ready (cached or just decoded)
    Ready(Arc<VideoFrame>),
    /// Frame is being decoded
    Loading,
    /// Error occurred during decode
    Error(String),
}

// TODO: For async decode (Phase 1)
// #[derive(Debug)]
// struct DecodeRequest {
//     key: FrameKey,
//     path: PathBuf,
//     timestamp: Duration,
//     width: u32,
//     height: u32,
// }

/// High-level video service
///
/// Manages video decoding, caching, and provides a simple interface
/// for the UI layer to request frames.
///
/// ## Decode Modes
/// - **Playback mode**: Uses PipeDecoder for continuous frame streaming (30fps+)
/// - **Random access mode**: Uses subprocess decoder for seeking/scrubbing
pub struct VideoService {
    decoder: VideoDecoder, // For random access and metadata
    cache: Arc<FrameCache>,
    /// Currently loaded video metadata
    current_video: Option<LoadedVideo>,
    /// Frame buffer ahead of playhead (pre-decoded frames)
    buffer_ahead_count: usize,
    /// Async decode worker (for random access)
    worker: DecodeWorker,
    /// Receiver for decode results (wrapped in Mutex for Sync)
    result_rx: Mutex<mpsc::Receiver<DecodeResult>>,
    /// Pipe decoder for playback mode (continuous frames)
    pipe_decoder: Option<PipeDecoder>,
    /// Whether we're in playback mode
    playback_mode: bool,
    /// Whether we're in scrubbing mode (rapid seeking)
    /// When true, uses optimized decode path that cancels stale requests
    is_scrubbing: bool,
    /// Track pending (in-flight) decode requests to avoid duplicate queuing
    /// Key is (timestamp_ms, width, height) - path is assumed to be current_video
    pending_requests: Mutex<HashSet<(u64, u32, u32)>>,
    /// Track timestamps that have FAILED decoding — prevents infinite retry loops.
    ///
    /// When a decode request fails (e.g., FFmpeg can't decode at EOF or
    /// corrupted data), the same FrameKey is added here. Subsequent requests
    /// for the same key immediately return Error instead of flooding the
    /// decode worker with doomed requests. Cleared on video load or seek.
    failed_requests: Mutex<HashSet<(u64, u32, u32)>>,
    /// Cooldown timer to prevent re-seeking the pipe decoder every frame.
    ///
    /// After a seek, FFmpeg needs ~200-500ms to restart and fill the buffer.
    /// Without a cooldown, the drift check triggers a new seek every frame,
    /// creating an infinite restart loop that freezes playback.
    last_seek_instant: Option<Instant>,
}

/// Information about a loaded video
#[derive(Debug, Clone)]
pub struct LoadedVideo {
    pub path: PathBuf,
    pub metadata: VideoMetadata,
}

impl VideoService {
    /// Create a new video service
    pub fn new() -> Result<Self, DecodeError> {
        let (worker, result_rx) = DecodeWorker::spawn();
        Ok(Self {
            decoder: VideoDecoder::new()?,
            cache: Arc::new(FrameCache::default()),
            current_video: None,
            buffer_ahead_count: 30, // 1 second at 30fps
            worker,
            result_rx: Mutex::new(result_rx),
            pipe_decoder: None,
            playback_mode: false,
            is_scrubbing: false,
            pending_requests: Mutex::new(HashSet::new()),
            failed_requests: Mutex::new(HashSet::new()),
            last_seek_instant: None,
        })
    }

    /// Create with custom cache size (in MB)
    pub fn with_cache_size(cache_size_mb: usize) -> Result<Self, DecodeError> {
        let (worker, result_rx) = DecodeWorker::spawn();
        Ok(Self {
            decoder: VideoDecoder::new()?,
            cache: Arc::new(FrameCache::new(cache_size_mb)),
            current_video: None,
            buffer_ahead_count: 30, // 1 second at 30fps
            worker,
            result_rx: Mutex::new(result_rx),
            pipe_decoder: None,
            playback_mode: false,
            is_scrubbing: false,
            pending_requests: Mutex::new(HashSet::new()),
            failed_requests: Mutex::new(HashSet::new()),
            last_seek_instant: None,
        })
    }

    /// Set the number of frames to pre-buffer ahead of playhead
    pub fn set_buffer_ahead(&mut self, count: usize) {
        self.buffer_ahead_count = count;
    }

    /// Load a video file and get its metadata
    pub fn load(&mut self, path: &Path) -> Result<VideoMetadata, DecodeError> {
        let metadata = self.decoder.get_metadata(path)?;

        self.current_video = Some(LoadedVideo {
            path: path.to_path_buf(),
            metadata: metadata.clone(),
        });

        // Clear failed decode cache — fresh video means fresh decode attempts
        if let Ok(mut failed) = self.failed_requests.lock() {
            failed.clear();
        }

        tracing::info!(
            "Loaded video: {} ({}x{} @ {:.2} fps, {:.2}s)",
            path.display(),
            metadata.width,
            metadata.height,
            metadata.frame_rate,
            metadata.duration.as_secs_f64()
        );

        Ok(metadata)
    }

    /// Get the currently loaded video info
    pub fn current_video(&self) -> Option<&LoadedVideo> {
        self.current_video.as_ref()
    }

    /// Request a frame at a specific timestamp
    ///
    /// Returns the frame status:
    /// - Ready: Frame is available (from cache or just decoded)
    /// - Loading: Frame is being decoded (not implemented yet - sync decode for now)
    /// - Error: Decode failed
    ///
    /// When in scrubbing mode, this automatically uses the scrub-optimized path
    /// which clears stale requests before submitting new ones.
    pub fn request_frame(&self, timestamp: Duration) -> FrameStatus {
        let video = match &self.current_video {
            Some(v) => v,
            None => return FrameStatus::Error("No video loaded".to_string()),
        };

        // Use scrub-optimized path when scrubbing (clears stale requests)
        if self.is_scrubbing {
            return self.request_frame_scrub(timestamp);
        }

        self.request_frame_at_resolution(
            &video.path,
            timestamp,
            video.metadata.width,
            video.metadata.height,
        )
    }

    pub fn request_frame_at_resolution(
        &self,
        path: &Path,
        timestamp: Duration,
        width: u32,
        height: u32,
    ) -> FrameStatus {
        let key = FrameKey::new(path.to_path_buf(), timestamp, width, height);

        // Check cache first
        if let Some(frame) = self.cache.get(&key) {
            tracing::trace!("Service: Cache HIT for {:.2}s", timestamp.as_secs_f64());
            return FrameStatus::Ready(frame);
        }

        // Check if this request is already in-flight (pending)
        let pending_key = (timestamp.as_millis() as u64, width, height);

        // Check failed cache — don't retry timestamps that already errored.
        // This prevents the infinite retry loop that floods the decode worker
        // and blocks all legitimate frame requests.
        {
            let failed = self
                .failed_requests
                .lock()
                .expect("Failed to lock failed_requests");
            if failed.contains(&pending_key) {
                return FrameStatus::Error(format!(
                    "Decode previously failed at {:.2}s — skipping retry",
                    timestamp.as_secs_f64()
                ));
            }
        }

        let already_pending = {
            let pending = self
                .pending_requests
                .lock()
                .expect("Failed to lock pending_requests");
            pending.contains(&pending_key)
        };

        if already_pending {
            // Already requested, just wait for it
            tracing::trace!(
                "Service: Request already pending for {:.2}s ({}x{})",
                timestamp.as_secs_f64(),
                width,
                height
            );
            return FrameStatus::Loading;
        }

        // Mark as pending and request async decode
        {
            let mut pending = self
                .pending_requests
                .lock()
                .expect("Failed to lock pending_requests");
            pending.insert(pending_key);
        }

        tracing::trace!(
            "Service: Cache MISS, requesting decode for {:.2}s ({}x{})",
            timestamp.as_secs_f64(),
            width,
            height
        );
        self.worker
            .request_decode(path.to_path_buf(), timestamp, width, height);

        FrameStatus::Loading
    }

    /// Poll for any completed async decode results
    /// Call this in the main update loop
    pub fn poll_results(&mut self) {
        // Lock the receiver
        let rx = self
            .result_rx
            .lock()
            .expect("Failed to lock result receiver");

        // Process up to 10 results per frame to avoid blocking too long
        let mut received_count = 0;
        for _ in 0..10 {
            match rx.try_recv() {
                Ok(result) => {
                    received_count += 1;
                    match result {
                        DecodeResult::FrameReady { key, frame } => {
                            tracing::trace!(
                                "Service: Received decoded frame at {}ms, inserting into cache",
                                key.timestamp_ms
                            );

                            // Remove from pending set
                            {
                                let pending_key =
                                    (key.timestamp_ms, key.resolution.0, key.resolution.1);
                                let mut pending = self
                                    .pending_requests
                                    .lock()
                                    .expect("Failed to lock pending_requests");
                                pending.remove(&pending_key);
                            }

                            self.cache.insert(key, frame);
                        }
                        DecodeResult::Error { key, error } => {
                            tracing::error!("Async decode error for {:?}: {}", key, error);

                            let pending_key =
                                (key.timestamp_ms, key.resolution.0, key.resolution.1);

                            // Remove from pending set
                            {
                                let mut pending = self
                                    .pending_requests
                                    .lock()
                                    .expect("Failed to lock pending_requests");
                                pending.remove(&pending_key);
                            }

                            // Add to failed set — prevents infinite retry loops.
                            // Without this, the same bad timestamp would be
                            // re-requested every frame, flooding the decode
                            // worker and blocking all legitimate requests.
                            {
                                let mut failed = self
                                    .failed_requests
                                    .lock()
                                    .expect("Failed to lock failed_requests");
                                failed.insert(pending_key);
                            }
                        }
                        DecodeResult::Stopped => {
                            tracing::warn!("Decode worker stopped");
                        }
                        DecodeResult::Cleared { count } => {
                            if count > 0 {
                                tracing::debug!("Cleared {} stale decode requests", count);
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("Service: Result channel disconnected!");
                    break;
                }
            }
        }
        if received_count > 0 {
            tracing::trace!("Service: poll_results received {} results", received_count);
        }
    }

    /// Request frame at a specific frame number
    pub fn request_frame_number(&self, frame_number: u64) -> FrameStatus {
        let video = match &self.current_video {
            Some(v) => v,
            None => return FrameStatus::Error("No video loaded".to_string()),
        };

        let frame_rate = video.metadata.frame_rate;
        let timestamp = Duration::from_secs_f64(frame_number as f64 / frame_rate);

        self.request_frame(timestamp)
    }

    /// Get a thumbnail for a video (smaller resolution for timeline)
    pub fn get_thumbnail(&self, path: &Path, timestamp: Duration, max_width: u32) -> FrameStatus {
        let metadata = match self.decoder.get_metadata(path) {
            Ok(m) => m,
            Err(e) => return FrameStatus::Error(e.to_string()),
        };

        // Calculate thumbnail size maintaining aspect ratio
        let aspect = metadata.width as f32 / metadata.height as f32;
        let thumb_width = max_width.min(metadata.width);
        let thumb_height = (thumb_width as f32 / aspect) as u32;

        self.request_frame_at_resolution(path, timestamp, thumb_width, thumb_height)
    }

    /// Pre-buffer frames around a timestamp
    ///
    /// Call this when playhead moves to ensure smooth playback.
    /// Returns the number of frames that need to be decoded.
    pub fn buffer_around(&self, timestamp: Duration) -> usize {
        let video = match &self.current_video {
            Some(v) => v,
            None => return 0,
        };

        let frame_rate = video.metadata.frame_rate;
        let frame_duration = Duration::from_secs_f64(1.0 / frame_rate);
        let mut decode_count = 0;

        // Buffer frames ahead
        for i in 0..self.buffer_ahead_count {
            let future_time = timestamp + frame_duration * i as u32;
            if future_time > video.metadata.duration {
                break;
            }

            let key = FrameKey::new(
                video.path.clone(),
                future_time,
                video.metadata.width,
                video.metadata.height,
            );

            if !self.cache.contains(&key) {
                // Request async decode
                self.worker.request_decode(
                    video.path.clone(),
                    future_time,
                    video.metadata.width,
                    video.metadata.height,
                );
                decode_count += 1;
            }
        }

        decode_count
    }

    /// Clear the frame cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Clear the failed-request cache.
    ///
    /// Call this when clip properties change (speed, reverse, loop mode) so
    /// that timestamps which previously failed (e.g., EOF at old speed) get
    /// a fresh decode attempt at the new source time.
    pub fn clear_failed_requests(&self) {
        if let Ok(mut failed) = self.failed_requests.lock() {
            failed.clear();
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            frame_count: self.cache.frame_count(),
            size_bytes: self.cache.size_bytes(),
        }
    }

    // =========================================================================
    // Scrubbing Mode (Optimized for rapid seeking)
    // =========================================================================

    /// Set scrubbing mode
    ///
    /// When true, frame requests will cancel stale pending decodes,
    /// ensuring only the latest position is decoded. This dramatically
    /// improves responsiveness during timeline scrubbing.
    pub fn set_scrubbing(&mut self, is_scrubbing: bool) {
        if self.is_scrubbing != is_scrubbing {
            self.is_scrubbing = is_scrubbing;
            if is_scrubbing {
                // Clear any pending requests when entering scrub mode
                self.worker.clear_pending();
                // Also clear our pending tracking set
                if let Ok(mut pending) = self.pending_requests.lock() {
                    pending.clear();
                }
                // Clear failed cache — scrub to a new position means fresh attempts
                if let Ok(mut failed) = self.failed_requests.lock() {
                    failed.clear();
                }
                tracing::trace!("Entered scrubbing mode");
            } else {
                tracing::trace!("Exited scrubbing mode");
            }
        }
    }

    /// Check if we're in scrubbing mode
    pub fn is_scrub_mode(&self) -> bool {
        self.is_scrubbing
    }

    /// Request a frame optimized for scrubbing
    ///
    /// This method:
    /// 1. Clears any pending decode requests (stale frames)
    /// 2. Requests only the current frame
    /// 3. Returns cached frame if available
    ///
    /// Use this during rapid timeline scrubbing for best responsiveness.
    pub fn request_frame_scrub(&self, timestamp: Duration) -> FrameStatus {
        let video = match &self.current_video {
            Some(v) => v,
            None => return FrameStatus::Error("No video loaded".to_string()),
        };

        let key: FrameKey = FrameKey::new(
            video.path.clone(),
            timestamp,
            video.metadata.width,
            video.metadata.height,
        );

        // Check cache first - if hit, no decode needed
        if let Some(frame) = self.cache.get(&key) {
            return FrameStatus::Ready(frame);
        }

        // Use scrub-optimized decode (clears pending requests first)
        self.worker.request_decode_scrub(
            video.path.clone(),
            timestamp,
            video.metadata.width,
            video.metadata.height,
        );

        FrameStatus::Loading
    }

    // =========================================================================
    // Playback Mode (Production-grade continuous decode)
    // =========================================================================

    /// Start playback mode at the given timestamp
    ///
    /// This enables the pipe decoder for continuous frame streaming.
    /// Call this when the user presses play.
    pub fn start_playback(&mut self, start_time: Duration) -> Result<(), DecodeError> {
        let video = self
            .current_video
            .as_ref()
            .ok_or_else(|| DecodeError::NoVideoLoaded)?;

        // Create and start pipe decoder at the specified time
        let mut pipe = PipeDecoder::new().map_err(|e| DecodeError::PipeDecoder(e.to_string()))?;

        pipe.load(&video.path, video.metadata.clone(), start_time)
            .map_err(|e| DecodeError::PipeDecoder(e.to_string()))?;

        self.pipe_decoder = Some(pipe);
        self.playback_mode = true;

        tracing::info!("Playback mode started at {:.2}s", start_time.as_secs_f64());

        Ok(())
    }

    /// Stop playback mode
    ///
    /// Returns to random access mode (subprocess per frame).
    pub fn stop_playback(&mut self) {
        if let Some(mut pipe) = self.pipe_decoder.take() {
            pipe.stop();
        }
        self.playback_mode = false;
        tracing::debug!("Playback mode stopped");
    }

    /// Request a frame in playback mode
    ///
    /// Returns the frame if available, or None if buffer is empty.
    /// This is much faster than request_frame() during playback.
    pub fn request_frame_playback(&self, frame_number: u64) -> Option<Arc<VideoFrame>> {
        if let Some(pipe) = &self.pipe_decoder {
            pipe.get_frame(frame_number)
        } else {
            None
        }
    }

    /// Get the latest frame from the playback buffer
    pub fn request_latest_playback_frame(&self) -> Option<Arc<VideoFrame>> {
        if let Some(pipe) = &self.pipe_decoder {
            pipe.get_latest_frame()
        } else {
            None
        }
    }

    /// Get a frame matching the requested timestamp (for synced playback)
    ///
    /// This is THE KEY method for video editors:
    /// - Syncs displayed frame with playhead position
    /// - Cleans up old frames that are no longer needed
    /// - **Auto-seeks** if source_time drifts too far from buffer
    ///   (happens when `ClipMetadata.speed` changes mid-playback)
    pub fn request_playback_frame_for_time(
        &mut self,
        timestamp: Duration,
    ) -> Option<Arc<VideoFrame>> {
        if let Some(pipe) = &self.pipe_decoder {
            // Try to get frame from current buffer
            if let Some(frame) = pipe.pop_frame_for_time(timestamp) {
                return Some(frame);
            }

            // Frame not in buffer — check if we should re-seek.
            //
            // COOLDOWN: After a seek, FFmpeg needs ~100-300ms to restart
            // and fill the buffer. Without a cooldown, the drift check
            // triggers a new seek every frame, creating an infinite
            // restart loop that freezes playback.
            if let Some(last_seek) = self.last_seek_instant {
                if last_seek.elapsed() < Duration::from_millis(200) {
                    // Still in cooldown — let the buffer fill
                    return None;
                }
            }

            // Check drift between requested time and pipe decode position
            let current_decode_pos: f64 = pipe.decode_position() as f64
                / self
                    .current_video
                    .as_ref()
                    .map_or(30.0, |v| v.metadata.frame_rate);
            let target_secs: f64 = timestamp.as_secs_f64();
            let drift: f64 = (target_secs - current_decode_pos).abs();

            if drift > 0.5 {
                tracing::info!(
                    "Pipe decoder drift {:.2}s (target={:.2}s, decode_pos={:.2}s) — re-seeking",
                    drift,
                    target_secs,
                    current_decode_pos
                );
                // Re-seek the pipe decoder to the new source_time
                self.seek_playback_to(timestamp);
            }

            None
        } else {
            None
        }
    }

    /// Re-seek the pipe decoder to a new source_time position.
    ///
    /// Called when `ClipMetadata.speed` changes during playback, causing
    /// the computed source_time to jump far from the pipe decoder's buffer.
    /// Restarts FFmpeg pipe at the new position for smooth continuation.
    fn seek_playback_to(&mut self, timestamp: Duration) {
        if let Some(ref mut pipe) = self.pipe_decoder {
            if let Err(e) = pipe.seek(timestamp) {
                tracing::error!("Failed to re-seek pipe decoder: {}", e);
            }
        }
        // Set cooldown to prevent re-seeking every frame while FFmpeg starts up
        self.last_seek_instant = Some(Instant::now());
    }

    /// Check if we're in playback mode
    pub fn is_playback_mode(&self) -> bool {
        self.playback_mode
    }

    /// Get buffer fullness (0.0 - 1.0) when in playback mode
    pub fn playback_buffer_fullness(&self) -> f64 {
        self.pipe_decoder
            .as_ref()
            .map(|p| p.buffer_fullness())
            .unwrap_or(0.0)
    }

    /// Seek during playback (restarts pipe from new position)
    pub fn seek_playback(&mut self, timestamp: Duration) -> Result<(), DecodeError> {
        if let Some(pipe) = &mut self.pipe_decoder {
            pipe.seek(timestamp)
                .map_err(|e| DecodeError::PipeDecoder(e.to_string()))?;
        }
        Ok(())
    }
}

impl Default for VideoService {
    fn default() -> Self {
        Self::new().expect("Failed to create VideoService")
    }
}

/// Cache statistics
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub frame_count: usize,
    pub size_bytes: usize,
}

impl CacheStats {
    pub fn size_mb(&self) -> f64 {
        self.size_bytes as f64 / (1024.0 * 1024.0)
    }
}

// ============================================================================
// Image Sequence Loading (For Instant Scrubbing)
// ============================================================================

/// Load a frame from a JPEG file (for image sequence proxies)
///
/// This is the core of INSTANT scrubbing - no video decoding needed!
/// Just load the JPEG file from disk and convert to VideoFrame.
///
/// Returns None if the file doesn't exist or can't be decoded.
pub fn load_frame_from_jpeg(path: &Path) -> Option<Arc<VideoFrame>> {
    use std::time::Instant;

    let start = Instant::now();

    // Load and decode JPEG (this is the slow part, but only ~2-5ms)
    let img = match image::open(path) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Failed to load JPEG {:?}: {}", path, e);
            return None;
        }
    };

    // Convert to RGBA8
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba.into_raw();

    let frame = VideoFrame::new(
        data,
        width,
        height,
        std::time::Duration::ZERO, // Timestamp will be tracked externally
        0,                         // Frame number will be tracked externally
    );

    let elapsed = start.elapsed();
    tracing::trace!(
        "Loaded JPEG frame {}x{} in {:.1}ms",
        width,
        height,
        elapsed.as_secs_f64() * 1000.0
    );

    Some(Arc::new(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_service_creation() {
        // This test will fail if FFmpeg isn't available, which is fine
        let service = VideoService::new();
        if let Ok(service) = service {
            assert!(service.current_video().is_none());
        }
    }
}
