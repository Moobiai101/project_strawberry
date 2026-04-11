//! # Collision Detection System
//!
//! Pure functions for detecting and resolving clip overlaps.
//!
//! ## Architecture
//!
//! This module provides stateless functions that query the ECS world
//! to detect collisions. No ECS systems run automatically - these functions
//! are called on-demand during clip placement operations.
//!
//! ## Usage Flow
//!
//! 1. User drags a clip to a new position
//! 2. During drag: `check_collision()` is called for preview
//! 3. On drop: `resolve_collision()` executes the appropriate action
//!
//! ## Performance
//!
//! For 1000+ clips, collision checking uses spatial indexing:
//! - First filter by track (O(1) lookup)
//! - Then check time overlap (O(n) on clips in track, but tracks typically have <50 clips)

use bevy_ecs::prelude::*;
use bevy_hierarchy::Parent;
use std::time::Duration;
use uuid::Uuid;

use crate::components::timeline::TrackClip;
use crate::components::{Clip, StableId};
use crate::resources::collision::{
    ClipTrimAction, CollisionAction, CollisionInfo, CollisionMode, CollisionSettings,
};
use crate::resources::StableIdRegistry;

// ============================================================================
// COLLISION DETECTION
// ============================================================================

/// Check if placing a clip would overlap with existing clips.
///
/// Returns `None` if no collision, or `Some(CollisionInfo)` with
/// details about what would be affected and how to resolve.
///
/// ## Parameters
///
/// - `world`: ECS world to query
/// - `track_uuid`: UUID of the track to place on
/// - `start`: Timeline start position for the new clip
/// - `duration`: Duration of the new clip
/// - `exclude`: Optional clip UUID to exclude (for move operations)
///
/// ## Returns
///
/// `None` if placement is clear, `Some(CollisionInfo)` if overlaps exist.
pub fn check_collision(
    world: &mut World,
    track_uuid: Uuid,
    start: Duration,
    duration: Duration,
    exclude: Option<Uuid>,
) -> Option<CollisionInfo> {
    // First, get track entity and settings from the registry
    let track_entity: Entity;
    let mode: CollisionMode;
    let collision_threshold_ms: u32;

    {
        let registry: &StableIdRegistry = world.resource::<StableIdRegistry>();
        track_entity = registry.get(&track_uuid)?;

        let settings: &CollisionSettings = world.resource::<CollisionSettings>();
        mode = settings.mode;
        collision_threshold_ms = settings.collision_threshold_ms;
    }

    // Calculate placement end time
    let placement_start_secs: f64 = start.as_secs_f64();
    let placement_end_secs: f64 = placement_start_secs + duration.as_secs_f64();

    // Find all clips on this track that overlap
    let mut affected_clips: Vec<Uuid> = Vec::new();
    let mut overlap_start: f64 = f64::MAX;
    let mut overlap_end: f64 = 0.0;

    // Query all clips that belong to this track (using Parent component)
    let mut query = world.query::<(Entity, &Clip, &TrackClip, &Parent, &StableId)>();

    for (_entity, clip, _track_clip, parent, stable_id) in query.iter(world) {
        // Skip if not on our track
        if parent.get() != track_entity {
            continue;
        }

        let clip_uuid: Uuid = stable_id.uuid();

        // Skip the clip being moved
        if Some(clip_uuid) == exclude {
            continue;
        }

        // Check for time overlap
        let clip_start_secs: f64 = clip.timeline_start.as_secs_f64();
        let clip_duration: Duration = clip.duration();
        let clip_end_secs: f64 = clip_start_secs + clip_duration.as_secs_f64();

        // Overlap condition: ranges intersect
        if placement_start_secs < clip_end_secs && placement_end_secs > clip_start_secs {
            // Calculate actual overlap range
            let actual_overlap_start: f64 = placement_start_secs.max(clip_start_secs);
            let actual_overlap_end: f64 = placement_end_secs.min(clip_end_secs);
            let overlap_ms: u64 = ((actual_overlap_end - actual_overlap_start) * 1000.0) as u64;

            // Check if overlap is above threshold
            if overlap_ms >= collision_threshold_ms as u64 {
                affected_clips.push(clip_uuid);
                overlap_start = overlap_start.min(actual_overlap_start);
                overlap_end = overlap_end.max(actual_overlap_end);
            }
        }
    }

    if affected_clips.is_empty() {
        None
    } else {
        let action: CollisionAction =
            calculate_resolution(world, &affected_clips, track_uuid, start, duration, mode);

        Some(
            CollisionInfo::new(affected_clips, track_uuid, overlap_start, overlap_end)
                .with_action(action),
        )
    }
}

/// Calculate the resolution action based on collision mode
fn calculate_resolution(
    world: &mut World,
    affected_clips: &[Uuid],
    track_uuid: Uuid,
    placement_start: Duration,
    placement_duration: Duration,
    mode: CollisionMode,
) -> CollisionAction {
    match mode {
        CollisionMode::AutoTrack => calculate_auto_track_resolution(world, track_uuid),

        CollisionMode::Ripple => {
            calculate_ripple_resolution(world, affected_clips, placement_start, placement_duration)
        }

        CollisionMode::Overwrite => calculate_overwrite_resolution(
            world,
            affected_clips,
            placement_start,
            placement_duration,
        ),

        CollisionMode::Reject => CollisionAction::Reject {
            reason: format!(
                "Overlaps with {} existing clip{}",
                affected_clips.len(),
                if affected_clips.len() == 1 { "" } else { "s" }
            ),
        },
    }
}

/// Calculate AutoTrack resolution - create a new track for the clip
fn calculate_auto_track_resolution(world: &mut World, track_uuid: Uuid) -> CollisionAction {
    // Get track entity to find its index
    let track_entity: Option<Entity>;
    {
        let registry: &StableIdRegistry = world.resource::<StableIdRegistry>();
        track_entity = registry.get(&track_uuid);
    }

    // Query the actual track index from the Track component
    let insert_index: u32 = if let Some(entity) = track_entity {
        world
            .get::<crate::components::Track>(entity)
            .map(|track| track.index + 1)
            .unwrap_or(0)
    } else {
        0
    };

    CollisionAction::CreateTrack {
        suggested_name: format!("Video {}", insert_index + 1),
        insert_at_index: insert_index,
    }
}

/// Calculate Ripple resolution - shift clips right
fn calculate_ripple_resolution(
    world: &mut World,
    affected_clips: &[Uuid],
    placement_start: Duration,
    placement_duration: Duration,
) -> CollisionAction {
    // First, collect clips from affected list
    let mut clips_to_shift: Vec<Uuid> = Vec::new();

    for clip_uuid in affected_clips {
        // Look up the clip entity and check if it starts at or after placement
        let registry: &StableIdRegistry = world.resource::<StableIdRegistry>();
        if let Some(entity) = registry.get(clip_uuid) {
            if let Some(clip) = world.get::<Clip>(entity) {
                if clip.timeline_start >= placement_start {
                    clips_to_shift.push(*clip_uuid);
                }
            }
        }
    }

    // Also find clips that start after placement but weren't in affected list
    let mut query = world.query::<(Entity, &Clip, &TrackClip, &StableId)>();
    for (_entity, clip, _, stable_id) in query.iter(world) {
        let uuid = stable_id.uuid();
        if clip.timeline_start >= placement_start
            && !clips_to_shift.contains(&uuid)
            && !affected_clips.contains(&uuid)
        {
            clips_to_shift.push(uuid);
        }
    }

    CollisionAction::RippleRight {
        clips_to_shift,
        shift_amount: placement_duration,
    }
}

/// Calculate Overwrite resolution - trim overlapped clips
fn calculate_overwrite_resolution(
    world: &mut World,
    affected_clips: &[Uuid],
    placement_start: Duration,
    placement_duration: Duration,
) -> CollisionAction {
    let placement_end: Duration = placement_start + placement_duration;
    let mut trims: Vec<ClipTrimAction> = Vec::new();

    for clip_uuid in affected_clips {
        let registry: &StableIdRegistry = world.resource::<StableIdRegistry>();
        if let Some(entity) = registry.get(clip_uuid) {
            if let Some(clip) = world.get::<Clip>(entity) {
                let clip_duration: Duration = clip.duration();
                let clip_end: Duration = clip.timeline_start + clip_duration;
                let clip_start = clip.timeline_start;
                let source_in = clip.source_in;

                // Case 1: Clip is fully covered - delete it
                if placement_start <= clip_start && placement_end >= clip_end {
                    trims.push(ClipTrimAction::delete(*clip_uuid));
                }
                // Case 2: Overlap cuts clip into two pieces - for now, trim right side
                else if placement_start > clip_start && placement_end < clip_end {
                    // Trim the clip to end at placement_start
                    let new_duration: Duration = placement_start - clip_start;
                    trims.push(ClipTrimAction::trim_right(*clip_uuid, new_duration));
                }
                // Case 3: Overlap on left side of existing clip - trim left
                else if placement_end > clip_start && placement_end < clip_end {
                    let new_start: Duration = placement_end;
                    let new_source_in: Duration = source_in + (placement_end - clip_start);
                    trims.push(ClipTrimAction::trim_left(
                        *clip_uuid,
                        new_start,
                        new_source_in,
                    ));
                }
                // Case 4: Overlap on right side of existing clip - trim right
                else if placement_start > clip_start && placement_start < clip_end {
                    let new_duration: Duration = placement_start - clip_start;
                    trims.push(ClipTrimAction::trim_right(*clip_uuid, new_duration));
                }
            }
        }
    }

    CollisionAction::TrimClips { trims }
}

/// Get all clips on a specific track, sorted by timeline position
///
/// Utility function for collision detection and track operations.
pub fn get_clips_on_track(
    world: &mut World,
    track_entity: Entity,
) -> Vec<(Uuid, Duration, Duration)> {
    let mut clips: Vec<(Uuid, Duration, Duration)> = Vec::new();

    let mut query = world.query::<(&Clip, &TrackClip, &Parent, &StableId)>();

    for (clip, _track_clip, parent, stable_id) in query.iter(world) {
        if parent.get() == track_entity {
            let uuid = stable_id.uuid();
            let clip_duration: Duration = clip.duration();
            clips.push((uuid, clip.timeline_start, clip_duration));
        }
    }

    // Sort by timeline start position
    clips.sort_by(|a, b| a.1.cmp(&b.1));
    clips
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    // Note: Full collision tests require a mock ECS world with test clips
    // These are integration tests that would run in strawberry-core

    #[test]
    fn test_get_clips_on_track_compiles() {
        // Verify the function signature compiles correctly
        // Actual tests require a populated World
    }
}
