//! # Strawberry UI
//!
//! Custom egui widgets for the creative engine.
//!
//! ## Widgets
//!
//! - Viewport
//! - Timeline
//! - Track Headers
//! - Effect Stack
//! - Color Wheels
//! - Graph Editor
//! - AI Chat Panel

pub mod panels;
pub mod theme;
pub mod widgets;

pub use widgets::aspect_ratio_dropdown::{AspectRatioDropdown, AspectRatioDropdownResponse};

pub use theme::StrawberryTheme;
pub use widgets::status_bar::{StatusBar, StatusBarConfig, StatusBarResponse};
pub use widgets::timeline_new::{
    ClipDragState, ClipInfo, DragOrigin, FilmstripAssetTextures, SlideDragState, SlipDragState,
    TimelineResponse, TimelineTool, TimelineWidget, TrackInfo, TrimDragState, TrimEdge,
};
pub use widgets::transport::{TransportResponse, TransportWidget};
pub use widgets::viewport::{
    render_clip_quad, render_clip_quad_identity, GizmoInputHandler, GizmoInputResult, GizmoOverlay,
    SelectedElementData, ViewportWidget, VisibleClip,
};
