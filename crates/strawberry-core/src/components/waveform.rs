//! # Waveform Peak Data Types
//!
//! Pure data types for pre-computed audio waveform peaks.
//! Lives in `strawberry-core` so both UI and audio crates can access them.
//!
//! ## Architecture
//!
//! - `WaveformPeaks`: Fixed-size peak array (512 entries, normalized 0.0–1.0)
//! - `WaveformCache`: HashMap<Uuid, Arc<WaveformPeaks>> for O(1) lookup
//!
//! Computation (`compute_peaks_from_frames()`) stays in `strawberry-audio`
//! since it needs kira's `Frame` type.

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Default number of peaks per clip.
///
/// 512 peaks at typical clip widths (200-800px) gives 0.4–1.6 peaks per pixel —
/// more than enough visual resolution. Higher values waste memory with no benefit.
pub const DEFAULT_PEAK_COUNT: usize = 512;

/// Pre-computed waveform peak data for a single audio clip.
///
/// Each peak represents the maximum absolute amplitude in a time window.
/// Values are normalized to [0.0, 1.0] where 1.0 = full-scale signal.
#[derive(Debug, Clone)]
pub struct WaveformPeaks {
    /// Peak amplitudes, normalized to [0.0, 1.0].
    /// Length is always `DEFAULT_PEAK_COUNT` (512).
    pub peaks: Vec<f32>,
    /// Sample rate of the source audio (for time mapping).
    pub sample_rate: u32,
    /// Total number of source frames (for duration calculation).
    pub total_frames: usize,
}

impl WaveformPeaks {
    /// Get the peak value at a normalized position (0.0–1.0).
    ///
    /// Returns 0.0 if out of range. Useful for rendering a clip
    /// that shows only a portion of the audio (source_in → source_out).
    #[inline]
    pub fn peak_at_normalized_position(&self, t: f32) -> f32 {
        if self.peaks.is_empty() {
            return 0.0;
        }
        let idx: usize = (t * self.peaks.len() as f32) as usize;
        self.peaks.get(idx).copied().unwrap_or(0.0)
    }
}

/// Thread-safe waveform cache keyed by asset UUID.
///
/// ## Performance
/// - O(1) HashMap lookup per visible clip
/// - `Arc<WaveformPeaks>` — zero-copy sharing
/// - `Clone` is cheap (only clones Arc pointers, not peak data)
/// - Never mutated in the render hot path
#[derive(Default, Clone)]
pub struct WaveformCache {
    cache: HashMap<Uuid, Arc<WaveformPeaks>>,
}

impl WaveformCache {
    /// Insert pre-computed peaks for an asset.
    pub fn insert(&mut self, asset_uuid: Uuid, peaks: WaveformPeaks) {
        self.cache.insert(asset_uuid, Arc::new(peaks));
    }

    /// Get peaks for an asset (O(1) lookup).
    pub fn get(&self, asset_uuid: &Uuid) -> Option<Arc<WaveformPeaks>> {
        self.cache.get(asset_uuid).cloned()
    }

    /// Remove peaks when a clip is unregistered.
    pub fn remove(&mut self, asset_uuid: &Uuid) {
        self.cache.remove(asset_uuid);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peak_at_normalized_position() {
        let mut peaks = WaveformPeaks {
            peaks: vec![0.0; 512],
            sample_rate: 44100,
            total_frames: 44100,
        };
        peaks.peaks[0] = 1.0;
        peaks.peaks[511] = 0.5;

        assert!((peaks.peak_at_normalized_position(0.0) - 1.0).abs() < f32::EPSILON);
        assert!(peaks.peak_at_normalized_position(0.5) == 0.0);
    }

    #[test]
    fn test_cache_insert_get_remove() {
        let mut cache = WaveformCache::default();
        let uuid = Uuid::new_v4();

        let peaks = WaveformPeaks {
            peaks: vec![0.5; 512],
            sample_rate: 44100,
            total_frames: 44100,
        };

        cache.insert(uuid, peaks);
        assert!(cache.get(&uuid).is_some());

        cache.remove(&uuid);
        assert!(cache.get(&uuid).is_none());
    }

    #[test]
    fn test_cache_clone_is_cheap() {
        let mut cache = WaveformCache::default();
        let uuid = Uuid::new_v4();

        let peaks = WaveformPeaks {
            peaks: vec![0.5; 512],
            sample_rate: 44100,
            total_frames: 44100,
        };

        cache.insert(uuid, peaks);
        let cloned = cache.clone();

        // Both should return the same Arc (same pointer)
        let original_arc = cache.get(&uuid).unwrap();
        let cloned_arc = cloned.get(&uuid).unwrap();
        assert!(Arc::ptr_eq(&original_arc, &cloned_arc));
    }
}
