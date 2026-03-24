//! Timeline drawing methods.
//!
//! Contains draw_* methods that render timeline components.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Rounding, Stroke, Ui};
use std::time::Duration;

use super::super::clip_renderer::{BatchedClipRenderer, VisibleClipRect};
use super::rendering::colors;
use super::widget::{
    TimelineWidget, CLIP_GAP, PLAYHEAD_HEAD_SIZE, PLAYHEAD_WIDTH, TRANSITION_BUTTON_SIZE,
};

// ============================================================================
// DRAWING METHOD IMPLEMENTATIONS
// ============================================================================

impl TimelineWidget {
    /// Draw the timeline ruler (timecode display)
    pub(crate) fn draw_ruler(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();

        // Background
        painter.rect_filled(rect, 0.0, colors::BACKGROUND);

        // Calculate visible time range
        let seconds_visible = rect.width() / self.zoom;
        let tick_interval = Self::calculate_tick_interval(seconds_visible);

        // Find first visible tick
        let start_time = (self.scroll_x / self.zoom).floor() as i64;
        let end_time = ((self.scroll_x + rect.width()) / self.zoom).ceil() as i64 + 1;

        // Draw ticks
        for t in (start_time..end_time).step_by(tick_interval as usize) {
            if t < 0 {
                continue;
            }

            let x = rect.left() + (t as f32 * self.zoom) - self.scroll_x;
            if x < rect.left() || x > rect.right() {
                continue;
            }

            let is_major = t % (tick_interval * 5) == 0;
            let tick_height = if is_major { 12.0 } else { 6.0 };

            painter.line_segment(
                [pos2(x, rect.bottom() - tick_height), pos2(x, rect.bottom())],
                Stroke::new(1.0, colors::TIMELINE_TICK),
            );

            if is_major {
                let label = Self::format_timecode(Duration::from_secs(t as u64), self.frame_rate);
                painter.text(
                    pos2(x + 3.0, rect.top() + 4.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(10.0),
                    colors::TEXT_SECONDARY,
                );
            }
        }
    }

    /// Calculate appropriate tick interval based on visible time
    fn calculate_tick_interval(seconds_visible: f32) -> i64 {
        if seconds_visible < 5.0 {
            1
        } else if seconds_visible < 30.0 {
            5
        } else if seconds_visible < 120.0 {
            15
        } else if seconds_visible < 300.0 {
            30
        } else {
            60
        }
    }

    /// Format a duration as timecode (HH:MM:SS or MM:SS)
    fn format_timecode(duration: Duration, _frame_rate: f64) -> String {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }

    /// Draw track lanes (background grid) with labels
    pub(crate) fn draw_track_lanes(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();

        // Background
        painter.rect_filled(rect, 0.0, colors::BACKGROUND);

        // Draw each track lane
        let mut y = rect.top() - self.scroll_y;
        for track in &self.tracks {
            let lane_rect =
                Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), track.height));

            // Skip if not visible
            if lane_rect.bottom() < rect.top() || lane_rect.top() > rect.bottom() {
                y += track.height;
                continue;
            }

            // Alternate track colors
            let bg_color = if track.index % 2 == 0 {
                colors::TRACK_BG
            } else {
                colors::TRACK_ALT
            };
            painter.rect_filled(lane_rect, 0.0, bg_color);

            // Track separator line
            painter.line_segment(
                [
                    pos2(rect.left(), y + track.height - 1.0),
                    pos2(rect.right(), y + track.height - 1.0),
                ],
                Stroke::new(1.0, colors::TRACK_SEPARATOR),
            );

            y += track.height;
        }
    }

    /// Draw clips on the timeline using batched rendering for performance.
    ///
    /// Uses BatchedClipRenderer to reduce draw calls from O(n*7) to O(1) per layer,
    /// enabling smooth 60fps rendering with 1000+ clips.
    pub(crate) fn draw_clips(
        &self,
        ui: &mut Ui,
        rect: Rect,
        hovered_clip: Option<u64>,
        waveform_cache: Option<&strawberry_core::components::waveform::WaveformCache>,
    ) {
        let painter = ui.painter();

        // First pass: collect all visible clips into VisibleClipRect structs
        let mut renderer = BatchedClipRenderer::new();

        for clip in &self.clips {
            let x = rect.left() + self.time_to_x(clip.timeline_start);
            let width = clip.duration.as_secs_f32() * self.zoom;
            let y = rect.top() + self.track_y(clip.track_index);
            let height = self.track_height(clip.track_index) - 4.0;

            // Viewport culling - skip non-visible clips
            if x + width < rect.left() || x > rect.right() {
                continue;
            }

            // Visual gap for transition button area
            let visual_gap = CLIP_GAP / 2.0;
            let clip_rect = Rect::from_min_size(
                pos2(x + visual_gap, y + 2.0),
                vec2(width - CLIP_GAP, height),
            );
            let is_hovered = hovered_clip == Some(clip.entity_bits);

            // Determine fill color
            let base_fill_color = if let Some(c) = clip.color {
                Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
            } else {
                let is_video = self
                    .tracks
                    .iter()
                    .find(|t| t.index == clip.track_index)
                    .map(|t| t.is_video)
                    .unwrap_or(true);
                if is_video {
                    colors::CLIP_VIDEO
                } else {
                    colors::CLIP_AUDIO
                }
            };

            // Apply hover highlight only — selection uses border, not fill change
            let fill_color = if is_hovered {
                base_fill_color.gamma_multiply(1.2)
            } else {
                base_fill_color
            };

            // Determine border color and width based on state
            let (border_color, border_width) = if clip.selected {
                (colors::CLIP_SELECTED_BORDER, 2.0)
            } else if is_hovered {
                (colors::ACCENT_CYAN, 1.5)
            } else {
                (colors::CLIP_BORDER, 1.0)
            };

            // Check for filmstrip textures
            let has_filmstrip = clip
                .asset_uuid
                .and_then(|uuid| self.filmstrip_textures.get(&uuid))
                .map(|textures| !textures.lod0.is_empty() || !textures.lod1.is_empty())
                .unwrap_or(false);

            // Add to batched renderer
            renderer.add_clip(VisibleClipRect {
                rect: clip_rect,
                fill_color,
                border_color,
                border_width,
                draw_shadow: clip.selected || is_hovered,
                name: clip.name.clone(),
                entity_bits: clip.entity_bits,
                track_height: height,
                has_filmstrip,
                asset_uuid: clip.asset_uuid,
                source_in: clip.source_in,
                source_out: clip.source_out,
                is_selected: clip.selected,
                audio_muted: clip.audio_muted,
            });
        }

        // Second pass: render all layers in batched fashion
        // Order matters: back to front
        renderer.render_shadows(&painter);
        renderer.render_backgrounds(&painter);

        // Filmstrips still per-clip (texture-based, already optimized)
        self.draw_filmstrips(&painter, renderer.clips());

        // Waveform peaks overlay (after filmstrips, before selection)
        if let Some(cache) = waveform_cache {
            renderer.render_waveforms(&painter, cache);
        }

        // Selection overlays: cyan glow + white tint for selected clips.
        // Rendered AFTER filmstrips so the overlay sits on top of thumbnails,
        // and BEFORE borders so the glow appears under the border stroke.
        renderer.render_selection_overlays(&painter);

        renderer.render_borders(&painter);
        renderer.render_handles(&painter);
        renderer.render_text(&painter, colors::CLIP_TEXT);
        renderer.render_audio_icons(&painter);
    }

    /// Draw filmstrip thumbnails for visible clips (texture-based, separate pass)
    fn draw_filmstrips(&self, painter: &egui::Painter, visible_clips: &[VisibleClipRect]) {
        for clip in visible_clips {
            if let Some(asset_uuid) = clip.asset_uuid {
                if let Some(asset_textures) = self.filmstrip_textures.get(&asset_uuid) {
                    let lod_level: u8 =
                        super::super::filmstrip::get_lod_for_pixels_per_second(self.zoom as f64);
                    let thumbnails_slice = if lod_level == 0 {
                        &asset_textures.lod0
                    } else {
                        &asset_textures.lod1
                    };

                    if !thumbnails_slice.is_empty() {
                        tracing::trace!(
                            "Filmstrip draw: entity={}, lod={}, source_in={:.3}s, source_out={:.3}s, thumbs={}, interval={:.1}s",
                            clip.entity_bits,
                            lod_level,
                            clip.source_in.as_secs_f64(),
                            clip.source_out.as_secs_f64(),
                            thumbnails_slice.len(),
                            if lod_level == 0 { asset_textures.lod0_interval } else { asset_textures.lod1_interval },
                        );
                        let filmstrip_thumbs = super::super::filmstrip::FilmstripThumbnails {
                            thumbnails: thumbnails_slice,
                            thumb_width: asset_textures.thumb_width,
                            thumb_height: asset_textures.thumb_height,
                            interval_seconds: if lod_level == 0 {
                                asset_textures.lod0_interval
                            } else {
                                asset_textures.lod1_interval
                            },
                        };

                        let filmstrip_rect = clip.rect.shrink2(vec2(2.0, 2.0));
                        let renderer = super::super::filmstrip::FilmstripRenderer::new();
                        renderer.draw(
                            &painter,
                            filmstrip_rect,
                            &filmstrip_thumbs,
                            clip.source_in,
                            clip.source_out,
                            self.zoom as f64,
                            None, // Clipping handled by batched renderer
                        );
                    }
                }
            }
        }
    }

    /// Draw transition zones between adjacent clips
    pub(crate) fn draw_transition_zones(
        &self,
        ui: &mut Ui,
        rect: Rect,
        hovered_gap: Option<(u64, u64)>,
    ) {
        let painter = ui.painter();

        for track in &self.tracks {
            let mut track_clips: Vec<_> = self
                .clips
                .iter()
                .filter(|c| c.track_index == track.index)
                .collect();
            track_clips.sort_by(|a, b| a.timeline_start.cmp(&b.timeline_start));

            for window in track_clips.windows(2) {
                let clip_a = window[0];
                let clip_b = window[1];

                let a_end_x = rect.left()
                    + self.time_to_x(clip_a.timeline_start)
                    + clip_a.duration.as_secs_f32() * self.zoom;
                let b_start_x = rect.left() + self.time_to_x(clip_b.timeline_start);

                let gap_width = b_start_x - a_end_x;
                if gap_width > -10.0 && gap_width < 30.0 {
                    let track_y = rect.top() + self.track_y(track.index);
                    let th = self.track_height(track.index);

                    let gap_center_x = (a_end_x + b_start_x) / 2.0;
                    let gap_center_y = track_y + th / 2.0;

                    let is_hovered = hovered_gap == Some((clip_a.entity_bits, clip_b.entity_bits));

                    if is_hovered {
                        let button_rect = Rect::from_center_size(
                            pos2(gap_center_x, gap_center_y),
                            vec2(TRANSITION_BUTTON_SIZE, TRANSITION_BUTTON_SIZE),
                        );

                        painter.rect_filled(
                            button_rect,
                            Rounding::same(TRANSITION_BUTTON_SIZE / 2.0),
                            colors::TRANSITION_PLUS_BG,
                        );

                        let plus_size = 8.0;
                        let center = button_rect.center();
                        painter.line_segment(
                            [
                                pos2(center.x - plus_size / 2.0, center.y),
                                pos2(center.x + plus_size / 2.0, center.y),
                            ],
                            Stroke::new(2.0, Color32::WHITE),
                        );
                        painter.line_segment(
                            [
                                pos2(center.x, center.y - plus_size / 2.0),
                                pos2(center.x, center.y + plus_size / 2.0),
                            ],
                            Stroke::new(2.0, Color32::WHITE),
                        );
                    }
                }
            }
        }
    }

    /// Draw the playhead
    pub(crate) fn draw_playhead(&self, ui: &mut Ui, ruler_rect: Rect, content_rect: Rect) {
        let painter = ui.painter();

        let center_time = (content_rect.width() / 2.0) / self.zoom;
        let should_center =
            self.centered_playhead && self.playhead_time.as_secs_f32() >= center_time;

        let x = if should_center {
            content_rect.center().x
        } else {
            content_rect.left() + self.time_to_x(self.playhead_time)
        };

        if x < content_rect.left() || x > content_rect.right() {
            return;
        }

        // Shadow
        painter.line_segment(
            [
                pos2(x + 1.0, ruler_rect.top()),
                pos2(x + 1.0, content_rect.bottom()),
            ],
            Stroke::new(3.0, colors::playhead_shadow()),
        );

        // Line
        painter.line_segment(
            [pos2(x, ruler_rect.top()), pos2(x, content_rect.bottom())],
            Stroke::new(PLAYHEAD_WIDTH, colors::PLAYHEAD),
        );

        // Head (triangle)
        let half_size = PLAYHEAD_HEAD_SIZE / 2.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(x - half_size, ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE),
                pos2(x + half_size, ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE),
                pos2(x, ruler_rect.bottom()),
            ],
            colors::PLAYHEAD,
            Stroke::NONE,
        ));
    }

    // =========================================================================
    // HIT TESTING (kept here since they use self state)
    // =========================================================================

    /// Hit test to find which clip (if any) is at the given position
    pub(crate) fn hit_test_clip(&self, pos: egui::Pos2, content_rect: Rect) -> Option<u64> {
        for clip in &self.clips {
            let clip_x = self.time_to_x(clip.timeline_start) + content_rect.left();
            let clip_width = clip.duration.as_secs_f32() * self.zoom;
            let track_y = self.track_y(clip.track_index) + content_rect.top();
            let th = self.track_height(clip.track_index);

            let clip_rect = Rect::from_min_size(pos2(clip_x, track_y), vec2(clip_width, th - 2.0));

            if clip_rect.contains(pos) {
                return Some(clip.entity_bits);
            }
        }
        None
    }

    /// Hit test for clip trim edges
    pub(crate) fn hit_test_clip_edge(
        &self,
        pos: egui::Pos2,
        content_rect: Rect,
    ) -> Option<(u64, super::types::TrimEdge)> {
        // 16px trim handle width — wider than the 12px default for easier targeting.
        // Professional NLEs use 14-20px edge zones to prevent missed trim attempts.
        const TRIM_HANDLE_WIDTH: f32 = 16.0;

        for clip in &self.clips {
            let clip_x = self.time_to_x(clip.timeline_start) + content_rect.left();
            let clip_width = clip.duration.as_secs_f32() * self.zoom;
            let track_y = self.track_y(clip.track_index) + content_rect.top();
            let th = self.track_height(clip.track_index);

            if pos.y < track_y || pos.y > track_y + th {
                continue;
            }

            if clip_width <= TRIM_HANDLE_WIDTH * 2.0 {
                continue;
            }

            let left_edge_min = clip_x;
            let left_edge_max = clip_x + TRIM_HANDLE_WIDTH;
            let right_edge_min = clip_x + clip_width - TRIM_HANDLE_WIDTH;
            let right_edge_max = clip_x + clip_width;

            if pos.x >= left_edge_min && pos.x <= left_edge_max {
                return Some((clip.entity_bits, super::types::TrimEdge::Left));
            }
            if pos.x >= right_edge_min && pos.x <= right_edge_max {
                return Some((clip.entity_bits, super::types::TrimEdge::Right));
            }
        }
        None
    }

    /// Hit test for gap between clips
    pub(crate) fn hit_test_gap(&self, pos: egui::Pos2, content_rect: Rect) -> Option<(u64, u64)> {
        for track in &self.tracks {
            let track_y = self.track_y(track.index) + content_rect.top();
            let th = self.track_height(track.index);

            if pos.y < track_y || pos.y > track_y + th {
                continue;
            }

            let mut track_clips: Vec<_> = self
                .clips
                .iter()
                .filter(|c| c.track_index == track.index)
                .collect();
            track_clips.sort_by(|a, b| a.timeline_start.cmp(&b.timeline_start));

            for window in track_clips.windows(2) {
                let clip_a = window[0];
                let clip_b = window[1];

                let a_end_x = content_rect.left()
                    + self.time_to_x(clip_a.timeline_start)
                    + clip_a.duration.as_secs_f32() * self.zoom;
                let b_start_x = content_rect.left() + self.time_to_x(clip_b.timeline_start);

                let gap_left = a_end_x - 10.0;
                let gap_right = b_start_x + 10.0;

                if pos.x >= gap_left && pos.x <= gap_right {
                    return Some((clip_a.entity_bits, clip_b.entity_bits));
                }
            }
        }
        None
    }

    /// Hit test for audio icon click on a clip.
    ///
    /// Returns `Some((entity_bits, current_muted_state))` if the click position
    /// falls within the 16×16px audio icon zone at bottoom-right of a clip.
    /// Called BEFORE `hit_test_clip()` so icon clicks don't trigger selection.
    ///
    /// ## Performance
    /// Only checks visible clips (same iteration as other hit tests).
    pub(crate) fn hit_test_audio_icon(pos: egui::Pos2, content_rect: Rect, clips: &[super::types::ClipInfo], zoom: f32, scroll_x: f32, tracks: &[super::types::TrackInfo]) -> Option<(u64, bool)> {
        use super::rendering::{time_to_x, track_height as get_track_height, track_y_position};

        for clip in clips {
            let clip_x: f32 = time_to_x(clip.timeline_start, zoom, scroll_x) + content_rect.left();
            let clip_width: f32 = clip.duration.as_secs_f32() * zoom;

            // Skip narrow clips (same threshold as render_audio_icons)
            if clip_width <= 60.0 {
                continue;
            }

            let track_y: f32 = track_y_position(tracks, clip.track_index) + content_rect.top();
            let th: f32 = get_track_height(tracks, clip.track_index) - 4.0;

            // Icon rect: 22x16 at BOTTOM-LEFT of clip (matches render_audio_icons)
            let icon_rect = Rect::from_min_size(
                egui::pos2(clip_x + 5.0, track_y + th - 19.0),
                egui::vec2(22.0, 16.0),
            );

            if icon_rect.contains(pos) {
                return Some((clip.entity_bits, clip.audio_muted));
            }
        }
        None
    }
}
