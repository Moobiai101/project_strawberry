//! # Timeline Drag Handler
//!
//! Extracted drag interaction logic from handler.rs to maintain the 400-line limit.
//!
//! ## Contents
//!
//! - `handle_drag()` — Move and trim drag operations with committed drag origin
//! - `auto_scroll_edge()` — Auto-scroll when dragging near timeline edges
//! - `draw_ghost_preview()` — Ghost overlay rendering for move/trim feedback
//!
//! ## Design
//!
//! Uses the "committed drag origin" pattern: the entity + edge detected during
//! `track_drag_origin()` is passed into `handle_drag()`, so we never re-run
//! `hit_test_clip_edge()` during the drag. This eliminates cursor drift bugs.

use egui::{pos2, vec2, Color32, Rect, Response, Stroke, Ui};
use std::time::Duration;

use super::interaction::validation;
use super::types::{ClipDragState, DragOrigin, TrimDragState, TrimEdge};
use super::widget::TimelineWidget;

// ============================================================================
// DRAG HANDLING
// ============================================================================

impl TimelineWidget {
    /// Handle drag interactions for move and trim operations.
    ///
    /// Uses the "committed drag origin" pattern: the `drag_origin` carries
    /// the entity + edge detected during `track_drag_origin()`, so we never
    /// re-run `hit_test_clip_edge()` at drag start. This eliminates the
    /// frame-to-frame cursor drift that caused trim edges to be missed.
    #[allow(clippy::type_complexity)]
    pub(crate) fn handle_drag(
        &self,
        ui: &Ui,
        response: &Response,
        content_rect: Rect,
        painter: &egui::Painter,
        seek_to: &mut Option<Duration>,
        drag_origin: DragOrigin,
    ) -> (
        Option<(u64, Option<u64>, f64)>,
        Option<(u64, TrimEdge, f64)>,
        Option<ClipDragState>,
        Option<TrimDragState>,
    ) {
        let mut move_clip = None;
        let mut trim_clip = None;
        let mut updated_drag_state: Option<ClipDragState> = self.drag_state.clone();
        let mut updated_trim_state: Option<TrimDragState> = self.trim_state.clone();

        // Drag start — use committed drag_origin from track_drag_origin().
        // This is the key fix: we do NOT re-run hit_test_clip_edge here.
        // The entity + edge were already detected and stored in drag_origin,
        // so even if the cursor shifted by a pixel between frames, the trim
        // operation is guaranteed to start correctly.
        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    match drag_origin {
                        DragOrigin::TrimEdge { entity_bits, edge } => {
                            // Committed trim edge — use pre-detected entity + edge
                            if let Some(clip) =
                                self.clips.iter().find(|c| c.entity_bits == entity_bits)
                            {
                                tracing::info!(
                                    "Trim drag started (committed): entity={}, edge={:?}, start={:.3}s, dur={:.3}s",
                                    entity_bits,
                                    edge,
                                    clip.timeline_start.as_secs_f64(),
                                    clip.duration.as_secs_f64(),
                                );
                                updated_trim_state = Some(TrimDragState {
                                    clip_entity: entity_bits,
                                    edge,
                                    original_timeline_start: clip.timeline_start,
                                    original_duration: clip.duration,
                                    ghost_value: match edge {
                                        TrimEdge::Left => clip.timeline_start,
                                        TrimEdge::Right => clip.duration,
                                    },
                                    is_active: true,
                                });
                                updated_drag_state = None;
                            }
                        }
                        DragOrigin::ClipBody => {
                            // Clip body drag — start a move operation
                            if let Some(entity_bits) = self.hit_test_clip(pointer_pos, content_rect)
                            {
                                if let Some(clip) =
                                    self.clips.iter().find(|c| c.entity_bits == entity_bits)
                                {
                                    // Compute click offset: distance from cursor to clip start.
                                    // This prevents the clip from "jumping" to the cursor on drag.
                                    let x_in_content: f32 = pointer_pos.x - content_rect.left();
                                    let cursor_time: Duration = self.x_to_time(x_in_content);
                                    let click_offset: Duration =
                                        cursor_time.saturating_sub(clip.timeline_start);

                                    tracing::info!(
                                        "Move drag started: entity={}, start={:.3}s, cursor={:.3}s, offset={:.3}s",
                                        entity_bits,
                                        clip.timeline_start.as_secs_f64(),
                                        cursor_time.as_secs_f64(),
                                        click_offset.as_secs_f64(),
                                    );

                                    updated_drag_state = Some(ClipDragState {
                                        clip_entity: entity_bits,
                                        original_track: clip.track_index,
                                        original_time: clip.timeline_start,
                                        ghost_track: clip.track_index,
                                        ghost_time: clip.timeline_start,
                                        duration: clip.duration,
                                        is_active: true,
                                        click_offset,
                                    });
                                }
                            }
                        }
                        _ => {
                            // Ruler, Empty, None — no clip drag/trim to start
                        }
                    }
                }
            }
        }

        // Drag update
        if response.dragged() {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                // Trim drag update
                if let Some(ref mut trim) = updated_trim_state {
                    if trim.is_active && content_rect.contains(pointer_pos) {
                        let x_in_content: f32 = pointer_pos.x - content_rect.left();
                        let new_time: Duration = self.x_to_time(x_in_content);
                        match trim.edge {
                            TrimEdge::Left => {
                                // Validate: keep at least 1 frame, then snap
                                let original_end: Duration =
                                    trim.original_timeline_start + trim.original_duration;
                                let validated: Duration = validation::validate_trim_in(
                                    new_time,
                                    original_end,
                                    self.frame_rate,
                                );
                                trim.ghost_value =
                                    validation::snap_to_frame(validated, self.frame_rate);
                            }
                            TrimEdge::Right => {
                                let raw_duration: Duration =
                                    new_time.saturating_sub(trim.original_timeline_start);
                                // Validate: keep at least 1 frame, then snap
                                let validated: Duration =
                                    validation::validate_trim_out(raw_duration, self.frame_rate);
                                trim.ghost_value =
                                    validation::snap_to_frame(validated, self.frame_rate);
                            }
                        }
                        self.auto_scroll_edge(pointer_pos, content_rect, seek_to);
                    }
                }

                // Move drag update
                if let Some(ref mut drag) = updated_drag_state {
                    if drag.is_active && content_rect.contains(pointer_pos) {
                        let x_in_content: f32 = pointer_pos.x - content_rect.left();
                        let cursor_time: Duration = self.x_to_time(x_in_content);
                        // Subtract click_offset so the clip doesn't jump to cursor.
                        // This keeps the grab point stable (professional NLE behavior).
                        let raw_time: Duration = cursor_time.saturating_sub(drag.click_offset);
                        // Validate: snap to frame, clamp ≥ 0 (Duration is unsigned)
                        drag.ghost_time = validation::snap_to_frame(raw_time, self.frame_rate);

                        let y_in_content: f32 = pointer_pos.y - content_rect.top() + self.scroll_y;
                        let mut accumulated_y: f32 = 0.0;
                        let max_track_index: u32 = self.tracks.last().map(|t| t.index).unwrap_or(0);
                        for track in &self.tracks {
                            if y_in_content < accumulated_y + track.height {
                                drag.ghost_track =
                                    validation::validate_track_index(track.index, max_track_index);
                                break;
                            }
                            accumulated_y += track.height;
                        }
                    }
                    if drag.is_active {
                        self.auto_scroll_edge(pointer_pos, content_rect, seek_to);
                    }
                }
            }
        }

        // Drag complete
        if response.drag_stopped() {
            if let Some(ref trim) = updated_trim_state {
                if trim.is_active {
                    let value_changed: bool = match trim.edge {
                        TrimEdge::Left => {
                            (trim.ghost_value.as_secs_f64()
                                - trim.original_timeline_start.as_secs_f64())
                            .abs()
                                > 0.001
                        }
                        TrimEdge::Right => {
                            (trim.ghost_value.as_secs_f64() - trim.original_duration.as_secs_f64())
                                .abs()
                                > 0.001
                        }
                    };

                    tracing::info!(
                        "Trim drag stopped: edge={:?}, ghost={:.3}s, orig_start={:.3}s, orig_dur={:.3}s, changed={}",
                        trim.edge,
                        trim.ghost_value.as_secs_f64(),
                        trim.original_timeline_start.as_secs_f64(),
                        trim.original_duration.as_secs_f64(),
                        value_changed,
                    );

                    if value_changed {
                        trim_clip =
                            Some((trim.clip_entity, trim.edge, trim.ghost_value.as_secs_f64()));
                        tracing::info!(
                            "Emitting trim_clip: entity={}, edge={:?}, value={:.3}s",
                            trim.clip_entity,
                            trim.edge,
                            trim.ghost_value.as_secs_f64(),
                        );
                    }
                }
            } else {
                tracing::debug!("Drag stopped but no trim state active");
            }
            updated_trim_state = None;

            if let Some(ref drag) = updated_drag_state {
                if drag.is_active {
                    let target_track_entity: Option<u64> = self
                        .tracks
                        .iter()
                        .find(|t| t.index == drag.ghost_track)
                        .map(|t| t.entity_bits);

                    let time_changed: bool =
                        (drag.ghost_time.as_secs_f64() - drag.original_time.as_secs_f64()).abs()
                            > 0.001;
                    let track_changed: bool = drag.ghost_track != drag.original_track;

                    if time_changed || track_changed {
                        move_clip = Some((
                            drag.clip_entity,
                            target_track_entity,
                            drag.ghost_time.as_secs_f64(),
                        ));
                    }
                }
            }
            updated_drag_state = None;
        }

        // Ghost preview rendering
        self.draw_ghost_preview(
            painter,
            content_rect,
            &updated_trim_state,
            &updated_drag_state,
        );

        (move_clip, trim_clip, updated_drag_state, updated_trim_state)
    }

    /// Auto-scroll the timeline when dragging near edges.
    ///
    /// Triggers when pointer is within 120px of the left or right edge
    /// of the content area. Scrolls by 2 frames per update for smooth movement.
    pub(crate) fn auto_scroll_edge(
        &self,
        pointer_pos: egui::Pos2,
        content_rect: Rect,
        seek_to: &mut Option<Duration>,
    ) {
        let edge_threshold: f32 = 120.0;
        let scroll_speed: f32 = 2.0;
        let frame_duration: f32 = 1.0_f32 / self.frame_rate as f32;

        if pointer_pos.x < content_rect.left() + edge_threshold {
            let scroll_amount: f32 = scroll_speed * frame_duration;
            *seek_to = Some(
                self.playhead_time
                    .saturating_sub(Duration::from_secs_f32(scroll_amount)),
            );
        } else if pointer_pos.x > content_rect.right() - edge_threshold {
            let scroll_amount: f32 = scroll_speed * frame_duration;
            *seek_to = Some(
                self.playhead_time
                    .saturating_add(Duration::from_secs_f32(scroll_amount)),
            );
        }
    }

    /// Draw ghost preview overlays for active move and trim operations.
    ///
    /// Ghost rendering:
    /// - **Trim ghost**: Orange semi-transparent rect showing proposed trim result
    /// - **Move ghost**: Blue semi-transparent rect showing proposed move position
    pub(crate) fn draw_ghost_preview(
        &self,
        painter: &egui::Painter,
        content_rect: Rect,
        trim_state: &Option<TrimDragState>,
        drag_state: &Option<ClipDragState>,
    ) {
        // Trim ghost
        if let Some(ref trim) = trim_state {
            if trim.is_active {
                if let Some(clip) = self
                    .clips
                    .iter()
                    .find(|c| c.entity_bits == trim.clip_entity)
                {
                    let track_y: f32 = self.track_y(clip.track_index) + content_rect.top();
                    let track_height: f32 = self.track_height(clip.track_index);

                    let (ghost_x, ghost_width): (f32, f32) = match trim.edge {
                        TrimEdge::Left => {
                            let orig_end: Duration =
                                trim.original_timeline_start + trim.original_duration;
                            let new_start: Duration = trim.ghost_value;
                            let new_duration: Duration = orig_end.saturating_sub(new_start);
                            (
                                self.time_to_x(new_start) + content_rect.left(),
                                new_duration.as_secs_f32() * self.zoom,
                            )
                        }
                        TrimEdge::Right => {
                            let x: f32 =
                                self.time_to_x(trim.original_timeline_start) + content_rect.left();
                            let width: f32 = trim.ghost_value.as_secs_f32() * self.zoom;
                            (x, width)
                        }
                    };

                    let ghost_rect: Rect = Rect::from_min_size(
                        pos2(ghost_x, track_y),
                        vec2(ghost_width.max(4.0), track_height - 2.0),
                    );

                    painter.rect_filled(
                        ghost_rect,
                        4.0,
                        Color32::from_rgba_unmultiplied(255, 180, 100, 100),
                    );
                    painter.rect_stroke(
                        ghost_rect,
                        4.0,
                        Stroke::new(2.0, Color32::from_rgb(255, 180, 100)),
                    );
                }
            }
        }

        // Move ghost
        if let Some(ref drag) = drag_state {
            if drag.is_active {
                let ghost_x: f32 = self.time_to_x(drag.ghost_time) + content_rect.left();
                let ghost_width: f32 = drag.duration.as_secs_f32() * self.zoom;
                let ghost_y: f32 = self.track_y(drag.ghost_track) + content_rect.top();
                let ghost_height: f32 = self.track_height(drag.ghost_track);

                let ghost_rect: Rect = Rect::from_min_size(
                    pos2(ghost_x, ghost_y),
                    vec2(ghost_width, ghost_height - 2.0),
                );

                painter.rect_filled(
                    ghost_rect,
                    4.0,
                    Color32::from_rgba_unmultiplied(100, 180, 255, 100),
                );
                painter.rect_stroke(
                    ghost_rect,
                    4.0,
                    Stroke::new(2.0, Color32::from_rgb(100, 180, 255)),
                );
            }
        }
    }
}
