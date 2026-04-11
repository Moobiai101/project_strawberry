//! # Track Audio Component
//!
//! Per-track audio mixing parameters (volume, pan).
//!
//! ## Design
//!
//! `TrackAudio` is lazy-inserted — only tracks that need custom audio mixing
//! get this component. Tracks WITHOUT `TrackAudio` default to volume=1.0, pan=0.0,
//! which means zero overhead for tracks that don't use track-level mixing.
//!
//! ## How It Interacts With Clip Audio
//!
//! The final audio volume for a clip is:
//! ```text
//! final_volume = clip_volume × fade_multiplier × crossfade_mult × track_volume
//! final_pan    = clip_pan + track_pan  (clamped to ±1.0)
//! ```
//!
//! This matches industry NLE behavior (Premiere, DaVinci, CapCut):
//! - Clip-level controls affect individual clips
//! - Track-level controls affect ALL clips on that track simultaneously

use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};

// ============================================================================
// TRACK AUDIO COMPONENT
// ============================================================================

/// Per-track audio mixing parameters.
///
/// Attached to track entities that need custom volume/pan.
/// Tracks without this component default to volume=1.0, pan=0.0
/// in the audio mixer sync system.
///
/// ## Volume Range
///
/// - `0.0` = silent
/// - `1.0` = unity gain (100%, default)
/// - `2.0` = maximum boost (200%)
///
/// ## Pan Range
///
/// - `-1.0` = hard left
/// - `0.0` = center (default)
/// - `1.0` = hard right
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct TrackAudio {
    /// Track volume multiplier (0.0–2.0, default 1.0)
    pub volume: f32,
    /// Stereo pan (-1.0 left, 0.0 center, 1.0 right)
    pub pan: f32,
}

impl Default for TrackAudio {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
        }
    }
}

impl TrackAudio {
    /// Create with custom volume (pan defaults to center)
    pub fn with_volume(volume: f32) -> Self {
        Self {
            volume: volume.clamp(0.0, 2.0),
            pan: 0.0,
        }
    }

    /// Create with custom volume and pan
    pub fn with_volume_and_pan(volume: f32, pan: f32) -> Self {
        Self {
            volume: volume.clamp(0.0, 2.0),
            pan: pan.clamp(-1.0, 1.0),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_audio_default() {
        let audio: TrackAudio = TrackAudio::default();
        assert!((audio.volume - 1.0).abs() < f32::EPSILON);
        assert!((audio.pan - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_track_audio_volume_clamping() {
        let audio: TrackAudio = TrackAudio::with_volume(5.0);
        assert!((audio.volume - 2.0).abs() < f32::EPSILON); // Clamped to max

        let audio: TrackAudio = TrackAudio::with_volume(-1.0);
        assert!((audio.volume - 0.0).abs() < f32::EPSILON); // Clamped to min
    }

    #[test]
    fn test_track_audio_pan_clamping() {
        let audio: TrackAudio = TrackAudio::with_volume_and_pan(1.0, 2.0);
        assert!((audio.pan - 1.0).abs() < f32::EPSILON); // Clamped to max

        let audio: TrackAudio = TrackAudio::with_volume_and_pan(1.0, -5.0);
        assert!((audio.pan - -1.0).abs() < f32::EPSILON); // Clamped to min
    }
}
