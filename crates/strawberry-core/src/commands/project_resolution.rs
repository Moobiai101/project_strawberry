//! # Project Resolution Command
//!
//! Command for changing the project's aspect ratio / canvas resolution.
//!
//! ## Architecture
//!
//! This command modifies the `ProjectSettings` resource, which is the single
//! source of truth for the project's output resolution. All viewport rendering,
//! gizmo coordinate mapping, and export encoding downstream read from this resource.
//!
//! The command is **not undoable** — aspect ratio changes are a "view" operation
//! (like zoom or pan), not an editing operation. Users don't expect Ctrl+Z to
//! revert an aspect ratio switch; they simply pick a different one.

use crate::commands::{Command, CommandError, UndoData};
use crate::resources::project_settings::{AspectRatioPreset, ProjectSettings};
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

// ============================================================================
// SET PROJECT RESOLUTION COMMAND
// ============================================================================

/// Command to change the project's aspect ratio and resolution.
///
/// Updates `ProjectSettings.active_preset` and recomputes `project_resolution`.
/// This immediately affects the viewport display, gizmo coordinate mapping,
/// and (future) export output resolution.
///
/// ## Not Undoable
///
/// Aspect ratio changes are view/configuration operations, not content edits.
/// Like playback commands (Play, Pause, Seek), they don't go on the undo stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetProjectResolutionCommand {
    /// The target aspect ratio preset to switch to
    pub new_preset: AspectRatioPreset,
}

impl SetProjectResolutionCommand {
    /// Create a new command to set the project resolution from a preset.
    pub fn new(preset: AspectRatioPreset) -> Self {
        Self { new_preset: preset }
    }
}

impl Command for SetProjectResolutionCommand {
    fn name(&self) -> &'static str {
        "Set Project Resolution"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut settings: bevy_ecs::world::Mut<'_, ProjectSettings> =
            world.get_resource_mut::<ProjectSettings>().ok_or_else(|| {
                CommandError::InvalidOperation("ProjectSettings resource not found".to_string())
            })?;

        let previous_preset: AspectRatioPreset = settings.active_preset;
        settings.set_preset(self.new_preset);

        tracing::info!(
            "Project resolution changed: {} → {} ({}×{})",
            previous_preset.label(),
            self.new_preset.label(),
            settings.project_resolution.x,
            settings.project_resolution.y,
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, _world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        // Not undoable — this is intentional, see doc comment above
        Ok(())
    }

    /// Aspect ratio changes are NOT undoable (view operation, not content edit)
    fn is_undoable(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET CUSTOM RESOLUTION COMMAND
// ============================================================================

/// Command to set a custom (user-defined) project resolution.
///
/// Switches the project to Custom preset with the specified pixel dimensions.
/// Used for non-standard aspect ratios not covered by the built-in presets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetCustomResolutionCommand {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
}

impl SetCustomResolutionCommand {
    /// Create a new command to set a custom resolution.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl Command for SetCustomResolutionCommand {
    fn name(&self) -> &'static str {
        "Set Custom Resolution"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut settings: bevy_ecs::world::Mut<'_, ProjectSettings> =
            world.get_resource_mut::<ProjectSettings>().ok_or_else(|| {
                CommandError::InvalidOperation("ProjectSettings resource not found".to_string())
            })?;

        settings.set_custom_resolution(self.width, self.height);

        tracing::info!(
            "Project resolution set to custom: {}×{}",
            settings.project_resolution.x,
            settings.project_resolution.y,
        );

        Ok(UndoData::None)
    }

    fn undo(&mut self, _world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        Ok(())
    }

    fn is_undoable(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_project_resolution_command_creation() {
        let cmd: SetProjectResolutionCommand =
            SetProjectResolutionCommand::new(AspectRatioPreset::Vertical9x16);
        assert_eq!(cmd.new_preset, AspectRatioPreset::Vertical9x16);
        assert_eq!(cmd.name(), "Set Project Resolution");
        assert!(!cmd.is_undoable());
    }

    #[test]
    fn test_set_custom_resolution_command_creation() {
        let cmd: SetCustomResolutionCommand = SetCustomResolutionCommand::new(3840, 2160);
        assert_eq!(cmd.width, 3840);
        assert_eq!(cmd.height, 2160);
        assert_eq!(cmd.name(), "Set Custom Resolution");
        assert!(!cmd.is_undoable());
    }
}
