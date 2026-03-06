//! # Timeline Utility Functions
//!
//! Pure functions for timeline calculations. Used by Commands and Systems.
//! No ECS dependencies beyond World read access.
//!
//! ## Purpose
//!
//! These utilities provide common timeline calculations that are used across
//! multiple commands and systems, particularly for auto-sequencing logic.
//!
//! ## Key Functions
//!
//! - `calculate_track_end_time()` - Find where to place the next clip (auto-sequence)
//! - `get_clips_on_track()` - List all clips on a specific track

use bevy_ecs::prelude::*;
use bevy_hierarchy::Children;
use std::time::Duration;

use crate::components::Clip;

// ============================================================================
// TRACK TIME CALCULATIONS
// ============================================================================

/// Calculate the end time of the last clip on a track.
///
/// Used for auto-sequencing: when adding a new clip, this determines
/// where to place it (immediately after the last existing clip).
///
/// ## Returns
///
/// - `Duration::ZERO` if track is empty
/// - End time of rightmost clip otherwise
///
/// ## Example
///
/// ```rust,ignore
/// let track_end = calculate_track_end_time(world, track_entity);
/// let clip_command = AddClipCommand::new(track, asset, track_end);
/// ```
pub fn calculate_track_end_time(world: &World, track: Entity) -> Duration {
    let children: Option<&Children> = world.get::<Children>(track);

    let Some(children) = children else {
        return Duration::ZERO;
    };

    let mut max_end_time: Duration = Duration::ZERO;

    for &child in children.iter() {
        if let Some(clip) = world.get::<Clip>(child) {
            let clip_end_time: Duration = clip.timeline_start + clip.duration();
            if clip_end_time > max_end_time {
                max_end_time = clip_end_time;
            }
        }
    }

    max_end_time
}

/// Get all clip entities on a track, sorted by timeline position.
///
/// Useful for collision detection and ripple operations.
pub fn get_clips_on_track_sorted(world: &World, track: Entity) -> Vec<(Entity, Duration)> {
    let children: Option<&Children> = world.get::<Children>(track);

    let Some(children) = children else {
        return Vec::new();
    };

    let mut clips: Vec<(Entity, Duration)> = children
        .iter()
        .filter_map(|&child| {
            world
                .get::<Clip>(child)
                .map(|clip| (child, clip.timeline_start))
        })
        .collect();

    // Sort by timeline start position
    clips.sort_by_key(|(_, start)| *start);

    clips
}

/// Calculate the total duration of content on a track (including gaps).
///
/// Returns the end time of the rightmost clip, which represents
/// the minimum timeline length needed to contain all clips on this track.
pub fn calculate_track_content_duration(world: &World, track: Entity) -> Duration {
    calculate_track_end_time(world, track)
}

/// Count the number of clips on a track.
pub fn count_clips_on_track(world: &World, track: Entity) -> usize {
    let children: Option<&Children> = world.get::<Children>(track);

    match children {
        Some(children) => children
            .iter()
            .filter(|&&child| world.get::<Clip>(child).is_some())
            .count(),
        None => 0,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{ClipMetadata, Track};
    use bevy_hierarchy::BuildChildren;

    fn setup_world() -> World {
        World::new()
    }

    #[test]
    fn test_calculate_track_end_time_empty() {
        let mut world = setup_world();
        let track = world.spawn(Track::default()).id();

        let end_time = calculate_track_end_time(&world, track);
        assert_eq!(end_time, Duration::ZERO);
    }

    #[test]
    fn test_calculate_track_end_time_single_clip() {
        let mut world = setup_world();
        let track = world.spawn(Track::default()).id();

        // Create a clip that starts at 2s and is 5s long (ends at 7s)
        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::from_secs(2),
                },
                ClipMetadata::default(),
            ))
            .id();

        world.entity_mut(track).add_child(clip);

        let end_time = calculate_track_end_time(&world, track);
        assert_eq!(end_time, Duration::from_secs(7)); // 2 + 5 = 7
    }

    #[test]
    fn test_calculate_track_end_time_multiple_clips() {
        let mut world = setup_world();
        let track = world.spawn(Track::default()).id();

        // Clip A: 0-3s
        let clip_a = world
            .spawn((
                Clip {
                    name: "A".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(3),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();

        // Clip B: 5-10s (with gap)
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

        // Clip C: 3-5s (between A and B, added last - out of order)
        let clip_c = world
            .spawn((
                Clip {
                    name: "C".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(2),
                    timeline_start: Duration::from_secs(3),
                },
                ClipMetadata::default(),
            ))
            .id();

        world.entity_mut(track).add_child(clip_a);
        world.entity_mut(track).add_child(clip_b);
        world.entity_mut(track).add_child(clip_c);

        // Should return 10s (end of clip B, the rightmost)
        let end_time = calculate_track_end_time(&world, track);
        assert_eq!(end_time, Duration::from_secs(10));
    }

    #[test]
    fn test_get_clips_sorted() {
        let mut world = setup_world();
        let track = world.spawn(Track::default()).id();

        // Add clips out of order
        let clip_b = world
            .spawn((
                Clip {
                    name: "B".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(3),
                    timeline_start: Duration::from_secs(5),
                },
                ClipMetadata::default(),
            ))
            .id();

        let clip_a = world
            .spawn((
                Clip {
                    name: "A".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(3),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();

        world.entity_mut(track).add_child(clip_b);
        world.entity_mut(track).add_child(clip_a);

        let sorted = get_clips_on_track_sorted(&world, track);

        // Should be sorted: A at 0s first, then B at 5s
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].0, clip_a);
        assert_eq!(sorted[1].0, clip_b);
    }

    #[test]
    fn test_count_clips_on_track() {
        let mut world = setup_world();
        let track = world.spawn(Track::default()).id();

        assert_eq!(count_clips_on_track(&world, track), 0);

        let clip = world
            .spawn((
                Clip {
                    name: "Test".to_string(),
                    source_in: Duration::ZERO,
                    source_out: Duration::from_secs(5),
                    timeline_start: Duration::ZERO,
                },
                ClipMetadata::default(),
            ))
            .id();

        world.entity_mut(track).add_child(clip);

        assert_eq!(count_clips_on_track(&world, track), 1);
    }
}
