//! # Events
//!
//! Inter-system communication events.

use bevy_ecs::event::Event;
use std::time::Duration;

use crate::commands::CommandId;

// ============================================================================
// PLAYHEAD EVENTS
// ============================================================================

/// Playhead position changed
#[derive(Event, Debug, Clone)]
pub struct PlayheadChangedEvent {
    pub old_time: Duration,
    pub new_time: Duration,
    pub old_frame: u64,
    pub new_frame: u64,
}

/// Playback started
#[derive(Event, Debug, Clone, Copy, Default)]
pub struct PlaybackStartedEvent;

/// Playback stopped
#[derive(Event, Debug, Clone, Copy, Default)]
pub struct PlaybackStoppedEvent;

// ============================================================================
// SELECTION EVENTS
// ============================================================================

/// Clip was selected
#[derive(Event, Debug, Clone)]
pub struct ClipSelectedEvent {
    pub clip: u64, // Entity bits
    pub added_to_selection: bool,
}

/// Clip was deselected
#[derive(Event, Debug, Clone)]
pub struct ClipDeselectedEvent {
    pub clip: u64,
}

/// Selection was cleared
#[derive(Event, Debug, Clone, Copy, Default)]
pub struct SelectionClearedEvent;

// ============================================================================
// COMMAND EVENTS
// ============================================================================

/// Command was executed
#[derive(Event, Debug, Clone)]
pub struct CommandExecutedEvent {
    pub command_id: CommandId,
    pub command_name: &'static str,
    pub success: bool,
}

/// Command was undone
#[derive(Event, Debug, Clone)]
pub struct CommandUndoneEvent {
    pub command_id: CommandId,
    pub command_name: &'static str,
}

/// Command was redone
#[derive(Event, Debug, Clone)]
pub struct CommandRedoneEvent {
    pub command_id: CommandId,
    pub command_name: &'static str,
}

// ============================================================================
// TIMELINE EVENTS
// ============================================================================

/// Track was added
#[derive(Event, Debug, Clone)]
pub struct TrackAddedEvent {
    pub track: u64,
    pub index: u32,
}

/// Track was removed
#[derive(Event, Debug, Clone)]
pub struct TrackRemovedEvent {
    pub track: u64,
}

/// Clip was added to timeline
#[derive(Event, Debug, Clone)]
pub struct ClipAddedEvent {
    pub clip: u64,
    pub track: u64,
}

/// Clip was removed from timeline
#[derive(Event, Debug, Clone)]
pub struct ClipRemovedEvent {
    pub clip: u64,
}

// ============================================================================
// EFFECT EVENTS
// ============================================================================

/// Effect was added to clip
#[derive(Event, Debug, Clone)]
pub struct EffectAddedEvent {
    pub clip: u64,
    pub effect: u64,
}

/// Effect was removed from clip
#[derive(Event, Debug, Clone)]
pub struct EffectRemovedEvent {
    pub clip: u64,
    pub effect: u64,
}

// ============================================================================
// PROJECT EVENTS
// ============================================================================

/// Project was created
#[derive(Event, Debug, Clone)]
pub struct ProjectCreatedEvent {
    pub project: u64,
    pub name: String,
}

/// Project was saved
#[derive(Event, Debug, Clone)]
pub struct ProjectSavedEvent {
    pub path: std::path::PathBuf,
}

/// Project was loaded
#[derive(Event, Debug, Clone)]
pub struct ProjectLoadedEvent {
    pub path: std::path::PathBuf,
}
