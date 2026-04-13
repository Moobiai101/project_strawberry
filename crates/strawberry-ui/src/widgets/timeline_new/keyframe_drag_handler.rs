//! # Keyframe Diamond Drag Handler
//!
//! Handles drag-to-edit for keyframe diamonds on the timeline.
//! Users drag vertically to change value, horizontally to change time.
//!
//! ## Interaction Priority
//!
//! This handler only activates when `DragOrigin::KeyframeDiamond` was set
//! during `track_drag_origin()`. It does NOT interfere with clip move,
//! trim, or scrub — those have separate DragOrigin values.
//!
//! ## Visual Feedback
//!
//! During drag, the diamond is drawn in white with a larger size (6px vs 3.5px)
//! and a tooltip showing the current value. This matches DaVinci Resolve's
//! "active keyframe highlight" pattern.
//!
//! ## Performance
//!
//! - Zero allocations in the drag update path (only screen→value math)
//! - hit_test_keyframe_diamond() runs only on drag_started (not every frame)
//! - Ghost drawing uses a single diamond shape per frame

use egui::{pos2, Color32, Pos2, Rect, Response, Shape, Stroke, Ui};

use super::interaction::keyframe_drag::{
    hit_test_keyframe_diamond, screen_pos_to_keyframe_value,
};
use super::types::{DragOrigin, KeyframeDragState};
use super::widget::TimelineWidget;

// ============================================================================
// KEYFRAME DRAG HANDLING
// ============================================================================

impl TimelineWidget {
    /// Handle keyframe diamond drag interactions.
    ///
    /// Returns (keyframe_edit, updated_keyframe_drag_state) where:
    /// - `keyframe_edit`: Some(...) when drag completes — the final value/time
    /// - `updated_keyframe_drag_state`: The current drag state to persist
    #[allow(clippy::type_complexity)]
    pub(crate) fn handle_keyframe_drag(
        &self,
        ui: &Ui,
        response: &Response,
        content_rect: Rect,
        painter: &egui::Painter,
        drag_origin: DragOrigin,
    ) -> (
        Option<(u64, usize, f64, f64, f32)>,
        Option<KeyframeDragState>,
    ) {
        let mut keyframe_edit: Option<(u64, usize, f64, f64, f32)> = None;
        let mut state: Option<KeyframeDragState> = self.keyframe_drag_state.clone();

        // =====================================================================
        // DRAG START: Detect which diamond was clicked
        // =====================================================================
        if response.drag_started() && drag_origin == DragOrigin::KeyframeDiamond {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    if let Some(hit) = hit_test_keyframe_diamond(
                        pointer_pos,
                        content_rect,
                        &self.clips,
                        &self.tracks,
                        self.zoom,
                        self.scroll_x,
                    ) {
                        tracing::info!(
                            "Keyframe drag started: clip={}, prop={}, kf={}, t={:.3}s, v={:.3}",
                            hit.clip_entity_bits,
                            hit.property_index,
                            hit.keyframe_index,
                            hit.original_time_secs,
                            hit.original_value,
                        );

                        state = Some(KeyframeDragState {
                            clip_entity_bits: hit.clip_entity_bits,
                            property_index: hit.property_index,
                            keyframe_index: hit.keyframe_index,
                            original_time_secs: hit.original_time_secs,
                            original_value: hit.original_value,
                            current_time_secs: hit.original_time_secs,
                            current_value: hit.original_value,
                            value_min: hit.value_min,
                            value_max: hit.value_max,
                            source_duration_secs: hit.source_duration_secs,
                            draw_rect: hit.draw_rect,
                            is_active: true,
                        });
                    }
                }
            }
        }

        // =====================================================================
        // DRAG UPDATE: Convert cursor position to new time/value
        // =====================================================================
        if response.dragged() {
            if let Some(ref mut kf_drag) = state {
                if kf_drag.is_active {
                    if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                        let (new_time, new_value) = screen_pos_to_keyframe_value(
                            pointer_pos,
                            kf_drag.draw_rect,
                            kf_drag.source_duration_secs,
                            kf_drag.value_min,
                            kf_drag.value_max,
                        );
                        kf_drag.current_time_secs = new_time;
                        kf_drag.current_value = new_value;
                    }
                }
            }
        }

        // =====================================================================
        // DRAG COMPLETE: Emit the edit if value/time changed
        // =====================================================================
        if response.drag_stopped() {
            if let Some(ref kf_drag) = state {
                if kf_drag.is_active {
                    let time_changed: bool =
                        (kf_drag.current_time_secs - kf_drag.original_time_secs).abs() > 0.001;
                    let value_changed: bool =
                        (kf_drag.current_value - kf_drag.original_value).abs() > 0.001;

                    if time_changed || value_changed {
                        tracing::info!(
                            "Keyframe drag completed: t={:.3}s→{:.3}s, v={:.3}→{:.3}",
                            kf_drag.original_time_secs,
                            kf_drag.current_time_secs,
                            kf_drag.original_value,
                            kf_drag.current_value,
                        );

                        keyframe_edit = Some((
                            kf_drag.clip_entity_bits,
                            kf_drag.property_index,
                            kf_drag.original_time_secs,
                            kf_drag.current_time_secs,
                            kf_drag.current_value,
                        ));
                    }
                }
            }
            state = None;
        }

        // =====================================================================
        // GHOST PREVIEW: Draw the diamond at its current drag position
        // =====================================================================
        if let Some(ref kf_drag) = state {
            if kf_drag.is_active {
                self.draw_keyframe_drag_ghost(painter, kf_drag);
            }
        }

        (keyframe_edit, state)
    }

    /// Draw a highlighted diamond at the current drag position.
    ///
    /// Uses a larger diamond (6px), white fill, and a tooltip label
    /// showing the current value. This matches DaVinci Resolve's
    /// "active keyframe" visual feedback.
    fn draw_keyframe_drag_ghost(
        &self,
        painter: &egui::Painter,
        kf_drag: &KeyframeDragState,
    ) {
        let draw_rect: Rect = kf_drag.draw_rect;
        let draw_width: f32 = draw_rect.width();
        let draw_height: f32 = draw_rect.height();

        if draw_width < 2.0 || draw_height < 2.0 {
            return;
        }

        let value_range: f32 = kf_drag.value_max - kf_drag.value_min;
        if value_range.abs() < f32::EPSILON {
            return;
        }

        // Map current drag values to screen position
        let t_norm: f32 = if kf_drag.source_duration_secs > 0.0 {
            (kf_drag.current_time_secs / kf_drag.source_duration_secs) as f32
        } else {
            0.0
        };
        let v_norm: f32 =
            ((kf_drag.current_value - kf_drag.value_min) / value_range).clamp(0.0, 1.0);

        let kf_x: f32 = draw_rect.left() + t_norm.clamp(0.0, 1.0) * draw_width;
        let kf_y: f32 = draw_rect.bottom() - v_norm * draw_height;
        let center: Pos2 = pos2(kf_x, kf_y);

        // Larger diamond for active state
        let diamond_size: f32 = 6.0;
        let diamond: Vec<Pos2> = vec![
            pos2(center.x, center.y - diamond_size),
            pos2(center.x + diamond_size, center.y),
            pos2(center.x, center.y + diamond_size),
            pos2(center.x - diamond_size, center.y),
        ];

        // White fill with gold border for visibility
        painter.add(Shape::convex_polygon(
            diamond,
            Color32::WHITE,
            Stroke::new(1.5, Color32::from_rgb(255, 215, 0)),
        ));

        // Value tooltip above the diamond
        let label: String = if kf_drag.property_index == 0 {
            format!("{:.0}%", kf_drag.current_value * 100.0)
        } else {
            format!("{:.0}%", kf_drag.current_value * 100.0)
        };

        let text_pos: Pos2 = pos2(center.x, center.y - diamond_size - 12.0);
        painter.text(
            text_pos,
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyframe_drag_state_creation() {
        let state: KeyframeDragState = KeyframeDragState {
            clip_entity_bits: 42,
            property_index: 0,
            keyframe_index: 1,
            original_time_secs: 1.0,
            original_value: 0.8,
            current_time_secs: 2.0,
            current_value: 0.5,
            value_min: 0.0,
            value_max: 2.0,
            source_duration_secs: 10.0,
            draw_rect: Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(200.0, 30.0)),
            is_active: true,
        };
        assert!(state.is_active);
        assert_eq!(state.property_index, 0);
    }
}
