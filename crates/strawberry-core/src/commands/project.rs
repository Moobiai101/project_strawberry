//! # Project Commands
//!
//! Commands for project-level operations.

use crate::commands::{Command, CommandError, UndoData};
use crate::components::{ColorSpace, FrameRate, Project, ProjectSettings};
use crate::impl_serializable_command;
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use glam::UVec2;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

/// Command to create a new project
///
/// This command creates a new project entity with Project and ProjectSettings components.
///
/// JSON Example:
/// ```json
/// {
///     "name": "My Project",
///     "width": 1920,
///     "height": 1080,
///     "frame_rate": "30",
///     "color_space": "sRGB",
///     "duration_secs": 60.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectCommand {
    /// Project name
    pub name: String,
    /// Resolution width in pixels
    #[serde(default = "default_width")]
    pub width: u32,
    /// Resolution height in pixels
    #[serde(default = "default_height")]
    pub height: u32,
    /// Frame rate as string (e.g., "30", "24", "60", "29.97")
    #[serde(default = "default_frame_rate")]
    pub frame_rate: String,
    /// Color space name (e.g., "sRGB", "Rec709", "Rec2020", "DCI-P3")
    #[serde(default = "default_color_space")]
    pub color_space: String,
    /// Duration in seconds
    #[serde(default = "default_duration")]
    pub duration_secs: f64,
}

fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_frame_rate() -> String {
    "30".to_string()
}
fn default_color_space() -> String {
    "sRGB".to_string()
}
fn default_duration() -> f64 {
    60.0
}

impl CreateProjectCommand {
    /// Create a new CreateProjectCommand with defaults
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            width: 1920,
            height: 1080,
            frame_rate: "30".to_string(),
            color_space: "sRGB".to_string(),
            duration_secs: 60.0,
        }
    }

    /// Set resolution
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set frame rate from preset
    pub fn with_frame_rate(mut self, frame_rate: FrameRate) -> Self {
        self.frame_rate = frame_rate.display_name();
        self
    }

    /// Set frame rate from string
    pub fn with_frame_rate_str(mut self, frame_rate: impl Into<String>) -> Self {
        self.frame_rate = frame_rate.into();
        self
    }

    /// Set color space from preset
    pub fn with_color_space(mut self, color_space: ColorSpace) -> Self {
        self.color_space = color_space.display_name().to_string();
        self
    }

    /// Set duration in seconds
    pub fn with_duration_secs(mut self, duration_secs: f64) -> Self {
        self.duration_secs = duration_secs;
        self
    }

    /// Parse frame rate string to FrameRate enum
    fn parse_frame_rate(&self) -> FrameRate {
        match self.frame_rate.as_str() {
            "23.976" => FrameRate::Fps23_976,
            "24" => FrameRate::Fps24,
            "25" => FrameRate::Fps25,
            "29.97" => FrameRate::Fps29_97,
            "30" => FrameRate::Fps30,
            "50" => FrameRate::Fps50,
            "59.94" => FrameRate::Fps59_94,
            "60" => FrameRate::Fps60,
            _ => {
                // Try to parse as custom frame rate
                if let Ok(fps) = self.frame_rate.parse::<f64>() {
                    let fps_1000 = (fps * 1000.0) as u32;
                    FrameRate::Custom(fps_1000, 1000)
                } else {
                    FrameRate::Fps30 // Default fallback
                }
            }
        }
    }

    /// Parse color space string to ColorSpace enum
    fn parse_color_space(&self) -> ColorSpace {
        match self.color_space.to_lowercase().as_str() {
            "srgb" | "sRGB" => ColorSpace::Srgb,
            "rec709" | "rec. 709" | "Rec. 709" => ColorSpace::Rec709,
            "rec2020" | "rec. 2020 (hdr)" | "Rec. 2020 (HDR)" => ColorSpace::Rec2020,
            "dci-p3" | "DCI-P3" | "dcip3" => ColorSpace::DciP3,
            "acescg" | "ACEScg" => ColorSpace::AcesCg,
            _ => ColorSpace::Srgb, // Default fallback
        }
    }
}

impl Command for CreateProjectCommand {
    fn name(&self) -> &'static str {
        "Create Project"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let resolution = UVec2::new(self.width, self.height);
        let frame_rate = self.parse_frame_rate();
        let color_space = self.parse_color_space();
        let duration = Duration::from_secs_f64(self.duration_secs);

        // Create project entity with both components
        let entity = world
            .spawn((
                Project::new(&self.name),
                ProjectSettings {
                    resolution,
                    frame_rate,
                    color_space,
                    duration,
                },
            ))
            .id();

        tracing::info!(
            "Created project '{}' ({}x{} @ {} fps, {})",
            self.name,
            self.width,
            self.height,
            frame_rate.display_name(),
            color_space.display_name(),
        );

        Ok(UndoData::EntityCreated(entity.to_bits()))
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        match undo_data {
            UndoData::EntityCreated(entity_bits) => {
                let entity = Entity::from_bits(entity_bits);

                // Check if entity still exists
                if world.get_entity(entity).is_err() {
                    return Err(CommandError::EntityNotFound(format!(
                        "Entity {:?} not found",
                        entity
                    )));
                }

                world.despawn(entity);
                tracing::info!("Undid project creation");
                Ok(())
            }
            _ => Err(CommandError::InvalidOperation(
                "Invalid undo data for CreateProjectCommand".to_string(),
            )),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Implement serializable command for AI invocation
impl_serializable_command!(CreateProjectCommand, "create_project");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project_command() {
        let mut world = World::new();

        let mut cmd = CreateProjectCommand::new("Test Project")
            .with_resolution(3840, 2160)
            .with_frame_rate(FrameRate::Fps60);

        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        // Verify entity was created
        let mut query = world.query::<(&Project, &ProjectSettings)>();
        let count = query.iter(&world).count();
        assert_eq!(count, 1);

        // Verify data
        for (project, settings) in query.iter(&world) {
            assert_eq!(project.name, "Test Project");
            assert_eq!(settings.resolution, UVec2::new(3840, 2160));
        }
    }

    #[test]
    fn test_create_project_undo() {
        let mut world = World::new();

        let mut cmd = CreateProjectCommand::new("Test");
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify created
        let mut query = world.query::<&Project>();
        assert_eq!(query.iter(&world).count(), 1);

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify deleted
        assert_eq!(query.iter(&world).count(), 0);
    }

    #[test]
    fn test_command_serialization() {
        let cmd = CreateProjectCommand::new("Serialized Project")
            .with_resolution(1280, 720)
            .with_frame_rate(FrameRate::Fps24);

        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: CreateProjectCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "Serialized Project");
        assert_eq!(deserialized.width, 1280);
        assert_eq!(deserialized.height, 720);
        assert_eq!(deserialized.frame_rate, "24");
    }

    #[test]
    fn test_command_from_json() {
        let json = r#"{
            "name": "JSON Project",
            "width": 4096,
            "height": 2160
        }"#;

        let cmd: CreateProjectCommand = serde_json::from_str(json).unwrap();

        assert_eq!(cmd.name, "JSON Project");
        assert_eq!(cmd.width, 4096);
        assert_eq!(cmd.height, 2160);
        assert_eq!(cmd.frame_rate, "30"); // Default
        assert_eq!(cmd.color_space, "sRGB"); // Default
    }
}
