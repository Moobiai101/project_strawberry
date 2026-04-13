//! # Timeline Input Handlers
//!
//! Extracted input handling methods from handler.rs to maintain the 400-line limit.
//!
//! ## Contents
//!
//! - `handle_scroll()` — Mouse wheel zoom + horizontal/vertical scroll
//! - `handle_keyboard()` — Tool shortcuts (V/B/Y/U), ripple delete, razor cut
//! - `handle_cursor()` — Context-aware cursor icon changes
//!
//! These methods read input state and produce output values but never
//! directly mutate ECS state. State changes flow back through `TimelineResponse`.

use egui::{vec2, Rect, Response, Ui};
use std::time::Duration;

use super::widget::{TimelineWidget, MAX_ZOOM, MIN_ZOOM, PLAYHEAD_GRAB_WIDTH};
use super::TimelineTool;

// ============================================================================
// SCROLL HANDLING
// ============================================================================

impl TimelineWidget {
    /// Handle mouse wheel and trackpad scroll events.
    ///
    /// Behaviors:
    /// - **Ctrl + scroll Y**: Zoom in/out (factor 1.1 / 0.9)
    /// - **Scroll X or Y (no modifier)**: Horizontal scroll (free mode)
    ///   or frame-snapped playhead seek (centered mode)
    /// - **Shift + scroll Y**: Vertical track scroll
    pub(crate) fn handle_scroll(
        &self,
        ui: &Ui,
        response: &Response,
        _content_rect: Rect,
        zoom_changed: &mut Option<f32>,
        scroll_changed: &mut Option<(f32, f32)>,
        seek_to: &mut Option<Duration>,
        is_scrubbing: &mut bool,
    ) {
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        let modifiers = ui.input(|i| i.modifiers);

        if response.hovered() && (scroll_delta.x.abs() > 0.01 || scroll_delta.y.abs() > 0.01) {
            if modifiers.ctrl {
                // Zoom
                if scroll_delta.y.abs() > 0.01 {
                    let zoom_factor: f32 = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                    let new_zoom: f32 = (self.zoom * zoom_factor).clamp(MIN_ZOOM, MAX_ZOOM);
                    *zoom_changed = Some(new_zoom);
                }
            } else {
                let (h_scroll, v_scroll): (f32, f32) = if modifiers.shift {
                    (0.0, scroll_delta.y)
                } else {
                    (scroll_delta.x + scroll_delta.y, 0.0)
                };

                if h_scroll.abs() > 0.01 {
                    if self.centered_playhead {
                        let time_change: f32 = h_scroll.abs() / self.zoom;
                        let frame_duration: f32 = 1.0_f32 / self.frame_rate as f32;
                        let frames_to_move: f32 = (time_change / frame_duration).round().max(1.0);
                        let snapped_time_change: f32 = frames_to_move * frame_duration;

                        let new_time: Duration = if h_scroll > 0.0 {
                            self.playhead_time
                                .saturating_sub(Duration::from_secs_f32(snapped_time_change))
                        } else {
                            self.playhead_time
                                .saturating_add(Duration::from_secs_f32(snapped_time_change))
                        };
                        *seek_to = Some(new_time);
                        *is_scrubbing = true;
                    } else {
                        let new_scroll_x: f32 = (self.scroll_x - h_scroll).max(0.0);
                        *scroll_changed = Some((new_scroll_x, self.scroll_y));
                    }
                }

                if v_scroll.abs() > 0.01 {
                    let new_scroll_y: f32 = (self.scroll_y - v_scroll).max(0.0);
                    *scroll_changed = Some((
                        scroll_changed.map(|(x, _)| x).unwrap_or(self.scroll_x),
                        new_scroll_y,
                    ));
                }
            }
        }
    }

    /// Handle keyboard shortcuts for tool switching and editing.
    ///
    /// Shortcuts:
    /// - **V**: Pointer tool
    /// - **B**: Razor tool
    /// - **Y**: Slip tool
    /// - **U**: Slide tool
    /// - **Alt+Delete / Shift+Backspace**: Ripple delete selected clip
    /// - **Click in Razor mode**: Cut at cursor position
    pub(crate) fn handle_keyboard(
        &self,
        ui: &Ui,
        response: &Response,
        content_rect: Rect,
    ) -> (Option<TimelineTool>, Option<u64>, Option<(u64, f64)>) {
        let mut tool_changed: Option<TimelineTool> = None;
        let mut ripple_delete: Option<u64> = None;
        let mut razor_cut: Option<(u64, f64)> = None;

        let has_focus: bool = response.has_focus();
        let is_hovered: bool = response.hovered();

        if is_hovered || has_focus {
            ui.input(|input| {
                if !input.modifiers.any() {
                    if input.key_pressed(egui::Key::V) {
                        tool_changed = Some(TimelineTool::Pointer);
                    } else if input.key_pressed(egui::Key::B) {
                        tool_changed = Some(TimelineTool::Razor);
                    } else if input.key_pressed(egui::Key::Y) {
                        tool_changed = Some(TimelineTool::Slip);
                    } else if input.key_pressed(egui::Key::U) {
                        tool_changed = Some(TimelineTool::Slide);
                    }
                }

                if (input.modifiers.alt && input.key_pressed(egui::Key::Delete))
                    || (input.modifiers.shift && input.key_pressed(egui::Key::Backspace))
                {
                    if let Some(selected) = self.clips.iter().find(|c| c.selected) {
                        ripple_delete = Some(selected.entity_bits);
                    }
                }
            });
        }

        // Razor tool click
        if self.active_tool == TimelineTool::Razor && response.clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    let x_in_content: f32 = pointer_pos.x - content_rect.left();
                    let cut_time: Duration = self.x_to_time(x_in_content);
                    razor_cut = Some((0, cut_time.as_secs_f64()));
                }
            }
        }

        (tool_changed, ripple_delete, razor_cut)
    }

    /// Update cursor icon based on pointer position and active tool.
    ///
    /// Cursor mapping:
    /// - **Ruler / Playhead grab zone**: Grab cursor
    /// - **Razor tool over content**: Crosshair
    /// - **Slip tool over clip**: Horizontal resize
    /// - **Slide tool over clip**: Grab
    /// - **Pointer tool over clip edge**: Horizontal resize (trim)
    pub(crate) fn handle_cursor(
        &self,
        ui: &Ui,
        response: &Response,
        ruler_rect: Rect,
        content_rect: Rect,
    ) {
        if response.hovered() {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                let playhead_x: f32 = content_rect.left() + self.time_to_x(self.playhead_time);
                let playhead_grab_rect: Rect = Rect::from_center_size(
                    egui::pos2(playhead_x, ruler_rect.center().y),
                    vec2(PLAYHEAD_GRAB_WIDTH, ruler_rect.height()),
                );

                if playhead_grab_rect.contains(pointer_pos) || ruler_rect.contains(pointer_pos) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                } else if content_rect.contains(pointer_pos) {
                    match self.active_tool {
                        TimelineTool::Razor => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                        TimelineTool::Slip => {
                            if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            }
                        }
                        TimelineTool::Slide => {
                            if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        }
                        TimelineTool::Pointer => {
                            if self.hit_test_clip_edge(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            } else if super::interaction::hit_test_keyframe_diamond(
                                pointer_pos,
                                content_rect,
                                &self.clips,
                                &self.tracks,
                                self.zoom,
                                self.scroll_x,
                            )
                            .is_some()
                            {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                            }
                        }
                    }
                }
            }
        }
    }
}
