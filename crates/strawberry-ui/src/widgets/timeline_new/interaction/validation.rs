//! Movement validation chain for timeline drag operations.
//!
//! Implements Olive Editor's 5-stage validation pattern (`pointer.cpp:948-1019`).
//! All functions are **pure** — they take current state and a proposed value,
//! returning the *clamped* value. No side effects, no UI dependencies.
//!
//! ## Validation chain during drag update
//!
//! ```text
//! 1. validate_move_time()     — clamp so no clip goes < t=0
//! 2. snap_to_frame()          — snap to nearest frame boundary
//! 3. validate_trim_in()       — ensure trim keeps ≥ 1 frame
//! 4. validate_trim_out()      — ensure trim keeps ≥ 1 frame
//! 5. validate_track_index()   — clamp within valid track range
//! ```

use std::time::Duration;

// ============================================================================
// MOVE VALIDATION
// ============================================================================

/// Clamp a proposed ghost time so the clip doesn't start before t=0.
///
/// `proposed_time` is the raw ghost position from pointer → time conversion.
/// Returns the clamped time (≥ 0).
pub fn validate_move_time(proposed_time: Duration) -> Duration {
    // Duration is unsigned, so it naturally can't go negative.
    // This function exists for API consistency and to guard against
    // future refactors that might switch to f64.
    proposed_time
}

/// Clamp a proposed ghost time for a *group* of clips being moved.
///
/// When multiple clips are selected, the delta must be clamped so the
/// *earliest* clip in the group doesn't go below t=0.
///
/// Returns the clamped delta (seconds). Negative delta = move left.
pub fn validate_group_move_delta(earliest_start_secs: f64, proposed_delta_secs: f64) -> f64 {
    // If moving left, don't let it go past -earliest_start
    if proposed_delta_secs < -earliest_start_secs {
        -earliest_start_secs
    } else {
        proposed_delta_secs
    }
}

// ============================================================================
// FRAME SNAPPING
// ============================================================================

/// Snap a Duration to the nearest frame boundary.
///
/// `frame_rate` is fps (e.g. 30). Returns the snapped Duration.
pub fn snap_to_frame(time: Duration, frame_rate: f64) -> Duration {
    if frame_rate <= 0.0 {
        return time;
    }
    let frame_duration_secs: f64 = 1.0 / frame_rate;
    let time_secs: f64 = time.as_secs_f64();
    let frame_index: f64 = (time_secs / frame_duration_secs).round();
    let snapped_secs: f64 = frame_index * frame_duration_secs;
    Duration::from_secs_f64(snapped_secs.max(0.0))
}

// ============================================================================
// TRIM VALIDATION
// ============================================================================

/// Minimum clip duration: 10 frames at the given frame rate.
///
/// We use 10 frames (not 1) so the clip remains visually wide enough
/// for the user to grab the trim handles at typical zoom levels.
/// At 30fps this is ~333ms ≈ 33px at 100px/s zoom.
const MIN_FRAMES: f64 = 10.0;

fn min_clip_duration(frame_rate: f64) -> Duration {
    if frame_rate <= 0.0 {
        return Duration::from_millis(333); // Fallback ~30fps × 10 frames
    }
    Duration::from_secs_f64(MIN_FRAMES / frame_rate)
}

/// Validate an in-trim (left edge drag).
///
/// `proposed_start` is the new timeline_start from the ghost.
/// `original_end` is `original_start + original_duration` (the right edge, fixed).
///
/// Returns the clamped start time such that:
/// - start ≥ 0
/// - remaining duration ≥ 1 frame
pub fn validate_trim_in(
    proposed_start: Duration,
    original_end: Duration,
    frame_rate: f64,
) -> Duration {
    let min_dur: Duration = min_clip_duration(frame_rate);

    // Can't start past (end - 1 frame)
    let max_start: Duration = original_end.saturating_sub(min_dur);

    // Clamp: proposed_start must be in [0, max_start]
    if proposed_start > max_start {
        max_start
    } else {
        proposed_start
    }
}

/// Validate an out-trim (right edge drag).
///
/// `proposed_duration` is the new duration from the ghost.
/// `timeline_start` is the clip's fixed start time.
///
/// Returns the clamped duration such that:
/// - duration ≥ 1 frame
pub fn validate_trim_out(proposed_duration: Duration, frame_rate: f64) -> Duration {
    let min_dur: Duration = min_clip_duration(frame_rate);

    if proposed_duration < min_dur {
        min_dur
    } else {
        proposed_duration
    }
}

// ============================================================================
// TRACK VALIDATION
// ============================================================================

/// Validate track index during vertical drag.
///
/// `proposed_track` is the track index from pointer y-position.
/// `max_track_index` is the highest valid track index.
///
/// Returns the clamped track index in [0, max_track_index].
pub fn validate_track_index(proposed_track: u32, max_track_index: u32) -> u32 {
    proposed_track.min(max_track_index)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const FPS_30: f64 = 30.0;
    const FPS_24: f64 = 24.0;

    // --- Frame snapping ---

    #[test]
    fn test_snap_to_frame_exact_boundary() {
        // 1.0s is exactly frame 30 at 30fps
        let snapped = snap_to_frame(Duration::from_secs(1), FPS_30);
        assert_eq!(snapped, Duration::from_secs(1));
    }

    #[test]
    fn test_snap_to_frame_rounds_to_nearest() {
        // 0.5167s ≈ frame 15.5 → should snap to frame 16 = 0.5333s
        let snapped = snap_to_frame(Duration::from_secs_f64(0.5167), FPS_30);
        let expected: f64 = 16.0 / 30.0;
        assert!(
            (snapped.as_secs_f64() - expected).abs() < 1e-9,
            "Expected ~{:.6}, got {:.6}",
            expected,
            snapped.as_secs_f64()
        );
    }

    #[test]
    fn test_snap_to_frame_zero() {
        let snapped = snap_to_frame(Duration::ZERO, FPS_30);
        assert_eq!(snapped, Duration::ZERO);
    }

    #[test]
    fn test_snap_to_frame_24fps() {
        // Frame 12 at 24fps = 0.5s
        let snapped = snap_to_frame(Duration::from_secs_f64(0.48), FPS_24);
        let expected: f64 = 12.0 / 24.0;
        assert!(
            (snapped.as_secs_f64() - expected).abs() < 1e-9,
            "Expected ~{:.6}, got {:.6}",
            expected,
            snapped.as_secs_f64()
        );
    }

    // --- Trim in validation ---

    #[test]
    fn test_validate_trim_in_normal() {
        // Clip ends at 5s, proposing start at 2s → valid
        let result = validate_trim_in(Duration::from_secs(2), Duration::from_secs(5), FPS_30);
        assert_eq!(result, Duration::from_secs(2));
    }

    #[test]
    fn test_validate_trim_in_clamps_to_min_duration() {
        // Clip ends at 5s, proposing start at 5s → clamped to (5 - 10/30)
        let result = validate_trim_in(Duration::from_secs(5), Duration::from_secs(5), FPS_30);
        let expected = Duration::from_secs(5).saturating_sub(min_clip_duration(FPS_30));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_validate_trim_in_clamps_past_end() {
        // Clip ends at 3s, proposing start at 10s → clamped
        let result = validate_trim_in(Duration::from_secs(10), Duration::from_secs(3), FPS_30);
        let expected = Duration::from_secs(3).saturating_sub(min_clip_duration(FPS_30));
        assert_eq!(result, expected);
    }

    // --- Trim out validation ---

    #[test]
    fn test_validate_trim_out_normal() {
        let result = validate_trim_out(Duration::from_secs(3), FPS_30);
        assert_eq!(result, Duration::from_secs(3));
    }

    #[test]
    fn test_validate_trim_out_clamps_to_min_duration() {
        let result = validate_trim_out(Duration::ZERO, FPS_30);
        assert_eq!(result, min_clip_duration(FPS_30));
    }

    #[test]
    fn test_validate_trim_out_tiny_duration() {
        // 100ms < 10 frames at 30fps (333ms) → clamped
        let result = validate_trim_out(Duration::from_millis(100), FPS_30);
        assert_eq!(result, min_clip_duration(FPS_30));
    }

    // --- Track validation ---

    #[test]
    fn test_validate_track_within_range() {
        assert_eq!(validate_track_index(2, 5), 2);
    }

    #[test]
    fn test_validate_track_clamps_to_max() {
        assert_eq!(validate_track_index(10, 5), 5);
    }

    #[test]
    fn test_validate_track_at_zero() {
        assert_eq!(validate_track_index(0, 5), 0);
    }

    // --- Group move delta ---

    #[test]
    fn test_validate_group_move_delta_normal() {
        // Earliest at 5s, delta -3s → valid
        assert_eq!(validate_group_move_delta(5.0, -3.0), -3.0);
    }

    #[test]
    fn test_validate_group_move_delta_clamps() {
        // Earliest at 2s, delta -5s → clamped to -2s
        assert_eq!(validate_group_move_delta(2.0, -5.0), -2.0);
    }

    #[test]
    fn test_validate_group_move_delta_positive_always_valid() {
        // Moving right is always valid
        assert_eq!(validate_group_move_delta(0.0, 5.0), 5.0);
    }
}
