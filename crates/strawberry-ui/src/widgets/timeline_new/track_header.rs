//! Track header rendering — CapCut-style minimal icon column.
//!
//! ## Design
//!
//! Renders a narrow (32px) column to the left of the timeline content area.
//! Each track gets a small icon (🎬 for video, 🔊 for audio) that responds
//! to clicks for track selection or mute/solo toggles via shift-click.
//!
//! This matches CapCut's clean, uncluttered timeline aesthetic where heavy
//! track controls live in the Inspector panel, not the timeline.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Stroke, Ui};

use super::rendering::colors;
use super::types::TrackInfo;
use super::widget::TimelineWidget;

// ============================================================================
// TRACK HEADER ACTIONS
// ============================================================================

/// Action emitted when user interacts with a track header.
///
/// These are lightweight enum variants that the consuming code (docking.rs)
/// converts into appropriate Commands for undo/redo support.
#[derive(Debug, Clone)]
pub enum TrackHeaderAction {
    /// User clicked the track icon — select this track in Inspector
    SelectTrack(u64),
    /// User toggled mute (Ctrl+click or dedicated button)
    ToggleMute(u64, bool),
    /// User toggled solo (Shift+click or dedicated button)
    ToggleSolo(u64, bool),
}

// ============================================================================
// TRACK HEADER RENDERING
// ============================================================================

impl TimelineWidget {
    /// Draw CapCut-style track headers — minimal icon column.
    ///
    /// ## Layout per track
    ///
    /// ```text
    /// ┌──────┐
    /// │  🎬  │  ← 32px wide, track_height tall
    /// └──────┘
    /// ```
    ///
    /// - Single click: emits `SelectTrack` (opens track in Inspector)
    /// - Visual indicators: dimmed icon when muted, highlight when soloed
    pub(crate) fn draw_track_headers(
        &self,
        ui: &mut Ui,
        header_rect: Rect,
    ) -> Vec<TrackHeaderAction> {
        let mut actions: Vec<TrackHeaderAction> = Vec::new();
        let painter = ui.painter();

        // Background for header column
        painter.rect_filled(header_rect, 0.0, colors::BACKGROUND);

        let mut y: f32 = header_rect.top() - self.scroll_y;

        for track in &self.tracks {
            let lane_rect: Rect = Rect::from_min_size(
                pos2(header_rect.left(), y),
                vec2(header_rect.width(), track.height),
            );

            // Skip if not visible
            if lane_rect.bottom() < header_rect.top() || lane_rect.top() > header_rect.bottom() {
                y += track.height;
                continue;
            }

            // Track header background — subtle difference from track lane
            let header_bg: Color32 = if track.index % 2 == 0 {
                Color32::from_rgb(28, 28, 35)
            } else {
                Color32::from_rgb(32, 32, 40)
            };
            painter.rect_filled(lane_rect, 0.0, header_bg);

            // Icon selection
            let icon: &str = if track.is_video { "🎬" } else { "🔊" };

            // Visual state: dim when muted, highlight when soloed
            let icon_color: Color32 = if track.muted {
                Color32::from_rgb(80, 80, 90) // Dimmed
            } else if track.solo {
                Color32::from_rgb(255, 200, 50) // Gold highlight for solo
            } else {
                Color32::from_rgb(160, 160, 175) // Normal
            };

            // Draw icon centered in the lane
            let icon_center = lane_rect.center();
            painter.text(
                icon_center,
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(14.0),
                icon_color,
            );

            // Solo indicator: tiny "S" badge in top-right corner
            if track.solo {
                let badge_pos = pos2(lane_rect.right() - 6.0, lane_rect.top() + 6.0);
                painter.circle_filled(badge_pos, 5.0, Color32::from_rgb(255, 200, 50));
                painter.text(
                    badge_pos,
                    Align2::CENTER_CENTER,
                    "S",
                    FontId::proportional(7.0),
                    Color32::BLACK,
                );
            }

            // Mute indicator: tiny strikethrough line over icon
            if track.muted {
                painter.line_segment(
                    [
                        pos2(icon_center.x - 6.0, icon_center.y - 6.0),
                        pos2(icon_center.x + 6.0, icon_center.y + 6.0),
                    ],
                    Stroke::new(2.0, Color32::from_rgb(255, 80, 80)),
                );
            }

            // Separator line at bottom of each track header
            painter.line_segment(
                [
                    pos2(header_rect.left(), y + track.height - 1.0),
                    pos2(header_rect.right(), y + track.height - 1.0),
                ],
                Stroke::new(1.0, colors::TRACK_SEPARATOR),
            );

            // Hit testing for click interactions
            if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                if lane_rect.contains(pointer_pos) {
                    // Hover highlight
                    painter.rect_filled(
                        lane_rect,
                        0.0,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    );

                    if ui.input(|i| i.pointer.any_click()) {
                        let modifiers = ui.input(|i| i.modifiers);

                        if modifiers.ctrl {
                            // Ctrl+click → toggle mute
                            // Emit CURRENT state; docking.rs inverts it
                            actions.push(TrackHeaderAction::ToggleMute(
                                track.entity_bits,
                                track.muted,
                            ));
                        } else if modifiers.shift {
                            // Shift+click → toggle solo
                            // Emit CURRENT state; docking.rs inverts it
                            actions.push(TrackHeaderAction::ToggleSolo(
                                track.entity_bits,
                                track.solo,
                            ));
                        } else {
                            // Plain click → select track for Inspector
                            actions.push(TrackHeaderAction::SelectTrack(track.entity_bits));
                        }
                    }
                }
            }

            y += track.height;
        }

        // Right border separator between headers and content
        painter.line_segment(
            [
                pos2(header_rect.right() - 1.0, header_rect.top()),
                pos2(header_rect.right() - 1.0, header_rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgb(50, 50, 60)),
        );

        actions
    }
}
