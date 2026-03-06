//! Timeline color definitions.
//!
//! Contains the color palette for the timeline widget styling.
//! Based on CapCut minimalist dark theme.

use egui::Color32;

// ============================================================================
// CAPCUT MINIMALIST - DARK BLACK & WHITE
// ============================================================================

// Backgrounds - Pure dark black
pub const BACKGROUND: Color32 = Color32::from_rgb(13, 13, 13); // #0d0d0d pure black
pub const BACKGROUND_ELEVATED: Color32 = Color32::from_rgb(24, 24, 24); // #181818

// ============================================================================
// RULER COLORS
// ============================================================================

pub const RULER_BG: Color32 = Color32::from_rgb(20, 20, 20); // #141414
pub const RULER_TEXT: Color32 = Color32::from_rgb(140, 140, 140); // Gray text
pub const RULER_MAJOR_LINE: Color32 = Color32::from_rgb(60, 60, 60);
pub const RULER_MINOR_LINE: Color32 = Color32::from_rgb(35, 35, 35);

// ============================================================================
// TRACK LANE COLORS
// ============================================================================

pub const TRACK_LANE_BG: Color32 = Color32::from_rgb(13, 13, 13); // #0d0d0d
pub const TRACK_LANE_ALT: Color32 = Color32::from_rgb(18, 18, 18); // #121212
pub const TRACK_DIVIDER: Color32 = Color32::from_rgb(40, 40, 40);

// ============================================================================
// CLIP COLORS
// ============================================================================

/// CapCut style muted purple-blue for video clips
pub const CLIP_VIDEO: Color32 = Color32::from_rgb(92, 107, 192); // #5c6bc0 muted indigo
pub const CLIP_VIDEO_HOVER: Color32 = Color32::from_rgb(112, 127, 212); // Lighter

/// Muted cyan-teal for audio clips
pub const CLIP_AUDIO: Color32 = Color32::from_rgb(78, 182, 196);
pub const CLIP_AUDIO_HOVER: Color32 = Color32::from_rgb(98, 202, 216); // Lighter

/// White selection border for selected clips
pub const CLIP_SELECTED_BORDER: Color32 = Color32::from_rgb(255, 255, 255);
/// Selection glow — semi-transparent accent cyan for outer glow effect
pub const CLIP_SELECTION_GLOW: Color32 = Color32::from_rgba_premultiplied(9, 21, 23, 30);
/// Subtle dark border for unselected clips
pub const CLIP_BORDER: Color32 = Color32::from_rgb(45, 45, 45);
/// Pure white text on clips
pub const CLIP_TEXT: Color32 = Color32::from_rgb(255, 255, 255);

// ============================================================================
// PLAYHEAD COLORS
// ============================================================================

/// Pure white playhead
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 255, 255);

/// Shadow behind playhead for contrast
pub fn playhead_shadow() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 120)
}

// ============================================================================
// TRANSITION BUTTON COLORS
// ============================================================================

/// White background for transition "+" button
pub const TRANSITION_PLUS_BG: Color32 = Color32::from_rgb(255, 255, 255);
pub const TRANSITION_PLUS_HOVER: Color32 = Color32::from_rgb(200, 200, 200);

// ============================================================================
// MISC COLORS
// ============================================================================

pub const SCROLLBAR_BG: Color32 = Color32::from_rgb(24, 24, 24);
/// Cyan accent for hover effects
pub const ACCENT_CYAN: Color32 = Color32::from_rgb(78, 182, 196);

// Aliases for compatibility with drawing code
pub const TRACK_BG: Color32 = TRACK_LANE_BG;
pub const TRACK_ALT: Color32 = TRACK_LANE_ALT;
pub const TRACK_SEPARATOR: Color32 = TRACK_DIVIDER;
pub const TIMELINE_TICK: Color32 = RULER_MINOR_LINE;
pub const TEXT_SECONDARY: Color32 = RULER_TEXT;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_values() {
        // Verify background is dark
        assert!(BACKGROUND.r() < 20);
        assert!(PLAYHEAD.r() == 255);
    }
}
