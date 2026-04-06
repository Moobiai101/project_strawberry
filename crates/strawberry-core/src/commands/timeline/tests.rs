//! # Timeline Command Tests
//!
//! Comprehensive tests for all timeline command operations.

use super::*;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use bevy_hierarchy::{BuildChildren, Children, Parent};
use std::time::Duration;

use crate::commands::Command;
use crate::components::{
    ActiveTimeline, Clip, ClipMetadata, FrameRate, Timeline, TimelineChild, TimelineState,
    Track, TrackClip, TrackType,
};
use crate::resources::StableIdRegistry;

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
