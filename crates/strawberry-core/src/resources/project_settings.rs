//! # Project Settings Resource
//!
//! Defines the project-level resolution and aspect ratio settings.
//!
//! ## Design Intent
//!
//! Professional NLEs (Clipchamp, DaVinci Resolve, Premiere) separate two concepts:
//!
//! - **Project Resolution**: User-chosen canvas size (e.g. 1080×1920 for vertical TikTok).
//!   This defines the output frame and how the viewport renders.
//! - **Source Resolution**: The native pixel dimensions of each video asset (from FFmpeg decode).
//!   This is per-clip, not project-wide.
//!
//! Previously, Project Strawberry conflated these — the viewport always used the first
//! video's native resolution. This module introduces a proper `ProjectSettings` resource
//! with 7 preset aspect ratios matching Clipchamp's "Size" dropdown, plus custom sizing.
//!
//! ## Data Flow
//!
//! ```text
//! User clicks "Size" dropdown → SetProjectResolutionCommand
//!     → updates ProjectSettings.active_preset + project_resolution
//!     → ViewportWidget reads project_resolution for display_rect
//!     → Gizmos use project_resolution for coordinate mapping
//!     → Export uses project_resolution for output encoding
//! ```

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};

// ============================================================================
// ASPECT RATIO PRESETS
// ============================================================================

/// Predefined aspect ratio presets matching Clipchamp's "Size" dropdown.
///
/// Each preset maps to a specific pixel resolution optimized for its target platform.
/// All presets use 1080p as the base dimension to ensure consistent quality.
///
/// ## Adding New Presets
///
/// To add a preset, add a variant here and update `preset_to_resolution()`,
/// `label()`, `subtitle()`, and `ALL_PRESETS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AspectRatioPreset {
    /// 16:9 — YouTube, streaming sites, standard widescreen
    Wide16x9,
    /// 9:16 — Instagram Reels, TikTok, YouTube Shorts
    Vertical9x16,
    /// 1:1 — Instagram posts, Facebook feed
    Square1x1,
    /// 4:3 — Classic television, presentations
    Classic4x3,
    /// 4:5 — Instagram/Facebook social posts (max vertical in feed)
    Social4x5,
    /// 21:9 — Ultrawide cinematic (anamorphic widescreen)
    Cinema21x9,
    /// 2:3 — Portrait photography, print media
    Portrait2x3,
    /// User-defined custom resolution
    Custom,
}

impl Default for AspectRatioPreset {
    fn default() -> Self {
        Self::Wide16x9
    }
}

impl AspectRatioPreset {
    /// All standard presets in display order (excludes Custom).
    ///
    /// Used by the UI dropdown to enumerate available options.
    pub const ALL_PRESETS: &'static [AspectRatioPreset] = &[
        Self::Wide16x9,
        Self::Vertical9x16,
        Self::Square1x1,
        Self::Classic4x3,
        Self::Social4x5,
        Self::Cinema21x9,
        Self::Portrait2x3,
    ];

    /// Convert preset to target pixel resolution.
    ///
    /// All presets use 1080 as the base dimension to ensure:
    /// - Consistent quality across all aspect ratios
    /// - No upscaling artifacts when switching between presets
    /// - Standard export resolutions for each platform
    pub fn to_resolution(self) -> glam::UVec2 {
        match self {
            Self::Wide16x9 => glam::UVec2::new(1920, 1080),
            Self::Vertical9x16 => glam::UVec2::new(1080, 1920),
            Self::Square1x1 => glam::UVec2::new(1080, 1080),
            Self::Classic4x3 => glam::UVec2::new(1440, 1080),
            Self::Social4x5 => glam::UVec2::new(1080, 1350),
            Self::Cinema21x9 => glam::UVec2::new(2520, 1080),
            Self::Portrait2x3 => glam::UVec2::new(1080, 1620),
            // Custom returns a sentinel — callers must use ProjectSettings.project_resolution
            Self::Custom => glam::UVec2::new(1920, 1080),
        }
    }

    /// Human-readable label for the preset (e.g. "Wide 16:9").
    ///
    /// Matches Clipchamp's labeling convention.
    pub fn label(self) -> &'static str {
        match self {
            Self::Wide16x9 => "Wide 16:9",
            Self::Vertical9x16 => "Vertical 9:16",
            Self::Square1x1 => "Square 1:1",
            Self::Classic4x3 => "Classic 4:3",
            Self::Social4x5 => "Social 4:5",
            Self::Cinema21x9 => "Cinema 21:9",
            Self::Portrait2x3 => "Portrait 2:3",
            Self::Custom => "Custom",
        }
    }

    /// Descriptive subtitle for the preset's target platform.
    ///
    /// Shown below the label in the dropdown for user guidance.
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Wide16x9 => "YouTube and streaming sites",
            Self::Vertical9x16 => "Instagram Reels and TikTok",
            Self::Square1x1 => "Instagram posts",
            Self::Classic4x3 => "Classic television",
            Self::Social4x5 => "Social media posts",
            Self::Cinema21x9 => "Cinematic ultrawide",
            Self::Portrait2x3 => "Portrait format",
            Self::Custom => "User-defined resolution",
        }
    }

    /// Get the aspect ratio as a floating-point value (width / height).
    ///
    /// Used for display_rect calculations and letterbox/pillarbox math.
    pub fn aspect_ratio_f32(self) -> f32 {
        let resolution: glam::UVec2 = self.to_resolution();
        resolution.x as f32 / resolution.y.max(1) as f32
    }
}

// ============================================================================
// PROJECT SETTINGS RESOURCE
// ============================================================================

/// Project-level settings for resolution, aspect ratio, and canvas configuration.
///
/// This resource is the single source of truth for the project's output resolution.
/// All viewport rendering, gizmo coordinate mapping, and export encoding should
/// read `project_resolution` from this resource — NOT the native video dimensions.
///
/// ## Lifecycle
///
/// - Created with default (Wide 16:9, 1920×1080) on app startup.
/// - Updated via `SetProjectResolutionCommand` when user clicks the "Size" dropdown.
/// - Persisted with the project file (future: project save/load).
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// Currently active aspect ratio preset
    pub active_preset: AspectRatioPreset,
    /// Effective project resolution in pixels
    ///
    /// For standard presets, this equals `active_preset.to_resolution()`.
    /// For Custom, this holds the user-specified resolution.
    pub project_resolution: glam::UVec2,
    /// User-specified custom resolution (only used when active_preset == Custom)
    pub custom_resolution: Option<glam::UVec2>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            active_preset: AspectRatioPreset::Wide16x9,
            project_resolution: AspectRatioPreset::Wide16x9.to_resolution(),
            custom_resolution: None,
        }
    }
}

impl ProjectSettings {
    /// Set the active preset and update `project_resolution` accordingly.
    ///
    /// For standard presets, resolution is computed from the preset.
    /// For Custom, resolution comes from the stored `custom_resolution`
    /// (or falls back to the previous resolution if none is set).
    pub fn set_preset(&mut self, preset: AspectRatioPreset) {
        self.active_preset = preset;
        if preset == AspectRatioPreset::Custom {
            // Use stored custom resolution, or keep current resolution
            if let Some(custom) = self.custom_resolution {
                self.project_resolution = custom;
            }
        } else {
            self.project_resolution = preset.to_resolution();
        }
    }

    /// Set a custom resolution and switch to Custom preset.
    ///
    /// Validates that both dimensions are at least 1 pixel.
    pub fn set_custom_resolution(&mut self, width: u32, height: u32) {
        let width: u32 = width.max(1);
        let height: u32 = height.max(1);
        let resolution: glam::UVec2 = glam::UVec2::new(width, height);
        self.custom_resolution = Some(resolution);
        self.active_preset = AspectRatioPreset::Custom;
        self.project_resolution = resolution;
    }

    /// Get the aspect ratio as a floating-point value (width / height).
    pub fn aspect_ratio_f32(&self) -> f32 {
        self.project_resolution.x as f32 / self.project_resolution.y.max(1) as f32
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_to_resolution_all_presets() {
        // Verify all presets have valid, non-zero resolutions
        for &preset in AspectRatioPreset::ALL_PRESETS {
            let resolution: glam::UVec2 = preset.to_resolution();
            assert!(resolution.x > 0, "Preset {:?} has zero width", preset);
            assert!(resolution.y > 0, "Preset {:?} has zero height", preset);
        }
    }

    #[test]
    fn test_preset_to_resolution_specific_values() {
        assert_eq!(
            AspectRatioPreset::Wide16x9.to_resolution(),
            glam::UVec2::new(1920, 1080)
        );
        assert_eq!(
            AspectRatioPreset::Vertical9x16.to_resolution(),
            glam::UVec2::new(1080, 1920)
        );
        assert_eq!(
            AspectRatioPreset::Square1x1.to_resolution(),
            glam::UVec2::new(1080, 1080)
        );
        assert_eq!(
            AspectRatioPreset::Classic4x3.to_resolution(),
            glam::UVec2::new(1440, 1080)
        );
        assert_eq!(
            AspectRatioPreset::Social4x5.to_resolution(),
            glam::UVec2::new(1080, 1350)
        );
        assert_eq!(
            AspectRatioPreset::Cinema21x9.to_resolution(),
            glam::UVec2::new(2520, 1080)
        );
        assert_eq!(
            AspectRatioPreset::Portrait2x3.to_resolution(),
            glam::UVec2::new(1080, 1620)
        );
    }

    #[test]
    fn test_default_is_wide_16x9() {
        let settings: ProjectSettings = ProjectSettings::default();
        assert_eq!(settings.active_preset, AspectRatioPreset::Wide16x9);
        assert_eq!(settings.project_resolution, glam::UVec2::new(1920, 1080));
    }

    #[test]
    fn test_set_preset_updates_resolution() {
        let mut settings: ProjectSettings = ProjectSettings::default();

        settings.set_preset(AspectRatioPreset::Vertical9x16);
        assert_eq!(settings.active_preset, AspectRatioPreset::Vertical9x16);
        assert_eq!(settings.project_resolution, glam::UVec2::new(1080, 1920));

        settings.set_preset(AspectRatioPreset::Square1x1);
        assert_eq!(settings.active_preset, AspectRatioPreset::Square1x1);
        assert_eq!(settings.project_resolution, glam::UVec2::new(1080, 1080));
    }

    #[test]
    fn test_set_custom_resolution() {
        let mut settings: ProjectSettings = ProjectSettings::default();

        settings.set_custom_resolution(2560, 1440);
        assert_eq!(settings.active_preset, AspectRatioPreset::Custom);
        assert_eq!(settings.project_resolution, glam::UVec2::new(2560, 1440));
        assert_eq!(
            settings.custom_resolution,
            Some(glam::UVec2::new(2560, 1440))
        );
    }

    #[test]
    fn test_custom_resolution_minimum_clamp() {
        let mut settings: ProjectSettings = ProjectSettings::default();

        // Zero values should be clamped to 1
        settings.set_custom_resolution(0, 0);
        assert_eq!(settings.project_resolution, glam::UVec2::new(1, 1));
    }

    #[test]
    fn test_custom_preset_preserves_resolution() {
        let mut settings: ProjectSettings = ProjectSettings::default();

        // Set custom, then switch to standard, then back to custom
        settings.set_custom_resolution(3840, 2160);
        settings.set_preset(AspectRatioPreset::Wide16x9);
        assert_eq!(settings.project_resolution, glam::UVec2::new(1920, 1080));

        // Going back to Custom should restore the stored custom resolution
        settings.set_preset(AspectRatioPreset::Custom);
        assert_eq!(settings.project_resolution, glam::UVec2::new(3840, 2160));
    }

    #[test]
    fn test_aspect_ratio_f32() {
        let settings: ProjectSettings = ProjectSettings::default();
        let aspect: f32 = settings.aspect_ratio_f32();
        assert!((aspect - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn test_all_presets_have_labels_and_subtitles() {
        for &preset in AspectRatioPreset::ALL_PRESETS {
            assert!(
                !preset.label().is_empty(),
                "Preset {:?} has empty label",
                preset
            );
            assert!(
                !preset.subtitle().is_empty(),
                "Preset {:?} has empty subtitle",
                preset
            );
        }
    }

    #[test]
    fn test_preset_aspect_ratios_are_correct() {
        // Verify aspect ratios match the named ratios
        let wide_aspect: f32 = AspectRatioPreset::Wide16x9.aspect_ratio_f32();
        assert!((wide_aspect - 16.0 / 9.0).abs() < 0.01);

        let vertical_aspect: f32 = AspectRatioPreset::Vertical9x16.aspect_ratio_f32();
        assert!((vertical_aspect - 9.0 / 16.0).abs() < 0.01);

        let square_aspect: f32 = AspectRatioPreset::Square1x1.aspect_ratio_f32();
        assert!((square_aspect - 1.0).abs() < 0.01);
    }
}
