//! # Import Asset Command
//!
//! Command to import media assets into the AssetRegistry.
//!
//! ## Command Pattern
//!
//! Following `agent.md` section 2.1 - ALL state mutations happen via Commands.
//! This command:
//! 1. Takes pre-probed metadata (probing happens in app/UI layer)
//! 2. Creates a RegisteredAsset with the provided UUID
//! 3. Registers it in the AssetRegistry
//! 4. Returns undo data to remove the asset on undo
//!
//! ## Separation of Concerns
//!
//! Per `agent.md`, the core crate must NOT depend on video service.
//! File probing happens in `strawberry-app` before submitting this command.
//!
//! ## Usage
//!
//! ```ignore
//! // In app layer after probing with FFmpeg:
//! let metadata = AssetMetadata::video(...);
//! let cmd = ImportAssetCommand::new(
//!     PathBuf::from("video.mp4"),
//!     metadata,
//! );
//! command_queue.submit(Box::new(cmd));
//! ```

use crate::commands::{Command, CommandError, UndoData};
use crate::resources::{AssetMetadata, AssetRegistry, RegisteredAsset};
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;
use uuid::Uuid;

/// Command to import a media asset into the project
///
/// This command:
/// 1. Creates a RegisteredAsset from provided path and metadata
/// 2. Assigns a stable UUID
/// 3. Registers in AssetRegistry
///
/// ## Note on Metadata
///
/// Metadata must be probed BEFORE creating this command. The probing
/// happens in the app layer using strawberry_video::FFmpeg. This keeps
/// strawberry-core free of video service dependencies.
///
/// Undo removes the asset from the registry.
/// Redo re-adds the same asset with its original UUID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAssetCommand {
    /// Path to the file to import (absolute path)
    pub source_path: PathBuf,
    /// Pre-probed metadata (from app layer)
    pub metadata: AssetMetadata,
    /// UUID assigned on first execute (stable across undo/redo)
    pub asset_uuid: Option<Uuid>,
}

impl ImportAssetCommand {
    /// Create a new import command with pre-probed metadata
    ///
    /// The metadata should be obtained by probing the file with FFmpeg
    /// in the app layer before creating this command.
    pub fn new(source_path: PathBuf, metadata: AssetMetadata) -> Self {
        Self {
            source_path,
            metadata,
            asset_uuid: None,
        }
    }

    /// Create with a pre-assigned UUID (for deterministic redo)
    pub fn with_uuid(source_path: PathBuf, metadata: AssetMetadata, uuid: Uuid) -> Self {
        Self {
            source_path,
            metadata,
            asset_uuid: Some(uuid),
        }
    }

    /// Get the asset UUID (populated after execute)
    pub fn get_asset_uuid(&self) -> Option<Uuid> {
        self.asset_uuid
    }
}

impl Command for ImportAssetCommand {
    fn name(&self) -> &'static str {
        "Import Asset"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Validate file exists
        if !self.source_path.exists() {
            return Err(CommandError::InvalidOperation(format!(
                "File not found: {:?}",
                self.source_path
            )));
        }

        // Generate or use existing UUID
        let asset_uuid: Uuid = self.asset_uuid.unwrap_or_else(Uuid::new_v4);
        self.asset_uuid = Some(asset_uuid);

        // Create the registered asset
        let asset: RegisteredAsset =
            RegisteredAsset::new(asset_uuid, self.source_path.clone(), self.metadata.clone());

        // Register in the AssetRegistry
        {
            let mut registry = world.resource_mut::<AssetRegistry>();

            // Check for duplicate - if already registered, return existing UUID
            if registry.contains_path(&self.source_path) {
                tracing::debug!(
                    "Asset at {:?} already registered, skipping import",
                    self.source_path
                );
                return Ok(UndoData::None);
            }

            registry.register(asset);
        }

        tracing::info!(
            "Imported asset '{}' with UUID {}",
            self.metadata.name,
            &asset_uuid.to_string()[..8]
        );

        // Return undo data - store the UUID for removal
        // We use PropertyChanged variant to store just the UUID
        Ok(UndoData::PropertyChanged {
            entity_bits: 0, // Not an entity
            property: "asset_uuid".to_string(),
            old_value: serde_json::json!(asset_uuid.to_string()),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let asset_uuid: Uuid = self
            .asset_uuid
            .ok_or_else(|| CommandError::InvalidOperation("No asset UUID to undo".to_string()))?;

        // Remove from registry
        {
            let mut registry = world.resource_mut::<AssetRegistry>();
            registry.remove(&asset_uuid);
        }

        tracing::info!("Undid import of asset '{}'", &asset_uuid.to_string()[..8]);

        Ok(())
    }

    fn is_undoable(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::AssetMetadata;
    use bevy_ecs::world::World;
    use std::time::Duration;

    fn setup_world() -> World {
        let mut world = World::new();
        world.insert_resource(AssetRegistry::new());
        world
    }

    fn create_test_metadata() -> AssetMetadata {
        AssetMetadata::video(
            "test_video".to_string(),
            Duration::from_secs(10),
            1920,
            1080,
            30.0,
            "h264".to_string(),
            300,
            10_000_000,
        )
    }

    #[test]
    fn test_import_command_nonexistent_file() {
        let mut world = setup_world();
        let mut cmd = ImportAssetCommand::new(
            PathBuf::from("/nonexistent/video.mp4"),
            create_test_metadata(),
        );

        let result = cmd.execute(&mut world);
        assert!(result.is_err());
    }

    // Note: Tests with actual files require temp file fixtures
    // See integration tests for full workflow testing
}
