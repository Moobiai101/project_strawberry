//! # AI Command Interface
//!
//! Interface for AI to submit commands (same path as UI).

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use strawberry_core::commands::{Command, CommandError, CommandId, CommandRequest};

/// Result of command submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub command_id: String,
    pub success: bool,
    pub message: String,
}

/// Error types for AI command interface
#[derive(Debug, thiserror::Error)]
pub enum AICommandError {
    #[error("Unknown command type: {0}")]
    UnknownCommand(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Confirmation required for destructive command")]
    ConfirmationRequired,

    #[error("Command execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Command queue unavailable")]
    QueueUnavailable,
}

/// Factory function type for creating commands from JSON
pub type CommandFactory =
    Box<dyn Fn(serde_json::Value) -> Result<Box<dyn Command>, CommandError> + Send + Sync>;

/// Registry entry for a command
pub struct CommandEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub requires_confirmation: bool,
    pub factory: CommandFactory,
}

/// AI Command Interface
///
/// This is how AI submits commands to the system.
/// Uses the same CommandQueue as the UI.
pub struct AICommandInterface {
    /// Registered command factories
    commands: std::collections::HashMap<String, CommandEntry>,

    /// Channel to send commands to the main thread
    command_sender: mpsc::UnboundedSender<Box<dyn Command>>,
}

impl AICommandInterface {
    /// Create a new command interface
    pub fn new(command_sender: mpsc::UnboundedSender<Box<dyn Command>>) -> Self {
        let mut interface = Self {
            commands: std::collections::HashMap::new(),
            command_sender,
        };

        // Register built-in commands
        interface.register_builtin_commands();

        interface
    }

    /// Register all built-in commands
    fn register_builtin_commands(&mut self) {
        use strawberry_core::commands::{
            AddClipCommand, CreateProjectCommand, CreateTimelineCommand, CreateTrackCommand,
            DeleteClipCommand, MoveClipCommand, PauseCommand, PlayCommand, SeekCommand,
            SetSpeedCommand,
        };
        use strawberry_core::components::FrameRate;

        // Create Project
        self.register(
            "create_project",
            "Create a new project with specified settings",
            false,
            Box::new(|params| {
                let cmd: CreateProjectCommand = serde_json::from_value(params)
                    .map_err(|e| CommandError::ValidationFailed(e.to_string()))?;
                Ok(Box::new(cmd))
            }),
        );

        // Play
        self.register(
            "play",
            "Start playback",
            false,
            Box::new(|_params| Ok(Box::new(PlayCommand))),
        );

        // Pause
        self.register(
            "pause",
            "Pause playback",
            false,
            Box::new(|_params| Ok(Box::new(PauseCommand))),
        );

        // Seek
        self.register(
            "seek",
            "Seek to a specific frame",
            false,
            Box::new(|params| {
                let frame = params
                    .get("frame")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        CommandError::ValidationFailed("frame is required".to_string())
                    })?;
                let frame_rate = params
                    .get("frame_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(30.0);
                Ok(Box::new(SeekCommand::new(frame, frame_rate)))
            }),
        );

        // Set Speed
        self.register(
            "set_speed",
            "Set playback speed multiplier",
            false,
            Box::new(|params| {
                let speed = params
                    .get("speed")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| {
                        CommandError::ValidationFailed("speed is required".to_string())
                    })?;
                Ok(Box::new(SetSpeedCommand::new(speed as f32)))
            }),
        );

        // =====================================================================
        // TIMELINE COMMANDS
        // =====================================================================

        // Create Timeline
        self.register(
            "create_timeline",
            "Create a new timeline with video and audio tracks",
            false,
            Box::new(|params| {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Timeline 1")
                    .to_string();

                let frame_rate = params
                    .get("frame_rate")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "Fps24" => FrameRate::Fps24,
                        "Fps25" => FrameRate::Fps25,
                        "Fps30" => FrameRate::Fps30,
                        "Fps60" => FrameRate::Fps60,
                        _ => FrameRate::Fps30,
                    })
                    .unwrap_or(FrameRate::Fps30);

                let mut cmd = CreateTimelineCommand::new(name, frame_rate);

                if let Some(video_tracks) = params.get("video_tracks").and_then(|v| v.as_u64()) {
                    cmd = cmd.with_video_tracks(video_tracks as u32);
                }
                if let Some(audio_tracks) = params.get("audio_tracks").and_then(|v| v.as_u64()) {
                    cmd = cmd.with_audio_tracks(audio_tracks as u32);
                }

                Ok(Box::new(cmd))
            }),
        );

        // Create Track
        self.register(
            "create_track",
            "Add a new track to a timeline",
            false,
            Box::new(|params| {
                use bevy_ecs::entity::Entity;
                use strawberry_core::components::TrackType;

                let timeline_bits =
                    params
                        .get("timeline")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| {
                            CommandError::ValidationFailed(
                                "timeline entity is required".to_string(),
                            )
                        })?;

                let track_type = params
                    .get("track_type")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "Audio" => TrackType::Audio,
                        "Text" => TrackType::Text,
                        "Effect" => TrackType::Effect,
                        _ => TrackType::Video,
                    })
                    .unwrap_or(TrackType::Video);

                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Track")
                    .to_string();

                let cmd =
                    CreateTrackCommand::new(Entity::from_bits(timeline_bits), track_type, name);
                Ok(Box::new(cmd))
            }),
        );

        // Add Clip
        self.register(
            "add_clip",
            "Add a clip to a track at a specific position",
            false,
            Box::new(|params| {
                use bevy_ecs::entity::Entity;
                use std::time::Duration;

                let track_bits = params
                    .get("track")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        CommandError::ValidationFailed("track entity is required".to_string())
                    })?;

                let asset_bits = params
                    .get("asset")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        CommandError::ValidationFailed("asset entity is required".to_string())
                    })?;

                let at_secs = params.get("at").and_then(|v| v.as_f64()).unwrap_or(0.0);

                let mut cmd = AddClipCommand::new(
                    Entity::from_bits(track_bits),
                    Entity::from_bits(asset_bits),
                    Duration::from_secs_f64(at_secs),
                );

                if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    cmd = cmd.with_name(name);
                }

                Ok(Box::new(cmd))
            }),
        );

        // Move Clip
        self.register(
            "move_clip",
            "Move a clip to a new position and/or track",
            false,
            Box::new(|params| {
                use bevy_ecs::entity::Entity;
                use std::time::Duration;

                let clip_bits = params.get("clip").and_then(|v| v.as_u64()).ok_or_else(|| {
                    CommandError::ValidationFailed("clip entity is required".to_string())
                })?;

                let at_secs = params.get("at").and_then(|v| v.as_f64()).ok_or_else(|| {
                    CommandError::ValidationFailed(
                        "at (position in seconds) is required".to_string(),
                    )
                })?;

                let cmd = if let Some(track_bits) = params.get("to_track").and_then(|v| v.as_u64())
                {
                    MoveClipCommand::to_track(
                        Entity::from_bits(clip_bits),
                        Entity::from_bits(track_bits),
                        Duration::from_secs_f64(at_secs),
                    )
                } else {
                    MoveClipCommand::to_time(
                        Entity::from_bits(clip_bits),
                        Duration::from_secs_f64(at_secs),
                    )
                };

                Ok(Box::new(cmd))
            }),
        );

        // Delete Clip
        self.register(
            "delete_clip",
            "Delete a clip from the timeline",
            true, // Requires confirmation for destructive action
            Box::new(|params| {
                use bevy_ecs::entity::Entity;

                let clip_bits = params.get("clip").and_then(|v| v.as_u64()).ok_or_else(|| {
                    CommandError::ValidationFailed("clip entity is required".to_string())
                })?;

                Ok(Box::new(DeleteClipCommand::new(Entity::from_bits(
                    clip_bits,
                ))))
            }),
        );
    }

    /// Register a command
    pub fn register(
        &mut self,
        name: &'static str,
        description: &'static str,
        requires_confirmation: bool,
        factory: CommandFactory,
    ) {
        self.commands.insert(
            name.to_string(),
            CommandEntry {
                name,
                description,
                requires_confirmation,
                factory,
            },
        );
    }

    /// Submit a command from AI
    pub fn submit(&self, request: CommandRequest) -> Result<CommandResult, AICommandError> {
        // Find the command entry
        let entry = self
            .commands
            .get(&request.command_type)
            .ok_or_else(|| AICommandError::UnknownCommand(request.command_type.clone()))?;

        // Check confirmation for destructive commands
        if entry.requires_confirmation && !request.user_confirmed {
            return Err(AICommandError::ConfirmationRequired);
        }

        // Create the command
        let command = (entry.factory)(request.params)
            .map_err(|e| AICommandError::ValidationFailed(e.to_string()))?;

        let command_id = CommandId::new();

        // Send to command queue
        self.command_sender
            .send(command)
            .map_err(|_| AICommandError::QueueUnavailable)?;

        tracing::info!(
            "AI submitted command: {} (id: {:?})",
            request.command_type,
            command_id
        );

        Ok(CommandResult {
            command_id: command_id.0.to_string(),
            success: true,
            message: format!("Command '{}' submitted", request.command_type),
        })
    }

    /// Get list of available commands for AI discovery
    pub fn list_commands(&self) -> Vec<CommandInfo> {
        self.commands
            .values()
            .map(|entry| CommandInfo {
                name: entry.name.to_string(),
                description: entry.description.to_string(),
                requires_confirmation: entry.requires_confirmation,
            })
            .collect()
    }

    /// Get OpenAI function calling schema
    pub fn get_openai_functions(&self) -> Vec<serde_json::Value> {
        self.commands
            .values()
            .map(|entry| {
                serde_json::json!({
                    "name": entry.name,
                    "description": entry.description,
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                })
            })
            .collect()
    }
}

/// Command info for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub requires_confirmation: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_command_submission() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let interface = AICommandInterface::new(tx);

        let request = CommandRequest {
            command_type: "play".to_string(),
            params: serde_json::json!({}),
            user_confirmed: false,
        };

        let result = interface.submit(request);
        assert!(result.is_ok());

        // Command should be in the channel
        let cmd = rx.recv().await;
        assert!(cmd.is_some());
    }

    #[test]
    fn test_list_commands() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let interface = AICommandInterface::new(tx);

        let commands = interface.list_commands();
        assert!(!commands.is_empty());

        let names: Vec<_> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"play"));
        assert!(names.contains(&"pause"));
        assert!(names.contains(&"seek"));
        assert!(names.contains(&"create_project"));
    }
}
