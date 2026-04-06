//! # Split Clip Command
//!
//! Splits a clip at a specific timeline position, creating two clips.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, DespawnRecursiveExt, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, ClipMetadata, StableId, TimelineChild, TrackClip};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// SPLIT CLIP COMMAND
// ============================================================================

/// Split a clip at a specific timeline position, creating two clips.
///
/// The original clip is shortened and a new clip is created covering the
/// remaining duration. Both clips reference the same source asset but with
/// different source in/out points.
///
/// ## How it works (Clean-room implementation)
///
/// Given a clip with:
/// - timeline_start = T
/// - source_in = S_in
/// - source_out = S_out
/// - duration = S_out - S_in
///
/// Splitting at position P (where T < P < T + duration) results in:
///
/// **Original clip (before split point):**
/// - timeline_start = T (unchanged)
/// - source_in = S_in (unchanged)
/// - source_out = S_in + (P - T)  ← new
///
/// **New clip (after split point):**
/// - timeline_start = P
/// - source_in = S_in + (P - T)
/// - source_out = S_out
///
/// ## AI Schema
/// ```json
/// {
///   "command": "split_clip",
///   "clip": 12345,
///   "at": 5.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitClipCommand {
    /// Clip entity to split (as bits) - may be stale on redo
    pub clip: u64,
    /// Timeline position to split at (in seconds)
    pub at: f64,

    /// Stored state for undo - the new clip entity created by split
    /// This is cleared on undo, updated on each execute
    #[serde(skip)]
    pub(crate) created_clip: Option<u64>,
    /// Original source_out of the clip before split (for undo)
    #[serde(skip)]
    original_source_out: Option<Duration>,
    /// Stored new clip data for redo - persists through undo/redo cycle
    /// This allows us to recreate the same clip on redo
    #[serde(skip)]
    stored_new_clip: Option<Clip>,
    /// Stored metadata for redo
    #[serde(skip)]
    stored_new_metadata: Option<ClipMetadata>,
    /// Stored asset reference for redo (CRITICAL: enables video playback)
    #[serde(skip)]
    stored_asset_ref: Option<ClipAssetRef>,
    /// Parent track entity bits (for redo)
    #[serde(skip)]
    parent_track: Option<u64>,
    /// UUID of the source clip (for redo lookup when entity ID is stale)
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
    /// UUID assigned to the created clip (persists through undo/redo)
    #[serde(skip)]
    created_clip_uuid: Option<uuid::Uuid>,
}

impl SplitClipCommand {
    /// Create a new split command
    ///
    /// # Arguments
    /// * `clip` - The clip entity to split
    /// * `at` - Timeline position to split at (must be within clip's timeline range)
    pub fn new(clip: Entity, at: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            at: at.as_secs_f64(),
            created_clip: None,
            original_source_out: None,
            stored_new_clip: None,
            stored_new_metadata: None,
            stored_asset_ref: None,
            parent_track: None,
            clip_uuid: None,
            created_clip_uuid: None,
        }
    }

    pub fn from_bits(clip_bits: u64, at_seconds: f64) -> Self {
        Self {
            clip: clip_bits,
            at: at_seconds,
            created_clip: None,
            original_source_out: None,
            stored_new_clip: None,
            stored_new_metadata: None,
            stored_asset_ref: None,
            parent_track: None,
            clip_uuid: None,
            created_clip_uuid: None,
        }
    }
}

impl Command for SplitClipCommand {
    fn name(&self) -> &'static str {
        "Split Clip"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // On redo, the original entity ID may be stale - lookup by UUID if we have it
        let clip_entity = if let Some(uuid) = self.clip_uuid {
            world
                .resource::<StableIdRegistry>()
                .get(&uuid)
                .ok_or_else(|| {
                    CommandError::EntityNotFound(format!(
                        "Clip UUID {} not found",
                        &uuid.to_string()[..8]
                    ))
                })?
        } else {
            Entity::from_bits(self.clip)
        };

        let split_point = Duration::from_secs_f64(self.at);

        // Get the clip component
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| CommandError::EntityNotFound(format!("Clip {} not found", self.clip)))?
            .clone();

        // Store source clip UUID on first execute (for redo lookups)
        if self.clip_uuid.is_none() {
            self.clip_uuid = world.get::<StableId>(clip_entity).map(|s| s.0);
        }

        // Validate: split point must be strictly inside the clip's timeline range
        // (not at the very start or very end)
        let clip_start = clip.timeline_start;
        let clip_end = clip.timeline_end();

        if split_point <= clip_start {
            return Err(CommandError::ValidationFailed(format!(
                "Split point {:?} is at or before clip start {:?}",
                split_point, clip_start
            )));
        }
        if split_point >= clip_end {
            return Err(CommandError::ValidationFailed(format!(
                "Split point {:?} is at or after clip end {:?}",
                split_point, clip_end
            )));
        }

        // Store original state for undo (only on first execute)
        if self.original_source_out.is_none() {
            self.original_source_out = Some(clip.source_out);
        }

        // Get parent track for the new clip
        let parent_track = world
            .get::<Parent>(clip_entity)
            .ok_or_else(|| CommandError::InvalidOperation("Clip has no parent track".to_string()))?
            .get();

        // Store parent track for redo
        self.parent_track = Some(parent_track.to_bits());

        // Calculate the offset from timeline_start to split_point
        let offset = split_point.saturating_sub(clip_start);

        // Calculate new source points
        let new_source_split = clip.source_in + offset;

        // === Create the new clip (second half) ===
        // Use stored data if available (redo case), otherwise create fresh
        let new_clip = if let Some(stored) = &self.stored_new_clip {
            stored.clone()
        } else {
            let fresh = Clip {
                name: format!("{} (2)", clip.name),
                source_in: new_source_split,
                source_out: clip.source_out,
                timeline_start: split_point,
            };
            // Store for future redo operations
            self.stored_new_clip = Some(fresh.clone());
            fresh
        };

        // Get or restore metadata
        let metadata = if let Some(stored) = &self.stored_new_metadata {
            stored.clone()
        } else {
            let meta = world
                .get::<ClipMetadata>(clip_entity)
                .cloned()
                .unwrap_or_default();
            // Store for future redo operations
            self.stored_new_metadata = Some(meta.clone());
            meta
        };

        // Get or restore asset reference (CRITICAL: enables video playback on new clip)
        let asset_ref: Option<ClipAssetRef> = if let Some(stored) = &self.stored_asset_ref {
            Some(stored.clone())
        } else {
            let ar: Option<ClipAssetRef> = world.get::<ClipAssetRef>(clip_entity).cloned();
            // Store for future redo operations
            if ar.is_some() {
                self.stored_asset_ref = ar.clone();
            }
            ar
        };

        // Get or create stable UUID for the new clip
        let new_clip_uuid = self.created_clip_uuid.unwrap_or_else(|| {
            let uuid = uuid::Uuid::new_v4();
            self.created_clip_uuid = Some(uuid);
            uuid
        });

        // Spawn the new clip with StableId
        let mut new_clip_cmd = world.spawn((
            new_clip.clone(),
            metadata,
            TimelineChild,
            TrackClip,
            StableId::from_uuid(new_clip_uuid),
        ));

        // Attach asset reference if present (CRITICAL for video playback)
        if let Some(ref ar) = asset_ref {
            new_clip_cmd.insert(ar.clone());
        }

        let new_clip_entity = new_clip_cmd.id();

        // Register in StableIdRegistry
        world
            .resource_mut::<StableIdRegistry>()
            .register(new_clip_uuid, new_clip_entity);

        // Add as child of the same track
        world.entity_mut(parent_track).add_child(new_clip_entity);

        // Store created entity for undo (updated on each execute)
        self.created_clip = Some(new_clip_entity.to_bits());

        // === Update the original clip (first half) ===
        if let Some(mut original_clip) = world.get_mut::<Clip>(clip_entity) {
            original_clip.source_out = new_source_split;
        }

        tracing::info!(
            "Split clip '{}' at {:?} - created new clip (UUID: {})",
            clip.name,
            split_point,
            &new_clip_uuid.to_string()[..8]
        );

        Ok(UndoData::Batch(vec![
            UndoData::EntityCreated(new_clip_entity.to_bits()),
            UndoData::PropertyChanged {
                entity_bits: self.clip,
                property: "source_out".to_string(),
                old_value: serde_json::json!(clip.source_out.as_secs_f64()),
            },
        ]))
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Restore original clip's source_out
        // CRITICAL: Use UUID lookup since entity ID may be stale after delete+undo cycle
        if let Some(original_source_out) = self.original_source_out {
            let clip_entity = if let Some(uuid) = self.clip_uuid {
                // Prefer UUID lookup (handles delete+undo case where entity ID changed)
                world.resource::<StableIdRegistry>().get(&uuid)
            } else {
                // Fallback to entity bits (first undo before any delete)
                let entity = Entity::from_bits(self.clip);
                if world.get_entity(entity).is_ok() {
                    Some(entity)
                } else {
                    None
                }
            };

            if let Some(entity) = clip_entity {
                if let Some(mut clip) = world.get_mut::<Clip>(entity) {
                    clip.source_out = original_source_out;
                }
                // Update stored entity bits to current ID for potential re-redo
                self.clip = entity.to_bits();
            } else {
                tracing::warn!(
                    "Could not find original clip (UUID: {:?}, bits: {}) for split undo",
                    self.clip_uuid,
                    self.clip
                );
            }
        }

        // Delete the created clip and park its UUID
        // CRITICAL: Use UUID lookup for created clip too
        let created_clip_entity = if let Some(uuid) = self.created_clip_uuid {
            world.resource::<StableIdRegistry>().get(&uuid)
        } else if let Some(bits) = self.created_clip {
            let entity = Entity::from_bits(bits);
            if world.get_entity(entity).is_ok() {
                Some(entity)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(entity) = created_clip_entity {
            // Park the UUID before despawning
            if let Some(uuid) = self.created_clip_uuid {
                world.resource_mut::<StableIdRegistry>().park(&uuid);
                tracing::debug!(
                    "Parked created clip UUID {} for potential redo",
                    &uuid.to_string()[..8]
                );
            }
            world.entity_mut(entity).despawn_recursive();
            // Clear created_clip since entity is despawned
            self.created_clip = None;
        }

        // NOTE: We intentionally do NOT clear:
        // - original_source_out (needed for redo validation)
        // - stored_new_clip (needed to recreate on redo)
        // - stored_new_metadata (needed to recreate on redo)
        // - parent_track (needed for redo)
        // - clip_uuid (needed for future undo after redo)
        // - created_clip_uuid (needed to restore on redo)
        // This follows Olive's pattern of "parking" data for redo

        tracing::debug!("Undid split clip");
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    // NOTE: remap_entity(), created_entity_id(), and new_created_entity_id() are no longer needed
    // with the UUID-based architecture. The StableIdRegistry handles entity identity across
    // undo/redo cycles automatically - commands lookup entities by their stable UUIDs.
}

impl_serializable_command!(SplitClipCommand, "split_clip");
