//! # Add Clip Command
//!
//! Adds a clip to a track with collision detection (CapCut/DaVinci-style AutoTrack).

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, DespawnRecursiveExt, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{
    Clip, ClipMetadata, StableId, TimelineChild, Track, TrackClip, TrackDisplay, TrackType,
    Transform2D,
};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// ADD CLIP COMMAND
// ============================================================================

/// Add a clip to a track at a specific timeline position.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "add_clip",
///   "track": 12345,
///   "asset": 67890,
///   "at": 5.0,
///   "name": "Interview.mp4"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddClipCommand {
    /// Track entity to add clip to
    pub track: u64,
    /// Source asset entity (for metadata lookup)
    pub asset: u64,
    /// Timeline position in seconds
    pub at: f64,
    /// Optional clip name (defaults to asset filename)
    #[serde(default)]
    pub name: Option<String>,
    /// Optional source in point in seconds
    #[serde(default)]
    pub source_in: Option<f64>,
    /// Optional source out point in seconds
    #[serde(default)]
    pub source_out: Option<f64>,
    /// UUID of the video asset in VideoAssetManager (for compositor)
    #[serde(default)]
    pub asset_uuid: Option<uuid::Uuid>,
    /// Filename for display (used with asset_uuid)
    #[serde(default)]
    pub filename: Option<String>,
    /// If true, automatically place clip at end of last clip on track (auto-sequence)
    /// This calculates the actual position at execute time, ignoring the `at` field.
    #[serde(default)]
    pub auto_sequence: bool,

    /// Stored entity for undo
    #[serde(skip)]
    created_entity: Option<u64>,
    /// Stable UUID for the created clip - persists across undo/redo
    #[serde(skip)]
    stable_id: Option<uuid::Uuid>,

    /// UUID of the track auto-created by collision resolution (AutoTrack mode).
    /// Persists across undo/redo so we can reuse the same track identity on redo
    /// and clean it up on undo.
    #[serde(skip)]
    auto_created_track_uuid: Option<uuid::Uuid>,
    /// Entity bits of the auto-created track (stale after undo, resolved via UUID on redo)
    #[serde(skip)]
    auto_created_track_entity: Option<u64>,
}

impl AddClipCommand {
    /// Create a new add clip command
    pub fn new(track: Entity, asset: Entity, at: Duration) -> Self {
        Self {
            track: track.to_bits(),
            asset: asset.to_bits(),
            at: at.as_secs_f64(),
            name: None,
            source_in: None,
            source_out: None,
            asset_uuid: None,
            filename: None,
            auto_sequence: false,
            created_entity: None,
            stable_id: None,
            auto_created_track_uuid: None,
            auto_created_track_entity: None,
        }
    }

    /// Enable auto-sequencing (place clip at end of track content)
    ///
    /// When enabled, the `at` position is ignored and the clip is placed
    /// immediately after the last clip on the track.
    pub fn with_auto_sequence(mut self, enabled: bool) -> Self {
        self.auto_sequence = enabled;
        self
    }

    /// Set clip name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set source in/out points
    pub fn with_source_range(mut self, source_in: Duration, source_out: Duration) -> Self {
        self.source_in = Some(source_in.as_secs_f64());
        self.source_out = Some(source_out.as_secs_f64());
        self
    }

    /// Set the video asset UUID for compositor integration
    pub fn with_asset_uuid(mut self, asset_uuid: uuid::Uuid, filename: impl Into<String>) -> Self {
        self.asset_uuid = Some(asset_uuid);
        self.filename = Some(filename.into());
        self
    }
}

impl Command for AddClipCommand {
    fn name(&self) -> &'static str {
        "Add Clip"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let track_entity = Entity::from_bits(self.track);
        let asset_entity = Entity::from_bits(self.asset);

        // Verify track exists
        if world.get::<Track>(track_entity).is_none() {
            return Err(CommandError::EntityNotFound(format!(
                "Track {} not found",
                self.track
            )));
        }

        // Calculate actual timeline position:
        // If auto_sequence is enabled, place clip at end of last clip on track.
        // CRITICAL: Store the resolved position back in self.at so that redo
        // uses the exact same position instead of re-calculating (which would
        // produce a different value if clips have changed).
        let actual_position_secs: f64 = if self.auto_sequence {
            if self.stable_id.is_some() {
                // Redo path: stable_id was set on first execute, use stored position
                self.at
            } else {
                // First execute: resolve and store
                let track_end = crate::resources::calculate_track_end_time(world, track_entity);
                let resolved: f64 = track_end.as_secs_f64();
                tracing::debug!(
                    "Auto-sequence: placing clip at track end ({:.2}s)",
                    resolved
                );
                self.at = resolved; // Pin for redo
                resolved
            }
        } else {
            self.at
        };

        // Determine clip name (from parameter or asset)
        let clip_name = self.name.clone().unwrap_or_else(|| {
            // Try to get name from video/audio asset path
            if let Some(video) = world.get::<crate::components::VideoAsset>(asset_entity) {
                video
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            } else if let Some(audio) = world.get::<crate::components::AudioAsset>(asset_entity) {
                audio
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            } else {
                "Untitled".to_string()
            }
        });

        // Determine source duration from asset
        let asset_duration = world
            .get::<crate::components::VideoAsset>(asset_entity)
            .map(|v| v.duration)
            .or_else(|| {
                world
                    .get::<crate::components::AudioAsset>(asset_entity)
                    .map(|a| a.duration)
            })
            .unwrap_or(Duration::from_secs(10));

        // Determine source in/out (default to full asset)
        let source_in = self
            .source_in
            .map(Duration::from_secs_f64)
            .unwrap_or(Duration::ZERO);
        let source_out = self
            .source_out
            .map(Duration::from_secs_f64)
            .unwrap_or(asset_duration);

        // Calculate clip duration for collision detection
        let clip_duration: Duration = source_out.saturating_sub(source_in);
        let timeline_start: Duration = Duration::from_secs_f64(actual_position_secs);

        // ====================================================================
        // COLLISION DETECTION (CapCut/DaVinci-style AutoTrack)
        // ====================================================================
        // Check if placing this clip would overlap existing clips.
        // If collision detected with AutoTrack mode, create a new track.
        //
        // On REDO: if we previously created a track for collision resolution,
        // we recreate it with the SAME UUID to maintain stable identity.
        // This prevents the "rows keep multiplying" bug.
        let mut final_track_entity: Entity = track_entity;

        if let Some(auto_track_uuid) = self.auto_created_track_uuid {
            // ================================================================
            // REDO PATH: Re-create the auto-track with the same UUID
            // ================================================================
            // We already know from the first execute that a collision occurred
            // and a track was created. Recreate it with the same identity.
            let timeline_entity: Option<Entity> =
                world.get::<Parent>(track_entity).map(|p| p.get());

            if let Some(timeline) = timeline_entity {
                let new_track_index: u32 = world
                    .get::<bevy_hierarchy::Children>(timeline)
                    .map(|children| {
                        children
                            .iter()
                            .filter_map(|&e| world.get::<Track>(e))
                            .filter(|t| matches!(t.track_type, TrackType::Video))
                            .count() as u32
                    })
                    .unwrap_or(0);

                let new_track: Entity = world
                    .spawn((
                        Track {
                            index: new_track_index,
                            track_type: TrackType::Video,
                            name: format!("V{}", new_track_index + 1),
                            locked: false,
                            muted: false,
                            solo: false,
                            visible: true,
                        },
                        TrackDisplay::default(),
                        TimelineChild,
                        StableId::from_uuid(auto_track_uuid),
                    ))
                    .id();

                world
                    .resource_mut::<StableIdRegistry>()
                    .register(auto_track_uuid, new_track);

                world.entity_mut(timeline).add_child(new_track);
                self.auto_created_track_entity = Some(new_track.to_bits());
                final_track_entity = new_track;

                tracing::debug!(
                    "Redo: re-created auto-track 'V{}' (UUID: {}) for collision resolution",
                    new_track_index + 1,
                    &auto_track_uuid.to_string()[..8]
                );
            }
        } else {
            // ================================================================
            // FIRST EXECUTE PATH: Run collision detection normally
            // ================================================================
            let track_uuid_opt: Option<uuid::Uuid> =
                world.get::<StableId>(track_entity).map(|s| s.uuid());

            if let Some(track_uuid) = track_uuid_opt {
                if let Some(collision) = crate::systems::check_collision(
                    world,
                    track_uuid,
                    timeline_start,
                    clip_duration,
                    None, // Not excluding any clip (this is a new clip)
                ) {
                    use crate::resources::collision::CollisionAction;

                    match collision.suggested_action {
                        CollisionAction::CreateTrack { .. } => {
                            // AutoTrack mode: Create a new track for this clip
                            tracing::info!(
                                "Collision detected on track. Creating new track (AutoTrack mode)."
                            );

                            let timeline_entity: Option<Entity> =
                                world.get::<Parent>(track_entity).map(|p| p.get());

                            if let Some(timeline) = timeline_entity {
                                let new_track_index: u32 = world
                                    .get::<bevy_hierarchy::Children>(timeline)
                                    .map(|children| {
                                        children
                                            .iter()
                                            .filter_map(|&e| world.get::<Track>(e))
                                            .filter(|t| matches!(t.track_type, TrackType::Video))
                                            .count() as u32
                                    })
                                    .unwrap_or(0);

                                // Create new track and persist its UUID for undo/redo
                                let new_track_uuid: uuid::Uuid = uuid::Uuid::new_v4();
                                let new_track: Entity = world
                                    .spawn((
                                        Track {
                                            index: new_track_index,
                                            track_type: TrackType::Video,
                                            name: format!("V{}", new_track_index + 1),
                                            locked: false,
                                            muted: false,
                                            solo: false,
                                            visible: true,
                                        },
                                        TrackDisplay::default(),
                                        TimelineChild,
                                        StableId::from_uuid(new_track_uuid),
                                    ))
                                    .id();

                                world
                                    .resource_mut::<StableIdRegistry>()
                                    .register(new_track_uuid, new_track);

                                world.entity_mut(timeline).add_child(new_track);

                                // Store auto-created track info for undo/redo
                                self.auto_created_track_uuid = Some(new_track_uuid);
                                self.auto_created_track_entity = Some(new_track.to_bits());

                                tracing::debug!(
                                    "Created new track 'V{}' (UUID: {}) for collision resolution",
                                    new_track_index + 1,
                                    &new_track_uuid.to_string()[..8]
                                );

                                final_track_entity = new_track;
                            }
                        }
                        CollisionAction::Reject { reason } => {
                            return Err(CommandError::InvalidOperation(format!(
                                "Cannot add clip: {}",
                                reason
                            )));
                        }
                        _ => {
                            // Ripple/Overwrite: Phase 2C - for now, proceed normally
                            tracing::trace!("Collision mode not AutoTrack/Reject, proceeding");
                        }
                    }
                }
            }
        }

        // Get or create stable ID for this clip
        // On first execute: generate new UUID
        // On redo: reuse the stored UUID
        let stable_uuid = self.stable_id.unwrap_or_else(|| {
            let new_uuid = uuid::Uuid::new_v4();
            self.stable_id = Some(new_uuid);
            new_uuid
        });

        // Create clip entity with StableId + Transform2D for viewport gizmo.
        //
        // Transform2D::default() uses position=(0,0), anchor=(0.5,0.5), scale=(1,1).
        // With the to_matrix() math (T*R*S*T decomposition), when scale=1 and
        // rotation=0, a point P maps to P + position. So position=(0,0) means
        // the source rectangle fills (0,0)→(W,H) — exactly the canvas bounds
        // when source_size = canvas_size.
        let clip_entity = world
            .spawn((
                Clip {
                    name: clip_name.clone(),
                    source_in,
                    source_out,
                    timeline_start: Duration::from_secs_f64(actual_position_secs),
                },
                ClipMetadata::default(),
                Transform2D::default(),
                TimelineChild,
                TrackClip,
                StableId::from_uuid(stable_uuid),
            ))
            .id();

        // Add ClipAssetRef if asset_uuid is provided (for compositor integration)
        if let Some(asset_uuid) = self.asset_uuid {
            let filename = self.filename.clone().unwrap_or_else(|| clip_name.clone());
            world
                .entity_mut(clip_entity)
                .insert(ClipAssetRef::new(asset_uuid, filename));
        }

        // Register in StableIdRegistry for UUID-based lookups
        world
            .resource_mut::<StableIdRegistry>()
            .register(stable_uuid, clip_entity);

        // Add as child of track (may be new track if collision was resolved)
        world.entity_mut(final_track_entity).add_child(clip_entity);

        self.created_entity = Some(clip_entity.to_bits());

        tracing::debug!(
            "Added clip '{}' (UUID: {}) to track at {:.2}s",
            clip_name,
            &stable_uuid.to_string()[..8],
            self.at
        );

        Ok(UndoData::EntityCreated(clip_entity.to_bits()))
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(bits) = self.created_entity {
            let entity = Entity::from_bits(bits);

            // Park UUID in registry (marks as inactive but preserves mapping)
            if let Some(uuid) = self.stable_id {
                world.resource_mut::<StableIdRegistry>().park(&uuid);
            }

            world.entity_mut(entity).despawn_recursive();
            self.created_entity = None;
            // NOTE: We do NOT clear stable_id - it's needed for redo
        }

        // Also clean up the auto-created track from collision resolution.
        // Without this, undo leaves an orphaned empty track row behind,
        // and redo would create yet another new track each time.
        if let Some(track_uuid) = self.auto_created_track_uuid {
            if let Some(track_entity) = world.resource::<StableIdRegistry>().get(&track_uuid) {
                world.resource_mut::<StableIdRegistry>().park(&track_uuid);
                world.entity_mut(track_entity).despawn_recursive();
                tracing::debug!(
                    "Undo: removed auto-created track (UUID: {})",
                    &track_uuid.to_string()[..8]
                );
            }
            self.auto_created_track_entity = None;
            // NOTE: Keep auto_created_track_uuid — needed for redo to recreate with same identity
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

impl_serializable_command!(AddClipCommand, "add_clip");
