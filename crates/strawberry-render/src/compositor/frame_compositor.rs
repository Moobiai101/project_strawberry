//! # Frame Compositor
//!
//! ECS system that composites timeline frames for viewport display.
//!
//! ## Responsibilities
//!
//! - Build node tree from ECS timeline data
//! - Collect layers at current playhead time
//! - Request frames from VideoAssetManager
//! - Handle playback mode transitions
//! - Update viewport state with rendered output
//!
//! ## Performance Optimizations
//!
//! - **CompositorCache**: Skips redundant decode/upload when playhead is idle
//! - **Double-buffering**: Uses last texture while new frame uploads in background
//! - **Layer hash**: Detects timeline content changes efficiently
//!
//! ## Future (Phase 2B)
//!
//! Multi-layer alpha-over compositing for overlapping clips.

use bevy_ecs::prelude::*;
use bevy_hierarchy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use strawberry_core::components::timeline::ClipMetadata;
use strawberry_core::components::{Clip, StableId, Track};
use strawberry_core::node::timeline::clip::ClipAssetRef;
use strawberry_core::resources::{Playhead, ScrubState};
use strawberry_video::service::FrameStatus;

use crate::compositor_cache::{calculate_layer_hash, CompositorCache};
use crate::texture_manager::TextureManager;
use crate::traverser::{
    ActiveLayersResource, CompositorLayer, CompositorOutput, TraverserResource,
};

use super::VideoAssetManager;

// ============================================================================
// COMPOSITOR SYSTEM
// ============================================================================

/// System that composites timeline content using NodeTraverser.
///
/// This system:
/// 1. Builds the node tree from ECS timeline data
/// 2. Collects layers at current playhead time
/// 3. Requests frames for each layer
/// 4. Updates compositor output AND viewport state
///
/// For Phase 1, we only show the topmost layer (no blending).
///
/// ## Performance Optimization
///
/// Uses CompositorCache to skip redundant work when:
/// - Playhead hasn't moved (same frame)
/// - Timeline content hasn't changed (same layer hash)
pub fn compositor_system(
    playhead: Res<Playhead>,
    scrub_state: Res<ScrubState>,
    mut traverser: ResMut<TraverserResource>,
    mut active_layers_res: ResMut<ActiveLayersResource>,
    mut output: ResMut<CompositorOutput>,
    mut cache: ResMut<CompositorCache>,
    mut viewport_state: ResMut<strawberry_core::resources::ViewportState>,
    mut asset_manager: ResMut<VideoAssetManager>,
    texture_manager: Res<TextureManager>,
    tracks_query: Query<(Entity, &Track, &StableId)>,
    clips_query: Query<(
        Entity,
        &Clip,
        &StableId,
        Option<&ClipAssetRef>,
        Option<&ClipMetadata>,
        &Parent,
    )>,
) {
    // Poll for async results (only needed for random-access mode)
    asset_manager.poll_results();

    // ========================================================================
    // PLAYBACK MODE TRANSITIONS
    // ========================================================================
    // Transition between streaming (playback) and random-access (scrubbing) modes.
    // This is THE KEY optimization for real-time video playback.
    let is_playing: bool = playhead.is_playing;
    let is_in_playback_mode: bool = asset_manager.is_playback_mode();

    if is_playing && !is_in_playback_mode {
        // Transition TO playback mode: start PipeDecoder streaming
        if let Err(e) = asset_manager.start_playback(playhead.current_time) {
            tracing::error!("Failed to start playback mode: {}", e);
        }
    } else if !is_playing && is_in_playback_mode {
        // Transition FROM playback mode: stop streaming, return to random-access
        asset_manager.stop_playback();
    }

    // ========================================================================
    // SCRUB MODE PROPAGATION
    // ========================================================================
    // Propagate scrub state to VideoService for optimized decode path.
    // Scrub mode clears stale requests - only enable during ACTIVE scrubbing,
    // NOT during normal paused state, otherwise initial video load breaks.
    asset_manager.set_scrubbing(scrub_state.is_scrubbing);

    // Build node tree from ECS
    traverser.build_tree(&tracks_query, &clips_query);

    // Check if we have any content
    let clip_count: usize = traverser.clip_count();
    if clip_count == 0 {
        // Debug: log why we have no clips
        let total_clips: usize = clips_query.iter().count();
        let clips_with_asset_ref: usize = clips_query
            .iter()
            .filter(|(_, _, _, ar, _, _)| ar.is_some())
            .count();
        // Skip verbose logging - no clips
        let _ = (total_clips, clips_with_asset_ref); // suppress unused warnings
        output.clear();
        cache.clear();
        viewport_state.current_texture = None;
        viewport_state.clip_textures.clear();
        viewport_state.is_loading = false;

        // CRITICAL: Clear active layers so audio_mixer_sync_system
        // stops clips that are no longer on the timeline. Without this,
        // stale layers persist and audio keeps playing after undo.
        active_layers_res.layers.clear();

        return;
    }

    // Collect layers at current time
    let layers: Vec<CompositorLayer> = traverser.collect_layers(playhead.current_time);

    // Share active layers with downstream systems (audio mixer)
    // This eliminates the need for audio_mixer_sync_system to call
    // collect_layers() redundantly — saving a full node-tree traversal.
    active_layers_res.layers = layers.clone();

    if layers.is_empty() {
        // Playhead is in a gap between clips, or before/after all clips
        //
        // INDUSTRY STANDARD (DaVinci Resolve, Premiere, FCP):
        // Gaps render as BLACK frames. Empty space = black on export.
        //
        // IMPORTANT: Do NOT clear cache here - that breaks playback resumption
        // when playhead moves from gap into a clip. The cache is essential for
        // efficient frame delivery.
        if clip_count == 0 {
            // No clips at all - show empty state ("drop video here")
            output.clear();
            viewport_state.current_texture = None;
            viewport_state.is_loading = false;
            viewport_state.in_gap = false; // Not in gap, just empty timeline
            tracing::trace!(
                "Compositor: layers=0, clip_count=0, setting in_gap=false (empty timeline)"
            );
        } else {
            // There are clips, but playhead is in a gap - show BLACK frame
            // This matches DaVinci Resolve / Premiere behavior
            tracing::debug!(
                "Compositor: Gap at playhead {:?} - showing black frame",
                playhead.current_time
            );
            tracing::trace!(
                "Compositor: layers=0, clip_count={}, setting in_gap=true",
                clip_count
            );
            // Do NOT clear output/cache - just signal gap mode
            viewport_state.current_texture = None; // Black frame
            viewport_state.clip_textures.clear();
            viewport_state.is_loading = false;
            viewport_state.in_gap = true; // Signal to viewport: show black, not placeholder
        }
        return;
    }

    // We have layers (clips) - not in a gap anymore
    viewport_state.in_gap = false;
    // Propagate the active clip's speed to audio (use first/topmost clip)
    viewport_state.active_clip_speed = layers.first().map_or(1.0, |l| l.clip_speed);

    // ========================================================================
    // CACHE CHECK: Skip expensive work if playhead and layers unchanged
    // ========================================================================
    let layer_hash: u64 = calculate_layer_hash(&layers);

    if let Some(cached_texture) = cache.get_cached(playhead.current_frame, layer_hash) {
        // Cache hit! Reuse the cached texture without decoding or uploading.
        // This is the fast path when playhead is idle.
        viewport_state.current_texture = Some(cached_texture);
        viewport_state.is_loading = false;
        return;
    }

    // ========================================================================
    // CACHE MISS: Per-clip texture upload path
    // ========================================================================
    //
    // Production-grade per-clip GPU rendering:
    // 1. Decode each layer's frame independently
    // 2. Upload each to its own GPU texture via TextureManager
    // 3. Build clip_textures vec for the viewport renderer
    // 4. Viewport renders each clip as an independent quad with its own Transform2D
    //
    // This enables per-clip spatial transforms (position, scale, rotation)
    // which is how Canva, CapCut, and DaVinci Resolve render multi-layer timelines.

    use strawberry_core::resources::ClipTextureLayer;

    // HOLD-FRAME PATTERN (industry standard: DaVinci Resolve, Premiere, FCP)
    //
    // Snapshot the PREVIOUS frame's per-clip textures into a HashMap.
    // When a layer's decode returns Loading (async not finished yet),
    // we reuse the previous texture instead of showing black.
    // O(1) lookup per clip, O(active_layers) to build — scales to 1000+ clips.
    let previous_clip_textures: HashMap<uuid::Uuid, ClipTextureLayer> = viewport_state
        .clip_textures
        .iter()
        .map(|ct| (ct.clip_uuid, ct.clone()))
        .collect();

    let mut clip_texture_layers: Vec<ClipTextureLayer> = Vec::with_capacity(layers.len());
    let mut topmost_texture: Option<egui::TextureId> = None;
    let mut topmost_resolution: glam::UVec2 = glam::UVec2::new(1920, 1080);
    let mut any_loading: bool = false;

    for (layer_idx, layer) in layers.iter().enumerate() {
        // Request frame for each layer — respects time remapping (speed/reverse/loop).
        match asset_manager.request_frame_for_layer(
            layer.asset_uuid,
            layer.source_time,
            layer.needs_random_access,
        ) {
            FrameStatus::Ready(frame) => {
                tracing::trace!(
                    "COMPOSITOR: Layer {} READY at source_time {:?} ({}x{})",
                    layer_idx,
                    layer.source_time,
                    frame.width,
                    frame.height,
                );

                // TEXTURE KEY DESIGN: Always use STABLE keys per clip.
                //
                // WHY NOT per-time keys for scrubbing?
                // TextureManager has a queue_upload → process_uploads timing gap:
                //   1. queue_upload() adds frame to pending list
                //   2. get_texture() tries to find the slot
                //   3. process_uploads() actually uploads to GPU (runs AFTER schedule)
                // Per-time keys create a NEW key each seek, so get_texture()
                // always misses (slot doesn't exist yet). Stable keys reuse the
                // same slot — get_texture() finds the existing slot while
                // process_uploads() overwrites its pixel data in-place.
                //
                // This is how DaVinci Resolve and Premiere handle it:
                // one GPU texture per active layer, data overwritten each frame.
                let layer_key: strawberry_video::cache::FrameKey =
                    strawberry_video::cache::FrameKey::new(
                        PathBuf::from(format!("clip_{}", layer.clip_uuid)),
                        Duration::ZERO, // stable: same slot reused every frame
                        frame.width,
                        frame.height,
                    );

                // Queue for GPU upload. The key is stable, so TextureManager
                // will rewrite the existing slot's pixel data in-place instead
                // of allocating a new one.
                texture_manager.queue_upload(layer_key.clone(), (*frame).clone());

                // Look up THIS layer's texture by its unique stable key.
                //
                // Because we always use stable keys, the slot from the previous
                // frame's process_uploads() is still valid. get_texture() finds
                // it immediately, and the queued upload will overwrite the pixel
                // data when process_uploads() runs later this frame.
                let layer_texture_id: Option<egui::TextureId> =
                    texture_manager.get_texture(&layer_key);

                if let Some(texture_id) = layer_texture_id {
                    let resolution: glam::UVec2 = glam::UVec2::new(frame.width, frame.height);

                    clip_texture_layers.push(ClipTextureLayer {
                        texture_id,
                        clip_uuid: layer.clip_uuid,
                        source_resolution: resolution,
                        opacity: layer.opacity,
                    });

                    // Track topmost layer for backward compat
                    topmost_texture = Some(texture_id);
                    topmost_resolution = resolution;
                } else if let Some(prev_layer) = previous_clip_textures.get(&layer.clip_uuid) {
                    // Slot doesn't exist yet (first frame for this clip).
                    // Use the previous frame's texture as a bridge until
                    // process_uploads() creates the slot.
                    clip_texture_layers.push(prev_layer.clone());
                    topmost_texture = Some(prev_layer.texture_id);
                    topmost_resolution = prev_layer.source_resolution;
                } else {
                    any_loading = true;
                }
            }
            FrameStatus::Loading => {
                // CHANGE 1: Hold-frame retention.
                //
                // Instead of showing black while the async decoder works,
                // reuse the PREVIOUS frame's texture for this clip. The stale
                // frame is visible for at most 1-2 render ticks (~16-33ms),
                // which is imperceptible. As soon as the decode finishes,
                // the next frame's Ready branch replaces it.
                if let Some(prev_layer) = previous_clip_textures.get(&layer.clip_uuid) {
                    clip_texture_layers.push(prev_layer.clone());
                    topmost_texture = Some(prev_layer.texture_id);
                    topmost_resolution = prev_layer.source_resolution;
                    tracing::trace!(
                        "Compositor: Layer {} loading — reusing previous texture",
                        layer_idx,
                    );
                } else {
                    // No previous texture exists (first frame ever for this clip)
                    tracing::trace!(
                        "Compositor: Layer {} loading at source_time {:?} (no previous)",
                        layer_idx,
                        layer.source_time
                    );
                    any_loading = true;
                }
            }
            FrameStatus::Error(e) => {
                tracing::warn!(
                    "Compositor: Layer {} error at {:?}: {}",
                    layer_idx,
                    layer.source_time,
                    e
                );
            }
        }
    }

    // Update viewport state with per-clip texture data
    if !clip_texture_layers.is_empty() {
        viewport_state.clip_textures = clip_texture_layers;
        viewport_state.current_texture = topmost_texture;
        viewport_state.source_resolution = topmost_resolution;
        viewport_state.is_loading = false;
        viewport_state.error = None;

        // Update compositor output with topmost layer for backward compat
        if let Some(texture_id) = topmost_texture {
            output.set_frame(playhead.current_frame, texture_id, topmost_resolution);

            // Update cache with the topmost layer
            cache.update(
                playhead.current_frame,
                playhead.current_time,
                layer_hash,
                texture_id,
                topmost_resolution,
            );
        }
    } else if any_loading {
        // Some layers still loading — keep showing last good state
        if let Some(last_texture) = viewport_state.current_texture {
            output.set_frame(
                playhead.current_frame,
                last_texture,
                viewport_state.source_resolution,
            );
            viewport_state.is_loading = false;
        } else {
            // No texture at all yet — show loading spinner
            tracing::trace!("Compositor: No layers ready, showing loading spinner");
            output.set_loading();
            viewport_state.is_loading = true;
        }
        viewport_state.error = None;
    } else {
        // No ready frames and nothing loading — all errored
        if let Some(last_texture) = viewport_state.current_texture {
            output.set_frame(
                playhead.current_frame,
                last_texture,
                viewport_state.source_resolution,
            );
        }
        viewport_state.is_loading = false;
        viewport_state.error = None;
    }
}
