//! # Compositor Cache
//!
//! Tracks compositor state to avoid redundant renders.
//!
//! ## Purpose
//!
//! The compositor performs expensive work every frame:
//! 1. Decode video frame (CPU-intensive)
//! 2. Upload to GPU (memory bandwidth)
//! 3. Composite layers (GPU work)
//!
//! When the playhead is idle, we don't need to repeat this work.
//! This module provides dirty-tracking to skip redundant renders.
//!
//! ## Cache Invalidation
//!
//! The cache is invalidated when:
//! - Playhead moves to a different frame
//! - Timeline content changes (clip added/removed/moved)
//! - Resolution changes
//! - Manual invalidation (force re-render)

use crate::traverser::CompositorLayer;
use bevy_ecs::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Tracks compositor state to avoid redundant renders.
///
/// The compositor checks this before doing work:
/// - If playhead hasn't moved AND layers haven't changed → reuse cached result
/// - If dirty flag set → full re-render
#[derive(Resource, Default)]
pub struct CompositorCache {
    /// Last successfully rendered playhead frame number
    last_rendered_frame: Option<u64>,
    /// Last successfully rendered playhead time
    last_rendered_time: Option<Duration>,
    /// Hash of active layer configuration (asset UUIDs + track indices + source times)
    last_layer_hash: u64,
    /// The texture ID of the last composited frame
    cached_texture_id: Option<egui::TextureId>,
    /// Resolution of cached frame
    cached_resolution: glam::UVec2,
    /// Force re-render on next frame
    dirty: bool,
}

impl CompositorCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if cache is valid for current state
    ///
    /// Cache is valid only if:
    /// - Not marked dirty
    /// - Same frame number as last render
    /// - Same layer configuration hash
    pub fn is_valid(&self, frame: u64, layer_hash: u64) -> bool {
        !self.dirty && self.last_rendered_frame == Some(frame) && self.last_layer_hash == layer_hash
    }

    /// Get cached texture if valid for current state
    ///
    /// Returns None if cache is invalid or no texture was cached.
    pub fn get_cached(&self, frame: u64, layer_hash: u64) -> Option<egui::TextureId> {
        if self.is_valid(frame, layer_hash) {
            self.cached_texture_id
        } else {
            None
        }
    }

    /// Update cache after successful render
    ///
    /// Call this after completing a full render cycle to cache the result.
    pub fn update(
        &mut self,
        frame: u64,
        time: Duration,
        layer_hash: u64,
        texture_id: egui::TextureId,
        resolution: glam::UVec2,
    ) {
        self.last_rendered_frame = Some(frame);
        self.last_rendered_time = Some(time);
        self.last_layer_hash = layer_hash;
        self.cached_texture_id = Some(texture_id);
        self.cached_resolution = resolution;
        self.dirty = false;
    }

    /// Mark cache as dirty (force re-render on next frame)
    ///
    /// Call this when you know the timeline content has changed but
    /// the layer hash might not reflect it yet.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Clear cache entirely
    ///
    /// Resets all cached state to default values.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Get the last rendered frame number (for debugging)
    pub fn last_frame(&self) -> Option<u64> {
        self.last_rendered_frame
    }

    /// Get the cached resolution (for debugging)
    pub fn cached_resolution(&self) -> glam::UVec2 {
        self.cached_resolution
    }

    /// Check if cache is currently dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// Calculate a hash of the layer configuration for dirty-tracking.
///
/// The hash includes:
/// - Asset UUIDs (which video)
/// - Track indices (z-order)
/// - Source times (which frame within the video)
///
/// If any of these change, the hash will be different and trigger a re-render.
pub fn calculate_layer_hash(layers: &[CompositorLayer]) -> u64 {
    let mut hasher: DefaultHasher = DefaultHasher::new();

    for layer in layers {
        layer.asset_uuid.hash(&mut hasher);
        layer.track_index.hash(&mut hasher);
        // Hash source time as nanoseconds for precision
        layer.source_time.as_nanos().hash(&mut hasher);
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_cache_default_is_invalid() {
        let cache: CompositorCache = CompositorCache::default();
        assert!(cache.get_cached(0, 0).is_none());
        assert!(!cache.is_valid(0, 0));
    }

    #[test]
    fn test_cache_update_makes_valid() {
        let mut cache: CompositorCache = CompositorCache::new();
        let texture_id: egui::TextureId = egui::TextureId::Managed(42);
        let resolution: glam::UVec2 = glam::UVec2::new(1920, 1080);

        cache.update(100, Duration::from_secs(5), 12345, texture_id, resolution);

        assert!(cache.is_valid(100, 12345));
        assert_eq!(cache.get_cached(100, 12345), Some(texture_id));
    }

    #[test]
    fn test_cache_different_frame_invalid() {
        let mut cache: CompositorCache = CompositorCache::new();
        let texture_id: egui::TextureId = egui::TextureId::Managed(42);

        cache.update(
            100,
            Duration::from_secs(5),
            12345,
            texture_id,
            glam::UVec2::new(1920, 1080),
        );

        // Different frame should be invalid
        assert!(!cache.is_valid(101, 12345));
        assert!(cache.get_cached(101, 12345).is_none());
    }

    #[test]
    fn test_cache_different_hash_invalid() {
        let mut cache: CompositorCache = CompositorCache::new();
        let texture_id: egui::TextureId = egui::TextureId::Managed(42);

        cache.update(
            100,
            Duration::from_secs(5),
            12345,
            texture_id,
            glam::UVec2::new(1920, 1080),
        );

        // Different layer hash should be invalid
        assert!(!cache.is_valid(100, 99999));
        assert!(cache.get_cached(100, 99999).is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let mut cache: CompositorCache = CompositorCache::new();
        let texture_id: egui::TextureId = egui::TextureId::Managed(42);

        cache.update(
            100,
            Duration::from_secs(5),
            12345,
            texture_id,
            glam::UVec2::new(1920, 1080),
        );

        cache.invalidate();

        // Should be invalid after invalidation
        assert!(!cache.is_valid(100, 12345));
        assert!(cache.is_dirty());
    }

    #[test]
    fn test_cache_clear() {
        let mut cache: CompositorCache = CompositorCache::new();
        let texture_id: egui::TextureId = egui::TextureId::Managed(42);

        cache.update(
            100,
            Duration::from_secs(5),
            12345,
            texture_id,
            glam::UVec2::new(1920, 1080),
        );

        cache.clear();

        assert!(cache.last_frame().is_none());
        assert!(!cache.is_dirty());
    }

    #[test]
    fn test_layer_hash_empty() {
        let hash: u64 = calculate_layer_hash(&[]);
        // Empty layers should produce consistent hash
        assert_eq!(hash, calculate_layer_hash(&[]));
    }

    #[test]
    fn test_layer_hash_changes_with_content() {
        let layer1: CompositorLayer = CompositorLayer {
            clip_uuid: Uuid::new_v4(),
            asset_uuid: Uuid::new_v4(),
            source_time: Duration::from_secs(1),
            track_index: 0,
            track_entity_bits: 0,
            opacity: 1.0,
            texture_id: None,
            needs_random_access: false,
            clip_speed: 1.0,
        };

        let layer2: CompositorLayer = CompositorLayer {
            clip_uuid: Uuid::new_v4(),
            asset_uuid: Uuid::new_v4(),
            source_time: Duration::from_secs(2),
            track_index: 1,
            track_entity_bits: 0,
            opacity: 1.0,
            texture_id: None,
            needs_random_access: false,
            clip_speed: 1.0,
        };

        let hash_single: u64 = calculate_layer_hash(&[layer1.clone()]);
        let hash_double: u64 = calculate_layer_hash(&[layer1.clone(), layer2]);

        assert_ne!(hash_single, hash_double);
    }
}
