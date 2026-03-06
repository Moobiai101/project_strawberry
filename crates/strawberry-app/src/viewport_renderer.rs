//! # Viewport Renderer
//!
//! Extracted viewport rendering logic from `docking.rs`.
//!
//! ## Responsibilities
//!
//! - **Clip Layer Quad**: Renders the video texture as a GPU-transformed quad,
//!   computing live preview transforms during gizmo drags.
//! - **Gizmo Overlay**: Builds `SelectedElementData` from ECS, runs the gizmo
//!   input handler, processes selection changes, and draws the overlay.
//!
//! Both functions take `&mut World` to query and mutate ECS state.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use strawberry_core::resources::Selection;

/// Render ALL visible clip layers as independent GPU quads.
///
/// Iterates over `viewport_state.clip_textures` and renders each clip with
/// its own Transform2D resolved from ECS via `StableIdRegistry`. This
/// enables per-clip positioning (Canva/CapCut-style multi-layer viewport).
///
/// Falls back to single-texture rendering when `clip_textures` is empty.
pub fn render_all_clip_layers(
    world: &World,
    ui: &egui::Ui,
    display_rect: egui::Rect,
    canvas_size: glam::Vec2,
    zoom: f32,
    pan: glam::Vec2,
) {
    use strawberry_core::resources::{ClipTextureLayer, ViewportState};

    let viewport_state: &ViewportState = world.resource::<ViewportState>();
    let clip_textures: Vec<ClipTextureLayer> = viewport_state.clip_textures.clone();

    if clip_textures.is_empty() {
        // Backward compat: single composited texture
        if let Some(texture_id) = viewport_state.current_texture {
            render_single_clip_quad(
                world,
                ui,
                texture_id,
                display_rect,
                canvas_size,
                zoom,
                pan,
                None, // No specific clip UUID — use Selection for transform
                viewport_state.source_resolution,
            );
        }
        return;
    }

    // Render each clip layer bottom-to-top (first = background, last = foreground)
    for layer in &clip_textures {
        render_single_clip_quad(
            world,
            ui,
            layer.texture_id,
            display_rect,
            canvas_size,
            zoom,
            pan,
            Some(layer.clip_uuid),
            layer.source_resolution,
        );
    }
}

/// Paint a single clip's video texture as a GPU-accelerated transformed quad.
///
/// When `clip_uuid` is provided, resolves the clip's `Transform2D` from ECS
/// via `StableIdRegistry`. When None, falls back to reading from `Selection`
/// (backward compatibility for single-texture mode).
///
/// During an **active gizmo drag** on the SELECTED clip, computes a live
/// preview transform from `GizmoState`. Non-selected clips always use their
/// stored transform.
fn render_single_clip_quad(
    world: &World,
    ui: &egui::Ui,
    texture_id: egui::TextureId,
    display_rect: egui::Rect,
    canvas_size: glam::Vec2,
    zoom: f32,
    pan: glam::Vec2,
    clip_uuid: Option<uuid::Uuid>,
    source_resolution: glam::UVec2,
) {
    use strawberry_core::components::Transform2D;
    use strawberry_core::gizmo::coordinate_transform::CoordinateTransform;
    use strawberry_core::gizmo::gizmo_state::{DragPhase, GizmoState};

    let painter: &egui::Painter = ui.painter();

    // Build CoordinateTransform — scene coordinate system is the PROJECT CANVAS.
    let coord: CoordinateTransform = CoordinateTransform::new(
        glam::Vec2::new(display_rect.min.x, display_rect.min.y),
        glam::Vec2::new(display_rect.width(), display_rect.height()),
        canvas_size,
        zoom,
        pan,
    );

    // ====================================================================
    // ASPECT-RATIO FIT SCALE (Clipchamp behavior)
    //
    // Uses per-clip source_resolution instead of ViewportState global
    // so each clip's aspect ratio is preserved independently.
    // ====================================================================
    let video_source_size: glam::Vec2 =
        glam::Vec2::new(source_resolution.x as f32, source_resolution.y as f32);

    let fit_scale_vec: glam::Vec2 = compute_aspect_fit_scale(canvas_size, video_source_size);
    let source_size: glam::Vec2 = canvas_size;

    // Resolve the clip's Transform2D from ECS
    let (stored_transform, is_selected): (Option<Transform2D>, bool) =
        resolve_clip_transform(world, clip_uuid);

    // Check if a gizmo drag is active on THIS clip
    let gizmo_state: &GizmoState = world.resource::<GizmoState>();
    let is_dragging: bool = gizmo_state.drag_phase == DragPhase::Dragging;

    let preview_transform: Option<Transform2D> = if is_dragging && is_selected {
        // Only apply drag preview to the selected clip
        compute_drag_preview_transform(gizmo_state, &stored_transform)
    } else {
        stored_transform.clone()
    };

    match preview_transform {
        Some(transform) => {
            let mut fitted_transform: Transform2D = transform.clone();
            fitted_transform.scale *= fit_scale_vec;

            strawberry_ui::render_clip_quad(
                painter,
                texture_id,
                &fitted_transform,
                source_size,
                &coord,
                transform.opacity,
                display_rect,
            );
        }
        None => {
            // No Transform2D — use default with aspect-ratio fit
            let mut default_fitted: Transform2D = Transform2D::default();
            default_fitted.scale *= fit_scale_vec;

            strawberry_ui::render_clip_quad(
                painter,
                texture_id,
                &default_fitted,
                source_size,
                &coord,
                1.0,
                display_rect,
            );
        }
    }
}

/// Resolve a clip's Transform2D from ECS using its UUID.
///
/// Returns the transform and whether this clip is currently selected.
/// When `clip_uuid` is None, falls back to reading from Selection.
fn resolve_clip_transform(
    world: &World,
    clip_uuid: Option<uuid::Uuid>,
) -> (Option<strawberry_core::components::Transform2D>, bool) {
    use strawberry_core::components::Transform2D;
    use strawberry_core::resources::StableIdRegistry;

    let selection = world.resource::<Selection>();

    if let Some(uuid) = clip_uuid {
        // Resolve clip entity via UUID → StableIdRegistry → Entity → Transform2D
        let registry: &StableIdRegistry = world.resource::<StableIdRegistry>();
        if let Some(entity) = registry.get(&uuid) {
            let is_selected: bool = selection.is_selected(entity.to_bits());
            let transform: Option<Transform2D> = world.get::<Transform2D>(entity).cloned();
            (transform, is_selected)
        } else {
            // UUID not found — clip may have been deleted
            (None, false)
        }
    } else {
        // Fallback: use Selection resource (single-texture backward compat)
        let stored_transform: Option<Transform2D> = selection.selected.iter().find_map(|&bits| {
            let entity: Entity = Entity::from_bits(bits);
            world.get::<Transform2D>(entity).cloned()
        });
        let is_selected: bool = !selection.selected.is_empty();
        (stored_transform, is_selected)
    }
}

// ============================================================================
// ASPECT-RATIO FIT HELPERS
// ============================================================================

/// Compute the non-uniform scale vector that adjusts a canvas-sized quad
/// to have the video's native aspect ratio.
///
/// This is the core Clipchamp-style calculation. The returned `Vec2` is
/// multiplied into the Transform2D's scale so the quad geometry matches
/// the video texture's proportions — preventing any stretching.
///
/// ## Returns
///
/// `Vec2(fit_x, fit_y)` where:
/// - `fit_x = (video_w * uniform_fit) / canvas_w`
/// - `fit_y = (video_h * uniform_fit) / canvas_h`
/// - `uniform_fit = min(canvas_w/video_w, canvas_h/video_h)`
///
/// When canvas and video have the same aspect ratio, returns `(1.0, 1.0)`.
pub fn compute_aspect_fit_scale(
    canvas_size: glam::Vec2,
    video_source_size: glam::Vec2,
) -> glam::Vec2 {
    let video_w: f32 = video_source_size.x.max(1.0);
    let video_h: f32 = video_source_size.y.max(1.0);
    let canvas_w: f32 = canvas_size.x.max(1.0);
    let canvas_h: f32 = canvas_size.y.max(1.0);

    // Uniform scale that fits the video inside the canvas
    let uniform_fit: f32 = (canvas_w / video_w).min(canvas_h / video_h);

    // Non-uniform: how much to scale each axis of the canvas-sized quad
    // so its shape matches the video's aspect ratio
    let fit_x: f32 = (video_w * uniform_fit) / canvas_w;
    let fit_y: f32 = (video_h * uniform_fit) / canvas_h;

    glam::Vec2::new(fit_x, fit_y)
}

/// Compute live preview transform during an active gizmo drag.
///
/// Mirrors the math in `GizmoInputHandler::compute_final_transform()`.
fn compute_drag_preview_transform(
    gizmo_state: &strawberry_core::gizmo::gizmo_state::GizmoState,
    stored_transform: &Option<strawberry_core::components::Transform2D>,
) -> Option<strawberry_core::components::Transform2D> {
    use strawberry_core::components::Transform2D;
    use strawberry_core::gizmo::gizmo_state::GizmoMode;
    use strawberry_core::gizmo::hit_testing::GizmoHandle;

    if let Some(ref original) = gizmo_state.original_transform {
        let delta: glam::Vec2 = gizmo_state.drag_delta_scene();
        let mut preview: Transform2D = original.clone();
        let source_size: glam::Vec2 = gizmo_state
            .original_source_size
            .unwrap_or(glam::Vec2::new(1920.0, 1080.0));

        match gizmo_state.mode {
            GizmoMode::Move => {
                preview.position = original.position + delta;
            }
            GizmoMode::Scale => {
                // Inline scale delta: normalize pixel delta by source_size
                let norm_x: f32 = delta.x / source_size.x.max(1.0);
                let norm_y: f32 = delta.y / source_size.y.max(1.0);

                let shift_held = gizmo_state.modifier_state.shift_held;
                let mut scale_delta: glam::Vec2 = gizmo_state
                    .active_handle
                    .scale_sign()
                    .unwrap_or(glam::Vec2::ONE)
                    * glam::Vec2::new(norm_x, norm_y);

                // Uniform scaling when Shift is held
                if shift_held {
                    let uniform: f32 = (scale_delta.x + scale_delta.y) * 0.5;
                    scale_delta = glam::Vec2::new(uniform, uniform);
                }

                // Opposite corner math to keep opposite corner fixed
                let opposite_uv: glam::Vec2 = match gizmo_state.active_handle {
                    GizmoHandle::TopLeft => glam::Vec2::new(1.0, 1.0),
                    GizmoHandle::TopRight => glam::Vec2::new(0.0, 1.0),
                    GizmoHandle::BottomRight => glam::Vec2::new(0.0, 0.0),
                    GizmoHandle::BottomLeft => glam::Vec2::new(1.0, 0.0),
                    GizmoHandle::Top => glam::Vec2::new(original.anchor.x, 1.0),
                    GizmoHandle::Bottom => glam::Vec2::new(original.anchor.x, 0.0),
                    GizmoHandle::Left => glam::Vec2::new(1.0, original.anchor.y),
                    GizmoHandle::Right => glam::Vec2::new(0.0, original.anchor.y),
                    _ => original.anchor,
                };

                let opposite_local: glam::Vec2 = (opposite_uv - original.anchor) * source_size;

                preview.scale = original.scale + scale_delta;
                preview.scale.x = preview.scale.x.max(0.05);
                preview.scale.y = preview.scale.y.max(0.05);

                let actual_s_delta = preview.scale - original.scale;
                let shift_local = actual_s_delta * opposite_local;
                let shift_scene = glam::Mat2::from_angle(original.rotation) * shift_local;

                preview.position = original.position - shift_scene;
            }
            GizmoMode::Rotate => {
                // Compute angle from drag origin→current relative to anchor
                let anchor_scene: glam::Vec2 = original
                    .to_matrix(source_size)
                    .transform_point2(original.anchor * source_size);
                let start_vec: glam::Vec2 = gizmo_state.drag_origin_scene - anchor_scene;
                let current_vec: glam::Vec2 = gizmo_state.drag_current_scene - anchor_scene;
                let angle_delta: f32 =
                    current_vec.y.atan2(current_vec.x) - start_vec.y.atan2(start_vec.x);
                preview.rotation = original.rotation + angle_delta;
            }
            _ => {}
        }
        Some(preview)
    } else {
        stored_transform.clone()
    }
}

// ============================================================================
// GIZMO OVERLAY RENDERING
// ============================================================================

/// Render gizmo overlay on top of the viewport and process gizmo input.
///
/// Queries ECS for ALL selected entities (multi-select), builds
/// `SelectedElementData` for each, gathers non-selected element bounds
/// for snapping, then runs the gizmo input handler and overlay renderer.
pub fn render_gizmo_overlay(
    world: &mut World,
    ui: &egui::Ui,
    response: &egui::Response,
    display_rect: egui::Rect,
    canvas_size: glam::Vec2,
    zoom: f32,
    pan: glam::Vec2,
) {
    use strawberry_core::components::{StableId, Transform2D};
    use strawberry_core::gizmo::coordinate_transform::CoordinateTransform;
    use strawberry_core::gizmo::gizmo_state::GizmoState;
    use strawberry_core::gizmo::hit_testing::BoundingCorners;
    use strawberry_core::gizmo::snapping::ElementBounds;
    use strawberry_core::resources::CommandQueue;
    use strawberry_ui::{GizmoInputHandler, GizmoOverlay, SelectedElementData};

    // Build CoordinateTransform using the DISPLAY RECT (aspect-fitted area
    // where the video actually renders), NOT response.rect (full panel).
    // This ensures the gizmo exactly matches the video's screen position.
    let coord = CoordinateTransform::new(
        glam::Vec2::new(display_rect.min.x, display_rect.min.y),
        glam::Vec2::new(display_rect.width(), display_rect.height()),
        canvas_size,
        zoom,
        pan,
    );

    // ====================================================================
    // ASPECT-RATIO FIT SCALE (must match render_clip_layer_quad)
    // Gizmo handles must be positioned at the SAME locations as the
    // rendered video quad, so we apply the same non-uniform fit scale.
    // ====================================================================
    let viewport_state = world.resource::<strawberry_core::resources::ViewportState>();
    let video_source_size: glam::Vec2 = glam::Vec2::new(
        viewport_state.source_resolution.x as f32,
        viewport_state.source_resolution.y as f32,
    );
    let fit_scale_vec: glam::Vec2 = compute_aspect_fit_scale(canvas_size, video_source_size);

    // Gather selected entity bits from ECS Selection resource
    let selection = world.resource::<Selection>();
    let selected_bits: Vec<u64> = selection.selected.iter().copied().collect();
    let primary_bits: Option<u64> = selection.primary.or_else(|| selected_bits.first().copied());

    // ================================================================
    // Build SelectedElementData for EVERY selected entity
    // ================================================================
    let mut selected_data: Vec<SelectedElementData> = Vec::with_capacity(selected_bits.len());
    let mut primary_entity: Option<Entity> = None;
    let mut primary_transform: Option<Transform2D> = None;
    let mut primary_source_size: glam::Vec2 = canvas_size;

    for &bits in &selected_bits {
        let entity: Entity = Entity::from_bits(bits);

        // Migration: ensure clips created before Transform2D was part of the
        // spawn bundle still get a default transform for gizmo rendering.
        // position=(0,0) means source fills (0,0)→(W,H) = canvas bounds.
        if world.get::<Transform2D>(entity).is_none()
            && world
                .get::<strawberry_core::components::Clip>(entity)
                .is_some()
        {
            world.entity_mut(entity).insert(Transform2D::default());
        }

        let transform_opt = world.get::<Transform2D>(entity).cloned();

        // Use canvas_size as source_size — matches the coordinate system.
        // Apply non-uniform fit scale for correct aspect-ratio matching.
        let source_size: glam::Vec2 = canvas_size;

        if let Some(ref transform) = transform_opt {
            // Apply aspect-ratio fit scale to match the rendered video position
            let mut fitted_transform: Transform2D = transform.clone();
            fitted_transform.scale *= fit_scale_vec;

            let corners: BoundingCorners =
                BoundingCorners::from_transform(&fitted_transform, source_size);
            let anchor_scene: glam::Vec2 = fitted_transform
                .to_matrix(source_size)
                .transform_point2(fitted_transform.anchor * source_size);

            selected_data.push(SelectedElementData {
                corners,
                anchor_scene,
                rotation_radians: transform.rotation,
            });
        }

        // Track the primary entity for input handler commands
        if Some(bits) == primary_bits {
            primary_entity = Some(entity);
            primary_transform = transform_opt;
            primary_source_size = source_size;
        }
    }

    // ================================================================
    // Gather non-selected element bounds for snapping
    // Only computed during active drags to avoid per-frame query cost
    // ================================================================
    let gizmo_state: GizmoState = world.resource::<GizmoState>().clone();
    let is_dragging: bool = matches!(
        gizmo_state.drag_phase,
        strawberry_core::gizmo::gizmo_state::DragPhase::Dragging { .. }
    );

    let other_elements: Vec<ElementBounds> = if is_dragging && !selected_bits.is_empty() {
        // Pre-compute selected UUIDs for O(1) lookup
        let selected_uuids: std::collections::HashSet<uuid::Uuid> = selected_bits
            .iter()
            .filter_map(|&bits| {
                world
                    .get::<StableId>(Entity::from_bits(bits))
                    .map(|sid| sid.0)
            })
            .collect();

        let mut others: Vec<ElementBounds> = Vec::new();
        let mut query = world.query::<(&Transform2D, &StableId)>();

        for (transform, stable_id) in query.iter(world) {
            if selected_uuids.contains(&stable_id.0) {
                continue;
            }

            // Use aspect-fit-adjusted bounds for snapping
            let mut fitted: Transform2D = transform.clone();
            fitted.scale *= fit_scale_vec;
            let half: glam::Vec2 = canvas_size * fitted.scale * 0.5;
            let center: glam::Vec2 = fitted.position;

            others.push(ElementBounds::new(
                stable_id.0,
                center.x - half.x,
                center.y - half.y,
                center.x + half.x,
                center.y + half.y,
            ));
        }
        others
    } else {
        Vec::new()
    };

    // ================================================================
    // Build visible_clips list for viewport click-to-select
    // All clips with Transform2D are considered visible in the viewport
    // ================================================================
    let visible_clips: Vec<strawberry_ui::VisibleClip> = {
        let mut clips: Vec<strawberry_ui::VisibleClip> = Vec::new();
        let mut query = world.query::<(Entity, &Transform2D)>();
        for (entity, transform) in query.iter(world) {
            // Apply aspect-ratio fit scale for hit testing
            let mut fitted: Transform2D = transform.clone();
            fitted.scale *= fit_scale_vec;
            let corners: BoundingCorners = BoundingCorners::from_transform(&fitted, canvas_size);
            clips.push(strawberry_ui::VisibleClip { entity, corners });
        }
        clips
    };

    // Run gizmo input handler
    let input_result = GizmoInputHandler::handle_input(
        ui,
        response,
        &coord,
        &gizmo_state,
        &selected_data,
        primary_entity,
        primary_transform.as_ref(),
        primary_source_size,
        &other_elements,
        canvas_size,
        &visible_clips,
    );

    // Apply cursor from gizmo
    if input_result.cursor_icon != egui::CursorIcon::Default && !selected_data.is_empty() {
        ui.ctx().set_cursor_icon(input_result.cursor_icon);
    }

    // Handle viewport click-to-select
    if let Some(clicked_entity) = input_result.selection_change {
        let mut selection = world.resource_mut::<Selection>();
        let bits: u64 = clicked_entity.to_bits();
        selection.selected.clear();
        selection.selected.insert(bits);
        selection.primary = Some(bits);
    }

    // Submit commands to ECS CommandQueue
    if !input_result.commands.is_empty() {
        let command_queue = world.resource::<CommandQueue>();
        for cmd in input_result.commands {
            command_queue.submit(cmd);
        }
    }

    // Update GizmoState resource
    if let Some(new_state) = input_result.updated_gizmo_state {
        *world.resource_mut::<GizmoState>() = new_state;
    }

    // Draw gizmo overlay on top of viewport
    let gizmo_state_for_draw: GizmoState = world.resource::<GizmoState>().clone();
    GizmoOverlay::draw(
        ui.painter(),
        &coord,
        &selected_data,
        input_result.hovered_handle,
        &gizmo_state_for_draw.active_guides,
        &gizmo_state_for_draw.distance_indicators,
    );
}
