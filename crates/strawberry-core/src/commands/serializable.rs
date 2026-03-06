//! # Serializable Command
//!
//! Trait for commands that can be serialized for AI invocation and project save/load.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{Command, CommandError};

/// A command request from AI or external source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    /// Type of command (e.g., "create_project", "seek", "play")
    pub command_type: String,
    /// Parameters as JSON
    pub params: Value,
    /// Whether user has confirmed destructive actions
    #[serde(default)]
    pub user_confirmed: bool,
}

/// Trait for commands that support serialization
///
/// This enables:
/// - AI to invoke commands via JSON
/// - Project save/load
/// - Command history serialization
/// - Network synchronization (future)
pub trait SerializableCommand: Command {
    /// The unique type identifier for this command
    /// Must be stable across versions
    fn command_type() -> &'static str
    where
        Self: Sized;

    /// Serialize this command to JSON
    fn to_json(&self) -> Result<Value, CommandError>;

    /// Deserialize from JSON parameters
    fn from_json(params: Value) -> Result<Box<dyn Command>, CommandError>
    where
        Self: Sized;

    /// JSON schema for validation and documentation
    fn json_schema() -> Value
    where
        Self: Sized;

    /// Whether this command requires user confirmation
    /// Used for destructive operations (delete, export, etc.)
    fn requires_confirmation() -> bool
    where
        Self: Sized,
    {
        false
    }
}

/// Macro to implement SerializableCommand for a command struct
///
/// Usage:
/// ```ignore
/// impl_serializable_command!(CreateProjectCommand, "create_project");
/// ```
#[macro_export]
macro_rules! impl_serializable_command {
    ($cmd_type:ty, $type_name:literal) => {
        impl $crate::commands::serializable::SerializableCommand for $cmd_type {
            fn command_type() -> &'static str {
                $type_name
            }

            fn to_json(&self) -> Result<serde_json::Value, $crate::commands::CommandError> {
                serde_json::to_value(self)
                    .map_err(|e| $crate::commands::CommandError::ValidationFailed(e.to_string()))
            }

            fn from_json(params: serde_json::Value) -> Result<Box<dyn $crate::commands::Command>, $crate::commands::CommandError> {
                let cmd: $cmd_type = serde_json::from_value(params)
                    .map_err(|e| $crate::commands::CommandError::ValidationFailed(e.to_string()))?;
                Ok(Box::new(cmd))
            }

            fn json_schema() -> serde_json::Value {
                // Generate basic schema from struct
                // In production, use schemars crate for full JSON Schema
                serde_json::json!({
                    "type": "object",
                    "name": $type_name,
                })
            }
        }
    };

    // Version with confirmation required
    ($cmd_type:ty, $type_name:literal, requires_confirmation) => {
        impl $crate::commands::serializable::SerializableCommand for $cmd_type {
            fn command_type() -> &'static str {
                $type_name
            }

            fn to_json(&self) -> Result<serde_json::Value, $crate::commands::CommandError> {
                serde_json::to_value(self)
                    .map_err(|e| $crate::commands::CommandError::ValidationFailed(e.to_string()))
            }

            fn from_json(params: serde_json::Value) -> Result<Box<dyn $crate::commands::Command>, $crate::commands::CommandError> {
                let cmd: $cmd_type = serde_json::from_value(params)
                    .map_err(|e| $crate::commands::CommandError::ValidationFailed(e.to_string()))?;
                Ok(Box::new(cmd))
            }

            fn json_schema() -> serde_json::Value {
                serde_json::json!({
                    "type": "object",
                    "name": $type_name,
                })
            }

            fn requires_confirmation() -> bool {
                true
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_request_parse() {
        let json = r#"{
            "command_type": "seek",
            "params": {"frame": 100, "frame_rate": 30.0}
        }"#;

        let request: CommandRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.command_type, "seek");
        assert!(!request.user_confirmed);
    }
}
