//! Clip type definitions for the timeline widget.
//!
//! Contains `ClipInfo` for rendering and `FilmstripAssetTextures` for thumbnails.

use std::time::Duration;
use strawberry_core::components::audio::FadeCurve;

// ============================================================================
// CLIP DATA
// ============================================================================

/// Clip information for rendering on the timeline.
///
/// Used by the timeline widget to render individual clips.
/// Contains all display-relevant data without ECS dependencies.
#[derive(Debug, Clone)]
pub struct ClipInfo {
    /// Clip entity ID (as bits for cross-frame identification)
    pub entity_bits: u64,
    /// Clip name (displayed on clip)
    pub name: String,
    /// Track index this clip is on
    pub track_index: u32,
    /// Start time on timeline
    pub timeline_start: Duration,
    /// Duration on timeline
    pub duration: Duration,
    /// Is currently selected
    pub selected: bool,
    /// Custom color (RGBA)
    pub color: Option<[u8; 4]>,
    /// Asset UUID for thumbnail lookup (None for audio-only clips)
    pub asset_uuid: Option<uuid::Uuid>,
    /// Source in point (for filmstrip rendering with slip edits)
    pub source_in: Duration,
    /// Source out point
    pub source_out: Duration,
    /// Whether this clip's audio is muted (for 🔊/🔇 icon rendering)
    pub audio_muted: bool,
    /// Clip audio volume 0.0–2.0 (for icon opacity hint)
    pub audio_volume: f32,
    /// Fade-in duration in milliseconds (0 = no fade-in)
    pub fade_in_ms: u32,
    /// Fade-out duration in milliseconds (0 = no fade-out)
    pub fade_out_ms: u32,
    /// Fade curve shape (for visual overlay rendering)
    pub fade_curve: FadeCurve,
}

// ============================================================================
// FILMSTRIP TEXTURES
// ============================================================================

/// Pre-loaded filmstrip textures for a single asset.
///
/// Used by TimelineWidget to render thumbnail filmstrips on clips.
/// Contains pre-uploaded GPU textures at multiple LOD levels for
/// efficient zooming.
#[derive(Debug, Clone, Default)]
pub struct FilmstripAssetTextures {
    /// LOD0 thumbnails: (timestamp_seconds, texture_id)
    /// Lower detail, used at low zoom levels
    pub lod0: Vec<(f64, egui::TextureId)>,
    /// LOD1 thumbnails: (timestamp_seconds, texture_id)
    /// Higher detail, used at high zoom levels
    pub lod1: Vec<(f64, egui::TextureId)>,
    /// Thumbnail width in pixels
    pub thumb_width: f32,
    /// Thumbnail height in pixels
    pub thumb_height: f32,
    /// Interval between LOD0 thumbnails (seconds)
    pub lod0_interval: f64,
    /// Interval between LOD1 thumbnails (seconds)
    pub lod1_interval: f64,
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_info_creation() {
        let clip = ClipInfo {
            entity_bits: 12345,
            name: "Test Clip".to_string(),
            track_index: 0,
            timeline_start: Duration::from_secs(5),
            duration: Duration::from_secs(10),
            selected: false,
            color: None,
            asset_uuid: None,
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(10),
            audio_muted: false,
            audio_volume: 1.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            fade_curve: FadeCurve::Linear,
        };
        assert_eq!(clip.name, "Test Clip");
        assert_eq!(clip.duration.as_secs(), 10);
    }

    #[test]
    fn test_filmstrip_textures_default() {
        let textures = FilmstripAssetTextures::default();
        assert!(textures.lod0.is_empty());
        assert!(textures.lod1.is_empty());
    }
}
