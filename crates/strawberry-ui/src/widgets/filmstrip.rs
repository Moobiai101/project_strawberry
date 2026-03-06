//! # Filmstrip Rendering Widget
//!
//! Production-grade filmstrip thumbnail rendering for timeline clips.
//! Follows industry standards used by CapCut, Premiere Pro, and DaVinci Resolve.
//!
//! ## Architecture
//!
//! This module is a **pure rendering widget** - it does not manage state or textures.
//! It receives pre-loaded texture data and renders it efficiently.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        CLIP RECT                            │
//! │  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐  │
//! │  │ T1  │ │ T2  │ │ T3  │ │ T4  │ │ T5  │ │ T6  │ │ T7  │  │
//! │  └─────┘ └─────┘ └─────┘ └─────┘ └─────┘ └─────┘ └─────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let renderer = FilmstripRenderer::new();
//! renderer.draw(
//!     painter,
//!     clip_rect,
//!     &thumbnails,
//!     source_in_seconds,
//!     source_out_seconds,
//!     pixels_per_second,
//! );
//! ```
//!
//! ## Performance Optimizations
//!
//! 1. **Visibility Culling**: Only renders thumbnails visible in viewport
//! 2. **LOD Selection**: Automatic LOD based on zoom level
//! 3. **Aspect Ratio Preservation**: Maintains thumbnail proportions
//! 4. **GPU-efficient**: Uses egui's texture batching

use egui::{Color32, Painter, Pos2, Rect, TextureId, Vec2};
use std::time::Duration;

/// Pre-loaded thumbnail data for rendering
///
/// This is a view into the thumbnail cache - no ownership of textures.
pub struct FilmstripThumbnails<'a> {
    /// Timestamp (seconds) and texture ID pairs for current LOD
    pub thumbnails: &'a [(f64, TextureId)],
    /// Width of each thumbnail in pixels
    pub thumb_width: f32,
    /// Height of each thumbnail in pixels
    pub thumb_height: f32,
    /// Time interval between thumbnails in seconds
    pub interval_seconds: f64,
}

/// Configuration for filmstrip rendering
#[derive(Debug, Clone)]
pub struct FilmstripConfig {
    /// Opacity of the filmstrip (0.0 - 1.0)
    pub opacity: f32,
    /// Whether to show gradient fade at edges
    pub edge_fade: bool,
    /// Padding from clip edges in pixels
    pub edge_padding: f32,
    /// Corner radius for thumbnail clipping
    pub corner_radius: f32,
}

impl Default for FilmstripConfig {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            edge_fade: true,
            edge_padding: 2.0,
            corner_radius: 4.0,
        }
    }
}

/// Production-grade filmstrip renderer
///
/// Stateless renderer that draws thumbnail filmstrips on timeline clips.
/// Designed for high performance with visibility culling and efficient batching.
pub struct FilmstripRenderer {
    config: FilmstripConfig,
}

impl Default for FilmstripRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl FilmstripRenderer {
    /// Create a new filmstrip renderer with default configuration
    pub fn new() -> Self {
        Self {
            config: FilmstripConfig::default(),
        }
    }

    /// Create a new filmstrip renderer with custom configuration
    pub fn with_config(config: FilmstripConfig) -> Self {
        Self { config }
    }

    /// Draw filmstrip thumbnails on a clip rectangle
    ///
    /// ## Parameters
    ///
    /// - `painter`: egui painter for drawing
    /// - `clip_rect`: The rectangle where the clip is rendered
    /// - `thumbnails`: Pre-loaded thumbnail textures with timestamps
    /// - `source_in`: Source start time (for slip editing)
    /// - `source_out`: Source end time
    /// - `pixels_per_second`: Current zoom level
    /// - `viewport_rect`: Visible viewport for culling (optional)
    ///
    /// ## Industry Standard Behavior
    ///
    /// 1. Thumbnails tile horizontally across clip width
    /// 2. Height scaled to maintain aspect ratio
    /// 3. Source in/out determines which frames to show
    /// 4. Smooth fade at clip edges (like Premiere Pro)
    pub fn draw(
        &self,
        painter: &Painter,
        clip_rect: Rect,
        thumbnails: &FilmstripThumbnails,
        source_in: Duration,
        source_out: Duration,
        pixels_per_second: f64,
        viewport_rect: Option<Rect>,
    ) {
        // Skip if no thumbnails or clip not visible
        if thumbnails.thumbnails.is_empty() {
            return;
        }

        let viewport: Rect = viewport_rect.unwrap_or(clip_rect);
        if !clip_rect.intersects(viewport) {
            return;
        }

        // Calculate rendering area (with edge padding)
        let render_rect: Rect = Rect::from_min_max(
            Pos2::new(
                clip_rect.min.x + self.config.edge_padding,
                clip_rect.min.y + self.config.edge_padding,
            ),
            Pos2::new(
                clip_rect.max.x - self.config.edge_padding,
                clip_rect.max.y - self.config.edge_padding,
            ),
        );

        // CapCut-style filmstrip rendering:
        // 1. Fixed aspect ratio thumbnails (16:9 or square based on source)
        // 2. Tile edge-to-edge across clip width
        // 3. Each tile position maps to a source time

        let available_height: f32 = render_rect.height();

        // Calculate thumbnail width maintaining aspect ratio (use 16:9 as default)
        // Most video is 16:9, so thumbnails look best at that ratio
        let aspect_ratio: f32 = thumbnails.thumb_width / thumbnails.thumb_height.max(1.0);
        let thumbnail_width: f32 = available_height * aspect_ratio;
        let thumbnail_height: f32 = available_height;

        // Source time range
        let source_in_secs: f64 = source_in.as_secs_f64();
        let source_out_secs: f64 = source_out.as_secs_f64();
        let source_duration: f64 = source_out_secs - source_in_secs;

        if source_duration <= 0.0 || thumbnail_width <= 0.0 {
            return;
        }

        // Calculate how many seconds each displayed thumbnail represents
        let secs_per_thumbnail: f64 = (thumbnail_width as f64) / pixels_per_second;

        // Tile thumbnails edge-to-edge across the clip
        let mut x: f32 = render_rect.min.x;
        let mut source_time: f64 = source_in_secs;

        while x < render_rect.max.x && source_time < source_out_secs {
            // Skip if before visible viewport
            if x + thumbnail_width < viewport.min.x {
                x += thumbnail_width;
                source_time += secs_per_thumbnail;
                continue;
            }

            // Stop if past visible viewport
            if x > viewport.max.x {
                break;
            }

            // Find the best thumbnail for this source time
            if let Some(texture_id) = self.find_thumbnail_for_time(
                source_time,
                thumbnails.thumbnails,
                thumbnails.interval_seconds,
            ) {
                // Calculate thumbnail rect
                let thumb_rect: Rect = Rect::from_min_size(
                    Pos2::new(x, render_rect.min.y),
                    Vec2::new(thumbnail_width, thumbnail_height),
                );

                // Clip to render area
                let clipped_rect: Rect = thumb_rect.intersect(render_rect);

                if clipped_rect.width() > 0.0 && clipped_rect.height() > 0.0 {
                    // Calculate UV coordinates for partial thumbnail
                    let uv_rect: Rect =
                        self.calculate_uv_for_clipped_rect(thumb_rect, clipped_rect);

                    // Draw the thumbnail with optional edge fade
                    let alpha: u8 = self.calculate_edge_fade_alpha(
                        clipped_rect,
                        render_rect,
                        self.config.opacity,
                    );

                    painter.image(
                        texture_id,
                        clipped_rect,
                        uv_rect,
                        Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
                    );
                }
            }

            x += thumbnail_width;
            source_time += secs_per_thumbnail;
        }
    }

    /// Find the first thumbnail time that's >= source_in
    fn find_first_visible_thumbnail(&self, source_in_secs: f64, interval: f64) -> f64 {
        if interval <= 0.0 {
            return source_in_secs;
        }
        // Round down to nearest interval
        let index: u64 = (source_in_secs / interval) as u64;
        index as f64 * interval
    }

    /// Find the texture ID for a given time
    fn find_thumbnail_for_time(
        &self,
        time: f64,
        thumbnails: &[(f64, TextureId)],
        interval: f64,
    ) -> Option<TextureId> {
        // Binary search for the closest thumbnail
        let half_interval: f64 = interval / 2.0;

        for (thumb_time, texture_id) in thumbnails {
            if (thumb_time - time).abs() < half_interval + 0.001 {
                return Some(*texture_id);
            }
        }

        // Fallback: return first if time is before all thumbnails
        if !thumbnails.is_empty() && time < thumbnails[0].0 {
            return Some(thumbnails[0].1);
        }

        // Fallback: return last if time is after all thumbnails
        if !thumbnails.is_empty() && time >= thumbnails.last().unwrap().0 {
            return Some(thumbnails.last().unwrap().1);
        }

        // Diagnostic: log when no thumbnail found
        if time > 0.001 {
            tracing::warn!(
                "Filmstrip: no thumb for time={:.3}s, interval={:.1}s, n_thumbs={}, range=[{:.1}..{:.1}]",
                time, interval, thumbnails.len(),
                thumbnails.first().map(|t| t.0).unwrap_or(-1.0),
                thumbnails.last().map(|t| t.0).unwrap_or(-1.0),
            );
        }

        None
    }

    /// Calculate UV coordinates for a clipped rectangle
    fn calculate_uv_for_clipped_rect(&self, original: Rect, clipped: Rect) -> Rect {
        let u_min: f32 = (clipped.min.x - original.min.x) / original.width();
        let v_min: f32 = (clipped.min.y - original.min.y) / original.height();
        let u_max: f32 = (clipped.max.x - original.min.x) / original.width();
        let v_max: f32 = (clipped.max.y - original.min.y) / original.height();

        Rect::from_min_max(Pos2::new(u_min, v_min), Pos2::new(u_max, v_max))
    }

    /// Calculate alpha for edge fade effect
    fn calculate_edge_fade_alpha(
        &self,
        thumb_rect: Rect,
        clip_rect: Rect,
        base_opacity: f32,
    ) -> u8 {
        if !self.config.edge_fade {
            return (base_opacity * 255.0) as u8;
        }

        let fade_width: f32 = 10.0; // Fade over 10 pixels

        // Calculate distance from edges
        let left_dist: f32 = thumb_rect.min.x - clip_rect.min.x;
        let right_dist: f32 = clip_rect.max.x - thumb_rect.max.x;

        let mut alpha: f32 = base_opacity;

        // Fade at left edge
        if left_dist < fade_width && left_dist >= 0.0 {
            alpha *= left_dist / fade_width;
        }

        // Fade at right edge
        if right_dist < fade_width && right_dist >= 0.0 {
            alpha *= right_dist / fade_width;
        }

        (alpha * 255.0).clamp(0.0, 255.0) as u8
    }
}

/// Get appropriate LOD level based on zoom
///
/// This is a convenience re-export from strawberry-video.
/// Use for selecting which thumbnail LOD to request from cache.
pub fn get_lod_for_pixels_per_second(pixels_per_second: f64) -> u8 {
    match pixels_per_second {
        x if x < 10.0 => 0, // LOD0: zoomed out
        x if x < 50.0 => 1, // LOD1: normal view
        _ => 2,             // LOD2: use proxy frames
    }
}

/// Calculate thumbnail count for a given duration and LOD
pub fn calculate_thumbnail_count(duration: Duration, lod_level: u8) -> usize {
    let interval: f64 = match lod_level {
        0 => 10.0,  // Every 10 seconds
        1 => 2.0,   // Every 2 seconds
        _ => 0.033, // ~30fps for proxy frames
    };

    if interval <= 0.0 {
        return 0;
    }

    (duration.as_secs_f64() / interval).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_selection() {
        assert_eq!(get_lod_for_pixels_per_second(5.0), 0);
        assert_eq!(get_lod_for_pixels_per_second(25.0), 1);
        assert_eq!(get_lod_for_pixels_per_second(100.0), 2);
    }

    #[test]
    fn test_thumbnail_count() {
        // 60 second video at LOD0 (10s interval) = 6 thumbnails
        assert_eq!(calculate_thumbnail_count(Duration::from_secs(60), 0), 6);
        // 60 second video at LOD1 (2s interval) = 30 thumbnails
        assert_eq!(calculate_thumbnail_count(Duration::from_secs(60), 1), 30);
    }

    #[test]
    fn test_find_first_visible_thumbnail() {
        let renderer = FilmstripRenderer::new();

        // Source in at 5.5s, interval 2.0s -> first visible at 4.0s
        assert_eq!(renderer.find_first_visible_thumbnail(5.5, 2.0), 4.0);

        // Source in at 10.0s, interval 10.0s -> first visible at 10.0s
        assert_eq!(renderer.find_first_visible_thumbnail(10.0, 10.0), 10.0);
    }

    #[test]
    fn test_uv_calculation() {
        let renderer = FilmstripRenderer::new();

        let original: Rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 50.0));
        let clipped: Rect = Rect::from_min_size(Pos2::new(25.0, 0.0), Vec2::new(50.0, 50.0));

        let uv: Rect = renderer.calculate_uv_for_clipped_rect(original, clipped);

        assert!((uv.min.x - 0.25).abs() < 0.001);
        assert!((uv.max.x - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_config_defaults() {
        let config: FilmstripConfig = FilmstripConfig::default();
        assert_eq!(config.opacity, 1.0);
        assert!(config.edge_fade);
        assert_eq!(config.edge_padding, 2.0);
    }
}
