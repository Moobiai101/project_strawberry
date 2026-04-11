//! # Keyframe Track — ECS Component
//!
//! Per-clip automation container that maps property IDs to sorted keyframe lists.
//! Lazy-inserted: only clips with active automation carry this component,
//! so clips without keyframes have zero overhead.
//!
//! ## Data Model
//!
//! ```text
//! KeyframeTrack
//!   └── HashMap<String, Vec<Keyframe>>
//!        ├── "volume" → [kf_0s, kf_1.5s, kf_3s]
//!        ├── "pan"    → [kf_0s, kf_2s]
//!        └── "opacity"→ [kf_0s]
//! ```
//!
//! ## Sort Invariant
//!
//! All `Vec<Keyframe>` lists are maintained in ascending `time_secs` order.
//! Every mutation method enforces this via insertion sort or re-sort.
//! Binary search in `evaluate_at_time()` depends on this invariant.
//!
//! ## Performance
//! - `evaluate`: O(log n) per property per frame
//! - `add_keyframe`: O(n) insertion (n typically < 20)
//! - `remove_keyframe`: O(n) scan + shift
//!
//! ## Well-Known Property IDs
//! | ID         | Range      | Default | Unit    |
//! |------------|------------|---------|---------|
//! | `"volume"` | 0.0 – 2.0  | 1.0     | linear  |
//! | `"pan"`    | -1.0 – 1.0 | 0.0     | linear  |
//! | `"opacity"`| 0.0 – 1.0  | 1.0     | linear  |

use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::keyframe::{evaluate_at_time, Keyframe};

// ============================================================================
// WELL-KNOWN PROPERTY IDS
// ============================================================================

/// Well-known property identifiers for keyframe automation.
/// Using constants prevents typo bugs and enables IDE autocomplete.
pub mod property_ids {
    /// Audio volume: 0.0 (silent) to 2.0 (200%). Default 1.0.
    pub const VOLUME: &str = "volume";
    /// Audio panning: -1.0 (full left) to 1.0 (full right). Default 0.0.
    pub const PAN: &str = "pan";
    /// Visual opacity: 0.0 (transparent) to 1.0 (opaque). Default 1.0.
    pub const OPACITY: &str = "opacity";
}

// ============================================================================
// KEYFRAME TRACK COMPONENT
// ============================================================================

/// Per-clip automation data — maps property IDs to sorted keyframe lists.
///
/// Lazy-inserted as an ECS component: clips without automation don't carry
/// this component at all, maintaining zero overhead for the common case.
///
/// ## Usage
/// ```ignore
/// // Read automation value at clip-relative time
/// if let Some(track) = world.get::<KeyframeTrack>(clip_entity) {
///     let volume: f32 = track.evaluate("volume", clip_local_time, 1.0);
///     let pan: f32 = track.evaluate("pan", clip_local_time, 0.0);
/// }
/// ```
#[derive(Component, Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyframeTrack {
    /// Property ID → sorted keyframe list.
    /// Keys are well-known property IDs (see `property_ids` module).
    tracks: HashMap<String, Vec<Keyframe>>,
}

impl KeyframeTrack {
    /// Create an empty keyframe track (no automation on any property).
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate the automation value for a property at a clip-relative time.
    ///
    /// Returns `default_value` if no keyframes exist for this property.
    /// Uses binary search: O(log n).
    #[inline]
    pub fn evaluate(&self, property_id: &str, time_secs: f64, default_value: f32) -> f32 {
        match self.tracks.get(property_id) {
            Some(keyframes) => evaluate_at_time(keyframes, time_secs, default_value),
            None => default_value,
        }
    }

    /// Check if this track has any keyframes for a property.
    pub fn has_keyframes(&self, property_id: &str) -> bool {
        self.tracks
            .get(property_id)
            .map(|kf| !kf.is_empty())
            .unwrap_or(false)
    }

    /// Check if this track has any automation at all (any property).
    pub fn is_empty(&self) -> bool {
        self.tracks.values().all(|kf| kf.is_empty())
    }

    /// Get the sorted keyframe list for a property (read-only).
    pub fn keyframes(&self, property_id: &str) -> Option<&[Keyframe]> {
        self.tracks.get(property_id).map(|v| v.as_slice())
    }

    /// Get the number of keyframes for a property.
    pub fn keyframe_count(&self, property_id: &str) -> usize {
        self.tracks.get(property_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Get all property IDs that have keyframes.
    pub fn animated_properties(&self) -> Vec<&str> {
        self.tracks
            .iter()
            .filter(|(_, kfs)| !kfs.is_empty())
            .map(|(id, _)| id.as_str())
            .collect()
    }

    // ========================================================================
    // MUTATION METHODS (used by Commands)
    // ========================================================================

    /// Add a keyframe, maintaining sort order by time.
    ///
    /// If a keyframe already exists at the exact same time (within 1μs tolerance),
    /// it is replaced. Returns the index where the keyframe was inserted.
    pub fn add_keyframe(&mut self, property_id: &str, keyframe: Keyframe) -> usize {
        let keyframes: &mut Vec<Keyframe> = self
            .tracks
            .entry(property_id.to_string())
            .or_default();

        // Check for existing keyframe at same time (replace semantics)
        const TIME_TOLERANCE: f64 = 1e-6;
        if let Some(existing_idx) = keyframes
            .iter()
            .position(|kf| (kf.time_secs - keyframe.time_secs).abs() < TIME_TOLERANCE)
        {
            keyframes[existing_idx] = keyframe;
            return existing_idx;
        }

        // Binary insertion to maintain sorted order
        let insert_idx: usize = keyframes
            .binary_search_by(|kf| {
                kf.time_secs
                    .partial_cmp(&keyframe.time_secs)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|pos| pos);

        keyframes.insert(insert_idx, keyframe);
        insert_idx
    }

    /// Remove a keyframe at a specific index for a property.
    ///
    /// Returns the removed keyframe, or None if the index is out of bounds.
    pub fn remove_keyframe_at(
        &mut self,
        property_id: &str,
        index: usize,
    ) -> Option<Keyframe> {
        let keyframes: &mut Vec<Keyframe> = self.tracks.get_mut(property_id)?;
        if index < keyframes.len() {
            Some(keyframes.remove(index))
        } else {
            None
        }
    }

    /// Remove a keyframe by matching time (within tolerance).
    ///
    /// Returns the removed keyframe and its index, or None if not found.
    pub fn remove_keyframe_by_time(
        &mut self,
        property_id: &str,
        time_secs: f64,
    ) -> Option<(usize, Keyframe)> {
        const TIME_TOLERANCE: f64 = 1e-6;
        let keyframes: &mut Vec<Keyframe> = self.tracks.get_mut(property_id)?;

        let idx: Option<usize> = keyframes
            .iter()
            .position(|kf| (kf.time_secs - time_secs).abs() < TIME_TOLERANCE);

        idx.map(|i| (i, keyframes.remove(i)))
    }

    /// Update a keyframe's time position, maintaining sort order.
    ///
    /// Returns the old keyframe data for undo, or None if index out of bounds.
    pub fn move_keyframe(
        &mut self,
        property_id: &str,
        index: usize,
        new_time_secs: f64,
    ) -> Option<Keyframe> {
        let keyframes: &mut Vec<Keyframe> = self.tracks.get_mut(property_id)?;
        if index >= keyframes.len() {
            return None;
        }

        let old_keyframe: Keyframe = keyframes[index].clone();
        keyframes[index].time_secs = new_time_secs;

        // Re-sort to maintain invariant (move is rare, n is small)
        keyframes.sort_by(|a, b| {
            a.time_secs
                .partial_cmp(&b.time_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Some(old_keyframe)
    }

    /// Insert a complete keyframe list for a property (used by undo/redo).
    /// Replaces any existing keyframes for this property.
    pub fn set_keyframes(&mut self, property_id: &str, keyframes: Vec<Keyframe>) {
        self.tracks.insert(property_id.to_string(), keyframes);
    }

    /// Remove all keyframes for a property, returning the old list.
    pub fn clear_property(&mut self, property_id: &str) -> Vec<Keyframe> {
        self.tracks.remove(property_id).unwrap_or_default()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::keyframe::KeyframeInterpolation;

    #[test]
    fn test_empty_track_returns_defaults() {
        let track: KeyframeTrack = KeyframeTrack::new();
        assert!((track.evaluate("volume", 0.0, 1.0) - 1.0).abs() < f32::EPSILON);
        assert!(track.is_empty());
        assert_eq!(track.keyframe_count("volume"), 0);
    }

    #[test]
    fn test_add_and_evaluate() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.5));

        assert_eq!(track.keyframe_count("volume"), 2);
        assert!(track.has_keyframes("volume"));

        // Midpoint of linear segment
        let v: f32 = track.evaluate("volume", 1.0, 1.0);
        assert!((v - 0.75).abs() < 0.01, "Expected ~0.75, got {v}");
    }

    #[test]
    fn test_sort_invariant_maintained() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        // Insert out of order
        track.add_keyframe("volume", Keyframe::new(3.0, 0.0));
        track.add_keyframe("volume", Keyframe::new(1.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.5));

        let kfs: &[Keyframe] = track.keyframes("volume").unwrap();
        assert_eq!(kfs.len(), 3);
        assert!((kfs[0].time_secs - 1.0).abs() < f64::EPSILON);
        assert!((kfs[1].time_secs - 2.0).abs() < f64::EPSILON);
        assert!((kfs[2].time_secs - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_replace_at_same_time() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(1.0, 0.5));
        track.add_keyframe("volume", Keyframe::new(1.0, 0.8)); // replace

        assert_eq!(track.keyframe_count("volume"), 1);
        let v: f32 = track.evaluate("volume", 1.0, 1.0);
        assert!((v - 0.8).abs() < 0.01, "Should be replaced to 0.8, got {v}");
    }

    #[test]
    fn test_remove_by_index() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("pan", Keyframe::new(0.0, -1.0));
        track.add_keyframe("pan", Keyframe::new(1.0, 1.0));

        let removed: Option<Keyframe> = track.remove_keyframe_at("pan", 0);
        assert!(removed.is_some());
        assert_eq!(track.keyframe_count("pan"), 1);
        assert!((track.evaluate("pan", 0.5, 0.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_remove_by_time() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(1.0, 0.5));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.0));

        let result = track.remove_keyframe_by_time("volume", 1.0);
        assert!(result.is_some());
        let (idx, kf) = result.unwrap();
        assert_eq!(idx, 1);
        assert!((kf.value - 0.5).abs() < f32::EPSILON);
        assert_eq!(track.keyframe_count("volume"), 2);
    }

    #[test]
    fn test_move_keyframe() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(1.0, 0.5));
        track.add_keyframe("volume", Keyframe::new(2.0, 0.0));

        // Move keyframe at index 1 (t=1.0) to t=1.5
        let old: Option<Keyframe> = track.move_keyframe("volume", 1, 1.5);
        assert!(old.is_some());

        // Verify re-sorted
        let kfs: &[Keyframe] = track.keyframes("volume").unwrap();
        assert!((kfs[0].time_secs - 0.0).abs() < f64::EPSILON);
        assert!((kfs[1].time_secs - 1.5).abs() < f64::EPSILON);
        assert!((kfs[2].time_secs - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_multiple_properties() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("pan", Keyframe::new(0.0, -0.5));

        assert!(track.has_keyframes("volume"));
        assert!(track.has_keyframes("pan"));
        assert!(!track.has_keyframes("opacity"));
        assert!(!track.is_empty());

        let props: Vec<&str> = track.animated_properties();
        assert_eq!(props.len(), 2);
    }

    #[test]
    fn test_clear_property() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));
        track.add_keyframe("volume", Keyframe::new(1.0, 0.5));

        let old: Vec<Keyframe> = track.clear_property("volume");
        assert_eq!(old.len(), 2);
        assert!(!track.has_keyframes("volume"));
    }

    #[test]
    fn test_set_keyframes_replaces() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe("volume", Keyframe::new(0.0, 1.0));

        let new_kfs: Vec<Keyframe> = vec![
            Keyframe::new(0.0, 0.0),
            Keyframe::new(5.0, 2.0),
        ];
        track.set_keyframes("volume", new_kfs);

        assert_eq!(track.keyframe_count("volume"), 2);
        let v: f32 = track.evaluate("volume", 2.5, 1.0);
        assert!((v - 1.0).abs() < 0.01, "Expected ~1.0, got {v}");
    }

    #[test]
    fn test_evaluate_with_hold_interpolation() {
        let mut track: KeyframeTrack = KeyframeTrack::new();
        track.add_keyframe(
            "volume",
            Keyframe::with_interpolation(0.0, 1.0, KeyframeInterpolation::Hold),
        );
        track.add_keyframe("volume", Keyframe::new(2.0, 0.0));

        // Should hold at 1.0 until the next keyframe
        let v: f32 = track.evaluate("volume", 1.0, 1.0);
        assert!((v - 1.0).abs() < 0.01, "Hold should be 1.0, got {v}");
    }
}
