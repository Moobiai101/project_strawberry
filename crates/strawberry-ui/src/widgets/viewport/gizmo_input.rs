//! # Gizmo Input Handler
//!
//! Routes mouse and keyboard events to gizmo operations, producing
//! Commands for the undo/redo system.
//!
//! ## Design
//!
//! This module reads `GizmoState` and the egui `Response` to detect
//! mouse press/drag/release and keyboard shortcuts. It produces
//! `Box<dyn Command>` values that are submitted to the `CommandQueue`.
//!
//! ## Input Pipeline
//!
//! ```text
//! egui::Response  ──╮
//!                   ├── GizmoInputHandler::handle_input()
//! GizmoState     ──╯         │
//!                            ▼
//!                    Vec<Box<dyn Command>>  → CommandQueue
//! ```

use bevy_ecs::entity::Entity;
use egui::Response;
use glam::Vec2;
use strawberry_core::commands::gizmo_commands::{FlipAxis, FlipCommand, GizmoDragCommand};
use strawberry_core::commands::Command;
use strawberry_core::components::Transform2D;
use strawberry_core::gizmo::coordinate_transform::CoordinateTransform;
use strawberry_core::gizmo::gizmo_state::{GizmoMode, GizmoState, ModifierState};
use strawberry_core::gizmo::hit_testing::{self, GizmoHandle};
use strawberry_core::gizmo::snapping::{self, CanvasRect, ElementBounds, SnapAxis};

use super::gizmo_overlay::SelectedElementData;

// ============================================================================
// CONSTANTS
// ============================================================================

/// Nudge distance in scene units for arrow keys
const NUDGE_DISTANCE: f32 = 1.0;
/// Nudge distance with Shift held (scene units)
const NUDGE_DISTANCE_SHIFT: f32 = 10.0;
/// Hit test radius in SCREEN pixels (converted to scene space at runtime)
const HIT_TEST_RADIUS_SCREEN_PX: f32 = 10.0;
/// Rotation handle offset in SCREEN pixels (converted to scene space at runtime)
const ROTATION_HANDLE_OFFSET_SCREEN_PX: f32 = 30.0;
/// Rotation snap increment in degrees (when Shift held)
const ROTATION_SNAP_DEGREES: f32 = 15.0;

// ============================================================================
// GIZMO INPUT RESULT
// ============================================================================

/// Result of processing gizmo input for a single frame.
///
/// The caller (docking.rs) applies these commands and state updates.
pub struct GizmoInputResult {
    /// Commands to submit to the undo/redo system
    pub commands: Vec<Box<dyn Command>>,
    /// Updated gizmo state (caller writes back to ECS)
    pub updated_gizmo_state: Option<GizmoState>,
    /// Currently hovered handle (for cursor + overlay highlight)
    pub hovered_handle: GizmoHandle,
    /// Cursor icon to set on the viewport
    pub cursor_icon: egui::CursorIcon,
    /// Entity to select via viewport click (None = no selection change)
    pub selection_change: Option<Entity>,
}

// ============================================================================
// GIZMO INPUT HANDLER
// ============================================================================

/// Stateless input handler for gizmo interactions.
///
/// Call `handle_input()` each frame with the viewport response and
/// current state. It returns commands and state updates.
pub struct GizmoInputHandler;

impl GizmoInputHandler {
    /// Process mouse/keyboard input and return commands + state updates.
    ///
    /// ## Parameters
    ///
    /// - `ui`: The egui UI context (for reading keyboard state)
    /// - `response`: The viewport widget's egui Response
    /// - `coord`: Coordinate transform for screen↔scene conversion
    /// - `gizmo_state`: Current gizmo interaction state
    /// - `selected`: Currently selected element data
    /// - `selected_entity`: Entity of the primary selected element
    /// - `current_transform`: Current Transform2D of the selected entity
    /// - `source_size`: Source size of the selected entity (for matrix computation)
    /// - `other_elements`: Bounds of non-selected elements (for snapping)
    /// - `canvas_size`: Canvas dimensions in scene coords
    pub fn handle_input(
        ui: &egui::Ui,
        response: &Response,
        coord: &CoordinateTransform,
        gizmo_state: &GizmoState,
        selected: &[SelectedElementData],
        selected_entity: Option<Entity>,
        current_transform: Option<&Transform2D>,
        source_size: Vec2,
        other_elements: &[ElementBounds],
        canvas_size: Vec2,
        visible_clips: &[super::viewport_selection::VisibleClip],
    ) -> GizmoInputResult {
        let mut result = GizmoInputResult {
            commands: Vec::new(),
            updated_gizmo_state: None,
            hovered_handle: GizmoHandle::None,
            cursor_icon: egui::CursorIcon::Default,
            selection_change: None,
        };

        // When nothing is selected, we can't do gizmo handle operations,
        // but we still need to check for viewport click-to-select so users
        // can click clips to select them from the viewport canvas.
        if selected.is_empty() {
            // Check for viewport click-to-select even with no selection
            if response.clicked() || response.drag_started() {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    let scene_pos: Vec2 =
                        coord.screen_to_scene(Vec2::new(pointer_pos.x, pointer_pos.y));
                    result.selection_change = super::viewport_selection::hit_test_viewport_clips(
                        scene_pos,
                        visible_clips,
                    );
                }
            }
            return result;
        }

        let element: &SelectedElementData = &selected[0];

        // Read modifier state
        let modifiers: egui::Modifiers = ui.input(|i| i.modifiers);
        let modifier_state = ModifierState {
            shift_held: modifiers.shift,
            ctrl_held: modifiers.ctrl,
            alt_held: modifiers.alt,
        };

        // ====================================================================
        // 1. HIT TEST — determine which handle the cursor is over
        // ====================================================================
        // Convert screen-pixel radii to scene space using the coordinate transform
        let hit_radius_scene: f32 =
            coord.screen_distance_to_scene_distance(HIT_TEST_RADIUS_SCREEN_PX);
        let rotation_offset_scene: f32 =
            coord.screen_distance_to_scene_distance(ROTATION_HANDLE_OFFSET_SCREEN_PX);

        let hovered_handle: GizmoHandle = if let Some(pointer_pos) = response.hover_pos() {
            let scene_pos: Vec2 = coord.screen_to_scene(Vec2::new(pointer_pos.x, pointer_pos.y));
            hit_testing::test_gizmo_handle_hit(
                scene_pos,
                &element.corners,
                element.anchor_scene,
                hit_radius_scene,
                rotation_offset_scene,
            )
        } else {
            GizmoHandle::None
        };

        result.hovered_handle = hovered_handle;
        result.cursor_icon = super::gizmo_overlay::GizmoOverlay::cursor_for_handle(
            hovered_handle,
            element.rotation_radians,
        );

        // ====================================================================
        // 2. MOUSE PRESS — begin potential drag
        // ====================================================================
        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let scene_pos: Vec2 =
                    coord.screen_to_scene(Vec2::new(pointer_pos.x, pointer_pos.y));

                let pressed_handle: GizmoHandle = hit_testing::test_gizmo_handle_hit(
                    scene_pos,
                    &element.corners,
                    element.anchor_scene,
                    hit_radius_scene,
                    rotation_offset_scene,
                );

                if pressed_handle != GizmoHandle::None {
                    if let Some(transform) = current_transform {
                        let mut new_state: GizmoState = gizmo_state.clone();
                        new_state.begin_press(
                            pressed_handle,
                            scene_pos,
                            transform.clone(),
                            source_size,
                        );
                        new_state.modifier_state = modifier_state;
                        result.updated_gizmo_state = Some(new_state);
                    }
                } else {
                    // No gizmo handle hit — try viewport click-to-select.
                    // Hit-test visible clips in reverse z-order (topmost first).
                    result.selection_change = super::viewport_selection::hit_test_viewport_clips(
                        scene_pos,
                        visible_clips,
                    );
                }
            }
        }

        // ====================================================================
        // 3. MOUSE DRAG — update drag state + compute preview transform
        // ====================================================================
        if response.dragged() && gizmo_state.is_active() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let scene_pos: Vec2 =
                    coord.screen_to_scene(Vec2::new(pointer_pos.x, pointer_pos.y));

                // Calculate screen-space distance for threshold check
                let origin_screen: Vec2 = coord.scene_to_screen(gizmo_state.drag_origin_scene);
                let screen_distance: f32 =
                    Vec2::new(pointer_pos.x, pointer_pos.y).distance(origin_screen);

                let mut new_state: GizmoState = gizmo_state.clone();
                let _is_dragging: bool = new_state.update_drag(scene_pos, screen_distance);
                new_state.modifier_state = modifier_state;

                // Apply snapping for move operations
                if new_state.mode == GizmoMode::Move && new_state.is_dragging() {
                    let delta: Vec2 = new_state.drag_delta_scene();

                    if let Some(ref original) = new_state.original_transform {
                        let moved_center: Vec2 = original.position + delta;
                        let half_size: Vec2 = original.scale * 0.5;

                        let moved_bounds = ElementBounds::new(
                            uuid::Uuid::nil(),
                            moved_center.x - half_size.x,
                            moved_center.y - half_size.y,
                            moved_center.x + half_size.x,
                            moved_center.y + half_size.y,
                        );

                        let canvas_rect = CanvasRect::new(canvas_size.x, canvas_size.y);
                        let snap_results = snapping::find_snaps(
                            &moved_bounds,
                            other_elements,
                            &canvas_rect,
                            hit_radius_scene, // Use dynamically computed hit_radius_scene
                        );

                        // Apply snap adjustments and collect guides
                        let mut snapped_delta: Vec2 = delta;
                        let mut guides = Vec::new();

                        for snap in &snap_results {
                            match snap.axis {
                                SnapAxis::Horizontal => {
                                    snapped_delta.x += snap.snap_delta;
                                }
                                SnapAxis::Vertical => {
                                    snapped_delta.y += snap.snap_delta;
                                }
                            }
                            guides.push(snap.guide_line.clone());
                        }

                        new_state.active_guides = guides;
                        // Adjust drag_current to reflect snapped position
                        new_state.drag_current_scene = new_state.drag_origin_scene + snapped_delta;
                    }
                }

                result.updated_gizmo_state = Some(new_state);
            }
        }

        // ====================================================================
        // 4. MOUSE RELEASE — commit drag, emit GizmoDragCommand
        // ====================================================================
        if response.drag_stopped() && gizmo_state.is_active() {
            if let Some(entity) = selected_entity {
                if gizmo_state.is_dragging() {
                    if let Some(ref original) = gizmo_state.original_transform {
                        let new_transform: Transform2D =
                            Self::compute_final_transform(gizmo_state, original);

                        let command = GizmoDragCommand::new(entity, new_transform);
                        result.commands.push(Box::new(command));
                    }
                }
            }

            // Reset gizmo state
            let mut new_state: GizmoState = GizmoState::new();
            new_state.modifier_state = modifier_state;
            result.updated_gizmo_state = Some(new_state);
        }

        // ====================================================================
        // 5. KEYBOARD SHORTCUTS
        // ====================================================================
        Self::handle_keyboard_shortcuts(ui, &mut result, selected_entity, &modifier_state);

        result
    }

    // ========================================================================
    // PRIVATE METHODS
    // ========================================================================

    /// Compute the final transform after a drag operation.
    fn compute_final_transform(gizmo_state: &GizmoState, original: &Transform2D) -> Transform2D {
        let delta: Vec2 = gizmo_state.drag_delta_scene();
        let mut result: Transform2D = original.clone();
        let source_size: Vec2 = gizmo_state
            .original_source_size
            .unwrap_or(Vec2::new(1920.0, 1080.0));

        match gizmo_state.mode {
            GizmoMode::Move => {
                result.position = original.position + delta;
            }
            GizmoMode::Scale => {
                let scale_delta: Vec2 = Self::compute_scale_delta(
                    gizmo_state.active_handle,
                    delta,
                    original,
                    source_size,
                    gizmo_state.modifier_state.shift_held,
                );

                // Opposite corner math: keep the opposite corner fixed in scene space
                let opposite_uv: Vec2 = match gizmo_state.active_handle {
                    GizmoHandle::TopLeft => Vec2::new(1.0, 1.0),
                    GizmoHandle::TopRight => Vec2::new(0.0, 1.0),
                    GizmoHandle::BottomRight => Vec2::new(0.0, 0.0),
                    GizmoHandle::BottomLeft => Vec2::new(1.0, 0.0),
                    GizmoHandle::Top => Vec2::new(original.anchor.x, 1.0),
                    GizmoHandle::Bottom => Vec2::new(original.anchor.x, 0.0),
                    GizmoHandle::Left => Vec2::new(1.0, original.anchor.y),
                    GizmoHandle::Right => Vec2::new(0.0, original.anchor.y),
                    _ => original.anchor,
                };

                let opposite_local: Vec2 = (opposite_uv - original.anchor) * source_size;

                result.scale = original.scale + scale_delta;
                // Allow downscaling to 5% minimum — prevents zero/negative scale
                result.scale.x = result.scale.x.max(0.05);
                result.scale.y = result.scale.y.max(0.05);

                // Calculate the actual scale delta applied (after clamping)
                let actual_s_delta: Vec2 = result.scale - original.scale;

                // The amount the opposite corner naturally moved due to scale
                let shift_local: Vec2 = actual_s_delta * opposite_local;

                // Rotate this local shift into scene space and subtract it from position
                let rotation_mat: glam::Mat2 = glam::Mat2::from_angle(original.rotation);
                let shift_scene: Vec2 = rotation_mat * shift_local;

                result.position = original.position - shift_scene;
            }
            GizmoMode::Rotate => {
                let angle_delta: f32 = Self::compute_rotation_delta(gizmo_state, original);
                result.rotation = original.rotation + angle_delta;
            }
            _ => {}
        }

        result
    }

    /// Compute scale delta from handle drag.
    ///
    /// Normalizes the pixel-space drag delta by source_size to produce
    /// a proper scale ratio change. E.g., dragging 100px on a 1920px-wide
    /// clip produces scale_delta.x = 100/1920 ≈ 0.052.
    fn compute_scale_delta(
        handle: GizmoHandle,
        delta: Vec2,
        original: &Transform2D,
        source_size: Vec2,
        shift_held: bool,
    ) -> Vec2 {
        let mut scale_delta = Vec2::ZERO;

        let (affects_x, affects_y, invert_x, invert_y) = match handle {
            GizmoHandle::TopLeft => (true, true, true, true),
            GizmoHandle::TopRight => (true, true, false, true),
            GizmoHandle::BottomRight => (true, true, false, false),
            GizmoHandle::BottomLeft => (true, true, true, false),
            GizmoHandle::Top => (false, true, false, true),
            GizmoHandle::Right => (true, false, false, false),
            GizmoHandle::Bottom => (false, true, false, false),
            GizmoHandle::Left => (true, false, true, false),
            _ => (false, false, false, false),
        };

        // Normalize pixel delta by source dimensions to get scale ratio change.
        // A drag of source_width pixels = 1.0 scale change (100%).
        if affects_x && source_size.x > 0.0 {
            let sign: f32 = if invert_x { -1.0 } else { 1.0 };
            scale_delta.x = sign * delta.x / source_size.x;
        }

        if affects_y && source_size.y > 0.0 {
            let sign: f32 = if invert_y { -1.0 } else { 1.0 };
            scale_delta.y = sign * delta.y / source_size.y;
        }

        // Uniform scaling (Shift held) — lock aspect ratio
        if shift_held && affects_x && affects_y {
            let average: f32 = (scale_delta.x.abs() + scale_delta.y.abs()) * 0.5;
            let aspect: f32 = original.scale.x / original.scale.y.max(0.001);
            scale_delta.x = average * scale_delta.x.signum() * aspect;
            scale_delta.y = average * scale_delta.y.signum();
        }

        scale_delta
    }

    /// Compute rotation delta from drag around anchor point.
    fn compute_rotation_delta(gizmo_state: &GizmoState, original: &Transform2D) -> f32 {
        let anchor: Vec2 = original.position;

        let start_offset: Vec2 = gizmo_state.drag_origin_scene - anchor;
        let start_angle: f32 = start_offset.y.atan2(start_offset.x);

        let current_offset: Vec2 = gizmo_state.drag_current_scene - anchor;
        let current_angle: f32 = current_offset.y.atan2(current_offset.x);

        let mut angle_delta: f32 = current_angle - start_angle;

        // Snap to 15° if Shift held
        if gizmo_state.modifier_state.shift_held {
            let snap_radians: f32 = ROTATION_SNAP_DEGREES.to_radians();
            angle_delta = (angle_delta / snap_radians).round() * snap_radians;
        }

        angle_delta
    }

    /// Handle keyboard shortcuts (arrow nudge, flip)
    fn handle_keyboard_shortcuts(
        ui: &egui::Ui,
        result: &mut GizmoInputResult,
        selected_entity: Option<Entity>,
        modifier_state: &ModifierState,
    ) {
        let entity: Entity = match selected_entity {
            Some(e) => e,
            None => return,
        };

        ui.input(|input| {
            let nudge_amount: f32 = if modifier_state.shift_held {
                NUDGE_DISTANCE_SHIFT
            } else {
                NUDGE_DISTANCE
            };

            if input.key_pressed(egui::Key::ArrowLeft) {
                let mut transform = Transform2D::from_position(-nudge_amount, 0.0);
                transform.position.x = -nudge_amount;
                let cmd = GizmoDragCommand::new(entity, transform);
                result.commands.push(Box::new(cmd));
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                let mut transform = Transform2D::from_position(nudge_amount, 0.0);
                transform.position.x = nudge_amount;
                let cmd = GizmoDragCommand::new(entity, transform);
                result.commands.push(Box::new(cmd));
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                let mut transform = Transform2D::from_position(0.0, -nudge_amount);
                transform.position.y = -nudge_amount;
                let cmd = GizmoDragCommand::new(entity, transform);
                result.commands.push(Box::new(cmd));
            }
            if input.key_pressed(egui::Key::ArrowDown) {
                let mut transform = Transform2D::from_position(0.0, nudge_amount);
                transform.position.y = nudge_amount;
                let cmd = GizmoDragCommand::new(entity, transform);
                result.commands.push(Box::new(cmd));
            }

            // Flip shortcuts (H = horizontal, V = vertical)
            if input.key_pressed(egui::Key::H) && !modifier_state.ctrl_held {
                let cmd = FlipCommand::new(entity, FlipAxis::Horizontal);
                result.commands.push(Box::new(cmd));
            }
            if input.key_pressed(egui::Key::V) && !modifier_state.ctrl_held {
                let cmd = FlipCommand::new(entity, FlipAxis::Vertical);
                result.commands.push(Box::new(cmd));
            }
        });
    }
}
