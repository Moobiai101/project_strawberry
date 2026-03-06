//! # Strawberry Core
//!
//! Core foundation for Project Strawberry - ECS, Commands, Events, and Resources.
//!
//! ## Architecture
//!
//! This crate provides the foundational patterns that ALL other crates depend on:
//!
//! - **Command Pattern**: All mutations go through commands for undo/redo and AI parity
//! - **ECS Components**: Core data structures as ECS components
//! - **Resources**: Global state (Playhead, Project, Selection)
//! - **Events**: Inter-system communication
//! - **Node System**: Graph-based processing for timeline rendering
//!
//! ## Usage
//!
//! ```rust,ignore
//! use strawberry_core::prelude::*;
//!
//! // Create a command
//! let cmd = CreateProjectCommand::new("My Project")
//!     .with_resolution(1920, 1080)
//!     .with_frame_rate(FrameRate::Fps30);
//!
//! // Submit to queue
//! command_queue.submit(Box::new(cmd));
//! ```

pub mod commands;
pub mod components;
pub mod events;
pub mod gizmo;
pub mod node;
pub mod resources;
pub mod systems;

/// Prelude for convenient imports
pub mod prelude {
    // Commands - re-export all command types and traits
    pub use crate::commands::{
        import::*, playback::*, project::*, registry::*, serializable::*,
        timeline as timeline_cmds, transform::*, Command, CommandError, CommandId, UndoData,
        UndoEntry, UndoStack,
    };

    // Components - re-export all component types
    pub use crate::components::{
        timeline as timeline_components, AudioAsset, AudioClip, BlendMode, Clip, ClipTransform,
        ColorSpace, FrameRate, PrimarySelection, Project, ProjectSettings, RenderLayer, Selected,
        StableId, Track, TrackType, Transform2D, VideoAsset, VideoFrame,
    };

    // Timeline-specific explicit imports to avoid ambiguity
    pub use crate::commands::timeline::{
        AddClipCommand, CreateTimelineCommand, CreateTrackCommand, DeleteClipCommand,
        MoveClipCommand, SplitClipCommand, TrimClipCommand, TrimSide,
    };
    pub use crate::components::timeline::{
        ActiveTimeline, ClipLoopMode, ClipMetadata, Timeline, TimelineChild, TimelineState,
        TrackClip, TrackDisplay,
    };

    // Events
    pub use crate::events::*;

    // Resources
    pub use crate::resources::*;

    // Systems
    pub use crate::systems::*;
}
