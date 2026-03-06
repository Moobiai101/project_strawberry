//! Timeline tool mode definitions.
//!
//! Contains the `TimelineTool` enum for different editing modes.

// ============================================================================
// TIMELINE TOOL MODE
// ============================================================================

/// Active editing tool mode (inspired by Olive's TimelineTool subclasses).
///
/// Each mode changes how mouse interactions are interpreted:
/// - **Pointer**: Default mode for selection, move, and trim
/// - **Razor**: Click to split clips at the cursor position
/// - **Slip**: Drag to shift clip's source content without moving clip position
/// - **Slide**: Drag to move clip while adjusting neighbor boundaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineTool {
    /// Default selection/move/trim mode
    #[default]
    Pointer,
    /// Split clips at click position (multi-track when Shift held)
    Razor,
    /// Shift source in/out points without moving clip position
    Slip,
    /// Move clip while adjusting neighbor clip boundaries
    Slide,
}

impl TimelineTool {
    /// Get display name for UI buttons.
    pub fn display_name(&self) -> &'static str {
        match self {
            TimelineTool::Pointer => "Select",
            TimelineTool::Razor => "Razor",
            TimelineTool::Slip => "Slip",
            TimelineTool::Slide => "Slide",
        }
    }

    /// Get keyboard shortcut hint.
    pub fn shortcut_hint(&self) -> &'static str {
        match self {
            TimelineTool::Pointer => "V",
            TimelineTool::Razor => "B",
            TimelineTool::Slip => "Y",
            TimelineTool::Slide => "U",
        }
    }

    /// Get the appropriate cursor icon for this tool.
    pub fn cursor_icon(&self) -> egui::CursorIcon {
        match self {
            TimelineTool::Pointer => egui::CursorIcon::Default,
            TimelineTool::Razor => egui::CursorIcon::Crosshair,
            TimelineTool::Slip => egui::CursorIcon::ResizeHorizontal,
            TimelineTool::Slide => egui::CursorIcon::Grab,
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
    fn test_timeline_tool_default() {
        let tool = TimelineTool::default();
        assert_eq!(tool, TimelineTool::Pointer);
    }

    #[test]
    fn test_display_names() {
        assert_eq!(TimelineTool::Pointer.display_name(), "Select");
        assert_eq!(TimelineTool::Razor.display_name(), "Razor");
    }

    #[test]
    fn test_shortcut_hints() {
        assert_eq!(TimelineTool::Pointer.shortcut_hint(), "V");
        assert_eq!(TimelineTool::Razor.shortcut_hint(), "B");
    }
}
