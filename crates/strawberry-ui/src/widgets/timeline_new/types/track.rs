//! Track type definitions for the timeline widget.
//!
//! Contains the `TrackInfo` struct representing individual timeline tracks.

// ============================================================================
// TRACK DATA
// ============================================================================

/// Default height for video tracks - CapCut style: thin strips (~35px)
pub const DEFAULT_VIDEO_TRACK_HEIGHT: f32 = 35.0;

/// Track information for rendering
///
/// Represents a single track on the timeline (video or audio).
/// Used by the timeline widget to render track lanes and route clips.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    /// Track entity ID (as bits for display)
    pub entity_bits: u64,
    /// Track index (0 = bottom)
    pub index: u32,
    /// Track name
    pub name: String,
    /// Whether video or audio
    pub is_video: bool,
    /// Is muted
    pub muted: bool,
    /// Is solo
    pub solo: bool,
    /// Is locked
    pub locked: bool,
    /// Track height override
    pub height: f32,
}

impl Default for TrackInfo {
    fn default() -> Self {
        Self {
            entity_bits: 0,
            index: 0,
            name: "Track".to_string(),
            is_video: true,
            muted: false,
            solo: false,
            locked: false,
            height: DEFAULT_VIDEO_TRACK_HEIGHT,
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
    fn test_track_info_default() {
        let track = TrackInfo::default();
        assert_eq!(track.index, 0);
        assert!(track.is_video);
        assert!(!track.muted);
        assert_eq!(track.height, DEFAULT_VIDEO_TRACK_HEIGHT);
    }
}
