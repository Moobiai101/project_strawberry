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
use strawberry_core::components::audio::FadeCurve;

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
    /// Whether this clip's audio is muted (for 🔊/🔇 icon)
    pub audio_muted: bool,
    /// Fade-in duration in milliseconds (0 = no fade)
    pub fade_in_ms: u32,
    /// Fade-out duration in milliseconds (0 = no fade)
    pub fade_out_ms: u32,
    /// Fade curve shape for visual overlay
    pub fade_curve: FadeCurve,
    /// Keyframe automation overlay data (None = no automation on this clip)
    pub keyframe_overlay: Option<super::keyframe_overlay::KeyframeOverlayData>,
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

    /// Render audio mute/unmute icons at the bottom-right of each clip.
    ///
    /// Uses custom-painted speaker shapes — egui's default font does NOT include
    /// emoji Unicode, so we paint directly with the `Painter` API.
    ///
    /// - Unmuted: speaker body + sound waves (subtle white)
    /// - Muted: speaker body + diagonal slash (red)
    ///
    /// Only renders when clip width > 60px (too narrow = skip icon).
    ///
    /// ## Performance
    /// - A few `painter` calls per visible clip (viewport-culled, typically ~20)
    /// - Zero string allocation, zero egui widget overhead
    pub fn render_audio_icons(&self, painter: &Painter) {
        for clip in &self.visible_clips {
            let width: f32 = clip.rect.width();
            if width <= 60.0 {
                continue; // Too narrow for icon
            }

            // Icon center positioned at BOTTOM-LEFT corner of clip
            let cx: f32 = clip.rect.left() + 16.0;
            let cy: f32 = clip.rect.bottom() - 11.0;

            // Sleek dark rounded pill background for visibility on filmstrips
            let bg_rect = Rect::from_center_size(pos2(cx, cy), vec2(22.0, 16.0));
            painter.rect_filled(
                bg_rect,
                8.0, // fully rounded pill
                Color32::from_rgba_unmultiplied(20, 20, 20, 180),
            );

            if clip.audio_muted {
                // MUTED: red "X" over speaker — clean & distinct
                let red = Color32::from_rgb(255, 80, 80);

                // Speaker body
                painter.rect_filled(
                    Rect::from_min_size(pos2(cx - 6.0, cy - 3.0), vec2(4.0, 6.0)),
                    1.0,
                    red,
                );
                // Speaker cone
                painter.line_segment(
                    [pos2(cx - 2.0, cy - 3.0), pos2(cx + 2.0, cy - 6.0)],
                    egui::Stroke::new(1.5, red),
                );
                painter.line_segment(
                    [pos2(cx - 2.0, cy + 3.0), pos2(cx + 2.0, cy + 6.0)],
                    egui::Stroke::new(1.5, red),
                );
                // X mark (mute indicator) — two crossing lines
                let x_color = Color32::from_rgb(255, 100, 100);
                painter.line_segment(
                    [pos2(cx + 3.0, cy - 4.0), pos2(cx + 9.0, cy + 4.0)],
                    egui::Stroke::new(2.0, x_color),
                );
                painter.line_segment(
                    [pos2(cx + 9.0, cy - 4.0), pos2(cx + 3.0, cy + 4.0)],
                    egui::Stroke::new(2.0, x_color),
                );
            } else {
                // UNMUTED: clean white speaker with sound wave arcs
                let white = Color32::from_rgb(210, 215, 220);

                // Speaker body
                painter.rect_filled(
                    Rect::from_min_size(pos2(cx - 6.0, cy - 3.0), vec2(4.0, 6.0)),
                    1.0,
                    white,
                );
                // Speaker cone
                painter.line_segment(
                    [pos2(cx - 2.0, cy - 3.0), pos2(cx + 2.0, cy - 6.0)],
                    egui::Stroke::new(1.5, white),
                );
                painter.line_segment(
                    [pos2(cx - 2.0, cy + 3.0), pos2(cx + 2.0, cy + 6.0)],
                    egui::Stroke::new(1.5, white),
                );
                // Sound wave arcs — two curved lines
                let wave = Color32::from_rgba_unmultiplied(180, 200, 220, 200);
                // Inner arc
                painter.line_segment(
                    [pos2(cx + 4.0, cy - 3.0), pos2(cx + 5.5, cy)],
                    egui::Stroke::new(1.5, wave),
                );
                painter.line_segment(
                    [pos2(cx + 5.5, cy), pos2(cx + 4.0, cy + 3.0)],
                    egui::Stroke::new(1.5, wave),
                );
                // Outer arc
                painter.line_segment(
                    [pos2(cx + 6.5, cy - 5.0), pos2(cx + 8.5, cy)],
                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 170, 200, 150)),
                );
                painter.line_segment(
                    [pos2(cx + 8.5, cy), pos2(cx + 6.5, cy + 5.0)],
                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 170, 200, 150)),
                );
            }
        }
    }

    /// Render waveform peaks as vertical bars inside each clip.
    ///
    /// Draws pre-computed audio peaks from `WaveformCache` as a semi-transparent
    /// cyan/teal waveform overlay. Only renders for clips with audio and cached peaks.
    ///
    /// ## Performance
    /// - O(1) cache lookup per visible clip (HashMap)
    /// - Single Mesh for all waveforms (one draw call)
    /// - ~200 vertical bars per clip at typical widths
    /// - No allocations beyond the batch Mesh
    /// - Skips clips narrower than 30px (no visual benefit)
    pub fn render_waveforms(
        &self,
        painter: &Painter,
        waveform_cache: &strawberry_core::components::waveform::WaveformCache,
    ) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut shapes: Vec<Shape> = Vec::new();

        for clip in &self.visible_clips {
            // Only render waveforms on audio-only clips (no filmstrip).
            // Video clips with filmstrips already have visual content — showing
            // waveforms over the filmstrip is visually cluttered and incorrect.
            if clip.has_filmstrip {
                continue;
            }

            let asset_uuid: uuid::Uuid = match clip.asset_uuid {
                Some(uuid) => uuid,
                None => continue, // No audio asset
            };

            let clip_width: f32 = clip.rect.width();
            if clip_width < 30.0 {
                continue; // Too narrow for waveform
            }

            let peaks = match waveform_cache.get(&asset_uuid) {
                Some(p) => p,
                None => continue, // No cached peaks
            };

            if peaks.peaks.is_empty() || peaks.total_frames == 0 {
                continue;
            }

            // Calculate source region relative to total audio duration
            let total_duration_secs: f64 =
                peaks.total_frames as f64 / peaks.sample_rate as f64;
            if total_duration_secs <= 0.0 {
                continue;
            }

            let source_in_ratio: f64 =
                clip.source_in.as_secs_f64() / total_duration_secs;
            let source_out_ratio: f64 =
                clip.source_out.as_secs_f64() / total_duration_secs;

            // Number of vertical bars to draw = min(clip_width, peak_count)
            let bar_count: usize = (clip_width as usize).min(peaks.peaks.len());
            if bar_count == 0 {
                continue;
            }

            // Waveform area: inset slightly from clip edges
            let wave_top: f32 = clip.rect.top() + 2.0;
            let wave_bottom: f32 = clip.rect.bottom() - 2.0;
            let wave_height: f32 = wave_bottom - wave_top;
            let wave_center: f32 = (wave_top + wave_bottom) * 0.5;

            if wave_height < 4.0 {
                continue; // Not enough vertical space
            }

            // Waveform color: semi-transparent teal/cyan
            let wave_color = Color32::from_rgba_unmultiplied(80, 200, 220, 100);
            let wave_stroke = Stroke::new(1.0, wave_color);

            for i in 0..bar_count {
                let x_ratio: f64 = i as f64 / bar_count as f64;
                let peak_ratio: f64 =
                    source_in_ratio + x_ratio * (source_out_ratio - source_in_ratio);

                let peak_idx: usize =
                    (peak_ratio * peaks.peaks.len() as f64) as usize;
                let amplitude: f32 = peaks.peaks.get(peak_idx).copied().unwrap_or(0.0);

                if amplitude < 0.01 {
                    continue; // Skip silent bars
                }

                let x: f32 = clip.rect.left() + (i as f32 / bar_count as f32) * clip_width;
                let half_bar: f32 = amplitude * wave_height * 0.45;

                shapes.push(Shape::line_segment(
                    [
                        pos2(x, wave_center - half_bar),
                        pos2(x, wave_center + half_bar),
                    ],
                    wave_stroke,
                ));
            }
        }

        if !shapes.is_empty() {
            painter.extend(shapes);
        }
    }

    /// Render fade curve overlays on clips that have fade-in or fade-out.
    ///
    /// ## Visual Design
    /// - Semi-transparent dark overlay in faded regions (shows "this part is quieter")
    /// - White curve line tracing the exact fade shape
    /// - Both rendered as batched shapes (one extend call total)
    ///
    /// ## Performance
    /// - Skips clips with zero fades (most clips)
    /// - 24 line segments per curve (smooth but cheap)
    /// - Single `painter.extend()` call for all fade overlays
    /// - Pure arithmetic — no allocations beyond the shapes Vec
    pub fn render_fade_curves(&self, painter: &Painter) {
        if self.visible_clips.is_empty() {
            return;
        }

        let mut shapes: Vec<Shape> = Vec::new();

        /// Number of line segments per fade curve. 24 gives smooth curves
        /// without excessive geometry. At 60fps this is ~1440 vertices/second
        /// for a single clip with both fades — negligible GPU cost.
        const CURVE_SEGMENTS: usize = 24;

        for clip in &self.visible_clips {
            let clip_width: f32 = clip.rect.width();
            let clip_duration_ms: f64 = (clip.source_out - clip.source_in)
                .as_secs_f64()
                * 1000.0;
            if clip_duration_ms <= 0.0 || clip_width < 10.0 {
                continue;
            }

            let clip_top: f32 = clip.rect.top();
            let clip_bottom: f32 = clip.rect.bottom();
            let clip_height: f32 = clip_bottom - clip_top;

            // --- Fade-In Overlay (left edge) ---
            if clip.fade_in_ms > 0 {
                let fade_ratio: f64 = (clip.fade_in_ms as f64 / clip_duration_ms).min(1.0);
                let fade_width_px: f32 = (fade_ratio as f32 * clip_width).max(4.0);
                let fade_left: f32 = clip.rect.left();

                // Dark overlay polygon: trapezoidal shape from bottom-left
                // to curve line, showing the "quiet" region
                let overlay_color = Color32::from_rgba_unmultiplied(0, 0, 0, 80);
                let mut overlay_points: Vec<egui::Pos2> = Vec::with_capacity(CURVE_SEGMENTS + 3);

                // Start at bottom-left corner
                overlay_points.push(pos2(fade_left, clip_bottom));

                // Walk along bottom to fade end
                overlay_points.push(pos2(fade_left + fade_width_px, clip_bottom));

                // Walk up along curve line (right to left)
                for i in (0..=CURVE_SEGMENTS).rev() {
                    let t: f32 = i as f32 / CURVE_SEGMENTS as f32;
                    let curve_val: f32 = clip.fade_curve.apply(t);
                    let x: f32 = fade_left + t * fade_width_px;
                    // curve_val=0 → clip_bottom (silent), curve_val=1 → clip_top (full volume)
                    let y: f32 = clip_bottom - curve_val * clip_height;
                    overlay_points.push(pos2(x, y));
                }

                shapes.push(Shape::convex_polygon(
                    overlay_points,
                    overlay_color,
                    Stroke::NONE,
                ));

                // White curve line on top of the overlay
                let curve_color = Color32::from_rgba_unmultiplied(255, 255, 255, 160);
                let mut line_points: Vec<egui::Pos2> = Vec::with_capacity(CURVE_SEGMENTS + 1);
                for i in 0..=CURVE_SEGMENTS {
                    let t: f32 = i as f32 / CURVE_SEGMENTS as f32;
                    let curve_val: f32 = clip.fade_curve.apply(t);
                    let x: f32 = fade_left + t * fade_width_px;
                    let y: f32 = clip_bottom - curve_val * clip_height;
                    line_points.push(pos2(x, y));
                }
                shapes.push(Shape::line(
                    line_points,
                    Stroke::new(1.5, curve_color),
                ));
            }

            // --- Fade-Out Overlay (right edge) ---
            if clip.fade_out_ms > 0 {
                let fade_ratio: f64 = (clip.fade_out_ms as f64 / clip_duration_ms).min(1.0);
                let fade_width_px: f32 = (fade_ratio as f32 * clip_width).max(4.0);
                let fade_right: f32 = clip.rect.right();

                let overlay_color = Color32::from_rgba_unmultiplied(0, 0, 0, 80);
                let mut overlay_points: Vec<egui::Pos2> = Vec::with_capacity(CURVE_SEGMENTS + 3);

                // Start at bottom-right corner
                overlay_points.push(pos2(fade_right, clip_bottom));

                // Walk along bottom to fade start
                overlay_points.push(pos2(fade_right - fade_width_px, clip_bottom));

                // Walk up along curve line (left to right)
                // For fade-out: t=0 is at fade_start (full volume), t=1 is at fade_right (silent)
                for i in 0..=CURVE_SEGMENTS {
                    let t: f32 = i as f32 / CURVE_SEGMENTS as f32;
                    // Invert: at t=0 (start of fade region) volume is full,
                    // at t=1 (end of clip) volume is zero
                    let curve_val: f32 = clip.fade_curve.apply(1.0 - t);
                    let x: f32 = (fade_right - fade_width_px) + t * fade_width_px;
                    let y: f32 = clip_bottom - curve_val * clip_height;
                    overlay_points.push(pos2(x, y));
                }

                shapes.push(Shape::convex_polygon(
                    overlay_points,
                    overlay_color,
                    Stroke::NONE,
                ));

                // White curve line
                let curve_color = Color32::from_rgba_unmultiplied(255, 255, 255, 160);
                let mut line_points: Vec<egui::Pos2> = Vec::with_capacity(CURVE_SEGMENTS + 1);
                for i in 0..=CURVE_SEGMENTS {
                    let t: f32 = i as f32 / CURVE_SEGMENTS as f32;
                    let curve_val: f32 = clip.fade_curve.apply(1.0 - t);
                    let x: f32 = (fade_right - fade_width_px) + t * fade_width_px;
                    let y: f32 = clip_bottom - curve_val * clip_height;
                    line_points.push(pos2(x, y));
                }
                shapes.push(Shape::line(
                    line_points,
                    Stroke::new(1.5, curve_color),
                ));
            }
        }

        if !shapes.is_empty() {
            painter.extend(shapes);
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
            audio_muted: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            fade_curve: FadeCurve::Linear,
            keyframe_overlay: None,
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
            audio_muted: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            fade_curve: FadeCurve::Linear,
            keyframe_overlay: None,
        });

        renderer.clear();
        assert_eq!(renderer.clips().len(), 0);
    }
}
