//! # Thumbnail Texture Manager (Production-Grade)
//!
//! Uploads thumbnail pixel data to GPU for filmstrip rendering.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────┐     ┌────────────────────────────┐
//! │ ThumbnailTextureCache │────▶│ ThumbnailTextureManager    │
//! │ (strawberry-core)   │     │ (strawberry-render)        │
//! │ - Stores pixel data │     │ - Uploads to GPU           │
//! │ - LOD0/LOD1 layers  │     │ - Returns egui::TextureId  │
//! └────────────────────┘     └────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! // In the render loop:
//! thumbnail_texture_manager.upload_thumbnails(
//!     &mut thumbnail_cache,
//!     device, queue, renderer,
//! );
//!
//! // TextureIds are now set in ThumbnailTexture.texture_id
//! ```

use bevy_ecs::system::Resource;
use egui_wgpu::wgpu;
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

/// A cached thumbnail texture on GPU
struct ThumbnailSlot {
    /// The egui texture ID
    texture_id: egui::TextureId,
    /// The underlying wgpu texture (for write_texture)
    _wgpu_texture: wgpu::Texture,
    /// Dimensions
    _width: u32,
    _height: u32,
}

/// Key for identifying thumbnail textures
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ThumbnailKey {
    /// Asset UUID
    pub asset_id: Uuid,
    /// LOD level (0 or 1)
    pub lod: u8,
    /// Thumbnail index within the LOD
    pub index: usize,
}

/// Production-grade thumbnail texture uploader
///
/// Manages GPU textures for timeline filmstrip thumbnails.
/// Thumbnails are uploaded once and cached until the asset is removed.
#[derive(Resource)]
pub struct ThumbnailTextureManager {
    /// Uploaded textures by key
    slots: RwLock<HashMap<ThumbnailKey, ThumbnailSlot>>,
    /// Total uploaded count (for monitoring)
    uploaded_count: RwLock<usize>,
}

impl Default for ThumbnailTextureManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThumbnailTextureManager {
    /// Create a new thumbnail texture manager
    pub fn new() -> Self {
        Self {
            slots: RwLock::new(HashMap::new()),
            uploaded_count: RwLock::new(0),
        }
    }

    /// Upload a single thumbnail to GPU
    ///
    /// Returns the egui texture ID for rendering.
    pub fn upload_thumbnail(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
        key: ThumbnailKey,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> egui::TextureId {
        // Check if already uploaded
        if let Some(slot) = self.slots.read().get(&key) {
            return slot.texture_id;
        }

        // Create GPU texture
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Thumbnail Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Write pixel data
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4), // RGBA = 4 bytes
                rows_per_image: Some(height),
            },
            size,
        );

        // Register with egui
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let texture_id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);

        // Store in cache
        let slot = ThumbnailSlot {
            texture_id,
            _wgpu_texture: texture,
            _width: width,
            _height: height,
        };

        self.slots.write().insert(key, slot);
        *self.uploaded_count.write() += 1;

        texture_id
    }

    /// Get texture ID if already uploaded
    pub fn get_texture(&self, key: &ThumbnailKey) -> Option<egui::TextureId> {
        self.slots.read().get(key).map(|s| s.texture_id)
    }

    /// Check if a thumbnail is uploaded
    pub fn is_uploaded(&self, key: &ThumbnailKey) -> bool {
        self.slots.read().contains_key(key)
    }

    /// Unload all thumbnails for an asset
    pub fn unload_asset(&self, asset_id: &Uuid, renderer: &mut egui_wgpu::Renderer) {
        let mut slots = self.slots.write();
        let keys_to_remove: Vec<ThumbnailKey> = slots
            .keys()
            .filter(|k| k.asset_id == *asset_id)
            .cloned()
            .collect();

        let count = keys_to_remove.len();
        for key in keys_to_remove {
            if let Some(slot) = slots.remove(&key) {
                renderer.free_texture(&slot.texture_id);
            }
        }

        if count > 0 {
            *self.uploaded_count.write() -= count;
            tracing::debug!(
                "Unloaded {} thumbnails for asset {}",
                count,
                &asset_id.to_string()[..8]
            );
        }
    }

    /// Get count of uploaded thumbnails (for monitoring)
    pub fn uploaded_count(&self) -> usize {
        *self.uploaded_count.read()
    }

    /// Get all texture IDs for an asset at a given LOD level
    ///
    /// Returns vec of (timestamp_seconds, texture_id) pairs.
    /// Timestamps must be provided separately (from ThumbnailTextureCache).
    pub fn get_asset_textures(&self, asset_id: &Uuid, lod: u8) -> Vec<(usize, egui::TextureId)> {
        let slots = self.slots.read();
        let mut result: Vec<(usize, egui::TextureId)> = slots
            .iter()
            .filter(|(k, _)| k.asset_id == *asset_id && k.lod == lod)
            .map(|(k, s)| (k.index, s.texture_id))
            .collect();

        // Sort by index
        result.sort_by_key(|(idx, _)| *idx);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumbnail_key_equality() {
        let key1 = ThumbnailKey {
            asset_id: Uuid::nil(),
            lod: 0,
            index: 5,
        };
        let key2 = ThumbnailKey {
            asset_id: Uuid::nil(),
            lod: 0,
            index: 5,
        };
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_manager_creation() {
        let manager = ThumbnailTextureManager::new();
        assert_eq!(manager.uploaded_count(), 0);
    }
}
