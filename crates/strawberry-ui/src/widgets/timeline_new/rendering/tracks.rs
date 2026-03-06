//! Track lane rendering.
//!
//! Contains functions for drawing track lane backgrounds and dividers.

use egui::{pos2, vec2, Painter, Rect, Stroke};

use super::colors;
use crate::widgets::timeline_new::types::TrackInfo;

// ============================================================================
// TRACK LANE DRAWING
// ============================================================================

/// Draw track lanes (background grid).
///
/// Renders alternating background colors for each track with divider lines.
///
/// # Parameters
/// - `painter`: The egui painter to draw with
/// - `rect`: The rectangle to draw in
/// - `tracks`: Track information for height calculation
/// - `scroll_y`: Current vertical scroll offset in pixels
pub fn draw_track_lanes(painter: &Painter, rect: Rect, tracks: &[TrackInfo], scroll_y: f32) {
    // Fill background
    painter.rect_filled(rect, 0.0, colors::BACKGROUND);

    let mut y: f32 = rect.top() - scroll_y;
    for (i, track) in tracks.iter().enumerate() {
        let lane_rect: Rect =
            Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), track.height));

        // Skip if not visible
        if lane_rect.bottom() < rect.top() || lane_rect.top() > rect.bottom() {
            y += track.height;
            continue;
        }

        // Alternating background
        let bg = if i % 2 == 0 {
            colors::TRACK_LANE_BG
        } else {
            colors::TRACK_LANE_ALT
        };
        painter.rect_filled(lane_rect, 0.0, bg);

        // Divider line
        painter.line_segment(
            [
                pos2(rect.left(), y + track.height),
                pos2(rect.right(), y + track.height),
            ],
            Stroke::new(1.0, colors::TRACK_DIVIDER),
        );

        y += track.height;
    }
}

/// Calculate total height of all tracks.
pub fn calculate_total_track_height(tracks: &[TrackInfo]) -> f32 {
    tracks.iter().map(|t| t.height).sum()
}

/// Get Y position for a specific track index.
pub fn track_y_position(tracks: &[TrackInfo], track_index: u32) -> f32 {
    tracks
        .iter()
        .take(track_index as usize)
        .map(|t| t.height)
        .sum()
}

/// Get height of a specific track.
pub fn track_height(tracks: &[TrackInfo], track_index: u32) -> f32 {
    tracks
        .get(track_index as usize)
        .map(|t| t.height)
        .unwrap_or(35.0)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_total_track_height() {
        let tracks = vec![
            TrackInfo {
                height: 35.0,
                ..Default::default()
            },
            TrackInfo {
                height: 40.0,
                ..Default::default()
            },
            TrackInfo {
                height: 35.0,
                ..Default::default()
            },
        ];
        assert_eq!(calculate_total_track_height(&tracks), 110.0);
    }

    #[test]
    fn test_track_y_position() {
        let tracks = vec![
            TrackInfo {
                height: 35.0,
                ..Default::default()
            },
            TrackInfo {
                height: 40.0,
                ..Default::default()
            },
            TrackInfo {
                height: 35.0,
                ..Default::default()
            },
        ];
        assert_eq!(track_y_position(&tracks, 0), 0.0);
        assert_eq!(track_y_position(&tracks, 1), 35.0);
        assert_eq!(track_y_position(&tracks, 2), 75.0);
    }

    #[test]
    fn test_track_height() {
        let tracks = vec![
            TrackInfo {
                height: 35.0,
                ..Default::default()
            },
            TrackInfo {
                height: 40.0,
                ..Default::default()
            },
        ];
        assert_eq!(track_height(&tracks, 0), 35.0);
        assert_eq!(track_height(&tracks, 1), 40.0);
        assert_eq!(track_height(&tracks, 99), 35.0); // Default fallback
    }
}
