//! # Timeline Components
//!
//! Timeline-specific ECS components for the multi-track editing system.
//!
//! ## Architecture
//!
//! The timeline uses Bevy's built-in hierarchy system (`Parent`/`Children`) for relationships:
//!
//! ```text
//! Timeline (entity)
//!     ├── Track (entity, child of Timeline)
//!     │   ├── Clip (entity, child of Track)
//!     │   └── Clip (entity, child of Track)
//!     └── Track (entity, child of Timeline)
//!         └── Clip (entity, child of Track)
//! ```
//!
//! This approach:
//! - Uses Bevy's optimized parent-child traversal
//! - Automatically handles cleanup when parent is despawned
//! - Avoids storing `Entity` in components (which can go stale)
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Create timeline with tracks
//! let timeline = commands.spawn((
//!     Timeline::new("Main Timeline", FrameRate::Fps30),
//!     SpatialBundle::default(),
//! )).id();
//!
//! let track = commands.spawn((
//!     Track::video("V1"),
//!     TimelineChild,
//! )).id();
//!
//! commands.entity(timeline).add_child(track);
//! ```

use bevy_ecs::component::Component;
use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::FrameRate;

// ============================================================================
// TIMELINE COMPONENT
// ============================================================================

/// The root timeline component.
///
/// A Timeline is the container for all tracks and clips that make up an edit.
/// Unlike traditional NLEs that have a single timeline, Strawberry supports
/// multiple timelines (like Premiere Pro sequences).
///
/// ## Frame Rate
///
/// The timeline's frame rate determines:
/// - Playhead quantization (snapping to frame boundaries)
/// - Timecode display format
/// - Export frame timing
///
/// Individual clips may have different source frame rates; the timeline
/// will handle frame blending/interpolation during playback and export.
///
/// ## Duration
///
/// Duration is **computed** from the latest clip endpoint, not stored.
/// This ensures consistency and avoids state synchronization issues.
/// Use `TimelineDuration` resource or compute via query when needed.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// Human-readable timeline name (e.g., "Main Sequence", "Rough Cut v2")
    pub name: String,

    /// Master frame rate for this timeline
    pub frame_rate: FrameRate,

    /// Default track count created with new timeline
    /// (not enforced after creation)
    #[serde(skip)]
    pub default_video_tracks: u32,

    /// Default audio track count created with new timeline
    #[serde(skip)]
    pub default_audio_tracks: u32,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            name: "Timeline 1".to_string(),
            frame_rate: FrameRate::Fps30,
            default_video_tracks: 2,
            default_audio_tracks: 2,
        }
    }
}

impl Timeline {
    /// Create a new timeline with the given name and frame rate
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        Self {
            name: name.into(),
            frame_rate,
            ..Default::default()
        }
    }

    /// Create a timeline with standard settings for the given name
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get frame number for a given time position
    pub fn time_to_frame(&self, time: Duration) -> u64 {
        self.frame_rate.timestamp_to_frame(time)
    }

    /// Get time position for a given frame number
    pub fn frame_to_time(&self, frame: u64) -> Duration {
        self.frame_rate.frame_to_timestamp(frame)
    }

    /// Get frame duration
    pub fn frame_duration(&self) -> Duration {
        self.frame_rate.frame_duration()
    }

    /// Quantize a time to the nearest frame boundary
    pub fn quantize_to_frame(&self, time: Duration) -> Duration {
        let frame = self.time_to_frame(time);
        self.frame_to_time(frame)
    }
}

// ============================================================================
// MARKER COMPONENTS FOR HIERARCHY
// ============================================================================

/// Marker component indicating this entity is part of a Timeline hierarchy.
///
/// Attach to tracks and clips to enable efficient queries like:
/// ```rust,ignore
/// fn get_timeline_children(
///     timelines: Query<&Children, With<Timeline>>,
///     timeline_children: Query<Entity, With<TimelineChild>>,
/// ) { ... }
/// ```
#[derive(Component, Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TimelineChild;

/// Marker component indicating this entity is a clip on a track.
///
/// Clips have both `TimelineChild` (for timeline-wide queries) and
/// `TrackClip` (for track-specific queries).
#[derive(Component, Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TrackClip;

// ============================================================================
// CLIP LOOP MODE
// ============================================================================

/// Per-clip loop mode for source media playback.
///
/// Controls how a clip handles source time that extends beyond its
/// `source_in..source_out` range (e.g. when speed < 1.0 makes the
/// clip play longer than its source media, or when explicitly looped).
///
/// This is **distinct** from the Playhead-level `LoopMode` (which controls
/// timeline-level loop playback). Per-clip loop mode is about how the
/// source media itself repeats within the clip's timeline duration.
///
/// Based on Olive Editor's `clip.h:188-195` loop semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ClipLoopMode {
    /// No looping — clamp to source boundaries (standard NLE behavior).
    /// If source time exceeds source_out, the last frame is held.
    #[default]
    None,

    /// Loop: when source time exceeds source duration, wrap around to the
    /// beginning. Used for seamless background loops, music loops, etc.
    Loop,

    /// Ping-pong: alternate between forward and reverse at source boundaries.
    /// Produces a smooth back-and-forth effect without a hard cut.
    PingPong,

    /// Clamp: freeze on the last frame when source time exceeds duration.
    /// Identical to `None` for standard playback, but semantically explicit
    /// and differs when combined with reverse playback.
    Clamp,
}

// ============================================================================
// CLIP EXTENSIONS
// ============================================================================

/// Extended clip metadata for timeline editing.
///
/// This component stores additional metadata that supplements the base `Clip`
/// component for timeline operations. Attach to clip entities alongside `Clip`.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct ClipMetadata {
    /// Display color for this clip in the timeline (RGBA)
    pub color: [u8; 4],

    /// Whether this clip is enabled (disabled clips are skipped in playback)
    pub enabled: bool,

    /// Speed multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double)
    /// Note: Audio pitch correction is handled separately
    pub speed: f32,

    /// Reverse playback
    pub reverse: bool,

    /// Per-clip source media loop mode.
    /// Controls how the source repeats when playback exceeds source duration.
    #[serde(default)]
    pub loop_mode: ClipLoopMode,

    /// User-defined labels/tags for organization
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Default for ClipMetadata {
    fn default() -> Self {
        Self {
            color: [100, 149, 237, 255], // Cornflower blue
            enabled: true,
            speed: 1.0,
            reverse: false,
            loop_mode: ClipLoopMode::None,
            labels: Vec::new(),
        }
    }
}

impl ClipMetadata {
    /// Create with a custom color (RGB, alpha defaults to 255)
    pub fn with_color(r: u8, g: u8, b: u8) -> Self {
        Self {
            color: [r, g, b, 255],
            ..Default::default()
        }
    }

    /// Create with a speed multiplier
    pub fn with_speed(speed: f32) -> Self {
        Self {
            speed: speed.clamp(0.01, 100.0),
            ..Default::default()
        }
    }

    /// Get effective duration considering speed
    pub fn effective_duration(&self, base_duration: Duration) -> Duration {
        if self.speed <= 0.0 {
            return base_duration;
        }
        Duration::from_secs_f64(base_duration.as_secs_f64() / self.speed as f64)
    }
}

// ============================================================================
// TRACK EXTENSIONS
// ============================================================================

/// Track display settings for the timeline UI.
///
/// Separated from `Track` to avoid polluting the core component with UI concerns.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct TrackDisplay {
    /// Track height in pixels (for UI rendering)
    pub height: f32,

    /// Track color (RGB, for header and clip tinting)
    pub color: [u8; 3],

    /// Whether track header is collapsed (showing minimal info)
    pub collapsed: bool,

    /// Whether to show clip thumbnails on this track
    pub show_thumbnails: bool,

    /// Whether to show audio waveforms on this track
    pub show_waveforms: bool,
}

impl Default for TrackDisplay {
    fn default() -> Self {
        Self {
            height: 60.0,
            color: [80, 80, 90],
            collapsed: false,
            show_thumbnails: true,
            show_waveforms: true,
        }
    }
}

impl TrackDisplay {
    /// Create with a custom height
    pub fn with_height(height: f32) -> Self {
        Self {
            height: height.clamp(20.0, 200.0),
            ..Default::default()
        }
    }

    /// Create with a custom color
    pub fn with_color(r: u8, g: u8, b: u8) -> Self {
        Self {
            color: [r, g, b],
            ..Default::default()
        }
    }
}

// ============================================================================
// TIMELINE RESOURCE
// ============================================================================

/// Global resource tracking the active timeline.
///
/// Most operations (playback, editing) happen on the "active" timeline.
/// This resource stores the entity ID of the active timeline.
///
/// When no timeline is active, operations that require a timeline will
/// either fail gracefully or prompt timeline creation.
#[derive(Resource, Debug, Clone, Default)]
pub struct ActiveTimeline {
    /// The currently active timeline entity (as bits for thread-safety)
    entity_bits: Option<u64>,
}

impl ActiveTimeline {
    /// Create with no active timeline
    pub fn none() -> Self {
        Self { entity_bits: None }
    }

    /// Set the active timeline
    pub fn set(&mut self, entity: bevy_ecs::entity::Entity) {
        self.entity_bits = Some(entity.to_bits());
    }

    /// Clear the active timeline
    pub fn clear(&mut self) {
        self.entity_bits = None;
    }

    /// Get the active timeline entity
    pub fn get(&self) -> Option<bevy_ecs::entity::Entity> {
        self.entity_bits.map(bevy_ecs::entity::Entity::from_bits)
    }

    /// Check if there's an active timeline
    pub fn is_some(&self) -> bool {
        self.entity_bits.is_some()
    }

    /// Check if there's no active timeline
    pub fn is_none(&self) -> bool {
        self.entity_bits.is_none()
    }
}

// ============================================================================
// COMPUTED TIMELINE STATE
// ============================================================================

/// Computed timeline state that's updated by systems.
///
/// This component is added to Timeline entities and updated each frame
/// by the `timeline_state_system`. It caches expensive computations.
#[derive(Component, Debug, Clone, Default)]
pub struct TimelineState {
    /// Total duration (from earliest clip start to latest clip end)
    pub duration: Duration,

    /// Total number of tracks
    pub track_count: u32,

    /// Total number of clips across all tracks
    pub clip_count: u32,

    /// Frame at which the timeline ends
    pub end_frame: u64,
}

impl TimelineState {
    /// Create empty state
    pub fn empty() -> Self {
        Self::default()
    }

    /// Update with new computed values
    pub fn update(
        &mut self,
        duration: Duration,
        track_count: u32,
        clip_count: u32,
        frame_rate: &FrameRate,
    ) {
        self.duration = duration;
        self.track_count = track_count;
        self.clip_count = clip_count;
        self.end_frame = frame_rate.timestamp_to_frame(duration);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_creation() {
        let timeline = Timeline::new("Test Timeline", FrameRate::Fps24);
        assert_eq!(timeline.name, "Test Timeline");
        assert_eq!(timeline.frame_rate, FrameRate::Fps24);
    }

    #[test]
    fn test_timeline_frame_conversion() {
        let timeline = Timeline::new("Test", FrameRate::Fps30);

        // 1 second = 30 frames at 30fps
        let time = Duration::from_secs(1);
        assert_eq!(timeline.time_to_frame(time), 30);

        // 30 frames = 1 second
        let frame = 30;
        assert_eq!(timeline.frame_to_time(frame), Duration::from_secs(1));
    }

    #[test]
    fn test_timeline_quantize() {
        let timeline = Timeline::new("Test", FrameRate::Fps30);

        // 35ms at 30fps should quantize to frame 1
        // Frame 1 at 30fps = 1/30 seconds ≈ 33.33ms
        let time = Duration::from_millis(35);
        let frame_before = timeline.time_to_frame(time);
        assert_eq!(frame_before, 1, "35ms should be frame 1 at 30fps");

        let quantized = timeline.quantize_to_frame(time);
        // Quantized time should be exactly at frame boundary
        // Frame 1 = 1/30 sec = 33.333... ms
        let expected_ms = 1000.0 / 30.0;
        let actual_ms = quantized.as_secs_f64() * 1000.0;
        assert!(
            (actual_ms - expected_ms).abs() < 0.001,
            "Quantized time should be ~33.33ms, got {}ms",
            actual_ms
        );
    }

    #[test]
    fn test_clip_metadata_speed() {
        let meta = ClipMetadata::with_speed(2.0);
        let base_duration = Duration::from_secs(10);

        // At 2x speed, 10s of content plays in 5s
        let effective = meta.effective_duration(base_duration);
        assert_eq!(effective, Duration::from_secs(5));
    }

    #[test]
    fn test_active_timeline() {
        let mut active = ActiveTimeline::none();
        assert!(active.is_none());

        let entity = bevy_ecs::entity::Entity::from_raw(42);
        active.set(entity);
        assert!(active.is_some());
        assert_eq!(active.get(), Some(entity));

        active.clear();
        assert!(active.is_none());
    }
}
