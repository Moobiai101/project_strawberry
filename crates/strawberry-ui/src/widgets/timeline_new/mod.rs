//! # Timeline Widget Module
//!
//! The heart of the video editor - a multi-track timeline for editing clips.
//!
//! ## Architecture
//!
//! This module is split into sub-modules for maintainability:
//!
//! - **types/**: Data structures (TrackInfo, ClipInfo, DragStates)
//! - **rendering/**: Color palettes and standalone rendering functions
//! - **interaction/**: Input handling, hit testing, tools
//! - **response**: Widget response struct
//! - **widget**: Main TimelineWidget struct and builders
//! - **drawing**: Drawing method implementations (clips, ruler, playhead)
//! - **handler**: show() method and orchestration (~370 lines)
//! - **drag_handler**: Move/trim drag operations + ghost preview
//! - **input_handlers**: Scroll, keyboard shortcuts, cursor icons
//! - **drawing_pipeline**: Empty-state drawing, bottom ruler ticks
//!
//! ## Data Flow
//!
//! ```text
//! handler::show() ──→ drawing::draw_clips() ──→ clip_renderer (batched)
//!        ├──→ drag_handler::handle_drag()
//!        ├──→ input_handlers::handle_scroll/keyboard/cursor()
//!        └──→ drawing_pipeline::draw_capcut_empty_state()
//! ```
//!
//! ## Key Types
//!
//! - `TimelineWidget`: The main egui widget
//! - `TrackInfo`: Track lane configuration
//! - `ClipInfo`: Clip rendering data
//! - `TimelineResponse`: Interaction results
//!
//! ## Usage
//!
//! ```ignore
//! let response = TimelineWidget::new()
//!     .with_tracks(tracks)
//!     .with_clips(clips)
//!     .with_zoom(100.0)
//!     .show(ui);
//! ```

// Sub-modules
pub mod interaction;
pub mod rendering;
pub mod response;
pub mod types;
pub mod widget;

// Implementation modules (extend TimelineWidget via `impl` blocks)
mod drag_handler;
mod drawing;
mod drawing_pipeline;
mod handler;
mod input_handlers;
mod keyframe_drag_handler;
mod track_header;

// Re-export types for convenience
pub use interaction::{
    hit_test_clip, hit_test_clip_edge, hit_test_gap, ClipHit, GapHit, TimelineTool, TrimHit,
    TRIM_HANDLE_WIDTH,
};
pub use rendering::{
    calculate_tick_interval, draw_playhead, draw_ruler, draw_track_lanes, format_timecode,
    time_to_x, x_to_time, PLAYHEAD_GRAB_ZONE, PLAYHEAD_HEAD_SIZE, PLAYHEAD_WIDTH,
};
pub use response::TimelineResponse;
pub use types::{
    ClipDragState, ClipInfo, DragOrigin, FilmstripAssetTextures, KeyframeDragState,
    SlideDragState, SlipDragState, TrackInfo, TrimDragState, TrimEdge,
    DEFAULT_VIDEO_TRACK_HEIGHT,
};
pub use widget::{
    TimelineWidget, AUDIO_TRACK_HEIGHT, CLIP_GAP, MAX_ZOOM, MIN_ZOOM, PLAYHEAD_GRAB_WIDTH,
    RULER_HEIGHT, SCROLLBAR_HEIGHT, TRACK_HEADER_WIDTH, TRANSITION_BUTTON_SIZE, VIDEO_TRACK_HEIGHT,
};
