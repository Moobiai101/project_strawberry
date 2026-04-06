//! # Move Clip Command
//!
//! Moves a clip to a new position and/or track with UUID-based undo/redo support.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, StableId, Track};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// MOVE CLIP COMMAND
// ============================================================================

/// Move a clip to a new position and/or track.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "move_clip",
///   "clip": 12345,
///   "to_track": 67890,
///   "at": 10.5
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveClipCommand {
    /// Clip entity to move (may be stale, use clip_uuid)
    pub clip: u64,
    /// Target track (None = keep on same track)
    #[serde(default)]
    pub to_track: Option<u64>,
    /// New timeline position in seconds
    pub at: f64,

    /// Stored original state for undo
    #[serde(skip)]
    original_track: Option<u64>,
    #[serde(skip)]
    original_position: Option<f64>,

    // =========================================================================
    // UUID Support
    // =========================================================================
    /// UUID of the clip (for stable lookup)
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
    /// UUID of the target track
    #[serde(skip)]
    to_track_uuid: Option<uuid::Uuid>,
    /// UUID of the original track (for undo)
    #[serde(skip)]
    original_track_uuid: Option<uuid::Uuid>,
}

impl MoveClipCommand {
    /// Create a move command that moves to a new time on the same track
    pub fn to_time(clip: Entity, at: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            to_track: None,
            at: at.as_secs_f64(),
            original_track: None,
            original_position: None,
            clip_uuid: None,
            to_track_uuid: None,
            original_track_uuid: None,
        }
    }

    /// Create a move command that moves to a different track
    pub fn to_track(clip: Entity, track: Entity, at: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            to_track: Some(track.to_bits()),
            at: at.as_secs_f64(),
            original_track: None,
            original_position: None,
            clip_uuid: None,
            to_track_uuid: None,
            original_track_uuid: None,
        }
    }

    /// Create a move command from entity bits and time in seconds (for UI integration)
    pub fn from_bits(clip_bits: u64, to_track_bits: Option<u64>, at_seconds: f64) -> Self {
        Self {
            clip: clip_bits,
            to_track: to_track_bits,
            at: at_seconds,
            original_track: None,
            original_position: None,
            clip_uuid: None,
            to_track_uuid: None,
            original_track_uuid: None,
        }
    }
}

impl Command for MoveClipCommand {
    fn name(&self) -> &'static str {
        "Move Clip"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Resolve clip entity from UUID if available, otherwise use stored bits
        let clip_entity = if let Some(uuid) = self.clip_uuid {
            world
                .resource::<StableIdRegistry>()
                .get(&uuid)
                .ok_or_else(|| {
                    CommandError::EntityNotFound(format!(
                        "Clip with UUID {} not found",
                        uuid.to_string()
                    ))
                })?
        } else {
            Entity::from_bits(self.clip)
        };

        // Cache UUID if not already stored
        if self.clip_uuid.is_none() {
            if let Some(stable_id) = world.get::<StableId>(clip_entity) {
                self.clip_uuid = Some(stable_id.uuid());
            }
        }

        // Get current clip state
        let clip = world.get::<Clip>(clip_entity).ok_or_else(|| {
            CommandError::EntityNotFound(format!("Clip {:?} not found", clip_entity))
        })?;

        // Store original position for undo
        self.original_position = Some(clip.timeline_start.as_secs_f64());

        // Get current track (parent)
        let current_track = world.get::<Parent>(clip_entity).map(|p| p.get());
        self.original_track = current_track.map(|e| e.to_bits());

        // Store original track UUID for robust undo
        if let Some(track) = current_track {
            if let Some(stable_id) = world.get::<StableId>(track) {
                self.original_track_uuid = Some(stable_id.uuid());
            }
        }

        // Update timeline position
        if let Some(mut clip) = world.get_mut::<Clip>(clip_entity) {
            clip.timeline_start = Duration::from_secs_f64(self.at);
        }

        // Resolve target track if needed
        let mut target_track_entity = None;

        // Try to resolve target track from UUID first
        if let Some(uuid) = self.to_track_uuid {
            if let Some(entity) = world.resource::<StableIdRegistry>().get(&uuid) {
                target_track_entity = Some(entity);
            }
        }

        // Fallback to bits if no UUID or lookup failed
        if target_track_entity.is_none() {
            if let Some(bits) = self.to_track {
                target_track_entity = Some(Entity::from_bits(bits));
            }
        }

        // If moving to a different track, reparent
        if let Some(to_track) = target_track_entity {
            // Cache target track UUID
            if self.to_track_uuid.is_none() {
                if let Some(stable_id) = world.get::<StableId>(to_track) {
                    self.to_track_uuid = Some(stable_id.uuid());
                }
            }

            // Verify target track exists
            if world.get::<Track>(to_track).is_none() {
                return Err(CommandError::EntityNotFound(format!(
                    "Target track {:?} not found",
                    to_track
                )));
            }

            // If different from current track, reparent
            if current_track != Some(to_track) {
                // Remove from current parent
                if let Some(old_track) = current_track {
                    world.entity_mut(old_track).remove_children(&[clip_entity]);
                }

                // Add to new parent
                world.entity_mut(to_track).add_child(clip_entity);
            }
        }

        tracing::debug!("Moved clip to {:.2}s", self.at);

        Ok(UndoData::PropertyChanged {
            entity_bits: self.clip,
            property: "position".to_string(),
            old_value: serde_json::json!({
                "track": self.original_track,
                "at": self.original_position,
            }),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Resolve clip from UUID if available
        let clip_entity = if let Some(uuid) = self.clip_uuid {
            world
                .resource::<StableIdRegistry>()
                .get(&uuid)
                .unwrap_or(Entity::from_bits(self.clip))
        } else {
            Entity::from_bits(self.clip)
        };

        // Restore original position
        if let Some(original_pos) = self.original_position {
            if let Some(mut clip) = world.get_mut::<Clip>(clip_entity) {
                clip.timeline_start = Duration::from_secs_f64(original_pos);
            }
        }

        // Restore original track if changed
        // Try UUID for original track first
        let original_track = if let Some(uuid) = self.original_track_uuid {
            world.resource::<StableIdRegistry>().get(&uuid)
        } else {
            self.original_track.map(Entity::from_bits)
        };

        let new_track = if let Some(uuid) = self.to_track_uuid {
            world.resource::<StableIdRegistry>().get(&uuid)
        } else {
            self.to_track.map(Entity::from_bits)
        };

        if let (Some(original_track), Some(new_track)) = (original_track, new_track) {
            if original_track != new_track {
                // Remove from current parent
                world.entity_mut(new_track).remove_children(&[clip_entity]);
                // Add back to original parent
                world.entity_mut(original_track).add_child(clip_entity);
            }
        }

        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        // Merge consecutive moves of the same clip (for dragging)
        if let Some(other_cmd) = other.as_any().downcast_ref::<MoveClipCommand>() {
            other_cmd.clip == self.clip
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<MoveClipCommand>() {
            // Keep original track/position from first command
            if self.original_track.is_none() {
                self.original_track = other_cmd.original_track;
            }
            if self.original_position.is_none() {
                self.original_position = other_cmd.original_position;
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(MoveClipCommand, "move_clip");
