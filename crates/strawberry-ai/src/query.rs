//! # AI Query Service
//!
//! Read-only ECS access for AI to understand project state.
//!
//! This is the AI's "eyes" into the project. All queries return
//! serializable JSON-compatible structures that can be included
//! in LLM context.

use bevy_ecs::entity::Entity;

use bevy_ecs::world::World;
use bevy_hierarchy::{Children, Parent};
use serde::{Deserialize, Serialize};

use strawberry_core::components::{
    ActiveTimeline, Clip, ClipMetadata, ClipTransform, Project, ProjectSettings, Timeline,
    TimelineState, Track, TrackDisplay,
};
use strawberry_core::resources::{Playhead, Selection};

/// Query service for AI to read ECS state
///
/// Methods take `&World` directly - the caller manages world access.
/// This is the production pattern used by Bevy systems.
pub struct AIQueryService;

impl AIQueryService {
    /// Get project context (Tier 1 - Always Present, <2K tokens)
    ///
    /// This is the core context that AI always needs:
    /// - Project metadata
    /// - Playhead position
    /// - Track/clip counts
    /// - Selection state
    pub fn get_project_context(world: &World) -> ProjectContext {
        let mut context = ProjectContext::default();

        // Get project and settings
        // Note: We iterate components directly since we're in a read context
        for entity in world.iter_entities() {
            if let Some(project) = entity.get::<Project>() {
                if let Some(settings) = entity.get::<ProjectSettings>() {
                    context.project_id = Some(entity.id().to_bits().to_string());
                    context.name = project.name.clone();
                    context.resolution = [settings.resolution.x, settings.resolution.y];
                    context.frame_rate = settings.frame_rate.as_f64();
                    context.color_space = settings.color_space.display_name().to_string();
                    context.duration_secs = settings.duration.as_secs_f64();
                    break; // Only one project expected
                }
            }
        }

        // Get playhead state
        if let Some(playhead) = world.get_resource::<Playhead>() {
            context.playhead = PlayheadContext {
                current_frame: playhead.current_frame,
                current_time_secs: playhead.current_time.as_secs_f64(),
                is_playing: playhead.is_playing,
                playback_speed: playhead.playback_speed,
            };
        }

        // Count tracks and clips
        for entity in world.iter_entities() {
            if entity.get::<Track>().is_some() {
                context.track_count += 1;
            }
            if entity.get::<Clip>().is_some() {
                context.clip_count += 1;
            }
        }

        // Get selection
        if let Some(selection) = world.get_resource::<Selection>() {
            context.selected_count = selection.count();
            context.primary_selection = selection.primary.map(|bits| bits.to_string());
        }

        context
    }

    /// Get playhead state only (lightweight query)
    pub fn get_playhead(world: &World) -> PlayheadContext {
        if let Some(playhead) = world.get_resource::<Playhead>() {
            PlayheadContext {
                current_frame: playhead.current_frame,
                current_time_secs: playhead.current_time.as_secs_f64(),
                is_playing: playhead.is_playing,
                playback_speed: playhead.playback_speed,
            }
        } else {
            PlayheadContext::default()
        }
    }

    /// Get clips near playhead (Tier 2 - Dynamic Context, <8K tokens)
    ///
    /// Returns clips within `range_secs` of the current playhead position.
    /// This gives AI spatial context of what's happening "now".
    pub fn get_clips_near_playhead(world: &World, range_secs: f64) -> Vec<ClipContext> {
        let playhead_time = world
            .get_resource::<Playhead>()
            .map(|p| p.current_time.as_secs_f64())
            .unwrap_or(0.0);

        let mut clips = Vec::new();

        for entity in world.iter_entities() {
            if let Some(clip) = entity.get::<Clip>() {
                let clip_start = clip.timeline_start.as_secs_f64();
                let clip_end = clip.timeline_end().as_secs_f64();

                // Check if clip is within range of playhead
                if (clip_start - range_secs) <= playhead_time
                    && (clip_end + range_secs) >= playhead_time
                {
                    let transform = entity.get::<ClipTransform>();

                    clips.push(ClipContext {
                        id: entity.id().to_bits().to_string(),
                        name: clip.name.clone(),
                        timeline_start_secs: clip_start,
                        duration_secs: clip.duration().as_secs_f64(),
                        source_in_secs: clip.source_in.as_secs_f64(),
                        opacity: transform.map(|t| t.opacity).unwrap_or(1.0),
                        track_id: None, // TODO: Add when we have parent queries
                    });
                }
            }
        }

        // Sort by timeline position
        clips.sort_by(|a, b| {
            a.timeline_start_secs
                .partial_cmp(&b.timeline_start_secs)
                .unwrap()
        });

        clips
    }

    /// Get all clips (for smaller projects or full context)
    pub fn get_all_clips(world: &World) -> Vec<ClipContext> {
        let mut clips = Vec::new();

        for entity in world.iter_entities() {
            if let Some(clip) = entity.get::<Clip>() {
                let transform = entity.get::<ClipTransform>();

                clips.push(ClipContext {
                    id: entity.id().to_bits().to_string(),
                    name: clip.name.clone(),
                    timeline_start_secs: clip.timeline_start.as_secs_f64(),
                    duration_secs: clip.duration().as_secs_f64(),
                    source_in_secs: clip.source_in.as_secs_f64(),
                    opacity: transform.map(|t| t.opacity).unwrap_or(1.0),
                    track_id: None,
                });
            }
        }

        clips.sort_by(|a, b| {
            a.timeline_start_secs
                .partial_cmp(&b.timeline_start_secs)
                .unwrap()
        });
        clips
    }

    /// Get all tracks
    pub fn get_all_tracks(world: &World) -> Vec<TrackContext> {
        let mut tracks = Vec::new();

        for entity in world.iter_entities() {
            if let Some(track) = entity.get::<Track>() {
                tracks.push(TrackContext {
                    id: entity.id().to_bits().to_string(),
                    index: track.index,
                    name: track.name.clone(),
                    track_type: format!("{:?}", track.track_type),
                    muted: track.muted,
                    locked: track.locked,
                });
            }
        }

        // Sort by index (important for AI to understand layer order)
        tracks.sort_by_key(|t| t.index);

        tracks
    }

    /// Get selected entities with full details (Tier 2 context)
    pub fn get_selected_entities(world: &World) -> Vec<SelectedEntityContext> {
        let mut selected = Vec::new();

        let selection = match world.get_resource::<Selection>() {
            Some(s) => s,
            None => return selected,
        };

        for &entity_bits in selection.selected.iter() {
            let entity = Entity::from_bits(entity_bits);

            if let Ok(entity_ref) = world.get_entity(entity) {
                let mut ctx = SelectedEntityContext {
                    id: entity_bits.to_string(),
                    entity_type: "unknown".to_string(),
                    name: None,
                    details: serde_json::json!({}),
                };

                // Check what type of entity this is
                if let Some(clip) = entity_ref.get::<Clip>() {
                    ctx.entity_type = "clip".to_string();
                    ctx.name = Some(clip.name.clone());
                    ctx.details = serde_json::json!({
                        "timeline_start": clip.timeline_start.as_secs_f64(),
                        "duration": clip.duration().as_secs_f64(),
                        "source_in": clip.source_in.as_secs_f64(),
                        "source_out": clip.source_out.as_secs_f64(),
                    });

                    if let Some(transform) = entity_ref.get::<ClipTransform>() {
                        ctx.details["transform"] = serde_json::json!({
                            "position": [transform.position.x, transform.position.y],
                            "scale": [transform.scale.x, transform.scale.y],
                            "rotation": transform.rotation,
                            "opacity": transform.opacity,
                        });
                    }
                } else if let Some(track) = entity_ref.get::<Track>() {
                    ctx.entity_type = "track".to_string();
                    ctx.name = Some(track.name.clone());
                    ctx.details = serde_json::json!({
                        "index": track.index,
                        "track_type": format!("{:?}", track.track_type),
                        "muted": track.muted,
                        "locked": track.locked,
                    });
                }

                selected.push(ctx);
            }
        }

        selected
    }

    /// Build full AI context (combines all tiers up to token budget)
    ///
    /// This is the main entry point for building LLM context.
    /// The result includes Tier 1 (always), Tier 2 (near playhead + selection),
    /// and can be extended with Tier 3 (semantic search) and Tier 4 (summaries).
    pub fn build_full_context(world: &World) -> FullAIContext {
        FullAIContext {
            project: Self::get_project_context(world),
            timeline: Self::get_timeline_context(world),
            clips_near_playhead: Self::get_clips_near_playhead(world, 30.0),
            selected_entities: Self::get_selected_entities(world),
            all_tracks: Self::get_all_tracks(world),
        }
    }

    // =========================================================================
    // TIMELINE-SPECIFIC QUERIES
    // =========================================================================

    /// Get active timeline context (Tier 1)
    ///
    /// Returns information about the currently active timeline including:
    /// - Timeline metadata (name, frame rate)
    /// - Computed state (duration, track/clip counts)
    /// - Hierarchical track list with clips
    pub fn get_timeline_context(world: &World) -> Option<TimelineContext> {
        // Get active timeline
        let active_timeline = world.get_resource::<ActiveTimeline>()?;
        let timeline_entity = active_timeline.get()?;

        // Safety check - get the actual entity ref
        let entity_ref = world.get_entity(timeline_entity).ok()?;

        // Get timeline component
        let timeline = entity_ref.get::<Timeline>()?;
        let state = entity_ref.get::<TimelineState>();

        // Build track list with clips
        let tracks = Self::get_timeline_tracks(world, timeline_entity);

        Some(TimelineContext {
            id: timeline_entity.to_bits().to_string(),
            name: timeline.name.clone(),
            frame_rate: timeline.frame_rate.as_f64(),
            duration_secs: state.map(|s| s.duration.as_secs_f64()).unwrap_or(0.0),
            track_count: state.map(|s| s.track_count).unwrap_or(tracks.len() as u32),
            clip_count: state.map(|s| s.clip_count).unwrap_or(0),
            tracks,
        })
    }

    /// Get tracks for a specific timeline with their clips
    fn get_timeline_tracks(world: &World, timeline_entity: Entity) -> Vec<TrackWithClipsContext> {
        let mut tracks = Vec::new();

        // Get children of timeline (tracks)
        let children = match world.get::<Children>(timeline_entity) {
            Some(c) => c,
            None => return tracks,
        };

        for &track_entity in children.iter() {
            // Check if this child is a Track
            let entity_ref = match world.get_entity(track_entity) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let Some(track) = entity_ref.get::<Track>() else {
                continue;
            };

            // Get track display settings
            let display = entity_ref.get::<TrackDisplay>();

            // Get clips on this track
            let clips = Self::get_track_clips(world, track_entity);

            tracks.push(TrackWithClipsContext {
                id: track_entity.to_bits().to_string(),
                index: track.index,
                name: track.name.clone(),
                track_type: format!("{:?}", track.track_type),
                muted: track.muted,
                solo: track.solo,
                locked: track.locked,
                height: display.map(|d| d.height).unwrap_or(60.0),
                clips,
            });
        }

        // Sort by index
        tracks.sort_by_key(|t| t.index);
        tracks
    }

    /// Get clips on a specific track
    fn get_track_clips(world: &World, track_entity: Entity) -> Vec<ClipOnTrackContext> {
        let mut clips = Vec::new();

        // Get children of track (clips)
        let children = match world.get::<Children>(track_entity) {
            Some(c) => c,
            None => return clips,
        };

        for &clip_entity in children.iter() {
            let entity_ref = match world.get_entity(clip_entity) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let Some(clip) = entity_ref.get::<Clip>() else {
                continue;
            };

            let metadata = entity_ref.get::<ClipMetadata>();
            let transform = entity_ref.get::<ClipTransform>();

            clips.push(ClipOnTrackContext {
                id: clip_entity.to_bits().to_string(),
                name: clip.name.clone(),
                timeline_start_secs: clip.timeline_start.as_secs_f64(),
                duration_secs: clip.duration().as_secs_f64(),
                source_in_secs: clip.source_in.as_secs_f64(),
                source_out_secs: clip.source_out.as_secs_f64(),
                enabled: metadata.map(|m| m.enabled).unwrap_or(true),
                speed: metadata.map(|m| m.speed).unwrap_or(1.0),
                opacity: transform.map(|t| t.opacity).unwrap_or(1.0),
            });
        }

        // Sort by timeline position
        clips.sort_by(|a, b| {
            a.timeline_start_secs
                .partial_cmp(&b.timeline_start_secs)
                .unwrap()
        });
        clips
    }

    /// Get all timelines in the project
    pub fn get_all_timelines(world: &World) -> Vec<TimelineListItem> {
        let mut timelines = Vec::new();

        // Get active timeline for comparison
        let active_bits = world
            .get_resource::<ActiveTimeline>()
            .and_then(|a| a.get())
            .map(|e| e.to_bits());

        for entity in world.iter_entities() {
            if let Some(timeline) = entity.get::<Timeline>() {
                let entity_bits = entity.id().to_bits();
                timelines.push(TimelineListItem {
                    id: entity_bits.to_string(),
                    name: timeline.name.clone(),
                    frame_rate: timeline.frame_rate.as_f64(),
                    is_active: Some(entity_bits) == active_bits,
                });
            }
        }

        timelines
    }
}

// =============================================================================
// CONTEXT STRUCTURES (JSON-serializable for LLM)
// =============================================================================

/// Project context for AI (Tier 1 - Always Present)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_id: Option<String>,
    pub name: String,
    pub resolution: [u32; 2],
    pub frame_rate: f64,
    pub color_space: String,
    pub duration_secs: f64,
    pub playhead: PlayheadContext,
    pub track_count: usize,
    pub clip_count: usize,
    pub selected_count: usize,
    pub primary_selection: Option<String>,
}

/// Playhead context
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayheadContext {
    pub current_frame: u64,
    pub current_time_secs: f64,
    pub is_playing: bool,
    pub playback_speed: f32,
}

/// Clip context for AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipContext {
    pub id: String,
    pub name: String,
    pub timeline_start_secs: f64,
    pub duration_secs: f64,
    pub source_in_secs: f64,
    pub opacity: f32,
    pub track_id: Option<String>,
}

/// Track context for AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackContext {
    pub id: String,
    pub index: u32,
    pub name: String,
    pub track_type: String,
    pub muted: bool,
    pub locked: bool,
}

/// Selected entity context with full details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedEntityContext {
    pub id: String,
    pub entity_type: String,
    pub name: Option<String>,
    pub details: serde_json::Value,
}

/// Full AI context combining all tiers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullAIContext {
    pub project: ProjectContext,
    pub timeline: Option<TimelineContext>,
    pub clips_near_playhead: Vec<ClipContext>,
    pub selected_entities: Vec<SelectedEntityContext>,
    pub all_tracks: Vec<TrackContext>,
}

// =============================================================================
// TIMELINE-SPECIFIC CONTEXT STRUCTURES
// =============================================================================

/// Active timeline context with hierarchical track/clip data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineContext {
    /// Timeline entity ID
    pub id: String,
    /// Timeline name
    pub name: String,
    /// Frame rate (fps)
    pub frame_rate: f64,
    /// Total duration in seconds
    pub duration_secs: f64,
    /// Number of tracks
    pub track_count: u32,
    /// Number of clips across all tracks
    pub clip_count: u32,
    /// Tracks with their clips (hierarchical)
    pub tracks: Vec<TrackWithClipsContext>,
}

/// Track context with embedded clips
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackWithClipsContext {
    pub id: String,
    pub index: u32,
    pub name: String,
    pub track_type: String,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub height: f32,
    pub clips: Vec<ClipOnTrackContext>,
}

/// Clip context within a track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipOnTrackContext {
    pub id: String,
    pub name: String,
    pub timeline_start_secs: f64,
    pub duration_secs: f64,
    pub source_in_secs: f64,
    pub source_out_secs: f64,
    pub enabled: bool,
    pub speed: f32,
    pub opacity: f32,
}

/// Timeline list item for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineListItem {
    pub id: String,
    pub name: String,
    pub frame_rate: f64,
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_context_serializes() {
        let context = ProjectContext {
            name: "Test".to_string(),
            resolution: [1920, 1080],
            frame_rate: 30.0,
            ..Default::default()
        };

        let json = serde_json::to_string(&context).unwrap();
        assert!(json.contains("Test"));
        assert!(json.contains("1920"));
    }

    #[test]
    fn test_full_context_serializes() {
        let context = FullAIContext {
            project: ProjectContext::default(),
            timeline: None,
            clips_near_playhead: vec![],
            selected_entities: vec![],
            all_tracks: vec![],
        };

        let json = serde_json::to_string_pretty(&context).unwrap();
        assert!(json.contains("project"));
        assert!(json.contains("clips_near_playhead"));
    }
}
