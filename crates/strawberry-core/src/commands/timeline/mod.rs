//! # Timeline Commands
//!
//! AI-invocable commands for timeline manipulation.
//!
//! All commands follow the Command Pattern and are:
//! - Undoable/Redoable
//! - Serializable (for project files and AI)
//! - AI-invocable via JSON schema
//!
//! ## Command Categories
//!
//! - **Timeline lifecycle** (`create.rs`): Create/delete timelines & tracks
//! - **Clip addition** (`add_clip.rs`): Add clips with collision detection
//! - **Clip movement** (`move_clip.rs`): Move clips between positions/tracks
//! - **Clip deletion** (`delete_clip.rs`): Delete individual clips
//! - **Clip splitting** (`split_clip.rs`): Split a single clip at a position
//! - **Clip trimming** (`trim_clip.rs`): Trim in/out edges
//! - **Ripple delete** (`ripple_delete.rs`): Delete + close gap
//! - **Slip** (`slip_clip.rs`): Shift source content without moving clip
//! - **Slide** (`slide_clip.rs`): Slide clip while adjusting neighbors
//! - **Razor tool** (`razor_tool.rs`): Multi-track split at a position
//!
//! ## Entity Relationships
//!
//! Commands use `bevy_hierarchy` for parent-child relationships:
//! - Tracks are children of Timeline
//! - Clips are children of Track
//!
//! This means despawning a Timeline will cascade-despawn all tracks and clips.

mod create;
mod add_clip;
mod move_clip;
mod delete_clip;
mod split_clip;
mod trim_clip;
mod ripple_delete;
mod slip_clip;
mod slide_clip;
mod razor_tool;

#[cfg(test)]
mod tests;

// Re-export all public types to preserve existing API surface.
// Any file that did `use crate::commands::timeline::*` or
// `use crate::commands::*` will continue to work unchanged.
pub use create::{CreateTimelineCommand, CreateTrackCommand};
pub use add_clip::AddClipCommand;
pub use move_clip::MoveClipCommand;
pub use delete_clip::DeleteClipCommand;
pub use split_clip::SplitClipCommand;
pub use trim_clip::{TrimSide, TrimClipCommand};
pub use ripple_delete::RippleDeleteCommand;
pub use slip_clip::SlipClipCommand;
pub use slide_clip::SlideClipCommand;
pub use razor_tool::RazorToolCommand;
