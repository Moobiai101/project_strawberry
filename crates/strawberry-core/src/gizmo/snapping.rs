//! # Smart Guide Snapping Engine
//!
//! Canva-inspired snapping system that provides visual alignment guides
//! and distance indicators when moving elements on the viewport canvas.
//!
//! ## Features (CapCut + Canva parity)
//!
//! - **Canvas center snap**: horizontal and vertical center lines
//! - **Canvas edge snap**: left, right, top, bottom edges
//! - **Element-to-element edge snap**: align edges of different elements
//! - **Element-to-element center snap**: align centers
//! - **Distance indicators**: show pixel spacing between elements
//! - **Configurable threshold**: snap only when within N scene units
//!
//! ## Algorithm
//!
//! For each axis (X and Y) independently:
//! 1. Compute 5 reference lines of the moving element (left, center, right for X)
//! 2. Compare against canvas edges/center and all other elements' edges/centers
//! 3. Return the closest snap within the threshold
//! 4. The UI layer draws magenta guide lines at snap positions

use glam::Vec2;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// SNAP TYPES
// ============================================================================

/// Axis for snapping operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapAxis {
    Horizontal,
    Vertical,
}

/// What the element snapped to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SnapTarget {
    /// Canvas horizontal or vertical center line
    CanvasCenter(SnapAxis),
    /// Canvas edge (left/right/top/bottom)
    CanvasEdge(Side),
    /// Another element's center line
    ElementCenter { uuid: Uuid, axis: SnapAxis },
    /// Another element's edge
    ElementEdge { uuid: Uuid, side: Side },
}

/// Side of a rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// A visual guide line to draw on the viewport.
#[derive(Debug, Clone)]
pub struct GuideLine {
    /// Start point in scene coordinates
    pub start: Vec2,
    /// End point in scene coordinates
    pub end: Vec2,
    /// What kind of alignment this guide represents
    pub kind: GuideKind,
}

/// Type of guide line (affects rendering color/style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// Center-to-center alignment (magenta dashed)
    CenterAlignment,
    /// Edge-to-edge alignment (cyan dashed)
    EdgeAlignment,
    /// Equal spacing indicator (orange)
    EqualSpacing,
}

/// Result of a snap computation for one axis.
#[derive(Debug, Clone)]
pub struct SnapResult {
    /// Which axis this snap applies to
    pub axis: SnapAxis,
    /// The snap offset to apply (add to the moving element's position)
    pub snap_delta: f32,
    /// What the element snapped to
    pub source: SnapTarget,
    /// Visual guide line to draw
    pub guide_line: GuideLine,
}

/// Axis-aligned bounding box of an element in scene coordinates.
#[derive(Debug, Clone)]
pub struct ElementBounds {
    /// Stable UUID of this element
    pub uuid: Uuid,
    /// Left edge X coordinate
    pub left: f32,
    /// Right edge X coordinate
    pub right: f32,
    /// Top edge Y coordinate
    pub top: f32,
    /// Bottom edge Y coordinate
    pub bottom: f32,
}

impl ElementBounds {
    /// Create from min/max coordinates.
    pub fn new(uuid: Uuid, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            uuid,
            left: left.min(right),
            right: left.max(right),
            top: top.min(bottom),
            bottom: top.max(bottom),
        }
    }

    /// Horizontal center.
    pub fn center_x(&self) -> f32 {
        (self.left + self.right) * 0.5
    }

    /// Vertical center.
    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) * 0.5
    }

    /// Width.
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// Height.
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// A distance indicator label between two elements.
#[derive(Debug, Clone)]
pub struct DistanceIndicator {
    /// Start point of the measurement line (scene coordinates)
    pub from: Vec2,
    /// End point of the measurement line (scene coordinates)
    pub to: Vec2,
    /// Distance in scene units (pixels at 100% zoom)
    pub distance: f32,
    /// Which axis this measurement is along
    pub axis: SnapAxis,
}

// ============================================================================
// CANVAS RECT (for snap targets)
// ============================================================================

/// Canvas dimensions for snapping to canvas edges/center.
#[derive(Debug, Clone)]
pub struct CanvasRect {
    pub width: f32,
    pub height: f32,
}

impl CanvasRect {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub fn center_x(&self) -> f32 {
        self.width * 0.5
    }

    pub fn center_y(&self) -> f32 {
        self.height * 0.5
    }
}

// ============================================================================
// SNAP ENGINE
// ============================================================================

/// Default snap threshold in scene units (~8 screen pixels at 1x zoom).
pub const DEFAULT_SNAP_THRESHOLD: f32 = 8.0;

/// Find all active snaps for a moving element against canvas and other elements.
///
/// Returns up to 2 results: one for each axis (X and Y). Only the closest
/// snap within the threshold is returned per axis.
///
/// # Arguments
///
/// * `moving` - AABB of the element being moved
/// * `others` - AABBs of all other visible elements (excluding the moving one)
/// * `canvas` - Canvas dimensions for edge/center snapping
/// * `threshold` - Maximum snap distance in scene units
pub fn find_snaps(
    moving: &ElementBounds,
    others: &[ElementBounds],
    canvas: &CanvasRect,
    threshold: f32,
) -> Vec<SnapResult> {
    let mut results: Vec<SnapResult> = Vec::new();

    // Find best X snap
    if let Some(x_snap) = find_best_x_snap(moving, others, canvas, threshold) {
        results.push(x_snap);
    }

    // Find best Y snap
    if let Some(y_snap) = find_best_y_snap(moving, others, canvas, threshold) {
        results.push(y_snap);
    }

    results
}

/// Find the closest horizontal (X-axis) snap.
fn find_best_x_snap(
    moving: &ElementBounds,
    others: &[ElementBounds],
    canvas: &CanvasRect,
    threshold: f32,
) -> Option<SnapResult> {
    let moving_reference_lines: [f32; 3] = [moving.left, moving.center_x(), moving.right];

    let mut best: Option<SnapResult> = None;
    let mut best_distance: f32 = threshold;

    // Canvas center X
    check_x_snap_candidate(
        &moving_reference_lines,
        canvas.center_x(),
        SnapTarget::CanvasCenter(SnapAxis::Horizontal),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Canvas left edge
    check_x_snap_candidate(
        &moving_reference_lines,
        0.0,
        SnapTarget::CanvasEdge(Side::Left),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Canvas right edge
    check_x_snap_candidate(
        &moving_reference_lines,
        canvas.width,
        SnapTarget::CanvasEdge(Side::Right),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Other elements
    for other in others {
        // Snap to other's left edge
        check_x_snap_candidate(
            &moving_reference_lines,
            other.left,
            SnapTarget::ElementEdge {
                uuid: other.uuid,
                side: Side::Left,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );

        // Snap to other's right edge
        check_x_snap_candidate(
            &moving_reference_lines,
            other.right,
            SnapTarget::ElementEdge {
                uuid: other.uuid,
                side: Side::Right,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );

        // Snap to other's center X
        check_x_snap_candidate(
            &moving_reference_lines,
            other.center_x(),
            SnapTarget::ElementCenter {
                uuid: other.uuid,
                axis: SnapAxis::Horizontal,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );
    }

    best
}

/// Check a single X snap candidate against all 3 reference lines of the moving element.
fn check_x_snap_candidate(
    moving_refs: &[f32; 3],
    target_x: f32,
    snap_target: SnapTarget,
    _moving: &ElementBounds,
    canvas: &CanvasRect,
    best: &mut Option<SnapResult>,
    best_distance: &mut f32,
) {
    for &ref_x in moving_refs {
        let distance: f32 = (ref_x - target_x).abs();
        if distance < *best_distance {
            let snap_delta: f32 = target_x - ref_x;
            let guide_line: GuideLine = build_vertical_guide(target_x, canvas, &snap_target);
            *best = Some(SnapResult {
                axis: SnapAxis::Horizontal,
                snap_delta,
                source: snap_target.clone(),
                guide_line,
            });
            *best_distance = distance;
        }
    }
}

/// Find the closest vertical (Y-axis) snap.
fn find_best_y_snap(
    moving: &ElementBounds,
    others: &[ElementBounds],
    canvas: &CanvasRect,
    threshold: f32,
) -> Option<SnapResult> {
    let moving_reference_lines: [f32; 3] = [moving.top, moving.center_y(), moving.bottom];

    let mut best: Option<SnapResult> = None;
    let mut best_distance: f32 = threshold;

    // Canvas center Y
    check_y_snap_candidate(
        &moving_reference_lines,
        canvas.center_y(),
        SnapTarget::CanvasCenter(SnapAxis::Vertical),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Canvas top edge
    check_y_snap_candidate(
        &moving_reference_lines,
        0.0,
        SnapTarget::CanvasEdge(Side::Top),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Canvas bottom edge
    check_y_snap_candidate(
        &moving_reference_lines,
        canvas.height,
        SnapTarget::CanvasEdge(Side::Bottom),
        moving,
        canvas,
        &mut best,
        &mut best_distance,
    );

    // Other elements
    for other in others {
        check_y_snap_candidate(
            &moving_reference_lines,
            other.top,
            SnapTarget::ElementEdge {
                uuid: other.uuid,
                side: Side::Top,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );

        check_y_snap_candidate(
            &moving_reference_lines,
            other.bottom,
            SnapTarget::ElementEdge {
                uuid: other.uuid,
                side: Side::Bottom,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );

        check_y_snap_candidate(
            &moving_reference_lines,
            other.center_y(),
            SnapTarget::ElementCenter {
                uuid: other.uuid,
                axis: SnapAxis::Vertical,
            },
            moving,
            canvas,
            &mut best,
            &mut best_distance,
        );
    }

    best
}

/// Check a single Y snap candidate against all 3 reference lines.
fn check_y_snap_candidate(
    moving_refs: &[f32; 3],
    target_y: f32,
    snap_target: SnapTarget,
    _moving: &ElementBounds,
    canvas: &CanvasRect,
    best: &mut Option<SnapResult>,
    best_distance: &mut f32,
) {
    for &ref_y in moving_refs {
        let distance: f32 = (ref_y - target_y).abs();
        if distance < *best_distance {
            let snap_delta: f32 = target_y - ref_y;
            let guide_line: GuideLine = build_horizontal_guide(target_y, canvas, &snap_target);
            *best = Some(SnapResult {
                axis: SnapAxis::Vertical,
                snap_delta,
                source: snap_target.clone(),
                guide_line,
            });
            *best_distance = distance;
        }
    }
}

// ============================================================================
// DISTANCE INDICATORS
// ============================================================================

/// Calculate distance indicators between a moving element and its nearest neighbors.
///
/// Returns measurement lines showing the pixel gap between the moving element
/// and the closest element on each side (left, right, above, below).
pub fn calculate_distance_indicators(
    moving: &ElementBounds,
    others: &[ElementBounds],
) -> Vec<DistanceIndicator> {
    let mut indicators: Vec<DistanceIndicator> = Vec::new();

    // Find nearest element to the left (where other.right < moving.left)
    let nearest_left: Option<&ElementBounds> = others
        .iter()
        .filter(|o| {
            o.right < moving.left && ranges_overlap(o.top, o.bottom, moving.top, moving.bottom)
        })
        .max_by(|a, b| {
            a.right
                .partial_cmp(&b.right)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(left_el) = nearest_left {
        let gap: f32 = moving.left - left_el.right;
        let mid_y: f32 = overlap_midpoint(left_el.top, left_el.bottom, moving.top, moving.bottom);
        indicators.push(DistanceIndicator {
            from: Vec2::new(left_el.right, mid_y),
            to: Vec2::new(moving.left, mid_y),
            distance: gap,
            axis: SnapAxis::Horizontal,
        });
    }

    // Find nearest element to the right
    let nearest_right: Option<&ElementBounds> = others
        .iter()
        .filter(|o| {
            o.left > moving.right && ranges_overlap(o.top, o.bottom, moving.top, moving.bottom)
        })
        .min_by(|a, b| {
            a.left
                .partial_cmp(&b.left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(right_el) = nearest_right {
        let gap: f32 = right_el.left - moving.right;
        let mid_y: f32 = overlap_midpoint(right_el.top, right_el.bottom, moving.top, moving.bottom);
        indicators.push(DistanceIndicator {
            from: Vec2::new(moving.right, mid_y),
            to: Vec2::new(right_el.left, mid_y),
            distance: gap,
            axis: SnapAxis::Horizontal,
        });
    }

    // Find nearest element above
    let nearest_above: Option<&ElementBounds> = others
        .iter()
        .filter(|o| {
            o.bottom < moving.top && ranges_overlap(o.left, o.right, moving.left, moving.right)
        })
        .max_by(|a, b| {
            a.bottom
                .partial_cmp(&b.bottom)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(above_el) = nearest_above {
        let gap: f32 = moving.top - above_el.bottom;
        let mid_x: f32 = overlap_midpoint(above_el.left, above_el.right, moving.left, moving.right);
        indicators.push(DistanceIndicator {
            from: Vec2::new(mid_x, above_el.bottom),
            to: Vec2::new(mid_x, moving.top),
            distance: gap,
            axis: SnapAxis::Vertical,
        });
    }

    // Find nearest element below
    let nearest_below: Option<&ElementBounds> = others
        .iter()
        .filter(|o| {
            o.top > moving.bottom && ranges_overlap(o.left, o.right, moving.left, moving.right)
        })
        .min_by(|a, b| {
            a.top
                .partial_cmp(&b.top)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(below_el) = nearest_below {
        let gap: f32 = below_el.top - moving.bottom;
        let mid_x: f32 = overlap_midpoint(below_el.left, below_el.right, moving.left, moving.right);
        indicators.push(DistanceIndicator {
            from: Vec2::new(mid_x, moving.bottom),
            to: Vec2::new(mid_x, below_el.top),
            distance: gap,
            axis: SnapAxis::Vertical,
        });
    }

    indicators
}

// ============================================================================
// GUIDE LINE BUILDERS
// ============================================================================

/// Build a vertical guide line (for X-axis snapping).
fn build_vertical_guide(x: f32, canvas: &CanvasRect, target: &SnapTarget) -> GuideLine {
    let kind: GuideKind = match target {
        SnapTarget::CanvasCenter(_) | SnapTarget::ElementCenter { .. } => {
            GuideKind::CenterAlignment
        }
        _ => GuideKind::EdgeAlignment,
    };
    GuideLine {
        start: Vec2::new(x, 0.0),
        end: Vec2::new(x, canvas.height),
        kind,
    }
}

/// Build a horizontal guide line (for Y-axis snapping).
fn build_horizontal_guide(y: f32, canvas: &CanvasRect, target: &SnapTarget) -> GuideLine {
    let kind: GuideKind = match target {
        SnapTarget::CanvasCenter(_) | SnapTarget::ElementCenter { .. } => {
            GuideKind::CenterAlignment
        }
        _ => GuideKind::EdgeAlignment,
    };
    GuideLine {
        start: Vec2::new(0.0, y),
        end: Vec2::new(canvas.width, y),
        kind,
    }
}

// ============================================================================
// HELPERS
// ============================================================================

/// Check if two ranges overlap on the same axis.
fn ranges_overlap(a_min: f32, a_max: f32, b_min: f32, b_max: f32) -> bool {
    a_min < b_max && b_min < a_max
}

/// Find the midpoint of the overlapping region of two ranges.
fn overlap_midpoint(a_min: f32, a_max: f32, b_min: f32, b_max: f32) -> f32 {
    let overlap_start: f32 = a_min.max(b_min);
    let overlap_end: f32 = a_max.min(b_max);
    (overlap_start + overlap_end) * 0.5
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_canvas() -> CanvasRect {
        CanvasRect::new(1920.0, 1080.0)
    }

    #[test]
    fn test_snap_to_canvas_center() {
        let canvas: CanvasRect = test_canvas();
        // Element near center: center_x = 962, center_y = 542
        let moving: ElementBounds = ElementBounds::new(
            Uuid::new_v4(),
            862.0,
            492.0, // left, top
            1062.0,
            592.0, // right, bottom (200×100)
        );

        let results: Vec<SnapResult> = find_snaps(&moving, &[], &canvas, 10.0);

        // Should snap X to center (960) — delta of -2
        let x_snap: Option<&SnapResult> = results.iter().find(|r| r.axis == SnapAxis::Horizontal);
        assert!(x_snap.is_some(), "Should find X snap to center");
        let x: &SnapResult = x_snap.unwrap();
        assert!(
            (x.snap_delta - (-2.0)).abs() < 0.1,
            "delta={}",
            x.snap_delta
        );

        // Should snap Y to center (540) — delta of -2
        let y_snap: Option<&SnapResult> = results.iter().find(|r| r.axis == SnapAxis::Vertical);
        assert!(y_snap.is_some(), "Should find Y snap to center");
        let y: &SnapResult = y_snap.unwrap();
        assert!(
            (y.snap_delta - (-2.0)).abs() < 0.1,
            "delta={}",
            y.snap_delta
        );
    }

    #[test]
    fn test_snap_to_canvas_edge() {
        let canvas: CanvasRect = test_canvas();
        // Element near left edge: left = 3
        let moving: ElementBounds = ElementBounds::new(Uuid::new_v4(), 3.0, 400.0, 203.0, 500.0);

        let results: Vec<SnapResult> = find_snaps(&moving, &[], &canvas, 10.0);

        let x_snap: Option<&SnapResult> = results.iter().find(|r| r.axis == SnapAxis::Horizontal);
        assert!(x_snap.is_some());
        let x: &SnapResult = x_snap.unwrap();
        // Should snap left edge to 0 → delta = -3
        assert!(
            (x.snap_delta - (-3.0)).abs() < 0.1,
            "delta={}",
            x.snap_delta
        );
    }

    #[test]
    fn test_snap_to_element_edge() {
        let canvas: CanvasRect = test_canvas();
        let other_uuid: Uuid = Uuid::new_v4();

        // Other element at (500, 300) → (700, 400)
        let other: ElementBounds = ElementBounds::new(other_uuid, 500.0, 300.0, 700.0, 400.0);

        // Moving element with left edge at 702 (near other's right edge 700)
        let moving: ElementBounds = ElementBounds::new(Uuid::new_v4(), 702.0, 300.0, 902.0, 400.0);

        let results: Vec<SnapResult> = find_snaps(&moving, &[other], &canvas, 10.0);

        let x_snap: Option<&SnapResult> = results.iter().find(|r| r.axis == SnapAxis::Horizontal);
        assert!(
            x_snap.is_some(),
            "Should snap to other element's right edge"
        );
        let x: &SnapResult = x_snap.unwrap();
        assert!(
            (x.snap_delta - (-2.0)).abs() < 0.1,
            "delta={}",
            x.snap_delta
        );
    }

    #[test]
    fn test_no_snap_outside_threshold() {
        let canvas: CanvasRect = test_canvas();
        // Element far from any snap target
        let moving: ElementBounds = ElementBounds::new(Uuid::new_v4(), 400.0, 300.0, 600.0, 400.0);

        let results: Vec<SnapResult> = find_snaps(&moving, &[], &canvas, 5.0);

        // Center is at (960, 540) — moving center is (500, 350)
        // Distance is way more than 5, so no snap
        assert!(results.is_empty(), "Should not snap when outside threshold");
    }

    #[test]
    fn test_distance_indicators_horizontal() {
        let other: ElementBounds = ElementBounds::new(Uuid::new_v4(), 100.0, 200.0, 300.0, 400.0);

        // Moving element to the right of other, with 50px gap
        let moving: ElementBounds = ElementBounds::new(Uuid::new_v4(), 350.0, 250.0, 550.0, 350.0);

        let indicators: Vec<DistanceIndicator> = calculate_distance_indicators(&moving, &[other]);

        // Should have at least one indicator showing the 50px gap to the left
        let left_indicator: Option<&DistanceIndicator> = indicators
            .iter()
            .find(|i| i.axis == SnapAxis::Horizontal && (i.distance - 50.0).abs() < 0.1);
        assert!(
            left_indicator.is_some(),
            "Should show 50px gap to left element"
        );
    }

    #[test]
    fn test_distance_indicators_vertical() {
        let other: ElementBounds = ElementBounds::new(Uuid::new_v4(), 200.0, 100.0, 400.0, 200.0);

        // Moving element below other, with 30px gap
        let moving: ElementBounds = ElementBounds::new(Uuid::new_v4(), 250.0, 230.0, 350.0, 330.0);

        let indicators: Vec<DistanceIndicator> = calculate_distance_indicators(&moving, &[other]);

        let above_indicator: Option<&DistanceIndicator> = indicators
            .iter()
            .find(|i| i.axis == SnapAxis::Vertical && (i.distance - 30.0).abs() < 0.1);
        assert!(
            above_indicator.is_some(),
            "Should show 30px gap to element above"
        );
    }

    #[test]
    fn test_element_bounds_properties() {
        let bounds: ElementBounds = ElementBounds::new(Uuid::new_v4(), 100.0, 200.0, 300.0, 500.0);
        assert_eq!(bounds.width(), 200.0);
        assert_eq!(bounds.height(), 300.0);
        assert_eq!(bounds.center_x(), 200.0);
        assert_eq!(bounds.center_y(), 350.0);
    }
}
