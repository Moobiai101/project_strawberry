//! # Keyframe Diamond Hit Testing & Drag Handling
//!
//! Provides interactive keyframe editing directly on the timeline.
//! Users can drag diamond markers to change a keyframe's value (vertical)
//! or time position (horizontal), matching DaVinci Resolve behavior.
//!
//! ## Priority Model
//!
//! Keyframe diamonds have HIGHER priority than clip body drag but LOWER
//! priority than trim edges. The check order in `track_drag_origin` is:
//!
//!   1. Trim edges (highest — always win)
//!   2. Keyframe diamonds (mid — only on selected clips with automation)
//!   3. Clip body move (lowest — fallback)
//!
//! ## Performance
//!
//! - Hit testing only runs on clips with `KeyframeOverlayData` (lazy check)
//! - Uses the same coordinate math as `keyframe_overlay.rs` rendering
//! - Zero allocations in the hot path

use egui::{pos2, Pos2, Rect};

use crate::widgets::keyframe_overlay::KeyframeOverlayData;
use super::super::rendering::{time_to_x, track_height, track_y_position};
use super::super::types::{ClipInfo, TrackInfo};

/// Hit radius for keyframe diamond click detection (pixels).
///
/// 7px matches DaVinci Resolve's diamond grab zone — large enough for mouse
/// precision but small enough to not overlap adjacent keyframes at moderate zoom.
const DIAMOND_HIT_RADIUS: f32 = 7.0;

/// Inset from clip rect matching the render inset in `keyframe_overlay.rs`.
const DRAW_INSET_X: f32 = 2.0;
const DRAW_INSET_Y: f32 = 4.0;

// ============================================================================
// HIT TEST RESULT
// ============================================================================

/// Result of a keyframe diamond hit test.
///
/// Contains enough info to identify the exact keyframe being interacted with,
/// plus screen-space position data needed for the drag handler.
#[derive(Debug, Clone, Copy)]
pub struct KeyframeHit {
    /// Entity bits of the clip containing this keyframe
    pub clip_entity_bits: u64,
    /// Which property the keyframe belongs to (index into overlay.properties)
    pub property_index: usize,
    /// Index of the keyframe within the property's sorted list
    pub keyframe_index: usize,
    /// Property ID string hash (for lookup on the core side)
    pub property_id_hash: u64,
    /// The original time_secs of the hit keyframe
    pub original_time_secs: f64,
    /// The original value of the hit keyframe
    pub original_value: f32,
    /// Value min for this property (for drag clamping)
    pub value_min: f32,
    /// Value max for this property (for drag clamping)
    pub value_max: f32,
    /// The clip's source duration in seconds (for time mapping)
    pub source_duration_secs: f64,
    /// The draw rect for the clip (for coordinate mapping during drag)
    pub draw_rect: Rect,
}

// ============================================================================
// HIT TESTING
// ============================================================================

/// Hit test keyframe diamonds on all visible clips.
///
/// Called from `track_drag_origin` in `handler.rs` when a drag starts
/// on the clip body (after trim edge check, before clip body check).
///
/// Only tests clips that:
/// 1. Are currently selected (matching DaVinci Resolve — unselected clips
///    don't expose keyframe handles to avoid accidental edits)
/// 2. Have `KeyframeOverlayData` (automation is active)
///
/// Returns the first diamond within `DIAMOND_HIT_RADIUS` pixels of `pos`.
pub fn hit_test_keyframe_diamond(
    pos: Pos2,
    content_rect: Rect,
    clips: &[ClipInfo],
    tracks: &[TrackInfo],
    zoom: f32,
    scroll_x: f32,
) -> Option<KeyframeHit> {
    for clip in clips {
        // Only test selected clips (pro editor behavior)
        if !clip.selected {
            continue;
        }

        let overlay: &KeyframeOverlayData = match &clip.keyframe_overlay {
            Some(o) if !o.is_empty() => o,
            _ => continue,
        };

        // Compute clip rect (same math as draw_clips / hit_test_clip)
        let clip_x: f32 = time_to_x(clip.timeline_start, zoom, scroll_x) + content_rect.left();
        let clip_width: f32 = clip.duration.as_secs_f32() * zoom;
        let track_y: f32 =
            track_y_position(tracks, clip.track_index) + content_rect.top();
        let th: f32 = track_height(tracks, clip.track_index);

        // Quick Y range check
        if pos.y < track_y || pos.y > track_y + th {
            continue;
        }

        let clip_rect: Rect =
            Rect::from_min_size(pos2(clip_x, track_y), egui::vec2(clip_width, th - 2.0));

        // Must be within clip X range (with some tolerance)
        if pos.x < clip_rect.left() - DIAMOND_HIT_RADIUS
            || pos.x > clip_rect.right() + DIAMOND_HIT_RADIUS
        {
            continue;
        }

        // Compute draw rect (same inset as keyframe_overlay.rs rendering)
        let draw_rect: Rect = clip_rect.shrink2(egui::vec2(DRAW_INSET_X, DRAW_INSET_Y));
        let draw_width: f32 = draw_rect.width();
        let draw_height: f32 = draw_rect.height();

        if draw_width < 2.0 || draw_height < 2.0 {
            continue;
        }

        let source_duration_secs: f64 =
            clip.source_out.saturating_sub(clip.source_in).as_secs_f64();
        if source_duration_secs <= 0.0 {
            continue;
        }

        // Test each property's keyframes
        for (prop_idx, property) in overlay.properties.iter().enumerate() {
            let value_range: f32 = property.value_max - property.value_min;
            if value_range.abs() < f32::EPSILON {
                continue;
            }

            for (kf_idx, kf) in property.keyframes.iter().enumerate() {
                // Map keyframe time → screen X (same as rendering)
                let t_norm: f32 = (kf.time_secs / source_duration_secs) as f32;
                if t_norm < -0.05 || t_norm > 1.05 {
                    continue;
                }

                let kf_x: f32 = draw_rect.left() + t_norm.clamp(0.0, 1.0) * draw_width;

                // Map value → screen Y (same as rendering)
                let v_norm: f32 =
                    ((kf.value - property.value_min) / value_range).clamp(0.0, 1.0);
                let kf_y: f32 = draw_rect.bottom() - v_norm * draw_height;

                // Distance check (squared to avoid sqrt)
                let dx: f32 = pos.x - kf_x;
                let dy: f32 = pos.y - kf_y;
                let dist_sq: f32 = dx * dx + dy * dy;

                if dist_sq <= DIAMOND_HIT_RADIUS * DIAMOND_HIT_RADIUS {
                    // Compute property_id hash for efficient lookup
                    let property_id_hash: u64 = hash_property_id(prop_idx);

                    return Some(KeyframeHit {
                        clip_entity_bits: clip.entity_bits,
                        property_index: prop_idx,
                        keyframe_index: kf_idx,
                        property_id_hash,
                        original_time_secs: kf.time_secs,
                        original_value: kf.value,
                        value_min: property.value_min,
                        value_max: property.value_max,
                        source_duration_secs,
                        draw_rect,
                    });
                }
            }
        }
    }

    None
}

/// Map a screen-space drag position to (time_secs, value) for a keyframe.
///
/// Used during active keyframe drag to convert cursor position into the
/// new time and value the keyframe should have.
///
/// ## Coordinate System
/// - X → time: left edge of draw_rect = 0.0s, right edge = source_duration_secs
/// - Y → value: bottom of draw_rect = value_min, top = value_max
pub fn screen_pos_to_keyframe_value(
    pos: Pos2,
    draw_rect: Rect,
    source_duration_secs: f64,
    value_min: f32,
    value_max: f32,
) -> (f64, f32) {
    let draw_width: f32 = draw_rect.width();
    let draw_height: f32 = draw_rect.height();

    // X → time
    let t_norm: f32 = if draw_width > 0.0 {
        ((pos.x - draw_rect.left()) / draw_width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let time_secs: f64 = t_norm as f64 * source_duration_secs;

    // Y → value (inverted: top = max, bottom = min)
    let v_norm: f32 = if draw_height > 0.0 {
        ((draw_rect.bottom() - pos.y) / draw_height).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let value: f32 = value_min + v_norm * (value_max - value_min);

    (time_secs, value)
}

/// Simple property index → hash for identification in the response.
///
/// Property indices are stable within a single frame. The consuming code
/// in docking.rs maps: index 0 = "volume", index 1 = "pan".
fn hash_property_id(prop_index: usize) -> u64 {
    prop_index as u64
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_pos_to_keyframe_value_center() {
        let draw_rect: Rect =
            Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(200.0, 100.0));
        let (time, value) = screen_pos_to_keyframe_value(
            pos2(200.0, 100.0), // center of rect
            draw_rect,
            10.0,  // 10 second clip
            0.0,   // volume min
            2.0,   // volume max
        );
        assert!((time - 5.0).abs() < 0.1); // ~5s
        assert!((value - 1.0).abs() < 0.1); // ~1.0 (center)
    }

    #[test]
    fn test_screen_pos_to_keyframe_value_edges() {
        let draw_rect: Rect =
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 50.0));

        // Top-left = (time=0, value=max)
        let (t, v) = screen_pos_to_keyframe_value(
            pos2(0.0, 0.0), draw_rect, 5.0, 0.0, 2.0,
        );
        assert!(t.abs() < 0.01);
        assert!((v - 2.0).abs() < 0.01);

        // Bottom-right = (time=max, value=min)
        let (t, v) = screen_pos_to_keyframe_value(
            pos2(100.0, 50.0), draw_rect, 5.0, 0.0, 2.0,
        );
        assert!((t - 5.0).abs() < 0.01);
        assert!(v.abs() < 0.01);
    }
}
