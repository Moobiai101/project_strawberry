//! # Slide Clip Command
//!
//! Slides a clip in time while adjusting adjacent clips' boundaries.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{Children, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, StableId};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// SLIDE CLIP COMMAND
// ============================================================================

/// Slide a clip in time while adjusting adjacent clips to compensate.
///
/// ## How it works (Clean-room implementation)
///
/// Unlike a normal move, slide adjusts neighboring clips to maintain continuity:
/// - The previous clip's source_out is extended/contracted
/// - The next clip's timeline_start and source_in are adjusted
/// - The total timeline duration remains the same
///
/// Think of it as "pushing" a clip between its neighbors.
///
/// ## What changes:
///
/// **Sliding RIGHT by delta:**
/// - Previous clip: source_out += delta (extends to cover more of its source)
/// - Target clip: timeline_start += delta (moves right)
/// - Next clip: timeline_start += delta, source_in += delta (contracts from the left)
///
/// **Sliding LEFT by delta:**
/// - Previous clip: source_out -= delta (contracts from the right)
/// - Target clip: timeline_start -= delta (moves left)
/// - Next clip: timeline_start -= delta, source_in -= delta (extends to show earlier source)
///
/// ## AI Schema
/// ```json
/// {
///   "command": "slide_clip",
///   "clip": 12345,
///   "delta": 1.5
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideClipCommand {
    /// Clip entity to slide (as bits)
    pub clip: u64,
    /// Slide delta in seconds (positive = right, negative = left)
    pub delta: f64,

    /// Original position of the clip for undo
    #[serde(skip)]
    original_position: Option<f64>,
    /// Previous clip entity bits (for undo)
    #[serde(skip)]
    previous_clip: Option<u64>,
    /// Previous clip's original source_out (for undo)
    #[serde(skip)]
    previous_source_out: Option<Duration>,
    /// Next clip entity bits (for undo)
    #[serde(skip)]
    next_clip: Option<u64>,
    /// Next clip's original timeline_start (for undo)
    #[serde(skip)]
    next_timeline_start: Option<Duration>,
    /// Next clip's original source_in (for undo)
    #[serde(skip)]
    next_source_in: Option<Duration>,

    // =========================================================================
    // UUID Support
    // =========================================================================
    /// UUID of the target clip (for stable lookup)
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
    /// UUID of previous clip (for stable undo)
    #[serde(skip)]
    previous_clip_uuid: Option<uuid::Uuid>,
    /// UUID of next clip (for stable undo)
    #[serde(skip)]
    next_clip_uuid: Option<uuid::Uuid>,
}

impl SlideClipCommand {
    /// Create a new slide command with a signed delta
    pub fn with_signed_delta(clip: Entity, delta_secs: f64) -> Self {
        Self {
            clip: clip.to_bits(),
            delta: delta_secs,
            original_position: None,
            previous_clip: None,
            previous_source_out: None,
            next_clip: None,
            next_timeline_start: None,
            next_source_in: None,
            clip_uuid: None,
            previous_clip_uuid: None,
            next_clip_uuid: None,
        }
    }

    /// Create from entity bits (for UI/AI integration)
    pub fn from_bits(clip_bits: u64, delta_secs: f64) -> Self {
        Self {
            clip: clip_bits,
            delta: delta_secs,
            original_position: None,
            previous_clip: None,
            previous_source_out: None,
            next_clip: None,
            next_timeline_start: None,
            next_source_in: None,
            clip_uuid: None,
            previous_clip_uuid: None,
            next_clip_uuid: None,
        }
    }
}

impl Command for SlideClipCommand {
    fn name(&self) -> &'static str {
        "Slide Clip"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Resolve clip entity from UUID if available
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

        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!("Clip {:?} not found", clip_entity))
            })?
            .clone();

        // Store original position
        self.original_position = Some(clip.timeline_start.as_secs_f64());

        let delta_duration = Duration::from_secs_f64(self.delta.abs());
        let moving_right = self.delta > 0.0;

        // Find neighboring clips on the same track
        let parent = world.get::<Parent>(clip_entity).map(|p| p.get());

        let (prev, next) = if let Some(track) = parent {
            let children: Vec<Entity> = world
                .get::<Children>(track)
                .map(|c| c.iter().copied().collect())
                .unwrap_or_default();

            let mut previous: Option<Entity> = None;
            let mut next: Option<Entity> = None;

            // Find clips immediately before and after the target clip
            for &child in &children {
                if child == clip_entity {
                    continue;
                }
                if let Some(child_clip) = world.get::<Clip>(child) {
                    // Previous: ends at or before our start, and is the latest such clip
                    if child_clip.timeline_end() <= clip.timeline_start {
                        if let Some(prev_entity) = previous {
                            let prev_clip = world.get::<Clip>(prev_entity).unwrap();
                            if child_clip.timeline_end() > prev_clip.timeline_end() {
                                previous = Some(child);
                            }
                        } else {
                            previous = Some(child);
                        }
                    }

                    // Next: starts at or after our end, and is the earliest such clip
                    if child_clip.timeline_start >= clip.timeline_end() {
                        if let Some(next_entity) = next {
                            let next_clip = world.get::<Clip>(next_entity).unwrap();
                            if child_clip.timeline_start < next_clip.timeline_start {
                                next = Some(child);
                            }
                        } else {
                            next = Some(child);
                        }
                    }
                }
            }

            (previous, next)
        } else {
            (None, None)
        };

        // 1. Adjust previous clip (if present)
        if let Some(prev_entity) = prev {
            let prev_clip = world.get::<Clip>(prev_entity).unwrap().clone();
            self.previous_clip = Some(prev_entity.to_bits());
            self.previous_source_out = Some(prev_clip.source_out);

            // Store previous clip UUID
            if let Some(stable_id) = world.get::<StableId>(prev_entity) {
                self.previous_clip_uuid = Some(stable_id.uuid());
            }

            if let Some(mut prev_clip_mut) = world.get_mut::<Clip>(prev_entity) {
                if moving_right {
                    // Extend previous clip (its source_out increases)
                    prev_clip_mut.source_out = prev_clip.source_out + delta_duration;
                } else {
                    // Contract previous clip (its source_out decreases)
                    prev_clip_mut.source_out =
                        prev_clip.source_out.saturating_sub(delta_duration);
                }
            }
        }

        // 2. Move the target clip
        if let Some(mut clip_mut) = world.get_mut::<Clip>(clip_entity) {
            if moving_right {
                clip_mut.timeline_start = clip.timeline_start + delta_duration;
            } else {
                clip_mut.timeline_start = clip.timeline_start.saturating_sub(delta_duration);
            }
        }

        // 3. Adjust next clip (if present)
        if let Some(next_entity) = next {
            let next_clip = world.get::<Clip>(next_entity).unwrap().clone();
            self.next_clip = Some(next_entity.to_bits());
            self.next_timeline_start = Some(next_clip.timeline_start);
            self.next_source_in = Some(next_clip.source_in);

            // Store next clip UUID
            if let Some(stable_id) = world.get::<StableId>(next_entity) {
                self.next_clip_uuid = Some(stable_id.uuid());
            }

            if let Some(mut next_clip_mut) = world.get_mut::<Clip>(next_entity) {
                if moving_right {
                    // Contract next clip (shift its start later, increase source_in)
                    next_clip_mut.timeline_start = next_clip.timeline_start + delta_duration;
                    next_clip_mut.source_in = next_clip.source_in + delta_duration;
                } else {
                    // Extend next clip (shift its start earlier, decrease source_in)
                    next_clip_mut.timeline_start =
                        next_clip.timeline_start.saturating_sub(delta_duration);
                    next_clip_mut.source_in = next_clip.source_in.saturating_sub(delta_duration);
                }
            }
        }

        tracing::info!(
            "Slid clip '{}' by {:.2}s ({})",
            clip.name,
            self.delta,
            if moving_right { "right" } else { "left" }
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Restore target clip position
        if let Some(orig_pos) = self.original_position {
            // Resolve clip from UUID if available
            let clip_entity = if let Some(uuid) = self.clip_uuid {
                world
                    .resource::<StableIdRegistry>()
                    .get(&uuid)
                    .unwrap_or(Entity::from_bits(self.clip))
            } else {
                Entity::from_bits(self.clip)
            };

            if let Some(mut clip) = world.get_mut::<Clip>(clip_entity) {
                clip.timeline_start = Duration::from_secs_f64(orig_pos);
            }
        }

        // Restore previous clip
        if let (Some(prev_bits), Some(prev_out)) = (self.previous_clip, self.previous_source_out) {
            // Resolve prev clip from UUID if available
            let prev_entity = if let Some(uuid) = self.previous_clip_uuid {
                world
                    .resource::<StableIdRegistry>()
                    .get(&uuid)
                    .unwrap_or(Entity::from_bits(prev_bits))
            } else {
                Entity::from_bits(prev_bits)
            };

            if let Some(mut prev_clip) = world.get_mut::<Clip>(prev_entity) {
                prev_clip.source_out = prev_out;
            }
        }

        // Restore next clip
        if let (Some(next_bits), Some(next_start), Some(next_in)) = (
            self.next_clip,
            self.next_timeline_start,
            self.next_source_in,
        ) {
            // Resolve next clip from UUID if available
            let next_entity = if let Some(uuid) = self.next_clip_uuid {
                world
                    .resource::<StableIdRegistry>()
                    .get(&uuid)
                    .unwrap_or(Entity::from_bits(next_bits))
            } else {
                Entity::from_bits(next_bits)
            };

            if let Some(mut next_clip) = world.get_mut::<Clip>(next_entity) {
                next_clip.timeline_start = next_start;
                next_clip.source_in = next_in;
            }
        }

        tracing::debug!("Undid slide clip");

        // Clear state
        self.original_position = None;
        self.previous_clip = None;
        self.previous_source_out = None;
        self.next_clip = None;
        self.next_timeline_start = None;
        self.next_source_in = None;

        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        // Merge consecutive slides of the same clip
        if let Some(other_cmd) = other.as_any().downcast_ref::<SlideClipCommand>() {
            other_cmd.clip == self.clip
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SlideClipCommand>() {
            // Keep original state from first command
            if self.original_position.is_none() {
                self.original_position = other_cmd.original_position;
            }
            if self.previous_clip.is_none() {
                self.previous_clip = other_cmd.previous_clip;
                self.previous_source_out = other_cmd.previous_source_out;
            }
            if self.next_clip.is_none() {
                self.next_clip = other_cmd.next_clip;
                self.next_timeline_start = other_cmd.next_timeline_start;
                self.next_source_in = other_cmd.next_source_in;
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

impl_serializable_command!(SlideClipCommand, "slide_clip");
