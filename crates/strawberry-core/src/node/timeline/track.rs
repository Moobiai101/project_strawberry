//! # Track Node
//!
//! Node implementation for timeline tracks.
//!
//! TrackNode wraps the ECS `Track` component and collects clips
//! for the current time range.

use crate::components::{StableId, Track, TrackType};
use crate::node::timeline::ClipNode;
use crate::node::{
    InputDefinition, InputFlags, Node, NodeCategory, NodeGlobals, NodeValue, NodeValueRow,
    NodeValueTable,
};

use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// TRACK NODE
// ============================================================================

/// Node wrapper for a timeline track.
///
/// TrackNode represents a single video or audio track on the timeline.
/// During processing, it:
/// 1. Queries its child clips
/// 2. Finds clips overlapping the current time
/// 3. Processes each overlapping clip
/// 4. Composites the results (for multiple overlapping clips)
pub struct TrackNode {
    /// Stable ID of this track entity
    stable_id: StableId,
    /// The track component data
    track: Track,
    /// Child clips (populated during traversal)
    clips: Vec<ClipNode>,
}

impl TrackNode {
    /// Create a new TrackNode from ECS components.
    ///
    /// # Arguments
    ///
    /// * `stable_id` - The track's stable ID component
    /// * `track` - The track component
    pub fn new(stable_id: StableId, track: Track) -> Self {
        Self {
            stable_id,
            track,
            clips: Vec::new(),
        }
    }

    /// Add a clip to this track for processing.
    ///
    /// Call this during traversal setup to populate the track with
    /// its child clips.
    pub fn add_clip(&mut self, clip: ClipNode) {
        self.clips.push(clip);
    }

    /// Add multiple clips at once
    pub fn with_clips(mut self, clips: impl IntoIterator<Item = ClipNode>) -> Self {
        self.clips.extend(clips);
        self
    }

    /// Get the track index (z-order)
    pub fn index(&self) -> u32 {
        self.track.index
    }

    /// Get the track type
    pub fn track_type(&self) -> TrackType {
        self.track.track_type
    }

    /// Check if the track is muted
    pub fn is_muted(&self) -> bool {
        self.track.muted
    }

    /// Check if the track is visible
    pub fn is_visible(&self) -> bool {
        self.track.visible
    }

    /// Check if this track is locked
    pub fn is_locked(&self) -> bool {
        self.track.locked
    }

    /// Get the track name
    pub fn name(&self) -> &str {
        &self.track.name
    }

    /// Get clips overlapping a given time
    pub fn clips_at_time(&self, time: Duration) -> impl Iterator<Item = &ClipNode> {
        self.clips.iter().filter(move |c| c.contains_time(time))
    }

    /// Get number of clips
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }
}

impl Node for TrackNode {
    fn id(&self) -> &'static str {
        "org.strawberry.track"
    }

    fn name(&self) -> &'static str {
        "Track"
    }

    fn category(&self) -> NodeCategory {
        NodeCategory::Timeline
    }

    fn inputs(&self) -> Vec<InputDefinition> {
        vec![
            // Clips input (connected internally during traversal)
            InputDefinition::texture("clips", "Clips")
                .with_flags(InputFlags::HIDDEN)
                .with_description("Internal clip compositor input"),
            // Track enabled
            InputDefinition::boolean("enabled", "Enabled", true)
                .with_flags(InputFlags::NOT_CONNECTABLE | InputFlags::NOT_KEYFRAMABLE)
                .with_description("Track visibility"),
            // Track opacity
            InputDefinition::float("opacity", "Opacity", 1.0)
                .with_range(0.0, 1.0)
                .with_description("Track opacity"),
        ]
    }

    fn process(&self, inputs: &NodeValueRow, globals: &NodeGlobals) -> NodeValueTable {
        let mut table: NodeValueTable = NodeValueTable::new();

        // Skip if muted or invisible
        if self.is_muted() || !self.is_visible() {
            return table;
        }

        // Get timeline time from globals
        let timeline_time: Duration = globals.time_range.start;

        // Find clips overlapping this time
        let overlapping_clips: Vec<&ClipNode> = self.clips_at_time(timeline_time).collect();

        if overlapping_clips.is_empty() {
            return table; // No clips at this time
        }

        // Process the first (topmost) overlapping clip
        // Phase 2 will add proper compositing for overlapping clips on same track
        if let Some(clip) = overlapping_clips.first() {
            let clip_table: NodeValueTable = clip.process(inputs, globals);

            // Get the first texture from the clip output
            if let Some(texture) = clip_table.first_texture() {
                // Apply track opacity (Phase 2: full transform/blend)
                let opacity: f64 = inputs
                    .get("opacity")
                    .and_then(|v| v.as_float())
                    .unwrap_or(1.0);

                if opacity >= 1.0 - f64::EPSILON {
                    // Full opacity - pass through
                    table.push(NodeValue::Texture(texture.clone()));
                } else if opacity > f64::EPSILON {
                    // Partial opacity - in Phase 2, apply via shader
                    // For now, just pass through
                    table.push(NodeValue::Texture(texture.clone()));
                }
                // If opacity is 0, don't add to table (invisible)
            } else {
                // Clip produced non-texture output (footage ref + duration)
                // Pass through for traverser to resolve
                for value in clip_table.iter() {
                    table.push(value.clone());
                }
            }
        }

        table
    }

    fn stable_id(&self) -> Option<Uuid> {
        Some(self.stable_id.uuid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Clip;

    fn make_test_track() -> TrackNode {
        let stable_id: StableId = StableId::new();
        let track: Track = Track {
            name: "Video 1".to_string(),
            track_type: TrackType::Video,
            index: 0,
            muted: false,
            visible: true,
            locked: false,
            solo: false,
        };
        TrackNode::new(stable_id, track)
    }

    fn make_test_clip(start_secs: u64, duration_secs: u64) -> ClipNode {
        let stable_id: StableId = StableId::new();
        let clip: Clip = Clip {
            name: "Test Clip".to_string(),
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(duration_secs),
            timeline_start: Duration::from_secs(start_secs),
        };
        ClipNode::new(stable_id, clip, None)
    }

    #[test]
    fn test_track_clips_at_time() {
        let mut track: TrackNode = make_test_track();
        track.add_clip(make_test_clip(0, 5)); // 0-5s
        track.add_clip(make_test_clip(3, 5)); // 3-8s
        track.add_clip(make_test_clip(10, 5)); // 10-15s

        // At 4s, should find 2 clips (0-5s and 3-8s)
        let clips_at_4: Vec<_> = track.clips_at_time(Duration::from_secs(4)).collect();
        assert_eq!(clips_at_4.len(), 2);

        // At 7s, should find 1 clip (3-8s)
        let clips_at_7: Vec<_> = track.clips_at_time(Duration::from_secs(7)).collect();
        assert_eq!(clips_at_7.len(), 1);

        // At 9s, should find no clips
        let clips_at_9: Vec<_> = track.clips_at_time(Duration::from_secs(9)).collect();
        assert_eq!(clips_at_9.len(), 0);
    }

    #[test]
    fn test_track_muted() {
        let stable_id: StableId = StableId::new();
        let track: Track = Track {
            name: "Video 1".to_string(),
            track_type: TrackType::Video,
            index: 0,
            muted: true,
            visible: true,
            locked: false,
            solo: false,
        };
        let node: TrackNode = TrackNode::new(stable_id, track);

        // Muted track should produce empty output
        let globals: NodeGlobals = NodeGlobals::default();
        let inputs: NodeValueRow = NodeValueRow::new();
        let table: NodeValueTable = node.process(&inputs, &globals);

        assert!(table.is_empty());
    }
}
