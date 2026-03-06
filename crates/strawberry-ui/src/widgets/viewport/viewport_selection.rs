//! # Viewport Selection
//!
//! Hit-testing against visible clips in the viewport for click-to-select.
//!
//! ## Design
//!
//! Mirrors Olive's `TryGizmoPress()` pattern (viewerdisplay.cpp:679-708)
//! which iterates gizmos in reverse order and tests containment.
//!
//! When the user clicks in the viewport and misses all gizmo handles,
//! this module determines which clip (if any) the click landed on.
//! Clips are tested in reverse z-order so the topmost visible clip
//! is selected first.

use bevy_ecs::entity::Entity;
use glam::Vec2;
use strawberry_core::gizmo::hit_testing::BoundingCorners;

/// A visible clip in the viewport with its bounding corners in scene space.
///
/// Built by the caller (docking.rs) from ECS queries. Each entry represents
/// one clip that is currently visible in the viewport.
#[derive(Debug, Clone)]
pub struct VisibleClip {
    /// The ECS entity for this clip
    pub entity: Entity,
    /// Bounding corners in scene coordinates (from Transform2D)
    pub corners: BoundingCorners,
}

/// Test a scene-space click position against all visible clip quads.
///
/// Returns the topmost entity under the cursor. Clips are tested in
/// reverse order (last element = topmost in z-order) so the visually
/// frontmost clip is returned first.
///
/// This mirrors Olive's `TryGizmoPress()` which iterates gizmos in
/// reverse order and tests containment.
///
/// ## Arguments
///
/// - `scene_pos`: Click position in scene coordinates (already converted
///   from screen via `CoordinateTransform::screen_to_scene()`)
/// - `visible_clips`: Slice of visible clips ordered by z-order
///   (first = backmost, last = frontmost)
///
/// ## Returns
///
/// `Some(Entity)` for the topmost clip under the cursor, or `None` if
/// the click landed on empty space.
pub fn hit_test_viewport_clips(scene_pos: Vec2, visible_clips: &[VisibleClip]) -> Option<Entity> {
    // Iterate in reverse z-order (topmost clip first)
    for clip in visible_clips.iter().rev() {
        if clip.corners.contains_point(scene_pos) {
            return Some(clip.entity);
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

    fn make_clip(index: u32, tl: Vec2, br: Vec2) -> VisibleClip {
        VisibleClip {
            entity: Entity::from_raw(index),
            corners: BoundingCorners {
                top_left: tl,
                top_right: Vec2::new(br.x, tl.y),
                bottom_right: br,
                bottom_left: Vec2::new(tl.x, br.y),
            },
        }
    }

    #[test]
    fn test_hit_single_clip() {
        let clips: Vec<VisibleClip> =
            vec![make_clip(1, Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))];

        let result: Option<Entity> = hit_test_viewport_clips(Vec2::new(50.0, 50.0), &clips);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Entity::from_raw(1));
    }

    #[test]
    fn test_miss_empty_space() {
        let clips: Vec<VisibleClip> =
            vec![make_clip(1, Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))];

        let result: Option<Entity> = hit_test_viewport_clips(Vec2::new(200.0, 200.0), &clips);
        assert!(result.is_none());
    }

    #[test]
    fn test_topmost_clip_wins() {
        // Two overlapping clips — the second (topmost) should win
        let clips: Vec<VisibleClip> = vec![
            make_clip(1, Vec2::new(0.0, 0.0), Vec2::new(200.0, 200.0)), // backmost
            make_clip(2, Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0)), // frontmost
        ];

        // Click in overlap region → should select clip 2 (topmost)
        let result: Option<Entity> = hit_test_viewport_clips(Vec2::new(100.0, 100.0), &clips);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Entity::from_raw(2));
    }

    #[test]
    fn test_click_on_back_clip_only() {
        let clips: Vec<VisibleClip> = vec![
            make_clip(1, Vec2::new(0.0, 0.0), Vec2::new(200.0, 200.0)),
            make_clip(2, Vec2::new(300.0, 300.0), Vec2::new(400.0, 400.0)),
        ];

        // Click on back clip only (no overlap)
        let result: Option<Entity> = hit_test_viewport_clips(Vec2::new(50.0, 50.0), &clips);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Entity::from_raw(1));
    }

    #[test]
    fn test_empty_clips() {
        let result: Option<Entity> = hit_test_viewport_clips(Vec2::new(50.0, 50.0), &[]);
        assert!(result.is_none());
    }
}
