//! # Gizmo Interaction State
//!
//! Tracks the current state of viewport gizmo interactions.
//! This is a resource (not a component) because there's only one active
//! gizmo interaction at a time across the entire viewport.
//!
//! ## State Machine
//!
//! ```text
//! Idle → (mouse press on handle) → Started → (mouse move) → Dragging → (mouse release) → Idle
//!                                                              ↓
//!                                                    (Escape key) → Cancelled → Idle
//! ```
//!
//! ## Design
//!
//! Inspired by Olive's `gizmo_drag_started_` / `current_gizmo_`/ `gizmo_start_drag_`
//! state tracking in viewerdisplay.cpp, but formalized as an explicit state machine
//! for clearer reasoning and testability.

use bevy_ecs::system::Resource;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::hit_testing::GizmoHandle;
use super::snapping::{DistanceIndicator, GuideLine};
use crate::components::Transform2D;

// ============================================================================
// GIZMO MODE
// ============================================================================

/// High-level gizmo interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GizmoMode {
    /// Default mode — no gizmo interaction active
    #[default]
    Idle,
    /// Moving the element (dragging body)
    Move,
    /// Scaling the element (dragging a scale handle)
    Scale,
    /// Rotating the element (dragging rotation handle)
    Rotate,
    /// Dragging the anchor/pivot point
    AnchorDrag,
    /// Crop mode (future: entered via double-click or toolbar button)
    Crop,
}

// ============================================================================
// DRAG PHASE
// ============================================================================

/// Phase of the current drag operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragPhase {
    /// No drag in progress
    #[default]
    Idle,
    /// Mouse pressed on a handle but not yet moved enough to start drag
    /// (prevents tiny accidental drags from creating undo entries)
    PressedAwaitingThreshold,
    /// Actively dragging — transform is being modified
    Dragging,
}

// ============================================================================
// MODIFIER STATE
// ============================================================================

/// Currently held keyboard modifiers during gizmo interaction.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModifierState {
    /// Shift held → aspect-ratio lock (scale), 15° snap (rotate), 10px nudge
    pub shift_held: bool,
    /// Ctrl held → snap to grid (future)
    pub ctrl_held: bool,
    /// Alt held → scale from center, duplicate drag (future)
    pub alt_held: bool,
}

// ============================================================================
// GIZMO STATE (ECS Resource)
// ============================================================================

/// The main gizmo interaction state resource.
///
/// Stored as a Bevy ECS resource. Updated every frame by the viewport
/// input handler. Read by the viewport overlay renderer for visuals.
#[derive(Debug, Clone, Default, Resource)]
pub struct GizmoState {
    /// Current interaction mode
    pub mode: GizmoMode,

    /// Which handle is currently being interacted with
    pub active_handle: GizmoHandle,

    /// Current phase of the drag operation
    pub drag_phase: DragPhase,

    /// Scene-space position where the drag started
    pub drag_origin_scene: Vec2,

    /// Current scene-space position during drag
    pub drag_current_scene: Vec2,

    /// The entity's Transform2D at drag start (for computing deltas)
    pub original_transform: Option<Transform2D>,

    /// The entity's source size at drag start (for matrix computation)
    pub original_source_size: Option<Vec2>,

    /// Currently active guide lines from snapping (rendered by overlay)
    pub active_guides: Vec<GuideLine>,

    /// Distance indicator labels between elements (rendered by overlay)
    pub distance_indicators: Vec<DistanceIndicator>,

    /// Current keyboard modifier state
    pub modifier_state: ModifierState,

    /// Minimum drag distance in screen pixels before starting a drag
    /// (prevents accidental micro-drags from creating undo entries)
    pub drag_threshold_screen_pixels: f32,
}

impl GizmoState {
    /// Create a new idle gizmo state.
    pub fn new() -> Self {
        Self {
            drag_threshold_screen_pixels: 3.0,
            ..Self::default()
        }
    }

    /// Begin a potential drag operation (mouse pressed on a handle).
    ///
    /// Doesn't start dragging immediately — waits for mouse movement
    /// past the threshold to avoid accidental micro-drags.
    pub fn begin_press(
        &mut self,
        handle: GizmoHandle,
        scene_pos: Vec2,
        current_transform: Transform2D,
        source_size: Vec2,
    ) {
        self.active_handle = handle;
        self.drag_phase = DragPhase::PressedAwaitingThreshold;
        self.drag_origin_scene = scene_pos;
        self.drag_current_scene = scene_pos;
        self.original_transform = Some(current_transform);
        self.original_source_size = Some(source_size);
        self.active_guides.clear();
        self.distance_indicators.clear();

        // Set mode based on handle
        self.mode = match handle {
            GizmoHandle::Body => GizmoMode::Move,
            GizmoHandle::Rotate => GizmoMode::Rotate,
            GizmoHandle::Anchor => GizmoMode::AnchorDrag,
            h if h.is_scale_handle() => GizmoMode::Scale,
            _ => GizmoMode::Idle,
        };
    }

    /// Update the current drag position and check if threshold is met.
    ///
    /// Returns true if the drag has been promoted to `Dragging` phase
    /// (either already was, or just crossed the threshold).
    pub fn update_drag(&mut self, scene_pos: Vec2, screen_distance_moved: f32) -> bool {
        self.drag_current_scene = scene_pos;

        match self.drag_phase {
            DragPhase::PressedAwaitingThreshold => {
                if screen_distance_moved >= self.drag_threshold_screen_pixels {
                    self.drag_phase = DragPhase::Dragging;
                    true
                } else {
                    false
                }
            }
            DragPhase::Dragging => true,
            DragPhase::Idle => false,
        }
    }

    /// Commit the drag and return to idle state.
    /// Returns the original transform (for creating undo commands).
    pub fn commit_drag(&mut self) -> Option<Transform2D> {
        let original: Option<Transform2D> = self.original_transform.take();
        self.reset();
        original
    }

    /// Cancel the drag and return to idle state.
    /// Returns the original transform so it can be restored.
    pub fn cancel_drag(&mut self) -> Option<Transform2D> {
        let original: Option<Transform2D> = self.original_transform.take();
        self.reset();
        original
    }

    /// Whether the gizmo is in the middle of a drag.
    pub fn is_dragging(&self) -> bool {
        self.drag_phase == DragPhase::Dragging
    }

    /// Whether the gizmo has a press or drag in progress.
    pub fn is_active(&self) -> bool {
        self.drag_phase != DragPhase::Idle
    }

    /// Get the scene-space delta from drag origin to current position.
    pub fn drag_delta_scene(&self) -> Vec2 {
        self.drag_current_scene - self.drag_origin_scene
    }

    /// Reset all state to idle.
    fn reset(&mut self) {
        self.mode = GizmoMode::Idle;
        self.active_handle = GizmoHandle::None;
        self.drag_phase = DragPhase::Idle;
        self.drag_origin_scene = Vec2::ZERO;
        self.drag_current_scene = Vec2::ZERO;
        self.original_transform = None;
        self.original_source_size = None;
        self.active_guides.clear();
        self.distance_indicators.clear();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gizmo_state_lifecycle() {
        let mut state: GizmoState = GizmoState::new();

        assert!(!state.is_active());
        assert!(!state.is_dragging());

        // Begin press
        let transform: Transform2D = Transform2D::from_position(100.0, 50.0);
        let source_size: Vec2 = Vec2::new(200.0, 100.0);
        state.begin_press(
            GizmoHandle::Body,
            Vec2::new(100.0, 50.0),
            transform.clone(),
            source_size,
        );

        assert!(state.is_active());
        assert!(!state.is_dragging());
        assert_eq!(state.mode, GizmoMode::Move);

        // Update drag (below threshold)
        let active: bool = state.update_drag(Vec2::new(101.0, 50.0), 1.0);
        assert!(!active);
        assert!(!state.is_dragging());

        // Update drag (above threshold)
        let active: bool = state.update_drag(Vec2::new(110.0, 55.0), 12.0);
        assert!(active);
        assert!(state.is_dragging());

        // Check delta
        let delta: Vec2 = state.drag_delta_scene();
        assert!((delta.x - 10.0).abs() < 0.1);
        assert!((delta.y - 5.0).abs() < 0.1);

        // Commit
        let original: Option<Transform2D> = state.commit_drag();
        assert!(original.is_some());
        assert!(!state.is_active());
    }

    #[test]
    fn test_gizmo_state_cancel() {
        let mut state: GizmoState = GizmoState::new();
        let transform: Transform2D = Transform2D::from_position(50.0, 25.0);
        state.begin_press(
            GizmoHandle::TopLeft,
            Vec2::ZERO,
            transform.clone(),
            Vec2::new(100.0, 100.0),
        );
        state.update_drag(Vec2::new(20.0, 20.0), 30.0);

        assert!(state.is_dragging());
        assert_eq!(state.mode, GizmoMode::Scale);

        let original: Option<Transform2D> = state.cancel_drag();
        assert!(original.is_some());
        assert_eq!(original.unwrap().position.x, 50.0);
        assert!(!state.is_active());
    }

    #[test]
    fn test_mode_from_handle() {
        let mut state: GizmoState = GizmoState::new();
        let t: Transform2D = Transform2D::default();
        let s: Vec2 = Vec2::ONE;

        state.begin_press(GizmoHandle::Body, Vec2::ZERO, t.clone(), s);
        assert_eq!(state.mode, GizmoMode::Move);
        state.cancel_drag();

        state.begin_press(GizmoHandle::Rotate, Vec2::ZERO, t.clone(), s);
        assert_eq!(state.mode, GizmoMode::Rotate);
        state.cancel_drag();

        state.begin_press(GizmoHandle::TopRight, Vec2::ZERO, t.clone(), s);
        assert_eq!(state.mode, GizmoMode::Scale);
        state.cancel_drag();

        state.begin_press(GizmoHandle::Anchor, Vec2::ZERO, t.clone(), s);
        assert_eq!(state.mode, GizmoMode::AnchorDrag);
        state.cancel_drag();
    }
}
