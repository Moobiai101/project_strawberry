//! # Playhead Advance System
//!
//! Production-grade playback clock system that advances the playhead each frame.
//!
//! ## How Professional NLEs Work
//!
//! Adobe Premiere, CapCut, DaVinci Resolve all have a dedicated playback clock:
//! 1. Each frame, calculate delta time since last frame
//! 2. If playing, advance playhead by (delta * playback_speed)
//! 3. Handle looping, timeline bounds, etc.
//! 4. Update current_frame based on new current_time
//!
//! This system implements that pattern.

use bevy_ecs::prelude::*;
use std::time::{Duration, Instant};

use crate::resources::Playhead;

/// Resource to track elapsed time for playback
///
/// We use our own time tracking rather than bevy_time::Time because:
/// 1. We need precise control over playback timing
/// 2. egui apps may not have bevy_time properly set up
/// 3. We want to handle frame drops gracefully
#[derive(Resource)]
pub struct PlaybackClock {
    /// Last frame timestamp
    last_frame: Instant,
    /// Timeline duration (from loaded video)
    pub timeline_duration: Duration,
    /// Frame rate of the current video
    pub frame_rate: f64,
}

impl PlaybackClock {
    pub fn new() -> Self {
        Self {
            last_frame: Instant::now(),
            timeline_duration: Duration::from_secs(60), // Default 1 minute
            frame_rate: 30.0,                           // Default 30 fps
        }
    }

    /// Set the timeline duration and frame rate from loaded video
    pub fn set_timeline(&mut self, duration: Duration, frame_rate: f64) {
        self.timeline_duration = duration;
        self.frame_rate = frame_rate;
    }

    /// Get delta time since last frame and reset timer
    pub fn delta(&mut self) -> Duration {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame);
        self.last_frame = now;

        // Cap delta to prevent huge jumps (e.g., after window minimize)
        // Max 100ms = 10 FPS minimum
        Duration::min(delta, Duration::from_millis(100))
    }
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::new()
    }
}

/// System that advances the playhead each frame during playback
///
/// This is the core "clock" of the video player. It:
/// 1. Calculates delta time since last frame
/// 2. Advances playhead by (delta * playback_speed) if playing
/// 3. Handles timeline bounds and looping
///
/// Call this BEFORE viewport_update_system so the new position is used.
pub fn playhead_advance_system(mut playhead: ResMut<Playhead>, mut clock: ResMut<PlaybackClock>) {
    // Always consume delta time to keep timing accurate
    let delta = clock.delta();

    // Only advance if playing
    if !playhead.is_playing {
        return;
    }

    // Advance playhead using existing method
    playhead.advance(delta, clock.frame_rate, clock.timeline_duration);

    // Log occasionally for debugging (every ~60 frames)
    if playhead.current_frame % 60 == 0 {
        tracing::trace!(
            "Playhead: frame={}, time={:.2}s, playing={}",
            playhead.current_frame,
            playhead.current_time.as_secs_f64(),
            playhead.is_playing
        );
    }
}

/// System that dynamically syncs PlaybackClock::timeline_duration from actual
/// clip endpoints on the active timeline.
///
/// ## Why this system exists
///
/// `PlaybackClock::timeline_duration` was previously only set on file open and
/// defaulted to 60 seconds. Any clips added via Media Browser, splits, trims,
/// or undo/redo would NOT update it, causing playback to hard-stop at 60s.
///
/// This system recomputes the duration every frame by traversing
/// Timeline → Tracks → Clips and finding the maximum endpoint. This is the
/// production-grade approach used by real NLEs (Premiere, Resolve) — it's
/// correct by construction and O(total_clips) cost is negligible.
///
/// ## Ordering
///
/// Must run BEFORE `playhead_advance_system` so the duration is current
/// when the playhead checks its bounds.
///
/// ## Why exclusive system
///
/// We need both read access to the ECS hierarchy (ActiveTimeline, Children,
/// Track, Clip) and write access to PlaybackClock. Bevy's normal system
/// params can't mix `&World` with `ResMut<T>`, so we use an exclusive system
/// that takes `&mut World` and accesses resources manually.
pub fn timeline_duration_sync_system(world: &mut World) {
    // Read active timeline entity
    let timeline_entity: Option<bevy_ecs::entity::Entity> = world
        .get_resource::<crate::components::ActiveTimeline>()
        .and_then(|at| at.get());

    let Some(timeline_entity) = timeline_entity else {
        // No active timeline — leave duration as-is (default or last-set)
        return;
    };

    // Compute duration from clip endpoints (read-only traversal)
    let computed_duration: Duration =
        crate::resources::calculate_timeline_total_duration(world, timeline_entity);

    // Update PlaybackClock if duration changed
    if computed_duration > Duration::ZERO {
        let clock = world.resource_mut::<PlaybackClock>();
        if computed_duration != clock.timeline_duration {
            tracing::debug!(
                "Timeline duration synced: {:.2}s → {:.2}s",
                clock.timeline_duration.as_secs_f64(),
                computed_duration.as_secs_f64(),
            );
            // Must reborrow mutably after the comparison
            world.resource_mut::<PlaybackClock>().timeline_duration = computed_duration;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_clock_delta() {
        let mut clock = PlaybackClock::new();

        // Small delay
        std::thread::sleep(Duration::from_millis(10));

        let delta = clock.delta();
        assert!(delta >= Duration::from_millis(10));
        assert!(delta < Duration::from_millis(100)); // Cap should work
    }

    #[test]
    fn test_delta_cap() {
        let mut clock = PlaybackClock::new();

        // Simulate a huge gap (like window minimize)
        std::thread::sleep(Duration::from_millis(5)); // Small sleep

        // Manually set last_frame to a long time ago
        clock.last_frame = Instant::now() - Duration::from_secs(5);

        let delta = clock.delta();
        // Should be capped at 100ms
        assert!(delta <= Duration::from_millis(100));
    }
}
