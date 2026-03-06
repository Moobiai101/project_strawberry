//! # Texture Manager (Production-Grade)
//!
//! Manages GPU textures for video frames using double-buffering.
//!
//! ## Industry Approach (Adobe/CapCut)
//!
//! Instead of creating a new texture for every frame (memory explosion!),
//! professional NLEs use a small fixed pool of textures:
//!
//! 1. **Double/Triple Buffering**: Only 2-3 textures needed for smooth playback
//! 2. **Write to existing texture**: Use `queue.write_texture()` instead of create
//! 3. **Slot rotation**: Alternate between texture slots

use bevy_ecs::system::Resource;
use egui_wgpu::wgpu;
use parking_lot::RwLock;
use std::collections::HashMap;
use strawberry_video::cache::FrameKey;
use strawberry_video::decode::VideoFrame;

/// Maximum number of texture slots.
///
/// Must accommodate: (active_clips × 2 for double-buffering) + scrubbing headroom.
/// 16 slots = 8 clips × 2 buffers each. At 720p JPEG proxy, each slot ≈ 3.7MB — total ≈ 60MB.
const MAX_TEXTURE_SLOTS: usize = 16;

/// A texture slot that can be reused
struct TextureSlot {
    /// The egui texture ID
    texture_id: egui::TextureId,
    /// The underlying wgpu texture (for write_texture)
    wgpu_texture: wgpu::Texture,
    /// Current dimensions
    width: u32,
    height: u32,
    /// Last used frame (for LRU eviction)
    last_used_frame: u64,
}

/// Production-grade texture manager with bounded memory usage
#[derive(Resource)]
pub struct TextureManager {
    /// Pre-allocated texture slots
    slots: RwLock<Vec<TextureSlot>>,
    /// Map from frame key to slot index
    key_to_slot: RwLock<HashMap<FrameKey, usize>>,
    /// Current frame number (for LRU tracking)
    current_frame: RwLock<u64>,
    /// Whether slots have been initialized (reserved for future use)
    #[allow(dead_code)]
    initialized: RwLock<bool>,
    /// Pending frames to upload — accumulated per-frame, cleared after upload.
    ///
    /// Multi-clip rendering queues one frame per active layer.
    /// All are uploaded in `process_uploads()` before the next paint.
    pending_frames: RwLock<Vec<(FrameKey, VideoFrame)>>,
    /// The most recently uploaded texture ID (backward compat for single-clip path)
    last_uploaded_texture: RwLock<Option<egui::TextureId>>,
}

impl TextureManager {
    pub fn new() -> Self {
        Self {
            slots: RwLock::new(Vec::with_capacity(MAX_TEXTURE_SLOTS)),
            key_to_slot: RwLock::new(HashMap::new()),
            current_frame: RwLock::new(0),
            initialized: RwLock::new(false),
            pending_frames: RwLock::new(Vec::with_capacity(8)),
            last_uploaded_texture: RwLock::new(None),
        }
    }

    /// Queue a frame for upload.
    ///
    /// Multi-clip: each active layer queues its own frame per render tick.
    /// All pending frames are uploaded in `process_uploads()` before paint.
    pub fn queue_upload(&self, key: FrameKey, frame: VideoFrame) {
        self.pending_frames.write().push((key, frame));
    }

    /// Process ALL pending uploads — called once per frame before paint.
    ///
    /// Uploads every queued frame to its own texture slot. With multi-clip,
    /// this uploads N frames (one per active layer) in a single pass.
    pub fn process_uploads(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
    ) {
        let pending: Vec<(FrameKey, VideoFrame)> = {
            let mut pending_guard = self.pending_frames.write();
            std::mem::take(&mut *pending_guard)
        };

        for (key, frame) in pending {
            self.upload_frame(device, queue, renderer, key, frame);
        }
    }

    /// Upload a single frame to a texture slot
    fn upload_frame(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
        key: FrameKey,
        frame: VideoFrame,
    ) {
        let mut slots = self.slots.write();
        let mut key_map = self.key_to_slot.write();
        let current_frame = *self.current_frame.read();

        // Check if we already have this key
        if let Some(&slot_idx) = key_map.get(&key) {
            if slot_idx < slots.len() {
                // Check if dimensions match BEFORE mutably borrowing the slot
                let size_matches: bool =
                    slots[slot_idx].width == frame.width && slots[slot_idx].height == frame.height;

                if size_matches {
                    // CRITICAL: Rewrite the pixel data in-place.
                    //
                    // During playback, the compositor uses a STABLE key per clip
                    // (same key every frame). The slot already exists, but its
                    // pixel data is from the PREVIOUS frame. We must overwrite
                    // it with the new frame's data for the viewport to update.
                    self.write_texture_data(queue, &slots[slot_idx].wgpu_texture, &frame);
                    slots[slot_idx].last_used_frame = current_frame;
                    *self.last_uploaded_texture.write() = Some(slots[slot_idx].texture_id);
                    return;
                }

                // Size changed (rare) — recreate the slot in-place
                renderer.free_texture(&slots[slot_idx].texture_id);
                slots[slot_idx] = self.create_slot(device, queue, renderer, &frame, current_frame);
                key_map.insert(key, slot_idx);
                *self.last_uploaded_texture.write() = Some(slots[slot_idx].texture_id);
                return;
            }
        }

        // Find or create a slot
        let slot_idx = if slots.len() < MAX_TEXTURE_SLOTS {
            // Create new slot
            let slot = self.create_slot(device, queue, renderer, &frame, current_frame);
            let idx = slots.len();
            slots.push(slot);
            idx
        } else {
            // Find LRU slot to reuse
            let lru_idx = slots
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.last_used_frame)
                .map(|(i, _)| i)
                .unwrap_or(0);

            // Remove old key mapping
            let old_key: Option<FrameKey> = key_map
                .iter()
                .find(|(_, &idx)| idx == lru_idx)
                .map(|(k, _)| k.clone());

            if let Some(old_key) = old_key {
                key_map.remove(&old_key);
            }

            // Check if we need to resize the texture
            let slot = &mut slots[lru_idx];
            if slot.width != frame.width || slot.height != frame.height {
                // Need to recreate texture with new size
                renderer.free_texture(&slot.texture_id);
                *slot = self.create_slot(device, queue, renderer, &frame, current_frame);
            } else {
                // Reuse existing texture - just update the data
                self.write_texture_data(queue, &slot.wgpu_texture, &frame);
                slot.last_used_frame = current_frame;
            }

            lru_idx
        };

        // Update key mapping
        key_map.insert(key, slot_idx);

        // Save the texture ID for playback access
        *self.last_uploaded_texture.write() = Some(slots[slot_idx].texture_id);

        // DEBUG: Log successful upload
        tracing::trace!("Uploaded frame to slot {}", slot_idx);
    }

    /// Create a new texture slot
    fn create_slot(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut egui_wgpu::Renderer,
        frame: &VideoFrame,
        current_frame: u64,
    ) -> TextureSlot {
        let size = wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Video Frame Slot"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Write initial frame data
        self.write_texture_data(queue, &texture, frame);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let texture_id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);

        TextureSlot {
            texture_id,
            wgpu_texture: texture,
            width: frame.width,
            height: frame.height,
            last_used_frame: current_frame,
        }
    }

    /// Write frame data to an existing texture
    fn write_texture_data(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, frame: &VideoFrame) {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4), // RGBA = 4 bytes
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Get texture ID for a key
    pub fn get_texture(&self, key: &FrameKey) -> Option<egui::TextureId> {
        let key_map = self.key_to_slot.read();
        let slots = self.slots.read();

        key_map
            .get(key)
            .and_then(|&idx| slots.get(idx).map(|s| s.texture_id))
    }

    /// Advance frame counter (call each frame)
    pub fn advance_frame(&self) {
        *self.current_frame.write() += 1;
    }

    /// GC is now a no-op since we use fixed slots
    pub fn gc(&self, _renderer: &mut egui_wgpu::Renderer, _valid_keys: &[FrameKey]) {
        // No-op - we use LRU eviction instead
    }

    /// Get the latest/current texture (for playback - just get most recent)
    pub fn get_current_texture(&self) -> Option<egui::TextureId> {
        // Simply return the last uploaded texture - this is what we need for playback
        // The actual content was updated in the previous frame's process_uploads
        *self.last_uploaded_texture.read()
    }
}

impl Default for TextureManager {
    fn default() -> Self {
        Self::new()
    }
}
