//! # Collision Resolution System
//!
//! Handles clip placement conflicts using CapCut-style resolution.
//! Clips CAN overlap during drag, but are resolved on drop.
//!
//! ## CapCut Model
//!
//! Unlike traditional NLEs that prevent overlaps, CapCut allows free placement
//! and then resolves conflicts on drop:
//!
//! | Mode | What Happens | When Used |
//! |------|--------------|-----------|
//! | **AutoTrack** | Creates new track below | Default behavior |
//! | **Ripple** | Pushes clips right | Insert mode |
//! | **Overwrite** | Trims overlapped portion | Replace mode |
//! | **Reject** | Cancels placement | Strict mode |
//!
//! ## Usage
//!
//! ```ignore
//! let mode = world.resource::<CollisionSettings>();
//! if let Some(collision) = check_collision(world, track, start, duration, None) {
//!     match mode.resolve(&collision) {
//!         CollisionAction::CreateTrack { .. } => create_new_track(),
//!         CollisionAction::RippleRight { shift } => shift_clips_right(shift),
//!         // ...
//!     }
//! }
//! ```

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// COLLISION MODE
// ============================================================================

/// How to resolve when a clip is dropped where another exists.
///
/// This determines the automatic resolution behavior when clips overlap
/// after a drag-and-drop or paste operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CollisionMode {
    /// Create new track below (CapCut default)
    ///
    /// When a clip overlaps existing content, automatically create
    /// a new track and place the clip there. This preserves all
    /// existing content and allows stacking.
    #[default]
    AutoTrack,

    /// Push existing clips right to make room (Insert mode)
    ///
    /// All clips at or after the insertion point are shifted right
    /// by the duration of the inserted clip. This creates a gap
    /// that the new clip fills.
    Ripple,

    /// Trim/delete overlapped portions of existing clips (Replace mode)
    ///
    /// Existing clips that overlap are trimmed. Fully covered clips
    /// are deleted. Partially covered clips have their in/out points
    /// adjusted to not overlap.
    Overwrite,

    /// Reject placement if overlap (Traditional mode)
    ///
    /// Simply prevents placement. The clip returns to its original
    /// position. Used when precise manual control is needed.
    Reject,
}

impl CollisionMode {
    /// Get human-readable name for UI display
    pub fn display_name(&self) -> &'static str {
        match self {
            CollisionMode::AutoTrack => "Auto Track",
            CollisionMode::Ripple => "Ripple Insert",
            CollisionMode::Overwrite => "Overwrite",
            CollisionMode::Reject => "Reject Overlap",
        }
    }

    /// Get description for tooltips
    pub fn description(&self) -> &'static str {
        match self {
            CollisionMode::AutoTrack => "Create new track when clips overlap",
            CollisionMode::Ripple => "Push all clips right to insert",
            CollisionMode::Overwrite => "Replace overlapped content",
            CollisionMode::Reject => "Prevent overlapping placements",
        }
    }
}

// ============================================================================
// COLLISION INFO
// ============================================================================

/// Information about a detected collision
///
/// Returned by `check_collision()` when a placement would overlap
/// existing clips. Contains details needed to resolve the conflict.
#[derive(Debug, Clone)]
pub struct CollisionInfo {
    /// UUIDs of clips that would be affected by the collision
    pub affected_clips: Vec<Uuid>,

    /// Track entity where collision was detected
    pub track_uuid: Uuid,

    /// Start of overlapping region (seconds)
    pub overlap_start_secs: f64,

    /// End of overlapping region (seconds)
    pub overlap_end_secs: f64,

    /// Total duration of overlap
    pub overlap_duration: Duration,

    /// Suggested resolution based on current mode
    pub suggested_action: CollisionAction,
}

impl CollisionInfo {
    /// Create a new collision info
    pub fn new(
        affected_clips: Vec<Uuid>,
        track_uuid: Uuid,
        overlap_start: f64,
        overlap_end: f64,
    ) -> Self {
        let overlap_duration: Duration =
            Duration::from_secs_f64((overlap_end - overlap_start).max(0.0));

        Self {
            affected_clips,
            track_uuid,
            overlap_start_secs: overlap_start,
            overlap_end_secs: overlap_end,
            overlap_duration,
            suggested_action: CollisionAction::None,
        }
    }

    /// Set the suggested action
    pub fn with_action(mut self, action: CollisionAction) -> Self {
        self.suggested_action = action;
        self
    }

    /// Get count of affected clips
    pub fn affected_count(&self) -> usize {
        self.affected_clips.len()
    }

    /// Check if this is a minor collision (single clip, short overlap)
    pub fn is_minor(&self) -> bool {
        self.affected_clips.len() == 1 && self.overlap_duration.as_millis() < 100
    }
}

// ============================================================================
// COLLISION ACTION
// ============================================================================

/// Action to take to resolve a collision
///
/// Generated based on the current `CollisionMode` and the specifics
/// of the collision (which clips are affected, overlap duration, etc.)
#[derive(Debug, Clone)]
pub enum CollisionAction {
    /// No collision - place normally
    None,

    /// Create new track and place there
    CreateTrack {
        /// Suggested name for the new track
        suggested_name: String,
        /// Index where track should be inserted
        insert_at_index: u32,
    },

    /// Ripple: shift these clips right by this duration
    RippleRight {
        /// UUIDs of clips to shift
        clips_to_shift: Vec<Uuid>,
        /// Amount to shift each clip
        shift_amount: Duration,
    },

    /// Overwrite: trim these clips
    TrimClips {
        /// Each entry: (clip_uuid, new_in_point, new_out_point)
        /// If new duration would be <= 0, clip should be deleted
        trims: Vec<ClipTrimAction>,
    },

    /// Reject: cannot place here
    Reject {
        /// Human-readable reason for rejection
        reason: String,
    },
}

/// Individual clip trim action for Overwrite mode
#[derive(Debug, Clone)]
pub struct ClipTrimAction {
    /// UUID of clip to trim
    pub clip_uuid: Uuid,
    /// New timeline start (for left-side trims)
    pub new_timeline_start: Option<Duration>,
    /// New duration (for any trim)
    pub new_duration: Option<Duration>,
    /// New source in point (for left-side trims)
    pub new_source_in: Option<Duration>,
    /// Should this clip be deleted entirely?
    pub should_delete: bool,
}

impl ClipTrimAction {
    /// Create a trim action to adjust clip start
    pub fn trim_left(clip_uuid: Uuid, new_start: Duration, new_source_in: Duration) -> Self {
        Self {
            clip_uuid,
            new_timeline_start: Some(new_start),
            new_duration: None,
            new_source_in: Some(new_source_in),
            should_delete: false,
        }
    }

    /// Create a trim action to adjust clip end
    pub fn trim_right(clip_uuid: Uuid, new_duration: Duration) -> Self {
        Self {
            clip_uuid,
            new_timeline_start: None,
            new_duration: Some(new_duration),
            new_source_in: None,
            should_delete: false,
        }
    }

    /// Create a delete action
    pub fn delete(clip_uuid: Uuid) -> Self {
        Self {
            clip_uuid,
            new_timeline_start: None,
            new_duration: None,
            new_source_in: None,
            should_delete: true,
        }
    }
}

// ============================================================================
// COLLISION SETTINGS RESOURCE
// ============================================================================

/// Global collision resolution settings
///
/// ECS Resource that controls how the timeline handles overlapping clips.
#[derive(Resource, Debug, Clone, Default)]
pub struct CollisionSettings {
    /// Current collision resolution mode
    pub mode: CollisionMode,

    /// Whether to show visual preview during drag
    pub show_collision_preview: bool,

    /// Minimum overlap (ms) before triggering collision
    /// Helps with frame-boundary precision
    pub collision_threshold_ms: u32,

    /// Whether auto-track mode should prefer video tracks
    pub prefer_video_tracks: bool,
}

impl CollisionSettings {
    /// Create with default settings
    pub fn new() -> Self {
        Self {
            mode: CollisionMode::AutoTrack,
            show_collision_preview: true,
            collision_threshold_ms: 16, // ~1 frame at 60fps
            prefer_video_tracks: true,
        }
    }

    /// Create with specific mode
    pub fn with_mode(mode: CollisionMode) -> Self {
        Self {
            mode,
            ..Self::new()
        }
    }

    /// Set collision mode
    pub fn set_mode(&mut self, mode: CollisionMode) {
        self.mode = mode;
    }

    /// Check if collisions should be ignored (below threshold)
    pub fn is_below_threshold(&self, overlap_ms: u64) -> bool {
        overlap_ms < self.collision_threshold_ms as u64
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collision_mode_display() {
        assert_eq!(CollisionMode::AutoTrack.display_name(), "Auto Track");
        assert_eq!(CollisionMode::Ripple.display_name(), "Ripple Insert");
        assert_eq!(CollisionMode::Overwrite.display_name(), "Overwrite");
        assert_eq!(CollisionMode::Reject.display_name(), "Reject Overlap");
    }

    #[test]
    fn test_collision_info() {
        let clip1_uuid = Uuid::new_v4();
        let clip2_uuid = Uuid::new_v4();
        let track_uuid = Uuid::new_v4();

        let collision = CollisionInfo::new(vec![clip1_uuid, clip2_uuid], track_uuid, 5.0, 7.5);

        assert_eq!(collision.affected_count(), 2);
        assert!((collision.overlap_duration.as_secs_f64() - 2.5).abs() < 0.001);
        assert!(!collision.is_minor());
    }

    #[test]
    fn test_minor_collision() {
        let clip_uuid = Uuid::new_v4();
        let track_uuid = Uuid::new_v4();

        let collision = CollisionInfo::new(
            vec![clip_uuid],
            track_uuid,
            1.0,
            1.05, // 50ms overlap
        );

        assert!(collision.is_minor());
    }

    #[test]
    fn test_collision_settings() {
        let settings = CollisionSettings::with_mode(CollisionMode::Ripple);
        assert_eq!(settings.mode, CollisionMode::Ripple);
        assert!(settings.show_collision_preview);
        assert!(settings.is_below_threshold(10));
        assert!(!settings.is_below_threshold(20));
    }

    #[test]
    fn test_clip_trim_action() {
        let uuid = Uuid::new_v4();

        let left_trim =
            ClipTrimAction::trim_left(uuid, Duration::from_secs(2), Duration::from_secs(2));
        assert!(!left_trim.should_delete);
        assert!(left_trim.new_timeline_start.is_some());

        let delete = ClipTrimAction::delete(uuid);
        assert!(delete.should_delete);
    }
}
