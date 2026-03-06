//! # Timeline Widget
//!
//! The heart of the video editor - a multi-track timeline for editing clips.
//!
//! ## Features
//!
//! - **Timeline Ruler**: Timecode display with frame/second markers
//! - **Track Lanes**: Horizontal lanes for video/audio tracks
//! - **Clip Blocks**: Visual representation of clips on tracks
//! - **Playhead**: Current position indicator
//! - **Scrolling/Zooming**: Navigate large timelines
//!
//! ## Architecture
//!
//! The TimelineWidget is a stateless view that reads from ECS components:
//! - `Timeline` - Timeline metadata (frame rate, name)
//! - `Track` - Track properties (index, type, mute/solo)
//! - `Clip` - Clip timing data
//! - `Playhead` - Current playback position
//!
//! Actions are dispatched as Commands to the CommandQueue.

use egui::{
    pos2, vec2, Align2, Color32, FontId, Id, Pos2, Rect, Response, Rounding, Sense, Stroke, Ui,
    Vec2, Widget,
};
use std::time::Duration;

// ============================================================================
// TIMELINE CONSTANTS
// ============================================================================

/// Height of the timeline ruler (time markings at top) - CapCut style: compact
const RULER_HEIGHT: f32 = 24.0;

/// Width of track headers on the left (0 = CapCut style, no headers)
const TRACK_HEADER_WIDTH: f32 = 0.0;

/// Default height for video tracks - CapCut style: thin strips (~35px)
const VIDEO_TRACK_HEIGHT: f32 = 35.0;

/// Playhead width
const PLAYHEAD_WIDTH: f32 = 2.0;

/// Playhead head (triangle) size - larger for easy grabbing (CapCut style)
const PLAYHEAD_HEAD_SIZE: f32 = 14.0;

/// Playhead grab zone width (pixels) - area around playhead that triggers drag
const PLAYHEAD_GRAB_WIDTH: f32 = 20.0;

/// Scroll bar height
const SCROLLBAR_HEIGHT: f32 = 0.0; // Hide scrollbar for cleaner look

/// Minimum zoom level (pixels per second)
const MIN_ZOOM: f32 = 5.0;

/// Maximum zoom level (pixels per second)
const MAX_ZOOM: f32 = 500.0;

/// Width of trim handle zone on each clip edge (pixels) - wider for easier detection
const TRIM_HANDLE_WIDTH: f32 = 12.0;

/// Gap between adjacent clips (pixels) - shows transition zone
const CLIP_GAP: f32 = 4.0;

/// Size of the transition "+" button
const TRANSITION_BUTTON_SIZE: f32 = 20.0;

// ============================================================================
// TIMELINE COLORS
// ============================================================================

mod colors {
    use egui::Color32;

    // ========================================================================
    // CAPCUT MINIMALIST - DARK BLACK & WHITE
    // ========================================================================

    // Backgrounds - Pure dark black
    pub const BACKGROUND: Color32 = Color32::from_rgb(13, 13, 13); // #0d0d0d pure black
    pub const BACKGROUND_ELEVATED: Color32 = Color32::from_rgb(24, 24, 24); // #181818

    // Ruler - Dark gray
    pub const RULER_BG: Color32 = Color32::from_rgb(20, 20, 20); // #141414
    pub const RULER_TEXT: Color32 = Color32::from_rgb(140, 140, 140); // Gray text
    pub const RULER_MAJOR_LINE: Color32 = Color32::from_rgb(60, 60, 60);
    pub const RULER_MINOR_LINE: Color32 = Color32::from_rgb(35, 35, 35);

    // Track lanes - Pure black alternating
    pub const TRACK_LANE_BG: Color32 = Color32::from_rgb(13, 13, 13); // #0d0d0d
    pub const TRACK_LANE_ALT: Color32 = Color32::from_rgb(18, 18, 18); // #121212
    pub const TRACK_DIVIDER: Color32 = Color32::from_rgb(40, 40, 40);

    // Clip colors - CapCut style muted purple-blue
    pub const CLIP_VIDEO: Color32 = Color32::from_rgb(92, 107, 192); // #5c6bc0 muted indigo
    pub const CLIP_VIDEO_HOVER: Color32 = Color32::from_rgb(112, 127, 212); // Lighter
    pub const CLIP_AUDIO: Color32 = Color32::from_rgb(78, 182, 196); // Muted cyan-teal
    pub const CLIP_AUDIO_HOVER: Color32 = Color32::from_rgb(98, 202, 216); // Lighter
    pub const CLIP_SELECTED_BORDER: Color32 = Color32::from_rgb(255, 255, 255); // White selection
    pub const CLIP_BORDER: Color32 = Color32::from_rgb(45, 45, 45); // Subtle dark border
    pub const CLIP_TEXT: Color32 = Color32::from_rgb(255, 255, 255); // Pure white text

    // Playhead - WHITE
    pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 255, 255); // Pure white
    pub fn playhead_shadow() -> Color32 {
        Color32::from_rgba_unmultiplied(0, 0, 0, 120)
    }

    // Transition zone - Simple white button
    pub const TRANSITION_PLUS_BG: Color32 = Color32::from_rgb(255, 255, 255); // White
    pub const TRANSITION_PLUS_HOVER: Color32 = Color32::from_rgb(200, 200, 200);

    // Misc
    pub const SCROLLBAR_BG: Color32 = Color32::from_rgb(24, 24, 24);
    pub const ACCENT_CYAN: Color32 = Color32::from_rgb(78, 182, 196); // For hover effects
}

// ============================================================================
// TRACK DATA
// ============================================================================

/// Track information for rendering
#[derive(Debug, Clone)]
pub struct TrackInfo {
    /// Track entity ID (as bits for display)
    pub entity_bits: u64,
    /// Track index (0 = bottom)
    pub index: u32,
    /// Track name
    pub name: String,
    /// Whether video or audio
    pub is_video: bool,
    /// Is muted
    pub muted: bool,
    /// Is solo
    pub solo: bool,
    /// Is locked
    pub locked: bool,
    /// Track height override
    pub height: f32,
}

impl Default for TrackInfo {
    fn default() -> Self {
        Self {
            entity_bits: 0,
            index: 0,
            name: "Track".to_string(),
            is_video: true,
            muted: false,
            solo: false,
            locked: false,
            height: VIDEO_TRACK_HEIGHT,
        }
    }
}

// ============================================================================
// TRACK BUTTON TYPE
// ============================================================================

// ============================================================================
// DRAG STATE (Ghost Item Pattern from Olive)
// ============================================================================

/// Drag state for clip move operations
///
/// Based on Olive's TimelineViewGhostItem pattern - tracks the clip being
/// dragged and its current ghost position for preview rendering.
#[derive(Debug, Clone, Default)]
pub struct ClipDragState {
    /// Entity bits of the clip being dragged
    pub clip_entity: u64,
    /// Original track index
    pub original_track: u32,
    /// Original timeline start
    pub original_time: Duration,
    /// Current ghost track index (where it would land if dropped)
    pub ghost_track: u32,
    /// Current ghost time position
    pub ghost_time: Duration,
    /// Clip duration (for rendering)
    pub duration: Duration,
    /// Whether this is an active drag
    pub is_active: bool,
}

/// Which edge of a clip is being trimmed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    /// Left edge (in point)
    Left,
    /// Right edge (out point)
    Right,
}

/// Drag state for clip trim operations
///
/// Tracks the clip being trimmed and the current ghost position for the
/// edge being adjusted. Used for visual preview during trim drag.
#[derive(Debug, Clone)]
pub struct TrimDragState {
    /// Entity bits of the clip being trimmed
    pub clip_entity: u64,
    /// Which edge is being trimmed
    pub edge: TrimEdge,
    /// Original timeline start
    pub original_timeline_start: Duration,
    /// Original duration
    pub original_duration: Duration,
    /// Current ghost new value (timeline_start for Left, new duration for Right)
    pub ghost_value: Duration,
    /// Whether this is an active trim drag
    pub is_active: bool,
}

/// Where a drag operation originated from
/// This is crucial for determining which operation to perform during dragged()
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOrigin {
    /// No drag active
    None,
    /// Drag started on ruler - do playhead scrub
    Ruler,
    /// Drag started on clip edge - do trim
    TrimEdge,
    /// Drag started on clip body - do move
    ClipBody,
    /// Drag started on empty area - do selection or nothing
    Empty,
}

// ============================================================================
// TIMELINE TOOL MODE
// ============================================================================

/// Active editing tool mode (inspired by Olive's TimelineTool subclasses)
///
/// Each mode changes how mouse interactions are interpreted:
/// - **Pointer**: Default mode for selection, move, and trim
/// - **Razor**: Click to split clips at the cursor position
/// - **Slip**: Drag to shift clip's source content without moving clip position
/// - **Slide**: Drag to move clip while adjusting neighbor boundaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineTool {
    /// Default selection/move/trim mode
    #[default]
    Pointer,
    /// Split clips at click position (multi-track when Shift held)
    Razor,
    /// Shift source in/out points without moving clip position
    Slip,
    /// Move clip while adjusting neighbor clip boundaries
    Slide,
}

impl TimelineTool {
    /// Get display name for UI buttons
    pub fn display_name(&self) -> &'static str {
        match self {
            TimelineTool::Pointer => "Select",
            TimelineTool::Razor => "Razor",
            TimelineTool::Slip => "Slip",
            TimelineTool::Slide => "Slide",
        }
    }

    /// Get keyboard shortcut hint
    pub fn shortcut_hint(&self) -> &'static str {
        match self {
            TimelineTool::Pointer => "V",
            TimelineTool::Razor => "B",
            TimelineTool::Slip => "Y",
            TimelineTool::Slide => "U",
        }
    }

    /// Get the appropriate cursor icon for this tool
    pub fn cursor_icon(&self) -> egui::CursorIcon {
        match self {
            TimelineTool::Pointer => egui::CursorIcon::Default,
            TimelineTool::Razor => egui::CursorIcon::Crosshair,
            TimelineTool::Slip => egui::CursorIcon::ResizeHorizontal,
            TimelineTool::Slide => egui::CursorIcon::Grab,
        }
    }
}

/// Drag state for slip operations
///
/// Slipping shifts which portion of source media is visible without
/// changing clip position or duration on the timeline.
#[derive(Debug, Clone)]
pub struct SlipDragState {
    /// Clip entity being slipped
    pub clip_entity: u64,
    /// Original source_in value
    pub original_source_in: Duration,
    /// Original source_out value
    pub original_source_out: Duration,
    /// Current accumulated offset (can be negative via i64 millis)
    pub offset_millis: i64,
    /// Whether slip is active
    pub is_active: bool,
}

/// Drag state for slide operations
///
/// Sliding moves a clip while adjusting the in/out points of
/// neighboring clips to maintain timeline coverage.
#[derive(Debug, Clone)]
pub struct SlideDragState {
    /// Clip entity being slid
    pub clip_entity: u64,
    /// Original timeline_start
    pub original_time: Duration,
    /// Previous clip entity (if any)
    pub previous_clip: Option<u64>,
    /// Previous clip's original source_out
    pub previous_original_out: Option<Duration>,
    /// Next clip entity (if any)
    pub next_clip: Option<u64>,
    /// Next clip's original timeline_start
    pub next_original_start: Option<Duration>,
    /// Next clip's original source_in
    pub next_original_in: Option<Duration>,
    /// Current delta offset (can be negative)
    pub delta_millis: i64,
    /// Whether slide is active
    pub is_active: bool,
}

// ============================================================================
// CLIP DATA
// ============================================================================

/// Clip information for rendering
#[derive(Debug, Clone)]
pub struct ClipInfo {
    /// Clip entity ID
    pub entity_bits: u64,
    /// Clip name
    pub name: String,
    /// Track index this clip is on
    pub track_index: u32,
    /// Start time on timeline
    pub timeline_start: Duration,
    /// Duration on timeline
    pub duration: Duration,
    /// Is currently selected
    pub selected: bool,
    /// Custom color (RGBA)
    pub color: Option<[u8; 4]>,
    /// Asset UUID for thumbnail lookup (None for audio-only clips)
    pub asset_uuid: Option<uuid::Uuid>,
    /// Source in point (for filmstrip rendering with slip edits)
    pub source_in: Duration,
    /// Source out point
    pub source_out: Duration,
}

// ============================================================================
// FILMSTRIP TEXTURES
// ============================================================================

/// Pre-loaded filmstrip textures for a single asset
///
/// Used by TimelineWidget to render thumbnail filmstrips on clips.
#[derive(Debug, Clone, Default)]
pub struct FilmstripAssetTextures {
    /// LOD0 thumbnails: (timestamp_seconds, texture_id)
    pub lod0: Vec<(f64, egui::TextureId)>,
    /// LOD1 thumbnails: (timestamp_seconds, texture_id)
    pub lod1: Vec<(f64, egui::TextureId)>,
    /// Thumbnail width in pixels
    pub thumb_width: f32,
    /// Thumbnail height in pixels
    pub thumb_height: f32,
    /// Interval between LOD0 thumbnails (seconds)
    pub lod0_interval: f64,
    /// Interval between LOD1 thumbnails (seconds)
    pub lod1_interval: f64,
}

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
pub struct TimelineWidget {
    /// Widget ID for state persistence
    id: Id,

    /// Timeline name (for header display)
    timeline_name: String,

    /// Frame rate for time calculations
    frame_rate: f64,

    /// Total timeline duration
    duration: Duration,

    /// Track data
    tracks: Vec<TrackInfo>,

    /// Clip data
    clips: Vec<ClipInfo>,

    /// Current playhead position
    playhead_time: Duration,

    /// Horizontal scroll offset (in pixels)
    scroll_x: f32,

    /// Vertical scroll offset (in pixels)
    scroll_y: f32,

    /// Zoom level (pixels per second)
    zoom: f32,

    /// Whether the timeline is playing
    is_playing: bool,

    /// Current drag state (for clip drag-to-move ghost preview)
    drag_state: Option<ClipDragState>,

    /// Current trim state (for clip trim ghost preview)
    trim_state: Option<TrimDragState>,

    /// Current slip state (for slip operation preview)
    slip_state: Option<SlipDragState>,

    /// Current slide state (for slide operation preview)
    slide_state: Option<SlideDragState>,

    /// Active editing tool mode
    active_tool: TimelineTool,

    /// Centered playhead mode: when true, dragging on ruler scrolls timeline
    /// instead of moving playhead (playhead stays centered in view)
    centered_playhead: bool,

    /// Filmstrip thumbnail textures per asset UUID
    /// Maps asset_uuid -> Vec<(timestamp_secs, TextureId)>
    /// Used by draw_clips to render filmstrip thumbnails
    filmstrip_textures: std::collections::HashMap<uuid::Uuid, FilmstripAssetTextures>,
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
            filmstrip_textures: std::collections::HashMap::new(),
        }
    }

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
        self.zoom = zoom.clamp(10.0, 1000.0);
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
        textures: std::collections::HashMap<uuid::Uuid, FilmstripAssetTextures>,
    ) -> Self {
        self.filmstrip_textures = textures;
        self
    }

    // ========================================================================
    // HELPER METHODS
    // ========================================================================

    /// Convert time to X position (relative to content area)
    fn time_to_x(&self, time: Duration) -> f32 {
        time.as_secs_f32() * self.zoom - self.scroll_x
    }

    /// Convert X position to time
    fn x_to_time(&self, x: f32) -> Duration {
        let secs = (x + self.scroll_x) / self.zoom;
        Duration::from_secs_f32(secs.max(0.0))
    }

    /// Get total track height - minimum 60px for empty state drop zone
    fn total_track_height(&self) -> f32 {
        let track_sum: f32 = self.tracks.iter().map(|t| t.height).sum();
        // Ensure minimum height for empty state (drop zone needs space)
        track_sum.max(60.0)
    }

    /// Get track Y position (relative to track area)
    fn track_y(&self, track_index: u32) -> f32 {
        let mut y = 0.0;
        for track in &self.tracks {
            if track.index >= track_index {
                break;
            }
            y += track.height;
        }
        y - self.scroll_y
    }

    /// Get track height
    fn track_height(&self, track_index: u32) -> f32 {
        self.tracks
            .iter()
            .find(|t| t.index == track_index)
            .map(|t| t.height)
            .unwrap_or(VIDEO_TRACK_HEIGHT)
    }

    // ========================================================================
    // DRAWING METHODS
    // ========================================================================

    /// Draw the timeline ruler (timecode display)
    fn draw_ruler(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();

        // Background
        painter.rect_filled(rect, 0.0, colors::RULER_BG);

        // Calculate tick spacing based on zoom
        let seconds_visible = rect.width() / self.zoom;
        let tick_interval = Self::calculate_tick_interval(seconds_visible);
        let minor_ticks = 5;

        // Draw ticks
        let start_time = (self.scroll_x / self.zoom).floor() as i64;
        let end_time = ((self.scroll_x + rect.width()) / self.zoom).ceil() as i64;

        for t in (start_time..=end_time).step_by(tick_interval.max(1) as usize) {
            let x = rect.left() + (t as f32 - self.scroll_x / self.zoom) * self.zoom;
            if x < rect.left() || x > rect.right() {
                continue;
            }

            // Major tick
            painter.line_segment(
                [pos2(x, rect.bottom() - 10.0), pos2(x, rect.bottom())],
                Stroke::new(1.0, colors::RULER_MAJOR_LINE),
            );

            // Time label
            let label = Self::format_timecode(Duration::from_secs(t as u64), self.frame_rate);
            painter.text(
                pos2(x + 4.0, rect.top() + 4.0),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(10.0),
                colors::RULER_TEXT,
            );

            // Minor ticks
            for m in 1..minor_ticks {
                let minor_x =
                    x + (m as f32 / minor_ticks as f32) * tick_interval as f32 * self.zoom;
                if minor_x >= rect.left() && minor_x <= rect.right() {
                    painter.line_segment(
                        [
                            pos2(minor_x, rect.bottom() - 5.0),
                            pos2(minor_x, rect.bottom()),
                        ],
                        Stroke::new(1.0, colors::RULER_MINOR_LINE),
                    );
                }
            }
        }
    }

    /// Calculate appropriate tick interval based on visible time
    fn calculate_tick_interval(seconds_visible: f32) -> i64 {
        if seconds_visible > 3600.0 {
            600 // 10 minutes
        } else if seconds_visible > 600.0 {
            60 // 1 minute
        } else if seconds_visible > 120.0 {
            10
        } else if seconds_visible > 30.0 {
            5
        } else if seconds_visible > 10.0 {
            1
        } else {
            1
        }
    }

    /// Format timecode for display
    fn format_timecode(time: Duration, _frame_rate: f64) -> String {
        let total_secs = time.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }

    /// Draw track lanes (background grid) with labels
    fn draw_track_lanes(&self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter();

        // Fill background
        painter.rect_filled(rect, 0.0, colors::BACKGROUND);

        let mut y = rect.top() - self.scroll_y;
        for (i, track) in self.tracks.iter().enumerate() {
            let lane_rect =
                Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), track.height));

            // Skip if not visible
            if lane_rect.bottom() < rect.top() || lane_rect.top() > rect.bottom() {
                y += track.height;
                continue;
            }

            // Alternating background
            let bg = if i % 2 == 0 {
                colors::TRACK_LANE_BG
            } else {
                colors::TRACK_LANE_ALT
            };
            painter.rect_filled(lane_rect, 0.0, bg);

            // Divider line
            painter.line_segment(
                [
                    pos2(rect.left(), y + track.height),
                    pos2(rect.right(), y + track.height),
                ],
                Stroke::new(1.0, colors::TRACK_DIVIDER),
            );

            y += track.height;
        }
    }

    /// Draw clips on the timeline with hover effects and modern styling
    fn draw_clips(&self, ui: &mut Ui, rect: Rect, hovered_clip: Option<u64>) {
        let painter = ui.painter();

        for clip in &self.clips {
            let x = rect.left() + self.time_to_x(clip.timeline_start);
            let width = clip.duration.as_secs_f32() * self.zoom;
            let y = rect.top() + self.track_y(clip.track_index);
            let height = self.track_height(clip.track_index) - 4.0; // Padding

            // Skip if not visible
            if x + width < rect.left() || x > rect.right() {
                continue;
            }

            // Apply visual gap - shrink clip slightly to create space between adjacent clips
            // This creates the Clipchamp-style gap where the "+" transition button appears
            let visual_gap = CLIP_GAP / 2.0; // 2px on each side = 4px total gap
            let clip_rect = Rect::from_min_size(
                pos2(x + visual_gap, y + 2.0),
                vec2(width - CLIP_GAP, height),
            );
            let is_hovered = hovered_clip == Some(clip.entity_bits);

            // Determine clip color with hover effect
            let fill_color = if clip.selected {
                colors::CLIP_VIDEO // Selected clips stay same color, border changes
            } else if let Some(c) = clip.color {
                let base = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                if is_hovered {
                    base.gamma_multiply(1.2)
                } else {
                    base
                }
            } else {
                // Determine by track type with hover effect
                let is_video = self
                    .tracks
                    .iter()
                    .find(|t| t.index == clip.track_index)
                    .map(|t| t.is_video)
                    .unwrap_or(true);
                if is_video {
                    if is_hovered {
                        colors::CLIP_VIDEO_HOVER
                    } else {
                        colors::CLIP_VIDEO
                    }
                } else {
                    if is_hovered {
                        colors::CLIP_AUDIO_HOVER
                    } else {
                        colors::CLIP_AUDIO
                    }
                }
            };

            // Draw subtle shadow for depth (shifted down-right)
            if clip.selected || is_hovered {
                let shadow_rect = clip_rect.translate(vec2(1.5, 1.5));
                painter.rect_filled(
                    shadow_rect,
                    Rounding::same(4.0),
                    Color32::from_rgba_unmultiplied(0, 0, 0, 40),
                );
            }

            // Check if filmstrip textures are available for this clip
            let has_filmstrip = clip
                .asset_uuid
                .and_then(|uuid| self.filmstrip_textures.get(&uuid))
                .map(|textures| !textures.lod0.is_empty() || !textures.lod1.is_empty())
                .unwrap_or(false);

            // Only draw solid clip body if no filmstrip (fallback background)
            if !has_filmstrip {
                painter.rect_filled(clip_rect, Rounding::same(4.0), fill_color);
            }

            // =================================================================
            // FILMSTRIP RENDERING: Draw thumbnails over clip background
            // =================================================================
            // Only render for clips with asset_uuid and available textures
            if let Some(asset_uuid) = clip.asset_uuid {
                if let Some(asset_textures) = self.filmstrip_textures.get(&asset_uuid) {
                    // Select LOD based on zoom level (pixels per second)
                    let lod_level: u8 =
                        super::filmstrip::get_lod_for_pixels_per_second(self.zoom as f64);
                    let thumbnails_slice: &[(f64, egui::TextureId)] = if lod_level == 0 {
                        &asset_textures.lod0
                    } else {
                        &asset_textures.lod1
                    };

                    // Only render if we have thumbnails
                    if !thumbnails_slice.is_empty() {
                        // Build FilmstripThumbnails for renderer
                        let filmstrip_thumbs = super::filmstrip::FilmstripThumbnails {
                            thumbnails: thumbnails_slice,
                            thumb_width: asset_textures.thumb_width,
                            thumb_height: asset_textures.thumb_height,
                            interval_seconds: if lod_level == 0 {
                                asset_textures.lod0_interval
                            } else {
                                asset_textures.lod1_interval
                            },
                        };

                        // Inset the filmstrip slightly for visual separation
                        let filmstrip_rect = clip_rect.shrink2(vec2(2.0, 2.0));

                        // Create renderer and draw filmstrip
                        let renderer = super::filmstrip::FilmstripRenderer::new();
                        renderer.draw(
                            &painter,
                            filmstrip_rect,
                            &filmstrip_thumbs,
                            clip.source_in,
                            clip.source_out,
                            self.zoom as f64,
                            Some(rect), // Viewport for culling
                        );
                    }
                }
            }

            // Border styling - Blue highlight for selected, subtle for normal
            if clip.selected {
                // Clean selection border (DaVinci style)
                painter.rect_stroke(
                    clip_rect,
                    Rounding::same(4.0),
                    Stroke::new(2.0, colors::CLIP_SELECTED_BORDER),
                );
            } else if is_hovered {
                painter.rect_stroke(
                    clip_rect,
                    Rounding::same(4.0),
                    Stroke::new(1.5, colors::ACCENT_CYAN), // Cyan hover border
                );
            } else {
                painter.rect_stroke(
                    clip_rect,
                    Rounding::same(4.0),
                    Stroke::new(1.0, colors::CLIP_BORDER),
                );
            }

            // Draw visible trim handles on edges - more visible when hovered/selected
            let handle_alpha = if clip.selected || is_hovered {
                0.7
            } else {
                0.35
            };
            let handle_color = Color32::from_rgb(255, 255, 255).gamma_multiply(handle_alpha);

            // Left handle
            let left_handle =
                Rect::from_min_size(pos2(clip_rect.left(), clip_rect.top()), vec2(5.0, height));
            painter.rect_filled(
                left_handle,
                Rounding {
                    nw: 4.0,
                    sw: 4.0,
                    ne: 0.0,
                    se: 0.0,
                },
                handle_color,
            );

            // Right handle
            let right_handle = Rect::from_min_size(
                pos2(clip_rect.right() - 5.0, clip_rect.top()),
                vec2(5.0, height),
            );
            painter.rect_filled(
                right_handle,
                Rounding {
                    nw: 0.0,
                    sw: 0.0,
                    ne: 4.0,
                    se: 4.0,
                },
                handle_color,
            );

            // Clip name (clipped to bounds)
            if width > 40.0 {
                let text_rect = clip_rect.shrink2(vec2(10.0, 4.0)); // More padding to avoid handles
                ui.painter_at(text_rect).text(
                    text_rect.left_center(),
                    Align2::LEFT_CENTER,
                    &clip.name,
                    FontId::proportional(11.0),
                    colors::CLIP_TEXT,
                );
            }
        }
    }

    /// Draw transition zones between adjacent clips
    /// Shows a "+" button when hovering over the gap between clips (Clipchamp style)
    fn draw_transition_zones(&self, ui: &mut Ui, rect: Rect, hovered_gap: Option<(u64, u64)>) {
        let painter = ui.painter();

        // Find adjacent clip pairs on the same track
        for track in &self.tracks {
            // Get clips on this track, sorted by start time
            let mut track_clips: Vec<_> = self
                .clips
                .iter()
                .filter(|c| c.track_index == track.index)
                .collect();
            track_clips.sort_by(|a, b| a.timeline_start.cmp(&b.timeline_start));

            // Check pairs of adjacent clips
            for window in track_clips.windows(2) {
                let clip_a = window[0];
                let clip_b = window[1];

                // Calculate gap position
                let a_end_x = rect.left()
                    + self.time_to_x(clip_a.timeline_start)
                    + clip_a.duration.as_secs_f32() * self.zoom;
                let b_start_x = rect.left() + self.time_to_x(clip_b.timeline_start);

                // If clips are close enough (within ~20 pixels), show transition zone
                let gap_width = b_start_x - a_end_x;
                if gap_width > -10.0 && gap_width < 30.0 {
                    let track_y = rect.top() + self.track_y(track.index);
                    let track_height = self.track_height(track.index);

                    // Center of the gap
                    let gap_center_x = (a_end_x + b_start_x) / 2.0;
                    let gap_center_y = track_y + track_height / 2.0;

                    // Check if this gap is being hovered
                    let is_hovered = hovered_gap == Some((clip_a.entity_bits, clip_b.entity_bits));

                    if is_hovered {
                        // Draw the "+" button pill
                        let button_rect = Rect::from_center_size(
                            pos2(gap_center_x, gap_center_y),
                            vec2(TRANSITION_BUTTON_SIZE, TRANSITION_BUTTON_SIZE),
                        );

                        // Button background
                        painter.rect_filled(
                            button_rect,
                            Rounding::same(TRANSITION_BUTTON_SIZE / 2.0),
                            colors::TRANSITION_PLUS_BG,
                        );

                        // "+" symbol
                        let plus_size = 8.0;
                        let center = button_rect.center();

                        // Horizontal line
                        painter.line_segment(
                            [
                                pos2(center.x - plus_size / 2.0, center.y),
                                pos2(center.x + plus_size / 2.0, center.y),
                            ],
                            Stroke::new(2.0, Color32::WHITE),
                        );

                        // Vertical line
                        painter.line_segment(
                            [
                                pos2(center.x, center.y - plus_size / 2.0),
                                pos2(center.x, center.y + plus_size / 2.0),
                            ],
                            Stroke::new(2.0, Color32::WHITE),
                        );
                    }
                }
            }
        }
    }

    /// Draw the playhead
    /// In centered_playhead mode: playhead is at center after scrolling past threshold
    /// At the beginning (time < center_time), playhead draws at actual time position
    fn draw_playhead(&self, ui: &mut Ui, ruler_rect: Rect, content_rect: Rect) {
        let painter = ui.painter();

        // Calculate when playhead should be at center vs at actual position
        let center_time = (content_rect.width() / 2.0) / self.zoom;
        let should_center =
            self.centered_playhead && self.playhead_time.as_secs_f32() >= center_time;

        // Calculate playhead x position
        let x = if should_center {
            // CapCut style: playhead at center (after scrolling past threshold)
            content_rect.center().x
        } else {
            // At start or normal mode: playhead at time position
            content_rect.left() + self.time_to_x(self.playhead_time)
        };

        // Skip if not visible
        if x < content_rect.left() || x > content_rect.right() {
            return;
        }

        // Draw shadow
        painter.line_segment(
            [
                pos2(x + 1.0, ruler_rect.top()),
                pos2(x + 1.0, content_rect.bottom()),
            ],
            Stroke::new(3.0, colors::playhead_shadow()),
        );

        // Draw playhead line
        painter.line_segment(
            [pos2(x, ruler_rect.top()), pos2(x, content_rect.bottom())],
            Stroke::new(PLAYHEAD_WIDTH, colors::PLAYHEAD),
        );

        // Draw playhead head (triangle)
        let head_center = pos2(x, ruler_rect.bottom());
        let half_size = PLAYHEAD_HEAD_SIZE / 2.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(
                    head_center.x - half_size,
                    ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE,
                ),
                pos2(
                    head_center.x + half_size,
                    ruler_rect.bottom() - PLAYHEAD_HEAD_SIZE,
                ),
                pos2(head_center.x, ruler_rect.bottom()),
            ],
            colors::PLAYHEAD,
            Stroke::NONE,
        ));
    }
}

// ============================================================================
// WIDGET IMPLEMENTATION
// ============================================================================

/// Response from timeline widget with interaction data
pub struct TimelineResponse {
    /// Base egui response
    pub response: Response,
    /// New playhead time if user clicked on ruler
    pub seek_to: Option<Duration>,
    /// Clip that was clicked
    pub clicked_clip: Option<u64>,
    /// Clip that was double-clicked
    pub double_clicked_clip: Option<u64>,
    /// New scroll position
    pub scroll_changed: Option<(f32, f32)>,
    /// New zoom level
    pub zoom_changed: Option<f32>,
    /// Track mute button toggled (track entity bits, current state)
    pub toggle_track_mute: Option<(u64, bool)>,
    /// Track solo button toggled (track entity bits, current state)
    pub toggle_track_solo: Option<(u64, bool)>,
    /// Track lock button toggled (track entity bits, current state)
    pub toggle_track_lock: Option<(u64, bool)>,
    /// Clip move completed (clip_entity, target_track_entity, new_time_seconds)
    /// This is emitted when a drag operation completes (mouse release)
    pub move_clip: Option<(u64, Option<u64>, f64)>,
    /// Updated drag state to persist between frames
    /// Pass this back to the widget on the next frame via with_drag_state()
    pub updated_drag_state: Option<ClipDragState>,
    /// Clip that was right-clicked (for context menu)
    /// Contains (clip_entity_bits, click_position) for context menu positioning
    pub context_menu_clip: Option<(u64, egui::Pos2)>,
    /// Trim operation completed (clip_entity_bits, edge, new_value_seconds)
    /// Emitted when a trim drag operation completes
    pub trim_clip: Option<(u64, TrimEdge, f64)>,
    /// Updated trim drag state to persist between frames
    pub updated_trim_state: Option<TrimDragState>,

    // =========================================================================
    // NEW: Editing tool response fields
    // =========================================================================
    /// Razor cut requested at position (timeline_entity_bits, time_seconds)
    /// Emitted when user clicks with Razor tool active
    pub razor_cut: Option<(u64, f64)>,

    /// Slip operation completed (clip_entity_bits, offset_seconds)
    /// Emitted when a slip drag operation completes
    pub slip_clip: Option<(u64, f64)>,

    /// Updated slip drag state to persist between frames
    pub updated_slip_state: Option<SlipDragState>,

    /// Slide operation completed (clip_entity_bits, delta_seconds)
    /// Emitted when a slide drag operation completes
    pub slide_clip: Option<(u64, f64)>,

    /// Updated slide drag state to persist between frames
    pub updated_slide_state: Option<SlideDragState>,

    /// Ripple delete requested for a clip (clip_entity_bits)
    /// Emitted when user presses Shift+Delete on selected clip
    pub ripple_delete: Option<u64>,

    /// Tool mode changed - new active tool
    pub tool_changed: Option<TimelineTool>,

    /// Current active tool (for cursor display purposes)
    pub active_tool: TimelineTool,

    /// Whether user is actively scrubbing (dragging playhead/ruler)
    ///
    /// True during drag, false when drag ends. Consuming code should use this
    /// to update `ScrubState` resource for compositor optimization.
    pub is_scrubbing: bool,
}

impl Widget for TimelineWidget {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

impl TimelineWidget {
    /// Show the timeline widget and return interaction response
    ///
    /// This is the preferred method to use as it returns a `TimelineResponse`
    /// with semantic interaction data (seek_to, clicked_clip, etc.).
    pub fn show(mut self, ui: &mut Ui) -> TimelineResponse {
        // Calculate layout rectangles
        let available = ui.available_rect_before_wrap();
        let total_height = RULER_HEIGHT + self.total_track_height() + SCROLLBAR_HEIGHT;
        let widget_rect = Rect::from_min_size(
            available.min,
            vec2(available.width(), total_height.min(available.height())),
        );

        // Allocate space with click and drag sensing
        let (response, painter) = ui.allocate_painter(widget_rect.size(), Sense::click_and_drag());
        let rect = response.rect;

        // Layout regions
        let ruler_rect = Rect::from_min_size(
            pos2(rect.left() + TRACK_HEADER_WIDTH, rect.top()),
            vec2(rect.width() - TRACK_HEADER_WIDTH, RULER_HEIGHT),
        );

        let content_rect = Rect::from_min_size(
            pos2(rect.left() + TRACK_HEADER_WIDTH, rect.top() + RULER_HEIGHT),
            vec2(
                rect.width() - TRACK_HEADER_WIDTH,
                rect.height() - RULER_HEIGHT - SCROLLBAR_HEIGHT,
            ),
        );

        // Draw components

        // CapCut style: in centered mode, auto-compute scroll_x based on playhead_time
        // This ensures clips are always positioned correctly relative to playhead
        if self.centered_playhead {
            let center_offset = content_rect.width() / 2.0;
            let center_time = center_offset / self.zoom; // Time in seconds at center
            let playhead_pixel_pos = self.playhead_time.as_secs_f32() * self.zoom;

            if self.playhead_time.as_secs_f32() >= center_time {
                // After threshold: playhead is centered, compute scroll_x to achieve that
                self.scroll_x = playhead_pixel_pos - center_offset;
            } else {
                // Before threshold: scroll_x = 0, playhead moves from left toward center
                self.scroll_x = 0.0;
            }
        }

        self.draw_ruler(ui, ruler_rect);

        // CapCut style: show content based on clips, not tracks
        if self.clips.is_empty() {
            // Empty state: CapCut-style drop zone
            // Fill background
            painter.rect_filled(content_rect, 0.0, colors::BACKGROUND);

            // Calculate drop zone pill dimensions (like CapCut)
            let pill_width = (content_rect.width() * 0.8).min(500.0);
            let pill_height = 40.0;
            let pill_rect =
                Rect::from_center_size(content_rect.center(), vec2(pill_width, pill_height));

            // Draw rounded pill background
            painter.rect_filled(
                pill_rect,
                Rounding::same(8.0),
                Color32::from_rgb(45, 48, 55), // Slightly lighter than background
            );

            // Draw border
            painter.rect_stroke(
                pill_rect,
                Rounding::same(8.0),
                Stroke::new(1.0, Color32::from_rgb(70, 75, 85)),
            );

            // Draw icon (film icon placeholder - simple rectangles)
            let icon_size = 16.0;
            let icon_x = pill_rect.left() + 16.0;
            let icon_y = pill_rect.center().y;
            let icon_rect =
                Rect::from_center_size(pos2(icon_x + 8.0, icon_y), vec2(icon_size, icon_size));
            painter.rect_filled(icon_rect, 2.0, Color32::from_rgb(90, 95, 105));

            // Draw text
            painter.text(
                pos2(icon_x + icon_size + 12.0, icon_y),
                Align2::LEFT_CENTER,
                "🎬 Drag videos or images here to create videos!",
                FontId::proportional(13.0),
                Color32::from_rgb(140, 145, 155),
            );
        } else {
            // Draw track lanes and clips when we have content
            self.draw_track_lanes(ui, content_rect);

            // Detect which clip is currently hovered for visual feedback
            let hovered_clip = ui.input(|i| i.pointer.latest_pos()).and_then(|pos| {
                if content_rect.contains(pos) {
                    self.hit_test_clip(pos, content_rect)
                } else {
                    None
                }
            });

            self.draw_clips(ui, content_rect, hovered_clip);

            // Detect hovered gap for transition plus icon
            let hovered_gap = ui.input(|i| i.pointer.latest_pos()).and_then(|pos| {
                if content_rect.contains(pos) {
                    self.hit_test_gap(pos, content_rect)
                } else {
                    None
                }
            });

            // Draw transition zones between clips (shows + on hover)
            self.draw_transition_zones(ui, content_rect, hovered_gap);
        }

        self.draw_playhead(ui, ruler_rect, content_rect);

        // No scrollbar (cleaner CapCut style)
        let scrollbar_rect = Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - SCROLLBAR_HEIGHT),
            vec2(rect.width(), SCROLLBAR_HEIGHT),
        );
        painter.rect_filled(scrollbar_rect, 0.0, colors::SCROLLBAR_BG);

        // =====================================================================
        // INTERACTION HANDLING
        // =====================================================================

        let mut seek_to = None;
        let mut clicked_clip = None;
        let mut context_menu_clip = None;
        let mut zoom_changed = None;
        let mut scroll_changed_val: Option<(f32, f32)> = None;

        // Handle clicks - clip selection works in ALL modes, seek only in normal mode
        if response.clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if ruler_rect.contains(pointer_pos) {
                    // Click on ruler = seek (only in normal mode)
                    if !self.centered_playhead {
                        let x_in_content = pointer_pos.x - ruler_rect.left();
                        seek_to = Some(self.x_to_time(x_in_content));
                    }
                    // In centered mode, ruler click does nothing (use drag to scrub)
                } else if content_rect.contains(pointer_pos) {
                    // Click in content area = ALWAYS check for clip hit (selection works in all modes)
                    clicked_clip = self.hit_test_clip(pointer_pos, content_rect);

                    // If no clip was hit but still clicked in content, seek (only in normal mode)
                    if clicked_clip.is_none() && !self.centered_playhead {
                        let x_in_content = pointer_pos.x - content_rect.left();
                        seek_to = Some(self.x_to_time(x_in_content));
                    }
                }
            }
        }

        // =====================================================================
        // DRAG ORIGIN TRACKING (Critical for proper operation routing)
        // =====================================================================

        // Retrieve current drag origin from memory (persists across frames)
        let mut drag_origin: DragOrigin = ui.memory(|mem| {
            mem.data
                .get_temp::<DragOrigin>(egui::Id::new("timeline_drag_origin"))
                .unwrap_or(DragOrigin::None)
        });

        // On drag start, determine WHERE the drag originated
        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                drag_origin = if ruler_rect.contains(pointer_pos) {
                    DragOrigin::Ruler
                } else if content_rect.contains(pointer_pos) {
                    // Check what's under the cursor - trim edge, clip body, or empty
                    if self.hit_test_clip_edge(pointer_pos, content_rect).is_some() {
                        DragOrigin::TrimEdge
                    } else if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                        DragOrigin::ClipBody
                    } else {
                        DragOrigin::Empty
                    }
                } else {
                    DragOrigin::None
                };

                // Store for next frame
                ui.memory_mut(|mem| {
                    mem.data
                        .insert_temp(egui::Id::new("timeline_drag_origin"), drag_origin);
                });
            }
        }

        // Clear drag origin when drag ends
        if response.drag_stopped() {
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<DragOrigin>(egui::Id::new("timeline_drag_origin"));
            });
            drag_origin = DragOrigin::None;
        }

        // =====================================================================
        // PLAYHEAD SCRUBBING / CENTERED MODE (CapCut style)
        // In centered mode: playhead is at center, dragging changes current time
        //                   which makes clips appear to scroll under fixed playhead
        // Scrub works from: Ruler, Empty content area, AND clips (when not actively moving)
        // Moving/Trimming is handled separately and takes priority when active
        // =====================================================================

        // Scrub from anywhere UNLESS we're in an active clip move or trim operation
        let is_moving_clip = self.drag_state.is_some();
        let is_trimming = self.trim_state.is_some();
        let can_scrub = !is_moving_clip
            && !is_trimming
            && matches!(
                drag_origin,
                DragOrigin::Ruler | DragOrigin::Empty | DragOrigin::ClipBody
            );

        // Track if we're currently scrubbing for ScrubState signaling
        // This starts with drag-based detection but can also be set by scroll-based seeking
        let mut is_scrubbing = response.dragged() && can_scrub;

        // DEBUG: Log scrubbing state for diagnostics
        if response.dragged() {
            tracing::debug!(
                "Timeline: dragged=true, can_scrub={}, drag_origin={:?}, is_moving={}, is_trimming={}, is_scrubbing={}",
                can_scrub, drag_origin, is_moving_clip, is_trimming, is_scrubbing
            );
        }

        if is_scrubbing {
            let drag_delta = response.drag_delta();
            if drag_delta.x.abs() > 0.1 {
                // Convert pixel drag to time change
                // Dragging left (negative x) = scrub forward (increase time)
                // Dragging right (positive x) = scrub backward (decrease time)
                let time_change_secs = drag_delta.x.abs() / self.zoom;

                let new_time = if drag_delta.x < 0.0 {
                    // Dragging left = scrubbing forward (add time)
                    self.playhead_time
                        .saturating_add(Duration::from_secs_f32(time_change_secs))
                } else {
                    // Dragging right = scrubbing backward (subtract time)
                    self.playhead_time
                        .saturating_sub(Duration::from_secs_f32(time_change_secs))
                };

                seek_to = Some(new_time);
            }
        }

        // Handle right-click (secondary click) for context menu
        if response.secondary_clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    // Check for clip hit on right-click
                    if let Some(entity_bits) = self.hit_test_clip(pointer_pos, content_rect) {
                        context_menu_clip = Some((entity_bits, pointer_pos));
                    }
                }
            }
        }

        // =====================================================================
        // SCROLL HANDLING (Improved for touchpad/mouse)
        // =====================================================================

        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        let modifiers = ui.input(|i| i.modifiers);
        // Note: scroll_changed_val is declared earlier in the interaction section

        if response.hovered() && (scroll_delta.x.abs() > 0.01 || scroll_delta.y.abs() > 0.01) {
            if modifiers.ctrl {
                // Ctrl + scroll = zoom
                if scroll_delta.y.abs() > 0.01 {
                    let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                    let new_zoom = (self.zoom * zoom_factor).clamp(MIN_ZOOM, MAX_ZOOM);
                    zoom_changed = Some(new_zoom);
                }
            } else {
                // Default behavior for timelines:
                // - Horizontal scroll (trackpad horizontal swipe) = pan timeline
                // - Vertical scroll (mouse wheel) = also pan timeline (common in NLEs)
                // - Shift + vertical = vertical track scroll (when we have many tracks)

                let (h_scroll, v_scroll) = if modifiers.shift {
                    // Shift inverts: vertical becomes track scroll
                    (0.0, scroll_delta.y)
                } else {
                    // Normal: both horizontal and vertical scroll the timeline
                    // Trackpads send horizontal in x, mice send wheel in y
                    (scroll_delta.x + scroll_delta.y, 0.0)
                };

                if h_scroll.abs() > 0.01 {
                    if self.centered_playhead {
                        // In centered mode: scroll changes playhead_time (seek)
                        // Scroll up/right = go forward, scroll down/left = go backward
                        let time_change = h_scroll.abs() / self.zoom;

                        // Snap to frame boundaries for smoother seeking
                        let frame_duration = 1.0_f32 / self.frame_rate as f32;
                        let frames_to_move = (time_change / frame_duration).round().max(1.0);
                        let snapped_time_change = frames_to_move * frame_duration;

                        let new_time = if h_scroll > 0.0 {
                            self.playhead_time
                                .saturating_sub(Duration::from_secs_f32(snapped_time_change))
                        } else {
                            self.playhead_time
                                .saturating_add(Duration::from_secs_f32(snapped_time_change))
                        };
                        seek_to = Some(new_time);
                        // Scroll-based seek in centered mode = scrubbing
                        is_scrubbing = true;
                    } else {
                        // Normal mode: scroll changes scroll_x
                        let new_scroll_x = (self.scroll_x - h_scroll).max(0.0);
                        scroll_changed_val = Some((new_scroll_x, self.scroll_y));
                    }
                }

                if v_scroll.abs() > 0.01 {
                    let new_scroll_y = (self.scroll_y - v_scroll).max(0.0);
                    scroll_changed_val = Some((
                        scroll_changed_val.map(|(x, _)| x).unwrap_or(self.scroll_x),
                        new_scroll_y,
                    ));
                }
            }
        }

        // Track button toggles (for future context menu or keyboard shortcuts)
        let toggle_track_mute: Option<(u64, bool)> = None;
        let toggle_track_solo: Option<(u64, bool)> = None;
        let toggle_track_lock: Option<(u64, bool)> = None;

        // =====================================================================
        // KEYBOARD SHORTCUT HANDLING
        // =====================================================================

        let mut tool_changed: Option<TimelineTool> = None;
        let mut ripple_delete: Option<u64> = None;
        let mut razor_cut: Option<(u64, f64)> = None;

        // Only process keyboard when timeline has focus (is hovered or was clicked)
        let has_focus = response.has_focus();
        let is_hovered = response.hovered();

        // DEBUG: Log focus state when Delete is pressed (even if not focused)
        ui.input(|i| {
            if i.key_pressed(egui::Key::Delete) {
                tracing::debug!(
                    "TimelineWidget: DELETE pressed. Focus: {}, Hover: {}",
                    has_focus,
                    is_hovered
                );
            }
        });

        if is_hovered || has_focus {
            ui.input(|input| {
                // TRACE: Log any key press to verify focus (avoid spam)
                if !input.keys_down.is_empty() {
                    tracing::trace!("TimelineWidget: Keys down: {:?}", input.keys_down);
                }

                // Tool switching shortcuts (only when no modifiers held)
                if !input.modifiers.any() {
                    if input.key_pressed(egui::Key::V) {
                        tool_changed = Some(TimelineTool::Pointer);
                    } else if input.key_pressed(egui::Key::B) {
                        tool_changed = Some(TimelineTool::Razor);
                    } else if input.key_pressed(egui::Key::Y) {
                        tool_changed = Some(TimelineTool::Slip);
                    } else if input.key_pressed(egui::Key::U) {
                        tool_changed = Some(TimelineTool::Slide);
                    }
                }

                // Alt+Delete OR Shift+Backspace = Ripple Delete (on selected clip)
                if (input.modifiers.alt && input.key_pressed(egui::Key::Delete))
                    || (input.modifiers.shift && input.key_pressed(egui::Key::Backspace))
                {
                    tracing::debug!("TimelineWidget: Ripple Delete shortcut detected");
                    // Find selected clip (first one found)
                    let selected_count = self.clips.iter().filter(|c| c.selected).count();
                    tracing::debug!(
                        "TimelineWidget: Shift+Delete pressed. Clips: {}, Selected: {}",
                        self.clips.len(),
                        selected_count
                    );
                    if let Some(selected) = self.clips.iter().find(|c| c.selected) {
                        ripple_delete = Some(selected.entity_bits);
                        tracing::info!(
                            "TimelineWidget: Ripple delete triggered for clip: {}",
                            selected.entity_bits
                        );
                    } else {
                        tracing::debug!("TimelineWidget: No clip selected for ripple delete");
                    }
                }
            });
        }

        // Razor tool click handling
        if self.active_tool == TimelineTool::Razor && response.clicked() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    let x_in_content = pointer_pos.x - content_rect.left();
                    let cut_time = self.x_to_time(x_in_content);
                    // We need the timeline entity - for now use 0 as placeholder
                    // The app layer should know which timeline is active
                    razor_cut = Some((0, cut_time.as_secs_f64()));
                }
            }
        }

        // =====================================================================
        // CURSOR CHANGE ON HOVER (Trim Handle + Playhead Feedback)
        // =====================================================================

        // Check if hovering and change cursor appropriately
        if response.hovered() {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                // Playhead grab zone - near the playhead head
                let playhead_x = content_rect.left() + self.time_to_x(self.playhead_time);
                let playhead_grab_rect = Rect::from_center_size(
                    pos2(playhead_x, ruler_rect.center().y),
                    vec2(PLAYHEAD_GRAB_WIDTH, ruler_rect.height()),
                );

                if playhead_grab_rect.contains(pointer_pos) || ruler_rect.contains(pointer_pos) {
                    // Hovering on playhead or ruler - show grab/text cursor
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                } else if content_rect.contains(pointer_pos) {
                    // Tool-based cursor handling
                    match self.active_tool {
                        TimelineTool::Razor => {
                            // Razor tool always shows crosshair in content area
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                        TimelineTool::Slip => {
                            // Slip tool shows horizontal resize on clips
                            if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            }
                        }
                        TimelineTool::Slide => {
                            // Slide tool shows grab cursor on clips
                            if self.hit_test_clip(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        }
                        TimelineTool::Pointer => {
                            // Pointer: Check for trim edge first, then default
                            if self.hit_test_clip_edge(pointer_pos, content_rect).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            }
                        }
                    }
                }
            }
        }

        // =====================================================================
        // DRAG HANDLING (Ghost Item Pattern from Olive)
        // =====================================================================

        let mut move_clip = None;
        let mut trim_clip = None;
        let mut updated_drag_state = self.drag_state.clone();
        let mut updated_trim_state = self.trim_state.clone();

        // Check for drag start - first check trim edges, then clip body
        if response.drag_started() {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if content_rect.contains(pointer_pos) {
                    // Priority 1: Check for trim edge drag
                    if let Some((entity_bits, edge)) =
                        self.hit_test_clip_edge(pointer_pos, content_rect)
                    {
                        // Find clip info for trim
                        if let Some(clip) = self.clips.iter().find(|c| c.entity_bits == entity_bits)
                        {
                            updated_trim_state = Some(TrimDragState {
                                clip_entity: entity_bits,
                                edge,
                                original_timeline_start: clip.timeline_start,
                                original_duration: clip.duration,
                                ghost_value: match edge {
                                    TrimEdge::Left => clip.timeline_start,
                                    TrimEdge::Right => clip.duration,
                                },
                                is_active: true,
                            });
                            // Clear any move drag since we're trimming
                            updated_drag_state = None;
                        }
                    } else if let Some(entity_bits) = self.hit_test_clip(pointer_pos, content_rect)
                    {
                        // Priority 2: Normal clip move drag
                        if let Some(clip) = self.clips.iter().find(|c| c.entity_bits == entity_bits)
                        {
                            updated_drag_state = Some(ClipDragState {
                                clip_entity: entity_bits,
                                original_track: clip.track_index,
                                original_time: clip.timeline_start,
                                ghost_track: clip.track_index,
                                ghost_time: clip.timeline_start,
                                duration: clip.duration,
                                is_active: true,
                            });
                        }
                    }
                }
            }
        }

        // Update positions during drag
        if response.dragged() {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.latest_pos()) {
                // Update trim drag
                if let Some(ref mut trim) = updated_trim_state {
                    if trim.is_active && content_rect.contains(pointer_pos) {
                        let x_in_content = pointer_pos.x - content_rect.left();
                        let new_time = self.x_to_time(x_in_content);

                        match trim.edge {
                            TrimEdge::Left => {
                                // Trim In: ghost_value is the new timeline_start
                                trim.ghost_value = new_time;
                            }
                            TrimEdge::Right => {
                                // Trim Out: calculate new duration based on pointer
                                // duration = pointer_time - original_timeline_start
                                let new_duration =
                                    new_time.saturating_sub(trim.original_timeline_start);
                                trim.ghost_value = new_duration;
                            }
                        }

                        // Auto-scroll when trimming near edges
                        let edge_threshold = 120.0; // pixels from edge
                        let scroll_speed = 2.0; // frames per tick
                        let frame_duration = 1.0_f32 / self.frame_rate as f32;

                        if pointer_pos.x < content_rect.left() + edge_threshold {
                            // Near left edge - scroll backward
                            let scroll_amount = scroll_speed * frame_duration;
                            seek_to = Some(
                                self.playhead_time
                                    .saturating_sub(Duration::from_secs_f32(scroll_amount)),
                            );
                        } else if pointer_pos.x > content_rect.right() - edge_threshold {
                            // Near right edge - scroll forward
                            let scroll_amount = scroll_speed * frame_duration;
                            seek_to = Some(
                                self.playhead_time
                                    .saturating_add(Duration::from_secs_f32(scroll_amount)),
                            );
                        }
                    }
                }

                // Update move drag
                if let Some(ref mut drag) = updated_drag_state {
                    if drag.is_active && content_rect.contains(pointer_pos) {
                        // Calculate new ghost position
                        let x_in_content = pointer_pos.x - content_rect.left();
                        drag.ghost_time = self.x_to_time(x_in_content);

                        // Calculate ghost track from Y position
                        let y_in_content = pointer_pos.y - content_rect.top() + self.scroll_y;
                        let mut accumulated_y = 0.0;
                        for track in &self.tracks {
                            if y_in_content < accumulated_y + track.height {
                                drag.ghost_track = track.index;
                                break;
                            }
                            accumulated_y += track.height;
                        }
                    }

                    // Auto-scroll when moving near edges (works even outside content_rect)
                    if drag.is_active {
                        let edge_threshold = 120.0;
                        let scroll_speed = 2.0;
                        let frame_duration = 1.0_f32 / self.frame_rate as f32;

                        if pointer_pos.x < content_rect.left() + edge_threshold {
                            let scroll_amount = scroll_speed * frame_duration;
                            seek_to = Some(
                                self.playhead_time
                                    .saturating_sub(Duration::from_secs_f32(scroll_amount)),
                            );
                        } else if pointer_pos.x > content_rect.right() - edge_threshold {
                            let scroll_amount = scroll_speed * frame_duration;
                            seek_to = Some(
                                self.playhead_time
                                    .saturating_add(Duration::from_secs_f32(scroll_amount)),
                            );
                        }
                    }
                }
            }
        }

        // Complete drag on release
        if response.drag_stopped() {
            // Handle trim completion
            if let Some(ref trim) = updated_trim_state {
                if trim.is_active {
                    // Only emit if actually changed
                    let value_changed = match trim.edge {
                        TrimEdge::Left => {
                            (trim.ghost_value.as_secs_f64()
                                - trim.original_timeline_start.as_secs_f64())
                            .abs()
                                > 0.001
                        }
                        TrimEdge::Right => {
                            (trim.ghost_value.as_secs_f64() - trim.original_duration.as_secs_f64())
                                .abs()
                                > 0.001
                        }
                    };

                    if value_changed {
                        // For Trim Out, we need to pass new_source_out
                        // new_source_out = source_in + new_duration
                        let trim_value = match trim.edge {
                            TrimEdge::Left => trim.ghost_value.as_secs_f64(),
                            TrimEdge::Right => {
                                // ghost_value is the new duration
                                // but TrimClipCommand needs source_out
                                // We'll pass it and let the handler calculate
                                trim.ghost_value.as_secs_f64()
                            }
                        };

                        trim_clip = Some((trim.clip_entity, trim.edge, trim_value));
                    }
                }
            }
            // Clear trim state
            updated_trim_state = None;

            // Handle move completion
            if let Some(ref drag) = updated_drag_state {
                if drag.is_active {
                    // Find target track entity from index
                    let target_track_entity = self
                        .tracks
                        .iter()
                        .find(|t| t.index == drag.ghost_track)
                        .map(|t| t.entity_bits);

                    // Only emit move if position actually changed
                    let time_changed =
                        (drag.ghost_time.as_secs_f64() - drag.original_time.as_secs_f64()).abs()
                            > 0.001;
                    let track_changed = drag.ghost_track != drag.original_track;

                    if time_changed || track_changed {
                        move_clip = Some((
                            drag.clip_entity,
                            target_track_entity,
                            drag.ghost_time.as_secs_f64(),
                        ));
                    }
                }
            }
            // Clear drag state
            updated_drag_state = None;
        }

        // =====================================================================
        // GHOST PREVIEW RENDERING
        // =====================================================================

        // Draw ghost preview for trim
        if let Some(ref trim) = updated_trim_state {
            if trim.is_active {
                // Find original clip for reference
                if let Some(clip) = self
                    .clips
                    .iter()
                    .find(|c| c.entity_bits == trim.clip_entity)
                {
                    let track_y = self.track_y(clip.track_index) + content_rect.top();
                    let track_height = self.track_height(clip.track_index);

                    let (ghost_x, ghost_width) = match trim.edge {
                        TrimEdge::Left => {
                            let orig_end = trim.original_timeline_start + trim.original_duration;
                            let new_start = trim.ghost_value;
                            let new_duration = orig_end.saturating_sub(new_start);
                            (
                                self.time_to_x(new_start) + content_rect.left(),
                                new_duration.as_secs_f32() * self.zoom,
                            )
                        }
                        TrimEdge::Right => {
                            let x =
                                self.time_to_x(trim.original_timeline_start) + content_rect.left();
                            let width = trim.ghost_value.as_secs_f32() * self.zoom;
                            (x, width)
                        }
                    };

                    let ghost_rect = Rect::from_min_size(
                        pos2(ghost_x, track_y),
                        vec2(ghost_width.max(4.0), track_height - 2.0),
                    );

                    // Draw semi-transparent orange ghost for trim
                    painter.rect_filled(
                        ghost_rect,
                        4.0,
                        Color32::from_rgba_unmultiplied(255, 180, 100, 100),
                    );
                    painter.rect_stroke(
                        ghost_rect,
                        4.0,
                        Stroke::new(2.0, Color32::from_rgb(255, 180, 100)),
                    );
                }
            }
        }

        // Draw ghost preview for move
        if let Some(ref drag) = updated_drag_state {
            if drag.is_active {
                let ghost_x = self.time_to_x(drag.ghost_time) + content_rect.left();
                let ghost_width = drag.duration.as_secs_f32() * self.zoom;
                let ghost_y = self.track_y(drag.ghost_track) + content_rect.top();
                let ghost_height = self.track_height(drag.ghost_track);

                let ghost_rect = Rect::from_min_size(
                    pos2(ghost_x, ghost_y),
                    vec2(ghost_width, ghost_height - 2.0),
                );

                // Draw semi-transparent blue ghost for move
                painter.rect_filled(
                    ghost_rect,
                    4.0, // rounded corners
                    Color32::from_rgba_unmultiplied(100, 180, 255, 100),
                );
                painter.rect_stroke(
                    ghost_rect,
                    4.0,
                    Stroke::new(2.0, Color32::from_rgb(100, 180, 255)),
                );
            }
        }

        TimelineResponse {
            response,
            seek_to,
            clicked_clip,
            double_clicked_clip: None, // TODO: implement double-click detection
            scroll_changed: scroll_changed_val,
            zoom_changed,
            toggle_track_mute,
            toggle_track_solo,
            toggle_track_lock,
            move_clip,
            updated_drag_state,
            context_menu_clip,
            trim_clip,
            updated_trim_state,
            // New editing tool fields
            razor_cut,
            slip_clip: None, // TODO: implement slip drag handling
            updated_slip_state: self.slip_state.clone(),
            slide_clip: None, // TODO: implement slide drag handling
            updated_slide_state: self.slide_state.clone(),
            ripple_delete,
            tool_changed,
            active_tool: self.active_tool,
            is_scrubbing,
        }
    }

    /// Hit test to find which clip (if any) is at the given position
    fn hit_test_clip(&self, pos: egui::Pos2, content_rect: Rect) -> Option<u64> {
        for clip in &self.clips {
            let clip_x = self.time_to_x(clip.timeline_start) + content_rect.left();
            let clip_width = clip.duration.as_secs_f32() * self.zoom;
            let track_y = self.track_y(clip.track_index) + content_rect.top();
            let track_height = self.track_height(clip.track_index);

            let clip_rect =
                Rect::from_min_size(pos2(clip_x, track_y), vec2(clip_width, track_height - 2.0));

            if clip_rect.contains(pos) {
                return Some(clip.entity_bits);
            }
        }
        None
    }

    /// Hit test to find if mouse is on a clip edge (for trim handles)
    /// Returns (clip_entity_bits, edge) if on an edge, None otherwise
    /// Based on Olive's IsCursorInTrimHandle() logic for precision
    fn hit_test_clip_edge(&self, pos: egui::Pos2, content_rect: Rect) -> Option<(u64, TrimEdge)> {
        for clip in &self.clips {
            let clip_x = self.time_to_x(clip.timeline_start) + content_rect.left();
            let clip_width = clip.duration.as_secs_f32() * self.zoom;
            let track_y = self.track_y(clip.track_index) + content_rect.top();
            let track_height = self.track_height(clip.track_index);

            // Check if in correct Y range first
            if pos.y < track_y || pos.y > track_y + track_height {
                continue;
            }

            // Olive's approach: Skip narrow clips where trim zones would overlap
            if clip_width <= TRIM_HANDLE_WIDTH * 2.0 {
                continue;
            }

            // Olive's approach: Edge zones are INSIDE the clip, not centered on boundary
            // Left edge: from clip start to clip start + handle width
            let left_edge_min = clip_x;
            let left_edge_max = clip_x + TRIM_HANDLE_WIDTH;

            // Right edge: from clip end - handle width to clip end
            let right_edge_min = clip_x + clip_width - TRIM_HANDLE_WIDTH;
            let right_edge_max = clip_x + clip_width;

            // Check edges (left takes priority if overlapping)
            if pos.x >= left_edge_min && pos.x <= left_edge_max {
                return Some((clip.entity_bits, TrimEdge::Left));
            }
            if pos.x >= right_edge_min && pos.x <= right_edge_max {
                return Some((clip.entity_bits, TrimEdge::Right));
            }
        }
        None
    }

    /// Hit test to find if mouse is over a gap between adjacent clips
    /// Returns (left_clip_entity, right_clip_entity) if in a gap zone
    fn hit_test_gap(&self, pos: egui::Pos2, content_rect: Rect) -> Option<(u64, u64)> {
        for track in &self.tracks {
            let track_y = self.track_y(track.index) + content_rect.top();
            let track_height = self.track_height(track.index);

            // Check if in correct Y range first
            if pos.y < track_y || pos.y > track_y + track_height {
                continue;
            }

            // Get clips on this track, sorted by start time
            let mut track_clips: Vec<_> = self
                .clips
                .iter()
                .filter(|c| c.track_index == track.index)
                .collect();
            track_clips.sort_by(|a, b| a.timeline_start.cmp(&b.timeline_start));

            // Check pairs of adjacent clips for gap zones
            for window in track_clips.windows(2) {
                let clip_a = window[0];
                let clip_b = window[1];

                let a_end_x = content_rect.left()
                    + self.time_to_x(clip_a.timeline_start)
                    + clip_a.duration.as_secs_f32() * self.zoom;
                let b_start_x = content_rect.left() + self.time_to_x(clip_b.timeline_start);

                // Check if position is in the gap zone (expanded area for easier targeting)
                let gap_left = a_end_x - 10.0; // Extend slightly into clips
                let gap_right = b_start_x + 10.0;

                if pos.x >= gap_left && pos.x <= gap_right {
                    return Some((clip_a.entity_bits, clip_b.entity_bits));
                }
            }
        }
        None
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
            .with_frame_rate(30.0)
            .with_zoom(100.0);

        assert_eq!(widget.timeline_name, "Test Timeline");
        assert_eq!(widget.frame_rate, 30.0);
        assert_eq!(widget.zoom, 100.0);
    }

    #[test]
    fn test_time_to_x_conversion() {
        let widget = TimelineWidget::new().with_zoom(100.0);

        // At 100 pixels per second, 1 second = 100 pixels
        let x = widget.time_to_x(Duration::from_secs(1));
        assert_eq!(x, 100.0);

        // With scroll offset
        let widget = widget.with_scroll(50.0, 0.0);
        let x = widget.time_to_x(Duration::from_secs(1));
        assert_eq!(x, 50.0); // 100 - 50 scroll
    }

    #[test]
    fn test_x_to_time_conversion() {
        let widget = TimelineWidget::new().with_zoom(100.0);

        let time = widget.x_to_time(100.0);
        assert_eq!(time, Duration::from_secs(1));
    }

    #[test]
    fn test_format_timecode() {
        assert_eq!(
            TimelineWidget::format_timecode(Duration::from_secs(0), 30.0),
            "00:00"
        );
        assert_eq!(
            TimelineWidget::format_timecode(Duration::from_secs(65), 30.0),
            "01:05"
        );
        assert_eq!(
            TimelineWidget::format_timecode(Duration::from_secs(3665), 30.0),
            "01:01:05"
        );
    }

    #[test]
    fn test_tick_interval_calculation() {
        assert_eq!(TimelineWidget::calculate_tick_interval(5.0), 1);
        assert_eq!(TimelineWidget::calculate_tick_interval(60.0), 5);
        assert_eq!(TimelineWidget::calculate_tick_interval(200.0), 10);
        assert_eq!(TimelineWidget::calculate_tick_interval(700.0), 60);
        assert_eq!(TimelineWidget::calculate_tick_interval(4000.0), 600);
    }
}
