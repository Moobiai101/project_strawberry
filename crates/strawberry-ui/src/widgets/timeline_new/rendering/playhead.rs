//! Playhead rendering.
//!
//! Contains functions for drawing the playhead indicator.

use egui::{pos2, Painter, Rect, Shape, Stroke};
use std::time::Duration;

use super::colors;

// ============================================================================
// PLAYHEAD CONSTANTS
// ============================================================================

/// Playhead line width in pixels
pub const PLAYHEAD_WIDTH: f32 = 2.0;

/// Playhead head (triangle) size - larger for easy grabbing (CapCut style)
pub const PLAYHEAD_HEAD_SIZE: f32 = 14.0;

/// Playhead grab zone width (pixels) - area around playhead that triggers drag
pub const PLAYHEAD_GRAB_ZONE: f32 = 10.0;

// ============================================================================
// PLAYHEAD DRAWING
// ============================================================================

/// Draw the playhead.
///
/// In centered_playhead mode: playhead is at center after scrolling past threshold.
/// At the beginning (time < center_time), playhead draws at actual time position.
///
/// # Parameters
/// - `painter`: The egui painter
/// - `ruler_rect`: Rectangle of the ruler area
/// - `content_rect`: Rectangle of the content area
/// - `playhead_time`: Current playhead time
/// - `zoom`: Pixels per second
/// - `scroll_x`: Current horizontal scroll in pixels
/// - `centered_playhead`: Whether to use centered playhead mode
pub fn draw_playhead(
    painter: &Painter,
    ruler_rect: Rect,
    content_rect: Rect,
    playhead_time: Duration,
    zoom: f32,
    scroll_x: f32,
    centered_playhead: bool,
) {
    // Calculate when playhead should be at center vs at actual position
    let center_time: f32 = (content_rect.width() / 2.0) / zoom;
    let should_center: bool = centered_playhead && playhead_time.as_secs_f32() >= center_time;

    // Calculate playhead x position
    let x: f32 = if should_center {
        // CapCut style: playhead at center (after scrolling past threshold)
        content_rect.center().x
    } else {
        // At start or normal mode: playhead at time position
        content_rect.left() + time_to_x(playhead_time, zoom, scroll_x)
    };

    // Skip if not visible
    if x < content_rect.left() || x > content_rect.right() {
        return;
    }

    // Draw shadow
    painter.line_segment(
        [
            pos2(x + 1.0, ruler_rect.top()),
            pos2(x + 1.0, content_rect.bottom()),
        ],
        Stroke::new(3.0, colors::playhead_shadow()),
    );

    // Draw playhead line
    painter.line_segment(
        [pos2(x, ruler_rect.top()), pos2(x, content_rect.bottom())],
        Stroke::new(PLAYHEAD_WIDTH, colors::PLAYHEAD),
    );

    // Draw playhead head (triangle)
    let head_center = pos2(x, ruler_rect.bottom());
    let half_size: f32 = PLAYHEAD_HEAD_SIZE / 2.0;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(
                head_center.x - half_size,
                ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE,
            ),
            pos2(
                head_center.x + half_size,
                ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE,
            ),
            pos2(head_center.x, ruler_rect.bottom()),
        ],
        colors::PLAYHEAD,
        Stroke::NONE,
    ));
}

// ============================================================================
// COORDINATE CONVERSION
// ============================================================================

/// Convert time to X coordinate (relative to content area left edge).
pub fn time_to_x(time: Duration, zoom: f32, scroll_x: f32) -> f32 {
    time.as_secs_f32() * zoom - scroll_x
}

/// Convert X coordinate to time.
pub fn x_to_time(x_relative: f32, zoom: f32, scroll_x: f32) -> Duration {
    let seconds: f32 = (x_relative + scroll_x) / zoom;
    Duration::from_secs_f32(seconds.max(0.0))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_to_x() {
        // At zoom 100 (100 px/sec), scroll 0
        assert_eq!(time_to_x(Duration::from_secs(1), 100.0, 0.0), 100.0);
        assert_eq!(time_to_x(Duration::from_secs(2), 100.0, 0.0), 200.0);

        // With scroll
        assert_eq!(time_to_x(Duration::from_secs(1), 100.0, 50.0), 50.0);
    }

    #[test]
    fn test_x_to_time() {
        // At zoom 100 (100 px/sec), scroll 0
        let time = x_to_time(100.0, 100.0, 0.0);
        assert_eq!(time.as_secs(), 1);

        // With scroll
        let time = x_to_time(50.0, 100.0, 50.0);
        assert_eq!(time.as_secs(), 1);
    }
}
