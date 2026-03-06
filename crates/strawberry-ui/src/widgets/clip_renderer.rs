//! # Batched Clip Renderer
//!
//! Production-grade batched rendering for timeline clips.
//! Based on Adobe Premiere and DaVinci Resolve rendering patterns.
//!
//! ## Performance
//!
//! Traditional approach: 7+ draw calls per clip = 7000+ calls for 1000 clips
//! Batched approach: ~10 draw calls total (one per visual layer)
//!
//! This is the single biggest optimization for rendering 1000+ clips
//! smoothly at 60fps.
//!
//! ## Architecture (Adobe/DaVinci style)
//!
//! Visual layers are batched separately and rendered in order:
//! 1. Shadows (background layer)
//! 2. Clip backgrounds (or skip if filmstrip)
//! 3. Filmstrip thumbnails
//! 4. Borders (normal, hover, selected)
//! 5. Trim handles
//! 6. Text labels (clip names)
//!
//! Each layer collects all clips' geometry into a single mesh,
//! submitting one draw call per layer instead of one per clip.

use egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Painter, Rect, Rounding, Shape, Stroke};
use std::time::Duration;

// ============================================================================
// VISIBLE CLIP RECT
// ============================================================================

/// Pre-computed visible clip rectangle for batched rendering.
///
/// Contains all rendering data needed without re-querying the clip.
/// This struct is collected in a first pass and then batched by layer.
#[derive(Debug, Clone)]
pub struct VisibleClipRect {
    /// Screen-space rectangle for the clip body
    pub rect: Rect,
    /// Fill color for clip background
    pub fill_color: Color32,
    /// Border color
    pub border_color: Color32,
    /// Border width
    pub border_width: f32,
    /// Whether to draw shadow
    pub draw_shadow: bool,
    /// Clip name for text rendering
    pub name: String,
    /// Entity bits for identification
    pub entity_bits: u64,
    /// Track height (for handles)
    pub track_height: f32,
    /// Whether filmstrip is available (skip solid background if true)
    pub has_filmstrip: bool,
    /// Asset UUID for filmstrip lookup
    pub asset_uuid: Option<uuid::Uuid>,
    /// Source in/out for filmstrip positioning
    pub source_in: Duration,
    pub source_out: Duration,
    /// Whether this clip is currently selected
    pub is_selected: bool,
}

// ============================================================================
// BATCHED CLIP RENDERER
// ============================================================================

/// Production-grade batched clip renderer following Adobe/DaVinci patterns.
///
/// Collects all visible clips and renders them in batched layers:
/// 1. One mesh for all shadows
/// 2. One mesh for all backgrounds
/// 3. Filmstrips (already optimized via FilmstripRenderer)
/// 4. One mesh for all borders
/// 5. One mesh for all handles
/// 6. Text batch
///
/// This reduces draw calls from O(n*7) to O(1) per layer.
#[derive(Default)]
pub struct BatchedClipRenderer {
    /// Collected visible clips for batching
    visible_clips: Vec<VisibleClipRect>,
}

impl BatchedClipRenderer {
    /// Create a new batched renderer
    pub fn new() -> Self {
        Self {
            visible_clips: Vec::with_capacity(256),
        }
    }

    /// Clear collected clips (call at start of frame)
    pub fn clear(&mut self) {
        self.visible_clips.clear();
    }

    /// Add a visible clip to the batch
    pub fn add_clip(&mut self, clip: VisibleClipRect) {
        self.visible_clips.push(clip);
    }

    /// Get the collected clips (for filmstrip rendering)
    pub fn clips(&self) -> &[VisibleClipRect] {
        &self.visible_clips
    }

    // ========================================================================
    // BATCHED RENDER PASSES
    // ========================================================================

    /// Render all shadows in one batch
    ///
    /// Shadows are rendered first (back layer) with slight offset.
    pub fn render_shadows(&self, painter: &Painter) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut mesh = Mesh::default();

        for clip in &self.visible_clips {
            if !clip.draw_shadow {
                continue;
            }

            let shadow_rect = clip.rect.translate(vec2(1.5, 1.5));
            let shadow_color = Color32::from_rgba_unmultiplied(0, 0, 0, 40);

            add_rounded_rect_to_mesh(&mut mesh, shadow_rect, 4.0, shadow_color);
        }

        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }

    /// Render all clip backgrounds in one batch
    ///
    /// Skips clips with filmstrip textures (they render their own background).
    pub fn render_backgrounds(&self, painter: &Painter) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut mesh = Mesh::default();

        for clip in &self.visible_clips {
            if clip.has_filmstrip {
                continue; // Filmstrip renders its own background
            }

            add_rounded_rect_to_mesh(&mut mesh, clip.rect, 4.0, clip.fill_color);
        }

        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }

    /// Render selection overlays for selected clips
    ///
    /// Draws two layers for selected clips:
    /// 1. A semi-transparent cyan glow (expanded 2px around the clip)
    /// 2. A 10% white brightness overlay on top of the clip body
    ///
    /// Call this AFTER filmstrips and BEFORE borders so the glow
    /// appears under the border stroke.
    pub fn render_selection_overlays(&self, painter: &Painter) {
        let selected_clips: Vec<&VisibleClipRect> = self
            .visible_clips
            .iter()
            .filter(|c| c.is_selected)
            .collect();

        if selected_clips.is_empty() {
            return;
        }

        // Layer 1: Outer glow (cyan, expanded 2px around clip)
        let mut glow_mesh = Mesh::default();
        for clip in &selected_clips {
            let glow_rect = clip.rect.expand(2.0);
            add_rounded_rect_to_mesh(
                &mut glow_mesh,
                glow_rect,
                6.0, // Slightly larger radius than clip's 4.0
                Color32::from_rgba_unmultiplied(78, 182, 196, 30),
            );
        }
        if !glow_mesh.is_empty() {
            painter.add(Shape::mesh(glow_mesh));
        }

        // Layer 2: Very subtle brightness overlay (~3% white on clip body)
        // Kept minimal so filmstrip thumbnails remain clearly visible
        let mut overlay_mesh = Mesh::default();
        for clip in &selected_clips {
            add_rounded_rect_to_mesh(
                &mut overlay_mesh,
                clip.rect,
                4.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, 8),
            );
        }
        if !overlay_mesh.is_empty() {
            painter.add(Shape::mesh(overlay_mesh));
        }
    }

    /// Render all borders in one batch
    ///
    /// Uses stroke shapes - these can't be meshed as easily but
    /// we can still batch by collecting into a single Vec<Shape>.
    pub fn render_borders(&self, painter: &Painter) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut shapes = Vec::with_capacity(self.visible_clips.len());

        for clip in &self.visible_clips {
            let stroke = Stroke::new(clip.border_width, clip.border_color);
            shapes.push(Shape::rect_stroke(clip.rect, Rounding::same(4.0), stroke));
        }

        painter.extend(shapes);
    }

    /// Render all trim handles in one batch
    ///
    /// Left and right handles for each clip.
    pub fn render_handles(&self, painter: &Painter) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut mesh = Mesh::default();

        for clip in &self.visible_clips {
            // Handle transparency based on state
            let handle_alpha = if clip.draw_shadow { 0.7 } else { 0.35 };
            let handle_color = Color32::from_rgb(255, 255, 255).gamma_multiply(handle_alpha);

            let handle_width = 5.0;
            let height = clip.track_height - 4.0; // Match clip padding

            // Left handle
            let left_handle = Rect::from_min_size(
                pos2(clip.rect.left(), clip.rect.top()),
                vec2(handle_width, height),
            );
            add_partial_rounded_rect_to_mesh(
                &mut mesh,
                left_handle,
                4.0, // NW
                4.0, // SW
                0.0, // NE
                0.0, // SE
                handle_color,
            );

            // Right handle
            let right_handle = Rect::from_min_size(
                pos2(clip.rect.right() - handle_width, clip.rect.top()),
                vec2(handle_width, height),
            );
            add_partial_rounded_rect_to_mesh(
                &mut mesh,
                right_handle,
                0.0, // NW
                0.0, // SW
                4.0, // NE
                4.0, // SE
                handle_color,
            );
        }

        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }

    /// Render all clip names as text batch
    ///
    /// Text rendering needs painter_at for clipping, so we still
    /// have individual calls but minimize overhead.
    pub fn render_text(&self, painter: &Painter, text_color: Color32) {
        for clip in &self.visible_clips {
            let width = clip.rect.width();
            if width > 40.0 {
                let text_rect = clip.rect.shrink2(vec2(10.0, 4.0));
                painter.text(
                    pos2(text_rect.left(), text_rect.center().y),
                    Align2::LEFT_CENTER,
                    &clip.name,
                    FontId::proportional(11.0),
                    text_color,
                );
            }
        }
    }
}

// ============================================================================
// MESH HELPERS
// ============================================================================

/// Add a rounded rectangle to a mesh (uniform corner radius).
///
/// This is a simplified version that approximates rounded corners.
/// For high-quality corners, more triangles would be needed.
fn add_rounded_rect_to_mesh(mesh: &mut Mesh, rect: Rect, radius: f32, color: Color32) {
    // For simplicity, we use a simple rectangle approximation.
    // A fuller implementation would triangulate the rounded corners.
    // egui's Mesh doesn't have a direct add_rect method with color,
    // so we manually add vertices and triangles.

    let idx_base = mesh.vertices.len() as u32;

    // Four corners (inset by radius for visual approximation)
    let inset = radius * 0.3; // Slight visual inset to approximate rounded look

    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.left() + inset, rect.top() + inset),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.right() - inset, rect.top() + inset),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.right() - inset, rect.bottom() - inset),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.left() + inset, rect.bottom() - inset),
        uv: egui::epaint::WHITE_UV,
        color,
    });

    // Two triangles forming the rectangle
    mesh.indices.push(idx_base);
    mesh.indices.push(idx_base + 1);
    mesh.indices.push(idx_base + 2);
    mesh.indices.push(idx_base);
    mesh.indices.push(idx_base + 2);
    mesh.indices.push(idx_base + 3);
}

/// Add a partially rounded rectangle to a mesh (different corner radii).
fn add_partial_rounded_rect_to_mesh(
    mesh: &mut Mesh,
    rect: Rect,
    nw: f32,
    sw: f32,
    ne: f32,
    se: f32,
    color: Color32,
) {
    let idx_base = mesh.vertices.len() as u32;

    // Inset each corner based on its radius
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.left() + nw * 0.3, rect.top() + nw * 0.3),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.right() - ne * 0.3, rect.top() + ne * 0.3),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.right() - se * 0.3, rect.bottom() - se * 0.3),
        uv: egui::epaint::WHITE_UV,
        color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: pos2(rect.left() + sw * 0.3, rect.bottom() - sw * 0.3),
        uv: egui::epaint::WHITE_UV,
        color,
    });

    mesh.indices.push(idx_base);
    mesh.indices.push(idx_base + 1);
    mesh.indices.push(idx_base + 2);
    mesh.indices.push(idx_base);
    mesh.indices.push(idx_base + 2);
    mesh.indices.push(idx_base + 3);
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_clip() {
        let mut renderer = BatchedClipRenderer::new();
        assert_eq!(renderer.clips().len(), 0);

        renderer.add_clip(VisibleClipRect {
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0)),
            fill_color: Color32::RED,
            border_color: Color32::WHITE,
            border_width: 1.0,
            draw_shadow: false,
            name: "Test".to_string(),
            entity_bits: 0,
            track_height: 35.0,
            has_filmstrip: false,
            asset_uuid: None,
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(5),
            is_selected: false,
        });

        assert_eq!(renderer.clips().len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut renderer = BatchedClipRenderer::new();
        renderer.add_clip(VisibleClipRect {
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0)),
            fill_color: Color32::RED,
            border_color: Color32::WHITE,
            border_width: 1.0,
            draw_shadow: false,
            name: "Test".to_string(),
            entity_bits: 0,
            track_height: 35.0,
            has_filmstrip: false,
            asset_uuid: None,
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(5),
            is_selected: false,
        });

        renderer.clear();
        assert_eq!(renderer.clips().len(), 0);
    }
}
