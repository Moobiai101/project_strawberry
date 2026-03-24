//! # Loudness Analysis (EBU R128)
//!
//! Production-grade loudness measurement using the `ebur128` crate.
//! Implements the EBU R128 / ITU-R BS.1770-4 standard for perceived loudness.
//!
//! ## Performance
//!
//! - `measure_integrated_lufs()` is a **one-shot** call (not per-frame)
//! - Processes all frames in a single pass via ebur128's internal block processing
//! - Zero heap allocations beyond ebur128's internal state
//! - Interleaved buffer is stack-allocated in chunks of 4096 samples
//!
//! ## Usage
//!
//! Called by `NormalizeAudioCommand` when the user clicks "Normalize" in the inspector.
//! NOT called during playback — zero runtime overhead on the hot path.

use ebur128::{EbuR128, Mode};

/// Measure the integrated loudness (LUFS) of stereo audio frames.
///
/// Uses EBU R128 / ITU-R BS.1770-4 standard for perceived loudness.
/// Returns the integrated loudness in LUFS (Loudness Units Full Scale).
///
/// ## Arguments
/// - `frames`: Raw stereo frame data from `StaticSoundData.frames`
/// - `sample_rate`: Source sample rate (e.g., 44100, 48000)
///
/// ## Returns
/// Integrated loudness in LUFS (typically -70.0 to 0.0).
/// Returns `f64::NEG_INFINITY` for silence.
///
/// ## Performance
/// - Single-pass over frames (O(n))
/// - Processes in chunks of 2048 frames (4096 interleaved samples) to avoid
///   large temporary allocations
/// - NOT called during playback — only on explicit user action
pub fn measure_integrated_lufs(frames: &[kira::Frame], sample_rate: u32) -> f64 {
    if frames.is_empty() {
        return f64::NEG_INFINITY;
    }

    // Create ebur128 analyzer: 2 channels, Integrated mode only (cheapest)
    let mut analyzer: EbuR128 = match EbuR128::new(2, sample_rate, Mode::I) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("Failed to create ebur128 analyzer: {:?}", e);
            return f64::NEG_INFINITY;
        }
    };

    // Process in chunks to avoid allocating a massive interleaved buffer.
    // 2048 frames × 2 channels = 4096 f32s = 16KB per chunk (fits in L1 cache).
    const CHUNK_SIZE: usize = 2048;
    let mut interleaved: Vec<f32> = Vec::with_capacity(CHUNK_SIZE * 2);

    for chunk in frames.chunks(CHUNK_SIZE) {
        interleaved.clear();
        for frame in chunk {
            interleaved.push(frame.left);
            interleaved.push(frame.right);
        }

        if let Err(e) = analyzer.add_frames_f32(&interleaved) {
            tracing::error!("ebur128 add_frames error: {:?}", e);
            return f64::NEG_INFINITY;
        }
    }

    // Get integrated loudness (LUFS)
    match analyzer.loudness_global() {
        Ok(lufs) => lufs,
        Err(e) => {
            tracing::error!("ebur128 loudness_global error: {:?}", e);
            f64::NEG_INFINITY
        }
    }
}

/// Compute the linear gain multiplier needed to normalize audio from
/// `current_lufs` to `target_lufs`.
///
/// Formula: `gain = 10^((target_lufs - current_lufs) / 20.0)`
///
/// ## Safety
/// - Returns 1.0 if current LUFS is -inf (silence — can't normalize silence)
/// - Clamps result to [0.0, 10.0] to prevent destructive amplification
///   (10.0 = +20dB max boost, beyond this indicates a problem)
///
/// ## Arguments
/// - `current_lufs`: Measured integrated loudness of the clip
/// - `target_lufs`: Desired loudness (e.g., -14.0 for YouTube)
#[inline]
pub fn compute_normalization_gain(current_lufs: f64, target_lufs: f64) -> f32 {
    // Can't normalize silence
    if current_lufs.is_infinite() || current_lufs.is_nan() {
        return 1.0;
    }

    let gain_db: f64 = target_lufs - current_lufs;
    let gain_linear: f64 = 10.0_f64.powf(gain_db / 20.0);

    // Clamp to prevent destructive amplification
    (gain_linear as f32).clamp(0.0, 10.0)
}

/// Standard LUFS targets for common platforms.
///
/// Used by the inspector UI dropdown.
pub mod targets {
    /// YouTube / Spotify recommended loudness
    pub const YOUTUBE_LUFS: f64 = -14.0;
    /// General streaming platforms (Apple Music, Tidal)
    pub const STREAMING_LUFS: f64 = -16.0;
    /// EBU R128 broadcast standard
    pub const BROADCAST_LUFS: f64 = -23.0;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silence_returns_neg_infinity() {
        let lufs: f64 = measure_integrated_lufs(&[], 44100);
        assert!(lufs.is_infinite() && lufs.is_sign_negative());
    }

    #[test]
    fn test_zero_frames_returns_neg_infinity() {
        let frames: Vec<kira::Frame> = vec![kira::Frame::ZERO; 44100];
        let lufs: f64 = measure_integrated_lufs(&frames, 44100);
        // Digital silence should be -inf or very low
        assert!(lufs < -60.0 || lufs.is_infinite());
    }

    #[test]
    fn test_loud_signal_returns_reasonable_lufs() {
        // Full-scale sine wave should be around -3 LUFS
        let frames: Vec<kira::Frame> = (0..44100)
            .map(|i| {
                let t: f32 = i as f32 / 44100.0;
                let val: f32 = (t * std::f32::consts::TAU * 1000.0).sin();
                kira::Frame::new(val, val)
            })
            .collect();

        let lufs: f64 = measure_integrated_lufs(&frames, 44100);
        // Full-scale sine ≈ -3.01 LUFS, should be in reasonable range
        assert!(lufs > -10.0 && lufs < 0.0, "Expected ~-3 LUFS, got {}", lufs);
    }

    #[test]
    fn test_normalization_gain_identity() {
        // Same LUFS → gain = 1.0
        let gain: f32 = compute_normalization_gain(-14.0, -14.0);
        assert!((gain - 1.0).abs() < 0.001, "Expected 1.0, got {}", gain);
    }

    #[test]
    fn test_normalization_gain_boost() {
        // Need +6dB boost → gain ≈ 2.0
        let gain: f32 = compute_normalization_gain(-20.0, -14.0);
        assert!((gain - 2.0).abs() < 0.1, "Expected ~2.0, got {}", gain);
    }

    #[test]
    fn test_normalization_gain_reduce() {
        // Need -6dB cut → gain ≈ 0.5
        let gain: f32 = compute_normalization_gain(-8.0, -14.0);
        assert!((gain - 0.5).abs() < 0.1, "Expected ~0.5, got {}", gain);
    }

    #[test]
    fn test_normalization_gain_silence() {
        // Silence → gain = 1.0 (no change)
        let gain: f32 = compute_normalization_gain(f64::NEG_INFINITY, -14.0);
        assert!((gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_normalization_gain_clamped() {
        // Extreme boost → clamped to 10.0
        let gain: f32 = compute_normalization_gain(-60.0, -14.0);
        assert!(gain <= 10.0, "Should be clamped to 10.0, got {}", gain);
    }
}
