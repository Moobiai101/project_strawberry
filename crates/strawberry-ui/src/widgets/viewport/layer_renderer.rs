//! # Layer Renderer
//!
//! Renders video clip layers as GPU-accelerated textured quads in the viewport.
//!
//! ## Canva-Style Approach
//!
//! Instead of CPU-rasterizing pixels, each clip is rendered as a textured
//! quad (4 vertices) with Transform2D applied to vertex positions. The GPU
//! handles scaling, rotation, interpolation, and alpha blending natively.
//!
//! ## Performance
//!
//! - Zero CPU pixel processing
//! - GPU handles all transform math via vertex shader
//! - Scales to any resolution without CPU cost
//! - Supports rotation without bilinear interpolation code

use egui::{Color32, Mesh, Pos2, Rect, Shape, TextureId};
use glam::Vec2;
use strawberry_core::components::Transform2D;
use strawberry_core::gizmo::coordinate_transform::CoordinateTransform;
use strawberry_core::gizmo::hit_testing::BoundingCorners;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Render a single clip layer as a GPU-accelerated textured quad.
///
/// Transforms the clip's source rectangle through `Transform2D` → scene space,
/// then converts to screen space via `CoordinateTransform`, and paints it as
/// an egui `Mesh` with UV-mapped texture coordinates.
///
/// ## Arguments
///
/// - `painter`: The egui painter to draw on (from viewport response)
/// - `texture_id`: The GPU texture for this clip's current frame
/// - `transform`: The clip's spatial transform (position, scale, rotation, anchor)
/// - `source_size`: The clip's source dimensions in scene units (canvas resolution)
/// - `coord`: The coordinate transform mapping scene space → screen pixels
/// - `opacity`: Layer opacity (0.0 = transparent, 1.0 = opaque)
/// - `clip_rect`: Clipping rectangle to prevent drawing outside viewport
pub fn render_clip_quad(
    painter: &egui::Painter,
    texture_id: TextureId,
    transform: &Transform2D,
    source_size: Vec2,
    coord: &CoordinateTransform,
    opacity: f32,
    clip_rect: Rect,
) {
    // Step 1: Compute the 4 corners in scene space using Transform2D matrix
    let corners: BoundingCorners = BoundingCorners::from_transform(transform, source_size);

    // Step 2: Convert each corner from scene space → screen pixels
    let screen_tl: Vec2 = coord.scene_to_screen(corners.top_left);
    let screen_tr: Vec2 = coord.scene_to_screen(corners.top_right);
    let screen_br: Vec2 = coord.scene_to_screen(corners.bottom_right);
    let screen_bl: Vec2 = coord.scene_to_screen(corners.bottom_left);

    // Step 3: Build the alpha color from opacity
    let alpha: u8 = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    let tint: Color32 = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);

    // Step 4: Build egui Mesh with 4 vertices and 2 triangles
    //
    // Vertex layout:
    //   0 (TL) ---- 1 (TR)
    //   |    \       |
    //   |      \     |
    //   |        \   |
    //   3 (BL) ---- 2 (BR)
    //
    // Triangle 1: TL → TR → BR (indices 0, 1, 2)
    // Triangle 2: TL → BR → BL (indices 0, 2, 3)
    let mut mesh: Mesh = Mesh::with_texture(texture_id);

    // UV coordinates map the full texture (0,0)→(1,1)
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(screen_tl.x, screen_tl.y),
        uv: Pos2::new(0.0, 0.0), // Top-left of texture
        color: tint,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(screen_tr.x, screen_tr.y),
        uv: Pos2::new(1.0, 0.0), // Top-right
        color: tint,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(screen_br.x, screen_br.y),
        uv: Pos2::new(1.0, 1.0), // Bottom-right
        color: tint,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: Pos2::new(screen_bl.x, screen_bl.y),
        uv: Pos2::new(0.0, 1.0), // Bottom-left
        color: tint,
    });

    // Two triangles forming the quad
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);

    // Step 5: Paint the mesh with clipping to viewport bounds
    painter.with_clip_rect(clip_rect).add(Shape::mesh(mesh));
}

/// Render a clip layer using identity transform (fills the canvas).
///
/// Used as fallback when no Transform2D is available — equivalent to
/// the old `painter.image(texture_id, display_rect, uv, Color32::WHITE)`.
pub fn render_clip_quad_identity(
    painter: &egui::Painter,
    texture_id: TextureId,
    display_rect: Rect,
    clip_rect: Rect,
) {
    let uv: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
    painter
        .with_clip_rect(clip_rect)
        .image(texture_id, display_rect, uv, Color32::WHITE);
}
