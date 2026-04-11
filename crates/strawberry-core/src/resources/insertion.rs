//! # Insertion Intent
//!
//! Defines where clips should be placed on the timeline.
//!
//! ## Purpose
//!
//! When adding a clip to the timeline, the user can specify different
//! insertion modes:
//! - **EndOfTrack**: Auto-sequence at the end of existing clips
//! - **AtPlayhead**: Insert at current playhead position
//! - **AtPosition**: Insert at exact position (from drag-drop)
//! - **RippleAt**: Insert and push subsequent clips (Phase 2C)
//!
//! ## Frame Accuracy
//!
//! All positions are snapped to frame boundaries to prevent sub-frame drift.
//! This is critical for professional NLE behavior where edits must align
//! to frame boundaries for proper playback.
//!
//! ## Architecture
//!
//! This module is pure logic with no UI dependencies. It provides:
//! - `InsertionIntent` enum for specifying intent
//! - `resolve_insertion_position()` for calculating actual position
//! - `snap_to_frame_boundary()` for frame-accurate positioning

use bevy_ecs::prelude::*;
use bevy_hierarchy::Children;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::components::{Clip, Track, TrackType};
use crate::resources::Playhead;

/// Specifies where a clip should be inserted on the timeline
///
/// Used by AddAssetToTimelineCommand to determine precise positioning.
/// All positions are calculated with frame-accurate snapping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InsertionIntent {
    /// Auto-sequence: place at end of last clip on track
    ///
    /// This is the default behavior for double-click in MediaBrowser.
    /// The clip will be placed immediately after the last clip on the track.
    EndOfTrack,

    /// Insert at current playhead position
    ///
    /// Used for Shift+double-click in MediaBrowser.
    /// Position is captured at command execution time.
    AtPlayhead,

    /// Insert at an exact timeline position
    ///
    /// The f64 is seconds with sub-frame precision (will be snapped).
    /// Used for drag-drop operations where user specifies exact position.
    AtPosition(f64),

    /// Insert with ripple: push subsequent clips to make room
    ///
    /// Phase 2C feature: collision detection will adjust surrounding clips.
    /// Currently behaves same as AtPosition but marked for future extension.
    RippleAt(f64),
}

impl Default for InsertionIntent {
    fn default() -> Self {
        InsertionIntent::EndOfTrack
    }
}

impl InsertionIntent {
    /// Check if this intent requires playhead position at execution time
    pub fn needs_playhead(&self) -> bool {
        matches!(self, InsertionIntent::AtPlayhead)
    }

    /// Check if this intent uses auto-sequencing
    pub fn is_auto_sequence(&self) -> bool {
        matches!(self, InsertionIntent::EndOfTrack)
    }
}

/// Calculate the end time of the last clip on a track
///
/// Returns Duration::ZERO if track has no clips.
/// This is used for EndOfTrack insertion to auto-sequence clips.
pub fn calculate_track_end_time_from_world(world: &World, track_entity: Entity) -> Duration {
    let mut end_time: Duration = Duration::ZERO;

    // Get children of the track (clips)
    if let Some(children) = world.get::<Children>(track_entity) {
        for &child in children.iter() {
            if let Some(clip) = world.get::<Clip>(child) {
                let clip_end: Duration = clip.timeline_start + clip.duration();
                if clip_end > end_time {
                    end_time = clip_end;
                }
            }
        }
    }

    end_time
}

/// Resolve an InsertionIntent to a concrete timeline position
///
/// This function calculates the exact frame-accurate position where
/// a clip should be placed based on the specified intent.
///
/// # Arguments
///
/// * `world` - ECS world for querying playhead and track state
/// * `intent` - The insertion intent specifying where to place
/// * `target_track` - The track entity to insert on
/// * `frame_rate` - Timeline frame rate for snapping
///
/// # Returns
///
/// Frame-accurate Duration for clip placement
pub fn resolve_insertion_position(
    world: &World,
    intent: &InsertionIntent,
    target_track: Entity,
    frame_rate: f64,
) -> Duration {
    let raw_seconds: f64 = match intent {
        InsertionIntent::EndOfTrack => {
            calculate_track_end_time_from_world(world, target_track).as_secs_f64()
        }
        InsertionIntent::AtPlayhead => {
            // Get current playhead position
            world
                .get_resource::<Playhead>()
                .map(|p| p.current_time.as_secs_f64())
                .unwrap_or(0.0)
        }
        InsertionIntent::AtPosition(secs) => *secs,
        InsertionIntent::RippleAt(secs) => {
            // Phase 2C: Ripple insertion — will push subsequent clips to make room.
            // Currently behaves same as AtPosition until collision system implements ripple mode.
            *secs
        }
    };

    // Snap to frame boundary to prevent sub-frame drift
    snap_to_frame_boundary(raw_seconds, frame_rate)
}

/// Snap a time value to the nearest frame boundary
///
/// Prevents accumulation errors from floating-point arithmetic.
/// This is critical for professional NLE behavior.
///
/// # Example
///
/// At 30fps:
/// - 1.034s -> 1.0333...s (frame 31)
/// - 0.017s -> 0.0333...s (frame 1)
///
/// # Arguments
///
/// * `seconds` - Raw time value in seconds
/// * `frame_rate` - Timeline frame rate (fps)
///
/// # Returns
///
/// Duration snapped to nearest frame boundary
pub fn snap_to_frame_boundary(seconds: f64, frame_rate: f64) -> Duration {
    if frame_rate <= 0.0 {
        // Safety fallback for invalid frame rate
        return Duration::from_secs_f64(seconds.max(0.0));
    }

    // Calculate frame number (round to nearest frame)
    let frame_number: u64 = (seconds * frame_rate).round() as u64;

    // Convert back to duration at exact frame boundary
    Duration::from_secs_f64(frame_number as f64 / frame_rate)
}

/// Convert a Duration to its frame number at a given frame rate
///
/// Useful for display purposes and frame-based calculations.
pub fn duration_to_frame(duration: Duration, frame_rate: f64) -> u64 {
    if frame_rate <= 0.0 {
        return 0;
    }
    (duration.as_secs_f64() * frame_rate).round() as u64
}

/// Convert a frame number to Duration at a given frame rate
///
/// Inverse of duration_to_frame for precise frame-based positioning.
pub fn frame_to_duration(frame: u64, frame_rate: f64) -> Duration {
    if frame_rate <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(frame as f64 / frame_rate)
}

/// Find the first video track in the world
///
/// Returns None if no video track exists.
/// Used by AddAssetToTimelineCommand when no target track specified.
///
/// Note: Uses entity iteration with component access instead of query()
/// because query creation requires &mut World, but we only have &World.
pub fn find_first_video_track(world: &World) -> Option<Entity> {
    let mut best_track: Option<(Entity, u32)> = None; // (entity, index)

    // Iterate all entities and check for Track component
    for entity in world.iter_entities() {
        if let Some(track) = entity.get::<Track>() {
            if matches!(track.track_type, TrackType::Video) {
                match &mut best_track {
                    None => {
                        best_track = Some((entity.id(), track.index));
                    }
                    Some((_, best_index)) if track.index < *best_index => {
                        best_track = Some((entity.id(), track.index));
                    }
                    _ => {}
                }
            }
        }
    }

    best_track.map(|(e, _)| e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_to_frame_boundary_at_30fps() {
        let frame_rate = 30.0;

        // Exactly on frame 0
        let result = snap_to_frame_boundary(0.0, frame_rate);
        assert_eq!(result, Duration::ZERO);

        // Exactly on frame 1 (0.0333...)
        let result = snap_to_frame_boundary(1.0 / 30.0, frame_rate);
        assert!((result.as_secs_f64() - 1.0 / 30.0).abs() < 0.0001);

        // Between frames - should snap to nearest
        let result = snap_to_frame_boundary(0.025, frame_rate); // Between frame 0 and 1
        assert!((result.as_secs_f64() - 1.0 / 30.0).abs() < 0.0001); // Closer to frame 1

        // At 1 second = frame 30
        let result = snap_to_frame_boundary(1.0, frame_rate);
        assert!((result.as_secs_f64() - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_snap_to_frame_boundary_at_24fps() {
        let frame_rate = 24.0;

        // Frame 24 = 1 second
        let result = snap_to_frame_boundary(1.0, frame_rate);
        assert!((result.as_secs_f64() - 1.0).abs() < 0.0001);

        // Frame 12 = 0.5 second
        let result = snap_to_frame_boundary(0.5, frame_rate);
        assert!((result.as_secs_f64() - 0.5).abs() < 0.0001);
    }

    #[test]
    fn test_snap_to_frame_boundary_invalid_fps() {
        // Should return the raw value clamped to 0
        let result = snap_to_frame_boundary(1.5, 0.0);
        assert_eq!(result, Duration::from_secs_f64(1.5));

        let result = snap_to_frame_boundary(-1.0, 30.0);
        // Negative becomes 0 due to rounding
        assert_eq!(result, Duration::ZERO);
    }

    #[test]
    fn test_duration_to_frame_conversion() {
        let frame_rate = 30.0;

        assert_eq!(duration_to_frame(Duration::ZERO, frame_rate), 0);
        assert_eq!(duration_to_frame(Duration::from_secs(1), frame_rate), 30);
        assert_eq!(duration_to_frame(Duration::from_secs(2), frame_rate), 60);
    }

    #[test]
    fn test_frame_to_duration_conversion() {
        let frame_rate = 30.0;

        assert_eq!(frame_to_duration(0, frame_rate), Duration::ZERO);
        assert!((frame_to_duration(30, frame_rate).as_secs_f64() - 1.0).abs() < 0.0001);
        assert!((frame_to_duration(60, frame_rate).as_secs_f64() - 2.0).abs() < 0.0001);
    }

    #[test]
    fn test_insertion_intent_default() {
        let intent = InsertionIntent::default();
        assert!(matches!(intent, InsertionIntent::EndOfTrack));
        assert!(intent.is_auto_sequence());
        assert!(!intent.needs_playhead());
    }

    #[test]
    fn test_insertion_intent_at_playhead() {
        let intent = InsertionIntent::AtPlayhead;
        assert!(intent.needs_playhead());
        assert!(!intent.is_auto_sequence());
    }

    #[test]
    fn test_insertion_intent_at_position() {
        let intent = InsertionIntent::AtPosition(5.5);
        assert!(!intent.needs_playhead());
        assert!(!intent.is_auto_sequence());

        if let InsertionIntent::AtPosition(secs) = intent {
            assert!((secs - 5.5).abs() < 0.0001);
        }
    }
}
