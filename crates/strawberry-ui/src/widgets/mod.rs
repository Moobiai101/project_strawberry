//! Custom widgets

pub mod aspect_ratio_dropdown;
pub mod clip_renderer;
pub mod filmstrip;
pub mod status_bar;
pub mod timeline_new;
pub mod transport;
pub mod viewport;

// Keep legacy timeline for reference during transition
#[allow(dead_code)]
mod timeline_legacy;

pub use clip_renderer::{BatchedClipRenderer, VisibleClipRect};

pub use filmstrip::{
    calculate_thumbnail_count, get_lod_for_pixels_per_second, FilmstripConfig, FilmstripRenderer,
    FilmstripThumbnails,
};
pub use status_bar::{StatusBar, StatusBarConfig, StatusBarResponse};

// Export from timeline_new (the refactored module)
pub use timeline_new::{
    ClipDragState, ClipInfo, FilmstripAssetTextures, SlideDragState, SlipDragState,
    TimelineResponse, TimelineTool, TimelineWidget, TrackInfo, TrimDragState, TrimEdge,
};
pub use transport::{TransportResponse, TransportWidget};
pub use viewport::{
    render_clip_quad, render_clip_quad_identity, GizmoInputHandler, GizmoInputResult, GizmoOverlay,
    SelectedElementData, ViewportWidget, VisibleClip,
};
