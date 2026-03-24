//! # Audio Fade Commands
//!
//! Commands for setting per-clip fade-in/fade-out durations and curve shapes.
//! Separated from `audio.rs` to keep files under 400 lines.
//!
//! ## AI Schema
//! ```json
//! { "command": "set_clip_fade", "clip_uuid": "...", "fade_in_ms": 500, "fade_out_ms": 300, "curve": "ease_in_out" }
//! ```

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use super::{Command, CommandError, UndoData};
use crate::components::audio::{AudioFade, FadeCurve};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// CLIP FADE PROPERTY ENUM
// ============================================================================

/// A per-clip fade property that can be changed via command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipFadeProperty {
    /// Fade-in duration in milliseconds
    FadeIn(u32),
    /// Fade-out duration in milliseconds
    FadeOut(u32),
    /// Fade curve shape
    Curve(FadeCurve),
}

impl ClipFadeProperty {
    /// Human-readable name for undo stack display.
    pub fn display_name(&self) -> &'static str {
        match self {
            ClipFadeProperty::FadeIn(_) => "Set Fade In",
            ClipFadeProperty::FadeOut(_) => "Set Fade Out",
            ClipFadeProperty::Curve(_) => "Set Fade Curve",
        }
    }
}

// ============================================================================
// SET CLIP FADE COMMAND
// ============================================================================

/// Set a fade property on a clip (fade-in, fade-out, or curve).
///
/// Resolves the clip entity via UUID. If no `AudioFade` component exists,
/// inserts one with defaults before applying (lazy insertion — zero overhead
/// for clips that never have fades configured).
///
/// `can_merge()` enables smooth slider dragging with a single undo entry.
///
/// ## AI Schema
/// ```json
/// { "command": "set_clip_fade", "clip_uuid": "...", "property": { "FadeIn": 500 } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetClipFadeCommand {
    /// UUID of the target clip
    pub clip_uuid: uuid::Uuid,
    /// The fade property + new value to set
    pub property: ClipFadeProperty,

    /// Previous property value (populated on execute, used for undo)
    #[serde(skip)]
    previous_value: Option<ClipFadeProperty>,
    /// Whether we inserted a brand-new AudioFade component (remove on undo)
    #[serde(skip)]
    inserted_audio_fade: bool,
}

impl SetClipFadeCommand {
    /// Create a new fade command.
    pub fn new(clip_uuid: uuid::Uuid, property: ClipFadeProperty) -> Self {
        Self {
            clip_uuid,
            property,
            previous_value: None,
            inserted_audio_fade: false,
        }
    }

    /// Convenience: set fade-in duration in milliseconds.
    pub fn set_fade_in(clip_uuid: uuid::Uuid, fade_in_ms: u32) -> Self {
        Self::new(clip_uuid, ClipFadeProperty::FadeIn(fade_in_ms))
    }

    /// Convenience: set fade-out duration in milliseconds.
    pub fn set_fade_out(clip_uuid: uuid::Uuid, fade_out_ms: u32) -> Self {
        Self::new(clip_uuid, ClipFadeProperty::FadeOut(fade_out_ms))
    }

    /// Convenience: set fade curve.
    pub fn set_curve(clip_uuid: uuid::Uuid, curve: FadeCurve) -> Self {
        Self::new(clip_uuid, ClipFadeProperty::Curve(curve))
    }

    /// Resolve the clip entity from UUID.
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

impl Command for SetClipFadeCommand {
    fn name(&self) -> &'static str {
        self.property.display_name()
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // Lazy insertion: ensure AudioFade exists
        let has_fade: bool = world.get::<AudioFade>(entity).is_some();
        if !has_fade {
            world.entity_mut(entity).insert(AudioFade::default());
            self.inserted_audio_fade = true;
        }

        // Read current value for undo
        let fade: &AudioFade = world
            .get::<AudioFade>(entity)
            .expect("AudioFade was just ensured");

        self.previous_value = Some(match &self.property {
            ClipFadeProperty::FadeIn(_) => {
                ClipFadeProperty::FadeIn(fade.fade_in.as_millis() as u32)
            }
            ClipFadeProperty::FadeOut(_) => {
                ClipFadeProperty::FadeOut(fade.fade_out.as_millis() as u32)
            }
            ClipFadeProperty::Curve(_) => ClipFadeProperty::Curve(fade.curve),
        });

        // Apply new value
        let fade_mut: &mut AudioFade = world
            .get_mut::<AudioFade>(entity)
            .expect("AudioFade was just ensured")
            .into_inner();

        match &self.property {
            ClipFadeProperty::FadeIn(ms) => {
                fade_mut.fade_in = Duration::from_millis(*ms as u64);
            }
            ClipFadeProperty::FadeOut(ms) => {
                fade_mut.fade_out = Duration::from_millis(*ms as u64);
            }
            ClipFadeProperty::Curve(curve) => {
                fade_mut.curve = *curve;
            }
        }

        tracing::info!(
            "SetClipFade: applied {:?} to clip {}",
            self.property,
            &self.clip_uuid.to_string()[..8],
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let entity: Entity = self.resolve_entity(world)?;

        // If we inserted the AudioFade, remove it entirely
        if self.inserted_audio_fade {
            world.entity_mut(entity).remove::<AudioFade>();
            self.inserted_audio_fade = false;
            return Ok(());
        }

        // Restore previous value
        let previous: &ClipFadeProperty = self.previous_value.as_ref().ok_or_else(|| {
            CommandError::InvalidOperation("No previous fade value stored for undo".into())
        })?;

        let fade_mut: &mut AudioFade = world
            .get_mut::<AudioFade>(entity)
            .ok_or_else(|| CommandError::EntityNotFound("AudioFade missing during undo".into()))?
            .into_inner();

        match previous {
            ClipFadeProperty::FadeIn(ms) => {
                fade_mut.fade_in = Duration::from_millis(*ms as u64);
            }
            ClipFadeProperty::FadeOut(ms) => {
                fade_mut.fade_out = Duration::from_millis(*ms as u64);
            }
            ClipFadeProperty::Curve(curve) => {
                fade_mut.curve = *curve;
            }
        }

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    /// Merge consecutive fade property changes on the same clip (slider dragging).
    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipFadeCommand>() {
            self.clip_uuid == other_cmd.clip_uuid
                && std::mem::discriminant(&self.property)
                    == std::mem::discriminant(&other_cmd.property)
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetClipFadeCommand>() {
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

impl_serializable_command!(SetClipFadeCommand, "set_clip_fade");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Clip, StableId};
    use crate::resources::StableIdRegistry;
    use bevy_ecs::world::World;

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
    fn test_set_fade_in_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world
            .resource::<StableIdRegistry>()
            .get(&clip_uuid)
            .unwrap();
        world.entity_mut(entity).insert(AudioFade::default());

        let mut cmd = SetClipFadeCommand::set_fade_in(clip_uuid, 500);
        cmd.execute(&mut world).unwrap();

        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.fade_in, Duration::from_millis(500));

        cmd.undo(&mut world, UndoData::None).unwrap();
        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.fade_in, Duration::ZERO);
    }

    #[test]
    fn test_set_fade_out_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world
            .resource::<StableIdRegistry>()
            .get(&clip_uuid)
            .unwrap();
        world.entity_mut(entity).insert(AudioFade::default());

        let mut cmd = SetClipFadeCommand::set_fade_out(clip_uuid, 300);
        cmd.execute(&mut world).unwrap();

        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.fade_out, Duration::from_millis(300));

        cmd.undo(&mut world, UndoData::None).unwrap();
        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.fade_out, Duration::ZERO);
    }

    #[test]
    fn test_lazy_insert_and_remove_on_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world
            .resource::<StableIdRegistry>()
            .get(&clip_uuid)
            .unwrap();
        assert!(world.get::<AudioFade>(entity).is_none());

        let mut cmd = SetClipFadeCommand::set_fade_in(clip_uuid, 200);
        cmd.execute(&mut world).unwrap();

        assert!(world.get::<AudioFade>(entity).is_some());
        assert_eq!(
            world.get::<AudioFade>(entity).unwrap().fade_in,
            Duration::from_millis(200)
        );

        cmd.undo(&mut world, UndoData::None).unwrap();
        assert!(world.get::<AudioFade>(entity).is_none());
    }

    #[test]
    fn test_set_curve_execute_and_undo() {
        let (mut world, clip_uuid) = setup_test_world();

        let entity = world
            .resource::<StableIdRegistry>()
            .get(&clip_uuid)
            .unwrap();
        world.entity_mut(entity).insert(AudioFade::default());

        let mut cmd = SetClipFadeCommand::set_curve(clip_uuid, FadeCurve::Logarithmic);
        cmd.execute(&mut world).unwrap();

        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.curve, FadeCurve::Logarithmic);

        cmd.undo(&mut world, UndoData::None).unwrap();
        let fade: &AudioFade = world.get::<AudioFade>(entity).unwrap();
        assert_eq!(fade.curve, FadeCurve::EaseInOut); // Default
    }

    #[test]
    fn test_can_merge_fade_commands() {
        let uuid = uuid::Uuid::new_v4();

        let cmd1 = SetClipFadeCommand::set_fade_in(uuid, 200);
        let cmd2 = SetClipFadeCommand::set_fade_in(uuid, 300);
        let cmd_out = SetClipFadeCommand::set_fade_out(uuid, 100);
        let cmd_other = SetClipFadeCommand::set_fade_in(uuid::Uuid::new_v4(), 200);

        assert!(cmd1.can_merge(&cmd2));
        assert!(!cmd1.can_merge(&cmd_out));
        assert!(!cmd1.can_merge(&cmd_other));
    }
}
