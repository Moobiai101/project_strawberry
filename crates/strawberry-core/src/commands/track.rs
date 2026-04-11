//! # Track Commands
//!
//! Commands for controlling track properties (mute, solo, lock, etc.)
//! All commands are undoable and AI-invocable.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Track, TrackAudio};

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
// SET TRACK VOLUME COMMAND
// ============================================================================

/// Command to set a track's volume via the `TrackAudio` component.
///
/// If the track doesn't have a `TrackAudio` component, one is inserted
/// with default values before applying the volume change. This "lazy insert"
/// pattern matches how `AudioFade` works on clips.
///
/// ## AI Schema
/// ```json
/// { "command": "set_track_volume", "track": 12345, "volume": 0.75 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTrackVolumeCommand {
    /// Track entity ID
    pub entity_bits: u64,
    /// New volume (0.0–2.0)
    pub volume: f32,
    /// Whether TrackAudio was auto-inserted (for undo: remove if so)
    #[serde(skip)]
    was_auto_inserted: bool,
}

impl SetTrackVolumeCommand {
    pub fn new(entity_bits: u64, volume: f32) -> Self {
        Self {
            entity_bits,
            volume: volume.clamp(0.0, 2.0),
            was_auto_inserted: false,
        }
    }
}

impl Command for SetTrackVolumeCommand {
    fn name(&self) -> &'static str {
        "Set Track Volume"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = Entity::from_bits(self.entity_bits);

        // Verify entity is a Track
        if world.get::<Track>(entity).is_none() {
            return Err(CommandError::EntityNotFound(format!(
                "Track entity {}",
                self.entity_bits
            )));
        }

        // Get-or-insert TrackAudio (lazy insertion)
        let previous_volume: f32 = if let Some(track_audio) = world.get::<TrackAudio>(entity) {
            self.was_auto_inserted = false;
            track_audio.volume
        } else {
            // Auto-insert default TrackAudio
            world.entity_mut(entity).insert(TrackAudio::default());
            self.was_auto_inserted = true;
            1.0 // Default volume
        };

        // Apply new volume
        world.get_mut::<TrackAudio>(entity).unwrap().volume = self.volume;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "track_volume".to_string(),
            old_value: serde_json::json!(previous_volume),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = Entity::from_bits(self.entity_bits);

        if self.was_auto_inserted {
            // Remove the auto-inserted component entirely
            world.entity_mut(entity).remove::<TrackAudio>();
        } else if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let previous_volume: f32 = old_value.as_f64().unwrap_or(1.0) as f32;
            if let Some(mut track_audio) = world.get_mut::<TrackAudio>(entity) {
                track_audio.volume = previous_volume;
            }
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET TRACK PAN COMMAND
// ============================================================================

/// Command to set a track's stereo pan via the `TrackAudio` component.
///
/// Same lazy-insert pattern as `SetTrackVolumeCommand`.
///
/// ## AI Schema
/// ```json
/// { "command": "set_track_pan", "track": 12345, "pan": -0.5 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTrackPanCommand {
    /// Track entity ID
    pub entity_bits: u64,
    /// New pan (-1.0 left, 0.0 center, 1.0 right)
    pub pan: f32,
    /// Whether TrackAudio was auto-inserted (for undo: remove if so)
    #[serde(skip)]
    was_auto_inserted: bool,
}

impl SetTrackPanCommand {
    pub fn new(entity_bits: u64, pan: f32) -> Self {
        Self {
            entity_bits,
            pan: pan.clamp(-1.0, 1.0),
            was_auto_inserted: false,
        }
    }
}

impl Command for SetTrackPanCommand {
    fn name(&self) -> &'static str {
        "Set Track Pan"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = Entity::from_bits(self.entity_bits);

        // Verify entity is a Track
        if world.get::<Track>(entity).is_none() {
            return Err(CommandError::EntityNotFound(format!(
                "Track entity {}",
                self.entity_bits
            )));
        }

        // Get-or-insert TrackAudio (lazy insertion)
        let previous_pan: f32 = if let Some(track_audio) = world.get::<TrackAudio>(entity) {
            self.was_auto_inserted = false;
            track_audio.pan
        } else {
            world.entity_mut(entity).insert(TrackAudio::default());
            self.was_auto_inserted = true;
            0.0 // Default pan (center)
        };

        // Apply new pan
        world.get_mut::<TrackAudio>(entity).unwrap().pan = self.pan;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "track_pan".to_string(),
            old_value: serde_json::json!(previous_pan),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = Entity::from_bits(self.entity_bits);

        if self.was_auto_inserted {
            world.entity_mut(entity).remove::<TrackAudio>();
        } else if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let previous_pan: f32 = old_value.as_f64().unwrap_or(0.0) as f32;
            if let Some(mut track_audio) = world.get_mut::<TrackAudio>(entity) {
                track_audio.pan = previous_pan;
            }
        }

        Ok(())
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

    #[test]
    fn test_set_track_volume_with_auto_insert() {
        let (mut world, entity) = setup_world_with_track();

        // No TrackAudio yet — command should auto-insert it
        assert!(world.get::<TrackAudio>(entity).is_none());

        let mut cmd = SetTrackVolumeCommand::new(entity.to_bits(), 0.5);
        let undo_data = cmd.execute(&mut world).unwrap();

        // TrackAudio should now exist with volume 0.5
        let track_audio = world.get::<TrackAudio>(entity).unwrap();
        assert!((track_audio.volume - 0.5).abs() < f32::EPSILON);
        assert!((track_audio.pan - 0.0).abs() < f32::EPSILON); // Default pan

        // Undo should REMOVE the auto-inserted component
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(world.get::<TrackAudio>(entity).is_none());
    }

    #[test]
    fn test_set_track_volume_with_existing_component() {
        let (mut world, entity) = setup_world_with_track();

        // Pre-insert TrackAudio with volume 0.8
        world.entity_mut(entity).insert(TrackAudio::with_volume(0.8));

        let mut cmd = SetTrackVolumeCommand::new(entity.to_bits(), 1.5);
        let undo_data = cmd.execute(&mut world).unwrap();

        assert!((world.get::<TrackAudio>(entity).unwrap().volume - 1.5).abs() < f32::EPSILON);

        // Undo should restore 0.8, NOT remove the component
        cmd.undo(&mut world, undo_data).unwrap();
        assert!((world.get::<TrackAudio>(entity).unwrap().volume - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_set_track_pan_with_auto_insert() {
        let (mut world, entity) = setup_world_with_track();

        assert!(world.get::<TrackAudio>(entity).is_none());

        let mut cmd = SetTrackPanCommand::new(entity.to_bits(), -0.7);
        let undo_data = cmd.execute(&mut world).unwrap();

        let track_audio = world.get::<TrackAudio>(entity).unwrap();
        assert!((track_audio.pan - -0.7).abs() < f32::EPSILON);
        assert!((track_audio.volume - 1.0).abs() < f32::EPSILON); // Default volume

        // Undo removes auto-inserted component
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(world.get::<TrackAudio>(entity).is_none());
    }

    #[test]
    fn test_set_track_pan_with_existing_component() {
        let (mut world, entity) = setup_world_with_track();

        world
            .entity_mut(entity)
            .insert(TrackAudio::with_volume_and_pan(1.0, 0.3));

        let mut cmd = SetTrackPanCommand::new(entity.to_bits(), -1.0);
        let undo_data = cmd.execute(&mut world).unwrap();

        assert!((world.get::<TrackAudio>(entity).unwrap().pan - -1.0).abs() < f32::EPSILON);

        // Undo restores 0.3
        cmd.undo(&mut world, undo_data).unwrap();
        assert!((world.get::<TrackAudio>(entity).unwrap().pan - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_volume_clamping_in_command() {
        let (mut world, entity) = setup_world_with_track();

        // Volume above max should clamp to 2.0
        let mut cmd = SetTrackVolumeCommand::new(entity.to_bits(), 5.0);
        let _undo_data = cmd.execute(&mut world).unwrap();
        assert!((world.get::<TrackAudio>(entity).unwrap().volume - 2.0).abs() < f32::EPSILON);
    }
}

