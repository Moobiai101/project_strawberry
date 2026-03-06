//! # Asset Commands
//!
//! Commands for managing project assets (import, delete, regenerate proxy).
//!
//! ## Commands
//!
//! - `DeleteAssetCommand` - Remove asset and cleanup proxy
//! - `RegenerateProxyCommand` - Force proxy regeneration
//! - `AddAssetToTimelineCommand` - Create clip from asset
//!
//! ## Architecture
//!
//! All commands use stable UUIDs for asset identification.
//! Proxy cleanup is handled via strawberry-video's ProxyManager.

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{Clip, Track};
use crate::resources::{AssetRegistry, ProxyStatus, RegisteredAsset, StableIdRegistry};
use bevy_ecs::prelude::*;
use bevy_hierarchy::BuildChildren;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// DELETE ASSET COMMAND
// ============================================================================

/// Command to delete an asset from the project
///
/// This removes the asset from AssetRegistry and optionally deletes
/// the proxy folder from disk to save space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAssetCommand {
    /// UUID of the asset to delete
    pub asset_uuid: Uuid,
    /// Whether to also delete proxy files from disk
    pub delete_proxy_files: bool,
}

impl DeleteAssetCommand {
    /// Create a new delete asset command
    pub fn new(asset_uuid: Uuid) -> Self {
        Self {
            asset_uuid,
            delete_proxy_files: true,
        }
    }

    /// Create without deleting proxy files (for undo support)
    pub fn keep_proxy(asset_uuid: Uuid) -> Self {
        Self {
            asset_uuid,
            delete_proxy_files: false,
        }
    }
}

impl Command for DeleteAssetCommand {
    fn name(&self) -> &'static str {
        "Delete Asset"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Get the asset data before removing
        let serialized = {
            let registry = world.get_resource::<AssetRegistry>().ok_or_else(|| {
                CommandError::InvalidOperation("AssetRegistry not found".to_string())
            })?;

            let asset = registry.get(&self.asset_uuid).ok_or_else(|| {
                CommandError::EntityNotFound(format!("Asset {} not found", self.asset_uuid))
            })?;

            // Serialize for undo
            serde_json::to_vec(asset).map_err(|e| {
                CommandError::InvalidOperation(format!("Failed to serialize asset: {}", e))
            })?
        };

        // Get proxy path before deletion (if we need to delete proxy files)
        let proxy_path = if self.delete_proxy_files {
            let registry = world.get_resource::<AssetRegistry>().unwrap();
            registry
                .get(&self.asset_uuid)
                .and_then(|a| a.proxy_path.clone())
        } else {
            None
        };

        // Remove from registry
        {
            let mut registry = world.get_resource_mut::<AssetRegistry>().ok_or_else(|| {
                CommandError::InvalidOperation("AssetRegistry not found".to_string())
            })?;
            registry.remove(&self.asset_uuid);
        }

        // Delete proxy folder if requested
        if self.delete_proxy_files {
            if let Some(proxy_path) = proxy_path {
                // The proxy_path points to the directory containing the proxy
                if proxy_path.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&proxy_path) {
                        tracing::warn!("Failed to delete proxy folder {:?}: {}", proxy_path, e);
                        // Don't fail the command, just log the warning
                    } else {
                        tracing::info!("Deleted proxy folder: {:?}", proxy_path);
                    }
                }
            }
        }

        tracing::info!("Deleted asset {}", &self.asset_uuid.to_string()[..8]);

        Ok(UndoData::EntityDeleted {
            entity_bits: 0, // Not an entity, using 0 as placeholder
            serialized,
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::EntityDeleted { serialized, .. } = undo_data {
            // Deserialize the asset
            let asset: RegisteredAsset = serde_json::from_slice(&serialized).map_err(|e| {
                CommandError::InvalidOperation(format!("Failed to deserialize asset: {}", e))
            })?;

            // Re-register the asset
            let mut registry = world.get_resource_mut::<AssetRegistry>().ok_or_else(|| {
                CommandError::InvalidOperation("AssetRegistry not found".to_string())
            })?;

            registry.register(asset);
            tracing::info!("Restored asset {}", &self.asset_uuid.to_string()[..8]);
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data for DeleteAssetCommand".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// REGENERATE PROXY COMMAND
// ============================================================================

/// Command to regenerate proxy for an asset
///
/// Marks the asset's proxy status as NotStarted, which triggers
/// the proxy generation system to regenerate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerateProxyCommand {
    /// UUID of the asset to regenerate proxy for
    pub asset_uuid: Uuid,
    /// Previous status (for undo)
    #[serde(skip)]
    previous_status: Option<ProxyStatus>,
}

impl RegenerateProxyCommand {
    pub fn new(asset_uuid: Uuid) -> Self {
        Self {
            asset_uuid,
            previous_status: None,
        }
    }
}

impl Command for RegenerateProxyCommand {
    fn name(&self) -> &'static str {
        "Regenerate Proxy"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut registry = world
            .get_resource_mut::<AssetRegistry>()
            .ok_or_else(|| CommandError::InvalidOperation("AssetRegistry not found".to_string()))?;

        let asset = registry.get_mut(&self.asset_uuid).ok_or_else(|| {
            CommandError::EntityNotFound(format!("Asset {} not found", self.asset_uuid))
        })?;

        // Store previous status for undo
        self.previous_status = Some(asset.proxy_status.clone());

        // Clear existing proxy path and reset status
        asset.proxy_path = None;
        asset.proxy_status = ProxyStatus::NotStarted;

        tracing::info!(
            "Reset proxy status for asset {} - will regenerate",
            &self.asset_uuid.to_string()[..8]
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(previous_status) = self.previous_status.take() {
            let mut registry = world.get_resource_mut::<AssetRegistry>().ok_or_else(|| {
                CommandError::InvalidOperation("AssetRegistry not found".to_string())
            })?;

            if let Some(asset) = registry.get_mut(&self.asset_uuid) {
                asset.proxy_status = previous_status;
            }
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

// ============================================================================
// ADD ASSET TO TIMELINE COMMAND
// ============================================================================

/// Command to add an asset to the timeline as a clip
///
/// Creates a new Clip entity from the asset and adds it to the specified track.
/// Supports multiple insertion modes via `InsertionIntent`:
/// - `EndOfTrack`: Auto-sequence at end of existing clips (default)
/// - `AtPlayhead`: Insert at current playhead position
/// - `AtPosition(f64)`: Insert at exact position (drag-drop)
/// - `RippleAt(f64)`: Insert and push subsequent clips (Phase 2C)
///
/// If no timeline exists, one will be auto-created with a default video track.
/// All positions are snapped to frame boundaries for frame-accurate placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAssetToTimelineCommand {
    /// UUID of the asset to add
    pub asset_uuid: Uuid,
    /// Target track entity (optional, will find first video track if None)
    pub target_track_bits: Option<u64>,
    /// Insertion intent - determines where the clip is placed
    pub insertion_intent: crate::resources::InsertionIntent,
    /// UUID assigned to the created clip (for undo/redo)
    pub created_clip_uuid: Uuid,
    /// Cached resolved position (set during execute for logging)
    #[serde(skip)]
    resolved_position: Option<Duration>,
    /// Whether this command auto-created a timeline (for undo)
    #[serde(skip)]
    auto_created_timeline: bool,
}

impl AddAssetToTimelineCommand {
    /// Create with auto-placement at end of first video track
    ///
    /// This is the default behavior for double-click in MediaBrowser.
    pub fn new(asset_uuid: Uuid) -> Self {
        Self {
            asset_uuid,
            target_track_bits: None,
            insertion_intent: crate::resources::InsertionIntent::EndOfTrack,
            created_clip_uuid: Uuid::new_v4(),
            resolved_position: None,
            auto_created_timeline: false,
        }
    }

    /// Insert at current playhead position
    ///
    /// Used for Shift+double-click in MediaBrowser.
    pub fn at_playhead(asset_uuid: Uuid) -> Self {
        Self {
            asset_uuid,
            target_track_bits: None,
            insertion_intent: crate::resources::InsertionIntent::AtPlayhead,
            created_clip_uuid: Uuid::new_v4(),
            resolved_position: None,
            auto_created_timeline: false,
        }
    }

    /// Insert at exact timeline position (from drag-drop)
    ///
    /// The position will be snapped to frame boundaries.
    pub fn at_position(asset_uuid: Uuid, track_bits: u64, position_seconds: f64) -> Self {
        Self {
            asset_uuid,
            target_track_bits: Some(track_bits),
            insertion_intent: crate::resources::InsertionIntent::AtPosition(position_seconds),
            created_clip_uuid: Uuid::new_v4(),
            resolved_position: None,
            auto_created_timeline: false,
        }
    }

    /// Insert with ripple (push subsequent clips)
    ///
    /// Phase 2C feature for collision-aware insertion.
    pub fn ripple_at(asset_uuid: Uuid, track_bits: u64, position_seconds: f64) -> Self {
        Self {
            asset_uuid,
            target_track_bits: Some(track_bits),
            insertion_intent: crate::resources::InsertionIntent::RippleAt(position_seconds),
            created_clip_uuid: Uuid::new_v4(),
            resolved_position: None,
            auto_created_timeline: false,
        }
    }

    /// Set target track explicitly
    pub fn with_track(mut self, track_bits: u64) -> Self {
        self.target_track_bits = Some(track_bits);
        self
    }
}

impl Command for AddAssetToTimelineCommand {
    fn name(&self) -> &'static str {
        "Add to Timeline"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        use crate::components::Timeline;
        use crate::resources::{find_first_video_track, resolve_insertion_position, Playhead};

        // Get asset metadata
        let (asset_duration, asset_name, asset_frame_rate) = {
            let registry = world.get_resource::<AssetRegistry>().ok_or_else(|| {
                CommandError::InvalidOperation("AssetRegistry not found".to_string())
            })?;

            let asset = registry.get(&self.asset_uuid).ok_or_else(|| {
                CommandError::EntityNotFound(format!("Asset {} not found", self.asset_uuid))
            })?;

            (
                asset.metadata.duration,
                asset.metadata.name.clone(),
                asset.metadata.frame_rate,
            )
        };

        // Get frame rate for snapping (default to 30 if playhead exists, else use asset rate)
        let frame_rate: f64 = if world.get_resource::<Playhead>().is_some() {
            // TODO: Get from active timeline's frame rate when available
            30.0
        } else {
            asset_frame_rate
        };

        // Check if any timeline exists, create if not
        let has_timeline: bool = {
            let mut query = world.query::<&Timeline>();
            query.iter(world).next().is_some()
        };

        if !has_timeline {
            // Auto-create timeline with a default video track
            self.auto_created_timeline = true;
            self.create_default_timeline(world, asset_frame_rate)?;
            tracing::info!("Auto-created timeline for first clip");
        }

        // Find target track
        let track_entity: Entity = if let Some(bits) = self.target_track_bits {
            Entity::from_bits(bits)
        } else {
            find_first_video_track(world)
                .ok_or_else(|| CommandError::InvalidOperation("No video track found".to_string()))?
        };

        // Resolve insertion position with frame-accurate snapping
        let insert_position: Duration =
            resolve_insertion_position(world, &self.insertion_intent, track_entity, frame_rate);
        self.resolved_position = Some(insert_position);

        // ===================================================================
        // MULTI-TRACK PLACEMENT (CapCut/DaVinci-style AutoTrack)
        // ===================================================================
        // Each clip goes on its own track so the compositor can layer them.
        // - EndOfTrack mode: if track has clips, create new track at position 0
        // - Explicit position modes: use collision detection for overlap resolution
        let mut final_track_entity: Entity = track_entity;
        let mut final_insert_position: Duration = insert_position;

        // Check if target track already has clips
        let track_has_clips: bool = {
            let mut clip_query =
                world.query::<(&crate::components::TrackClip, &bevy_hierarchy::Parent)>();
            clip_query
                .iter(world)
                .any(|(_tc, parent)| parent.get() == track_entity)
        };

        if track_has_clips {
            // Track has clips - create a new track for this clip
            let timeline_entity: Option<Entity> = world
                .get::<bevy_hierarchy::Parent>(track_entity)
                .map(|p| p.get());

            if let Some(timeline) = timeline_entity {
                let new_track_index: u32 = world
                    .get::<bevy_hierarchy::Children>(timeline)
                    .map(|children| {
                        children
                            .iter()
                            .filter_map(|&e| world.get::<Track>(e))
                            .filter(|t| matches!(t.track_type, crate::components::TrackType::Video))
                            .count() as u32
                    })
                    .unwrap_or(0);

                let new_track_uuid: Uuid = Uuid::new_v4();
                let new_track: Entity = world
                    .spawn((
                        Track {
                            index: new_track_index,
                            track_type: crate::components::TrackType::Video,
                            name: format!("V{}", new_track_index + 1),
                            locked: false,
                            muted: false,
                            solo: false,
                            visible: true,
                        },
                        crate::components::StableId::from_uuid(new_track_uuid),
                    ))
                    .id();

                if let Some(mut registry) = world.get_resource_mut::<StableIdRegistry>() {
                    registry.register(new_track_uuid, new_track);
                }

                world.entity_mut(timeline).add_child(new_track);

                tracing::info!(
                    "Track has clips — created 'V{}' for new clip",
                    new_track_index + 1
                );

                final_track_entity = new_track;
                // For EndOfTrack mode, the resolved position was the OLD track's
                // end time — meaningless on a brand-new empty track, so reset to 0.
                // For all explicit-position intents (AtPlayhead, AtPosition,
                // RippleAt), preserve the resolved position so the clip starts
                // where the user expects.
                if self.insertion_intent.is_auto_sequence() {
                    final_insert_position = Duration::ZERO;
                }
            }
        }

        // Create clip entity with ClipAssetRef for video lookup.
        //
        // Transform2D::default() is REQUIRED for viewport gizmo rendering and
        // viewport click-to-select. Without it, the clip won't appear in
        // viewport queries and cannot be selected by clicking the viewport.
        let clip_entity = world
            .spawn((
                Clip {
                    name: asset_name.clone(),
                    source_in: Duration::ZERO,
                    source_out: asset_duration,
                    timeline_start: final_insert_position,
                },
                crate::components::ClipMetadata::default(),
                crate::components::Transform2D::default(),
                crate::components::TimelineChild,
                crate::components::TrackClip,
                crate::components::StableId::from_uuid(self.created_clip_uuid),
                crate::node::timeline::ClipAssetRef::new(self.asset_uuid, asset_name.clone()),
            ))
            .id();

        // Register with StableIdRegistry
        if let Some(mut registry) = world.get_resource_mut::<StableIdRegistry>() {
            registry.register(self.created_clip_uuid, clip_entity);
        }

        // Add as child of track (new track if one was created)
        world.entity_mut(final_track_entity).add_child(clip_entity);

        // Increment usage count
        if let Some(mut registry) = world.get_resource_mut::<AssetRegistry>() {
            if let Some(asset) = registry.get_mut(&self.asset_uuid) {
                asset.usage_count += 1;
            }
        }

        tracing::info!(
            "Added clip '{}' (asset {}) at {:.3}s on track '{}'",
            asset_name,
            &self.asset_uuid.to_string()[..8],
            final_insert_position.as_secs_f64(),
            if final_track_entity == track_entity {
                "V1"
            } else {
                "new"
            }
        );

        Ok(UndoData::EntityCreated(clip_entity.to_bits()))
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::EntityCreated(entity_bits) = undo_data {
            let entity = Entity::from_bits(entity_bits);

            // Decrement usage count
            if let Some(mut registry) = world.get_resource_mut::<AssetRegistry>() {
                if let Some(asset) = registry.get_mut(&self.asset_uuid) {
                    asset.usage_count = asset.usage_count.saturating_sub(1);
                }
            }

            // Unregister from StableIdRegistry
            if let Some(mut registry) = world.get_resource_mut::<StableIdRegistry>() {
                registry.park(&self.created_clip_uuid);
            }

            // Despawn the clip
            world.despawn(entity);

            // TODO: If we auto-created timeline, consider cleaning it up if empty

            tracing::info!(
                "Undid add to timeline for asset {}",
                &self.asset_uuid.to_string()[..8]
            );
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data for AddAssetToTimelineCommand".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AddAssetToTimelineCommand {
    /// Create a default timeline with a single video track
    ///
    /// Called automatically when adding first clip and no timeline exists.
    fn create_default_timeline(
        &self,
        world: &mut World,
        frame_rate: f64,
    ) -> Result<(), CommandError> {
        use crate::components::timeline::ActiveTimeline;
        use crate::components::{FrameRate, Timeline};

        // Determine FrameRate enum from f64
        let fr_enum: FrameRate = match frame_rate as u32 {
            24 => FrameRate::Fps24,
            25 => FrameRate::Fps25,
            30 => FrameRate::Fps30,
            50 => FrameRate::Fps50,
            60 => FrameRate::Fps60,
            _ => {
                let fps_rounded = (frame_rate * 1000.0).round() as u32;
                FrameRate::Custom(fps_rounded, 1000)
            }
        };

        // Create timeline entity using constructor (handles default fields)
        let timeline_entity = world
            .spawn((
                Timeline::new("Timeline 1", fr_enum),
                crate::components::StableId::new(),
            ))
            .id();

        // Create default video track with all required fields
        let track_entity = world
            .spawn((
                Track {
                    name: "V1".to_string(),
                    track_type: crate::components::TrackType::Video,
                    index: 0,
                    muted: false,
                    solo: false,
                    locked: false,
                    visible: true,
                },
                crate::components::StableId::new(),
            ))
            .id();

        // Add track as child of timeline
        world.entity_mut(timeline_entity).add_child(track_entity);

        // Set as active timeline
        if let Some(mut active) = world.get_resource_mut::<ActiveTimeline>() {
            active.set(timeline_entity);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_asset_command_creation() {
        let uuid = Uuid::new_v4();
        let cmd = DeleteAssetCommand::new(uuid);
        assert_eq!(cmd.asset_uuid, uuid);
        assert!(cmd.delete_proxy_files);
    }

    #[test]
    fn test_regenerate_proxy_command_creation() {
        let uuid = Uuid::new_v4();
        let cmd = RegenerateProxyCommand::new(uuid);
        assert_eq!(cmd.asset_uuid, uuid);
    }

    #[test]
    fn test_add_asset_to_timeline_command_creation() {
        let uuid = Uuid::new_v4();
        let cmd = AddAssetToTimelineCommand::new(uuid);
        assert_eq!(cmd.asset_uuid, uuid);
        assert!(cmd.target_track_bits.is_none());
        assert!(cmd.insertion_intent.is_auto_sequence());
    }
}
