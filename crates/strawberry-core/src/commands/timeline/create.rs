//! # Timeline & Track Creation Commands
//!
//! Commands for creating timelines and tracks.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Children, DespawnRecursiveExt};
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{
    ActiveTimeline, FrameRate, StableId, Timeline, TimelineChild,
    TimelineState, Track, TrackDisplay, TrackType,
};
use crate::impl_serializable_command;
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
    pub(crate) created_entity: Option<u64>,
    /// Stored track entities for undo
    #[serde(skip)]
    pub(crate) created_tracks: Vec<u64>,
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
    pub(crate) created_entity: Option<u64>,
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
