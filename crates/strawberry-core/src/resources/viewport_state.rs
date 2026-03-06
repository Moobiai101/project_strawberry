//! # Viewport State Resource
//!
//! Tracks the visible portion of the timeline for virtualization.
//! Used by UI to cull off-screen clips and by compositor to skip
//! rendering non-visible layers.
//!
//! ## Performance Impact
//!
//! For 1000+ clips, checking visibility against viewport eliminates 95%+
//! of clip rendering work on each frame. This is the single biggest
//! optimization for timeline scalability.
//!
//! ## Usage
//!
//! ```ignore
//! // In timeline render loop:
//! let viewport = world.resource::<TimelineViewport>();
//!
//! for clip in clips {
//!     if viewport.is_clip_visible(clip.timeline_start, clip.duration) {
//!         render_clip(clip);
//!     }
//! }
//! ```

use bevy_ecs::system::Resource;
use std::time::Duration;

// ============================================================================
// TIMELINE VIEWPORT
// ============================================================================

/// Resource: Current visible timeline range for UI virtualization.
///
/// This resource is updated every frame by the timeline widget to reflect
/// the current scroll position and zoom level. Other systems can query it
/// to skip processing off-screen content.
///
/// ## Fields
///
/// - `visible_start_secs`: Start of visible range in seconds (based on scroll)
/// - `visible_end_secs`: End of visible range in seconds (scroll + viewport width)
/// - `visible_clip_count`: Number of clips currently in the visible range
/// - `total_clip_count`: Total clips in timeline (for debug stats)
/// - `zoom_level`: Current pixels-per-second zoom (for LOD decisions)
/// - `viewport_width_pixels`: Width of the timeline viewport in pixels
#[derive(Resource, Default, Clone, Debug)]
pub struct TimelineViewport {
    /// Start of visible range in seconds (derived from scroll_offset / zoom)
    pub visible_start_secs: f64,
    /// End of visible range in seconds (visible_start + viewport_duration)
    pub visible_end_secs: f64,
    /// Number of clips currently visible in viewport
    pub visible_clip_count: usize,
    /// Total clips in timeline (for stats/debug display)
    pub total_clip_count: usize,
    /// Current zoom level in pixels per second
    pub zoom_level: f64,
    /// Viewport width in pixels (for recalculating visible range)
    pub viewport_width_pixels: f32,
    /// Track index range that's visible (for vertical culling)
    pub visible_track_start: u32,
    /// Last visible track index
    pub visible_track_end: u32,
}

impl TimelineViewport {
    /// Create a new viewport with initial values
    pub fn new() -> Self {
        Self {
            visible_start_secs: 0.0,
            visible_end_secs: 10.0, // Default 10 seconds visible
            visible_clip_count: 0,
            total_clip_count: 0,
            zoom_level: 100.0,             // Default 100 pixels per second
            viewport_width_pixels: 1920.0, // Common default
            visible_track_start: 0,
            visible_track_end: 10,
        }
    }

    /// Update the viewport state from scroll/zoom parameters
    ///
    /// Call this every frame from the timeline widget before rendering clips.
    ///
    /// ## Parameters
    ///
    /// - `scroll_offset_pixels`: Current horizontal scroll offset in pixels
    /// - `zoom_pixels_per_second`: Current zoom level (pixels per second)
    /// - `viewport_width`: Width of the viewport in pixels
    pub fn update_from_view_state(
        &mut self,
        scroll_offset_pixels: f32,
        zoom_pixels_per_second: f32,
        viewport_width: f32,
    ) {
        let scroll_offset_f64: f64 = scroll_offset_pixels as f64;
        let zoom_f64: f64 = zoom_pixels_per_second as f64;

        self.zoom_level = zoom_f64;
        self.viewport_width_pixels = viewport_width;

        // Calculate visible time range
        self.visible_start_secs = scroll_offset_f64 / zoom_f64;
        let visible_duration_secs: f64 = viewport_width as f64 / zoom_f64;
        self.visible_end_secs = self.visible_start_secs + visible_duration_secs;
    }

    /// Update vertical track visibility range
    pub fn update_track_visibility(
        &mut self,
        scroll_y: f32,
        viewport_height: f32,
        track_heights: &[f32],
    ) {
        let mut cumulative_y: f32 = 0.0;
        let mut start_found: bool = false;

        self.visible_track_start = 0;
        self.visible_track_end = track_heights.len() as u32;

        for (index, height) in track_heights.iter().enumerate() {
            let track_bottom: f32 = cumulative_y + height;

            // Check if track is visible in viewport
            if !start_found && track_bottom > scroll_y {
                self.visible_track_start = index as u32;
                start_found = true;
            }

            if cumulative_y > scroll_y + viewport_height {
                self.visible_track_end = index as u32;
                break;
            }

            cumulative_y = track_bottom;
        }
    }

    /// Check if a clip is visible in the current viewport
    ///
    /// This is the hot-path function called for every clip during rendering.
    /// Optimized for minimal branching and no allocations.
    ///
    /// ## Parameters
    ///
    /// - `clip_start`: Clip's timeline start time in seconds
    /// - `clip_duration_secs`: Clip's duration in seconds
    /// - `track_index`: Track index the clip is on (for vertical culling)
    ///
    /// ## Returns
    ///
    /// True if any part of the clip overlaps the visible viewport.
    #[inline]
    pub fn is_clip_visible(
        &self,
        clip_start_secs: f64,
        clip_duration_secs: f64,
        track_index: u32,
    ) -> bool {
        let clip_end_secs: f64 = clip_start_secs + clip_duration_secs;

        // Horizontal visibility: clip overlaps visible time range
        let horizontal_visible: bool =
            clip_end_secs > self.visible_start_secs && clip_start_secs < self.visible_end_secs;

        // Vertical visibility: track is in visible range
        let vertical_visible: bool =
            track_index >= self.visible_track_start && track_index <= self.visible_track_end;

        horizontal_visible && vertical_visible
    }

    /// Check if a clip is visible (time-only, ignores track)
    ///
    /// Use when you only need horizontal visibility check.
    #[inline]
    pub fn is_time_visible(&self, clip_start_secs: f64, clip_duration_secs: f64) -> bool {
        let clip_end_secs: f64 = clip_start_secs + clip_duration_secs;
        clip_end_secs > self.visible_start_secs && clip_start_secs < self.visible_end_secs
    }

    /// Check if a Duration-based clip is visible
    #[inline]
    pub fn is_clip_duration_visible(&self, timeline_start: Duration, duration: Duration) -> bool {
        self.is_time_visible(timeline_start.as_secs_f64(), duration.as_secs_f64())
    }

    /// Get visible duration in seconds
    pub fn visible_duration_secs(&self) -> f64 {
        self.visible_end_secs - self.visible_start_secs
    }

    /// Get the center of the visible viewport (for priority loading)
    pub fn visible_center_secs(&self) -> f64 {
        (self.visible_start_secs + self.visible_end_secs) / 2.0
    }

    /// Calculate distance from viewport center (for priority sorting)
    ///
    /// Clips closer to viewport center should load first.
    /// Returns 0.0 for clips at center, increasing value for clips further away.
    pub fn distance_from_center(&self, clip_start_secs: f64, clip_duration_secs: f64) -> f64 {
        let clip_center: f64 = clip_start_secs + clip_duration_secs / 2.0;
        let viewport_center: f64 = self.visible_center_secs();
        (clip_center - viewport_center).abs()
    }

    /// Update clip counts (call after filtering visible clips)
    pub fn update_clip_counts(&mut self, visible: usize, total: usize) {
        self.visible_clip_count = visible;
        self.total_clip_count = total;
    }

    /// Get efficiency ratio (visible_clips / total_clips)
    ///
    /// Used for performance monitoring - lower is better (more culling)
    pub fn culling_efficiency(&self) -> f64 {
        if self.total_clip_count == 0 {
            return 1.0;
        }
        1.0 - (self.visible_clip_count as f64 / self.total_clip_count as f64)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_update() {
        let mut viewport = TimelineViewport::new();

        // 100px scroll, 100 pixels/second zoom, 800px wide viewport
        viewport.update_from_view_state(100.0, 100.0, 800.0);

        assert!((viewport.visible_start_secs - 1.0).abs() < 0.001);
        assert!((viewport.visible_end_secs - 9.0).abs() < 0.001);
    }

    #[test]
    fn test_clip_visibility() {
        let mut viewport = TimelineViewport::new();
        viewport.update_from_view_state(100.0, 100.0, 800.0);
        // visible_start = 1.0, visible_end = 9.0

        // Clip fully inside viewport
        assert!(viewport.is_time_visible(2.0, 3.0));

        // Clip partially overlapping left edge
        assert!(viewport.is_time_visible(0.5, 1.0));

        // Clip partially overlapping right edge
        assert!(viewport.is_time_visible(8.5, 1.0));

        // Clip fully outside (left)
        assert!(!viewport.is_time_visible(0.0, 0.5));

        // Clip fully outside (right)
        assert!(!viewport.is_time_visible(10.0, 2.0));
    }

    #[test]
    fn test_distance_from_center() {
        let mut viewport = TimelineViewport::new();
        viewport.update_from_view_state(0.0, 100.0, 1000.0);
        // visible_start = 0.0, visible_end = 10.0, center = 5.0

        // Clip at center
        let dist_center: f64 = viewport.distance_from_center(4.5, 1.0);
        assert!(dist_center.abs() < 0.001);

        // Clip at left edge
        let dist_left: f64 = viewport.distance_from_center(0.0, 1.0);
        assert!((dist_left - 4.5).abs() < 0.001);
    }

    #[test]
    fn test_culling_efficiency() {
        let mut viewport = TimelineViewport::new();
        viewport.update_clip_counts(10, 100);

        let efficiency: f64 = viewport.culling_efficiency();
        assert!((efficiency - 0.9).abs() < 0.001);
    }
}
