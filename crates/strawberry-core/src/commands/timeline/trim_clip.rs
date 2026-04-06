//! # Trim Clip Command
//!
//! Trims a clip's in or out point (left/right edge adjustment).

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
// TRIM CLIP COMMAND
// ============================================================================

/// Side of the clip to trim
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrimSide {
    /// Trim the in point (left edge) - changes timeline_start and source_in
    In,
    /// Trim the out point (right edge) - changes source_out
    Out,
}

/// Trim a clip's in or out point.
///
/// This command adjusts the boundaries of a clip, either from the left edge
/// (In) or right edge (Out).
///
/// ## How it works (Clean-room implementation)
///
/// **Trim In (left edge):**
/// - Changes both `timeline_start` and `source_in` together
/// - The clip's position on timeline shifts, and the source content shifts
/// - Clip end position remains the same, duration changes
///
/// **Trim Out (right edge):**
/// - Changes only `source_out`
/// - The clip's start position stays the same, duration changes
///
/// ## Validation
///
/// - Trim In: new_timeline_start >= 0, new_source_in >= 0, resulting duration > 0
/// - Trim Out: new_source_out > source_in (resulting duration > 0)
///
/// ## AI Schema
/// ```json
/// {
///   "command": "trim_clip",
///   "clip": 12345,
///   "side": "In",
///   "new_value": 2.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrimClipCommand {
    /// Clip entity to trim (as bits)
    pub clip: u64,
    /// Which side to trim
    pub side: TrimSide,
    /// New value for the trim point (in seconds)
    /// For TrimSide::In: new timeline_start position
    /// For TrimSide::Out: new source_out time
    pub new_value: f64,

    /// Stored state for undo - original timeline_start
    #[serde(skip)]
    original_timeline_start: Option<Duration>,
    /// Stored state for undo - original source_in
    #[serde(skip)]
    original_source_in: Option<Duration>,
    /// Stored state for undo - original source_out
    #[serde(skip)]
    original_source_out: Option<Duration>,

    // =========================================================================
    // UUID Support
    // =========================================================================
    /// UUID of the clip (for stable lookup)
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
}

impl TrimClipCommand {
    /// Create a new trim command
    ///
    /// # Arguments
    /// * `clip` - The clip entity to trim
    /// * `side` - Which side of the clip to trim (In or Out)
    /// * `new_value` - New value for the trim edge:
    ///   - For TrimSide::In: new timeline_start position
    ///   - For TrimSide::Out: new source_out time
    pub fn new(clip: Entity, side: TrimSide, new_value: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            side,
            new_value: new_value.as_secs_f64(),
            original_timeline_start: None,
            original_source_in: None,
            original_source_out: None,
            clip_uuid: None,
        }
    }

    /// Create a trim command for the In point (left edge)
    pub fn trim_in(clip: Entity, new_timeline_start: Duration) -> Self {
        Self::new(clip, TrimSide::In, new_timeline_start)
    }

    /// Create a trim command for the Out point (right edge)
    pub fn trim_out(clip: Entity, new_source_out: Duration) -> Self {
        Self::new(clip, TrimSide::Out, new_source_out)
    }

    /// Create from entity bits (for UI/AI integration)
    pub fn from_bits(clip_bits: u64, side: TrimSide, new_value_seconds: f64) -> Self {
        Self {
            clip: clip_bits,
            side,
            new_value: new_value_seconds,
            original_timeline_start: None,
            original_source_in: None,
            original_source_out: None,
            clip_uuid: None,
        }
    }
}

impl Command for TrimClipCommand {
    fn name(&self) -> &'static str {
        match self.side {
            TrimSide::In => "Trim In",
            TrimSide::Out => "Trim Out",
        }
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

        let new_value = Duration::from_secs_f64(self.new_value);

        // Get the clip component
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!("Clip {:?} not found", clip_entity))
            })?
            .clone();

        // Store original values for undo
        self.original_timeline_start = Some(clip.timeline_start);
        self.original_source_in = Some(clip.source_in);
        self.original_source_out = Some(clip.source_out);

        match self.side {
            TrimSide::In => {
                // Trim In: changes timeline_start and source_in together
                // The clip's end position stays the same
                let new_timeline_start = new_value;

                // Calculate how much we're moving the start
                let delta = if new_timeline_start >= clip.timeline_start {
                    new_timeline_start - clip.timeline_start
                } else {
                    // Moving start earlier (extending clip)
                    Duration::ZERO
                };

                let new_source_in = if new_timeline_start >= clip.timeline_start {
                    // Moving start later (trimming clip shorter)
                    clip.source_in + delta
                } else {
                    // Moving start earlier (extending clip)
                    // Can only extend if source_in > 0 (there's media before current in point)
                    let extend_amount = clip.timeline_start - new_timeline_start;
                    clip.source_in.saturating_sub(extend_amount)
                };

                // Validate: new_source_in must be < source_out (positive duration)
                if new_source_in >= clip.source_out {
                    return Err(CommandError::ValidationFailed(format!(
                        "Trim would result in zero or negative duration: source_in {:?} >= source_out {:?}",
                        new_source_in, clip.source_out
                    )));
                }

                // Apply the trim
                let mut clip_mut = world.get_mut::<Clip>(clip_entity).unwrap();
                clip_mut.timeline_start = new_timeline_start;
                clip_mut.source_in = new_source_in;

                tracing::debug!(
                    "Trimmed In: timeline_start {:?} -> {:?}, source_in {:?} -> {:?}",
                    clip.timeline_start,
                    new_timeline_start,
                    clip.source_in,
                    new_source_in,
                );
            }

            TrimSide::Out => {
                // Trim Out: changes only source_out
                let new_source_out = new_value;

                // Validate: new_source_out must be > source_in (positive duration)
                if new_source_out <= clip.source_in {
                    return Err(CommandError::ValidationFailed(format!(
                        "Trim would result in zero or negative duration: source_out {:?} <= source_in {:?}",
                        new_source_out, clip.source_in
                    )));
                }

                // Apply the trim
                let mut clip_mut = world.get_mut::<Clip>(clip_entity).unwrap();
                clip_mut.source_out = new_source_out;

                tracing::debug!(
                    "Trimmed Out: source_out {:?} -> {:?}",
                    clip.source_out,
                    new_source_out
                );
            }
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
        if let (Some(orig_timeline_start), Some(orig_source_in), Some(orig_source_out)) = (
            self.original_timeline_start,
            self.original_source_in,
            self.original_source_out,
        ) {
            if let Some(mut clip) = world.get_mut::<Clip>(clip_entity) {
                clip.timeline_start = orig_timeline_start;
                clip.source_in = orig_source_in;
                clip.source_out = orig_source_out;

                tracing::debug!("Undid trim - restored original clip bounds");
            } else {
                return Err(CommandError::EntityNotFound(format!(
                    "Clip {} not found for undo",
                    self.clip
                )));
            }
        }

        // Clear stored state
        self.original_timeline_start = None;
        self.original_source_in = None;
        self.original_source_out = None;

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(TrimClipCommand, "trim_clip");
