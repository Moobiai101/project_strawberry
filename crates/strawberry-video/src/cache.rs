//! Frame cache for video playback
//!
//! Caches decoded frames for smooth playback and scrubbing.

use crate::decode::VideoFrame;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Key for cached frames
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FrameKey {
    /// Source file path
    pub path: PathBuf,
    /// Frame timestamp
    pub timestamp_ms: u64,
    /// Resolution (width, height)
    pub resolution: (u32, u32),
}

impl FrameKey {
    pub fn new(path: PathBuf, timestamp: Duration, width: u32, height: u32) -> Self {
        Self {
            path,
            timestamp_ms: timestamp.as_millis() as u64,
            resolution: (width, height),
        }
    }
}

/// Frame cache with LRU eviction
pub struct FrameCache {
    /// Cached frames
    cache: RwLock<HashMap<FrameKey, Arc<VideoFrame>>>,
    /// Maximum cache size in bytes
    max_size_bytes: usize,
    /// Current cache size in bytes
    current_size: RwLock<usize>,
}

impl FrameCache {
    /// Create a new frame cache with given max size (in MB)
    pub fn new(max_size_mb: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size_bytes: max_size_mb * 1024 * 1024,
            current_size: RwLock::new(0),
        }
    }

    /// Get a cached frame
    pub fn get(&self, key: &FrameKey) -> Option<Arc<VideoFrame>> {
        self.cache.read().get(key).cloned()
    }

    /// Insert a frame into the cache
    pub fn insert(&self, key: FrameKey, frame: Arc<VideoFrame>) -> Arc<VideoFrame> {
        let frame_size: usize = frame.data.len();

        // Evict if necessary
        while *self.current_size.read() + frame_size > self.max_size_bytes {
            if !self.evict_one() {
                break; // Cache is empty
            }
        }

        *self.current_size.write() += frame_size;
        self.cache.write().insert(key, frame.clone());
        frame
    }

    /// Check if a frame is cached
    pub fn contains(&self, key: &FrameKey) -> bool {
        self.cache.read().contains_key(key)
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        self.cache.write().clear();
        *self.current_size.write() = 0;
    }

    /// Get current cache size in bytes
    pub fn size_bytes(&self) -> usize {
        *self.current_size.read()
    }

    /// Get number of cached frames
    pub fn frame_count(&self) -> usize {
        self.cache.read().len()
    }

    /// Evict one frame (simple strategy - could be LRU later)
    fn evict_one(&self) -> bool {
        let mut cache = self.cache.write();
        if let Some(key) = cache.keys().next().cloned() {
            if let Some(frame) = cache.remove(&key) {
                *self.current_size.write() -= frame.data.len();  // FrameData::len()
                return true;
            }
        }
        false
    }

    /// Get the nearest cached frame to the requested timestamp
    /// Returns (frame, actual_timestamp_ms) or None if no matching frames
    pub fn get_nearest(
        &self,
        path: &PathBuf,
        timestamp: Duration,
        resolution: (u32, u32),
    ) -> Option<(Arc<VideoFrame>, u64)> {
        let target_ms = timestamp.as_millis() as i64;
        let cache = self.cache.read();

        let mut best: Option<(&FrameKey, &Arc<VideoFrame>, i64)> = None;

        for (key, frame) in cache.iter() {
            // Only consider frames from same file and resolution
            if &key.path == path && key.resolution == resolution {
                let diff = (key.timestamp_ms as i64 - target_ms).abs();

                match best {
                    None => best = Some((key, frame, diff)),
                    Some((_, _, best_diff)) if diff < best_diff => {
                        best = Some((key, frame, diff));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(key, frame, _)| (frame.clone(), key.timestamp_ms))
    }
}

impl Default for FrameCache {
    fn default() -> Self {
        // Default to 512MB cache
        Self::new(512)
    }
}
