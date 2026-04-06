//! # Delete Clip Command
//!
//! Deletes a clip from the timeline with UUID-based undo/redo support.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, DespawnRecursiveExt, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, ClipMetadata, StableId, TimelineChild, TrackClip};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// DELETE CLIP COMMAND
// ============================================================================

/// Delete a clip from the timeline.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "delete_clip",
///   "clip": 12345
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteClipCommand {
    /// Clip entity to delete
    pub clip: u64,

    /// Stored state for undo (serialized clip data)
    #[serde(skip)]
    stored_clip: Option<Clip>,
    #[serde(skip)]
    stored_metadata: Option<ClipMetadata>,
    /// Stored asset reference for undo (CRITICAL: enables video playback + filmstrip)
    #[serde(skip)]
    stored_asset_ref: Option<ClipAssetRef>,
    #[serde(skip)]
    parent_track: Option<u64>,
    /// Stored UUID for the clip - persists across undo/redo
    #[serde(skip)]
    stored_uuid: Option<uuid::Uuid>,
}

impl DeleteClipCommand {
    /// Create a delete clip command
    pub fn new(clip: Entity) -> Self {
        Self {
            clip: clip.to_bits(),
            stored_clip: None,
            stored_metadata: None,
            stored_asset_ref: None,
            parent_track: None,
            stored_uuid: None,
        }
    }
}

impl Command for DeleteClipCommand {
    fn name(&self) -> &'static str {
        "Delete Clip"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // On redo, the original entity ID is stale - lookup by UUID if we have it
        let clip_entity = if let Some(uuid) = self.stored_uuid {
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

        // Store clip data for undo
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| CommandError::EntityNotFound(format!("Clip {} not found", self.clip)))?
            .clone();
        self.stored_clip = Some(clip);

        // Store metadata if present
        self.stored_metadata = world.get::<ClipMetadata>(clip_entity).cloned();

        // Store asset reference (CRITICAL: enables video playback + filmstrip after undo)
        self.stored_asset_ref = world.get::<ClipAssetRef>(clip_entity).cloned();

        // Store parent track
        self.parent_track = world.get::<Parent>(clip_entity).map(|p| p.get().to_bits());

        // Store UUID for redo - get from component or registry
        if self.stored_uuid.is_none() {
            self.stored_uuid = world.get::<StableId>(clip_entity).map(|s| s.0);
        }

        // Park UUID in registry (marks as inactive)
        if let Some(uuid) = self.stored_uuid {
            world.resource_mut::<StableIdRegistry>().park(&uuid);
        }

        // Despawn the clip
        world.entity_mut(clip_entity).despawn_recursive();

        tracing::debug!("Deleted clip {} (UUID: {:?})", self.clip, self.stored_uuid);

        Ok(UndoData::EntityDeleted {
            entity_bits: clip_entity.to_bits(),
            serialized: Vec::new(), // We use stored_clip instead
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let (Some(clip), Some(parent_bits)) = (&self.stored_clip, self.parent_track) {
            let parent_track = Entity::from_bits(parent_bits);

            // Get or create stable ID
            let stable_uuid = self.stored_uuid.unwrap_or_else(|| {
                let new_uuid = uuid::Uuid::new_v4();
                self.stored_uuid = Some(new_uuid);
                new_uuid
            });

            // Recreate clip entity with StableId
            let mut cmd = world.spawn((
                clip.clone(),
                TimelineChild,
                TrackClip,
                StableId::from_uuid(stable_uuid),
            ));

            if let Some(meta) = &self.stored_metadata {
                cmd.insert(meta.clone());
            }

            // Restore asset reference (CRITICAL: enables video playback + filmstrip)
            if let Some(asset_ref) = &self.stored_asset_ref {
                cmd.insert(asset_ref.clone());
            }

            let clip_entity = cmd.id();

            // Restore UUID in registry
            world
                .resource_mut::<StableIdRegistry>()
                .restore(stable_uuid, clip_entity);

            // Re-parent to track
            world.entity_mut(parent_track).add_child(clip_entity);

            // Update self.clip to the new entity ID for potential re-redo
            self.clip = clip_entity.to_bits();

            tracing::debug!("Restored clip (UUID: {})", &stable_uuid.to_string()[..8]);
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

impl_serializable_command!(DeleteClipCommand, "delete_clip");
