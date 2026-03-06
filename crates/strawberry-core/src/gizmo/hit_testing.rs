//! # Handle Hit Testing
//!
//! Determines which part of a transform gizmo the user clicked on.
//!
//! ## Handle Layout (CapCut/Canva style)
//!
//! ```text
//!   [TL]────[T]────[TR]
//!    |                |
//!   [L]    [Anchor]  [R]     ↻ Rotation handle (above TR)
//!    |                |
//!   [BL]────[B]────[BR]
//! ```
//!
//! - 8 scale handles at corners and edge midpoints
//! - 1 rotation handle above top-center
//! - 1 anchor point at the transform anchor position
//! - Body hit = click inside the transformed quad
//!
//! ## Algorithm
//!
//! Inspired by Olive's `TryGizmoPress` (viewerdisplay.cpp:L679-709) which
//! tests gizmos in reverse z-order. We test handles first (smallest targets),
//! then rotation, then body (largest target).

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::components::Transform2D;

// ============================================================================
// GIZMO HANDLE ENUM
// ============================================================================

/// Which part of the gizmo the user is interacting with.
///
/// Handle positions relative to the bounding box:
/// - Corners: TopLeft, TopRight, BottomLeft, BottomRight
/// - Edges: Top (midpoint), Right, Bottom, Left
/// - Special: Rotate (above top-center), Anchor (at pivot point)
/// - Body: anywhere inside the element but not on a handle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GizmoHandle {
    /// No handle hit
    #[default]
    None,
    /// Top-left corner scale handle
    TopLeft,
    /// Top edge midpoint scale handle
    Top,
    /// Top-right corner scale handle
    TopRight,
    /// Right edge midpoint scale handle
    Right,
    /// Bottom-right corner scale handle
    BottomRight,
    /// Bottom edge midpoint scale handle
    Bottom,
    /// Bottom-left corner scale handle
    BottomLeft,
    /// Left edge midpoint scale handle
    Left,
    /// Rotation handle (above top-center of bounding box)
    Rotate,
    /// Anchor/pivot point handle
    Anchor,
    /// Body of the element (inside the quad, but not on any handle)
    Body,
}

impl GizmoHandle {
    /// Whether this handle is a scale handle (corner or edge).
    pub fn is_scale_handle(&self) -> bool {
        matches!(
            self,
            GizmoHandle::TopLeft
                | GizmoHandle::Top
                | GizmoHandle::TopRight
                | GizmoHandle::Right
                | GizmoHandle::BottomRight
                | GizmoHandle::Bottom
                | GizmoHandle::BottomLeft
                | GizmoHandle::Left
        )
    }

    /// Whether this handle is a corner handle (for diagonal scaling).
    pub fn is_corner_handle(&self) -> bool {
        matches!(
            self,
            GizmoHandle::TopLeft
                | GizmoHandle::TopRight
                | GizmoHandle::BottomRight
                | GizmoHandle::BottomLeft
        )
    }

    /// Whether this is any active interaction (not None).
    pub fn is_active(&self) -> bool {
        *self != GizmoHandle::None
    }

    /// Returns the scale sign multipliers for this handle, or None if not a scale handle.
    ///
    /// Each component is -1.0, 0.0, or 1.0 indicating the direction of scale
    /// change for that axis. Used by both gizmo_input and the viewport preview
    /// to compute scale deltas consistently.
    pub fn scale_sign(&self) -> Option<Vec2> {
        match self {
            GizmoHandle::TopLeft => Some(Vec2::new(-1.0, -1.0)),
            GizmoHandle::TopRight => Some(Vec2::new(1.0, -1.0)),
            GizmoHandle::BottomRight => Some(Vec2::new(1.0, 1.0)),
            GizmoHandle::BottomLeft => Some(Vec2::new(-1.0, 1.0)),
            GizmoHandle::Top => Some(Vec2::new(0.0, -1.0)),
            GizmoHandle::Bottom => Some(Vec2::new(0.0, 1.0)),
            GizmoHandle::Left => Some(Vec2::new(-1.0, 0.0)),
            GizmoHandle::Right => Some(Vec2::new(1.0, 0.0)),
            _ => None,
        }
    }
}

// ============================================================================
// BOUNDING CORNERS
// ============================================================================

/// The four corners of a bounding box in scene coordinates.
///
/// These are the *transformed* corners of the element — i.e., after
/// position/rotation/scale have been applied. For an axis-aligned element,
/// these will be a simple rectangle. For a rotated element, these form
/// an oriented bounding box (OBB).
#[derive(Debug, Clone)]
pub struct BoundingCorners {
    pub top_left: Vec2,
    pub top_right: Vec2,
    pub bottom_right: Vec2,
    pub bottom_left: Vec2,
}

impl BoundingCorners {
    /// Compute bounding corners from a Transform2D and source dimensions.
    ///
    /// This transforms the four corners of the source rect (0,0)→(w,h)
    /// through the full transform matrix.
    pub fn from_transform(transform: &Transform2D, source_size: Vec2) -> Self {
        let matrix: glam::Mat3 = transform.to_matrix(source_size);

        Self {
            top_left: matrix.transform_point2(Vec2::ZERO),
            top_right: matrix.transform_point2(Vec2::new(source_size.x, 0.0)),
            bottom_right: matrix.transform_point2(source_size),
            bottom_left: matrix.transform_point2(Vec2::new(0.0, source_size.y)),
        }
    }

    /// Midpoint of the top edge.
    pub fn top_mid(&self) -> Vec2 {
        (self.top_left + self.top_right) * 0.5
    }

    /// Midpoint of the right edge.
    pub fn right_mid(&self) -> Vec2 {
        (self.top_right + self.bottom_right) * 0.5
    }

    /// Midpoint of the bottom edge.
    pub fn bottom_mid(&self) -> Vec2 {
        (self.bottom_left + self.bottom_right) * 0.5
    }

    /// Midpoint of the left edge.
    pub fn left_mid(&self) -> Vec2 {
        (self.top_left + self.bottom_left) * 0.5
    }

    /// Center of the bounding box.
    pub fn center(&self) -> Vec2 {
        (self.top_left + self.bottom_right) * 0.5
    }

    /// Position of the rotation handle (above top-center).
    /// The `offset` is in scene units (should be derived from a constant
    /// screen-pixel distance).
    pub fn rotation_handle_position(&self, offset_scene_units: f32) -> Vec2 {
        let top_mid: Vec2 = self.top_mid();
        let center: Vec2 = self.center();

        // Direction from center to top-mid (outward)
        let outward_direction: Vec2 = (top_mid - center).normalize_or_zero();
        top_mid + outward_direction * offset_scene_units
    }

    /// Test whether a scene-space point is inside this quad.
    ///
    /// Uses the cross-product winding method for convex quads.
    /// Used by viewport click-to-select to determine if a click
    /// falls inside a visible clip's bounding box.
    pub fn contains_point(&self, point: Vec2) -> bool {
        is_point_in_quad(
            point,
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }
}

// ============================================================================
// HIT TESTER
// ============================================================================

/// Hit test a scene-space point against the gizmo handles of a bounding box.
///
/// Tests in priority order (smallest target first):
/// 1. Scale handles (8 points: corners + edge midpoints)
/// 2. Rotation handle (above top-center)
/// 3. Anchor point handle
/// 4. Body (point-in-quad test)
///
/// # Arguments
///
/// * `scene_pos` - Click position in scene coordinates
/// * `corners` - Transformed bounding corners of the element
/// * `anchor_scene_pos` - Anchor point position in scene coordinates
/// * `handle_radius` - Hit radius in scene units (should be ~8 screen px converted)
/// * `rotation_handle_offset` - Distance of rotation handle above top-center in scene units
///
/// # Returns
///
/// The highest-priority handle that was hit, or `GizmoHandle::None`.
pub fn test_gizmo_handle_hit(
    scene_pos: Vec2,
    corners: &BoundingCorners,
    anchor_scene_pos: Vec2,
    handle_radius: f32,
    rotation_handle_offset: f32,
) -> GizmoHandle {
    let radius_squared: f32 = handle_radius * handle_radius;

    // 1. Test corner handles first (highest priority for precision)
    let corner_tests: [(Vec2, GizmoHandle); 4] = [
        (corners.top_left, GizmoHandle::TopLeft),
        (corners.top_right, GizmoHandle::TopRight),
        (corners.bottom_right, GizmoHandle::BottomRight),
        (corners.bottom_left, GizmoHandle::BottomLeft),
    ];

    for (pos, handle) in &corner_tests {
        if scene_pos.distance_squared(*pos) <= radius_squared {
            return *handle;
        }
    }

    // 2. Test edge midpoint handles
    let edge_tests: [(Vec2, GizmoHandle); 4] = [
        (corners.top_mid(), GizmoHandle::Top),
        (corners.right_mid(), GizmoHandle::Right),
        (corners.bottom_mid(), GizmoHandle::Bottom),
        (corners.left_mid(), GizmoHandle::Left),
    ];

    for (pos, handle) in &edge_tests {
        if scene_pos.distance_squared(*pos) <= radius_squared {
            return *handle;
        }
    }

    // 3. Test rotation handle
    let rotation_pos: Vec2 = corners.rotation_handle_position(rotation_handle_offset);
    if scene_pos.distance_squared(rotation_pos) <= radius_squared {
        return GizmoHandle::Rotate;
    }

    // 4. Test anchor point
    if scene_pos.distance_squared(anchor_scene_pos) <= radius_squared {
        return GizmoHandle::Anchor;
    }

    // 5. Test body (point-in-quad using cross product winding test)
    if is_point_in_quad(
        scene_pos,
        corners.top_left,
        corners.top_right,
        corners.bottom_right,
        corners.bottom_left,
    ) {
        return GizmoHandle::Body;
    }

    GizmoHandle::None
}

/// Test if a point is inside a convex quad using the cross-product winding method.
///
/// Vertices must be in order (clockwise or counter-clockwise).
/// Returns true if the point is inside the quad.
fn is_point_in_quad(point: Vec2, v0: Vec2, v1: Vec2, v2: Vec2, v3: Vec2) -> bool {
    let edges: [(Vec2, Vec2); 4] = [(v0, v1), (v1, v2), (v2, v3), (v3, v0)];

    let mut sign: Option<bool> = Option::None;

    for (a, b) in &edges {
        let edge: Vec2 = *b - *a;
        let to_point: Vec2 = point - *a;
        let cross: f32 = edge.x * to_point.y - edge.y * to_point.x;

        let is_positive: bool = cross > 0.0;

        match sign {
            Option::None => sign = Some(is_positive),
            Some(prev_sign) => {
                if is_positive != prev_sign {
                    return false;
                }
            }
        }
    }

    true
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a simple axis-aligned 200×100 rect at position (100, 50)
    fn simple_corners() -> BoundingCorners {
        BoundingCorners {
            top_left: Vec2::new(100.0, 50.0),
            top_right: Vec2::new(300.0, 50.0),
            bottom_right: Vec2::new(300.0, 150.0),
            bottom_left: Vec2::new(100.0, 150.0),
        }
    }

    #[test]
    fn test_hit_corner_top_left() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(102.0, 52.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::TopLeft);
    }

    #[test]
    fn test_hit_corner_bottom_right() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(298.0, 148.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::BottomRight);
    }

    #[test]
    fn test_hit_edge_top() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        // Top midpoint is at (200, 50)
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(200.0, 52.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::Top);
    }

    #[test]
    fn test_hit_edge_right() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        // Right midpoint is at (300, 100)
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(299.0, 100.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::Right);
    }

    #[test]
    fn test_hit_rotation_handle() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        // Top-mid is (200, 50), center is (200, 100), outward is up
        // Rotation handle at (200, 50 - 30) = (200, 20)
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(200.0, 22.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::Rotate);
    }

    #[test]
    fn test_hit_anchor_point() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(201.0, 101.0), &corners, anchor, 10.0, 30.0);
        // Anchor is inside the body, but anchor handle has priority over body
        assert_eq!(result, GizmoHandle::Anchor);
    }

    #[test]
    fn test_hit_body() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        // Click inside body but away from all handles
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(150.0, 80.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::Body);
    }

    #[test]
    fn test_hit_miss() {
        let corners: BoundingCorners = simple_corners();
        let anchor: Vec2 = Vec2::new(200.0, 100.0);
        // Click far outside the element
        let result: GizmoHandle =
            test_gizmo_handle_hit(Vec2::new(500.0, 500.0), &corners, anchor, 10.0, 30.0);
        assert_eq!(result, GizmoHandle::None);
    }

    #[test]
    fn test_hit_with_transform() {
        // Test with actual Transform2D → BoundingCorners pipeline
        // Transform2D matrix: T(position) × T(anchor_px) × R × S × T(-anchor_px)
        // With identity rotation/scale and anchor=(0.5,0.5), source=(200,100):
        //   anchor_px = (100, 50)
        //   Origin (0,0) → (-100,-50) → scale → rotate → (+100,+50) → (+100,+50) = (200, 100)
        //   So top-left = position + (0, 0) = (100, 50)
        //   Bottom-right = position + source = (300, 150)
        //   Center = (200, 100)
        let transform: Transform2D = Transform2D {
            position: Vec2::new(100.0, 50.0),
            scale: Vec2::ONE,
            rotation: 0.0,
            anchor: Vec2::splat(0.5),
            opacity: 1.0,
        };
        let source_size: Vec2 = Vec2::new(200.0, 100.0);
        let corners: BoundingCorners = BoundingCorners::from_transform(&transform, source_size);

        // Center = position + anchor_offset = (100+100, 50+50) = (200, 100)
        let center: Vec2 = corners.center();
        assert!((center.x - 200.0).abs() < 1.0, "center.x={}", center.x);
        assert!((center.y - 100.0).abs() < 1.0, "center.y={}", center.y);
    }

    #[test]
    fn test_point_in_rotated_quad() {
        // 45-degree rotated quad — center should still pass
        let half: f32 = 50.0;
        let corners: BoundingCorners = BoundingCorners {
            top_left: Vec2::new(0.0, -half), // diamond shape
            top_right: Vec2::new(half, 0.0),
            bottom_right: Vec2::new(0.0, half),
            bottom_left: Vec2::new(-half, 0.0),
        };

        // Center (0,0) should be inside
        let result: GizmoHandle = test_gizmo_handle_hit(
            Vec2::ZERO,
            &corners,
            Vec2::new(999.0, 999.0), // anchor far away
            5.0,
            30.0,
        );
        assert_eq!(result, GizmoHandle::Body);

        // Point outside the diamond
        let result_outside: GizmoHandle = test_gizmo_handle_hit(
            Vec2::new(40.0, 40.0), // outside diamond
            &corners,
            Vec2::new(999.0, 999.0),
            5.0,
            30.0,
        );
        assert_eq!(result_outside, GizmoHandle::None);
    }

    #[test]
    fn test_handle_is_scale() {
        assert!(GizmoHandle::TopLeft.is_scale_handle());
        assert!(GizmoHandle::Right.is_scale_handle());
        assert!(!GizmoHandle::Rotate.is_scale_handle());
        assert!(!GizmoHandle::Body.is_scale_handle());
        assert!(!GizmoHandle::None.is_scale_handle());
    }

    #[test]
    fn test_handle_is_corner() {
        assert!(GizmoHandle::TopLeft.is_corner_handle());
        assert!(GizmoHandle::BottomRight.is_corner_handle());
        assert!(!GizmoHandle::Top.is_corner_handle());
        assert!(!GizmoHandle::Left.is_corner_handle());
    }
}
