//! # Frame Prefetcher
//!
//! Predicts scrub direction and preloads adjacent frames on a background thread.
//! This reduces perceived latency during timeline scrubbing by loading frames
//! before they're needed.
//!
//! ## How It Works
//!
//! 1. Track the last N requested frame numbers to detect scrub direction
//! 2. When a frame is requested, predict the next 2-3 frames based on velocity
//! 3. Submit prefetch jobs to background thread (if not already cached)
//! 4. Prefetched frames go directly into JpegFrameCache for instant access
//!
//! ## Integration
//!
//! Called from compositor after each random-access frame request.
//! Works with JpegFrameCache to avoid redundant loads.

use crate::jpeg_cache::{JpegCacheKey, JpegFrameCache};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use uuid::Uuid;

/// Request to prefetch frames in the background
#[derive(Debug)]
pub struct PrefetchRequest {
    /// Asset UUID
    pub asset_uuid: Uuid,
    /// Frame numbers to prefetch
    pub frame_numbers: Vec<u32>,
    /// Proxy directory containing JPEG frames
    pub proxy_dir: PathBuf,
}

/// Frame prefetcher with direction prediction
pub struct FramePrefetcher {
    /// Recent frame request history for direction prediction
    request_history: VecDeque<(Uuid, u32)>,
    /// Maximum history size
    history_size: usize,
    /// Channel to send prefetch requests
    prefetch_tx: Option<mpsc::Sender<PrefetchRequest>>,
    /// Worker thread handle
    worker_handle: Option<JoinHandle<()>>,
    /// Shared frame cache for storing prefetched frames
    cache: Arc<Mutex<JpegFrameCache>>,
    /// Number of frames to prefetch ahead
    prefetch_count: u32,
    /// Whether prefetcher is active
    active: bool,
}

impl FramePrefetcher {
    /// Create a new prefetcher that writes to the given cache
    pub fn new(cache: Arc<Mutex<JpegFrameCache>>) -> Self {
        let (tx, rx) = mpsc::channel::<PrefetchRequest>();

        // Clone cache for worker thread
        let worker_cache = Arc::clone(&cache);

        // Spawn background worker
        let handle = thread::Builder::new()
            .name("prefetcher".to_string())
            .spawn(move || {
                Self::worker_loop(rx, worker_cache);
            })
            .expect("Failed to spawn prefetcher thread");

        Self {
            request_history: VecDeque::with_capacity(10),
            history_size: 10,
            prefetch_tx: Some(tx),
            worker_handle: Some(handle),
            cache,
            prefetch_count: 3, // Default: prefetch 3 frames ahead
            active: true,
        }
    }

    /// Create a disabled prefetcher (for testing or low-memory mode)
    pub fn disabled(cache: Arc<Mutex<JpegFrameCache>>) -> Self {
        Self {
            request_history: VecDeque::new(),
            history_size: 0,
            prefetch_tx: None,
            worker_handle: None,
            cache,
            prefetch_count: 0,
            active: false,
        }
    }

    /// Record a frame request and trigger prefetch of predicted frames
    ///
    /// Call this after each frame request (from compositor).
    pub fn on_frame_request(&mut self, asset_uuid: Uuid, frame_num: u32, proxy_dir: &PathBuf) {
        if !self.active {
            return;
        }

        // Add to history
        self.request_history.push_back((asset_uuid, frame_num));
        if self.request_history.len() > self.history_size {
            self.request_history.pop_front();
        }

        // Predict next frames
        let predicted = self.predict_next_frames(asset_uuid, frame_num);

        if predicted.is_empty() {
            return;
        }

        // Filter out frames already in cache
        let frames_to_prefetch: Vec<u32> = {
            let cache = self.cache.lock().unwrap();
            predicted
                .into_iter()
                .filter(|&f| !cache.contains(&(asset_uuid, f)))
                .collect()
        };

        if frames_to_prefetch.is_empty() {
            return;
        }

        // Submit prefetch request
        if let Some(tx) = &self.prefetch_tx {
            let request = PrefetchRequest {
                asset_uuid,
                frame_numbers: frames_to_prefetch,
                proxy_dir: proxy_dir.clone(),
            };

            // Non-blocking send - if channel is full, skip this prefetch
            if let Err(e) = tx.send(request) {
                tracing::trace!("Prefetch channel full, skipping: {}", e);
            }
        }
    }

    /// Predict the next N frames based on scrub direction
    ///
    /// Returns empty vec if direction is unclear or history is insufficient.
    fn predict_next_frames(&self, asset_uuid: Uuid, current_frame: u32) -> Vec<u32> {
        // Need at least 2 samples to detect direction
        if self.request_history.len() < 2 {
            return Vec::new();
        }

        // Get recent frames for this asset
        let recent: Vec<u32> = self
            .request_history
            .iter()
            .filter(|(uuid, _)| *uuid == asset_uuid)
            .map(|(_, f)| *f)
            .collect();

        if recent.len() < 2 {
            return Vec::new();
        }

        // Calculate velocity (frames per request, can be negative)
        let len = recent.len();
        let last = recent[len - 1] as i64;
        let prev = recent[len - 2] as i64;
        let velocity = last - prev;

        // If velocity is 0 (paused), no prediction
        if velocity == 0 {
            return Vec::new();
        }

        // Predict next frames in the direction of travel
        let mut predicted = Vec::with_capacity(self.prefetch_count as usize);
        for i in 1..=self.prefetch_count {
            let next_frame = (current_frame as i64) + (velocity * i as i64);
            if next_frame >= 0 {
                predicted.push(next_frame as u32);
            }
        }

        predicted
    }

    /// Set number of frames to prefetch ahead
    pub fn set_prefetch_count(&mut self, count: u32) {
        self.prefetch_count = count.clamp(1, 10);
    }

    /// Check if prefetcher is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Clear the request history (call when switching assets/projects)
    pub fn clear_history(&mut self) {
        self.request_history.clear();
    }

    /// Background worker loop
    fn worker_loop(rx: mpsc::Receiver<PrefetchRequest>, cache: Arc<Mutex<JpegFrameCache>>) {
        tracing::info!("Prefetcher worker started");

        while let Ok(request) = rx.recv() {
            for frame_num in request.frame_numbers {
                let key: JpegCacheKey = (request.asset_uuid, frame_num);

                // Skip if already cached (may have been loaded by main thread)
                {
                    let cache_guard = cache.lock().unwrap();
                    if cache_guard.contains(&key) {
                        continue;
                    }
                }

                // Build path to JPEG frame
                let jpeg_path = request
                    .proxy_dir
                    .join(format!("frame_{:05}.jpg", frame_num));

                if !jpeg_path.exists() {
                    continue;
                }

                // Load and decode JPEG
                match image::open(&jpeg_path) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let width = rgba.width();
                        let height = rgba.height();
                        let data = rgba.into_raw();

                        // Store in cache (frame at None since this is just raw data)
                        // Note: For now we're just pre-loading to reduce latency
                        // The actual VideoFrame creation happens when the frame is requested
                        tracing::trace!(
                            "Prefetched frame {} for asset {} ({}x{})",
                            frame_num,
                            &request.asset_uuid.to_string()[..8],
                            width,
                            height
                        );

                        // Store raw data in cache
                        // This is a simplified version - in production, we'd store
                        // the full VideoFrame. For now, we just warm the disk cache.
                        let mut cache_guard = cache.lock().unwrap();
                        if let Err(e) = cache_guard.insert_raw(key, data, width, height) {
                            tracing::debug!("Failed to cache prefetched frame: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Failed to prefetch frame {} from {:?}: {}",
                            frame_num,
                            jpeg_path,
                            e
                        );
                    }
                }
            }
        }

        tracing::info!("Prefetcher worker stopped");
    }
}

impl Drop for FramePrefetcher {
    fn drop(&mut self) {
        // Drop sender to signal worker to stop
        self.prefetch_tx.take();

        // Wait for worker to finish
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cache() -> Arc<Mutex<JpegFrameCache>> {
        Arc::new(Mutex::new(JpegFrameCache::new(100)))
    }

    #[test]
    fn test_prefetcher_creation() {
        let cache = create_test_cache();
        let prefetcher = FramePrefetcher::new(cache);
        assert!(prefetcher.is_active());
    }

    #[test]
    fn test_disabled_prefetcher() {
        let cache = create_test_cache();
        let prefetcher = FramePrefetcher::disabled(cache);
        assert!(!prefetcher.is_active());
    }

    #[test]
    fn test_direction_prediction_forward() {
        let cache = create_test_cache();
        let mut prefetcher = FramePrefetcher::disabled(cache);
        prefetcher.history_size = 10;
        prefetcher.prefetch_count = 3;

        let uuid = Uuid::new_v4();

        // Simulate forward scrubbing: 10, 11, 12
        prefetcher.request_history.push_back((uuid, 10));
        prefetcher.request_history.push_back((uuid, 11));
        prefetcher.request_history.push_back((uuid, 12));

        let predicted = prefetcher.predict_next_frames(uuid, 12);

        // Should predict: 13, 14, 15
        assert_eq!(predicted, vec![13, 14, 15]);
    }

    #[test]
    fn test_direction_prediction_backward() {
        let cache = create_test_cache();
        let mut prefetcher = FramePrefetcher::disabled(cache);
        prefetcher.history_size = 10;
        prefetcher.prefetch_count = 3;

        let uuid = Uuid::new_v4();

        // Simulate backward scrubbing: 20, 19, 18
        prefetcher.request_history.push_back((uuid, 20));
        prefetcher.request_history.push_back((uuid, 19));
        prefetcher.request_history.push_back((uuid, 18));

        let predicted = prefetcher.predict_next_frames(uuid, 18);

        // Should predict: 17, 16, 15
        assert_eq!(predicted, vec![17, 16, 15]);
    }

    #[test]
    fn test_direction_prediction_fast_forward() {
        let cache = create_test_cache();
        let mut prefetcher = FramePrefetcher::disabled(cache);
        prefetcher.history_size = 10;
        prefetcher.prefetch_count = 2;

        let uuid = Uuid::new_v4();

        // Simulate fast forward scrubbing: 10, 15, 20 (step of 5)
        prefetcher.request_history.push_back((uuid, 10));
        prefetcher.request_history.push_back((uuid, 15));
        prefetcher.request_history.push_back((uuid, 20));

        let predicted = prefetcher.predict_next_frames(uuid, 20);

        // Should predict: 25, 30 (step of 5 each)
        assert_eq!(predicted, vec![25, 30]);
    }

    #[test]
    fn test_insufficient_history() {
        let cache = create_test_cache();
        let mut prefetcher = FramePrefetcher::disabled(cache);
        prefetcher.history_size = 10;

        let uuid = Uuid::new_v4();

        // Only one sample
        prefetcher.request_history.push_back((uuid, 10));

        let predicted = prefetcher.predict_next_frames(uuid, 10);

        // Not enough history, should return empty
        assert!(predicted.is_empty());
    }

    #[test]
    fn test_clear_history() {
        let cache = create_test_cache();
        let mut prefetcher = FramePrefetcher::disabled(cache);

        let uuid = Uuid::new_v4();
        prefetcher.request_history.push_back((uuid, 10));
        prefetcher.request_history.push_back((uuid, 11));

        assert_eq!(prefetcher.request_history.len(), 2);

        prefetcher.clear_history();

        assert!(prefetcher.request_history.is_empty());
    }
}
