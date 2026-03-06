//! # Node Traverser
//!
//! Walks the timeline node tree and produces composited frames.
//!
//! ## Purpose
//!
//! The `NodeTraverser` is the bridge between the ECS timeline (Tracks, Clips)
//! and the rendering pipeline. It:
//!
//! 1. Queries all tracks from the ECS world
//! 2. For each track, queries its child clips
//! 3. Calculates source time using `ClipNode.timeline_to_source()`
//! 4. Requests frames from `VideoAssetManager`
//! 5. Produces a composited output for the viewport
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    NodeTraverser                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Input: Playhead.current_time                               │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  1. Query all Track entities (sorted by index)              │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  2. For each Track, wrap in TrackNode                       │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  3. Query child Clips, wrap in ClipNode                     │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  4. ClipNode.timeline_to_source() → source_time             │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  5. VideoAssetManager.request_frame(asset_uuid, source_time)│
//! │         │                                                   │
//! │         ▼                                                   │
//! │  Output: NodeValueTable with composited texture             │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use bevy_ecs::prelude::*;
use bevy_hierarchy::prelude::*;
use std::time::Duration;
use uuid::Uuid;

use strawberry_core::components::timeline::ClipMetadata;
use strawberry_core::components::{Clip, StableId, Track};
use strawberry_core::node::timeline::clip::ClipAssetRef;
use strawberry_core::node::timeline::{ClipNode, TrackNode};
use strawberry_core::node::{Node, NodeGlobals, NodeValueRow, TimeRange};

// ============================================================================
// COMPOSITOR OUTPUT
// ============================================================================

/// Resource holding the composited frame for the current playhead position.
///
/// This is the output of the NodeTraverser and input to the viewport system.
#[derive(Resource, Default)]
pub struct CompositorOutput {
    /// The final composited texture ID (for egui rendering)
    pub texture_id: Option<egui::TextureId>,
    /// Resolution of the output
    pub resolution: glam::UVec2,
    /// Frame number that was composited
    pub frame: u64,
    /// Whether the frame is still loading
    pub loading: bool,
    /// Error message if composition failed
    pub error: Option<String>,
}

impl CompositorOutput {
    /// Create a new empty output
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the composited frame
    pub fn set_frame(&mut self, frame: u64, texture_id: egui::TextureId, resolution: glam::UVec2) {
        self.frame = frame;
        self.texture_id = Some(texture_id);
        self.resolution = resolution;
        self.loading = false;
        self.error = None;
    }

    /// Set loading state
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.error = None;
    }

    /// Set error state
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.loading = false;
    }

    /// Clear the output
    pub fn clear(&mut self) {
        self.texture_id = None;
        self.frame = 0;
        self.loading = false;
        self.error = None;
    }
}

// ============================================================================
// COMPOSITOR LAYER
// ============================================================================

/// A single layer to be composited.
///
/// Collected from ClipNodes, then composited bottom-to-top.
#[derive(Debug, Clone)]
pub struct CompositorLayer {
    /// UUID of the source clip
    pub clip_uuid: Uuid,
    /// UUID of the source asset
    pub asset_uuid: Uuid,
    /// Source time in the media file
    pub source_time: Duration,
    /// Track index (z-order, 0 = bottom)
    pub track_index: u32,
    /// Opacity (0.0 - 1.0)
    pub opacity: f32,
    /// Whether the layer has a texture ready
    pub texture_id: Option<egui::TextureId>,
    /// True when time remapping (speed≠1, reverse, loop) requires random-access
    /// decode instead of the sequential pipe decoder.
    pub needs_random_access: bool,
    /// The clip's speed multiplier (1.0 = normal)
    /// Propagated to audio for pitch-shift sync
    pub clip_speed: f32,
}

// ============================================================================
// NODE TRAVERSER
// ============================================================================

/// Traverses the timeline node tree and produces composited frames.
///
/// This is the main compositor system that:
/// 1. Reads the current timeline state from ECS
/// 2. Builds a tree of TrackNode/ClipNode
/// 3. Processes nodes to collect layers
/// 4. Requests frames from VideoAssetManager
/// 5. Produces a composited output
pub struct NodeTraverser {
    /// Cached track nodes (rebuilt each frame for now, cached in Phase 2)
    track_cache: Vec<TrackNode>,
    /// Output video resolution
    output_resolution: glam::UVec2,
    /// Output frame rate
    output_frame_rate: f64,
}

impl NodeTraverser {
    /// Create a new traverser with default settings
    pub fn new() -> Self {
        Self {
            track_cache: Vec::new(),
            output_resolution: glam::UVec2::new(1920, 1080),
            output_frame_rate: 30.0,
        }
    }

    /// Create with specific output settings
    pub fn with_output(resolution: glam::UVec2, frame_rate: f64) -> Self {
        Self {
            track_cache: Vec::new(),
            output_resolution: resolution,
            output_frame_rate: frame_rate,
        }
    }

    /// Set output resolution
    pub fn set_resolution(&mut self, resolution: glam::UVec2) {
        self.output_resolution = resolution;
    }

    /// Set output frame rate
    pub fn set_frame_rate(&mut self, frame_rate: f64) {
        self.output_frame_rate = frame_rate;
    }

    /// Build node tree from ECS world.
    ///
    /// Queries tracks and clips, wrapping them in node wrappers.
    /// ClipMetadata is optional — clips without metadata use default speed/reverse/loop.
    pub fn build_tree(
        &mut self,
        tracks_query: &Query<(Entity, &Track, &StableId)>,
        clips_query: &Query<(
            Entity,
            &Clip,
            &StableId,
            Option<&ClipAssetRef>,
            Option<&ClipMetadata>,
            &Parent,
        )>,
    ) {
        self.track_cache.clear();

        // Collect and sort tracks by index (bottom to top)
        let mut sorted_tracks: Vec<_> = tracks_query.iter().collect();
        sorted_tracks.sort_by_key(|(_, track, _)| track.index);

        for (track_entity, track, track_stable_id) in sorted_tracks {
            let mut track_node: TrackNode = TrackNode::new(*track_stable_id, track.clone());

            // Find clips that belong to this track
            for (_clip_entity, clip, clip_stable_id, asset_ref, metadata, parent) in
                clips_query.iter()
            {
                if parent.get() != track_entity {
                    continue;
                }

                // Wire ClipMetadata values into ClipNode.
                // If no ClipMetadata component is attached, use safe defaults.
                let clip_speed: f64 = metadata.map(|m| m.speed as f64).unwrap_or(1.0);
                let clip_reverse: bool = metadata.map(|m| m.reverse).unwrap_or(false);
                let clip_loop_mode = metadata.map(|m| m.loop_mode).unwrap_or_default();

                let clip_node: ClipNode =
                    ClipNode::new(*clip_stable_id, clip.clone(), asset_ref.cloned())
                        .with_speed(clip_speed)
                        .with_reverse(clip_reverse)
                        .with_loop_mode(clip_loop_mode);

                track_node.add_clip(clip_node);
            }

            self.track_cache.push(track_node);
        }
    }

    /// Collect layers at a given timeline time.
    ///
    /// Returns layers from bottom to top (ready for compositing).
    pub fn collect_layers(&self, timeline_time: Duration) -> Vec<CompositorLayer> {
        let mut layers: Vec<CompositorLayer> = Vec::new();

        let _globals: NodeGlobals = NodeGlobals::new(
            TimeRange::at(
                timeline_time,
                Duration::from_secs_f64(1.0 / self.output_frame_rate),
            ),
            self.output_resolution,
            self.output_frame_rate,
        );

        let _inputs: NodeValueRow = NodeValueRow::new();

        for track_node in &self.track_cache {
            // Skip muted/invisible tracks
            if track_node.is_muted() || !track_node.is_visible() {
                continue;
            }

            // Get clips at this time
            for clip_node in track_node.clips_at_time(timeline_time) {
                // Get asset UUID and source time
                if let Some(asset_uuid) = clip_node.asset_uuid() {
                    if let Some(source_time) = clip_node.timeline_to_source(
                        timeline_time,
                        strawberry_core::node::timeline::clip::TimeRemapFlags::empty(),
                    ) {
                        let random_access: bool = clip_node.needs_random_access();
                        if random_access {
                            tracing::debug!(
                                "Time remap active: speed={:.2}x reverse={} loop={:?} → source_time={:.3}s (random-access decode)",
                                clip_node.speed(),
                                clip_node.is_reverse(),
                                clip_node.loop_mode(),
                                source_time.as_secs_f64(),
                            );
                        }
                        layers.push(CompositorLayer {
                            clip_uuid: clip_node.stable_id().unwrap_or(Uuid::nil()),
                            asset_uuid,
                            source_time,
                            track_index: track_node.index(),
                            opacity: 1.0, // TODO: Get from ClipTransform
                            texture_id: None,
                            needs_random_access: random_access,
                            clip_speed: clip_node.speed() as f32,
                        });
                    }
                }
            }
        }

        layers
    }

    /// Get track count
    pub fn track_count(&self) -> usize {
        self.track_cache.len()
    }

    /// Get total clip count across all tracks
    pub fn clip_count(&self) -> usize {
        self.track_cache.iter().map(|t| t.clip_count()).sum()
    }
}

impl Default for NodeTraverser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TRAVERSER RESOURCE
// ============================================================================

/// ECS Resource wrapper for NodeTraverser
#[derive(Resource)]
pub struct TraverserResource(pub NodeTraverser);

impl Default for TraverserResource {
    fn default() -> Self {
        Self(NodeTraverser::new())
    }
}

impl std::ops::Deref for TraverserResource {
    type Target = NodeTraverser;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for TraverserResource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Shared active layers at the current playhead — written by compositor,
/// read by audio_mixer_sync_system.
///
/// This eliminates redundant `collect_layers()` calls. The compositor already
/// traverses the node tree each frame; storing the result here lets downstream
/// systems (audio, AI) read it for free.
#[derive(Resource, Default, Clone)]
pub struct ActiveLayersResource {
    pub layers: Vec<CompositorLayer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compositor_output_default() {
        let output: CompositorOutput = CompositorOutput::default();
        assert!(output.texture_id.is_none());
        assert!(!output.loading);
        assert!(output.error.is_none());
    }

    #[test]
    fn test_traverser_default() {
        let traverser: NodeTraverser = NodeTraverser::new();
        assert_eq!(traverser.output_resolution, glam::UVec2::new(1920, 1080));
        assert_eq!(traverser.output_frame_rate, 30.0);
        assert_eq!(traverser.track_count(), 0);
    }
}
