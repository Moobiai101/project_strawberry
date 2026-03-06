//! # Media Browser Resource
//!
//! Manages imported media assets available for timeline placement.
//! Assets are stored here after import, then placed on timeline via AddClipCommand.
//!
//! ## Purpose
//!
//! Separates "imported media" from "placed clips":
//! - An asset can be imported once but placed multiple times on the timeline
//! - Enables drag-drop from media browser to timeline
//! - Stores metadata for UI display (name, duration, thumbnail)
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Import a video file
//! let uuid = asset_manager.load_asset(&path)?;
//!
//! // Register in MediaBrowser for UI display
//! media_browser.register(MediaBrowserAsset::new(uuid, name, path, duration));
//!
//! // Later, user drags to timeline:
//! AddClipCommand::new(track, asset, Duration::ZERO)
//!     .with_asset_uuid(uuid, name)
//!     .with_auto_sequence(true);
//! ```

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// MEDIA BROWSER ASSET
// ============================================================================

/// Represents an imported asset available for timeline placement.
///
/// Contains metadata for UI display (name, duration) and
/// timeline integration (UUID for ClipAssetRef).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaBrowserAsset {
    /// Unique identifier - matches UUID in VideoAssetManager
    pub uuid: Uuid,
    /// Display name (usually filename without extension)
    pub name: String,
    /// Path to the source file
    pub path: PathBuf,
    /// Total duration of the asset
    pub duration: Duration,
    /// Whether this asset is currently selected in the browser
    #[serde(skip)]
    pub selected: bool,
}

impl MediaBrowserAsset {
    /// Create a new media browser asset
    pub fn new(uuid: Uuid, name: impl Into<String>, path: PathBuf, duration: Duration) -> Self {
        Self {
            uuid,
            name: name.into(),
            path,
            duration,
            selected: false,
        }
    }

    /// Get the file extension (lowercase)
    pub fn extension(&self) -> Option<String> {
        self.path
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
    }

    /// Check if this is a video file (by extension)
    pub fn is_video(&self) -> bool {
        matches!(
            self.extension().as_deref(),
            Some("mp4" | "mov" | "avi" | "mkv" | "webm" | "mxf" | "mts")
        )
    }

    /// Check if this is an audio file (by extension)
    pub fn is_audio(&self) -> bool {
        matches!(
            self.extension().as_deref(),
            Some("mp3" | "wav" | "aac" | "flac" | "ogg")
        )
    }

    /// Check if this is an image file (by extension)
    pub fn is_image(&self) -> bool {
        matches!(
            self.extension().as_deref(),
            Some("png" | "jpg" | "jpeg" | "tiff" | "exr" | "dpx")
        )
    }
}

// ============================================================================
// MEDIA BROWSER RESOURCE
// ============================================================================

/// Resource: All imported media available for timeline placement.
///
/// Separates "imported media" from "placed clips" to enable
/// drag-drop from media browser to timeline. Multiple clips
/// can reference the same asset.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "import_media",
///   "path": "/path/to/video.mp4"
/// }
/// ```
#[derive(Resource, Default, Debug)]
pub struct MediaBrowser {
    /// Registered assets keyed by UUID
    assets: HashMap<Uuid, MediaBrowserAsset>,
    /// Import order for consistent UI display
    import_order: Vec<Uuid>,
}

impl MediaBrowser {
    /// Create a new empty media browser
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
            import_order: Vec::new(),
        }
    }

    /// Register a new asset in the media browser
    ///
    /// If an asset with the same UUID already exists, it will be updated.
    pub fn register(&mut self, asset: MediaBrowserAsset) {
        let uuid = asset.uuid;
        if !self.assets.contains_key(&uuid) {
            self.import_order.push(uuid);
        }
        self.assets.insert(uuid, asset);
        tracing::debug!("MediaBrowser: Registered asset {}", &uuid.to_string()[..8]);
    }

    /// Get an asset by UUID
    pub fn get(&self, uuid: &Uuid) -> Option<&MediaBrowserAsset> {
        self.assets.get(uuid)
    }

    /// Get mutable access to an asset by UUID
    pub fn get_mut(&mut self, uuid: &Uuid) -> Option<&mut MediaBrowserAsset> {
        self.assets.get_mut(uuid)
    }

    /// Remove an asset by UUID
    pub fn remove(&mut self, uuid: &Uuid) -> Option<MediaBrowserAsset> {
        self.import_order.retain(|id| id != uuid);
        self.assets.remove(uuid)
    }

    /// Check if an asset is registered
    pub fn contains(&self, uuid: &Uuid) -> bool {
        self.assets.contains_key(uuid)
    }

    /// Get all assets in import order
    pub fn assets_in_order(&self) -> impl Iterator<Item = &MediaBrowserAsset> {
        self.import_order
            .iter()
            .filter_map(|uuid| self.assets.get(uuid))
    }

    /// Get the count of registered assets
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Check if the browser is empty
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Clear all assets
    pub fn clear(&mut self) {
        self.assets.clear();
        self.import_order.clear();
    }

    /// Get all video assets
    pub fn videos(&self) -> impl Iterator<Item = &MediaBrowserAsset> {
        self.assets_in_order().filter(|a| a.is_video())
    }

    /// Get all audio assets
    pub fn audios(&self) -> impl Iterator<Item = &MediaBrowserAsset> {
        self.assets_in_order().filter(|a| a.is_audio())
    }

    /// Get all image assets
    pub fn images(&self) -> impl Iterator<Item = &MediaBrowserAsset> {
        self.assets_in_order().filter(|a| a.is_image())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_browser_new() {
        let browser = MediaBrowser::new();
        assert!(browser.is_empty());
        assert_eq!(browser.len(), 0);
    }

    #[test]
    fn test_media_browser_register() {
        let mut browser = MediaBrowser::new();
        let uuid = Uuid::new_v4();
        let asset = MediaBrowserAsset::new(
            uuid,
            "test_video",
            PathBuf::from("/path/to/video.mp4"),
            Duration::from_secs(10),
        );

        browser.register(asset);

        assert!(!browser.is_empty());
        assert_eq!(browser.len(), 1);
        assert!(browser.contains(&uuid));

        let retrieved = browser.get(&uuid).unwrap();
        assert_eq!(retrieved.name, "test_video");
        assert_eq!(retrieved.duration, Duration::from_secs(10));
    }

    #[test]
    fn test_media_browser_import_order() {
        let mut browser = MediaBrowser::new();

        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        let uuid3 = Uuid::new_v4();

        browser.register(MediaBrowserAsset::new(
            uuid1,
            "first",
            PathBuf::from("a.mp4"),
            Duration::from_secs(1),
        ));
        browser.register(MediaBrowserAsset::new(
            uuid2,
            "second",
            PathBuf::from("b.mp4"),
            Duration::from_secs(2),
        ));
        browser.register(MediaBrowserAsset::new(
            uuid3,
            "third",
            PathBuf::from("c.mp4"),
            Duration::from_secs(3),
        ));

        let names: Vec<_> = browser.assets_in_order().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn test_media_browser_remove() {
        let mut browser = MediaBrowser::new();
        let uuid = Uuid::new_v4();

        browser.register(MediaBrowserAsset::new(
            uuid,
            "test",
            PathBuf::from("test.mp4"),
            Duration::from_secs(1),
        ));
        assert!(browser.contains(&uuid));

        let removed = browser.remove(&uuid);
        assert!(removed.is_some());
        assert!(!browser.contains(&uuid));
        assert!(browser.is_empty());
    }

    #[test]
    fn test_asset_type_detection() {
        let video = MediaBrowserAsset::new(
            Uuid::new_v4(),
            "vid",
            PathBuf::from("test.mp4"),
            Duration::ZERO,
        );
        assert!(video.is_video());
        assert!(!video.is_audio());
        assert!(!video.is_image());

        let audio = MediaBrowserAsset::new(
            Uuid::new_v4(),
            "aud",
            PathBuf::from("test.mp3"),
            Duration::ZERO,
        );
        assert!(!audio.is_video());
        assert!(audio.is_audio());
        assert!(!audio.is_image());

        let image = MediaBrowserAsset::new(
            Uuid::new_v4(),
            "img",
            PathBuf::from("test.png"),
            Duration::ZERO,
        );
        assert!(!image.is_video());
        assert!(!image.is_audio());
        assert!(image.is_image());
    }
}
