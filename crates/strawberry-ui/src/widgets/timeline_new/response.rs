//! Timeline widget response type.
//!
//! Contains the response struct returned by TimelineWidget::show().

use egui::{Pos2, Response};
use std::time::Duration;

use super::interaction::TimelineTool;
use super::types::{ClipDragState, SlideDragState, SlipDragState, TrimDragState, TrimEdge};

// ============================================================================
// TIMELINE RESPONSE
// ============================================================================

/// Response returned by TimelineWidget::show()
///
/// Contains all interaction results that the consuming code needs to handle,
/// including seeks, clip actions, tool changes, and state updates.
#[derive(Debug)]
pub struct TimelineResponse {
    /// Base egui response
    pub response: Response,

    /// New playhead time if user clicked on ruler
    pub seek_to: Option<Duration>,

    /// Clip that was clicked
    pub clicked_clip: Option<u64>,

    /// Clip that was double-clicked
    pub double_clicked_clip: Option<u64>,

    /// New scroll position (x, y)
    pub scroll_changed: Option<(f32, f32)>,

    /// New zoom level
    pub zoom_changed: Option<f32>,

    // =========================================================================
    // TRACK BUTTON TOGGLES
    // =========================================================================
    /// Track mute button toggled (track_entity_bits, new_state)
    pub toggle_track_mute: Option<(u64, bool)>,

    /// Track solo button toggled (track_entity_bits, new_state)
    pub toggle_track_solo: Option<(u64, bool)>,

    /// Track lock button toggled (track_entity_bits, new_state)
    pub toggle_track_lock: Option<(u64, bool)>,

    // =========================================================================
    // CLIP OPERATIONS
    // =========================================================================
    /// Clip move completed (clip_entity_bits, target_track_entity, new_time_seconds)
    /// Emitted when a drag operation completes (mouse release)
    pub move_clip: Option<(u64, Option<u64>, f64)>,

    /// Updated drag state to persist between frames
    /// Pass this back to the widget on the next frame via with_drag_state()
    pub updated_drag_state: Option<ClipDragState>,

    /// Clip that was right-clicked (for context menu)
    /// Contains (clip_entity_bits, click_position) for context menu positioning
    pub context_menu_clip: Option<(u64, Pos2)>,

    /// Trim operation completed (clip_entity_bits, edge, new_value_seconds)
    /// Emitted when a trim drag operation completes
    pub trim_clip: Option<(u64, TrimEdge, f64)>,

    /// Updated trim drag state to persist between frames
    pub updated_trim_state: Option<TrimDragState>,

    // =========================================================================
    // EDITING TOOL OPERATIONS
    // =========================================================================
    /// Razor cut requested at position (clip_entity_bits, time_seconds)
    /// Emitted when user clicks with Razor tool active
    pub razor_cut: Option<(u64, f64)>,

    /// Slip operation completed (clip_entity_bits, offset_seconds)
    /// Emitted when a slip drag operation completes
    pub slip_clip: Option<(u64, f64)>,

    /// Updated slip drag state to persist between frames
    pub updated_slip_state: Option<SlipDragState>,

    /// Slide operation completed (clip_entity_bits, delta_seconds)
    /// Emitted when a slide drag operation completes
    pub slide_clip: Option<(u64, f64)>,

    /// Updated slide drag state to persist between frames
    pub updated_slide_state: Option<SlideDragState>,

    /// Ripple delete requested for a clip (clip_entity_bits)
    /// Emitted when user presses Shift+Delete on selected clip
    pub ripple_delete: Option<u64>,

    /// Tool mode changed - new active tool
    pub tool_changed: Option<TimelineTool>,

    /// Current active tool (for cursor display purposes)
    pub active_tool: TimelineTool,

    /// Whether user is actively scrubbing (dragging playhead/ruler)
    ///
    /// True during drag, false when drag ends. Consuming code should use this
    /// to update `ScrubState` resource for compositor optimization.
    pub is_scrubbing: bool,

    // =========================================================================
    // CLIP AUDIO
    // =========================================================================
    /// Clip audio mute toggled via 🔊/🔇 icon click (clip_entity_bits, current_muted_state)
    /// Consuming code should submit `SetClipAudioCommand::set_muted(!current_muted)`
    pub toggle_clip_mute: Option<(u64, bool)>,

    /// Track selected by clicking its header icon (track_entity_bits).
    /// Consuming code should update Selection resource or show track in Inspector.
    pub selected_track: Option<u64>,
}

impl TimelineResponse {
    /// Create a new, empty response with the given base egui response.
    pub fn new(response: Response) -> Self {
        Self {
            response,
            seek_to: None,
            clicked_clip: None,
            double_clicked_clip: None,
            scroll_changed: None,
            zoom_changed: None,
            toggle_track_mute: None,
            toggle_track_solo: None,
            toggle_track_lock: None,
            move_clip: None,
            updated_drag_state: None,
            context_menu_clip: None,
            trim_clip: None,
            updated_trim_state: None,
            razor_cut: None,
            slip_clip: None,
            updated_slip_state: None,
            slide_clip: None,
            updated_slide_state: None,
            ripple_delete: None,
            tool_changed: None,
            active_tool: TimelineTool::Pointer,
            is_scrubbing: false,
            toggle_clip_mute: None,
            selected_track: None,
        }
    }

    /// Check if any action requires processing.
    pub fn has_action(&self) -> bool {
        self.seek_to.is_some()
            || self.clicked_clip.is_some()
            || self.double_clicked_clip.is_some()
            || self.move_clip.is_some()
            || self.trim_clip.is_some()
            || self.razor_cut.is_some()
            || self.slip_clip.is_some()
            || self.slide_clip.is_some()
            || self.ripple_delete.is_some()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    // Note: TimelineResponse tests require a full egui Response which needs a running context.
    // Unit tests for has_action() would need integration test setup.
}
