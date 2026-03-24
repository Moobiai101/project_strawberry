//! # Audio Normalize Command
//!
//! Command to normalize a clip's volume to a target LUFS level.
//!
//! ## Design
//!
//! The UI layer measures LUFS (via `AudioMixerResource`) and computes
//! the normalization gain BEFORE submitting this command. The command
//! itself only applies the pre-computed gain to `AudioClip.volume`.
//! This keeps `strawberry-core` free of `strawberry-render` dependencies.
//!
//! ## Undo/Redo
//!
//! Stores the previous `AudioClip.volume` in undo data for perfect reversal.

use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid::Uuid;

use super::{Command, CommandError, UndoData};
use crate::components::AudioClip;
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// NORMALIZE AUDIO COMMAND
// ============================================================================

/// Normalize a clip's audio volume by applying a pre-computed gain multiplier.
///
/// ## Workflow
/// 1. UI measures clip's LUFS via `AudioMixerResource.measure_clip_loudness()`
/// 2. UI computes gain via `compute_normalization_gain(current_lufs, target_lufs)`
/// 3. UI submits this command with the computed `gain_multiplier`
/// 4. Command applies: `AudioClip.volume = current_volume * gain_multiplier`
///
/// ## AI Schema
/// ```json
/// { "command": "normalize_audio", "clip_uuid": "...", "gain_multiplier": 1.5, "target_lufs": -14.0 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeAudioCommand {
    /// Target clip UUID
    pub clip_uuid: Uuid,
    /// Pre-computed linear gain multiplier (from `compute_normalization_gain`)
    pub gain_multiplier: f32,
    /// Target LUFS (for display/logging only, not used in computation)
    pub target_lufs: f64,
}

impl NormalizeAudioCommand {
    /// Create a new normalize command with pre-computed gain.
    ///
    /// ## Arguments
    /// - `clip_uuid`: Target clip's stable UUID
    /// - `gain_multiplier`: Linear gain (from `compute_normalization_gain()`)
    /// - `target_lufs`: The target LUFS level (for logging)
    pub fn new(clip_uuid: Uuid, gain_multiplier: f32, target_lufs: f64) -> Self {
        Self {
            clip_uuid,
            gain_multiplier,
            target_lufs,
        }
    }
}

impl Command for NormalizeAudioCommand {
    fn name(&self) -> &'static str {
        "Normalize Audio"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Resolve UUID to Entity
        let entity = world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or(CommandError::EntityNotFound(self.clip_uuid.to_string()))?;

        // Get current volume (or default 1.0 if no AudioClip yet)
        let current_volume: f32 = world
            .get::<AudioClip>(entity)
            .map(|ac| ac.volume)
            .unwrap_or(1.0);

        // Compute new volume: current × gain, clamped to [0.0, 4.0]
        // 4.0 = +12dB max, beyond this is almost certainly wrong
        let new_volume: f32 = (current_volume * self.gain_multiplier).clamp(0.0, 4.0);

        // Apply to AudioClip (auto-insert if not present)
        if let Some(mut audio_clip) = world.get_mut::<AudioClip>(entity) {
            audio_clip.volume = new_volume;
        } else {
            // Auto-insert AudioClip with normalized volume
            world.entity_mut(entity).insert(AudioClip {
                volume: new_volume,
                ..Default::default()
            });
        }

        tracing::info!(
            "Normalized clip {} → {:.1} LUFS (gain: {:.3}×, volume: {:.3} → {:.3})",
            self.clip_uuid,
            self.target_lufs,
            self.gain_multiplier,
            current_volume,
            new_volume,
        );

        // Store previous volume for undo
        Ok(UndoData::PropertyChanged {
            entity_bits: entity.to_bits(),
            property: "volume".to_string(),
            old_value: serde_json::json!(current_volume),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let previous_volume: f32 = match undo_data {
            UndoData::PropertyChanged { old_value, .. } => {
                old_value.as_f64().unwrap_or(1.0) as f32
            }
            _ => return Err(CommandError::InvalidOperation(
                "Invalid undo data for NormalizeAudioCommand".to_string(),
            )),
        };

        let entity = world
            .resource::<StableIdRegistry>()
            .get(&self.clip_uuid)
            .ok_or(CommandError::EntityNotFound(self.clip_uuid.to_string()))?;

        if let Some(mut audio_clip) = world.get_mut::<AudioClip>(entity) {
            audio_clip.volume = previous_volume;
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

impl_serializable_command!(NormalizeAudioCommand, "normalize_audio");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::StableId;

    fn setup_world_with_clip(uuid: Uuid) -> (World, bevy_ecs::entity::Entity) {
        let mut world = World::new();
        world.insert_resource(StableIdRegistry::default());

        let entity = world
            .spawn((
                StableId(uuid),
                AudioClip {
                    volume: 0.5,
                    pan: 0.0,
                    muted: false,
                    ..Default::default()
                },
            ))
            .id();

        world.resource_mut::<StableIdRegistry>().register(uuid, entity);
        (world, entity)
    }

    #[test]
    fn test_normalize_applies_gain() {
        let uuid = Uuid::new_v4();
        let (mut world, entity) = setup_world_with_clip(uuid);

        let mut cmd = NormalizeAudioCommand::new(uuid, 2.0, -14.0);
        let undo = cmd.execute(&mut world).unwrap();

        let volume: f32 = world.get::<AudioClip>(entity).unwrap().volume;
        assert!((volume - 1.0).abs() < 0.001, "0.5 × 2.0 = 1.0, got {}", volume);

        // Verify undo data
        match undo {
            UndoData::PropertyChanged { old_value, .. } => {
                let v: f64 = old_value.as_f64().unwrap();
                assert!((v - 0.5).abs() < 0.001);
            }
            _ => panic!("Expected PropertyChanged undo data"),
        }
    }

    #[test]
    fn test_normalize_undo_restores_volume() {
        let uuid = Uuid::new_v4();
        let (mut world, entity) = setup_world_with_clip(uuid);

        let mut cmd = NormalizeAudioCommand::new(uuid, 2.0, -14.0);
        let undo = cmd.execute(&mut world).unwrap();

        // Undo
        cmd.undo(&mut world, undo).unwrap();
        let volume: f32 = world.get::<AudioClip>(entity).unwrap().volume;
        assert!((volume - 0.5).abs() < 0.001, "Should restore to 0.5, got {}", volume);
    }

    #[test]
    fn test_normalize_clamps_volume() {
        let uuid = Uuid::new_v4();
        let (mut world, entity) = setup_world_with_clip(uuid);

        // Extreme gain that would push volume above 4.0
        let mut cmd = NormalizeAudioCommand::new(uuid, 100.0, -14.0);
        cmd.execute(&mut world).unwrap();

        let volume: f32 = world.get::<AudioClip>(entity).unwrap().volume;
        assert!((volume - 4.0).abs() < 0.001, "Should clamp to 4.0, got {}", volume);
    }

    #[test]
    fn test_normalize_missing_entity() {
        let mut world = World::new();
        world.insert_resource(StableIdRegistry::default());

        let mut cmd = NormalizeAudioCommand::new(Uuid::new_v4(), 2.0, -14.0);
        assert!(cmd.execute(&mut world).is_err());
    }
}
