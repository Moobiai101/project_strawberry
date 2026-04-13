//! Timeline type definitions.
//!
//! Contains all data structures used by the timeline widget.
//!
//! ## Modules
//!
//! - `track`: Track information (TrackInfo)
//! - `clip`: Clip information (ClipInfo, FilmstripAssetTextures)
//! - `drag_state`: Drag operation state structs

pub mod clip;
pub mod drag_state;
pub mod track;

// Re-exports for convenience
pub use clip::{ClipInfo, FilmstripAssetTextures};
pub use drag_state::{
    ClipDragState, DragOrigin, KeyframeDragState, SlideDragState, SlipDragState, TrimDragState,
    TrimEdge,
};
pub use track::{TrackInfo, DEFAULT_VIDEO_TRACK_HEIGHT};
