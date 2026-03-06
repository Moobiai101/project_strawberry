//! # Clip Node
//!
//! Node implementation for timeline clips.
//!
//! Wraps the ECS `Clip` component and provides time remapping logic
//! based on Olive's `ClipBlock::SequenceToMediaTime` algorithm.

use crate::components::timeline::ClipLoopMode;
use crate::components::{Clip, StableId};
use crate::node::value::FootageRef;
use crate::node::{
    InputDefinition, InputFlags, Node, NodeCategory, NodeGlobals, NodeValue, NodeValueRow,
    NodeValueTable,
};

use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// CLIP ASSET REF COMPONENT
// ============================================================================

/// Links a Clip entity to its source video asset.
///
/// Attach this to clip entities to enable video playback.
/// The `asset_uuid` is resolved by `VideoAssetManager` to get frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bevy_ecs::component::Component)]
pub struct ClipAssetRef {
    /// UUID of the source video asset in VideoAssetManager
    pub asset_uuid: Uuid,
    /// Original filename (for display in UI)
    pub filename: String,
}

impl ClipAssetRef {
    /// Create a new clip asset reference
    pub fn new(asset_uuid: Uuid, filename: impl Into<String>) -> Self {
        Self {
            asset_uuid,
            filename: filename.into(),
        }
    }
}

// ============================================================================
// TIME REMAP FLAGS
// ============================================================================

bitflags::bitflags! {
    /// Flags for time remapping calculations.
    ///
    /// Based on Olive's `SequenceToMediaTime` flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TimeRemapFlags: u32 {
        /// Ignore speed multiplier
        const IGNORE_SPEED = 0b00000001;
        /// Ignore reverse playback
        const IGNORE_REVERSE = 0b00000010;
        /// Ignore loop mode
        const IGNORE_LOOP = 0b00000100;
    }
}

// ============================================================================
// CLIP NODE
// ============================================================================

/// Node wrapper for a timeline clip.
///
/// ClipNode reads from ECS `Clip` and `ClipAssetRef` components,
/// implementing the `Node` trait for the rendering pipeline.
///
/// # Time Remapping
///
/// Converts timeline time to source media time, handling:
/// - Source in/out points
/// - Speed multiplier (Phase 2)
/// - Reverse playback (Phase 2)
/// - Loop mode (Phase 2)
pub struct ClipNode {
    /// Stable ID of this clip entity
    stable_id: StableId,
    /// The clip component data
    clip: Clip,
    /// Asset reference (if video clip)
    asset_ref: Option<ClipAssetRef>,
    /// Speed multiplier (1.0 = normal, 2.0 = 2x fast)
    speed: f64,
    /// Reverse playback
    reverse: bool,
    /// Per-clip loop mode for source media
    loop_mode: ClipLoopMode,
}

impl ClipNode {
    /// Create a new ClipNode from ECS components.
    ///
    /// # Arguments
    ///
    /// * `stable_id` - The clip's stable ID component
    /// * `clip` - The clip component
    /// * `asset_ref` - Optional asset reference (None for audio-only clips)
    pub fn new(stable_id: StableId, clip: Clip, asset_ref: Option<ClipAssetRef>) -> Self {
        Self {
            stable_id,
            clip,
            asset_ref,
            speed: 1.0,
            reverse: false,
            loop_mode: ClipLoopMode::None,
        }
    }

    /// Set speed multiplier
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Set reverse playback
    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Set per-clip loop mode for source media playback
    pub fn with_loop_mode(mut self, loop_mode: ClipLoopMode) -> Self {
        self.loop_mode = loop_mode;
        self
    }

    /// Get speed multiplier
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Get reverse flag
    pub fn is_reverse(&self) -> bool {
        self.reverse
    }

    /// Get loop mode
    pub fn loop_mode(&self) -> ClipLoopMode {
        self.loop_mode
    }

    /// Returns true when this clip requires random-access decode instead of
    /// the sequential pipe decoder.
    ///
    /// ## When Random-Access is Required
    ///
    /// The pipe decoder streams frames sequentially from a running FFmpeg
    /// process. It works perfectly for 1.0x forward playback but cannot
    /// handle non-sequential or speed-adjusted access patterns:
    ///
    /// - **Speed ≠ 1.0**: When speed changes mid-playback, the pipe decoder's
    ///   buffer position is stale (e.g., buffer at 5s, but source_time jumps to
    ///   15s at 3x speed). Re-seeking FFmpeg takes ~200-500ms, causing an
    ///   infinite re-seek loop that freezes the viewport.
    /// - **Reverse playback**: source_time goes backward.
    /// - **Loop mode**: source_time wraps from `source_out` back to `source_in`.
    ///
    /// ## Why JPEG Proxy Works
    ///
    /// The random-access path uses the JPEG proxy image sequence (pre-rendered
    /// to disk during import) + LRU cache. Frame lookups take ~2-5ms from disk
    /// or ~0.01ms from cache — fast enough for 30fps+ playback at any speed.
    ///
    /// This matches the architecture used by Olive, DaVinci Resolve, and
    /// Premiere Pro: pre-rendered frame caches with random-access lookup.
    pub fn needs_random_access(&self) -> bool {
        let speed_changed = (self.speed - 1.0).abs() > f64::EPSILON;
        speed_changed || self.reverse || self.loop_mode != ClipLoopMode::None
    }

    /// Get the clip's timeline start position
    pub fn timeline_start(&self) -> Duration {
        self.clip.timeline_start
    }

    /// Get the clip's timeline end position, accounting for speed.
    ///
    /// At speed > 1.0 the clip plays through its source media faster,
    /// so its effective timeline footprint shrinks:
    ///   `effective_end = timeline_start + (media_duration / speed)`
    ///
    /// Example: A 22s clip at 2x speed ends at timeline_start + 11s.
    ///
    /// This matches Premiere Pro / DaVinci speed-with-ripple behavior:
    /// the clip visually shortens and there's a gap after it.
    ///
    /// **Loop modes override this**: Loop and PingPong fill the full raw
    /// duration by repeating source media.
    pub fn timeline_end(&self) -> Duration {
        self.clip.timeline_start + self.effective_media_duration()
    }

    /// Effective media playback duration at current speed.
    ///
    /// At 2x speed, a 22s clip plays through its media in 11s.
    ///
    /// - For Loop/PingPong modes: returns raw duration (media repeats).
    /// - For None/Clamp modes: returns `media_duration / speed`.
    pub fn effective_media_duration(&self) -> Duration {
        let raw_duration: Duration = self.clip.duration();

        match self.loop_mode {
            ClipLoopMode::Loop | ClipLoopMode::PingPong => {
                // Loop modes fill the full clip duration by repeating media
                raw_duration
            }
            ClipLoopMode::None | ClipLoopMode::Clamp => {
                if self.speed <= 0.0 || (self.speed - 1.0).abs() < f64::EPSILON {
                    raw_duration
                } else {
                    // Speed up → shorter timeline footprint
                    // Speed down → longer timeline footprint
                    let effective_secs: f64 = raw_duration.as_secs_f64() / self.speed;
                    Duration::from_secs_f64(effective_secs.max(0.0))
                }
            }
        }
    }

    /// Check if this clip contains a given timeline time.
    ///
    /// Uses speed-adjusted `timeline_end()` so the clip correctly reports
    /// as not containing times past where the source media ends at the
    /// current playback speed.
    pub fn contains_time(&self, time: Duration) -> bool {
        time >= self.clip.timeline_start && time < self.timeline_end()
    }

    /// Convert timeline time to source media time.
    ///
    /// Implements the algorithm from Olive's `ClipBlock::SequenceToMediaTime`:
    /// 1. Calculate relative time from clip start
    /// 2. Apply speed multiplier
    /// 3. Apply reverse (if enabled)
    /// 4. Add source in-point offset
    ///
    /// # Arguments
    ///
    /// * `timeline_time` - Time on the timeline
    /// * `flags` - Optional flags to ignore certain transformations
    ///
    /// # Returns
    ///
    /// The corresponding source media time, or None if outside clip bounds.
    pub fn timeline_to_source(
        &self,
        timeline_time: Duration,
        flags: TimeRemapFlags,
    ) -> Option<Duration> {
        // Check bounds
        if !self.contains_time(timeline_time) {
            return None;
        }

        // Calculate relative time from clip start
        let relative_time: Duration = timeline_time.saturating_sub(self.clip.timeline_start);

        // Apply reverse
        let reversed_time: Duration =
            if self.reverse && !flags.contains(TimeRemapFlags::IGNORE_REVERSE) {
                self.clip.duration().saturating_sub(relative_time)
            } else {
                relative_time
            };

        // Apply speed
        let speed_adjusted: Duration = if !flags.contains(TimeRemapFlags::IGNORE_SPEED) {
            if self.speed == 0.0 {
                // Freeze frame at in-point
                Duration::ZERO
            } else if (self.speed - 1.0).abs() > f64::EPSILON {
                // Apply speed multiplier
                Duration::from_secs_f64(reversed_time.as_secs_f64() * self.speed)
            } else {
                reversed_time
            }
        } else {
            reversed_time
        };

        // Apply per-clip loop mode.
        //
        // The source duration is the available media range (source_out - source_in).
        // After speed adjustment, if the result exceeds this range, the loop mode
        // determines what happens:
        //   None  → clamp to [0, source_duration) — standard NLE, holds last frame
        //   Loop  → modulo wrap — seamless repeat
        //   PingPong → alternating forward/reverse — smooth oscillation
        //   Clamp → explicit freeze at boundary (same as None for forward playback)
        let looped_time: Duration = if !flags.contains(TimeRemapFlags::IGNORE_LOOP) {
            let source_duration_secs: f64 = self.clip.duration().as_secs_f64();
            let speed_secs: f64 = speed_adjusted.as_secs_f64();

            if source_duration_secs <= 0.0 {
                // Degenerate clip — no source range
                Duration::ZERO
            } else if speed_secs < source_duration_secs {
                // Within source range — no loop logic needed
                speed_adjusted
            } else {
                // Exceeds source range — apply loop mode
                match self.loop_mode {
                    ClipLoopMode::None | ClipLoopMode::Clamp => {
                        // Clamp to last safe position (one frame before EOF).
                        // FFmpeg frequently can't decode the very last frame
                        // of a container — using ~33ms (1 frame at 30fps) as
                        // the safety margin prevents EOF decode errors.
                        let one_frame_sec: f64 = 1.0 / 30.0;
                        let clamped_secs: f64 =
                            speed_secs.min(source_duration_secs - one_frame_sec);
                        Duration::from_secs_f64(clamped_secs.max(0.0))
                    }
                    ClipLoopMode::Loop => {
                        // Modulo wrap — seamless repeat from the beginning
                        let wrapped_secs: f64 = speed_secs % source_duration_secs;
                        Duration::from_secs_f64(wrapped_secs)
                    }
                    ClipLoopMode::PingPong => {
                        // Ping-pong: determine which "period" we're in.
                        // Even periods play forward, odd periods play in reverse.
                        let period_count: u64 = (speed_secs / source_duration_secs) as u64;
                        let remainder_secs: f64 = speed_secs % source_duration_secs;
                        let looped_secs: f64 = if period_count % 2 == 0 {
                            // Even period: forward
                            remainder_secs
                        } else {
                            // Odd period: reverse
                            source_duration_secs - remainder_secs
                        };
                        Duration::from_secs_f64(looped_secs.clamp(0.0, source_duration_secs))
                    }
                }
            }
        } else {
            speed_adjusted
        };

        // Add source in-point
        let source_time: Duration = self.clip.source_in + looped_time;

        Some(source_time)
    }

    /// Get the asset UUID if this is a video clip
    pub fn asset_uuid(&self) -> Option<Uuid> {
        self.asset_ref.as_ref().map(|r| r.asset_uuid)
    }

    /// Get the asset filename for display
    pub fn asset_filename(&self) -> Option<&str> {
        self.asset_ref.as_ref().map(|r| r.filename.as_str())
    }
}

impl Node for ClipNode {
    fn id(&self) -> &'static str {
        "org.strawberry.clip"
    }

    fn name(&self) -> &'static str {
        "Clip"
    }

    fn category(&self) -> NodeCategory {
        NodeCategory::Timeline
    }

    fn inputs(&self) -> Vec<InputDefinition> {
        vec![
            // Footage input (connected to VideoAssetManager)
            InputDefinition::footage("footage", "Footage")
                .with_description("Source video or image"),
            // Media in point
            InputDefinition::duration("media_in", "Media In", Duration::ZERO)
                .with_flags(InputFlags::NOT_CONNECTABLE | InputFlags::NOT_KEYFRAMABLE)
                .with_description("Source start point"),
            // Speed
            InputDefinition::float("speed", "Speed", 1.0)
                .with_range(0.0, 10.0)
                .with_flags(InputFlags::NOT_KEYFRAMABLE)
                .with_description("Playback speed multiplier"),
            // Reverse
            InputDefinition::boolean("reverse", "Reverse", false)
                .with_flags(InputFlags::NOT_KEYFRAMABLE)
                .with_description("Play in reverse"),
        ]
    }

    fn process(&self, inputs: &NodeValueRow, globals: &NodeGlobals) -> NodeValueTable {
        let mut table: NodeValueTable = NodeValueTable::new();

        // Get timeline time from globals
        let timeline_time: Duration = globals.time_range.start;

        // Check if we cover this time
        if !self.contains_time(timeline_time) {
            return table; // Empty - we don't contribute to this frame
        }

        // Convert to source time
        let source_time: Option<Duration> =
            self.timeline_to_source(timeline_time, TimeRemapFlags::empty());

        if let Some(source_time) = source_time {
            // Get footage input (this would be resolved by the traverser)
            if let Some(footage) = inputs.get("footage").and_then(|v| v.as_texture()) {
                // Pass through the texture (Phase 1: no effects)
                table.push(NodeValue::Texture(footage.clone()));
            } else {
                // No footage - check if we have an asset ref and push a job
                if let Some(asset_ref) = &self.asset_ref {
                    // Create a footage reference for the traverser to resolve
                    table.push(NodeValue::Footage(FootageRef::new(
                        asset_ref.asset_uuid,
                        &asset_ref.filename,
                    )));

                    // Store the source time for the traverser
                    table.push(NodeValue::Duration(source_time));
                }
            }
        }

        table
    }

    fn stable_id(&self) -> Option<Uuid> {
        Some(self.stable_id.uuid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_clip() -> ClipNode {
        let stable_id: StableId = StableId::new();
        let clip: Clip = Clip {
            name: "Test Clip".to_string(),
            source_in: Duration::from_secs(5),
            source_out: Duration::from_secs(15),
            timeline_start: Duration::from_secs(10),
        };
        ClipNode::new(stable_id, clip, None)
    }

    #[test]
    fn test_timeline_to_source_basic() {
        let node: ClipNode = make_test_clip();

        // Clip starts at timeline 10s, source starts at 5s
        // At timeline 12s (2s into clip), source should be at 7s
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(12), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(7)));
    }

    #[test]
    fn test_timeline_to_source_at_start() {
        let node: ClipNode = make_test_clip();

        // At clip start, source time should be source_in
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(10), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_timeline_to_source_outside_bounds() {
        let node: ClipNode = make_test_clip();

        // Before clip start
        assert!(node
            .timeline_to_source(Duration::from_secs(5), TimeRemapFlags::empty())
            .is_none());

        // After clip end
        assert!(node
            .timeline_to_source(Duration::from_secs(25), TimeRemapFlags::empty())
            .is_none());
    }

    #[test]
    fn test_timeline_to_source_with_speed() {
        let node: ClipNode = make_test_clip().with_speed(2.0);

        // At 2x speed, 2s of timeline = 4s of source
        // At timeline 12s (2s into clip, at 2x speed = 4s of source)
        // source_in (5s) + 4s = 9s
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(12), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(9)));
    }

    #[test]
    fn test_timeline_to_source_frozen() {
        let node: ClipNode = make_test_clip().with_speed(0.0);

        // At 0 speed, source time is always source_in (freeze frame)
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(12), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(5)));
    }

    // ====================================================================
    // LOOP MODE TESTS
    // ====================================================================

    /// Create a clip where speed causes playback to exceed source duration.
    /// Clip: source 5s..15s (10s of media), timeline_start=10s
    /// At 2x speed, 5s of timeline covers 10s of source — exactly at boundary.
    /// At 2x speed, 7s of timeline covers 14s of source — 4s beyond boundary.
    fn make_overcranked_clip() -> ClipNode {
        let stable_id: StableId = StableId::new();
        let clip: Clip = Clip {
            name: "Overcranked Clip".to_string(),
            source_in: Duration::from_secs(0),
            source_out: Duration::from_secs(10),
            timeline_start: Duration::from_secs(0),
        };
        ClipNode::new(stable_id, clip, None).with_speed(2.0)
    }

    #[test]
    fn test_loop_mode_none_clamps_at_source_boundary() {
        let node: ClipNode = make_overcranked_clip().with_loop_mode(ClipLoopMode::None);

        // At 2x speed on a 10s clip, effective_media_duration = 5s.
        // timeline_end = 0 + 5s = 5s.
        // At 7s timeline: contains_time(7s) = false (7 >= 5), so → None.
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(7), TimeRemapFlags::empty());
        assert_eq!(
            source, None,
            "Expected None at 7s (past effective end of 5s for 10s clip at 2x)"
        );

        // Query at 4.9s → within effective bounds (4.9 < 5.0)
        // relative = 4.9s, speed_adjusted = 9.8s < 10s → valid
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs_f64(4.9), TimeRemapFlags::empty());
        let src_secs: f64 = source.unwrap().as_secs_f64();
        assert!(
            (src_secs - 9.8).abs() < 0.001,
            "Expected ~9.8s, got {}",
            src_secs
        );
    }

    #[test]
    fn test_loop_mode_loop_wraps_around() {
        let node: ClipNode = make_overcranked_clip().with_loop_mode(ClipLoopMode::Loop);

        // At 2x speed, 7s of timeline = 14s of source.
        // Source is 10s. Loop mode: 14 % 10 = 4s.
        // source_in=0, so final source_time = 4s.
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(7), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(4)));
    }

    #[test]
    fn test_loop_mode_pingpong_reverses_on_odd_periods() {
        let node: ClipNode = make_overcranked_clip().with_loop_mode(ClipLoopMode::PingPong);

        // At 2x speed, 7s of timeline = 14s of source.
        // Source is 10s. Period count = 14/10 = 1 (odd → reverse).
        // Remainder = 14 % 10 = 4s.
        // Odd period: result = 10 - 4 = 6s.
        // source_in=0, so final source_time = 6s.
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(7), TimeRemapFlags::empty());

        assert_eq!(source, Some(Duration::from_secs(6)));
    }

    #[test]
    fn test_loop_mode_clamp_freezes_last_frame() {
        let node: ClipNode = make_overcranked_clip().with_loop_mode(ClipLoopMode::Clamp);

        // Clamp behaves identically to None for timeline bounds:
        // At 2x speed, effective_media_duration = 5s, timeline_end = 5s.
        // At 7s: contains_time(7s) = false (7 >= 5), so → None.
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(7), TimeRemapFlags::empty());
        assert_eq!(
            source, None,
            "Expected None at 7s (past effective end of 5s for 10s clip at 2x)"
        );

        // Query within bounds: 4.9s → speed_adjusted = 9.8s < 10s → valid
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs_f64(4.9), TimeRemapFlags::empty());
        let src_secs: f64 = source.unwrap().as_secs_f64();
        assert!(
            (src_secs - 9.8).abs() < 0.001,
            "Expected ~9.8s, got {}",
            src_secs
        );
    }

    #[test]
    fn test_loop_within_source_range_no_effect() {
        // When speed_adjusted is within source range, loop mode has no effect.
        let node: ClipNode = make_test_clip().with_loop_mode(ClipLoopMode::Loop);

        // Normal speed, 2s into clip — well within 10s source range.
        let source: Option<Duration> =
            node.timeline_to_source(Duration::from_secs(12), TimeRemapFlags::empty());

        // source_in(5) + relative(2) = 7
        assert_eq!(source, Some(Duration::from_secs(7)));
    }
}
