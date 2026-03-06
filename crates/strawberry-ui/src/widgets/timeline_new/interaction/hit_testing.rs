//! Hit testing functions for timeline interactions.
//!
//! Contains functions to determine what element (if any) is at a given position.

use egui::{pos2, vec2, Pos2, Rect};

use crate::widgets::timeline_new::rendering::{time_to_x, track_height, track_y_position};
use crate::widgets::timeline_new::types::{ClipInfo, TrackInfo, TrimEdge};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Width of trim handle zone on each clip edge (pixels) - wider for easier detection
pub const TRIM_HANDLE_WIDTH: f32 = 12.0;

// ============================================================================
// HIT TEST RESULTS
// ============================================================================

/// Result of a clip hit test.
#[derive(Debug, Clone, Copy)]
pub struct ClipHit {
    /// Entity bits of the hit clip
    pub entity_bits: u64,
    /// Track index of the clip
    pub track_index: u32,
}

/// Result of a trim edge hit test.
#[derive(Debug, Clone, Copy)]
pub struct TrimHit {
    /// Entity bits of the clip
    pub entity_bits: u64,
    /// Which edge was hit
    pub edge: TrimEdge,
}

/// Result of a gap hit test (for transition buttons).
#[derive(Debug, Clone, Copy)]
pub struct GapHit {
    /// Entity bits of the left clip
    pub left_clip: u64,
    /// Entity bits of the right clip
    pub right_clip: u64,
}

// ============================================================================
// HIT TESTING FUNCTIONS
// ============================================================================

/// Hit test to find which clip (if any) is at the given position.
///
/// # Parameters
/// - `pos`: Mouse position in screen coordinates
/// - `content_rect`: The content area rectangle
/// - `clips`: All clips to test
/// - `tracks`: Track information for layout
/// - `zoom`: Pixels per second
/// - `scroll_x`: Horizontal scroll offset in pixels
pub fn hit_test_clip(
    pos: Pos2,
    content_rect: Rect,
    clips: &[ClipInfo],
    tracks: &[TrackInfo],
    zoom: f32,
    scroll_x: f32,
) -> Option<ClipHit> {
    for clip in clips {
        let clip_x: f32 = time_to_x(clip.timeline_start, zoom, scroll_x) + content_rect.left();
        let clip_width: f32 = clip.duration.as_secs_f32() * zoom;
        let track_y: f32 = track_y_position(tracks, clip.track_index) + content_rect.top();
        let h: f32 = track_height(tracks, clip.track_index);

        let clip_rect: Rect = Rect::from_min_size(pos2(clip_x, track_y), vec2(clip_width, h - 2.0));

        if clip_rect.contains(pos) {
            return Some(ClipHit {
                entity_bits: clip.entity_bits,
                track_index: clip.track_index,
            });
        }
    }
    None
}

/// Hit test to find if mouse is on a clip edge (for trim handles).
///
/// Returns the clip and edge if on an edge, None otherwise.
/// Based on Olive's IsCursorInTrimHandle() logic for precision.
pub fn hit_test_clip_edge(
    pos: Pos2,
    content_rect: Rect,
    clips: &[ClipInfo],
    tracks: &[TrackInfo],
    zoom: f32,
    scroll_x: f32,
) -> Option<TrimHit> {
    for clip in clips {
        let clip_x: f32 = time_to_x(clip.timeline_start, zoom, scroll_x) + content_rect.left();
        let clip_width: f32 = clip.duration.as_secs_f32() * zoom;
        let track_y: f32 = track_y_position(tracks, clip.track_index) + content_rect.top();
        let h: f32 = track_height(tracks, clip.track_index);

        // Check if in correct Y range first
        if pos.y < track_y || pos.y > track_y + h {
            continue;
        }

        // Olive's approach: Skip narrow clips where trim zones would overlap
        if clip_width <= TRIM_HANDLE_WIDTH * 2.0 {
            continue;
        }

        // Olive's approach: Edge zones are INSIDE the clip, not centered on boundary
        // Left edge: from clip start to clip start + handle width
        let left_edge_min: f32 = clip_x;
        let left_edge_max: f32 = clip_x + TRIM_HANDLE_WIDTH;

        // Right edge: from clip end - handle width to clip end
        let right_edge_min: f32 = clip_x + clip_width - TRIM_HANDLE_WIDTH;
        let right_edge_max: f32 = clip_x + clip_width;

        // Check edges (left takes priority if overlapping)
        if pos.x >= left_edge_min && pos.x <= left_edge_max {
            return Some(TrimHit {
                entity_bits: clip.entity_bits,
                edge: TrimEdge::Left,
            });
        }
        if pos.x >= right_edge_min && pos.x <= right_edge_max {
            return Some(TrimHit {
                entity_bits: clip.entity_bits,
                edge: TrimEdge::Right,
            });
        }
    }
    None
}

/// Hit test to find if mouse is over a gap between adjacent clips.
///
/// Returns the left and right clip entities if in a gap zone.
pub fn hit_test_gap(
    pos: Pos2,
    content_rect: Rect,
    clips: &[ClipInfo],
    tracks: &[TrackInfo],
    zoom: f32,
    scroll_x: f32,
) -> Option<GapHit> {
    for track in tracks {
        let track_y: f32 = track_y_position(tracks, track.index) + content_rect.top();
        let h: f32 = track_height(tracks, track.index);

        // Check if in correct Y range first
        if pos.y < track_y || pos.y > track_y + h {
            continue;
        }

        // Get clips on this track, sorted by start time
        let mut track_clips: Vec<&ClipInfo> = clips
            .iter()
            .filter(|c| c.track_index == track.index)
            .collect();
        track_clips.sort_by(|a, b| a.timeline_start.cmp(&b.timeline_start));

        // Check pairs of adjacent clips for gap zones
        for window in track_clips.windows(2) {
            let clip_a = window[0];
            let clip_b = window[1];

            let a_end_x: f32 = content_rect.left()
                + time_to_x(clip_a.timeline_start, zoom, scroll_x)
                + clip_a.duration.as_secs_f32() * zoom;
            let b_start_x: f32 =
                content_rect.left() + time_to_x(clip_b.timeline_start, zoom, scroll_x);

            // Check if position is in the gap zone (expanded area for easier targeting)
            let gap_left: f32 = a_end_x - 10.0; // Extend slightly into clips
            let gap_right: f32 = b_start_x + 10.0;

            if pos.x >= gap_left && pos.x <= gap_right {
                return Some(GapHit {
                    left_clip: clip_a.entity_bits,
                    right_clip: clip_b.entity_bits,
                });
            }
        }
    }
    None
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_clip(
        entity_bits: u64,
        track_index: u32,
        start_secs: u64,
        duration_secs: u64,
    ) -> ClipInfo {
        ClipInfo {
            entity_bits,
            name: format!("Clip {}", entity_bits),
            track_index,
            timeline_start: Duration::from_secs(start_secs),
            duration: Duration::from_secs(duration_secs),
            selected: false,
            color: None,
            asset_uuid: None,
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(duration_secs),
        }
    }

    fn create_test_track(index: u32) -> TrackInfo {
        TrackInfo {
            entity_bits: index as u64 * 1000,
            index,
            height: 35.0,
            ..Default::default()
        }
    }

    #[test]
    fn test_hit_test_clip() {
        let clips = vec![create_test_clip(1, 0, 0, 5)];
        let tracks = vec![create_test_track(0)];
        let content_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 100.0));

        // Hit in clip (zoom 100 = 100px/sec, so clip is 0-500px)
        let hit = hit_test_clip(pos2(250.0, 17.0), content_rect, &clips, &tracks, 100.0, 0.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().entity_bits, 1);

        // Miss (outside clip)
        let miss = hit_test_clip(pos2(600.0, 17.0), content_rect, &clips, &tracks, 100.0, 0.0);
        assert!(miss.is_none());
    }

    #[test]
    fn test_hit_test_clip_edge() {
        let clips = vec![create_test_clip(1, 0, 0, 5)];
        let tracks = vec![create_test_track(0)];
        let content_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 100.0));

        // Hit left edge (within TRIM_HANDLE_WIDTH of left side)
        let hit = hit_test_clip_edge(pos2(5.0, 17.0), content_rect, &clips, &tracks, 100.0, 0.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().edge, TrimEdge::Left);

        // Hit right edge (within TRIM_HANDLE_WIDTH of right side)
        let hit = hit_test_clip_edge(pos2(495.0, 17.0), content_rect, &clips, &tracks, 100.0, 0.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().edge, TrimEdge::Right);
    }
}
