//! Timeline widget show() implementation.
//!
//! Contains the main `show()` method that orchestrates the entire timeline UI.
//! This is the central coordinator — all heavy logic has been extracted to:
//!
//! - **drag_handler**: Move/trim drag operations + ghost preview rendering
//! - **input_handlers**: Scroll, keyboard shortcuts, cursor icon changes
//! - **drawing_pipeline**: Empty state drawing, bottom ruler ticks
//!
//! This file should remain under 400 lines (orchestration only).

use egui::{pos2, vec2, Rect, Response, Sense, Ui, Widget};
use std::time::Duration;

use super::rendering::colors;
use super::types::DragOrigin;
use super::widget::{TimelineWidget, RULER_HEIGHT, SCROLLBAR_HEIGHT, TRACK_HEADER_WIDTH};
use super::TimelineResponse;

// ============================================================================
// WIDGET TRAIT IMPLEMENTATION
// ============================================================================

impl Widget for TimelineWidget {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

// ============================================================================
// SHOW METHOD IMPLEMENTATION
// ============================================================================

impl TimelineWidget {
    /// Show the timeline widget and return interaction response
    ///
    /// This is the main entry point that orchestrates:
    /// 1. Layout calculation
    /// 2. Drawing (ruler, tracks, clips, playhead)
    /// 3. Input handling (clicks, drags, keyboard)
    /// 4. Response building
    pub fn show(mut self, ui: &mut Ui) -> TimelineResponse {
        // Calculate layout rectangles
        let available = ui.available_rect_before_wrap();

        // When empty, use full available height for clean CapCut-style look
        // When populated, use actual track height
        let total_height: f32 = if self.clips.is_empty() {
            // Use full available height for empty state (minimum 150px)
            available.height().max(150.0)
        } else {
            RULER_HEIGHT + self.total_track_height() + SCROLLBAR_HEIGHT
        };

        let widget_rect: Rect = Rect::from_min_size(
            available.min,
            vec2(available.width(), total_height.min(available.height())),
        );

        // Allocate space with click and drag sensing
        let (response, painter) = ui.allocate_painter(widget_rect.size(), Sense::click_and_drag());
        let rect: Rect = response.rect;

        // Layout regions
        let ruler_rect: Rect = Rect::from_min_size(
            pos2(rect.left() + TRACK_HEADER_WIDTH, rect.top()),
            vec2(rect.width() - TRACK_HEADER_WIDTH, RULER_HEIGHT),
        );

        let content_rect: Rect = Rect::from_min_size(
            pos2(rect.left() + TRACK_HEADER_WIDTH, rect.top() + RULER_HEIGHT),
            vec2(
                rect.width() - TRACK_HEADER_WIDTH,
                rect.height() - RULER_HEIGHT - SCROLLBAR_HEIGHT,
            ),
        );

        // =====================================================================
        // AUTO-SCROLL IN CENTERED MODE
        // =====================================================================
        if self.centered_playhead {
            let center_offset: f32 = content_rect.width() / 2.0;
            let center_time: f32 = center_offset / self.zoom;
            let playhead_pixel_pos: f32 = self.playhead_time.as_secs_f32() * self.zoom;

            if self.playhead_time.as_secs_f32() >= center_time {
                self.scroll_x = playhead_pixel_pos - center_offset;
            } else {
                self.scroll_x = 0.0;
            }
        }

        // =====================================================================
        // DRAWING
        // =====================================================================
        if self.clips.is_empty() {
            // CapCut-style empty state: full clean area with ruler at bottom
            let full_content_rect: Rect = Rect::from_min_size(
                rect.min,
                vec2(rect.width(), rect.height() - SCROLLBAR_HEIGHT),
            );
            self.draw_capcut_empty_state(&painter, full_content_rect);
        } else {
            // Normal timeline with ruler at top
            self.draw_ruler(ui, ruler_rect);
            self.draw_track_lanes(ui, content_rect);

            let hovered_clip: Option<u64> = self.detect_hovered_clip(ui, content_rect);
            self.draw_clips(ui, content_rect, hovered_clip, self.waveform_cache.as_ref());

            let hovered_gap: Option<(u64, u64)> = self.detect_hovered_gap(ui, content_rect);
            self.draw_transition_zones(ui, content_rect, hovered_gap);
            self.draw_playhead(ui, ruler_rect, content_rect);
        }

        // Hidden scrollbar
        let scrollbar_rect: Rect = Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - SCROLLBAR_HEIGHT),
            vec2(rect.width(), SCROLLBAR_HEIGHT),
        );
        painter.rect_filled(scrollbar_rect, 0.0, colors::SCROLLBAR_BG);

        // =====================================================================
        // INTERACTION HANDLING
        // =====================================================================
        let mut seek_to: Option<Duration> = None;
        let mut clicked_clip: Option<u64> = None;
        let mut context_menu_clip: Option<(u64, egui::Pos2)> = None;
        let mut zoom_changed: Option<f32> = None;
        let mut scroll_changed_val: Option<(f32, f32)> = None;
        let mut toggle_clip_mute: Option<(u64, bool)> = None;

        // Click handling — audio icon checked FIRST (before clip selection)
        self.handle_clicks(
            &response,
            ruler_rect,
            content_rect,
            &mut seek_to,
            &mut clicked_clip,
            &mut toggle_clip_mute,
        );

        // Drag origin tracking
        let drag_origin: DragOrigin =
            self.track_drag_origin(ui, &response, ruler_rect, content_rect);

        // Scrubbing
        let mut is_scrubbing: bool = self.handle_scrubbing(&response, drag_origin, &mut seek_to);

        // Right-click context menu
        self.handle_right_click(&response, content_rect, &mut context_menu_clip);

        // Scroll handling (extracted to input_handlers.rs)
        self.handle_scroll(
            ui,
            &response,
            content_rect,
            &mut zoom_changed,
            &mut scroll_changed_val,
            &mut seek_to,
            &mut is_scrubbing,
        );

        // Keyboard shortcuts (extracted to input_handlers.rs)
        let (tool_changed, ripple_delete, razor_cut) =
            self.handle_keyboard(ui, &response, content_rect);

        // Cursor change on hover (extracted to input_handlers.rs)
        self.handle_cursor(ui, &response, ruler_rect, content_rect);

        // Drag handling (extracted to drag_handler.rs)
        // Pass the committed drag_origin so handle_drag can use the pre-detected
        // entity+edge from track_drag_origin instead of re-running hit tests.
        let (move_clip, trim_clip, updated_drag_state, updated_trim_state) = self.handle_drag(
            ui,
            &response,
            content_rect,
            &painter,
            &mut seek_to,
            drag_origin,
        );

        // Track button toggles (placeholder)
        let toggle_track_mute: Option<(u64, bool)> = None;
        let toggle_track_solo: Option<(u64, bool)> = None;
        let toggle_track_lock: Option<(u64, bool)> = None;

        TimelineResponse {
            response,
            seek_to,
            clicked_clip,
            double_clicked_clip: None,
            scroll_changed: scroll_changed_val,
            zoom_changed,
            toggle_track_mute,
            toggle_track_solo,
            toggle_track_lock,
            move_clip,
            updated_drag_state,
            context_menu_clip,
            trim_clip,
            updated_trim_state,
            razor_cut,
            slip_clip: None,
            updated_slip_state: self.slip_state.clone(),
            slide_clip: None,
            updated_slide_state: self.slide_state.clone(),
            ripple_delete,
            tool_changed,
            active_tool: self.active_tool,
            is_scrubbing,
            toggle_clip_mute,
        }
    }

    // =========================================================================
    // HIT DETECTION HELPERS (kept here — tiny, used by multiple handlers)
    // =========================================================================

    fn detect_hovered_clip(&self, ui: &Ui, content_rect: Rect) -> Option<u64> {
        ui.input(|i| i.pointer.latest_pos()).and_then(|pos| {
            if content_rect.contains(pos) {
                self.hit_test_clip(pos, content_rect)
            } else {
                None
            }
        })
    }

    fn detect_hovered_gap(&self, ui: &Ui, content_rect: Rect) -> Option<(u64, u64)> {
        ui.input(|i| i.pointer.latest_pos()).and_then(|pos| {
            if content_rect.contains(pos) {
                self.hit_test_gap(pos, content_rect)
            } else {
                None
            }
        })
    }

    // =========================================================================
    // CLICK + DRAG ORIGIN HANDLERS (kept here — tight coupling with show())
    // =========================================================================

    fn handle_clicks(
        &self,
        response: &Response,
        ruler_rect: Rect,
        content_rect: Rect,
        seek_to: &mut Option<Duration>,
        clicked_clip: &mut Option<u64>,
        toggle_clip_mute: &mut Option<(u64, bool)>,
    ) {
        // egui's clicked() fires only on press→release with ZERO mouse movement.
        // Real users nearly always slip 1-2px, causing egui to classify the
        // interaction as a drag instead. To fix this, we detect clip selection
        // on BOTH clicked() and drag_started().
        //
        // - clicked()      → precise click (no movement)
        // - drag_started() → "click with slight movement" (common case)
        //
        // For ruler seeks, we only use clicked() since drags on the ruler mean
        // scrubbing, not seeking.

        if response.clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if ruler_rect.contains(pointer_pos) {
                    if !self.centered_playhead {
                        let x_in_content: f32 = pointer_pos.x - ruler_rect.left();
                        *seek_to = Some(self.x_to_time(x_in_content));
                    }
                } else if content_rect.contains(pointer_pos) {
                    // Check audio icon FIRST — icon clicks toggle mute, not selection
                    if let Some((entity_bits, current_muted)) =
                        Self::hit_test_audio_icon(pointer_pos, content_rect, &self.clips, self.zoom, self.scroll_x, &self.tracks)
                    {
                        *toggle_clip_mute = Some((entity_bits, current_muted));
                    } else {
                        *clicked_clip = self.hit_test_clip(pointer_pos, content_rect);
                        tracing::info!(
                            "SELECTION: clicked() hit_test_clip result={:?}",
                            clicked_clip
                        );
                        if clicked_clip.is_none() && !self.centered_playhead {
                            let x_in_content: f32 = pointer_pos.x - content_rect.left();
                            *seek_to = Some(self.x_to_time(x_in_content));
                        }
                    }
                }
            }
        }

        // Also select on drag_started — covers the "click with slight movement"
        // case that egui classifies as a drag rather than a click.
        // Only fires for clips in the content area (not ruler).
        if clicked_clip.is_none() && response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) && !ruler_rect.contains(pointer_pos) {
                    *clicked_clip = self.hit_test_clip(pointer_pos, content_rect);
                    tracing::info!(
                        "SELECTION: drag_started hit_test_clip result={:?}",
                        clicked_clip
                    );
                }
            }
        }
    }

    fn track_drag_origin(
        &self,
        ui: &mut Ui,
        response: &Response,
        ruler_rect: Rect,
        content_rect: Rect,
    ) -> DragOrigin {
        let mut drag_origin: DragOrigin = ui.memory(|mem| {
            mem.data
                .get_temp::<DragOrigin>(egui::Id::new("timeline_drag_origin"))
                .unwrap_or(DragOrigin::None)
        });

        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                drag_origin = if ruler_rect.contains(pointer_pos) {
                    DragOrigin::Ruler
                } else if content_rect.contains(pointer_pos) {
                    // Committed drag origin: store entity + edge at detection time.
                    // This result is reused in handle_drag, eliminating the need
                    // for a redundant hit_test_clip_edge call that could miss due
                    // to sub-pixel cursor drift between frames.
                    if let Some((entity_bits, edge)) =
                        self.hit_test_clip_edge(pointer_pos, content_rect)
                    {
                        DragOrigin::TrimEdge { entity_bits, edge }
                    } else if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                        DragOrigin::ClipBody
                    } else {
                        DragOrigin::Empty
                    }
                } else {
                    DragOrigin::None
                };

                ui.memory_mut(|mem| {
                    mem.data
                        .insert_temp(egui::Id::new("timeline_drag_origin"), drag_origin);
                });
            }
        }

        if response.drag_stopped() {
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<DragOrigin>(egui::Id::new("timeline_drag_origin"));
            });
            return DragOrigin::None;
        }

        drag_origin
    }

    fn handle_scrubbing(
        &self,
        response: &Response,
        drag_origin: DragOrigin,
        seek_to: &mut Option<Duration>,
    ) -> bool {
        let is_moving_clip: bool = self.drag_state.is_some();
        let is_trimming: bool = self.trim_state.is_some();
        // NOTE: DragOrigin::ClipBody is intentionally excluded from scrubbing.
        // Dragging a clip body should MOVE the clip, not scrub the playhead.
        // Previously, ClipBody was included here, causing the playhead to seek
        // when the user intended to drag-move a clip (trim-to-move bug).
        let can_scrub: bool = !is_moving_clip
            && !is_trimming
            && matches!(drag_origin, DragOrigin::Ruler | DragOrigin::Empty);

        let is_scrubbing: bool = response.dragged() && can_scrub;

        if is_scrubbing {
            let drag_delta = response.drag_delta();
            if drag_delta.x.abs() > 0.1 {
                let time_change_secs: f32 = drag_delta.x.abs() / self.zoom;
                let new_time: Duration = if drag_delta.x < 0.0 {
                    self.playhead_time
                        .saturating_add(Duration::from_secs_f32(time_change_secs))
                } else {
                    self.playhead_time
                        .saturating_sub(Duration::from_secs_f32(time_change_secs))
                };
                *seek_to = Some(new_time);
            }
        }

        is_scrubbing
    }

    fn handle_right_click(
        &self,
        response: &Response,
        content_rect: Rect,
        context_menu_clip: &mut Option<(u64, egui::Pos2)>,
    ) {
        if response.secondary_clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    if let Some(entity_bits) = self.hit_test_clip(pointer_pos, content_rect) {
                        *context_menu_clip = Some((entity_bits, pointer_pos));
                    }
                }
            }
        }
    }
}
