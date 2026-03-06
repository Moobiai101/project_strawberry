//! # Viewport Widget Module
//!
//! Custom egui widget for video preview display with gizmo overlay.
//!
//! ## Architecture
//!
//! ```text
//! viewport/
//! ├── mod.rs             ← ViewportWidget (frame display + zoom/pan)
//! ├── gizmo_overlay.rs   ← GizmoOverlay (bbox, handles, guides, cursors)
//! ├── gizmo_input.rs     ← GizmoInputHandler (mouse/keyboard → Commands)
//! └── tests.rs           ← Unit tests
//! ```
//!
//! ## Data Flow
//!
//! ```text
//! docking.rs → ViewportWidget::new().with_texture().ui()
//!                                          │
//!                                          ▼
//!                                   calculate_display_rect()
//!                                          │
//!               ┌──────────────────────────┼──────────────────────────┐
//!               │                          │                          │
//!               ▼                          ▼                          ▼
//!        draw_frame()             GizmoOverlay::draw()      GizmoInputHandler
//!        (texture/placeholder)    (bbox, handles, guides)   (mouse→Commands)
//! ```

mod gizmo_input;
mod gizmo_overlay;
mod layer_renderer;
#[cfg(test)]
mod tests;
mod viewport_selection;

pub use gizmo_input::{GizmoInputHandler, GizmoInputResult};
pub use gizmo_overlay::{GizmoOverlay, SelectedElementData};
pub use layer_renderer::{render_clip_quad, render_clip_quad_identity};
pub use viewport_selection::{hit_test_viewport_clips, VisibleClip};

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, TextureId, Ui, Vec2, Widget};
use glam::UVec2;

/// Viewport widget for video preview
///
/// Displays a single video frame with proper aspect ratio,
/// zoom/pan support, and status overlays. Gizmo rendering
/// and input are handled by companion modules.
pub struct ViewportWidget {
    /// Texture to display (None = show placeholder)
    texture_id: Option<TextureId>,
    /// Whether to skip painting the texture (used when external renderer paints it)
    skip_texture: bool,
    /// Source resolution for aspect ratio calculation
    source_resolution: UVec2,
    /// Current zoom level (1.0 = fit to viewport)
    zoom: f32,
    /// Pan offset (normalized, 0-1 range)
    pan: glam::Vec2,
    /// Is currently loading a frame?
    is_loading: bool,
    /// Error message to display
    error: Option<String>,
    /// Whether to show safe area guides (title safe, action safe)
    show_safe_areas: bool,
    /// Background color for letterbox areas
    background_color: Color32,
    /// Is playhead in a gap? (show black frame instead of placeholder)
    in_gap: bool,
}

impl ViewportWidget {
    /// Create a new viewport widget
    pub fn new() -> Self {
        Self {
            texture_id: None,
            skip_texture: false,
            source_resolution: UVec2::new(1920, 1080),
            zoom: 1.0,
            pan: glam::Vec2::ZERO,
            is_loading: false,
            error: None,
            show_safe_areas: false,
            background_color: Color32::from_rgb(20, 20, 25),
            in_gap: false,
        }
    }

    /// Set the texture to display
    pub fn with_texture(mut self, texture_id: TextureId) -> Self {
        self.texture_id = Some(texture_id);
        self
    }

    /// Whether to skip painting the texture itself (e.g. if painted externally)
    pub fn with_skip_texture(mut self, skip: bool) -> Self {
        self.skip_texture = skip;
        self
    }

    /// Set source resolution for aspect ratio
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.source_resolution = UVec2::new(width, height);
        self
    }

    /// Set zoom level
    pub fn with_zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }

    /// Set pan offset
    pub fn with_pan(mut self, pan: glam::Vec2) -> Self {
        self.pan = pan;
        self
    }

    /// Set loading state
    pub fn with_loading(mut self, is_loading: bool) -> Self {
        self.is_loading = is_loading;
        self
    }

    /// Set error message
    pub fn with_error(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }

    /// Enable safe area guides
    pub fn with_safe_areas(mut self, show: bool) -> Self {
        self.show_safe_areas = show;
        self
    }

    /// Set gap mode (show black frame instead of placeholder)
    pub fn with_in_gap(mut self, in_gap: bool) -> Self {
        self.in_gap = in_gap;
        self
    }

    /// Calculate display rect maintaining aspect ratio
    ///
    /// Returns the rect where the video frame should be rendered,
    /// centered in the available space with proper letterboxing.
    /// This is also used by gizmo_overlay to map scene coordinates
    /// to screen coordinates.
    pub fn calculate_display_rect(&self, available: Rect) -> Rect {
        let source_aspect: f32 =
            self.source_resolution.x as f32 / self.source_resolution.y.max(1) as f32;
        let available_aspect: f32 = available.width() / available.height().max(1.0);

        let (display_width, display_height): (f32, f32) = if source_aspect > available_aspect {
            // Source is wider - fit to width
            (available.width(), available.width() / source_aspect)
        } else {
            // Source is taller - fit to height
            (available.height() * source_aspect, available.height())
        };

        // Apply zoom
        let zoomed_width: f32 = display_width * self.zoom;
        let zoomed_height: f32 = display_height * self.zoom;

        // Center in available space with pan offset
        let center: Pos2 = available.center();
        let pan_pixels: Vec2 = Vec2::new(
            self.pan.x * available.width(),
            self.pan.y * available.height(),
        );

        Rect::from_center_size(center + pan_pixels, Vec2::new(zoomed_width, zoomed_height))
    }

    /// Draw placeholder when no texture is loaded
    ///
    /// Layout adapts dynamically to the active aspect ratio's canvas shape.
    /// For narrow canvases (e.g. 9:16 vertical), text wraps and icon scales down.
    fn draw_placeholder(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();

        // Dark background
        painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 30, 35));

        // Border
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(60, 60, 70)));

        // Center icon and text
        let center: Pos2 = rect.center();
        let canvas_width: f32 = rect.width();

        // Scale icon based on canvas width (smaller for narrow canvases)
        let icon_size: f32 = (canvas_width * 0.25).clamp(24.0, 48.0);
        let icon_rect: Rect = Rect::from_center_size(
            Pos2::new(center.x, center.y - icon_size * 0.6),
            Vec2::splat(icon_size),
        );
        painter.rect_filled(icon_rect, 4.0, Color32::from_rgb(80, 80, 90));

        // Perforations (scale with icon)
        let perf_w: f32 = (icon_size * 0.125).max(3.0);
        let perf_h: f32 = (icon_size * 0.167).max(4.0);
        let perf_count: usize = if icon_size > 32.0 { 3 } else { 2 };
        let perf_spacing: f32 = (icon_size - perf_h) / perf_count as f32;

        for i in 0..perf_count {
            let y: f32 = icon_rect.min.y + perf_h * 0.5 + i as f32 * perf_spacing;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(icon_rect.min.x + perf_w * 0.5, y),
                    Vec2::new(perf_w, perf_h),
                ),
                1.0,
                Color32::from_rgb(30, 30, 35),
            );
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(icon_rect.max.x - perf_w * 1.5, y),
                    Vec2::new(perf_w, perf_h),
                ),
                1.0,
                Color32::from_rgb(30, 30, 35),
            );
        }

        // Adaptive text: render each line centered individually
        let font_size: f32 = if canvas_width < 200.0 { 11.0 } else { 13.0 };
        let font: egui::FontId = egui::FontId::proportional(font_size);
        let text_color: Color32 = Color32::from_rgb(140, 140, 150);
        let line_spacing: f32 = font_size + 4.0;

        let lines: &[&str] = if canvas_width < 150.0 {
            &["Drop video", "here"]
        } else {
            &["Drop video here", "or import from File menu"]
        };

        let text_start_y: f32 = center.y + icon_size * 0.5 + 8.0;
        for (i, line) in lines.iter().enumerate() {
            painter.text(
                Pos2::new(center.x, text_start_y + i as f32 * line_spacing),
                egui::Align2::CENTER_TOP,
                *line,
                font.clone(),
                text_color,
            );
        }
    }

    /// Draw black frame for timeline gaps (industry standard: gaps = black)
    fn draw_black_frame(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();
        // Pure black fill - matches DaVinci Resolve / Premiere behavior for gaps
        painter.rect_filled(rect, 0.0, Color32::BLACK);
    }

    /// Draw loading indicator
    fn draw_loading(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();
        let center: Pos2 = rect.center();

        // Animated spinner
        let time: f64 = ui.ctx().input(|i| i.time);
        let angle: f32 = (time * 2.0) as f32;
        let radius: f32 = 20.0;

        for i in 0..8 {
            let segment_angle: f32 = angle + (i as f32 * std::f32::consts::PI / 4.0);
            let alpha: u8 = 50 + (i * 25).min(255) as u8;
            let color: Color32 = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);

            let x: f32 = center.x + segment_angle.cos() * radius;
            let y: f32 = center.y + segment_angle.sin() * radius;

            painter.circle_filled(Pos2::new(x, y), 3.0, color);
        }

        // Request repaint for animation
        ui.ctx().request_repaint();
    }

    /// Draw error message
    fn draw_error(&self, ui: &mut Ui, rect: Rect, error: &str) {
        let painter = ui.painter();
        let center: Pos2 = rect.center();

        // Error icon
        painter.text(
            Pos2::new(center.x, center.y - 20.0),
            egui::Align2::CENTER_CENTER,
            "⚠",
            egui::FontId::proportional(32.0),
            Color32::from_rgb(220, 80, 80),
        );

        // Error message
        painter.text(
            Pos2::new(center.x, center.y + 15.0),
            egui::Align2::CENTER_CENTER,
            error,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(180, 100, 100),
        );
    }

    /// Draw safe area guides
    fn draw_safe_areas(&self, painter: &egui::Painter, rect: Rect) {
        // Title safe (inner 80%)
        let title_safe: Rect = Rect::from_center_size(rect.center(), rect.size() * 0.8);
        painter.rect_stroke(
            title_safe,
            0.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(100, 200, 100, 100)),
        );

        // Action safe (inner 90%)
        let action_safe: Rect = Rect::from_center_size(rect.center(), rect.size() * 0.9);
        painter.rect_stroke(
            action_safe,
            0.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 100, 100)),
        );
    }
}

impl Default for ViewportWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ViewportWidget {
    fn ui(self, ui: &mut Ui) -> Response {
        // Allocate the full available space
        let available_rect: Rect = ui.available_rect_before_wrap();
        let (response, painter) =
            ui.allocate_painter(available_rect.size(), Sense::click_and_drag());
        let rect: Rect = response.rect;

        // Draw background (letterbox/pillarbox areas — dark gray)
        painter.rect_filled(rect, 0.0, self.background_color);

        // Calculate where the video should be displayed
        let display_rect: Rect = self.calculate_display_rect(rect);

        // Draw canvas area (pure black — distinct from letterbox areas)
        // This makes the project canvas shape clearly visible when the
        // aspect ratio differs from the viewport panel shape (Clipchamp behavior).
        painter.rect_filled(display_rect, 0.0, Color32::BLACK);

        // Render based on state
        if let Some(error) = &self.error {
            // Error state
            self.draw_placeholder(ui, display_rect);
            self.draw_error(ui, display_rect, error);
        } else if self.is_loading {
            // Loading state
            if let Some(texture_id) = self.texture_id {
                // Show last frame while loading
                let uv: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                painter.image(texture_id, display_rect, uv, Color32::WHITE);
            } else {
                // No texture yet - show BLACK background (not placeholder)
                // This prevents "drop video here" from showing during gap→clip transitions
                self.draw_black_frame(ui, display_rect);
            }
            self.draw_loading(ui, display_rect);
        } else if let Some(texture_id) = self.texture_id {
            // Normal display
            if !self.skip_texture {
                let uv: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                painter.image(texture_id, display_rect, uv, Color32::WHITE);
            }
        } else if self.in_gap {
            // Gap between clips - show BLACK frame (industry standard)
            self.draw_black_frame(ui, display_rect);
        } else {
            // No content - show placeholder ("drop video here")
            self.draw_placeholder(ui, display_rect);
        }

        // Safe area guides (on top of everything)
        if self.show_safe_areas && self.texture_id.is_some() {
            self.draw_safe_areas(&painter, display_rect);
        }

        response
    }
}
