//! # Thumbnail Texture Cache
//!
//! ECS Resource for pre-loaded thumbnail GPU textures.
//! Enables instant zoom level switching in the timeline.
//!
//! ## Why Pre-load All LODs?
//!
//! When user zooms in/out, we need to switch between LOD levels.
//! If we load textures on-demand, there's a visible loading delay.
//! By pre-loading all LOD levels when a clip is added to the timeline,
//! zoom switching is instant (~0ms).
//!
//! ## Memory Usage
//!
//! LOD0 + LOD1 together use roughly:
//! - LOD0 (40px, every 10s): ~2KB per thumbnail
//! - LOD1 (80px, every 2s): ~8KB per thumbnail
//!
//! For a 1-minute video:
//! - LOD0: 6 thumbnails × 2KB = 12KB
//! - LOD1: 30 thumbnails × 8KB = 240KB
//! - Total: ~250KB per minute of video
//!
//! A 10-minute video uses ~2.5MB of GPU memory for all thumbnails.

use bevy_ecs::system::Resource;
use std::collections::HashMap;
use uuid::Uuid;

/// Pre-loaded thumbnails for a single asset
///
/// Both LOD levels are always loaded together to enable
/// instant zoom level switching without loading delay.
#[derive(Debug, Clone)]
pub struct LoadedThumbnails {
    /// LOD0 textures (40px, every 10s)
    /// Vec of (timestamp_seconds, texture_handle)
    pub lod0: Vec<(f64, ThumbnailTexture)>,
    /// LOD1 textures (80px, every 2s)
    /// Vec of (timestamp_seconds, texture_handle)
    pub lod1: Vec<(f64, ThumbnailTexture)>,
    /// Whether LOD2 should fall back to proxy frames
    pub use_proxy_for_lod2: bool,
}

impl LoadedThumbnails {
    /// Create empty loaded thumbnails
    pub fn new() -> Self {
        Self {
            lod0: Vec::new(),
            lod1: Vec::new(),
            use_proxy_for_lod2: true,
        }
    }

    /// Check if any thumbnails are loaded
    pub fn is_empty(&self) -> bool {
        self.lod0.is_empty() && self.lod1.is_empty()
    }
}

impl Default for LoadedThumbnails {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to a loaded texture
///
/// This is a lightweight wrapper that can be used to reference
/// textures in the GPU. The actual texture data is managed by egui.
#[derive(Debug, Clone)]
pub struct ThumbnailTexture {
    /// Texture ID from egui (when using egui backend)
    /// This is stored as Option because texture may not be loaded yet
    pub texture_id: Option<u64>,
    /// Raw RGBA pixel data (kept for CPU-side operations)
    pub pixels: Option<Vec<u8>>,
    /// Width of the texture
    pub width: u32,
    /// Height of the texture
    pub height: u32,
}

impl ThumbnailTexture {
    /// Create a new thumbnail texture from raw pixels
    pub fn from_pixels(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            texture_id: None,
            pixels: Some(pixels),
            width,
            height,
        }
    }

    /// Create an empty/placeholder texture
    pub fn placeholder() -> Self {
        Self {
            texture_id: None,
            pixels: None,
            width: 0,
            height: 0,
        }
    }

    /// Check if texture has pixel data loaded
    pub fn has_pixels(&self) -> bool {
        self.pixels.is_some()
    }
}

/// ECS Resource for pre-loaded thumbnail GPU textures
///
/// Stores all loaded thumbnails for clips currently visible in the timeline.
/// Enables instant switching between zoom levels without loading delay.
///
/// ## Usage
///
/// ```ignore
/// // When clip is added to timeline:
/// thumbnail_cache.preload_asset(&asset_id, &thumbnail_manager, proxy_dir);
///
/// // During timeline render:
/// let lod = get_lod_for_zoom(pixels_per_second);
/// if let Some(thumbnails) = thumbnail_cache.get_textures(&asset_id, lod) {
///     // Render thumbnails (instant!)
/// }
///
/// // When clip is removed from timeline:
/// thumbnail_cache.unload_asset(&asset_id);
/// ```
#[derive(Resource, Default)]
pub struct ThumbnailTextureCache {
    /// All LOD levels pre-loaded per asset
    loaded: HashMap<Uuid, LoadedThumbnails>,
    /// Assets that are currently loading
    loading: HashMap<Uuid, bool>,
}

impl ThumbnailTextureCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            loaded: HashMap::new(),
            loading: HashMap::new(),
        }
    }

    /// Check if an asset's thumbnails are loaded
    pub fn is_loaded(&self, asset_id: &Uuid) -> bool {
        self.loaded.contains_key(asset_id)
    }

    /// Check if an asset is currently loading
    pub fn is_loading(&self, asset_id: &Uuid) -> bool {
        self.loading.get(asset_id).copied().unwrap_or(false)
    }

    /// Mark an asset as loading
    pub fn mark_loading(&mut self, asset_id: Uuid) {
        self.loading.insert(asset_id, true);
    }

    /// Store loaded thumbnails for an asset
    ///
    /// Call this after loading thumbnails from disk.
    pub fn store_thumbnails(&mut self, asset_id: Uuid, thumbnails: LoadedThumbnails) {
        self.loaded.insert(asset_id, thumbnails);
        self.loading.remove(&asset_id);
        tracing::debug!(
            "Stored thumbnails for asset {}: LOD0={} LOD1={}",
            &asset_id.to_string()[..8],
            self.loaded
                .get(&asset_id)
                .map(|t| t.lod0.len())
                .unwrap_or(0),
            self.loaded
                .get(&asset_id)
                .map(|t| t.lod1.len())
                .unwrap_or(0),
        );
    }

    /// Get all loaded thumbnails for an asset
    pub fn get(&self, asset_id: &Uuid) -> Option<&LoadedThumbnails> {
        self.loaded.get(asset_id)
    }

    /// Get thumbnails for a specific LOD level
    ///
    /// Returns slice of (timestamp_seconds, texture) tuples.
    /// Returns None if asset not loaded or LOD2 requested (use proxy frames).
    ///
    /// LOD levels:
    /// - 0: 40px thumbnails, every 10s (zoomed out)
    /// - 1: 80px thumbnails, every 2s (default)
    /// - 2: Use proxy frames directly (zoomed in)
    pub fn get_lod_textures(
        &self,
        asset_id: &Uuid,
        lod_level: u8,
    ) -> Option<&[(f64, ThumbnailTexture)]> {
        let loaded = self.loaded.get(asset_id)?;
        match lod_level {
            0 => Some(&loaded.lod0),
            1 => Some(&loaded.lod1),
            _ => None, // LOD2 uses proxy frames directly
        }
    }

    /// Unload thumbnails for an asset
    ///
    /// Call when clip is removed from timeline to free GPU memory.
    pub fn unload_asset(&mut self, asset_id: &Uuid) {
        self.loaded.remove(asset_id);
        self.loading.remove(asset_id);
        tracing::debug!(
            "Unloaded thumbnails for asset {}",
            &asset_id.to_string()[..8]
        );
    }

    /// Clear all loaded thumbnails
    ///
    /// Call when switching projects.
    pub fn clear(&mut self) {
        self.loaded.clear();
        self.loading.clear();
    }

    /// Get number of loaded assets
    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// Get list of all loaded asset IDs
    pub fn loaded_assets(&self) -> Vec<Uuid> {
        self.loaded.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumbnail_cache_lifecycle() {
        let mut cache = ThumbnailTextureCache::new();
        let asset_id = Uuid::new_v4();

        // Initially not loaded
        assert!(!cache.is_loaded(&asset_id));

        // Mark as loading
        cache.mark_loading(asset_id);
        assert!(cache.is_loading(&asset_id));

        // Store thumbnails
        let thumbnails = LoadedThumbnails::new();
        cache.store_thumbnails(asset_id, thumbnails);

        // Now loaded, not loading
        assert!(cache.is_loaded(&asset_id));
        assert!(!cache.is_loading(&asset_id));

        // Unload
        cache.unload_asset(&asset_id);
        assert!(!cache.is_loaded(&asset_id));
    }

    #[test]
    fn test_loaded_thumbnails() {
        let mut thumbnails = LoadedThumbnails::new();
        assert!(thumbnails.is_empty());

        thumbnails.lod0.push((0.0, ThumbnailTexture::placeholder()));
        assert!(!thumbnails.is_empty());
    }
}
