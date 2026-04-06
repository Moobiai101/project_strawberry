//! # Ripple Delete Command
//!
//! Deletes a clip and closes the resulting gap by shifting subsequent clips.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Children, DespawnRecursiveExt, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, ClipMetadata, StableId, TimelineChild, TrackClip};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// RIPPLE DELETE COMMAND
// ============================================================================

/// Delete a clip and close the resulting gap by shifting all subsequent clips
/// on the same track earlier in time.
///
/// ## How it works (Clean-room implementation)
///
/// Unlike regular delete which leaves a gap, ripple delete:
/// 1. Stores the deleted clip's data for undo
/// 2. Finds all clips on the same track that start AFTER the deleted clip ends
/// 3. Shifts each subsequent clip earlier by the deleted clip's duration
/// 4. Despawns the clip
///
/// This maintains continuity on the track without leaving gaps.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "ripple_delete",
///   "clip": 12345
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RippleDeleteCommand {
    /// Clip entity to delete (as bits)
    pub clip: u64,

    /// Stored clip data for undo
    #[serde(skip)]
    stored_clip: Option<Clip>,
    #[serde(skip)]
    stored_metadata: Option<ClipMetadata>,
    /// Stored asset reference for undo (CRITICAL: enables video playback + filmstrip)
    #[serde(skip)]
    stored_asset_ref: Option<ClipAssetRef>,
    #[serde(skip)]
    parent_track: Option<u64>,
    /// Clips that were shifted: (entity_bits, original_timeline_start_secs)
    #[serde(skip)]
    shifted_clips: Vec<(u64, f64)>,
    /// Stored UUID for redo support
    #[serde(skip)]
    stored_uuid: Option<uuid::Uuid>,
}

impl RippleDeleteCommand {
    /// Create a new ripple delete command
    pub fn new(clip: Entity) -> Self {
        Self {
            clip: clip.to_bits(),
            stored_clip: None,
            stored_metadata: None,
            stored_asset_ref: None,
            parent_track: None,
            shifted_clips: Vec::new(),
            stored_uuid: None,
        }
    }

    /// Create from entity bits (for UI/AI integration)
    pub fn from_bits(clip_bits: u64) -> Self {
        Self {
            clip: clip_bits,
            stored_clip: None,
            stored_metadata: None,
            stored_asset_ref: None,
            parent_track: None,
            shifted_clips: Vec::new(),
            stored_uuid: None,
        }
    }
}

impl Command for RippleDeleteCommand {
    fn name(&self) -> &'static str {
        "Ripple Delete"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // On redo, try to lookup by stored UUID first (in case entity bits are stale)
        let clip_entity = if let Some(uuid) = self.stored_uuid {
            world
                .resource::<StableIdRegistry>()
                .get(&uuid)
                .ok_or_else(|| {
                    CommandError::EntityNotFound(format!(
                        "Clip with UUID {} not found in registry",
                        &uuid.to_string()[..8]
                    ))
                })?
        } else {
            Entity::from_bits(self.clip)
        };

        // Get clip data
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| CommandError::EntityNotFound(format!("Clip {} not found", self.clip)))?
            .clone();

        // Store UUID if not already stored (first execution)
        if self.stored_uuid.is_none() {
            if let Some(stable_id) = world.get::<StableId>(clip_entity) {
                self.stored_uuid = Some(stable_id.uuid());
            }
        }

        // Store for undo
        self.stored_clip = Some(clip.clone());
        self.stored_metadata = world.get::<ClipMetadata>(clip_entity).cloned();

        // Store asset reference (CRITICAL: enables video playback + filmstrip after undo)
        self.stored_asset_ref = world.get::<ClipAssetRef>(clip_entity).cloned();

        // Get parent track
        let parent_track = world
            .get::<Parent>(clip_entity)
            .ok_or_else(|| CommandError::InvalidOperation("Clip has no parent track".to_string()))?
            .get();
        self.parent_track = Some(parent_track.to_bits());

        // Calculate the duration of the clip being deleted
        let clip_duration = clip.duration();
        let clip_end = clip.timeline_end();

        // Find all sibling clips on the same track
        let track_children: Vec<Entity> = world
            .get::<Children>(parent_track)
            .map(|c| c.iter().copied().collect())
            .unwrap_or_default();

        // Find clips that start after this clip ends and shift them
        self.shifted_clips.clear();
        for &sibling in &track_children {
            if sibling == clip_entity {
                continue;
            }
            if let Some(sibling_clip) = world.get::<Clip>(sibling) {
                if sibling_clip.timeline_start >= clip_end {
                    // This clip needs to be shifted
                    self.shifted_clips
                        .push((sibling.to_bits(), sibling_clip.timeline_start.as_secs_f64()));
                }
            }
        }

        // Apply the shift to subsequent clips
        for &(sibling_bits, _original_start) in &self.shifted_clips {
            let sibling = Entity::from_bits(sibling_bits);
            if let Some(mut sibling_clip) = world.get_mut::<Clip>(sibling) {
                sibling_clip.timeline_start =
                    sibling_clip.timeline_start.saturating_sub(clip_duration);
            }
        }

        // Park the UUID before despawning
        if let Some(uuid) = self.stored_uuid {
            world.resource_mut::<StableIdRegistry>().park(&uuid);
        }

        // Despawn the clip
        world.entity_mut(clip_entity).despawn_recursive();

        tracing::info!(
            "Ripple deleted clip '{}', shifted {} subsequent clips by {:?}",
            clip.name,
            self.shifted_clips.len(),
            clip_duration
        );

        Ok(UndoData::EntityDeleted {
            entity_bits: self.clip,
            serialized: Vec::new(),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Restore shifted clips to their original positions
        for &(sibling_bits, original_start) in &self.shifted_clips {
            let sibling = Entity::from_bits(sibling_bits);
            if let Some(mut sibling_clip) = world.get_mut::<Clip>(sibling) {
                sibling_clip.timeline_start = Duration::from_secs_f64(original_start);
            }
        }

        // Respawn the deleted clip with StableId
        if let (Some(clip), Some(parent_bits)) = (&self.stored_clip, self.parent_track) {
            let parent_track = Entity::from_bits(parent_bits);

            // Get or create UUID for the restored clip
            let clip_uuid = self.stored_uuid.unwrap_or_else(|| {
                let uuid = uuid::Uuid::new_v4();
                self.stored_uuid = Some(uuid);
                uuid
            });

            let mut cmd = world.spawn((
                clip.clone(),
                TimelineChild,
                TrackClip,
                StableId::from_uuid(clip_uuid),
            ));

            if let Some(meta) = &self.stored_metadata {
                cmd.insert(meta.clone());
            }

            // Restore asset reference (CRITICAL: enables video playback + filmstrip)
            if let Some(asset_ref) = &self.stored_asset_ref {
                cmd.insert(asset_ref.clone());
            }

            let clip_entity = cmd.id();
            world.entity_mut(parent_track).add_child(clip_entity);

            // Restore UUID mapping in registry
            world
                .resource_mut::<StableIdRegistry>()
                .restore(clip_uuid, clip_entity);

            // Update self.clip for potential re-redo
            self.clip = clip_entity.to_bits();

            tracing::debug!(
                "Undid ripple delete, restored clip (UUID: {}) and {} shifted clips",
                &clip_uuid.to_string()[..8],
                self.shifted_clips.len()
            );
        }

        // NOTE: Don't clear shifted_clips - needed for redo to restore shifts

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl_serializable_command!(RippleDeleteCommand, "ripple_delete");
