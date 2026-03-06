//! # Command Registry
//!
//! Registry for all commands - enables AI discovery and JSON-based invocation.

use crate::commands::{Command, CommandError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Metadata about a command for AI discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMetadata {
    /// Command name (e.g., "create_project")
    pub name: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Category for grouping
    pub category: CommandCategory,
    /// Whether this command requires user confirmation
    pub requires_confirmation: bool,
    /// JSON schema for parameters
    pub parameters_schema: serde_json::Value,
}

/// Command categories for organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandCategory {
    Project,
    Timeline,
    Clip,
    Effect,
    Playback,
    Export,
    AI,
}

impl CommandCategory {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Timeline => "Timeline",
            Self::Clip => "Clip",
            Self::Effect => "Effect",
            Self::Playback => "Playback",
            Self::Export => "Export",
            Self::AI => "AI",
        }
    }
}

/// Factory function type for creating commands from JSON
pub type CommandFactory =
    Arc<dyn Fn(serde_json::Value) -> Result<Box<dyn Command>, CommandError> + Send + Sync>;

/// Registry entry for a command
pub struct CommandRegistryEntry {
    pub metadata: CommandMetadata,
    pub factory: CommandFactory,
}

/// Registry of all available commands
///
/// Used for:
/// - AI to discover available actions
/// - JSON-based command invocation
/// - Documentation generation
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<&'static str, CommandRegistryEntry>,
}

impl CommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command with its metadata and factory
    pub fn register(&mut self, metadata: CommandMetadata, factory: CommandFactory) {
        self.commands
            .insert(metadata.name, CommandRegistryEntry { metadata, factory });
    }

    /// Get metadata for a command
    pub fn get_metadata(&self, name: &str) -> Option<&CommandMetadata> {
        self.commands.get(name).map(|e| &e.metadata)
    }

    /// Create a command from JSON parameters
    pub fn create_command(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<Box<dyn Command>, CommandError> {
        let entry = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::InvalidOperation(format!("Unknown command: {}", name)))?;

        (entry.factory)(params)
    }

    /// Get all registered command names
    pub fn command_names(&self) -> Vec<&'static str> {
        self.commands.keys().copied().collect()
    }

    /// Get all commands in a category
    pub fn commands_in_category(&self, category: CommandCategory) -> Vec<&CommandMetadata> {
        self.commands
            .values()
            .filter(|e| e.metadata.category == category)
            .map(|e| &e.metadata)
            .collect()
    }

    /// Generate OpenAI function schema for all commands
    pub fn to_openai_functions(&self) -> Vec<serde_json::Value> {
        self.commands
            .values()
            .map(|entry| {
                serde_json::json!({
                    "name": entry.metadata.name,
                    "description": entry.metadata.description,
                    "parameters": entry.metadata.parameters_schema,
                })
            })
            .collect()
    }

    /// Get full schema for AI context
    pub fn to_ai_schema(&self) -> serde_json::Value {
        let commands: Vec<serde_json::Value> = self
            .commands
            .values()
            .map(|entry| {
                serde_json::json!({
                    "name": entry.metadata.name,
                    "description": entry.metadata.description,
                    "category": entry.metadata.category,
                    "requires_confirmation": entry.metadata.requires_confirmation,
                    "parameters": entry.metadata.parameters_schema,
                })
            })
            .collect();

        serde_json::json!({
            "version": "1.0",
            "commands": commands,
        })
    }
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("command_count", &self.commands.len())
            .field("commands", &self.command_names())
            .finish()
    }
}

/// Macro to simplify command registration
#[macro_export]
macro_rules! register_command {
    ($registry:expr, $cmd_type:ty, $name:literal, $desc:literal, $category:expr, $confirm:expr) => {
        $registry.register(
            $crate::commands::registry::CommandMetadata {
                name: $name,
                description: $desc,
                category: $category,
                requires_confirmation: $confirm,
                parameters_schema: <$cmd_type>::json_schema(),
            },
            std::sync::Arc::new(|params| {
                let cmd: $cmd_type = serde_json::from_value(params)
                    .map_err(|e| $crate::commands::CommandError::ValidationFailed(e.to_string()))?;
                Ok(Box::new(cmd) as Box<dyn $crate::commands::Command>)
            }),
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = CommandRegistry::new();
        assert!(registry.command_names().is_empty());
    }
}
