//! # JPEG Frame Cache
//!
//! LRU cache for recently loaded JPEG frames (image sequence proxy).
//! Keeps decoded frames in RAM for instant re-access during scrubbing.
//!
//! ## Why This Exists
//!
//! When scrubbing the timeline back and forth, the same frames are often
//! requested multiple times. Loading from disk (~2-5ms per JPEG) adds up.
//! This cache stores recently loaded frames in memory for instant access (~0.1ms).
//!
//! ## Memory Management
//!
//! Uses an LRU (Least Recently Used) eviction policy with configurable
//! memory limit. Default is 50MB which holds roughly 30-50 720p frames.
//!
//! ## Thread Safety
//!
//! The cache uses `parking_lot::RwLock` for thread-safe access.
//! Multiple readers can access the cache concurrently.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// Cache key: (asset_uuid, frame_number)
///
/// Using frame number instead of timestamp for exact cache hits.
/// Frame numbers are stable across zoom/pan operations.
pub type JpegCacheKey = (Uuid, u32);

/// A cached video frame with RGBA pixel data
///
/// This is the same format as VideoFrame in decode.rs but optimized
/// for cache storage with Arc for cheap cloning.
#[derive(Debug, Clone)]
pub struct CachedFrame {
    /// RGBA pixel data (4 bytes per pixel)
    pub data: Arc<Vec<u8>>,
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
    /// Memory size in bytes (for cache accounting)
    pub memory_size: usize,
    /// When this frame was last accessed (for LRU eviction)
    last_access: Instant,
}

impl CachedFrame {
    /// Create a new cached frame from raw RGBA data
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self {
        let memory_size = data.len();
        Self {
            data: Arc::new(data),
            width,
            height,
            memory_size,
            last_access: Instant::now(),
        }
    }

    /// Create from existing Arc data (for efficient cloning)
    pub fn from_arc(data: Arc<Vec<u8>>, width: u32, height: u32) -> Self {
        let memory_size = data.len();
        Self {
            data,
            width,
            height,
            memory_size,
            last_access: Instant::now(),
        }
    }

    /// Update last access time (called on cache hit)
    fn touch(&mut self) {
        self.last_access = Instant::now();
    }

    /// Convert to Arc<VideoFrame> for use with FrameStatus::Ready.
    ///
    /// Uses zero-copy path: shares the `Arc<Vec<u8>>` pixel data with
    /// the VideoFrame instead of cloning ~2MB. This is the critical
    /// optimization for scrubbing performance — cache hits are now
    /// near-instant (~0.01ms) instead of ~0.5ms per frame.
    pub fn to_video_frame(&self) -> std::sync::Arc<crate::decode::VideoFrame> {
        std::sync::Arc::new(crate::decode::VideoFrame::from_shared(
            Arc::clone(&self.data),
            self.width,
            self.height,
            std::time::Duration::ZERO, // Lost in cache
            0,                         // Lost in cache
        ))
    }
}

/// LRU cache for decoded JPEG frames
///
/// Provides instant access to recently loaded frames, dramatically
/// improving scrubbing responsiveness by avoiding disk I/O.
///
/// ## Usage
///
/// ```ignore
/// let cache = JpegFrameCache::new(50); // 50 MB limit
/// let key = (asset_uuid, frame_number);
///
/// // Try cache first
/// if let Some(frame) = cache.get(&key) {
///     // Instant! (~0.1ms)
///     return frame;
/// }
///
/// // Cache miss - load from disk (~2-5ms)
/// let frame = load_jpeg_from_disk(path)?;
/// cache.insert(key, frame.clone());
/// ```
pub struct JpegFrameCache {
    /// Cache storage (thread-safe)
    cache: RwLock<HashMap<JpegCacheKey, CachedFrame>>,
    /// Maximum memory usage in bytes
    max_memory_bytes: usize,
    /// Current memory usage in bytes
    current_memory_bytes: RwLock<usize>,
}

impl JpegFrameCache {
    /// Create a new cache with the specified memory limit in megabytes
    ///
    /// # Arguments
    /// * `max_memory_mb` - Maximum memory usage in MB (default: 50)
    ///
    /// # Example
    /// ```ignore
    /// let cache = JpegFrameCache::new(50); // 50 MB limit
    /// ```
    pub fn new(max_memory_mb: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_memory_bytes: max_memory_mb * 1024 * 1024,
            current_memory_bytes: RwLock::new(0),
        }
    }

    /// Get a cached frame if available
    ///
    /// Returns `Some(CachedFrame)` on cache hit, `None` on miss.
    /// Updates the frame's last access time for LRU tracking.
    ///
    /// This is very fast (~0.1ms) compared to loading from disk (~2-5ms).
    pub fn get(&self, key: &JpegCacheKey) -> Option<CachedFrame> {
        let mut cache = self.cache.write();
        if let Some(frame) = cache.get_mut(key) {
            frame.touch();
            Some(frame.clone())
        } else {
            None
        }
    }

    /// Check if a key exists in the cache without updating access time
    pub fn contains(&self, key: &JpegCacheKey) -> bool {
        self.cache.read().contains_key(key)
    }

    /// Insert a frame into the cache
    ///
    /// May evict oldest frames if memory limit is exceeded.
    /// The eviction uses LRU (Least Recently Used) policy.
    pub fn insert(&self, key: JpegCacheKey, frame: CachedFrame) {
        let frame_size = frame.memory_size;

        // Evict old frames if needed to make room
        self.evict_if_needed(frame_size);

        // Insert the new frame
        {
            let mut cache = self.cache.write();
            // If replacing an existing frame, subtract its size first
            if let Some(old_frame) = cache.remove(&key) {
                let mut current = self.current_memory_bytes.write();
                *current = current.saturating_sub(old_frame.memory_size);
            }
            cache.insert(key, frame);
        }

        // Update memory tracking
        {
            let mut current = self.current_memory_bytes.write();
            *current += frame_size;
        }

        tracing::trace!(
            "Cache insert: asset={}, frame={}, size={}KB, total={}MB/{}MB",
            &key.0.to_string()[..8],
            key.1,
            frame_size / 1024,
            *self.current_memory_bytes.read() / (1024 * 1024),
            self.max_memory_bytes / (1024 * 1024),
        );
    }

    /// Insert raw RGBA pixel data into the cache
    ///
    /// Convenience method for prefetcher to insert decoded frames directly.
    /// Returns an error if data size doesn't match expected dimensions.
    pub fn insert_raw(
        &mut self,
        key: JpegCacheKey,
        data: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let expected_size = (width * height * 4) as usize;
        if data.len() != expected_size {
            return Err(format!(
                "Data size mismatch: expected {} bytes for {}x{}, got {}",
                expected_size,
                width,
                height,
                data.len()
            ));
        }

        let frame = CachedFrame::new(data, width, height);
        self.insert(key, frame);
        Ok(())
    }

    /// Insert an Arc<VideoFrame> into the cache
    ///
    /// Convenience method for compositor integration. Clones the data.
    pub fn insert_video_frame(
        &self,
        key: JpegCacheKey,
        frame: &std::sync::Arc<crate::decode::VideoFrame>,
    ) {
        let cached = CachedFrame::new(frame.data.as_slice().to_vec(), frame.width, frame.height);
        self.insert(key, cached);
    }

    /// Evict oldest frames until there's room for `needed_bytes`k
    fn evict_if_needed(&self, needed_bytes: usize) {
        let target_bytes = self.max_memory_bytes.saturating_sub(needed_bytes);
        let current = *self.current_memory_bytes.read();

        if current <= target_bytes {
            return; // Enough room already
        }

        let mut cache = self.cache.write();
        let mut memory = self.current_memory_bytes.write();

        // Collect keys sorted by last access time (oldest first)
        let mut entries: Vec<_> = cache
            .iter()
            .map(|(k, v)| (*k, v.last_access, v.memory_size))
            .collect();
        entries.sort_by_key(|(_, access, _)| *access);

        // Evict oldest entries until we have enough room
        for (key, _, size) in entries {
            if *memory <= target_bytes {
                break;
            }
            cache.remove(&key);
            *memory = memory.saturating_sub(size);
            tracing::trace!(
                "Cache evict: asset={}, frame={}, freed={}KB",
                &key.0.to_string()[..8],
                key.1,
                size / 1024,
            );
        }
    }

    /// Clear all cached frames
    ///
    /// Call this when switching projects or when assets change.
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        cache.clear();
        *self.current_memory_bytes.write() = 0;
        tracing::debug!("JPEG cache cleared");
    }

    /// Clear all cached frames for a specific asset
    ///
    /// Call this when an asset is deleted or its proxy is regenerated.
    pub fn clear_asset(&self, asset_id: &Uuid) {
        let mut cache = self.cache.write();
        let mut memory = self.current_memory_bytes.write();

        let keys_to_remove: Vec<_> = cache
            .keys()
            .filter(|(uuid, _)| uuid == asset_id)
            .cloned()
            .collect();

        let mut freed = 0;
        for key in keys_to_remove {
            if let Some(frame) = cache.remove(&key) {
                *memory = memory.saturating_sub(frame.memory_size);
                freed += frame.memory_size;
            }
        }

        if freed > 0 {
            tracing::debug!(
                "Cleared cache for asset {}: freed {}KB",
                &asset_id.to_string()[..8],
                freed / 1024
            );
        }
    }

    /// Get current cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.read();
        CacheStats {
            frame_count: cache.len(),
            memory_used_mb: *self.current_memory_bytes.read() as f64 / (1024.0 * 1024.0),
            memory_limit_mb: self.max_memory_bytes as f64 / (1024.0 * 1024.0),
        }
    }

    /// Get list of all cached frame keys (for debugging)
    pub fn cached_keys(&self) -> Vec<JpegCacheKey> {
        self.cache.read().keys().cloned().collect()
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of frames currently cached
    pub frame_count: usize,
    /// Current memory usage in MB
    pub memory_used_mb: f64,
    /// Maximum memory limit in MB
    pub memory_limit_mb: f64,
}

impl CacheStats {
    /// Get cache fill percentage (0.0 to 100.0)
    pub fn fill_percentage(&self) -> f64 {
        if self.memory_limit_mb > 0.0 {
            (self.memory_used_mb / self.memory_limit_mb) * 100.0
        } else {
            0.0
        }
    }
}

impl Default for JpegFrameCache {
    fn default() -> Self {
        Self::new(50) // 50 MB default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_frame(size_kb: usize) -> CachedFrame {
        let data = vec![0u8; size_kb * 1024];
        CachedFrame::new(data, 100, 100)
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = JpegFrameCache::new(10); // 10 MB
        let key = (Uuid::new_v4(), 1);
        let frame = create_test_frame(100); // 100 KB

        cache.insert(key, frame.clone());
        let retrieved = cache.get(&key);

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().width, 100);
    }

    #[test]
    fn test_cache_miss() {
        let cache = JpegFrameCache::new(10);
        let key = (Uuid::new_v4(), 999);

        let result = cache.get(&key);
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_eviction() {
        // Create a 1MB cache
        let cache = JpegFrameCache::new(1);
        let asset_id = Uuid::new_v4();

        // Insert frames until we exceed the limit
        for i in 0..20 {
            let frame = create_test_frame(100); // 100 KB each
            cache.insert((asset_id, i), frame);
        }

        // Cache should have evicted some frames
        let stats = cache.stats();
        assert!(stats.memory_used_mb <= 1.0 + 0.1); // Allow small overage for last insert
    }

    #[test]
    fn test_cache_clear_asset() {
        let cache = JpegFrameCache::new(10);
        let asset1 = Uuid::new_v4();
        let asset2 = Uuid::new_v4();

        // Insert frames for two assets
        cache.insert((asset1, 1), create_test_frame(50));
        cache.insert((asset1, 2), create_test_frame(50));
        cache.insert((asset2, 1), create_test_frame(50));

        // Clear only asset1
        cache.clear_asset(&asset1);

        // asset1 frames should be gone
        assert!(cache.get(&(asset1, 1)).is_none());
        assert!(cache.get(&(asset1, 2)).is_none());

        // asset2 frame should still exist
        assert!(cache.get(&(asset2, 1)).is_some());
    }

    #[test]
    fn test_cache_stats() {
        let cache = JpegFrameCache::new(10);
        let key = (Uuid::new_v4(), 1);
        let frame = create_test_frame(1024); // 1 MB

        cache.insert(key, frame);
        let stats = cache.stats();

        assert_eq!(stats.frame_count, 1);
        assert!((stats.memory_used_mb - 1.0).abs() < 0.1);
        assert_eq!(stats.memory_limit_mb, 10.0);
    }

    #[test]
    fn test_lru_eviction_order() {
        let cache = JpegFrameCache::new(1); // 1 MB limit
        let asset_id = Uuid::new_v4();

        // Insert 3 frames of 256KB each = 768KB total (under limit)
        for i in 0..3 {
            cache.insert((asset_id, i), create_test_frame(256));
        }

        // All 3 should be cached
        assert!(cache.contains(&(asset_id, 0)));
        assert!(cache.contains(&(asset_id, 1)));
        assert!(cache.contains(&(asset_id, 2)));

        // Access frame 0 to make it "recently used"
        let _ = cache.get(&(asset_id, 0));

        // Now insert 2 more frames to exceed limit and trigger eviction
        for i in 3..5 {
            cache.insert((asset_id, i), create_test_frame(256));
        }

        // Frame 0 should still exist (was accessed recently)
        // Frame 1 should be evicted first (oldest access)
        assert!(cache.contains(&(asset_id, 0)));
        assert!(!cache.contains(&(asset_id, 1))); // Should be evicted
    }
}
