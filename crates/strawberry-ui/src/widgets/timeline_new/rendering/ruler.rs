//! Timeline ruler rendering.
//!
//! Contains functions for drawing the timecode ruler at the top of the timeline.

use egui::{pos2, Align2, FontId, Painter, Rect, Stroke};
use std::time::Duration;

use super::colors;

// ============================================================================
// RULER DRAWING
// ============================================================================

/// Draw the timeline ruler (timecode display).
///
/// Renders major and minor tick marks with time labels.
///
/// # Parameters
/// - `painter`: The egui painter to draw with
/// - `rect`: The rectangle to draw in
/// - `scroll_x`: Current horizontal scroll offset in pixels
/// - `zoom`: Pixels per second
/// - `frame_rate`: Frame rate for timecode display
pub fn draw_ruler(painter: &Painter, rect: Rect, scroll_x: f32, zoom: f32, frame_rate: f64) {
    // Background
    painter.rect_filled(rect, 0.0, colors::RULER_BG);

    // Calculate tick spacing based on zoom
    let seconds_visible: f32 = rect.width() / zoom;
    let tick_interval: i64 = calculate_tick_interval(seconds_visible);
    let minor_ticks: i32 = 5;

    // Draw ticks
    let start_time: i64 = (scroll_x / zoom).floor() as i64;
    let end_time: i64 = ((scroll_x + rect.width()) / zoom).ceil() as i64;

    for t in (start_time..=end_time).step_by(tick_interval.max(1) as usize) {
        let x: f32 = rect.left() + (t as f32 - scroll_x / zoom) * zoom;
        if x < rect.left() || x > rect.right() {
            continue;
        }

        // Major tick
        painter.line_segment(
            [pos2(x, rect.bottom() - 10.0), pos2(x, rect.bottom())],
            Stroke::new(1.0, colors::RULER_MAJOR_LINE),
        );

        // Time label
        let label: String = format_timecode(Duration::from_secs(t as u64), frame_rate);
        painter.text(
            pos2(x + 4.0, rect.top() + 4.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(10.0),
            colors::RULER_TEXT,
        );

        // Minor ticks
        for m in 1..minor_ticks {
            let minor_x: f32 = x + (m as f32 / minor_ticks as f32) * tick_interval as f32 * zoom;
            if minor_x >= rect.left() && minor_x <= rect.right() {
                painter.line_segment(
                    [
                        pos2(minor_x, rect.bottom() - 5.0),
                        pos2(minor_x, rect.bottom()),
                    ],
                    Stroke::new(1.0, colors::RULER_MINOR_LINE),
                );
            }
        }
    }
}

// ============================================================================
// TICK INTERVAL CALCULATION
// ============================================================================

/// Calculate appropriate tick interval based on visible time range.
///
/// Returns interval in seconds between major ticks.
pub fn calculate_tick_interval(seconds_visible: f32) -> i64 {
    if seconds_visible > 3600.0 {
        600 // 10 minutes
    } else if seconds_visible > 600.0 {
        60 // 1 minute
    } else if seconds_visible > 120.0 {
        10
    } else if seconds_visible > 30.0 {
        5
    } else if seconds_visible > 10.0 {
        1
    } else {
        1
    }
}

// ============================================================================
// TIMECODE FORMATTING
// ============================================================================

/// Format a duration as timecode for display.
///
/// Returns HH:MM:SS or MM:SS format depending on duration.
pub fn format_timecode(time: Duration, _frame_rate: f64) -> String {
    let total_secs: u64 = time.as_secs();
    let hours: u64 = total_secs / 3600;
    let minutes: u64 = (total_secs % 3600) / 60;
    let seconds: u64 = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_interval_calculation() {
        assert_eq!(calculate_tick_interval(4000.0), 600);
        assert_eq!(calculate_tick_interval(700.0), 60);
        assert_eq!(calculate_tick_interval(150.0), 10);
        assert_eq!(calculate_tick_interval(50.0), 5);
        assert_eq!(calculate_tick_interval(15.0), 1);
    }

    #[test]
    fn test_format_timecode() {
        assert_eq!(format_timecode(Duration::from_secs(0), 30.0), "00:00");
        assert_eq!(format_timecode(Duration::from_secs(65), 30.0), "01:05");
        assert_eq!(format_timecode(Duration::from_secs(3665), 30.0), "01:01:05");
    }
}
