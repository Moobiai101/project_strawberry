//! Timeline widget struct and builder methods.
//!
//! Contains the main `TimelineWidget` struct that orchestrates
//! the entire timeline UI.

use egui::Id;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use super::interaction::TimelineTool;
use super::types::{
    ClipDragState, ClipInfo, FilmstripAssetTextures, SlideDragState, SlipDragState, TrackInfo,
    TrimDragState,
};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Default height for video tracks
pub const VIDEO_TRACK_HEIGHT: f32 = 35.0;

/// Default height for audio tracks
pub const AUDIO_TRACK_HEIGHT: f32 = 28.0;

/// Ruler height
pub const RULER_HEIGHT: f32 = 24.0;

/// Track header width (for track name/buttons)
pub const TRACK_HEADER_WIDTH: f32 = 0.0; // Hidden for now

/// Playhead width
pub const PLAYHEAD_WIDTH: f32 = 2.0;

/// Playhead head (triangle) size - larger for easy grabbing (CapCut style)
pub const PLAYHEAD_HEAD_SIZE: f32 = 14.0;

/// Playhead grab zone width (pixels) - area around playhead that triggers drag
pub const PLAYHEAD_GRAB_WIDTH: f32 = 20.0;

/// Scroll bar height
pub const SCROLLBAR_HEIGHT: f32 = 0.0; // Hide scrollbar for cleaner look

/// Minimum zoom level (pixels per second)
pub const MIN_ZOOM: f32 = 5.0;

/// Maximum zoom level (pixels per second)
pub const MAX_ZOOM: f32 = 500.0;

/// Width of trim handle zone on each clip edge (pixels) - wider for easier detection
pub const TRIM_HANDLE_WIDTH: f32 = 12.0;

/// Gap between adjacent clips (pixels) - shows transition zone
pub const CLIP_GAP: f32 = 4.0;

/// Size of the transition "+" button
pub const TRANSITION_BUTTON_SIZE: f32 = 20.0;

// ============================================================================
// TIMELINE WIDGET
// ============================================================================

/// The main timeline widget.
///
/// This widget renders the full timeline UI including:
/// - Time ruler at the top
/// - Track headers on the left
/// - Clip blocks in the main area
/// - Playhead indicator
///
/// ## Usage
///
/// ```ignore
/// let response = TimelineWidget::new()
///     .with_tracks(tracks)
///     .with_clips(clips)
///     .with_playhead(current_time)
///     .with_zoom(100.0)
///     .show(ui);
/// ```
pub struct TimelineWidget {
    /// Widget ID for state persistence
    pub(crate) id: Id,

    /// Timeline name (for header display)
    pub(crate) timeline_name: String,

    /// Frame rate for time calculations
    pub(crate) frame_rate: f64,

    /// Total timeline duration
    pub(crate) duration: Duration,

    /// Track data
    pub(crate) tracks: Vec<TrackInfo>,

    /// Clip data
    pub(crate) clips: Vec<ClipInfo>,

    /// Current playhead position
    pub(crate) playhead_time: Duration,

    /// Horizontal scroll offset (in pixels)
    pub(crate) scroll_x: f32,

    /// Vertical scroll offset (in pixels)
    pub(crate) scroll_y: f32,

    /// Zoom level (pixels per second)
    pub(crate) zoom: f32,

    /// Whether the timeline is playing
    pub(crate) is_playing: bool,

    /// Current drag state (for clip drag-to-move ghost preview)
    pub(crate) drag_state: Option<ClipDragState>,

    /// Current trim state (for clip trim ghost preview)
    pub(crate) trim_state: Option<TrimDragState>,

    /// Current slip state (for slip operation preview)
    pub(crate) slip_state: Option<SlipDragState>,

    /// Current slide state (for slide operation preview)
    pub(crate) slide_state: Option<SlideDragState>,

    /// Active editing tool mode
    pub(crate) active_tool: TimelineTool,

    /// Centered playhead mode: when true, dragging on ruler scrolls timeline
    /// instead of moving playhead (playhead stays centered in view)
    pub(crate) centered_playhead: bool,

    /// Filmstrip thumbnail textures per asset UUID
    /// Maps asset_uuid -> Vec<(timestamp_secs, TextureId)>
    /// Used by draw_clips to render filmstrip thumbnails
    pub(crate) filmstrip_textures: HashMap<Uuid, FilmstripAssetTextures>,
}

impl Default for TimelineWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl TimelineWidget {
    /// Create a new empty timeline widget
    pub fn new() -> Self {
        Self {
            id: Id::new("timeline"),
            timeline_name: "Timeline".to_string(),
            frame_rate: 30.0,
            duration: Duration::from_secs(60),
            tracks: Vec::new(),
            clips: Vec::new(),
            playhead_time: Duration::ZERO,
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 100.0, // 100 pixels per second
            is_playing: false,
            drag_state: None,
            trim_state: None,
            slip_state: None,
            slide_state: None,
            active_tool: TimelineTool::default(),
            centered_playhead: true, // Default to centered playhead mode
            filmstrip_textures: HashMap::new(),
        }
    }

    // ========================================================================
    // BUILDER METHODS
    // ========================================================================

    /// Set the widget ID
    pub fn with_id(mut self, id: impl Into<Id>) -> Self {
        self.id = id.into();
        self
    }

    /// Set timeline name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.timeline_name = name.into();
        self
    }

    /// Set frame rate
    pub fn with_frame_rate(mut self, fps: f64) -> Self {
        self.frame_rate = fps;
        self
    }

    /// Set timeline duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set tracks
    pub fn with_tracks(mut self, tracks: Vec<TrackInfo>) -> Self {
        self.tracks = tracks;
        self
    }

    /// Set clips
    pub fn with_clips(mut self, clips: Vec<ClipInfo>) -> Self {
        self.clips = clips;
        self
    }

    /// Set playhead position
    pub fn with_playhead(mut self, time: Duration) -> Self {
        self.playhead_time = time;
        self
    }

    /// Set scroll position
    pub fn with_scroll(mut self, x: f32, y: f32) -> Self {
        self.scroll_x = x;
        self.scroll_y = y;
        self
    }

    /// Set zoom level (pixels per second)
    pub fn with_zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self
    }

    /// Set playing state
    pub fn with_playing(mut self, is_playing: bool) -> Self {
        self.is_playing = is_playing;
        self
    }

    /// Set drag state (for persisting drag state between frames)
    ///
    /// Pass the `updated_drag_state` from the previous frame's `TimelineResponse`
    /// to maintain drag continuity across frames.
    pub fn with_drag_state(mut self, drag_state: Option<ClipDragState>) -> Self {
        self.drag_state = drag_state;
        self
    }

    /// Set trim state (for persisting trim state between frames)
    ///
    /// Pass the `updated_trim_state` from the previous frame's `TimelineResponse`
    /// to maintain trim continuity across frames.
    pub fn with_trim_state(mut self, trim_state: Option<TrimDragState>) -> Self {
        self.trim_state = trim_state;
        self
    }

    /// Set slip state (for persisting slip operation between frames)
    pub fn with_slip_state(mut self, slip_state: Option<SlipDragState>) -> Self {
        self.slip_state = slip_state;
        self
    }

    /// Set slide state (for persisting slide operation between frames)
    pub fn with_slide_state(mut self, slide_state: Option<SlideDragState>) -> Self {
        self.slide_state = slide_state;
        self
    }

    /// Set the active editing tool mode
    pub fn with_active_tool(mut self, tool: TimelineTool) -> Self {
        self.active_tool = tool;
        self
    }

    /// Set filmstrip textures for rendering thumbnails on clips
    ///
    /// Pass a map of asset_uuid -> FilmstripAssetTextures where each contains
    /// pre-uploaded GPU textures for LOD0 and LOD1 levels.
    pub fn with_filmstrip_textures(
        mut self,
        textures: HashMap<Uuid, FilmstripAssetTextures>,
    ) -> Self {
        self.filmstrip_textures = textures;
        self
    }

    // ========================================================================
    // HELPER METHODS
    // ========================================================================

    /// Convert time to X position (relative to content area)
    pub fn time_to_x(&self, time: Duration) -> f32 {
        time.as_secs_f32() * self.zoom - self.scroll_x
    }

    /// Convert X position to time
    pub fn x_to_time(&self, x_relative: f32) -> Duration {
        let seconds: f32 = (x_relative + self.scroll_x) / self.zoom;
        Duration::from_secs_f32(seconds.max(0.0))
    }

    /// Get Y position for a track index (relative to content top)
    pub fn track_y(&self, track_index: u32) -> f32 {
        self.tracks
            .iter()
            .take(track_index as usize)
            .map(|t| t.height)
            .sum()
    }

    /// Get height for a track index
    pub fn track_height(&self, track_index: u32) -> f32 {
        self.tracks
            .get(track_index as usize)
            .map(|t| t.height)
            .unwrap_or(VIDEO_TRACK_HEIGHT)
    }

    /// Get total height of all tracks
    pub fn total_track_height(&self) -> f32 {
        self.tracks.iter().map(|t| t.height).sum()
    }

    /// Get zoom level
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Get scroll X
    pub fn scroll_x(&self) -> f32 {
        self.scroll_x
    }

    /// Get scroll Y
    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    /// Get frame rate
    pub fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// Get clips reference
    pub fn clips(&self) -> &[ClipInfo] {
        &self.clips
    }

    /// Get tracks reference
    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_widget_creation() {
        let widget = TimelineWidget::new()
            .with_name("Test Timeline")
            .with_frame_rate(24.0)
            .with_zoom(150.0);

        assert_eq!(widget.timeline_name, "Test Timeline");
        assert_eq!(widget.frame_rate, 24.0);
        assert_eq!(widget.zoom, 150.0);
    }

    #[test]
    fn test_time_to_x_conversion() {
        let widget = TimelineWidget::new().with_zoom(100.0).with_scroll(0.0, 0.0);

        // 1 second = 100 pixels at zoom 100
        let x = widget.time_to_x(Duration::from_secs(1));
        assert_eq!(x, 100.0);
    }

    #[test]
    fn test_x_to_time_conversion() {
        let widget = TimelineWidget::new().with_zoom(100.0).with_scroll(0.0, 0.0);

        // 100 pixels = 1 second at zoom 100
        let time = widget.x_to_time(100.0);
        assert_eq!(time.as_secs(), 1);
    }

    #[test]
    fn test_zoom_clamping() {
        let widget_low = TimelineWidget::new().with_zoom(1.0); // Below MIN_ZOOM
        assert_eq!(widget_low.zoom, MIN_ZOOM);

        let widget_high = TimelineWidget::new().with_zoom(10000.0); // Above MAX_ZOOM
        assert_eq!(widget_high.zoom, MAX_ZOOM);
    }
}
