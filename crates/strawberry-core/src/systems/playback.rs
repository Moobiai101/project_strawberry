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
