//! # Timeline Commands
//!
//! AI-invocable commands for timeline manipulation.
//!
//! All commands follow the Command Pattern and are:
//! - Undoable/Redoable
//! - Serializable (for project files and AI)
//! - AI-invocable via JSON schema
//!
//! ## Command Categories
//!
//! - **Timeline lifecycle**: Create, delete timelines
//! - **Track operations**: Add, remove, reorder tracks
//! - **Clip operations**: Add, move, delete clips
//!
//! ## Entity Relationships
//!
//! Commands use `bevy_hierarchy` for parent-child relationships:
//! - Tracks are children of Timeline
//! - Clips are children of Track
//!
//! This means despawning a Timeline will cascade-despawn all tracks and clips.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Children, DespawnRecursiveExt, Parent};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

use super::{Command, CommandError, UndoData};
use crate::components::{
    ActiveTimeline, Clip, ClipMetadata, FrameRate, StableId, Timeline, TimelineChild,
    TimelineState, Track, TrackClip, TrackDisplay, TrackType, Transform2D,
};
use crate::impl_serializable_command;
use crate::node::timeline::clip::ClipAssetRef;
use crate::resources::StableIdRegistry;

// ============================================================================
// CREATE TIMELINE COMMAND
// ============================================================================

/// Create a new timeline.
///
/// Creates a timeline entity with default tracks (2 video, 2 audio).
/// The new timeline becomes the active timeline.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "create_timeline",
///   "name": "My Sequence",
///   "frame_rate": "Fps30"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTimelineCommand {
    /// Timeline name
    pub name: String,
    /// Frame rate for the timeline
    pub frame_rate: FrameRate,
    /// Number of default video tracks to create (default: 2)
    #[serde(default = "default_video_tracks")]
    pub video_tracks: u32,
    /// Number of default audio tracks to create (default: 2)
    #[serde(default = "default_audio_tracks")]
    pub audio_tracks: u32,

    /// Stored entity for undo
    #[serde(skip)]
    created_entity: Option<u64>,
    /// Stored track entities for undo
    #[serde(skip)]
    created_tracks: Vec<u64>,
}

fn default_video_tracks() -> u32 {
    1 // CapCut style: at least 1 track for content
}
fn default_audio_tracks() -> u32 {
    0 // Audio tracks created on-demand
}

impl CreateTimelineCommand {
    /// Create a new timeline command with the given name and frame rate
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        Self {
            name: name.into(),
            frame_rate,
            video_tracks: 1, // CapCut style: 1 track for initial content
            audio_tracks: 0,
            created_entity: None,
            created_tracks: Vec::new(),
        }
    }

    /// Set the number of video tracks
    pub fn with_video_tracks(mut self, count: u32) -> Self {
        self.video_tracks = count;
        self
    }

    /// Set the number of audio tracks
    pub fn with_audio_tracks(mut self, count: u32) -> Self {
        self.audio_tracks = count;
        self
    }
}

impl Command for CreateTimelineCommand {
    fn name(&self) -> &'static str {
        "Create Timeline"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Create timeline entity
        let timeline_entity = world
            .spawn((
                Timeline::new(self.name.clone(), self.frame_rate.clone()),
                TimelineState::empty(),
            ))
            .id();

        self.created_entity = Some(timeline_entity.to_bits());
        self.created_tracks.clear();

        // Create video tracks
        for i in 0..self.video_tracks {
            let track_uuid = uuid::Uuid::new_v4();
            let track_entity = world
                .spawn((
                    Track {
                        index: i,
                        track_type: TrackType::Video,
                        name: format!("V{}", i + 1),
                        locked: false,
                        muted: false,
                        solo: false,
                        visible: true,
                    },
                    TrackDisplay::default(),
                    TimelineChild,
                    StableId::from_uuid(track_uuid),
                ))
                .id();

            world
                .resource_mut::<StableIdRegistry>()
                .register(track_uuid, track_entity);

            world.entity_mut(timeline_entity).add_child(track_entity);
            self.created_tracks.push(track_entity.to_bits());
        }

        // Create audio tracks (placed below video tracks in index)
        for i in 0..self.audio_tracks {
            let track_uuid = uuid::Uuid::new_v4();
            let track_entity = world
                .spawn((
                    Track {
                        index: self.video_tracks + i,
                        track_type: TrackType::Audio,
                        name: format!("A{}", i + 1),
                        locked: false,
                        muted: false,
                        solo: false,
                        visible: true,
                    },
                    TrackDisplay::with_height(40.0), // Audio tracks are shorter
                    TimelineChild,
                    StableId::from_uuid(track_uuid),
                ))
                .id();

            world
                .resource_mut::<StableIdRegistry>()
                .register(track_uuid, track_entity);

            world.entity_mut(timeline_entity).add_child(track_entity);
            self.created_tracks.push(track_entity.to_bits());
        }

        // Set as active timeline
        if let Some(mut active) = world.get_resource_mut::<ActiveTimeline>() {
            active.set(timeline_entity);
        }

        tracing::info!(
            "Created timeline '{}' with {} video and {} audio tracks",
            self.name,
            self.video_tracks,
            self.audio_tracks
        );

        Ok(UndoData::EntityCreated(timeline_entity.to_bits()))
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(bits) = self.created_entity {
            let entity = Entity::from_bits(bits);

            // Check if this was the active timeline
            let was_active = world
                .get_resource::<ActiveTimeline>()
                .map(|a| a.get() == Some(entity))
                .unwrap_or(false);

            // Despawn recursively (takes tracks and clips with it)
            world.entity_mut(entity).despawn_recursive();

            // Clear active timeline if this was it
            if was_active {
                if let Some(mut active) = world.get_resource_mut::<ActiveTimeline>() {
                    active.clear();
                }
            }

            self.created_entity = None;
            self.created_tracks.clear();
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

impl_serializable_command!(CreateTimelineCommand, "create_timeline");

// ============================================================================
// CREATE TRACK COMMAND
// ============================================================================

/// Add a new track to a timeline.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "create_track",
///   "timeline": 12345,
///   "track_type": "Video",
///   "name": "V3"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrackCommand {
    /// Timeline entity to add track to (as bits)
    pub timeline: u64,
    /// Type of track
    pub track_type: TrackType,
    /// Track name
    pub name: String,
    /// Optional: insertion index (None = append at end)
    #[serde(default)]
    pub index: Option<u32>,

    /// Stored entity for undo
    #[serde(skip)]
    created_entity: Option<u64>,
    /// Stable UUID for the created track
    #[serde(skip)]
    stable_id: Option<uuid::Uuid>,
}

impl CreateTrackCommand {
    /// Create a new track command
    pub fn new(timeline: Entity, track_type: TrackType, name: impl Into<String>) -> Self {
        Self {
            timeline: timeline.to_bits(),
            track_type,
            name: name.into(),
            index: None,
            created_entity: None,
            stable_id: None,
        }
    }

    /// Set insertion index
    pub fn at_index(mut self, index: u32) -> Self {
        self.index = Some(index);
        self
    }
}

impl Command for CreateTrackCommand {
    fn name(&self) -> &'static str {
        "Create Track"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let timeline_entity = Entity::from_bits(self.timeline);

        // Verify timeline exists
        if world.get::<Timeline>(timeline_entity).is_none() {
            return Err(CommandError::EntityNotFound(format!(
                "Timeline {} not found",
                self.timeline
            )));
        }

        // Count existing tracks to determine index
        let track_index = if let Some(idx) = self.index {
            idx
        } else {
            // Count children that are tracks
            let child_count = world
                .get::<Children>(timeline_entity)
                .map(|c| {
                    c.iter()
                        .filter(|e| world.get::<Track>(**e).is_some())
                        .count() as u32
                })
                .unwrap_or(0);
            child_count
        };

        // Get or create stable ID
        let stable_uuid = self.stable_id.unwrap_or_else(|| {
            let new_uuid = uuid::Uuid::new_v4();
            self.stable_id = Some(new_uuid);
            new_uuid
        });

        // Create track entity
        let track_entity = world
            .spawn((
                Track {
                    index: track_index,
                    track_type: self.track_type,
                    name: self.name.clone(),
                    locked: false,
                    muted: false,
                    solo: false,
                    visible: true,
                },
                TrackDisplay::default(),
                TimelineChild,
                StableId::from_uuid(stable_uuid),
            ))
            .id();

        world
            .resource_mut::<StableIdRegistry>()
            .register(stable_uuid, track_entity);

        // Add as child of timeline
        world.entity_mut(timeline_entity).add_child(track_entity);

        self.created_entity = Some(track_entity.to_bits());

        tracing::debug!(
            "Created {:?} track '{}' at index {}",
            self.track_type,
            self.name,
            track_index
        );

        Ok(UndoData::EntityCreated(track_entity.to_bits()))
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(bits) = self.created_entity {
            let entity = Entity::from_bits(bits);

            if let Some(uuid) = self.stable_id {
                world.resource_mut::<StableIdRegistry>().park(&uuid);
            }

            world.entity_mut(entity).despawn_recursive();
            self.created_entity = None;
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

impl_serializable_command!(CreateTrackCommand, "create_track");

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
    created_clip: Option<u64>,
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

// ============================================================================
// SLIDE CLIP COMMAND
// ============================================================================

/// Slide a clip to a new position while adjusting the boundaries of neighboring clips.
///
/// ## How it works (Clean-room implementation)
///
/// Sliding moves a clip along the timeline while maintaining the overall timeline
/// coverage. Adjacent clips expand or contract to fill the space:
///
/// - The previous clip (if any) extends/shrinks its out point
/// - The next clip (if any) shifts its in point and timeline_start
/// - The total timeline duration is preserved
///
/// Example: Clips A|B|C with B slid right by 1s:
/// - A extends by 1s (source_out increases)
/// - B moves right by 1s (timeline_start increases)
/// - C shifts right by 1s (timeline_start and source_in adjust)
///
/// ## AI Schema
/// ```json
/// {
///   "command": "slide_clip",
///   "clip": 12345,
///   "delta": 2.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideClipCommand {
    /// Clip entity to slide (as bits)
    pub clip: u64,
    /// Time delta in seconds (positive = move right/later, negative = move left/earlier)
    pub delta: f64,

    /// Original timeline_start of the slid clip for undo
    #[serde(skip)]
    original_position: Option<f64>,
    /// Previous clip entity that was adjusted
    #[serde(skip)]
    previous_clip: Option<u64>,
    /// Original source_out of previous clip for undo
    #[serde(skip)]
    previous_source_out: Option<Duration>,
    /// Next clip entity that was adjusted
    #[serde(skip)]
    next_clip: Option<u64>,
    /// Original timeline_start of next clip for undo
    #[serde(skip)]
    next_timeline_start: Option<Duration>,
    /// Original source_in of next clip for undo
    #[serde(skip)]
    next_source_in: Option<Duration>,

    // =========================================================================
    // UUID Support
    // =========================================================================
    /// UUID of the clip
    #[serde(skip)]
    clip_uuid: Option<uuid::Uuid>,
    /// UUID of the previous clip
    #[serde(skip)]
    previous_clip_uuid: Option<uuid::Uuid>,
    /// UUID of the next clip
    #[serde(skip)]
    next_clip_uuid: Option<uuid::Uuid>,
}

impl SlideClipCommand {
    /// Create a new slide command
    pub fn new(clip: Entity, delta: Duration) -> Self {
        Self {
            clip: clip.to_bits(),
            delta: delta.as_secs_f64(),
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

    /// Create a slide command with signed delta
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

        // Get clip data
        let clip = world
            .get::<Clip>(clip_entity)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!("Clip {:?} not found", clip_entity))
            })?
            .clone();

        self.original_position = Some(clip.timeline_start.as_secs_f64());

        // Get parent track
        let parent_track = world
            .get::<Parent>(clip_entity)
            .ok_or_else(|| CommandError::InvalidOperation("Clip has no parent track".to_string()))?
            .get();

        // Get all sibling clips on the same track
        let track_children: Vec<Entity> = world
            .get::<Children>(parent_track)
            .map(|c| c.iter().copied().collect())
            .unwrap_or_default();

        // Find clips and sort by timeline position
        let mut clips_with_times: Vec<(Entity, Duration, Duration)> = Vec::new();
        for &child in &track_children {
            if let Some(child_clip) = world.get::<Clip>(child) {
                clips_with_times.push((
                    child,
                    child_clip.timeline_start,
                    child_clip.timeline_end(),
                ));
            }
        }
        clips_with_times.sort_by_key(|(_, start, _)| *start);

        // Find our clip's index
        let our_index = clips_with_times
            .iter()
            .position(|(e, _, _)| *e == clip_entity)
            .ok_or_else(|| CommandError::InvalidOperation("Clip not found in track".to_string()))?;

        let delta_duration = Duration::from_secs_f64(self.delta.abs());
        let moving_right = self.delta > 0.0;

        // Find previous and next clips (if any)
        let previous = if our_index > 0 {
            Some(clips_with_times[our_index - 1].0)
        } else {
            None
        };

        let next = if our_index + 1 < clips_with_times.len() {
            Some(clips_with_times[our_index + 1].0)
        } else {
            None
        };

        // Validation: Check if slide is possible
        if moving_right {
            // Moving right: need space from next clip or it must be able to contract
            if let Some(next_entity) = next {
                let next_clip = world.get::<Clip>(next_entity).unwrap();
                let next_duration = next_clip.duration();
                if delta_duration >= next_duration {
                    return Err(CommandError::ValidationFailed(
                        "Cannot slide: would eliminate next clip entirely".to_string(),
                    ));
                }
            }
        } else {
            // Moving left: need space from previous clip or it must be able to contract
            if let Some(prev_entity) = previous {
                let prev_clip = world.get::<Clip>(prev_entity).unwrap();
                let prev_duration = prev_clip.duration();
                if delta_duration >= prev_duration {
                    return Err(CommandError::ValidationFailed(
                        "Cannot slide: would eliminate previous clip entirely".to_string(),
                    ));
                }
            } else {
                // No previous clip - check if we'd go negative
                if delta_duration > clip.timeline_start {
                    return Err(CommandError::ValidationFailed(
                        "Cannot slide: would result in negative timeline position".to_string(),
                    ));
                }
            }
        }

        // Apply the slide

        // 1. Adjust previous clip (if present)
        if let Some(prev_entity) = previous {
            let prev_clip = world.get::<Clip>(prev_entity).unwrap().clone();
            self.previous_clip = Some(prev_entity.to_bits());
            self.previous_source_out = Some(prev_clip.source_out);

            // Store previous clip UUID
            if let Some(stable_id) = world.get::<StableId>(prev_entity) {
                self.previous_clip_uuid = Some(stable_id.uuid());
            }

            if let Some(mut prev_clip_mut) = world.get_mut::<Clip>(prev_entity) {
                if moving_right {
                    // Extend previous clip
                    prev_clip_mut.source_out = prev_clip.source_out + delta_duration;
                } else {
                    // Contract previous clip
                    prev_clip_mut.source_out = prev_clip.source_out.saturating_sub(delta_duration);
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

// ============================================================================
// RAZOR TOOL COMMAND (Multi-track split)
// ============================================================================

/// Split result for a single clip during razor operation
#[derive(Debug, Clone)]
struct RazorSplitResult {
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
    split_results: Vec<RazorSplitResult>,
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    fn setup_world() -> World {
        let mut world = World::new();
        world.insert_resource(ActiveTimeline::none());
        world.insert_resource(StableIdRegistry::default());
        world
    }

    #[test]
    fn test_create_timeline() {
        let mut world = setup_world();

        let mut cmd = CreateTimelineCommand::new("Test Timeline", FrameRate::Fps30);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify timeline was created
        let timeline_entity = Entity::from_bits(cmd.created_entity.unwrap());
        assert!(world.get::<Timeline>(timeline_entity).is_some());

        // Verify tracks were created (1 video track - CapCut style)
        let children = world.get::<Children>(timeline_entity).unwrap();
        assert_eq!(children.len(), 1);

        // Verify this is now the active timeline
        let active = world.resource::<ActiveTimeline>();
        assert_eq!(active.get(), Some(timeline_entity));
    }

    #[test]
    fn test_create_timeline_undo() {
        let mut world = setup_world();

        let mut cmd = CreateTimelineCommand::new("Test Timeline", FrameRate::Fps30);
        let undo_data = cmd.execute(&mut world).unwrap();
        let timeline_entity = Entity::from_bits(cmd.created_entity.unwrap());

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify timeline no longer exists
        assert!(world.get_entity(timeline_entity).is_err());

        // Verify active timeline was cleared
        let active = world.resource::<ActiveTimeline>();
        assert!(active.is_none());
    }

    #[test]
    fn test_create_track() {
        let mut world = setup_world();

        // First create a timeline
        let timeline = world
            .spawn((Timeline::default(), TimelineState::empty()))
            .id();

        // Create a new track
        let mut cmd = CreateTrackCommand::new(timeline, TrackType::Video, "V3");
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify track was created and parented
        let track_entity = Entity::from_bits(cmd.created_entity.unwrap());
        let parent = world.get::<Parent>(track_entity);
        assert_eq!(parent.unwrap().get(), timeline);
    }

    #[test]
    fn test_move_clip() {
        let mut world = setup_world();

        // Create track
        let track = world.spawn(Track::default()).id();

        // Create clip at position 0
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Move to 5 seconds
        let mut cmd = MoveClipCommand::to_time(clip, Duration::from_secs(5));
        cmd.execute(&mut world).unwrap();

        // Verify position changed
        let clip_comp = world.get::<Clip>(clip).unwrap();
        assert_eq!(clip_comp.timeline_start, Duration::from_secs(5));
    }

    #[test]
    fn test_delete_clip() {
        let mut world = setup_world();

        // Create track with clip
        let track = world.spawn(Track::default()).id();
        let clip = world.spawn((Clip::default(), ClipMetadata::default())).id();
        world.entity_mut(track).add_child(clip);

        // Delete clip
        let mut cmd = DeleteClipCommand::new(clip);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify clip is gone
        assert!(world.get_entity(clip).is_err());

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // We can't verify the exact entity exists (it's a new one), but
        // we can verify the track has a child again
        let children = world.get::<Children>(track).unwrap();
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_split_clip() {
        let mut world = setup_world();

        // Create track
        let track = world.spawn(Track::default()).id();

        // Create clip: timeline 0-10s, source 0-10s
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Split at 5 seconds
        let mut cmd = SplitClipCommand::new(clip, Duration::from_secs(5));
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify original clip now ends at 5s
        let original_clip = world.get::<Clip>(clip).unwrap();
        assert_eq!(original_clip.timeline_start, Duration::ZERO);
        assert_eq!(original_clip.source_in, Duration::ZERO);
        assert_eq!(original_clip.source_out, Duration::from_secs(5));
        assert_eq!(original_clip.duration(), Duration::from_secs(5));

        // Verify new clip was created
        let new_clip_entity = Entity::from_bits(cmd.created_clip.unwrap());
        let new_clip = world.get::<Clip>(new_clip_entity).unwrap();
        assert_eq!(new_clip.timeline_start, Duration::from_secs(5));
        assert_eq!(new_clip.source_in, Duration::from_secs(5));
        assert_eq!(new_clip.source_out, Duration::from_secs(10));
        assert_eq!(new_clip.duration(), Duration::from_secs(5));

        // Verify track has 2 children now
        let children = world.get::<Children>(track).unwrap();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_split_clip_invalid_position_before() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::from_secs(5), // Starts at 5s
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Try to split before clip starts - should fail
        let mut cmd = SplitClipCommand::new(clip, Duration::from_secs(2));
        let result = cmd.execute(&mut world);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_clip_invalid_position_after() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Try to split after clip ends - should fail
        let mut cmd = SplitClipCommand::new(clip, Duration::from_secs(15));
        let result = cmd.execute(&mut world);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_clip_undo() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Split at 5 seconds
        let mut cmd = SplitClipCommand::new(clip, Duration::from_secs(5));
        let undo_data = cmd.execute(&mut world).unwrap();
        let new_clip_entity = Entity::from_bits(cmd.created_clip.unwrap());

        // Verify split happened
        assert_eq!(world.get::<Children>(track).unwrap().len(), 2);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify original clip restored
        let original_clip = world.get::<Clip>(clip).unwrap();
        assert_eq!(original_clip.source_out, Duration::from_secs(10));
        assert_eq!(original_clip.duration(), Duration::from_secs(10));

        // Verify new clip was removed
        assert!(world.get_entity(new_clip_entity).is_err());

        // Verify track has 1 child again
        let children = world.get::<Children>(track).unwrap();
        assert_eq!(children.len(), 1);
    }

    // ========================================================================
    // TRIM CLIP TESTS
    // ========================================================================

    #[test]
    fn test_trim_clip_in() {
        let mut world = setup_world();

        // Create a clip: timeline 0-10s, source 0-10s
        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Trim in from 0s to 3s (move left edge right)
        let mut cmd = TrimClipCommand::trim_in(clip, Duration::from_secs(3));
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify: timeline_start = 3s, source_in = 3s, duration = 7s
        let trimmed = world.get::<Clip>(clip).unwrap();
        assert_eq!(trimmed.timeline_start, Duration::from_secs(3));
        assert_eq!(trimmed.source_in, Duration::from_secs(3));
        assert_eq!(trimmed.source_out, Duration::from_secs(10)); // unchanged
        assert_eq!(trimmed.duration(), Duration::from_secs(7));
    }

    #[test]
    fn test_trim_clip_out() {
        let mut world = setup_world();

        // Create a clip: timeline 0-10s, source 0-10s
        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Trim out from 10s to 6s (move right edge left)
        let mut cmd = TrimClipCommand::trim_out(clip, Duration::from_secs(6));
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify: source_out = 6s, duration = 6s
        let trimmed = world.get::<Clip>(clip).unwrap();
        assert_eq!(trimmed.timeline_start, Duration::ZERO); // unchanged
        assert_eq!(trimmed.source_in, Duration::ZERO); // unchanged
        assert_eq!(trimmed.source_out, Duration::from_secs(6));
        assert_eq!(trimmed.duration(), Duration::from_secs(6));
    }

    #[test]
    fn test_trim_clip_invalid_zero_duration() {
        let mut world = setup_world();

        // Create a clip: timeline 0-10s, source 0-10s
        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Try to trim out to 0s (would result in zero duration) - should fail
        let mut cmd = TrimClipCommand::trim_out(clip, Duration::ZERO);
        let result = cmd.execute(&mut world);
        assert!(result.is_err());

        // Try to trim in past out point - should fail
        let mut cmd2 = TrimClipCommand::trim_in(clip, Duration::from_secs(15));
        let result2 = cmd2.execute(&mut world);
        assert!(result2.is_err());
    }

    #[test]
    fn test_trim_clip_undo() {
        let mut world = setup_world();

        // Create a clip
        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Trim out
        let mut cmd = TrimClipCommand::trim_out(clip, Duration::from_secs(5));
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify trim applied
        assert_eq!(
            world.get::<Clip>(clip).unwrap().source_out,
            Duration::from_secs(5)
        );

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify restored
        let restored = world.get::<Clip>(clip).unwrap();
        assert_eq!(restored.source_out, Duration::from_secs(10));
        assert_eq!(restored.duration(), Duration::from_secs(10));
    }

    // ========================================================================
    // RIPPLE DELETE TESTS
    // ========================================================================

    #[test]
    fn test_ripple_delete() {
        let mut world = setup_world();

        // Create track with 3 clips back-to-back
        let track = world.spawn(Track::default()).id();

        let clip1 = world
            .spawn((
                Clip {
                    name: "Clip1".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip1);

        let clip2 = world
            .spawn((
                Clip {
                    name: "Clip2".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(5), // Starts at 5s
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip2);

        let clip3 = world
            .spawn((
                Clip {
                    name: "Clip3".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(10), // Starts at 10s
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip3);

        // Ripple delete clip2 (middle clip, 5s duration)
        let mut cmd = RippleDeleteCommand::new(clip2);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify clip2 is gone
        assert!(world.get_entity(clip2).is_err());

        // Verify clip3 shifted left by 5s (clip2's duration)
        let clip3_data = world.get::<Clip>(clip3).unwrap();
        assert_eq!(clip3_data.timeline_start, Duration::from_secs(5));

        // Verify clip1 unchanged
        let clip1_data = world.get::<Clip>(clip1).unwrap();
        assert_eq!(clip1_data.timeline_start, Duration::ZERO);
    }

    #[test]
    fn test_ripple_delete_undo() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();

        let clip1 = world
            .spawn((
                Clip {
                    name: "Clip1".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip1);

        let clip2 = world
            .spawn((
                Clip {
                    name: "Clip2".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(5),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip2);

        // Ripple delete clip1
        let mut cmd = RippleDeleteCommand::new(clip1);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify clip2 shifted to 0
        assert_eq!(
            world.get::<Clip>(clip2).unwrap().timeline_start,
            Duration::ZERO
        );

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify clip2 restored to 5s
        assert_eq!(
            world.get::<Clip>(clip2).unwrap().timeline_start,
            Duration::from_secs(5)
        );

        // Verify track has 2 children again
        assert_eq!(world.get::<Children>(track).unwrap().len(), 2);
    }

    #[test]
    fn test_ripple_delete_last_clip() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();

        let clip = world
            .spawn((
                Clip {
                    name: "Only Clip".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Ripple delete the only clip (no subsequent clips to shift)
        let mut cmd = RippleDeleteCommand::new(clip);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify clip is gone
        assert!(world.get_entity(clip).is_err());

        // Verify no children on track
        assert!(
            world.get::<Children>(track).is_none()
                || world.get::<Children>(track).unwrap().is_empty()
        );
    }

    // ========================================================================
    // SLIP CLIP TESTS
    // ========================================================================

    #[test]
    fn test_slip_clip() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::from_secs(5),
                    source_out: Duration::from_secs(15),
                    timeline_start: Duration::from_secs(10),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Slip by +2s (later in source)
        let mut cmd = SlipClipCommand::with_signed_offset(clip, 2.0);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify source points shifted, but timeline position unchanged
        let slipped = world.get::<Clip>(clip).unwrap();
        assert_eq!(slipped.source_in, Duration::from_secs(7)); // 5 + 2
        assert_eq!(slipped.source_out, Duration::from_secs(17)); // 15 + 2
        assert_eq!(slipped.timeline_start, Duration::from_secs(10)); // unchanged
        assert_eq!(slipped.duration(), Duration::from_secs(10)); // unchanged
    }

    #[test]
    fn test_slip_clip_negative() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::from_secs(10),
                    source_out: Duration::from_secs(20),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Slip by -3s (earlier in source)
        let mut cmd = SlipClipCommand::with_signed_offset(clip, -3.0);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        let slipped = world.get::<Clip>(clip).unwrap();
        assert_eq!(slipped.source_in, Duration::from_secs(7)); // 10 - 3
        assert_eq!(slipped.source_out, Duration::from_secs(17)); // 20 - 3
    }

    #[test]
    fn test_slip_clip_boundary() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::from_secs(2),
                    source_out: Duration::from_secs(12),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Slip by -5s would push source_in below 0, should clamp
        let mut cmd = SlipClipCommand::with_signed_offset(clip, -5.0);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Should be clamped to source_in = 0
        let slipped = world.get::<Clip>(clip).unwrap();
        assert_eq!(slipped.source_in, Duration::ZERO);
        // source_out adjusted accordingly
        assert_eq!(slipped.source_out, Duration::from_secs(10)); // 12 - 2
    }

    #[test]
    fn test_slip_clip_undo() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::from_secs(0),
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        let mut cmd = SlipClipCommand::with_signed_offset(clip, 5.0);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify slipped
        assert_eq!(
            world.get::<Clip>(clip).unwrap().source_in,
            Duration::from_secs(5)
        );

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify restored
        let restored = world.get::<Clip>(clip).unwrap();
        assert_eq!(restored.source_in, Duration::ZERO);
        assert_eq!(restored.source_out, Duration::from_secs(10));
    }

    // ========================================================================
    // SLIDE CLIP TESTS
    // ========================================================================

    #[test]
    fn test_slide_clip() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();

        // Create 3 adjacent clips: A(0-5) | B(5-10) | C(10-15)
        let clip_a = world
            .spawn((
                Clip {
                    name: "A".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip_a);

        let clip_b = world
            .spawn((
                Clip {
                    name: "B".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(5),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip_b);

        let clip_c = world
            .spawn((
                Clip {
                    name: "C".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(10),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip_c);

        // Slide B right by 2s
        let mut cmd = SlideClipCommand::with_signed_delta(clip_b, 2.0);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify: A extended (source_out increased by 2s)
        let a = world.get::<Clip>(clip_a).unwrap();
        assert_eq!(a.source_out, Duration::from_secs(7)); // 5 + 2

        // Verify: B moved right
        let b = world.get::<Clip>(clip_b).unwrap();
        assert_eq!(b.timeline_start, Duration::from_secs(7)); // 5 + 2

        // Verify: C contracted (timeline_start and source_in increased by 2s)
        let c = world.get::<Clip>(clip_c).unwrap();
        assert_eq!(c.timeline_start, Duration::from_secs(12)); // 10 + 2
        assert_eq!(c.source_in, Duration::from_secs(2)); // 0 + 2
    }

    #[test]
    fn test_slide_clip_undo() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();

        let clip_a = world
            .spawn((
                Clip {
                    name: "A".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip_a);

        let clip_b = world
            .spawn((
                Clip {
                    name: "B".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(5),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip_b);

        let mut cmd = SlideClipCommand::with_signed_delta(clip_b, 2.0);
        let undo_data = cmd.execute(&mut world).unwrap();

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify all restored
        assert_eq!(
            world.get::<Clip>(clip_a).unwrap().source_out,
            Duration::from_secs(5)
        );
        assert_eq!(
            world.get::<Clip>(clip_b).unwrap().timeline_start,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn test_slide_clip_no_neighbors() {
        let mut world = setup_world();

        let track = world.spawn(Track::default()).id();

        // Single clip at 5s
        let clip = world
            .spawn((
                Clip {
                    name: "Only".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(5),
                },
                ClipMetadata::default(),
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        // Slide right by 2s (no neighbors to adjust)
        let mut cmd = SlideClipCommand::with_signed_delta(clip, 2.0);
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify moved
        let moved = world.get::<Clip>(clip).unwrap();
        assert_eq!(moved.timeline_start, Duration::from_secs(7));
    }

    // ========================================================================
    // RAZOR TOOL TESTS
    // ========================================================================

    #[test]
    fn test_razor_tool() {
        let mut world = setup_world();

        // Create timeline with 2 tracks
        let timeline = world
            .spawn((Timeline::default(), TimelineState::empty()))
            .id();

        let track1 = world.spawn((Track::default(), TimelineChild)).id();
        world.entity_mut(timeline).add_child(track1);

        let track2 = world.spawn((Track::default(), TimelineChild)).id();
        world.entity_mut(timeline).add_child(track2);

        // Add clips to both tracks (0-10s each)
        let clip1 = world
            .spawn((
                Clip {
                    name: "Clip1".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
                TimelineChild,
                TrackClip,
            ))
            .id();
        world.entity_mut(track1).add_child(clip1);

        let clip2 = world
            .spawn((
                Clip {
                    name: "Clip2".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
                TimelineChild,
                TrackClip,
            ))
            .id();
        world.entity_mut(track2).add_child(clip2);

        // Razor at 5s
        let mut cmd = RazorToolCommand::new(timeline, Duration::from_secs(5));
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify both clips were split (2 splits)
        assert_eq!(cmd.split_results.len(), 2);

        // Verify original clips now end at 5s
        assert_eq!(
            world.get::<Clip>(clip1).unwrap().source_out,
            Duration::from_secs(5)
        );
        assert_eq!(
            world.get::<Clip>(clip2).unwrap().source_out,
            Duration::from_secs(5)
        );

        // Verify each track now has 2 clips
        assert_eq!(world.get::<Children>(track1).unwrap().len(), 2);
        assert_eq!(world.get::<Children>(track2).unwrap().len(), 2);
    }

    #[test]
    fn test_razor_tool_undo() {
        let mut world = setup_world();

        let timeline = world
            .spawn((Timeline::default(), TimelineState::empty()))
            .id();

        let track = world.spawn((Track::default(), TimelineChild)).id();
        world.entity_mut(timeline).add_child(track);

        let clip = world
            .spawn((
                Clip {
                    name: "Clip".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(10),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
                TimelineChild,
                TrackClip,
            ))
            .id();
        world.entity_mut(track).add_child(clip);

        let mut cmd = RazorToolCommand::new(timeline, Duration::from_secs(5));
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify split occurred
        assert_eq!(world.get::<Children>(track).unwrap().len(), 2);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify back to 1 clip
        assert_eq!(world.get::<Children>(track).unwrap().len(), 1);

        // Verify original clip restored
        assert_eq!(
            world.get::<Clip>(clip).unwrap().source_out,
            Duration::from_secs(10)
        );
    }
}
