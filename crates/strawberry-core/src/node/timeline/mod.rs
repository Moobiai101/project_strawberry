//! # Timeline Nodes
//!
//! Node wrappers for timeline ECS components.
//!
//! See `README.md` in this directory for architecture details.

pub mod clip;
pub mod track;

// Re-export main types
pub use clip::{ClipAssetRef, ClipNode};
pub use track::TrackNode;
