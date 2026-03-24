//! # Audio Commands
//!
//! Commands for modifying per-clip audio properties (volume, pan, mute) and
//! master audio state with full undo/redo support.
//!
//! ## Design (1000+ clip scalability)
//!
//! - `SetClipAudioCommand` uses UUID resolution via `StableIdRegistry` (O(1) lookup).
//! - `can_merge()` ensures slider-dragging produces ONE undo entry, not 60/sec.
//! - Auto-inserts `AudioClip` component only on first mutation (lazy insertion).
//! - `SetMasterAudioCommand` mutates a single ECS resource (zero iteration).
//!
//! ## AI Schema
//! ```json
//! { "command": "set_clip_audio", "clip_uuid": "...", "volume_percent": 75, "pan_percent": -30 }
//! { "command": "set_master_audio", "volume_percent": 80, "muted": true }
//! ```

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::{Command, CommandError, UndoData};
use crate::components::AudioClip;
use crate::impl_serializable_command;
use crate::resources::{MasterAudioState, StableIdRegistry};

// ============================================================================
// CLIP AUDIO PROPERTY ENUM
// ============================================================================

/// A per-clip audio property that can be changed via command.
///
/// AI agents use percentage-based values when constructing commands.
/// Internally stored as linear floats:
/// - Volume: 0.0–2.0 (0%–200%)
/// - Pan: -1.0–1.0 (-100%–100%, left–right)
/// - Muted: bool
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipAudioProperty {
    /// Volume as linear float (0.0 = silence, 1.0 = unity, 2.0 = +6dB boost)
    Volume(f32),
    /// Stereo pan (-1.0 = full left, 0.0 = center, 1.0 = full right)
    Pan(f32),
    /// Mute toggle
    Muted(bool),
}

impl ClipAudioProperty {
    /// Human-readable name for undo stack display
    pub fn display_name(&self) -> &'static str {
        match self {
            ClipAudioProperty::Volume(_) => "Set Clip Volume",
            ClipAudioProperty::Pan(_) => "Set Clip Pan",
            ClipAudioProperty::Muted(_) => "Set Clip Mute",
        }
    }
}

// ============================================================================
// SET CLIP AUDIO COMMAND
// ============================================================================

/// Set an audio property on a clip (volume, pan, or mute).
///
/// Resolves the clip entity via UUID (StableIdRegistry) for robust undo/redo.
/// If the clip has no `AudioClip` component, one is inserted with defaults
/// before applying the property change (lazy insertion = zero overhead for
/// clips that never have audio tweaks).
///
/// ## AI Schema
/// ```json
/// {
///   "command": "set_clip_audio",
///   "clip_uuid": "550e8400-e29b-41d4-a716-446655440000",
///   "property": { "Volume": 0.75 }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetClipAudioCommand {
    /// UUID of the target clip
    pub clip_uuid: uuid::Uuid,
    /// The audio property + new value to set
    pub property: ClipAudioProperty,

    /// Previous property value for undo (populated on execute)
    #[serde(skip)]
    previous_value: Option<ClipAudioProperty>,
    /// Whether we inserted a brand-new AudioClip component (remove on undo)
    #[serde(skip)]
    inserted_audio_clip: bool,
}

impl SetClipAudioCommand {
    /// Create a new clip audio command.
    pub fn new(clip_uuid: uuid::Uuid, property: ClipAudioProperty) -> Self {
        Self {
            clip_uuid,
            property,
            previous_value: None,
            inserted_audio_clip: false,
        }
    }

    /// Convenience: set volume (clamped 0.0–2.0, i.e. 0%–200%)
    pub fn set_volume(clip_uuid: uuid::Uuid, volume: f32) -> Self {
        Self::new(clip_uuid, ClipAudioProperty::Volume(volume.clamp(0.0, 2.0)))
    }

    /// Convenience: set pan (clamped -1.0–1.0)
    pub fn set_pan(clip_uuid: uuid::Uuid, pan: f32) -> Self {
        Self::new(clip_uuid, ClipAudioProperty::Pan(pan.clamp(-1.0, 1.0)))
    }

    /// Convenience: set muted
    pub fn set_muted(clip_uuid: uuid::Uuid, muted: bool) -> Self {
        Self::new(clip_uuid, ClipAudioProperty::Muted(muted))
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

impl Command for SetClipAudioCommand {
    fn name(&self) -> &'static str {
        self.property.display_name()
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // Lazy insertion: ensure AudioClip exists on the entity
        let has_audio_clip: bool = world.get::<AudioClip>(entity).is_some();
        if !has_audio_clip {
            world.entity_mut(entity).insert(AudioClip::default());
            self.inserted_audio_clip = true;
        }

        // Read current value for undo
        let audio_clip: &AudioClip = world
            .get::<AudioClip>(entity)
            .expect("AudioClip was just ensured");

        self.previous_value = Some(match &self.property {
            ClipAudioProperty::Volume(_) => ClipAudioProperty::Volume(audio_clip.volume),
            ClipAudioProperty::Pan(_) => ClipAudioProperty::Pan(audio_clip.pan),
            ClipAudioProperty::Muted(_) => ClipAudioProperty::Muted(audio_clip.muted),
        });

        // Apply new value with clamping
        let audio_clip_mut: &mut AudioClip = world
            .get_mut::<AudioClip>(entity)
            .expect("AudioClip was just ensured")
            .into_inner();

        match &self.property {
            ClipAudioProperty::Volume(volume) => {
                audio_clip_mut.volume = volume.clamp(0.0, 2.0);
            }
            ClipAudioProperty::Pan(pan) => {
                audio_clip_mut.pan = pan.clamp(-1.0, 1.0);
            }
            ClipAudioProperty::Muted(muted) => {
                audio_clip_mut.muted = *muted;
            }
        }

        tracing::info!(
            "SetClipAudio: applied {:?} to clip {}",
            self.property,
            &self.clip_uuid.to_string()[..8],
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // If we inserted the AudioClip component, just remove it entirely
        if self.inserted_audio_clip {
            world.entity_mut(entity).remove::<AudioClip>();
            self.inserted_audio_clip = false;
            return Ok(());
        }

        // Restore previous value
        let previous: &ClipAudioProperty = self.previous_value.as_ref().ok_or_else(|| {
            CommandError::InvalidOperation("No previous audio value stored for undo".into())
        })?;

        let audio_clip_mut: &mut AudioClip = world
            .get_mut::<AudioClip>(entity)
            .ok_or_else(|| CommandError::EntityNotFound("AudioClip missing during undo".into()))?
            .into_inner();

        match previous {
            ClipAudioProperty::Volume(volume) => audio_clip_mut.volume = *volume,
            ClipAudioProperty::Pan(pan) => audio_clip_mut.pan = *pan,
            ClipAudioProperty::Muted(muted) => audio_clip_mut.muted = *muted,
        }

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    /// Merge consecutive audio property changes on the same clip (slider dragging).
    ///
    /// Without merging, dragging a volume slider for 1 second at 60fps would create
    /// 60 undo entries. With merging, it creates exactly 1.
    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipAudioCommand>() {
            self.clip_uuid == other_cmd.clip_uuid
                && std::mem::discriminant(&self.property)
                    == std::mem::discriminant(&other_cmd.property)
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipAudioCommand>() {
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

impl_serializable_command!(SetClipAudioCommand, "set_clip_audio");

// ============================================================================
// MASTER AUDIO PROPERTY ENUM
// ============================================================================

/// A master audio property that can be changed via command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MasterAudioProperty {
    /// Master volume as linear float (0.0–2.0)
    Volume(f32),
    /// Master mute toggle
    Muted(bool),
}

impl MasterAudioProperty {
    /// Human-readable name for display
    pub fn display_name(&self) -> &'static str {
        match self {
            MasterAudioProperty::Volume(_) => "Set Master Volume",
            MasterAudioProperty::Muted(_) => "Set Master Mute",
        }
    }
}

// ============================================================================
// SET MASTER AUDIO COMMAND
// ============================================================================

/// Set master audio volume or mute.
///
/// Mutates the `MasterAudioState` resource (single ECS resource, zero iteration).
/// Not undoable — master volume doesn't pollute the undo stack
/// (same pattern as playback speed commands).
///
/// ## AI Schema
/// ```json
/// { "command": "set_master_audio", "property": { "Volume": 0.8 } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetMasterAudioCommand {
    /// The master audio property + new value to set
    pub property: MasterAudioProperty,

    /// Previous value for undo (populated on execute, unused since not undoable)
    #[serde(skip)]
    previous_value: Option<MasterAudioProperty>,
}

impl SetMasterAudioCommand {
    /// Create a new master audio command.
    pub fn new(property: MasterAudioProperty) -> Self {
        Self {
            property,
            previous_value: None,
        }
    }

    /// Convenience: set master volume (clamped 0.0–2.0)
    pub fn set_volume(volume: f32) -> Self {
        Self::new(MasterAudioProperty::Volume(volume.clamp(0.0, 2.0)))
    }

    /// Convenience: set master mute
    pub fn set_muted(muted: bool) -> Self {
        Self::new(MasterAudioProperty::Muted(muted))
    }
}

impl Command for SetMasterAudioCommand {
    fn name(&self) -> &'static str {
        self.property.display_name()
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let master_audio: &MasterAudioState = world.resource::<MasterAudioState>();

        // Store previous value (even though not undoable, good for debugging)
        self.previous_value = Some(match &self.property {
            MasterAudioProperty::Volume(_) => {
                MasterAudioProperty::Volume(master_audio.master_volume)
            }
            MasterAudioProperty::Muted(_) => {
                MasterAudioProperty::Muted(master_audio.master_muted)
            }
        });

        // Apply new value
        let master_audio_mut: &mut MasterAudioState =
            &mut world.resource_mut::<MasterAudioState>();

        match &self.property {
            MasterAudioProperty::Volume(volume) => {
                master_audio_mut.master_volume = volume.clamp(0.0, 2.0);
            }
            MasterAudioProperty::Muted(muted) => {
                master_audio_mut.master_muted = *muted;
            }
        }

        tracing::info!("SetMasterAudio: applied {:?}", self.property);

        Ok(UndoData::None)
    }

    fn undo(&mut self, _world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Not undoable — this method is never called
        Ok(())
    }

    /// Master volume changes don't go on the undo stack.
    /// Same pattern as playback commands (Play, Pause, SeekCommand).
    fn is_undoable(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(SetMasterAudioCommand, "set_master_audio");

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

    /// Helper: create a test world with a clip entity registered via StableIdRegistry
    fn setup_test_world() -> (World, uuid::Uuid) {
        let mut world = World::new();
        let clip_uuid: uuid::Uuid = uuid::Uuid::new_v4();

        // Insert StableIdRegistry
        world.insert_resource(StableIdRegistry::default());

        // Insert MasterAudioState
        world.insert_resource(MasterAudioState::default());

        // Create clip entity with StableId
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

        // Register in StableIdRegistry
        world.resource_mut::<StableIdRegistry>().register(clip_uuid, entity);

        (world, clip_uuid)
    }

    #[test]
    fn test_set_volume_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        // Insert AudioClip with known volume
        let entity = world.resource::<StableIdRegistry>().get(&clip_uuid).unwrap();
        world.entity_mut(entity).insert(AudioClip {
            volume: 1.0,
            ..Default::default()
        });

        let mut cmd = SetClipAudioCommand::set_volume(clip_uuid, 0.5);
        cmd.execute(&mut world).unwrap();

        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.volume - 0.5).abs() < f32::EPSILON);

        cmd.undo(&mut world, UndoData::None).unwrap();
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_set_pan_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world.resource::<StableIdRegistry>().get(&clip_uuid).unwrap();
        world.entity_mut(entity).insert(AudioClip::default());

        let mut cmd = SetClipAudioCommand::set_pan(clip_uuid, -0.7);
        cmd.execute(&mut world).unwrap();

        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.pan - (-0.7)).abs() < f32::EPSILON);

        cmd.undo(&mut world, UndoData::None).unwrap();
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.pan - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_set_muted_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world.resource::<StableIdRegistry>().get(&clip_uuid).unwrap();
        world.entity_mut(entity).insert(AudioClip::default());

        let mut cmd = SetClipAudioCommand::set_muted(clip_uuid, true);
        cmd.execute(&mut world).unwrap();

        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!(audio.muted);

        cmd.undo(&mut world, UndoData::None).unwrap();
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!(!audio.muted);
    }

    #[test]
    fn test_inserts_audio_clip_if_missing() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world.resource::<StableIdRegistry>().get(&clip_uuid).unwrap();
        assert!(world.get::<AudioClip>(entity).is_none());

        let mut cmd = SetClipAudioCommand::set_volume(clip_uuid, 0.8);
        cmd.execute(&mut world).unwrap();

        // AudioClip should now exist
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.volume - 0.8).abs() < f32::EPSILON);

        // Undo should remove it entirely
        cmd.undo(&mut world, UndoData::None).unwrap();
        assert!(world.get::<AudioClip>(entity).is_none());
    }

    #[test]
    fn test_volume_clamped_to_valid_range() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world.resource::<StableIdRegistry>().get(&clip_uuid).unwrap();
        world.entity_mut(entity).insert(AudioClip::default());

        // Clamp high
        let mut cmd = SetClipAudioCommand::set_volume(clip_uuid, 5.0);
        cmd.execute(&mut world).unwrap();
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.volume - 2.0).abs() < f32::EPSILON);

        // Clamp low
        let mut cmd2 = SetClipAudioCommand::set_volume(clip_uuid, -1.0);
        cmd2.execute(&mut world).unwrap();
        let audio: &AudioClip = world.get::<AudioClip>(entity).unwrap();
        assert!((audio.volume - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_can_merge_consecutive_volume_changes() {
        let clip_uuid: uuid::Uuid = uuid::Uuid::new_v4();

        let cmd1 = SetClipAudioCommand::set_volume(clip_uuid, 0.5);
        let cmd2 = SetClipAudioCommand::set_volume(clip_uuid, 0.6);
        let cmd_pan = SetClipAudioCommand::set_pan(clip_uuid, 0.3);
        let cmd_other_clip = SetClipAudioCommand::set_volume(uuid::Uuid::new_v4(), 0.5);

        // Same clip + same property type → merge
        assert!(cmd1.can_merge(&cmd2));

        // Same clip + different property type → no merge
        assert!(!cmd1.can_merge(&cmd_pan));

        // Different clip → no merge
        assert!(!cmd1.can_merge(&cmd_other_clip));
    }

    #[test]
    fn test_set_master_volume() {
        let (mut world, _) = setup_test_world();

        let mut cmd = SetMasterAudioCommand::set_volume(0.6);
        cmd.execute(&mut world).unwrap();

        let master: &MasterAudioState = world.resource::<MasterAudioState>();
        assert!((master.master_volume - 0.6).abs() < f32::EPSILON);
        assert!(!cmd.is_undoable());
    }

    #[test]
    fn test_set_master_muted() {
        let (mut world, _) = setup_test_world();

        let mut cmd = SetMasterAudioCommand::set_muted(true);
        cmd.execute(&mut world).unwrap();

        let master: &MasterAudioState = world.resource::<MasterAudioState>();
        assert!(master.master_muted);
    }
}
