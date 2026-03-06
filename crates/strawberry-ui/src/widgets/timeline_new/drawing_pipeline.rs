//! # Timeline Drawing Pipeline
//!
//! Extracted drawing helpers from handler.rs to maintain the 400-line limit.
//!
//! ## Contents
//!
//! - `draw_capcut_empty_state()` — CapCut-style empty timeline with drop pill
//! - `draw_bottom_ruler_ticks()` — Time tick marks for bottom ruler in empty state
//!
//! These are purely visual functions with no state mutation or interaction logic.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Rounding, Stroke};

use super::widget::TimelineWidget;

// ============================================================================
// DRAWING PIPELINE — EMPTY STATE
// ============================================================================

impl TimelineWidget {
    /// CapCut-style empty state: clean dark area with ruler at bottom and centered drop pill
    ///
    /// Renders a full-height dark timeline area with:
    /// - Bottom ruler with time ticks
    /// - Centered "Drag and drop media here" pill
    /// - Folder icon decoration
    pub(crate) fn draw_capcut_empty_state(&self, painter: &egui::Painter, rect: Rect) {
        // Dark background fills entire timeline area
        painter.rect_filled(rect, 0.0, Color32::from_rgb(28, 30, 35));

        // Ruler at BOTTOM (CapCut style) - 30px height
        let ruler_height = 30.0;
        let ruler_rect = Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - ruler_height),
            vec2(rect.width(), ruler_height),
        );

        // Ruler background
        painter.rect_filled(ruler_rect, 0.0, Color32::from_rgb(22, 24, 28));

        // Draw time ticks on bottom ruler
        self.draw_bottom_ruler_ticks(painter, ruler_rect);

        // Content area above ruler (where the drop pill goes)
        let content_rect = Rect::from_min_max(rect.min, pos2(rect.right(), ruler_rect.top()));

        // CapCut-style drop pill - full width with padding
        let pill_margin_x = 20.0;
        let pill_height = 48.0;
        let pill_rect = Rect::from_center_size(
            content_rect.center(),
            vec2(content_rect.width() - pill_margin_x * 2.0, pill_height),
        );

        // Pill background - subtle dark with border
        painter.rect_filled(
            pill_rect,
            Rounding::same(6.0),
            Color32::from_rgb(38, 40, 48),
        );
        painter.rect_stroke(
            pill_rect,
            Rounding::same(6.0),
            Stroke::new(1.0, Color32::from_rgb(55, 58, 68)),
        );

        // Media icon (folder icon style)
        let icon_x = pill_rect.left() + 20.0;
        let icon_y = pill_rect.center().y;

        // Draw folder icon using simple shapes
        let icon_rect = Rect::from_center_size(pos2(icon_x, icon_y), vec2(20.0, 16.0));
        painter.rect_filled(icon_rect, 2.0, Color32::from_rgb(80, 85, 95));
        // Folder tab
        let tab_rect = Rect::from_min_size(
            pos2(icon_rect.left(), icon_rect.top() - 3.0),
            vec2(8.0, 5.0),
        );
        painter.rect_filled(tab_rect, Rounding::same(1.0), Color32::from_rgb(80, 85, 95));

        // Centered text
        painter.text(
            pos2(pill_rect.center().x, pill_rect.center().y),
            Align2::CENTER_CENTER,
            "Drag and drop media here",
            FontId::proportional(14.0),
            Color32::from_rgb(120, 125, 135),
        );
    }

    /// Draw time ticks for bottom ruler (empty state only)
    ///
    /// Calculates tick intervals based on visible time range at current zoom
    /// and renders major/minor ticks with MM:SS labels at major positions.
    pub(crate) fn draw_bottom_ruler_ticks(&self, painter: &egui::Painter, rect: Rect) {
        // Calculate visible time range
        let seconds_visible: f32 = rect.width() / self.zoom.max(1.0);
        let tick_interval: i64 = if seconds_visible < 10.0 {
            5
        } else if seconds_visible < 60.0 {
            15
        } else {
            30
        };

        // Draw ticks every tick_interval seconds
        let start_time: i64 = 0;
        let end_time: i64 = ((rect.width() / self.zoom.max(1.0)) as i64).max(180); // At least 3 minutes

        for t in (start_time..end_time).step_by(tick_interval as usize) {
            let x: f32 = rect.left() + (t as f32 * self.zoom);
            if x < rect.left() || x > rect.right() {
                continue;
            }

            let is_major: bool = t % (tick_interval * 2) == 0;
            let tick_height: f32 = if is_major { 8.0 } else { 4.0 };

            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.top() + tick_height)],
                Stroke::new(1.0, Color32::from_rgb(55, 58, 65)),
            );

            if is_major {
                let mins: i64 = t / 60;
                let secs: i64 = t % 60;
                let label: String = format!("{:02}:{:02}", mins, secs);
                painter.text(
                    pos2(x + 4.0, rect.center().y + 2.0),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(10.0),
                    Color32::from_rgb(85, 90, 100),
                );
            }
        }
    }
}
