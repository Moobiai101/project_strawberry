//! # Waveform Peak Computation
//!
//! Computes audio peak data from kira's `StaticSoundData` for waveform rendering.
//!
//! ## Architecture
//!
//! Data types (`WaveformPeaks`, `WaveformCache`) live in `strawberry-core`
//! so both UI and audio crates can reference them.
//! This module provides the computation function that needs kira's `Frame` type.
//!
//! ## Performance
//!
//! - Single O(n) pass over frames — no repeated iteration
//! - Direct `Arc<[Frame]>` slice access — no function call overhead
//! - No heap allocations beyond the output Vec

pub use strawberry_core::components::waveform::{WaveformCache, WaveformPeaks, DEFAULT_PEAK_COUNT};

/// Compute waveform peaks from raw kira Frame data.
///
/// Divides frames into `peak_count` equal buckets, takes the maximum
/// absolute amplitude (mono-mixed) from each bucket. Results are
/// normalized to [0.0, 1.0].
///
/// ## Performance
/// - Single O(n) pass over frames — no repeated iteration
/// - Direct slice access (frames is `&[Frame]`, no per-call overhead)
/// - No heap allocations beyond the output Vec
///
/// ## Arguments
/// - `frames`: Raw stereo frame data from `StaticSoundData.frames`
/// - `sample_rate`: Source sample rate
/// - `peak_count`: Number of peaks to generate (default: 512)
pub fn compute_peaks_from_frames(
    frames: &[kira::Frame],
    sample_rate: u32,
    peak_count: usize,
) -> WaveformPeaks {
    let total_frames: usize = frames.len();

    if total_frames == 0 || peak_count == 0 {
        return WaveformPeaks {
            peaks: vec![0.0; peak_count],
            sample_rate,
            total_frames,
        };
    }

    let frames_per_peak: f64 = total_frames as f64 / peak_count as f64;
    let mut peaks: Vec<f32> = Vec::with_capacity(peak_count);

    // Track global max for normalization
    let mut global_max: f32 = 0.0;

    for i in 0..peak_count {
        let start: usize = (i as f64 * frames_per_peak) as usize;
        let end: usize = ((i + 1) as f64 * frames_per_peak).ceil() as usize;
        let end: usize = end.min(total_frames);

        let mut bucket_max: f32 = 0.0;

        // Direct slice iteration — no function call overhead per sample
        for frame in &frames[start..end] {
            // Mono-mix: average of abs(left) and abs(right)
            let amplitude: f32 = (frame.left.abs() + frame.right.abs()) * 0.5;
            if amplitude > bucket_max {
                bucket_max = amplitude;
            }
        }

        if bucket_max > global_max {
            global_max = bucket_max;
        }
        peaks.push(bucket_max);
    }

    // Normalize to [0.0, 1.0] relative to the loudest peak
    if global_max > f32::EPSILON {
        for peak in &mut peaks {
            *peak /= global_max;
        }
    }

    WaveformPeaks {
        peaks,
        sample_rate,
        total_frames,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_frames(count: usize, amplitude: f32) -> Vec<kira::Frame> {
        (0..count)
            .map(|i| {
                let t: f32 = i as f32 / count as f32;
                let val: f32 = (t * std::f32::consts::TAU * 4.0).sin() * amplitude;
                kira::Frame::new(val, val)
            })
            .collect()
    }

    #[test]
    fn test_compute_peaks_basic() {
        let frames = make_test_frames(44100, 0.8); // 1 second at 44.1kHz
        let peaks = compute_peaks_from_frames(&frames, 44100, 512);

        assert_eq!(peaks.peaks.len(), 512);
        assert_eq!(peaks.sample_rate, 44100);
        assert_eq!(peaks.total_frames, 44100);

        // All peaks should be in [0.0, 1.0] (normalized)
        for &p in &peaks.peaks {
            assert!(p >= 0.0 && p <= 1.0, "Peak {} out of range", p);
        }

        // At least some peaks should be non-zero (sine wave)
        let non_zero: usize = peaks.peaks.iter().filter(|&&p| p > 0.01).count();
        assert!(non_zero > 100, "Expected many non-zero peaks, got {}", non_zero);
    }

    #[test]
    fn test_compute_peaks_empty() {
        let peaks = compute_peaks_from_frames(&[], 44100, 512);
        assert_eq!(peaks.peaks.len(), 512);
        assert!(peaks.peaks.iter().all(|&p| p == 0.0));
    }

    #[test]
    fn test_compute_peaks_silence() {
        let frames: Vec<kira::Frame> = vec![kira::Frame::ZERO; 44100];
        let peaks = compute_peaks_from_frames(&frames, 44100, 512);
        assert!(peaks.peaks.iter().all(|&p| p == 0.0));
    }

    #[test]
    fn test_normalization() {
        // Frames with known max amplitude of 0.5
        let frames: Vec<kira::Frame> = vec![
            kira::Frame::new(0.5, 0.5),
            kira::Frame::new(0.25, 0.25),
            kira::Frame::new(0.1, 0.1),
            kira::Frame::new(0.5, 0.5),
        ];
        let peaks = compute_peaks_from_frames(&frames, 44100, 2);

        // First bucket: max of 0.5 and 0.25 → 0.5 → normalized to 1.0
        assert!((peaks.peaks[0] - 1.0).abs() < f32::EPSILON);
        // Second bucket: max of 0.1 and 0.5 → 0.5 → normalized to 1.0
        assert!((peaks.peaks[1] - 1.0).abs() < f32::EPSILON);
    }
}
