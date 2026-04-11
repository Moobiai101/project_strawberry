//! # Command System
//!
//! All mutations to the ECS World go through Commands.
//! This enables:
//! - Undo/Redo
//! - AI uses same path as UI
//! - Full audit log
//! - Serializable operations

pub mod asset;
pub mod audio;
pub mod audio_fade;
pub mod audio_normalize;
pub mod clip_property;
pub mod gizmo_commands;
pub mod import;
pub mod keyframe_commands;
pub mod playback;
pub mod project;
pub mod project_resolution;
pub mod registry;
pub mod selection;
pub mod serializable;
pub mod timeline;
pub mod track;
pub mod transform;

pub use asset::*;
pub use audio::*;
pub use audio_fade::*;
pub use audio_normalize::*;
pub use clip_property::*;
pub use import::*;
pub use keyframe_commands::*;
pub use playback::*;
pub use project::*;
pub use project_resolution::*;
pub use registry::*;
pub use selection::*;
pub use serializable::*;
pub use timeline::*;
pub use track::*;
pub use transform::*;

use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;
use thiserror::Error;

/// Error types for command execution
#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Data stored for undoing a command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UndoData {
    /// An entity was created - store its ID to despawn on undo
    EntityCreated(u64),

    /// An entity was deleted - store serialized data to respawn on undo
    EntityDeleted {
        entity_bits: u64,
        serialized: Vec<u8>,
    },

    /// A property was changed - store old value
    PropertyChanged {
        entity_bits: u64,
        property: String,
        old_value: serde_json::Value,
    },

    /// Selection was changed - store previous selection state
    Selection {
        previous_selected: Vec<u64>,
        previous_primary: Option<u64>,
    },

    /// Multiple undo operations
    Batch(Vec<UndoData>),

    /// No undo data needed
    None,
}

/// Unique identifier for a command execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandId(pub u64);

impl CommandId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for CommandId {
    fn default() -> Self {
        Self::new()
    }
}

/// The core Command trait that all commands must implement
///
/// Commands are the ONLY way to mutate the ECS World.
/// Both UI and AI submit commands to the same queue.
pub trait Command: Send + Sync + Debug {
    /// Human-readable name for the command
    fn name(&self) -> &'static str;

    /// Execute the command on the world
    ///
    /// Returns undo data that can be used to reverse the operation
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError>;

    /// Undo the command using the provided undo data
    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError>;

    /// Whether this command should be added to the undo stack
    ///
    /// Override to return `false` for navigation/playback commands
    /// that shouldn't be undoable (Seek, Play, Pause, Stop, SetSpeed).
    /// Editing commands (Delete, Split, Move, Trim) return `true` (default).
    fn is_undoable(&self) -> bool {
        true
    }

    /// Whether this command can be merged with a previous command of the same type
    ///
    /// Used for continuous operations like dragging
    fn can_merge(&self, _other: &dyn Command) -> bool {
        false
    }

    /// Merge with another command (for continuous operations)
    fn merge(&mut self, _other: Box<dyn Command>) {
        // Default: no merging
    }

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Convert to mutable Any for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // NOTE: The following methods have been removed as part of the UUID-based undo/redo refactor:
    // - remap_entity() - no longer needed, commands lookup entities by UUID via StableIdRegistry
    // - created_entity_id() - no longer needed, UUIDs persist across undo/redo
    // - new_created_entity_id() - no longer needed, UUIDs are stable identifiers
}

/// Entry in the undo stack
#[derive(Debug)]
pub struct UndoEntry {
    pub id: CommandId,
    pub command: Box<dyn Command>,
    pub undo_data: UndoData,
}

/// The undo/redo stack
#[derive(Debug, Default)]
pub struct UndoStack {
    /// Executed commands that can be undone
    undo_stack: Vec<UndoEntry>,

    /// Undone commands that can be redone
    redo_stack: Vec<UndoEntry>,

    /// Maximum number of undo steps to keep
    max_size: usize,
}

impl UndoStack {
    pub fn new(max_size: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_size,
        }
    }

    /// Push an executed command onto the undo stack
    pub fn push(&mut self, entry: UndoEntry) {
        // Clear redo stack when new command is executed
        self.redo_stack.clear();

        // Add to undo stack
        self.undo_stack.push(entry);

        // Trim if too large
        while self.undo_stack.len() > self.max_size {
            self.undo_stack.remove(0);
        }
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Pop the most recent command for undoing
    pub fn pop_for_undo(&mut self) -> Option<UndoEntry> {
        self.undo_stack.pop()
    }

    /// Push an undone entry to the redo stack
    pub fn push_to_redo(&mut self, entry: UndoEntry) {
        self.redo_stack.push(entry);
    }

    /// Pop from redo stack for redoing
    pub fn pop_for_redo(&mut self) -> Option<UndoEntry> {
        self.redo_stack.pop()
    }

    /// Get the name of the command that would be undone
    pub fn undo_name(&self) -> Option<&'static str> {
        self.undo_stack.last().map(|e| e.command.name())
    }

    /// Get the name of the command that would be redone
    pub fn redo_name(&self) -> Option<&'static str> {
        self.redo_stack.last().map(|e| e.command.name())
    }

    /// Push a redone entry back to the undo stack
    ///
    /// Unlike `push()`, this does NOT clear the redo stack.
    /// Used when a redo operation completes successfully.
    pub fn push_from_redo(&mut self, entry: UndoEntry) {
        // Do NOT clear redo stack - this is a redo operation
        self.undo_stack.push(entry);

        // Trim if too large
        while self.undo_stack.len() > self.max_size {
            self.undo_stack.remove(0);
        }
    }

    // NOTE: remap_entity_in_redo() has been removed as part of the UUID-based undo/redo refactor.
    // Commands now lookup entities by UUID via StableIdRegistry, making entity ID remapping unnecessary.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_id_uniqueness() {
        let id1 = CommandId::new();
        let id2 = CommandId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_undo_stack_push_clears_redo() {
        let stack = UndoStack::new(100);

        // Simulate: execute, undo, then execute new command
        // The redo stack should be cleared

        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
    }
}
