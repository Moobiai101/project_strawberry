//! # Razor Tool Command
//!
//! Multi-track split at a specific timeline position.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Children, DespawnRecursiveExt};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, ClipMetadata, StableId, Timeline, TimelineChild, Track, TrackClip};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// RAZOR TOOL COMMAND (Multi-track split)
// ============================================================================

/// Split result for a single clip during razor operation
#[derive(Debug, Clone)]
pub(crate) struct RazorSplitResult {
    /// Original clip entity bits (may be stale after delete+undo)
    original_clip: u64,
    /// Original clip UUID (for reliable lookup after delete+undo)
    original_clip_uuid: uuid::Uuid,
    /// Original source_out before split
    original_source_out: Duration,
    /// Created clip entity bits (may be stale)
    created_clip: u64,
    /// UUID of created clip for reliable undo/redo
    created_clip_uuid: uuid::Uuid,
}

/// Split all clips at a given timeline position across multiple tracks.
///
/// ## How it works (Clean-room implementation)
///
/// The Razor tool cuts through the timeline at a specific position:
/// 1. Iterates through all tracks (or a filtered set)
/// 2. For each track, finds the clip that contains the cut position
/// 3. If found and the position is strictly inside (not at edges), splits the clip
/// 4. Collects all split operations for a single undo
///
/// ## AI Schema
/// ```json
/// {
///   "command": "razor_tool",
///   "timeline": 12345,
///   "at": 5.0,
///   "tracks": null
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RazorToolCommand {
    /// Timeline entity to operate on (as bits)
    pub timeline: u64,
    /// Timeline position to split at (in seconds)
    pub at: f64,
    /// Optional track filter (null = all tracks)
    pub tracks: Option<Vec<u64>>,

    /// Results from split operations for undo
    #[serde(skip)]
    pub(crate) split_results: Vec<RazorSplitResult>,
}

impl RazorToolCommand {
    /// Create a new razor tool command to split all clips at a position
    pub fn new(timeline: Entity, at: Duration) -> Self {
        Self {
            timeline: timeline.to_bits(),
            at: at.as_secs_f64(),
            tracks: None,
            split_results: Vec::new(),
        }
    }

    /// Create with specific track filter
    pub fn with_tracks(timeline: Entity, at: Duration, tracks: Vec<Entity>) -> Self {
        Self {
            timeline: timeline.to_bits(),
            at: at.as_secs_f64(),
            tracks: Some(tracks.iter().map(|e| e.to_bits()).collect()),
            split_results: Vec::new(),
        }
    }

    /// Create from bits (for UI/AI integration)
    pub fn from_bits(timeline_bits: u64, at_secs: f64, tracks: Option<Vec<u64>>) -> Self {
        Self {
            timeline: timeline_bits,
            at: at_secs,
            tracks,
            split_results: Vec::new(),
        }
    }
}

impl Command for RazorToolCommand {
    fn name(&self) -> &'static str {
        "Razor Tool"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let timeline_entity = Entity::from_bits(self.timeline);
        let split_point = Duration::from_secs_f64(self.at);

        // Verify timeline exists
        if world.get::<Timeline>(timeline_entity).is_none() {
            return Err(CommandError::EntityNotFound(format!(
                "Timeline {} not found",
                self.timeline
            )));
        }

        // Get all tracks in the timeline
        let timeline_children: Vec<Entity> = world
            .get::<Children>(timeline_entity)
            .map(|c| c.iter().copied().collect())
            .unwrap_or_default();

        // Filter tracks based on Track component and optional filter
        let tracks_to_process: Vec<Entity> = timeline_children
            .into_iter()
            .filter(|&child| {
                // Must be a Track
                if world.get::<Track>(child).is_none() {
                    return false;
                }
                // Apply optional filter
                if let Some(ref filter) = self.tracks {
                    filter.contains(&child.to_bits())
                } else {
                    true
                }
            })
            .collect();

        self.split_results.clear();

        // Process each track
        for track in tracks_to_process {
            // Skip locked tracks
            if let Some(track_comp) = world.get::<Track>(track) {
                if track_comp.locked {
                    continue;
                }
            }

            // Get clips on this track
            let track_clips: Vec<Entity> = world
                .get::<Children>(track)
                .map(|c| c.iter().copied().collect())
                .unwrap_or_default();

            // Find clip that contains the split point
            for clip_entity in track_clips {
                if let Some(clip) = world.get::<Clip>(clip_entity) {
                    let clip_start = clip.timeline_start;
                    let clip_end = clip.timeline_end();

                    // Check if split point is strictly inside this clip
                    if split_point > clip_start && split_point < clip_end {
                        // Store original state
                        let original_source_out = clip.source_out;

                        // Calculate the offset from timeline_start to split_point
                        let offset = split_point.saturating_sub(clip_start);
                        let new_source_split = clip.source_in + offset;

                        // Create the new clip (second half)
                        let new_clip = Clip {
                            name: format!("{} (2)", clip.name),
                            source_in: new_source_split,
                            source_out: clip.source_out,
                            timeline_start: split_point,
                        };

                        // Copy metadata
                        let metadata = world
                            .get::<ClipMetadata>(clip_entity)
                            .cloned()
                            .unwrap_or_default();

                        // Copy ClipAssetRef for filmstrip rendering (CRITICAL)
                        // This links the new clip to the same asset's thumbnails
                        let asset_ref: Option<ClipAssetRef> =
                            world.get::<ClipAssetRef>(clip_entity).cloned();

                        // Generate UUID for the new clip
                        let new_clip_uuid = uuid::Uuid::new_v4();

                        // Spawn the new clip with StableId
                        let new_clip_entity = {
                            let mut entity_cmd = world.spawn((
                                new_clip,
                                metadata,
                                TimelineChild,
                                TrackClip,
                                StableId::from_uuid(new_clip_uuid),
                            ));

                            // Copy ClipAssetRef to preserve filmstrip rendering
                            if let Some(ref ar) = asset_ref {
                                entity_cmd.insert(ar.clone());
                            }

                            entity_cmd.id()
                        };

                        // Register in StableIdRegistry
                        world
                            .resource_mut::<StableIdRegistry>()
                            .register(new_clip_uuid, new_clip_entity);

                        // Add as child of the track
                        world.entity_mut(track).add_child(new_clip_entity);

                        // Update the original clip (first half)
                        if let Some(mut original_clip) = world.get_mut::<Clip>(clip_entity) {
                            original_clip.source_out = new_source_split;
                        }

                        // Get original clip UUID for reliable undo lookup
                        let original_clip_uuid = world
                            .get::<StableId>(clip_entity)
                            .map(|s| s.0)
                            .unwrap_or_else(uuid::Uuid::new_v4);

                        // Store result for undo
                        self.split_results.push(RazorSplitResult {
                            original_clip: clip_entity.to_bits(),
                            original_clip_uuid,
                            original_source_out,
                            created_clip: new_clip_entity.to_bits(),
                            created_clip_uuid: new_clip_uuid,
                        });

                        // Only one clip per track can contain the split point
                        break;
                    }
                }
            }
        }

        tracing::info!(
            "Razor tool split {} clips at {:?}",
            self.split_results.len(),
            split_point
        );

        Ok(UndoData::Batch(
            self.split_results
                .iter()
                .map(|r| UndoData::EntityCreated(r.created_clip))
                .collect(),
        ))
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Undo each split in reverse order
        for result in self.split_results.iter_mut().rev() {
            // CRITICAL: Use UUID lookup for original clip (entity ID may be stale after delete+undo)
            let original_entity = world
                .resource::<StableIdRegistry>()
                .get(&result.original_clip_uuid)
                .or_else(|| {
                    // Fallback to entity bits if UUID lookup fails
                    let entity = Entity::from_bits(result.original_clip);
                    if world.get_entity(entity).is_ok() {
                        Some(entity)
                    } else {
                        None
                    }
                });

            if let Some(entity) = original_entity {
                if let Some(mut clip) = world.get_mut::<Clip>(entity) {
                    clip.source_out = result.original_source_out;
                }
                // Update stored entity bits for potential re-redo
                result.original_clip = entity.to_bits();
            } else {
                tracing::warn!(
                    "Could not find original clip (UUID: {}) for razor undo",
                    &result.original_clip_uuid.to_string()[..8]
                );
            }

            // Park the created clip UUID before despawning
            world
                .resource_mut::<StableIdRegistry>()
                .park(&result.created_clip_uuid);

            // CRITICAL: Use UUID lookup for created clip too
            let created_entity = world
                .resource::<StableIdRegistry>()
                .get(&result.created_clip_uuid)
                .or_else(|| {
                    let entity = Entity::from_bits(result.created_clip);
                    if world.get_entity(entity).is_ok() {
                        Some(entity)
                    } else {
                        None
                    }
                });

            if let Some(entity) = created_entity {
                world.entity_mut(entity).despawn_recursive();
            }
        }

        tracing::debug!(
            "Undid razor tool - restored {} clips",
            self.split_results.len()
        );

        // NOTE: Don't clear split_results - needed for redo
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(RazorToolCommand, "razor_tool");
