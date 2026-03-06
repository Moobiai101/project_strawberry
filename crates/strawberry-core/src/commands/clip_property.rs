//! # Clip Property Commands
//!
//! Commands for modifying clip metadata properties (speed, reverse, loop mode)
//! with full undo/redo support.
//!
//! Uses a unified `ClipProperty` enum so all property changes share one command type,
//! resulting in clean undo stack entries like "Set Clip Speed" / "Set Clip Reverse".

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::{Command, CommandError, UndoData};
use crate::components::timeline::{ClipLoopMode, ClipMetadata};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// CLIP PROPERTY ENUM
// ============================================================================

/// A clip property that can be changed via command.
///
/// Each variant holds the new value to apply. The command stores
/// the old value internally for undo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipProperty {
    /// Playback speed multiplier (0.01 to 100.0, default 1.0)
    Speed(f32),
    /// Reverse playback toggle
    Reverse(bool),
    /// Per-clip source media loop mode
    LoopMode(ClipLoopMode),
}

impl ClipProperty {
    /// Human-readable name for undo stack display
    pub fn display_name(&self) -> &'static str {
        match self {
            ClipProperty::Speed(_) => "Set Clip Speed",
            ClipProperty::Reverse(_) => "Set Clip Reverse",
            ClipProperty::LoopMode(_) => "Set Clip Loop Mode",
        }
    }
}

// ============================================================================
// SET CLIP PROPERTY COMMAND
// ============================================================================

/// Set a property on a clip's metadata.
///
/// Resolves the clip entity via UUID (StableIdRegistry) for robust undo/redo.
/// If the clip has no `ClipMetadata` component, one is inserted with defaults
/// before applying the property change.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "set_clip_property",
///   "clip_uuid": "550e8400-e29b-41d4-a716-446655440000",
///   "property": { "Speed": 2.0 }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetClipPropertyCommand {
    /// UUID of the target clip
    pub clip_uuid: uuid::Uuid,
    /// The property + new value to set
    pub property: ClipProperty,

    /// Previous property value for undo (populated on execute)
    #[serde(skip)]
    previous_value: Option<ClipProperty>,
    /// Whether we inserted a brand-new ClipMetadata (remove on undo)
    #[serde(skip)]
    inserted_metadata: bool,
}

impl SetClipPropertyCommand {
    /// Create a new clip property command.
    ///
    /// # Arguments
    /// * `clip_uuid` - UUID of the target clip entity
    /// * `property` - The property variant with the new value
    pub fn new(clip_uuid: uuid::Uuid, property: ClipProperty) -> Self {
        Self {
            clip_uuid,
            property,
            previous_value: None,
            inserted_metadata: false,
        }
    }

    /// Convenience: set speed on a clip
    pub fn set_speed(clip_uuid: uuid::Uuid, speed: f32) -> Self {
        Self::new(clip_uuid, ClipProperty::Speed(speed.clamp(0.01, 100.0)))
    }

    /// Convenience: set reverse on a clip
    pub fn set_reverse(clip_uuid: uuid::Uuid, reverse: bool) -> Self {
        Self::new(clip_uuid, ClipProperty::Reverse(reverse))
    }

    /// Convenience: set loop mode on a clip
    pub fn set_loop_mode(clip_uuid: uuid::Uuid, loop_mode: ClipLoopMode) -> Self {
        Self::new(clip_uuid, ClipProperty::LoopMode(loop_mode))
    }

    /// Resolve the clip entity from UUID via StableIdRegistry
    fn resolve_entity(&self, world: &World) -> Result<Entity, CommandError> {
        world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!(
                    "Clip with UUID {} not found in registry",
                    self.clip_uuid
                ))
            })
    }
}

impl Command for SetClipPropertyCommand {
    fn name(&self) -> &'static str {
        self.property.display_name()
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // Ensure ClipMetadata exists on the entity
        let has_metadata: bool = world.get::<ClipMetadata>(entity).is_some();
        if !has_metadata {
            world.entity_mut(entity).insert(ClipMetadata::default());
            self.inserted_metadata = true;
        }

        // Read current value for undo, then apply new value
        let metadata: &ClipMetadata = world
            .get::<ClipMetadata>(entity)
            .expect("ClipMetadata was just ensured");

        // Store previous value
        self.previous_value = Some(match &self.property {
            ClipProperty::Speed(_) => ClipProperty::Speed(metadata.speed),
            ClipProperty::Reverse(_) => ClipProperty::Reverse(metadata.reverse),
            ClipProperty::LoopMode(_) => ClipProperty::LoopMode(metadata.loop_mode),
        });

        // Apply new value
        let metadata_mut: &mut ClipMetadata = world
            .get_mut::<ClipMetadata>(entity)
            .expect("ClipMetadata was just ensured")
            .into_inner();

        match &self.property {
            ClipProperty::Speed(speed) => {
                metadata_mut.speed = speed.clamp(0.01, 100.0);
            }
            ClipProperty::Reverse(reverse) => {
                metadata_mut.reverse = *reverse;
            }
            ClipProperty::LoopMode(loop_mode) => {
                metadata_mut.loop_mode = *loop_mode;
            }
        }

        tracing::info!(
            "SetClipProperty: applied {:?} to clip {}",
            self.property,
            &self.clip_uuid.to_string()[..8],
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // If we inserted the metadata, just remove it entirely
        if self.inserted_metadata {
            world.entity_mut(entity).remove::<ClipMetadata>();
            self.inserted_metadata = false;
            return Ok(());
        }

        // Restore previous value
        let previous: &ClipProperty = self.previous_value.as_ref().ok_or_else(|| {
            CommandError::InvalidOperation("No previous value stored for undo".into())
        })?;

        let metadata_mut: &mut ClipMetadata = world
            .get_mut::<ClipMetadata>(entity)
            .ok_or_else(|| CommandError::EntityNotFound("ClipMetadata missing during undo".into()))?
            .into_inner();

        match previous {
            ClipProperty::Speed(speed) => metadata_mut.speed = *speed,
            ClipProperty::Reverse(reverse) => metadata_mut.reverse = *reverse,
            ClipProperty::LoopMode(loop_mode) => metadata_mut.loop_mode = *loop_mode,
        }

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    /// Merge consecutive speed changes on the same clip (slider dragging)
    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipPropertyCommand>() {
            // Only merge same-property changes on the same clip
            self.clip_uuid == other_cmd.clip_uuid
                && std::mem::discriminant(&self.property)
                    == std::mem::discriminant(&other_cmd.property)
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipPropertyCommand>() {
            // Take the newer value, keep our original previous_value for undo
            self.property = other_cmd.property.clone();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(SetClipPropertyCommand, "set_clip_property");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Clip, StableId};
    use crate::resources::StableIdRegistry;
    use bevy_ecs::world::World;
    use std::time::Duration;

    /// Set up a minimal world with StableIdRegistry, a clip entity with ClipMetadata
    fn setup_world() -> (World, uuid::Uuid) {
        let mut world = World::new();
        world.insert_resource(StableIdRegistry::default());

        let stable_id: StableId = StableId::new();
        let uuid: uuid::Uuid = stable_id.uuid();

        let clip = Clip {
            name: "Test Clip".into(),
            source_in: Duration::from_secs(0),
            source_out: Duration::from_secs(10),
            timeline_start: Duration::from_secs(5),
        };

        let entity = world.spawn((stable_id, clip, ClipMetadata::default())).id();

        // Register in StableIdRegistry
        world
            .resource_mut::<StableIdRegistry>()
            .register(uuid, entity);

        (world, uuid)
    }

    #[test]
    fn test_set_speed_execute_and_undo() {
        let (mut world, uuid) = setup_world();

        let mut cmd = SetClipPropertyCommand::set_speed(uuid, 2.5);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify speed changed
        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!((meta.speed - 2.5).abs() < f32::EPSILON);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!(
            (meta.speed - 1.0).abs() < f32::EPSILON,
            "Speed should be restored to 1.0"
        );
    }

    #[test]
    fn test_set_reverse_execute_and_undo() {
        let (mut world, uuid) = setup_world();

        let mut cmd = SetClipPropertyCommand::set_reverse(uuid, true);
        let undo_data = cmd.execute(&mut world).unwrap();

        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!(meta.reverse);

        cmd.undo(&mut world, undo_data).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!(!meta.reverse, "Reverse should be restored to false");
    }

    #[test]
    fn test_set_loop_mode_execute_and_undo() {
        let (mut world, uuid) = setup_world();

        let mut cmd = SetClipPropertyCommand::set_loop_mode(uuid, ClipLoopMode::PingPong);
        let undo_data = cmd.execute(&mut world).unwrap();

        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert_eq!(meta.loop_mode, ClipLoopMode::PingPong);

        cmd.undo(&mut world, undo_data).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert_eq!(
            meta.loop_mode,
            ClipLoopMode::None,
            "Loop mode should be restored"
        );
    }

    #[test]
    fn test_speed_clamped_to_valid_range() {
        let (mut world, uuid) = setup_world();

        // Over max
        let mut cmd = SetClipPropertyCommand::set_speed(uuid, 999.0);
        cmd.execute(&mut world).unwrap();

        let entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!(
            (meta.speed - 100.0).abs() < f32::EPSILON,
            "Speed should be clamped to 100.0"
        );
    }

    #[test]
    fn test_can_merge_same_property_same_clip() {
        let uuid = uuid::Uuid::new_v4();
        let cmd1 = SetClipPropertyCommand::set_speed(uuid, 1.5);
        let cmd2 = SetClipPropertyCommand::set_speed(uuid, 2.0);

        assert!(cmd1.can_merge(&cmd2));
    }

    #[test]
    fn test_cannot_merge_different_properties() {
        let uuid = uuid::Uuid::new_v4();
        let cmd1 = SetClipPropertyCommand::set_speed(uuid, 1.5);
        let cmd2 = SetClipPropertyCommand::set_reverse(uuid, true);

        assert!(!cmd1.can_merge(&cmd2));
    }

    #[test]
    fn test_inserts_metadata_if_missing() {
        let mut world = World::new();
        world.insert_resource(StableIdRegistry::default());

        let stable_id: StableId = StableId::new();
        let uuid: uuid::Uuid = stable_id.uuid();

        let clip = Clip {
            name: "No Metadata".into(),
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(5),
            timeline_start: Duration::ZERO,
        };

        // Spawn WITHOUT ClipMetadata
        let entity = world.spawn((stable_id, clip)).id();
        world
            .resource_mut::<StableIdRegistry>()
            .register(uuid, entity);

        let mut cmd = SetClipPropertyCommand::set_speed(uuid, 3.0);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Should have inserted metadata and set speed
        let meta = world.get::<ClipMetadata>(entity).unwrap();
        assert!((meta.speed - 3.0).abs() < f32::EPSILON);

        // Undo should REMOVE the metadata entirely
        cmd.undo(&mut world, undo_data).unwrap();
        assert!(
            world.get::<ClipMetadata>(entity).is_none(),
            "Undo should remove ClipMetadata that was auto-inserted"
        );
    }
}
