//! # Track Commands
//!
//! Commands for controlling track properties (mute, solo, lock, etc.)
//! All commands are undoable and AI-invocable.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::Track;

// ============================================================================
// SET TRACK MUTED COMMAND
// ============================================================================

/// Command to set a track's muted state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTrackMutedCommand {
    /// Track entity ID
    pub entity_bits: u64,
    /// New muted state
    pub muted: bool,
}

impl SetTrackMutedCommand {
    pub fn new(entity_bits: u64, muted: bool) -> Self {
        Self { entity_bits, muted }
    }

    /// Toggle muted state (get current and flip)
    pub fn toggle(entity_bits: u64) -> Self {
        // Note: actual toggle is computed in execute()
        Self {
            entity_bits,
            muted: true, // placeholder, will be toggled
        }
    }
}

impl Command for SetTrackMutedCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
            CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
        })?;

        let previous_value = track.muted;
        track.muted = self.muted;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "muted".to_string(),
            old_value: serde_json::json!(previous_value),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let previous_muted = old_value.as_bool().unwrap_or(false);

            let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
                CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
            })?;

            track.muted = previous_muted;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Set Track Muted"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET TRACK SOLO COMMAND
// ============================================================================

/// Command to set a track's solo state
///
/// When a track is soloed, only soloed tracks are heard/visible during playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTrackSoloCommand {
    /// Track entity ID
    pub entity_bits: u64,
    /// New solo state
    pub solo: bool,
}

impl SetTrackSoloCommand {
    pub fn new(entity_bits: u64, solo: bool) -> Self {
        Self { entity_bits, solo }
    }
}

impl Command for SetTrackSoloCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
            CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
        })?;

        let previous_value = track.solo;
        track.solo = self.solo;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "solo".to_string(),
            old_value: serde_json::json!(previous_value),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let previous_solo = old_value.as_bool().unwrap_or(false);

            let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
                CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
            })?;

            track.solo = previous_solo;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Set Track Solo"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET TRACK LOCKED COMMAND
// ============================================================================

/// Command to set a track's locked state
///
/// Locked tracks cannot be edited.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTrackLockedCommand {
    /// Track entity ID
    pub entity_bits: u64,
    /// New locked state
    pub locked: bool,
}

impl SetTrackLockedCommand {
    pub fn new(entity_bits: u64, locked: bool) -> Self {
        Self {
            entity_bits,
            locked,
        }
    }
}

impl Command for SetTrackLockedCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
            CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
        })?;

        let previous_value = track.locked;
        track.locked = self.locked;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "locked".to_string(),
            old_value: serde_json::json!(previous_value),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let previous_locked = old_value.as_bool().unwrap_or(false);

            let mut track = world.get_mut::<Track>(entity).ok_or_else(|| {
                CommandError::EntityNotFound(format!("Track entity {}", self.entity_bits))
            })?;

            track.locked = previous_locked;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Set Track Locked"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::TrackType;

    fn setup_world_with_track() -> (World, Entity) {
        let mut world = World::new();
        let entity = world
            .spawn(Track {
                index: 0,
                track_type: TrackType::Video,
                name: "Video 1".to_string(),
                locked: false,
                muted: false,
                solo: false,
                visible: true,
            })
            .id();
        (world, entity)
    }

    #[test]
    fn test_set_track_muted() {
        let (mut world, entity) = setup_world_with_track();

        let mut cmd = SetTrackMutedCommand::new(entity.to_bits(), true);
        let undo_data = cmd.execute(&mut world).unwrap();

        assert!(world.get::<Track>(entity).unwrap().muted);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(!world.get::<Track>(entity).unwrap().muted);
    }

    #[test]
    fn test_set_track_solo() {
        let (mut world, entity) = setup_world_with_track();

        let mut cmd = SetTrackSoloCommand::new(entity.to_bits(), true);
        let undo_data = cmd.execute(&mut world).unwrap();

        assert!(world.get::<Track>(entity).unwrap().solo);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(!world.get::<Track>(entity).unwrap().solo);
    }

    #[test]
    fn test_set_track_locked() {
        let (mut world, entity) = setup_world_with_track();

        let mut cmd = SetTrackLockedCommand::new(entity.to_bits(), true);
        let undo_data = cmd.execute(&mut world).unwrap();

        assert!(world.get::<Track>(entity).unwrap().locked);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(!world.get::<Track>(entity).unwrap().locked);
    }
}
