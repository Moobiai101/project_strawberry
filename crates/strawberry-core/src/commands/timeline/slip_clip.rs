//! # Slip Clip Command
//!
//! Slips a clip's source content without changing its timeline position or duration.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, StableId};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// SLIP CLIP COMMAND
// ============================================================================

/// Slip a clip's source content without changing its timeline position or duration.
///
/// ## How it works (Clean-room implementation)
///
/// Slipping shifts which portion of the source media is visible in the clip:
/// - source_in and source_out both shift by the same offset
/// - timeline_start remains unchanged
/// - Duration (source_out - source_in) remains unchanged
///
/// Example: A 10-second clip showing source 0:00-0:10, slipped by +2s
/// would now show source 0:02-0:12 (if media is long enough).
///
/// ## Validation
/// - new source_in must be >= 0
/// - If source media has known duration, new source_out must not exceed it
///
/// ## AI Schema
/// ```json
/// {
///   "command": "slip_clip",
///   "clip": 12345,
///   "offset": 1.5
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlipClipCommand {
    /// Clip entity to slip (as bits)
    pub clip: u64,
    /// Offset in seconds (positive = later in source, negative = earlier)
    pub offset: f64,

    /// Original source_in for undo
    #[serde(skip)]
    original_source_in: Option<Duration>,
    /// Original source_out for undo
    #[serde(skip)]
    original_source_out: Option<Duration>,

    // =========================================================================
    // UUID Support
    // =========================================================================
    /// UUID of the clip (for stable lookup)
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
}

impl SlipClipCommand {
    /// Create a new slip command
    ///
    /// # Arguments
    /// * `clip` - The clip entity to slip
    /// * `offset` - Offset duration (positive = later in source, negative = earlier)
    pub fn new(clip: Entity, offset: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            offset: offset.as_secs_f64(),
            original_source_in: None,
            original_source_out: None,
            clip_uuid: None,
        }
    }

    /// Create a slip command with a signed offset
    ///
    /// # Arguments
    /// * `clip` - The clip entity to slip
    /// * `offset_secs` - Offset in seconds (can be negative)
    pub fn with_signed_offset(clip: Entity, offset_secs: f64) -> Self {
        Self {
            clip: clip.to_bits(),
            offset: offset_secs,
            original_source_in: None,
            original_source_out: None,
            clip_uuid: None,
        }
    }

    /// Create from entity bits (for UI/AI integration)
    pub fn from_bits(clip_bits: u64, offset_secs: f64) -> Self {
        Self {
            clip: clip_bits,
            offset: offset_secs,
            original_source_in: None,
            original_source_out: None,
            clip_uuid: None,
        }
    }
}

impl Command for SlipClipCommand {
    fn name(&self) -> &'static str {
        "Slip Clip"
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

        // Get clip
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!("Clip {:?} not found", clip_entity))
            })?
            .clone();

        // Store original values
        self.original_source_in = Some(clip.source_in);
        self.original_source_out = Some(clip.source_out);

        // Calculate new source points
        let offset_duration = Duration::from_secs_f64(self.offset.abs());
        let (new_source_in, new_source_out) = if self.offset >= 0.0 {
            // Positive offset: shift later in source
            (
                clip.source_in + offset_duration,
                clip.source_out + offset_duration,
            )
        } else {
            // Negative offset: shift earlier in source
            (
                clip.source_in.saturating_sub(offset_duration),
                clip.source_out.saturating_sub(offset_duration),
            )
        };

        // Validate: source_in cannot go below zero (already handled by saturating_sub)
        // but we need to ensure the offset doesn't exceed available media at the start
        if self.offset < 0.0 && clip.source_in < offset_duration {
            // Clamp the offset to not go below 0
            let actual_offset = clip.source_in;
            let new_source_in = Duration::ZERO;
            let new_source_out = clip.source_out.saturating_sub(actual_offset);

            // Ensure we still have positive duration
            if new_source_out <= new_source_in {
                return Err(CommandError::ValidationFailed(
                    "Slip would result in zero or negative duration".to_string(),
                ));
            }

            // Apply clamped slip
            if let Some(mut clip_mut) = world.get_mut::<Clip>(clip_entity) {
                clip_mut.source_in = new_source_in;
                clip_mut.source_out = new_source_out;
            }

            tracing::debug!(
                "Slipped clip (clamped): source_in {:?} -> {:?}, source_out {:?} -> {:?}",
                clip.source_in,
                new_source_in,
                clip.source_out,
                new_source_out
            );
        } else {
            // Apply the slip
            if let Some(mut clip_mut) = world.get_mut::<Clip>(clip_entity) {
                clip_mut.source_in = new_source_in;
                clip_mut.source_out = new_source_out;
            }

            tracing::debug!(
                "Slipped clip: source_in {:?} -> {:?}, source_out {:?} -> {:?}",
                clip.source_in,
                new_source_in,
                clip.source_out,
                new_source_out
            );
        }

        Ok(UndoData::None)
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

        // Restore original values
        if let (Some(orig_in), Some(orig_out)) = (self.original_source_in, self.original_source_out)
        {
            if let Some(mut clip) = world.get_mut::<Clip>(clip_entity) {
                clip.source_in = orig_in;
                clip.source_out = orig_out;
                tracing::debug!(
                    "Undid slip - restored source_in {:?}, source_out {:?}",
                    orig_in,
                    orig_out
                );
            } else {
                return Err(CommandError::EntityNotFound(format!(
                    "Clip {} not found for undo",
                    self.clip
                )));
            }
        }

        // Clear state
        self.original_source_in = None;
        self.original_source_out = None;

        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        // Merge consecutive slips of the same clip
        if let Some(other_cmd) = other.as_any().downcast_ref::<SlipClipCommand>() {
            other_cmd.clip == self.clip
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SlipClipCommand>() {
            // Keep original source points from first command
            if self.original_source_in.is_none() {
                self.original_source_in = other_cmd.original_source_in;
            }
            if self.original_source_out.is_none() {
                self.original_source_out = other_cmd.original_source_out;
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

impl_serializable_command!(SlipClipCommand, "slip_clip");
