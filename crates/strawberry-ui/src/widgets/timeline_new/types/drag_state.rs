//! Drag state definitions for timeline interactions.
//!
//! Contains state structs for various drag operations:
//! - Move (ClipDragState)
//! - Trim (TrimDragState)
//! - Slip (SlipDragState)
//! - Slide (SlideDragState)
//! - Keyframe (KeyframeDragState)

use egui::Rect;
use std::time::Duration;

// ============================================================================
// DRAG ORIGIN
// ============================================================================

/// Where a drag operation originated from.
///
/// This is crucial for determining which operation to perform during dragged().
///
/// The `TrimEdge` variant carries the committed entity + edge from the initial
/// hit test (the "committed drag origin" pattern). This means we detect the
/// trim edge ONCE during cursor/drag_origin and reuse it in drag_started,
/// eliminating race conditions where the cursor position shifts by a few pixels
/// between hover and click frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOrigin {
    /// No drag active
    None,
    /// Drag started on ruler - do playhead scrub
    Ruler,
    /// Drag started on clip edge - do trim.
    /// Carries the committed entity_bits and edge from the initial hit test.
    TrimEdge {
        /// Entity bits of the clip whose edge was detected
        entity_bits: u64,
        /// Which edge (Left or Right) was detected
        edge: TrimEdge,
    },
    /// Drag started on clip body - do move
    ClipBody,
    /// Drag started on a keyframe diamond - do keyframe value/time edit.
    /// Higher priority than ClipBody, lower than TrimEdge.
    KeyframeDiamond,
    /// Drag started on empty area - do selection or nothing
    Empty,
}

// ============================================================================
// TRIM EDGE
// ============================================================================

/// Which edge of a clip is being trimmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    /// Left edge (in point)
    Left,
    /// Right edge (out point)
    Right,
}

// ============================================================================
// CLIP DRAG STATE (Move Operation)
// ============================================================================

/// Drag state for clip move operations.
///
/// Based on Olive's TimelineViewGhostItem pattern - tracks the clip being
/// dragged and its current ghost position for preview rendering.
///
/// The `click_offset` field stores the time difference between the cursor
/// position and the clip's start when the drag begins. This prevents the
/// clip from "jumping" to the cursor — instead, it moves smoothly relative
/// to where the user grabbed it (standard NLE behavior).
#[derive(Debug, Clone, Default)]
pub struct ClipDragState {
    /// Entity bits of the clip being dragged
    pub clip_entity: u64,
    /// Original track index
    pub original_track: u32,
    /// Original timeline start
    pub original_time: Duration,
    /// Current ghost track index (where it would land if dropped)
    pub ghost_track: u32,
    /// Current ghost time position
    pub ghost_time: Duration,
    /// Clip duration (for rendering)
    pub duration: Duration,
    /// Whether this is an active drag
    pub is_active: bool,
    /// Offset from click position to clip start (prevents clip jumping to cursor)
    ///
    /// Computed at drag start as: cursor_time - clip.timeline_start
    /// Applied during drag as: ghost_time = cursor_time - click_offset
    pub click_offset: Duration,
}

// ============================================================================
// TRIM DRAG STATE
// ============================================================================

/// Drag state for clip trim operations.
///
/// Tracks the clip being trimmed and the current ghost position for the
/// edge being adjusted. Used for visual preview during trim drag.
#[derive(Debug, Clone)]
pub struct TrimDragState {
    /// Entity bits of the clip being trimmed
    pub clip_entity: u64,
    /// Which edge is being trimmed
    pub edge: TrimEdge,
    /// Original timeline start
    pub original_timeline_start: Duration,
    /// Original duration
    pub original_duration: Duration,
    /// Current ghost new value (timeline_start for Left, new duration for Right)
    pub ghost_value: Duration,
    /// Whether this is an active trim drag
    pub is_active: bool,
}

// ============================================================================
// SLIP DRAG STATE
// ============================================================================

/// Drag state for slip operations.
///
/// Slipping shifts which portion of source media is visible without
/// changing clip position or duration on the timeline.
#[derive(Debug, Clone)]
pub struct SlipDragState {
    /// Clip entity being slipped
    pub clip_entity: u64,
    /// Original source_in value
    pub original_source_in: Duration,
    /// Original source_out value
    pub original_source_out: Duration,
    /// Current accumulated offset (can be negative via i64 millis)
    pub offset_millis: i64,
    /// Whether slip is active
    pub is_active: bool,
}

// ============================================================================
// SLIDE DRAG STATE
// ============================================================================

/// Drag state for slide operations.
///
/// Sliding moves a clip while adjusting the in/out points of
/// neighboring clips to maintain timeline coverage.
#[derive(Debug, Clone)]
pub struct SlideDragState {
    /// Clip entity being slid
    pub clip_entity: u64,
    /// Original timeline_start
    pub original_time: Duration,
    /// Previous clip entity (if any)
    pub previous_clip: Option<u64>,
    /// Previous clip's original source_out
    pub previous_original_out: Option<Duration>,
    /// Next clip entity (if any)
    pub next_clip: Option<u64>,
    /// Next clip's original timeline_start
    pub next_original_start: Option<Duration>,
    /// Next clip's original source_in
    pub next_original_in: Option<Duration>,
    /// Current delta offset (can be negative)
    pub delta_millis: i64,
    /// Whether slide is active
    pub is_active: bool,
}

// ============================================================================
// KEYFRAME DRAG STATE
// ============================================================================

/// Drag state for keyframe diamond dragging.
///
/// Tracks which keyframe is being dragged and its original values.
/// Vertical drag = value change, horizontal drag = time change.
/// Matches DaVinci Resolve and After Effects keyframe editing.
#[derive(Debug, Clone)]
pub struct KeyframeDragState {
    /// Entity bits of the clip owning the keyframe
    pub clip_entity_bits: u64,
    /// Property index (0=volume, 1=pan)
    pub property_index: usize,
    /// Keyframe index within the property's sorted list
    pub keyframe_index: usize,
    /// Original time before drag started (for undo)
    pub original_time_secs: f64,
    /// Original value before drag started (for undo)
    pub original_value: f32,
    /// Current dragged time
    pub current_time_secs: f64,
    /// Current dragged value
    pub current_value: f32,
    /// Value min for this property
    pub value_min: f32,
    /// Value max for this property
    pub value_max: f32,
    /// Source duration in seconds (for time mapping)
    pub source_duration_secs: f64,
    /// Draw rect of the clip (for coordinate mapping)
    pub draw_rect: Rect,
    /// Whether drag is active
    pub is_active: bool,
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_drag_state_default() {
        let state = ClipDragState::default();
        assert!(!state.is_active);
        assert_eq!(state.clip_entity, 0);
    }

    #[test]
    fn test_drag_origin_variants() {
        assert_eq!(DragOrigin::None, DragOrigin::None);
        assert_ne!(DragOrigin::Ruler, DragOrigin::ClipBody);
        let trim = DragOrigin::TrimEdge {
            entity_bits: 42,
            edge: TrimEdge::Left,
        };
        assert_ne!(trim, DragOrigin::ClipBody);
    }

    #[test]
    fn test_trim_edge_variants() {
        assert_eq!(TrimEdge::Left, TrimEdge::Left);
        assert_ne!(TrimEdge::Left, TrimEdge::Right);
    }
}
