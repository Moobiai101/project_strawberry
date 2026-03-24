//! # Audio Components
//!
//! ECS components for audio fading and advanced audio control.
//!
//! Separated from `mod.rs` to keep each file under 400 lines.
//! `AudioClip` remains in `mod.rs` for backward compatibility.

use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ============================================================================
// FADE CURVE
// ============================================================================

/// Shape of the audio fade curve.
///
/// Determines how volume ramps during fade-in or fade-out.
/// Pro editors typically default to `EaseInOut` (S-curve) for smooth transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FadeCurve {
    /// Constant-rate volume change (straight line 0→1 or 1→0)
    #[default]
    Linear,
    /// Slow start, accelerating end (quadratic ease-in)
    EaseIn,
    /// Fast start, decelerating end (quadratic ease-out)
    EaseOut,
    /// S-curve: slow start + slow end (smoothstep — industry standard)
    EaseInOut,
    /// Audio-natural logarithmic ramp (perceptually linear to human ear)
    Logarithmic,
}

impl FadeCurve {
    /// All variants for UI dropdown.
    pub const ALL: &'static [FadeCurve] = &[
        FadeCurve::Linear,
        FadeCurve::EaseIn,
        FadeCurve::EaseOut,
        FadeCurve::EaseInOut,
        FadeCurve::Logarithmic,
    ];

    /// Display name for UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear",
            FadeCurve::EaseIn => "Ease In",
            FadeCurve::EaseOut => "Ease Out",
            FadeCurve::EaseInOut => "Ease In/Out",
            FadeCurve::Logarithmic => "Logarithmic",
        }
    }

    /// Apply the curve shape to a normalized time `t` (0.0–1.0).
    ///
    /// Returns a value in [0.0, 1.0] that represents the fade amount.
    /// `t = 0.0` → start of fade, `t = 1.0` → end of fade.
    ///
    /// ## Performance
    /// Pure arithmetic — no allocations, no branching beyond the match.
    #[inline]
    pub fn apply(&self, t: f32) -> f32 {
        let t: f32 = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::EaseIn => t * t,
            FadeCurve::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            FadeCurve::EaseInOut => {
                // Hermite smoothstep: 3t² - 2t³
                t * t * (3.0 - 2.0 * t)
            }
            FadeCurve::Logarithmic => {
                // Attempt audio-natural curve: fast initial ramp,
                // slow approach to full volume.
                // Using x^0.5 (square root) as approximation.
                t.sqrt()
            }
        }
    }
}

// ============================================================================
// AUDIO FADE COMPONENT
// ============================================================================

/// Per-clip fade configuration.
///
/// Lazy-inserted (same pattern as `AudioClip`) — only clips with
/// explicitly configured fades have this component, zero overhead otherwise.
///
/// ## How It Works
///
/// The `audio_mixer_sync_system` checks for this component each frame.
/// Based on the clip's `source_time` relative to clip start/end:
/// - During fade-in: volume multiplied by `curve.apply(source_time / fade_in)`
/// - During fade-out: volume multiplied by `curve.apply((clip_end - source_time) / fade_out)`
/// - Outside fade regions: volume unaffected (multiplier = 1.0)
///
/// ## Performance
/// - One `f32` multiply per active clip per frame during fade regions
/// - Pure arithmetic in `compute_fade_multiplier` — no allocations
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct AudioFade {
    /// Fade-in duration (0 = no fade-in)
    pub fade_in: Duration,
    /// Fade-out duration (0 = no fade-out)
    pub fade_out: Duration,
    /// Curve shape for both fade-in and fade-out
    pub curve: FadeCurve,
}

impl Default for AudioFade {
    fn default() -> Self {
        Self {
            fade_in: Duration::ZERO,
            fade_out: Duration::ZERO,
            curve: FadeCurve::EaseInOut,
        }
    }
}

impl AudioFade {
    /// Create with fade-in only.
    pub fn fade_in(duration: Duration) -> Self {
        Self {
            fade_in: duration,
            ..Default::default()
        }
    }

    /// Create with fade-out only.
    pub fn fade_out(duration: Duration) -> Self {
        Self {
            fade_out: duration,
            ..Default::default()
        }
    }

    /// Create with both fades.
    pub fn both(fade_in: Duration, fade_out: Duration) -> Self {
        Self {
            fade_in,
            fade_out,
            ..Default::default()
        }
    }
}

// ============================================================================
// FADE MULTIPLIER COMPUTATION
// ============================================================================

/// Compute the fade volume multiplier for a clip at a given source time.
///
/// Returns `1.0` when outside fade regions, `0.0–1.0` during fades.
/// This is a **pure function** — no state, no allocations.
///
/// ## Arguments
/// - `source_time`: Current playback position within the clip's source media
/// - `source_in`: Clip's source-in point
/// - `clip_duration`: Total duration of the clip on the timeline
/// - `fade`: The fade configuration
///
/// ## Example
/// ```ignore
/// // Clip: source_in=0s, duration=10s, fade_in=1s, fade_out=2s
/// // At source_time = 0.5s → in fade-in zone → multiplier ≈ 0.5 (linear)
/// // At source_time = 5.0s → no fade → multiplier = 1.0
/// // At source_time = 9.0s → in fade-out zone → multiplier ≈ 0.5 (linear)
/// ```
#[inline]
pub fn compute_fade_multiplier(
    source_time: Duration,
    source_in: Duration,
    clip_duration: Duration,
    fade: &AudioFade,
) -> f32 {
    let elapsed_secs: f64 = source_time
        .saturating_sub(source_in)
        .as_secs_f64();
    let duration_secs: f64 = clip_duration.as_secs_f64();

    if duration_secs <= 0.0 {
        return 1.0;
    }

    // Fade-in: ramp from 0→1 during [0, fade_in]
    let fade_in_secs: f64 = fade.fade_in.as_secs_f64();
    if fade_in_secs > 0.0 && elapsed_secs < fade_in_secs {
        let t: f32 = (elapsed_secs / fade_in_secs) as f32;
        return fade.curve.apply(t);
    }

    // Fade-out: ramp from 1→0 during [duration - fade_out, duration]
    let fade_out_secs: f64 = fade.fade_out.as_secs_f64();
    if fade_out_secs > 0.0 {
        let fade_out_start: f64 = duration_secs - fade_out_secs;
        if elapsed_secs > fade_out_start {
            let remaining: f64 = duration_secs - elapsed_secs;
            let t: f32 = (remaining / fade_out_secs).clamp(0.0, 1.0) as f32;
            return fade.curve.apply(t);
        }
    }

    1.0 // Outside fade regions
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fade_curve_linear() {
        let curve = FadeCurve::Linear;
        assert!((curve.apply(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((curve.apply(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((curve.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fade_curve_ease_in() {
        let curve = FadeCurve::EaseIn;
        assert!((curve.apply(0.0) - 0.0).abs() < f32::EPSILON);
        // EaseIn (t²): at 0.5 → 0.25
        assert!((curve.apply(0.5) - 0.25).abs() < f32::EPSILON);
        assert!((curve.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fade_curve_ease_out() {
        let curve = FadeCurve::EaseOut;
        assert!((curve.apply(0.0) - 0.0).abs() < f32::EPSILON);
        // EaseOut: 1-(1-0.5)² = 1-0.25 = 0.75
        assert!((curve.apply(0.5) - 0.75).abs() < f32::EPSILON);
        assert!((curve.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fade_curve_smoothstep() {
        let curve = FadeCurve::EaseInOut;
        assert!((curve.apply(0.0) - 0.0).abs() < f32::EPSILON);
        // Smoothstep: 3(0.5)² - 2(0.5)³ = 0.75 - 0.25 = 0.5
        assert!((curve.apply(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((curve.apply(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fade_curve_clamps_input() {
        let curve = FadeCurve::Linear;
        assert!((curve.apply(-1.0) - 0.0).abs() < f32::EPSILON);
        assert!((curve.apply(2.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compute_fade_in_only() {
        let fade = AudioFade::fade_in(Duration::from_secs(1));
        let src_in = Duration::ZERO;
        let dur = Duration::from_secs(10);

        // At t=0: fully faded
        let m = compute_fade_multiplier(Duration::ZERO, src_in, dur, &fade);
        assert!(m < 0.01, "At start should be near-zero, got {}", m);

        // At t=0.5s: halfway through fade-in
        let m = compute_fade_multiplier(Duration::from_millis(500), src_in, dur, &fade);
        assert!(m > 0.2 && m < 0.8, "At mid-fade should be mid, got {}", m);

        // At t=2s: past fade-in, full volume
        let m = compute_fade_multiplier(Duration::from_secs(2), src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON, "Past fade should be 1.0, got {}", m);
    }

    #[test]
    fn test_compute_fade_out_only() {
        let fade = AudioFade::fade_out(Duration::from_secs(2));
        let src_in = Duration::ZERO;
        let dur = Duration::from_secs(10);

        // At t=5s: before fade-out, full volume
        let m = compute_fade_multiplier(Duration::from_secs(5), src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON, "Before fade-out should be 1.0, got {}", m);

        // At t=9s: in fade-out zone
        let m = compute_fade_multiplier(Duration::from_secs(9), src_in, dur, &fade);
        assert!(m > 0.2 && m < 0.8, "In fade-out should be mid, got {}", m);

        // At t=10s: end of clip, should approach 0
        let m = compute_fade_multiplier(Duration::from_secs(10), src_in, dur, &fade);
        assert!(m < 0.01, "At end should be near-zero, got {}", m);
    }

    #[test]
    fn test_compute_fade_both() {
        let fade = AudioFade::both(
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let src_in = Duration::ZERO;
        let dur = Duration::from_secs(10);

        // Fade-in region
        let m = compute_fade_multiplier(Duration::from_millis(500), src_in, dur, &fade);
        assert!(m > 0.0 && m < 1.0, "In fade-in should be partial, got {}", m);

        // Middle: full volume
        let m = compute_fade_multiplier(Duration::from_secs(5), src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON, "Middle should be 1.0, got {}", m);

        // Fade-out region
        let m = compute_fade_multiplier(Duration::from_millis(9500), src_in, dur, &fade);
        assert!(m > 0.0 && m < 1.0, "In fade-out should be partial, got {}", m);
    }

    #[test]
    fn test_compute_fade_with_source_offset() {
        let fade = AudioFade::fade_in(Duration::from_secs(1));
        let src_in = Duration::from_secs(5); // Clip starts at source 5s
        let dur = Duration::from_secs(10);

        // At source_time=5s (clip start): fully faded
        let m = compute_fade_multiplier(Duration::from_secs(5), src_in, dur, &fade);
        assert!(m < 0.01, "At clip start should be near-zero, got {}", m);

        // At source_time=6s (1s into clip): past fade
        let m = compute_fade_multiplier(Duration::from_secs(6), src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON, "Past fade should be 1.0, got {}", m);
    }

    #[test]
    fn test_no_fade_returns_unity() {
        let fade = AudioFade::default(); // Both durations = 0
        let src_in = Duration::ZERO;
        let dur = Duration::from_secs(10);

        let m = compute_fade_multiplier(Duration::ZERO, src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON);

        let m = compute_fade_multiplier(Duration::from_secs(5), src_in, dur, &fade);
        assert!((m - 1.0).abs() < f32::EPSILON);
    }
}
