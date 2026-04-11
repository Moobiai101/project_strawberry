//! # Keyframe Overlay Renderer
//!
//! Renders automation curves on top of timeline clips as translucent lines.
//! Follows the same batched drawing pattern as the fade overlay in `clip_renderer.rs`.
//!
//! ## Visual Design (After Effects / DaVinci Resolve style)
//!
//! - Thin line (1.5px) traces the interpolation curve across the clip
//! - Keyframe points rendered as small diamonds (◇)
//! - Volume: yellow (#FFD700), Pan: cyan (#00CED1)
//! - Curve height maps value range to clip height (bottom=min, top=max)
//! - Semi-transparent fill beneath the curve for volume (optional)
//!
//! ## Performance
//!
//! - Only renders for clips that have `KeyframeOverlayData` attached
//! - Segments sampled at ~2px resolution (adaptive to zoom)
//! - Single mesh per property per frame, not per clip

use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use strawberry_core::components::keyframe::{evaluate_at_time, Keyframe};

// ============================================================================
// OVERLAY DATA (passed from drawing.rs → renderer)
// ============================================================================

/// Keyframe data for a single property, ready for rendering.
///
/// Pre-extracted from `KeyframeTrack` in the clip collection pass (drawing.rs)
/// to avoid ECS queries during rendering. Contains only what the renderer needs.
#[derive(Debug, Clone)]
pub struct PropertyKeyframeData {
    /// The sorted keyframe list (cloned from KeyframeTrack)
    pub keyframes: Vec<Keyframe>,
    /// Display color for this property's curve
    pub color: Color32,
    /// Value range for this property (maps to clip height)
    pub value_min: f32,
    pub value_max: f32,
    /// Default value (used for empty regions)
    pub default_value: f32,
}

/// All keyframe overlay data for a single clip.
///
/// Attached to `VisibleClipRect` as `Option<KeyframeOverlayData>`.
/// `None` = no automation on this clip (zero rendering cost).
#[derive(Debug, Clone, Default)]
pub struct KeyframeOverlayData {
    /// Per-property keyframe data (typically 0–3 properties)
    pub properties: Vec<PropertyKeyframeData>,
}

impl KeyframeOverlayData {
    /// Check if there's any automation data to render.
    pub fn is_empty(&self) -> bool {
        self.properties.iter().all(|p| p.keyframes.is_empty())
    }
}

// ============================================================================
// WELL-KNOWN COLORS
// ============================================================================

/// Automation curve color for volume property.
pub const VOLUME_CURVE_COLOR: Color32 = Color32::from_rgb(255, 215, 0); // Gold

/// Automation curve color for pan property.
pub const PAN_CURVE_COLOR: Color32 = Color32::from_rgb(0, 206, 209); // Dark Turquoise

/// Automation curve color for opacity property.
pub const OPACITY_CURVE_COLOR: Color32 = Color32::from_rgb(180, 130, 255); // Lavender

// ============================================================================
// OVERLAY RENDERER
// ============================================================================

/// Render keyframe automation curves for all clips that have overlay data.
///
/// This is the entry point called from the batched render pipeline in `drawing.rs`.
/// Renders after fade curves, before selection overlays.
///
/// ## Parameters
/// - `painter`: egui Painter for the timeline area
/// - `clips`: All visible clips (only those with `keyframe_overlay` are rendered)
/// - `get_overlay`: Closure to get overlay data for a clip by index
pub fn render_keyframe_overlays(
    painter: &Painter,
    clip_rects: &[Rect],
    overlays: &[Option<KeyframeOverlayData>],
    clip_source_durations_secs: &[f64],
) {
    debug_assert_eq!(clip_rects.len(), overlays.len());
    debug_assert_eq!(clip_rects.len(), clip_source_durations_secs.len());

    for (idx, overlay_opt) in overlays.iter().enumerate() {
        let overlay: &KeyframeOverlayData = match overlay_opt {
            Some(o) if !o.is_empty() => o,
            _ => continue,
        };

        let clip_rect: Rect = clip_rects[idx];
        let source_duration_secs: f64 = clip_source_durations_secs[idx];

        if clip_rect.width() < 4.0 || source_duration_secs <= 0.0 {
            continue; // Too small to draw
        }

        for property in &overlay.properties {
            render_property_curve(
                painter,
                clip_rect,
                source_duration_secs,
                property,
            );
        }
    }
}

// ============================================================================
// CURVE RENDERING
// ============================================================================

/// Render a single property's automation curve within a clip rectangle.
///
/// The curve is drawn as a polyline with adaptive sampling — approximately
/// one sample per 2 pixels of width for smooth appearance at any zoom level.
fn render_property_curve(
    painter: &Painter,
    clip_rect: Rect,
    source_duration_secs: f64,
    property: &PropertyKeyframeData,
) {
    if property.keyframes.is_empty() {
        return;
    }

    let value_range: f32 = property.value_max - property.value_min;
    if value_range.abs() < f32::EPSILON {
        return;
    }

    // Inset slightly so curve doesn't draw over clip borders
    let draw_rect: Rect = clip_rect.shrink2(egui::vec2(2.0, 4.0));
    let draw_width: f32 = draw_rect.width();
    let draw_height: f32 = draw_rect.height();

    if draw_width < 2.0 || draw_height < 2.0 {
        return;
    }

    // Adaptive sample count: ~1 sample per 2px, clamped to [8, 512]
    let sample_count: usize = (draw_width as usize / 2).clamp(8, 512);

    // Build polyline points
    let mut points: Vec<Pos2> = Vec::with_capacity(sample_count + 1);

    for i in 0..=sample_count {
        let t_normalized: f64 = i as f64 / sample_count as f64;
        let time_secs: f64 = t_normalized * source_duration_secs;

        let value: f32 = evaluate_at_time(
            &property.keyframes,
            time_secs,
            property.default_value,
        );

        // Map value to Y position (inverted: high values at top)
        let value_normalized: f32 = (value - property.value_min) / value_range;
        let value_clamped: f32 = value_normalized.clamp(0.0, 1.0);

        let x: f32 = draw_rect.left() + t_normalized as f32 * draw_width;
        let y: f32 = draw_rect.bottom() - value_clamped * draw_height;

        points.push(pos2(x, y));
    }

    // Draw the curve line
    let stroke: Stroke = Stroke::new(1.5, property.color);
    painter.add(Shape::line(points.clone(), stroke));

    // Draw keyframe diamonds at each keyframe position
    render_keyframe_diamonds(
        painter,
        draw_rect,
        source_duration_secs,
        property,
        value_range,
    );
}

/// Render small diamond markers at each keyframe position.
///
/// Diamonds are 5px wide, filled with the property color,
/// providing visual anchors for where keyframes are placed.
fn render_keyframe_diamonds(
    painter: &Painter,
    draw_rect: Rect,
    source_duration_secs: f64,
    property: &PropertyKeyframeData,
    value_range: f32,
) {
    let draw_width: f32 = draw_rect.width();
    let draw_height: f32 = draw_rect.height();
    let diamond_size: f32 = 3.5;

    for kf in &property.keyframes {
        // Map time to X
        let t_normalized: f32 = if source_duration_secs > 0.0 {
            (kf.time_secs / source_duration_secs) as f32
        } else {
            0.0
        };

        if t_normalized < -0.01 || t_normalized > 1.01 {
            continue; // Outside visible range
        }

        // Map value to Y
        let value_normalized: f32 =
            ((kf.value - property.value_min) / value_range).clamp(0.0, 1.0);

        let center_x: f32 = draw_rect.left() + t_normalized.clamp(0.0, 1.0) * draw_width;
        let center_y: f32 = draw_rect.bottom() - value_normalized * draw_height;

        // Diamond shape (4 points: top, right, bottom, left)
        let diamond: Vec<Pos2> = vec![
            pos2(center_x, center_y - diamond_size),      // top
            pos2(center_x + diamond_size, center_y),       // right
            pos2(center_x, center_y + diamond_size),       // bottom
            pos2(center_x - diamond_size, center_y),       // left
        ];

        // Filled diamond with thin border
        painter.add(Shape::convex_polygon(
            diamond,
            property.color,
            Stroke::new(0.5, Color32::from_rgb(40, 40, 40)),
        ));
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_data_empty() {
        let data: KeyframeOverlayData = KeyframeOverlayData::default();
        assert!(data.is_empty());
    }

    #[test]
    fn test_overlay_data_with_keyframes() {
        let data: KeyframeOverlayData = KeyframeOverlayData {
            properties: vec![PropertyKeyframeData {
                keyframes: vec![Keyframe::new(0.0, 1.0), Keyframe::new(2.0, 0.5)],
                color: VOLUME_CURVE_COLOR,
                value_min: 0.0,
                value_max: 2.0,
                default_value: 1.0,
            }],
        };
        assert!(!data.is_empty());
    }
}
