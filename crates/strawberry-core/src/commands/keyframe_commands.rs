//! # Keyframe Commands
//!
//! Command pattern implementations for keyframe automation: add, remove, and move.
//! All mutations go through commands for full undo/redo support.
//!
//! ## AI Schema
//! ```json
//! { "command": "add_keyframe", "clip_uuid": "...", "property_id": "volume", "time_secs": 1.5, "value": 0.8 }
//! { "command": "remove_keyframe", "clip_uuid": "...", "property_id": "volume", "keyframe_index": 2 }
//! { "command": "move_keyframe", "clip_uuid": "...", "property_id": "volume", "keyframe_index": 1, "new_time_secs": 2.0 }
//! ```

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::{Command, CommandError, UndoData};
use crate::components::keyframe::{Keyframe, KeyframeInterpolation};
use crate::components::keyframe_track::KeyframeTrack;
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// ADD KEYFRAME COMMAND
// ============================================================================

/// Add a keyframe to a clip's automation track.
///
/// Lazy-inserts the `KeyframeTrack` component if absent (zero overhead for
/// un-automated clips). If a keyframe already exists at the same time, it
/// is replaced (last-write-wins semantics, matching After Effects behavior).
///
/// Undo removes the keyframe. If we inserted the component, undo removes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddKeyframeCommand {
    pub clip_uuid: uuid::Uuid,
    pub property_id: String,
    pub time_secs: f64,
    pub value: f32,
    pub interpolation: KeyframeInterpolation,
    pub handle_right: [f32; 2],
    pub handle_left: [f32; 2],

    /// Index where the keyframe landed (populated on execute, used for undo)
    #[serde(skip)]
    inserted_index: Option<usize>,
    /// True if we created the KeyframeTrack component (remove on undo)
    #[serde(skip)]
    inserted_component: bool,
    /// If a keyframe was replaced (same time), store it for undo
    #[serde(skip)]
    replaced_keyframe: Option<Keyframe>,
}

impl AddKeyframeCommand {
    /// Create a new add keyframe command with default linear handles.
    pub fn new(
        clip_uuid: uuid::Uuid,
        property_id: String,
        time_secs: f64,
        value: f32,
    ) -> Self {
        Self {
            clip_uuid,
            property_id,
            time_secs,
            value,
            interpolation: KeyframeInterpolation::Linear,
            handle_right: [1.0 / 3.0, 1.0 / 3.0],
            handle_left: [2.0 / 3.0, 2.0 / 3.0],
            inserted_index: None,
            inserted_component: false,
            replaced_keyframe: None,
        }
    }

    /// Set the interpolation mode.
    pub fn with_interpolation(mut self, interpolation: KeyframeInterpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    fn resolve_entity(&self, world: &World) -> Result<Entity, CommandError> {
        world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!(
                    "Clip {} not found in registry",
                    &self.clip_uuid.to_string()[..8]
                ))
            })
    }
}

impl Command for AddKeyframeCommand {
    fn name(&self) -> &'static str {
        "Add Keyframe"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // Lazy insert: create KeyframeTrack if absent
        let had_track: bool = world.get::<KeyframeTrack>(entity).is_some();
        if !had_track {
            world.entity_mut(entity).insert(KeyframeTrack::new());
            self.inserted_component = true;
        }

        // Check if we're replacing an existing keyframe at this time
        let existing: Option<Keyframe> = world
            .get::<KeyframeTrack>(entity)
            .and_then(|track| {
                track.keyframes(&self.property_id).and_then(|kfs| {
                    const TOL: f64 = 1e-6;
                    kfs.iter()
                        .find(|kf| (kf.time_secs - self.time_secs).abs() < TOL)
                        .cloned()
                })
            });
        self.replaced_keyframe = existing;

        // Build and insert the keyframe
        let keyframe: Keyframe = Keyframe {
            time_secs: self.time_secs,
            value: self.value,
            interpolation: self.interpolation,
            handle_right: self.handle_right,
            handle_left: self.handle_left,
        };

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .expect("KeyframeTrack was just ensured")
            .into_inner();

        let idx: usize = track.add_keyframe(&self.property_id, keyframe);
        self.inserted_index = Some(idx);

        tracing::info!(
            "AddKeyframe: {}@{:.3}s = {:.3} on clip {}",
            self.property_id,
            self.time_secs,
            self.value,
            &self.clip_uuid.to_string()[..8],
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        if self.inserted_component {
            // We created the component — remove it entirely
            world.entity_mut(entity).remove::<KeyframeTrack>();
            self.inserted_component = false;
            return Ok(());
        }

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .ok_or_else(|| CommandError::InvalidOperation("KeyframeTrack missing on undo".into()))?
            .into_inner();

        if let Some(replaced) = self.replaced_keyframe.take() {
            // We replaced an existing keyframe — restore it
            track.remove_keyframe_by_time(&self.property_id, self.time_secs);
            track.add_keyframe(&self.property_id, replaced);
        } else {
            // We added a new keyframe — remove it
            track.remove_keyframe_by_time(&self.property_id, self.time_secs);
        }

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(AddKeyframeCommand, "add_keyframe");

// ============================================================================
// REMOVE KEYFRAME COMMAND
// ============================================================================

/// Remove a keyframe from a clip's automation track by index.
///
/// Undo re-inserts the exact same keyframe at the same position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveKeyframeCommand {
    pub clip_uuid: uuid::Uuid,
    pub property_id: String,
    pub keyframe_index: usize,

    /// The removed keyframe (populated on execute, used for undo)
    #[serde(skip)]
    removed_keyframe: Option<Keyframe>,
}

impl RemoveKeyframeCommand {
    pub fn new(clip_uuid: uuid::Uuid, property_id: String, keyframe_index: usize) -> Self {
        Self {
            clip_uuid,
            property_id,
            keyframe_index,
            removed_keyframe: None,
        }
    }

    fn resolve_entity(&self, world: &World) -> Result<Entity, CommandError> {
        world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!(
                    "Clip {} not found in registry",
                    &self.clip_uuid.to_string()[..8]
                ))
            })
    }
}

impl Command for RemoveKeyframeCommand {
    fn name(&self) -> &'static str {
        "Remove Keyframe"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .ok_or_else(|| {
                CommandError::InvalidOperation("No KeyframeTrack on clip".into())
            })?
            .into_inner();

        let removed: Keyframe = track
            .remove_keyframe_at(&self.property_id, self.keyframe_index)
            .ok_or_else(|| {
                CommandError::InvalidOperation(format!(
                    "Keyframe index {} out of bounds for property '{}'",
                    self.keyframe_index, self.property_id
                ))
            })?;

        tracing::info!(
            "RemoveKeyframe: {}@{:.3}s from clip {}",
            self.property_id,
            removed.time_secs,
            &self.clip_uuid.to_string()[..8],
        );

        self.removed_keyframe = Some(removed);
        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        let keyframe: Keyframe = self.removed_keyframe.take().ok_or_else(|| {
            CommandError::InvalidOperation("No keyframe data stored for undo".into())
        })?;

        // Ensure KeyframeTrack exists (may have been removed if empty)
        if world.get::<KeyframeTrack>(entity).is_none() {
            world.entity_mut(entity).insert(KeyframeTrack::new());
        }

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .expect("KeyframeTrack was just ensured")
            .into_inner();

        // Re-insert restores sort order automatically
        self.removed_keyframe = Some(keyframe.clone());
        track.add_keyframe(&self.property_id, keyframe);

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(RemoveKeyframeCommand, "remove_keyframe");

// ============================================================================
// MOVE KEYFRAME COMMAND
// ============================================================================

/// Move a keyframe to a new time position (drag-to-reposition).
///
/// Maintains sort order. Undo restores the original time.
/// Supports `can_merge` for smooth dragging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveKeyframeCommand {
    pub clip_uuid: uuid::Uuid,
    pub property_id: String,
    pub keyframe_index: usize,
    pub new_time_secs: f64,

    /// The original keyframe before the move (for undo)
    #[serde(skip)]
    original_keyframe: Option<Keyframe>,
}

impl MoveKeyframeCommand {
    pub fn new(
        clip_uuid: uuid::Uuid,
        property_id: String,
        keyframe_index: usize,
        new_time_secs: f64,
    ) -> Self {
        Self {
            clip_uuid,
            property_id,
            keyframe_index,
            new_time_secs,
            original_keyframe: None,
        }
    }

    fn resolve_entity(&self, world: &World) -> Result<Entity, CommandError> {
        world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!(
                    "Clip {} not found in registry",
                    &self.clip_uuid.to_string()[..8]
                ))
            })
    }
}

impl Command for MoveKeyframeCommand {
    fn name(&self) -> &'static str {
        "Move Keyframe"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .ok_or_else(|| {
                CommandError::InvalidOperation("No KeyframeTrack on clip".into())
            })?
            .into_inner();

        let old: Keyframe = track
            .move_keyframe(&self.property_id, self.keyframe_index, self.new_time_secs)
            .ok_or_else(|| {
                CommandError::InvalidOperation(format!(
                    "Keyframe index {} out of bounds for property '{}'",
                    self.keyframe_index, self.property_id
                ))
            })?;

        tracing::info!(
            "MoveKeyframe: {} {:.3}s → {:.3}s on clip {}",
            self.property_id,
            old.time_secs,
            self.new_time_secs,
            &self.clip_uuid.to_string()[..8],
        );

        self.original_keyframe = Some(old);
        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        let original: &Keyframe = self.original_keyframe.as_ref().ok_or_else(|| {
            CommandError::InvalidOperation("No original keyframe data for undo".into())
        })?;

        let track: &mut KeyframeTrack = world
            .get_mut::<KeyframeTrack>(entity)
            .ok_or_else(|| CommandError::InvalidOperation("KeyframeTrack missing on undo".into()))?
            .into_inner();

        // Find the moved keyframe by its new time and restore original time
        let original_time: f64 = original.time_secs;
        track.remove_keyframe_by_time(&self.property_id, self.new_time_secs);
        track.add_keyframe(&self.property_id, original.clone());

        // After undo, the keyframe is back at original_time.
        // Find its new index for potential re-redo.
        if let Some(kfs) = track.keyframes(&self.property_id) {
            const TOL: f64 = 1e-6;
            if let Some(new_idx) = kfs.iter().position(|kf| (kf.time_secs - original_time).abs() < TOL) {
                self.keyframe_index = new_idx;
            }
        }

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    /// Merge consecutive move commands on the same keyframe (smooth dragging).
    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<MoveKeyframeCommand>() {
            self.clip_uuid == other_cmd.clip_uuid
                && self.property_id == other_cmd.property_id
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<MoveKeyframeCommand>() {
            self.new_time_secs = other_cmd.new_time_secs;
            self.keyframe_index = other_cmd.keyframe_index;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(MoveKeyframeCommand, "move_keyframe");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Clip, StableId};
    use std::time::Duration;

    fn setup_test_world() -> (World, uuid::Uuid) {
        let mut world = World::new();
        let clip_uuid: uuid::Uuid = uuid::Uuid::new_v4();

        world.insert_resource(StableIdRegistry::default());

        let entity = world
            .spawn((
                Clip {
                    name: "Test Clip".into(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                StableId::from_uuid(clip_uuid),
            ))
            .id();

        world
            .resource_mut::<StableIdRegistry>()
            .register(clip_uuid, entity);

        (world, clip_uuid)
    }

    #[test]
    fn test_add_keyframe_creates_component() {
        let (mut world, uuid) = setup_test_world();
        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();

        // No KeyframeTrack initially
        assert!(world.get::<KeyframeTrack>(entity).is_none());

        let mut cmd = AddKeyframeCommand::new(uuid, "volume".into(), 1.0, 0.5);
        cmd.execute(&mut world).unwrap();

        // Should have created the component
        let track = world.get::<KeyframeTrack>(entity).unwrap();
        assert_eq!(track.keyframe_count("volume"), 1);
    }

    #[test]
    fn test_add_keyframe_undo_removes_component() {
        let (mut world, uuid) = setup_test_world();
        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();

        let mut cmd = AddKeyframeCommand::new(uuid, "volume".into(), 1.0, 0.5);
        cmd.execute(&mut world).unwrap();
        assert!(world.get::<KeyframeTrack>(entity).is_some());

        cmd.undo(&mut world, UndoData::None).unwrap();
        // Component should be removed since we inserted it
        assert!(world.get::<KeyframeTrack>(entity).is_none());
    }

    #[test]
    fn test_add_multiple_keyframes() {
        let (mut world, uuid) = setup_test_world();

        let mut cmd1 = AddKeyframeCommand::new(uuid, "volume".into(), 0.0, 1.0);
        let mut cmd2 = AddKeyframeCommand::new(uuid, "volume".into(), 2.0, 0.5);
        cmd1.execute(&mut world).unwrap();
        cmd2.execute(&mut world).unwrap();

        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
        let track = world.get::<KeyframeTrack>(entity).unwrap();
        assert_eq!(track.keyframe_count("volume"), 2);

        // Evaluate
        let v: f32 = track.evaluate("volume", 1.0, 1.0);
        assert!((v - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_remove_keyframe_and_undo() {
        let (mut world, uuid) = setup_test_world();
        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();

        // Setup: add 2 keyframes
        world.entity_mut(entity).insert(KeyframeTrack::new());
        let track = world.get_mut::<KeyframeTrack>(entity).unwrap().into_inner();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.0));

        // Remove index 1
        let mut cmd = RemoveKeyframeCommand::new(uuid, "volume".into(), 1);
        cmd.execute(&mut world).unwrap();

        let track = world.get::<KeyframeTrack>(entity).unwrap();
        assert_eq!(track.keyframe_count("volume"), 1);

        // Undo → should restore
        cmd.undo(&mut world, UndoData::None).unwrap();
        let track = world.get::<KeyframeTrack>(entity).unwrap();
        assert_eq!(track.keyframe_count("volume"), 2);
    }

    #[test]
    fn test_move_keyframe_and_undo() {
        let (mut world, uuid) = setup_test_world();
        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();

        // Setup
        world.entity_mut(entity).insert(KeyframeTrack::new());
        let track = world.get_mut::<KeyframeTrack>(entity).unwrap().into_inner();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(1.0, 0.5));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.0));

        // Move keyframe at index 1 (t=1.0) to t=1.5
        let mut cmd = MoveKeyframeCommand::new(uuid, "volume".into(), 1, 1.5);
        cmd.execute(&mut world).unwrap();

        let track = world.get::<KeyframeTrack>(entity).unwrap();
        let kfs = track.keyframes("volume").unwrap();
        assert!((kfs[1].time_secs - 1.5).abs() < 1e-6);

        // Undo → should restore to t=1.0
        cmd.undo(&mut world, UndoData::None).unwrap();
        let track = world.get::<KeyframeTrack>(entity).unwrap();
        let kfs = track.keyframes("volume").unwrap();
        assert!((kfs[1].time_secs - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_move_can_merge() {
        let uuid = uuid::Uuid::new_v4();
        let cmd1 = MoveKeyframeCommand::new(uuid, "volume".into(), 1, 1.5);
        let cmd2 = MoveKeyframeCommand::new(uuid, "volume".into(), 1, 2.0);
        let cmd_other = MoveKeyframeCommand::new(uuid::Uuid::new_v4(), "volume".into(), 1, 1.5);

        assert!(cmd1.can_merge(&cmd2));
        assert!(!cmd1.can_merge(&cmd_other));
    }
}
