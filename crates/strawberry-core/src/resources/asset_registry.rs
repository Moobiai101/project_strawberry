//! # Asset Registry
//!
//! Central registry of all imported media assets.
//!
//! ## Purpose
//!
//! The AssetRegistry provides a single source of truth for all media files
//! imported into the project. It tracks:
//! - Source file paths
//! - Proxy file paths and generation status
//! - Video/audio metadata
//! - Relationships between assets and timeline clips
//!
//! ## Proxy Workflow
//!
//! When a video is imported:
//! 1. Asset is registered with `ProxyStatus::NotStarted`
//! 2. ProxyManager begins generating DNxHR proxy (intra-frame codec)
//! 3. Status updates to `ProxyStatus::Generating { progress }`
//! 4. On completion, status becomes `ProxyStatus::Ready`
//! 5. VideoService uses proxy for instant scrubbing
//!
//! ## Why This Exists
//!
//! H.264/H.265 cannot be seeked instantly due to temporal compression (GOP).
//! Professional NLEs (Premiere, Resolve, Final Cut) solve this with proxy files.
//! We generate DNxHR LB proxies which are intra-frame (every frame is standalone).

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// Status of proxy generation for an asset
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProxyStatus {
    /// Proxy generation has not started
    NotStarted,
    /// Proxy is being generated (0.0 to 1.0 progress)
    Generating { progress: f32 },
    /// Proxy is ready for use
    Ready,
    /// Proxy generation failed
    Failed { error: String },
    /// User disabled proxy for this asset
    Disabled,
}

impl Default for ProxyStatus {
    fn default() -> Self {
        ProxyStatus::NotStarted
    }
}

impl ProxyStatus {
    /// Check if proxy is ready for use
    pub fn is_ready(&self) -> bool {
        matches!(self, ProxyStatus::Ready)
    }

    /// Check if proxy is currently being generated
    pub fn is_generating(&self) -> bool {
        matches!(self, ProxyStatus::Generating { .. })
    }

    /// Get generation progress (0.0 to 1.0), or None if not generating
    pub fn progress(&self) -> Option<f32> {
        match self {
            ProxyStatus::Generating { progress } => Some(*progress),
            ProxyStatus::Ready => Some(1.0),
            _ => None,
        }
    }
}

/// Status of audio extraction for an asset
///
/// Audio is extracted from video containers via FFmpeg into a universally
/// decodable format (FLAC/WAV). This mirrors the proxy generation workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioStatus {
    /// Audio extraction has not started
    NotStarted,
    /// Audio is being extracted by a background FFmpeg process
    Extracting,
    /// Audio has been extracted and is ready for playback
    Ready,
    /// Audio extraction failed
    Failed { error: String },
    /// Source file has no audio stream (not an error)
    NoAudioStream,
}

impl Default for AudioStatus {
    fn default() -> Self {
        AudioStatus::NotStarted
    }
}

impl AudioStatus {
    /// Check if audio is ready for playback
    pub fn is_ready(&self) -> bool {
        matches!(self, AudioStatus::Ready)
    }

    /// Check if audio processing is complete (ready or no audio)
    pub fn is_complete(&self) -> bool {
        matches!(self, AudioStatus::Ready | AudioStatus::NoAudioStream)
    }

    /// Check if audio extraction is in progress
    pub fn is_extracting(&self) -> bool {
        matches!(self, AudioStatus::Extracting)
    }
}

/// Type of media asset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetType {
    /// Video file (may contain audio)
    Video,
    /// Audio-only file
    Audio,
    /// Image file (still)
    Image,
    /// Image sequence (folder of frames)
    ImageSequence,
}

/// Metadata about a registered asset
///
/// This is separate from VideoMetadata in strawberry-video because:
/// 1. It includes proxy-related fields
/// 2. It's serializable for project saving
/// 3. It covers all asset types, not just video
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetadata {
    /// Display name (typically filename without extension)
    pub name: String,
    /// Asset type
    pub asset_type: AssetType,
    /// Duration (zero for images)
    pub duration: Duration,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Frame rate (for video/image sequences)
    pub frame_rate: f64,
    /// Video/audio codec identifier
    pub codec: String,
    /// Total frame count
    pub frame_count: u64,
    /// File size in bytes
    pub file_size: u64,
}

impl AssetMetadata {
    /// Create metadata for a video asset
    pub fn video(
        name: String,
        duration: Duration,
        width: u32,
        height: u32,
        frame_rate: f64,
        codec: String,
        frame_count: u64,
        file_size: u64,
    ) -> Self {
        Self {
            name,
            asset_type: AssetType::Video,
            duration,
            width,
            height,
            frame_rate,
            codec,
            frame_count,
            file_size,
        }
    }

    /// Create metadata for an audio-only asset
    pub fn audio(name: String, duration: Duration, codec: String, file_size: u64) -> Self {
        Self {
            name,
            asset_type: AssetType::Audio,
            duration,
            width: 0,
            height: 0,
            frame_rate: 0.0,
            codec,
            frame_count: 0,
            file_size,
        }
    }

    /// Create metadata for an image asset
    pub fn image(name: String, width: u32, height: u32, file_size: u64) -> Self {
        Self {
            name,
            asset_type: AssetType::Image,
            duration: Duration::ZERO,
            width,
            height,
            frame_rate: 0.0,
            codec: String::new(),
            frame_count: 1,
            file_size,
        }
    }
}

/// A registered media asset in the project
///
/// Each asset has a stable UUID that persists across save/load and undo/redo.
/// The source_path is absolute, while proxy_path (if generated) is in the
/// project's cache directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredAsset {
    /// Unique identifier (stable across undo/redo and save/load)
    pub id: Uuid,
    /// Absolute path to the original source file
    pub source_path: PathBuf,
    /// Path to the generated proxy file (if any)
    pub proxy_path: Option<PathBuf>,
    /// Current proxy generation status
    pub proxy_status: ProxyStatus,
    /// Path to the extracted audio cache file (WAV/FLAC)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_cache_path: Option<PathBuf>,
    /// Current audio extraction status
    #[serde(default)]
    pub audio_status: AudioStatus,
    /// Asset metadata (resolution, duration, codec, etc.)
    pub metadata: AssetMetadata,
    /// Usage count (number of clips referencing this asset)
    pub usage_count: u32,
    /// Source file modification time (for proxy invalidation)
    ///
    /// Used to detect if the source file has changed since proxy was generated.
    /// If source changes, proxy should be regenerated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mtime: Option<u64>,
    /// Source file size in bytes (for quick change detection)
    ///
    /// Combined with mtime, provides reliable detection of source file changes.
    #[serde(default)]
    pub source_size: u64,
}

impl RegisteredAsset {
    /// Create a new registered asset
    pub fn new(id: Uuid, source_path: PathBuf, metadata: AssetMetadata) -> Self {
        // Try to get source file modification time and size
        let (source_mtime, source_size) = std::fs::metadata(&source_path)
            .map(|m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                let size = m.len();
                (mtime, size)
            })
            .unwrap_or((None, 0));

        Self {
            id,
            source_path,
            proxy_path: None,
            proxy_status: ProxyStatus::NotStarted,
            audio_cache_path: None,
            audio_status: AudioStatus::NotStarted,
            metadata,
            usage_count: 0,
            source_mtime,
            source_size,
        }
    }

    /// Check if source file has changed since asset was registered
    ///
    /// Returns true if mtime or size differs from stored values.
    /// Used to determine if proxy needs regeneration.
    pub fn source_has_changed(&self) -> bool {
        match std::fs::metadata(&self.source_path) {
            Ok(m) => {
                let current_size = m.len();
                let current_mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                // Size mismatch = definitely changed
                if current_size != self.source_size {
                    return true;
                }

                // Mtime mismatch = probably changed
                if let (Some(current), Some(stored)) = (current_mtime, self.source_mtime) {
                    if current != stored {
                        return true;
                    }
                }

                false
            }
            Err(_) => {
                // Can't access file - treat as changed to trigger re-check
                true
            }
        }
    }

    /// Update stored source file info from current file state
    ///
    /// Call this after proxy regeneration to update the baseline.
    pub fn update_source_info(&mut self) {
        if let Ok(m) = std::fs::metadata(&self.source_path) {
            self.source_size = m.len();
            self.source_mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
        }
    }

    /// Get the path to use for playback (proxy if available, else source)
    pub fn playback_path(&self) -> &PathBuf {
        match (&self.proxy_status, &self.proxy_path) {
            (ProxyStatus::Ready, Some(proxy)) => proxy,
            _ => &self.source_path,
        }
    }

    /// Check if this asset should use proxy for playback
    pub fn should_use_proxy(&self) -> bool {
        self.proxy_status.is_ready() && self.proxy_path.is_some()
    }

    /// Update proxy generation progress
    pub fn set_proxy_progress(&mut self, progress: f32) {
        self.proxy_status = ProxyStatus::Generating {
            progress: progress.clamp(0.0, 1.0),
        };
    }

    /// Mark proxy as ready
    pub fn set_proxy_ready(&mut self, proxy_path: PathBuf) {
        self.proxy_path = Some(proxy_path);
        self.proxy_status = ProxyStatus::Ready;
    }

    /// Mark proxy generation as failed
    pub fn set_proxy_failed(&mut self, error: String) {
        self.proxy_status = ProxyStatus::Failed { error };
    }

    /// Increment usage count (called when clip references this asset)
    pub fn increment_usage(&mut self) {
        self.usage_count = self.usage_count.saturating_add(1);
    }

    /// Decrement usage count (called when clip is deleted)
    pub fn decrement_usage(&mut self) {
        self.usage_count = self.usage_count.saturating_sub(1);
    }

    /// Check if asset is in use by any clips
    pub fn is_in_use(&self) -> bool {
        self.usage_count > 0
    }

    /// Check if asset is fully ready to be added to the timeline.
    ///
    /// Returns `true` only when ALL background processing is complete:
    /// - Proxy: `Ready` or `Disabled`
    /// - Audio: `Ready` or `NoAudioStream`
    ///
    /// This prevents the user from adding half-processed clips that would
    /// cause playback issues (missing audio, unoptimized seeking, etc).
    pub fn is_ready_for_timeline(&self) -> bool {
        let proxy_done: bool = matches!(
            self.proxy_status,
            ProxyStatus::Ready | ProxyStatus::Disabled
        );
        let audio_done: bool = self.audio_status.is_complete();
        proxy_done && audio_done
    }

    /// Mark audio extraction as in-progress
    pub fn set_audio_extracting(&mut self) {
        self.audio_status = AudioStatus::Extracting;
    }

    /// Mark audio as ready and store the cache path
    pub fn set_audio_ready(&mut self, cache_path: PathBuf) {
        self.audio_cache_path = Some(cache_path);
        self.audio_status = AudioStatus::Ready;
    }

    /// Mark audio extraction as failed
    pub fn set_audio_failed(&mut self, error: String) {
        self.audio_status = AudioStatus::Failed { error };
    }

    /// Mark source as having no audio stream
    pub fn set_no_audio_stream(&mut self) {
        self.audio_status = AudioStatus::NoAudioStream;
    }
}

/// Central registry of all imported media assets
///
/// This is an ECS Resource that provides:
/// - Fast UUID-based lookup
/// - Path-based duplicate detection
/// - Usage tracking for garbage collection
/// - Serialization for project save/load
///
/// ## Thread Safety
///
/// The registry itself is not thread-safe. Access it via ECS resource
/// borrowing (`Res<AssetRegistry>` or `ResMut<AssetRegistry>`).
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetRegistry {
    /// All registered assets indexed by UUID
    assets: HashMap<Uuid, RegisteredAsset>,
    /// Map from source path to asset UUID (for duplicate detection)
    path_to_uuid: HashMap<PathBuf, Uuid>,
}

impl AssetRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new asset in the registry
    ///
    /// Returns the asset UUID. If the path is already registered,
    /// returns the existing asset's UUID.
    pub fn register(&mut self, asset: RegisteredAsset) -> Uuid {
        let id = asset.id;
        let path = asset.source_path.clone();

        // Check for duplicate path
        if let Some(&existing_id) = self.path_to_uuid.get(&path) {
            tracing::debug!(
                "Asset at {:?} already registered with UUID {}",
                path,
                &existing_id.to_string()[..8]
            );
            return existing_id;
        }

        tracing::info!(
            "Registered asset '{}' with UUID {}",
            asset.metadata.name,
            &id.to_string()[..8]
        );

        self.path_to_uuid.insert(path, id);
        self.assets.insert(id, asset);
        id
    }

    /// Get an asset by UUID
    pub fn get(&self, id: &Uuid) -> Option<&RegisteredAsset> {
        self.assets.get(id)
    }

    /// Get a mutable reference to an asset by UUID
    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut RegisteredAsset> {
        self.assets.get_mut(id)
    }

    /// Get an asset by source path
    pub fn get_by_path(&self, path: &PathBuf) -> Option<&RegisteredAsset> {
        self.path_to_uuid
            .get(path)
            .and_then(|id| self.assets.get(id))
    }

    /// Check if a path is already registered
    pub fn contains_path(&self, path: &PathBuf) -> bool {
        self.path_to_uuid.contains_key(path)
    }

    /// Check if a UUID is registered
    pub fn contains(&self, id: &Uuid) -> bool {
        self.assets.contains_key(id)
    }

    /// Remove an asset from the registry
    ///
    /// Returns the removed asset, or None if not found.
    /// Note: This does NOT delete the proxy file on disk.
    pub fn remove(&mut self, id: &Uuid) -> Option<RegisteredAsset> {
        if let Some(asset) = self.assets.remove(id) {
            self.path_to_uuid.remove(&asset.source_path);
            tracing::info!(
                "Removed asset '{}' with UUID {}",
                asset.metadata.name,
                &id.to_string()[..8]
            );
            Some(asset)
        } else {
            None
        }
    }

    /// Get all registered asset IDs
    pub fn asset_ids(&self) -> impl Iterator<Item = &Uuid> {
        self.assets.keys()
    }

    /// Get all registered assets
    pub fn assets(&self) -> impl Iterator<Item = &RegisteredAsset> {
        self.assets.values()
    }

    /// Get mutable iterator over all assets
    pub fn assets_mut(&mut self) -> impl Iterator<Item = &mut RegisteredAsset> {
        self.assets.values_mut()
    }

    /// Get number of registered assets
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Get all assets that need proxy generation
    pub fn assets_needing_proxy(&self) -> impl Iterator<Item = &RegisteredAsset> {
        self.assets.values().filter(|a| {
            a.metadata.asset_type == AssetType::Video
                && matches!(a.proxy_status, ProxyStatus::NotStarted)
        })
    }

    /// Get all assets currently generating proxies
    pub fn assets_generating_proxy(&self) -> impl Iterator<Item = &RegisteredAsset> {
        self.assets
            .values()
            .filter(|a| a.proxy_status.is_generating())
    }

    /// Get assets filtered by type
    pub fn assets_of_type(&self, asset_type: AssetType) -> impl Iterator<Item = &RegisteredAsset> {
        self.assets
            .values()
            .filter(move |a| a.metadata.asset_type == asset_type)
    }

    /// Update proxy status for an asset
    pub fn update_proxy_status(&mut self, id: &Uuid, status: ProxyStatus) {
        if let Some(asset) = self.assets.get_mut(id) {
            asset.proxy_status = status;
        }
    }

    /// Set proxy as ready for an asset
    pub fn set_proxy_ready(&mut self, id: &Uuid, proxy_path: PathBuf) {
        if let Some(asset) = self.assets.get_mut(id) {
            asset.set_proxy_ready(proxy_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_register_asset() {
        let mut registry = AssetRegistry::new();
        let id = Uuid::new_v4();
        let asset =
            RegisteredAsset::new(id, PathBuf::from("/test/video.mp4"), create_test_metadata());

        let returned_id = registry.register(asset);
        assert_eq!(returned_id, id);
        assert!(registry.contains(&id));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_duplicate_path_detection() {
        let mut registry = AssetRegistry::new();
        let path = PathBuf::from("/test/video.mp4");

        let id1 = Uuid::new_v4();
        let asset1 = RegisteredAsset::new(id1, path.clone(), create_test_metadata());
        registry.register(asset1);

        let id2 = Uuid::new_v4();
        let asset2 = RegisteredAsset::new(id2, path.clone(), create_test_metadata());
        let returned_id = registry.register(asset2);

        // Should return the original ID, not the new one
        assert_eq!(returned_id, id1);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_proxy_status_transitions() {
        let id = Uuid::new_v4();
        let mut asset =
            RegisteredAsset::new(id, PathBuf::from("/test/video.mp4"), create_test_metadata());

        // Initial state
        assert!(!asset.should_use_proxy());
        assert_eq!(asset.playback_path(), &PathBuf::from("/test/video.mp4"));

        // Generating
        asset.set_proxy_progress(0.5);
        assert!(!asset.should_use_proxy());

        // Ready
        asset.set_proxy_ready(PathBuf::from("/cache/video_proxy.mov"));
        assert!(asset.should_use_proxy());
        assert_eq!(
            asset.playback_path(),
            &PathBuf::from("/cache/video_proxy.mov")
        );
    }

    #[test]
    fn test_usage_count() {
        let mut asset = RegisteredAsset::new(
            Uuid::new_v4(),
            PathBuf::from("/test/video.mp4"),
            create_test_metadata(),
        );

        assert!(!asset.is_in_use());
        asset.increment_usage();
        assert!(asset.is_in_use());
        assert_eq!(asset.usage_count, 1);
        asset.decrement_usage();
        assert!(!asset.is_in_use());
    }
}
