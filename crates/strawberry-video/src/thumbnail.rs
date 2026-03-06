//! # Thumbnail Manager with LOD Support
//!
//! Generates multi-resolution thumbnails for timeline filmstrip display.
//! Thumbnails are persisted to disk and validated on load.
//!
//! ## Why This Exists
//!
//! Professional NLEs (CapCut, Premiere, DaVinci) display video frame thumbnails
//! tiled across clips in the timeline. This provides visual context for editing.
//!
//! ## LOD System
//!
//! Different zoom levels need different thumbnail densities:
//! - **LOD0 (40px)**: Every 10 seconds - for heavily zoomed out views
//! - **LOD1 (80px)**: Every 2 seconds - for normal editing view
//! - **LOD2**: Uses proxy frames directly - for detailed/zoomed in view
//!
//! ## Disk Cache Structure
//!
//! ```text
//! %LOCALAPPDATA%/strawberry/proxies/<asset-uuid>/
//! ├── frame_00001.jpg      # Proxy frames (already exist)
//! ├── frame_00002.jpg
//! ├── ...
//! └── thumbnails/
//!     ├── manifest.json    # Validation metadata
//!     ├── lod0/            # Low-res thumbnails
//!     │   ├── thumb_0001.jpg
//!     │   └── ...
//!     └── lod1/            # Medium-res thumbnails
//!         ├── thumb_0001.jpg
//!         └── ...
//! ```

use crate::ffmpeg::{FFmpeg, FFmpegError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// LOD level for thumbnails
///
/// Each level has different resolution and frame interval,
/// optimized for specific zoom levels in the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailLod {
    /// 40px height, thumbnail every 10 seconds
    /// Use when: pixels_per_second < 10 (very zoomed out)
    Lod0,
    /// 80px height, thumbnail every 2 seconds
    /// Use when: 10 <= pixels_per_second < 50 (normal view)
    Lod1,
    /// Use proxy frames directly
    /// Use when: pixels_per_second >= 50 (zoomed in)
    Lod2,
}

impl ThumbnailLod {
    /// Get the height in pixels for this LOD level
    pub fn height(&self) -> u32 {
        match self {
            ThumbnailLod::Lod0 => 40,
            ThumbnailLod::Lod1 => 80,
            ThumbnailLod::Lod2 => 0, // Uses proxy frames
        }
    }

    /// Get the interval between thumbnails in seconds
    pub fn interval_seconds(&self) -> f64 {
        match self {
            ThumbnailLod::Lod0 => 10.0,
            ThumbnailLod::Lod1 => 2.0,
            ThumbnailLod::Lod2 => 0.0, // Uses proxy frames
        }
    }

    /// Get the directory name for this LOD level
    pub fn dir_name(&self) -> &'static str {
        match self {
            ThumbnailLod::Lod0 => "lod0",
            ThumbnailLod::Lod1 => "lod1",
            ThumbnailLod::Lod2 => "", // Not stored separately
        }
    }
}

/// Calculate which LOD level to use based on zoom level
///
/// # Arguments
/// * `pixels_per_second` - Current timeline zoom (pixels showing per second of video)
///
/// # Returns
/// The appropriate LOD level for instant thumbnail display
pub fn get_lod_for_zoom(pixels_per_second: f64) -> ThumbnailLod {
    if pixels_per_second < 10.0 {
        ThumbnailLod::Lod0 // Very zoomed out
    } else if pixels_per_second < 50.0 {
        ThumbnailLod::Lod1 // Normal view
    } else {
        ThumbnailLod::Lod2 // Zoomed in - use proxy frames
    }
}

/// Information about a single LOD level in the manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LodInfo {
    /// LOD level (0, 1)
    pub level: u32,
    /// Thumbnail height in pixels
    pub height: u32,
    /// Interval between thumbnails in seconds
    pub interval_seconds: f64,
    /// Number of thumbnail files
    pub count: u32,
}

/// Manifest file for cache validation
///
/// Stored as `thumbnails/manifest.json` in the proxy folder.
/// Used to verify thumbnails are still valid (source hasn't changed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailManifest {
    /// Source file modification time (seconds since UNIX epoch)
    pub source_mtime: Option<u64>,
    /// Source file size in bytes
    pub source_size: u64,
    /// Duration of the video in seconds
    pub duration_seconds: f64,
    /// LOD levels that have been generated
    pub lod_levels: Vec<LodInfo>,
}

impl ThumbnailManifest {
    /// Create a new manifest for a source file
    pub fn new(source_path: &Path, duration: Duration) -> Self {
        let (source_mtime, source_size) = std::fs::metadata(source_path)
            .map(|m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                (mtime, m.len())
            })
            .unwrap_or((None, 0));

        Self {
            source_mtime,
            source_size,
            duration_seconds: duration.as_secs_f64(),
            lod_levels: Vec::new(),
        }
    }

    /// Check if the manifest matches the current source file
    pub fn is_valid_for(&self, source_path: &Path) -> bool {
        match std::fs::metadata(source_path) {
            Ok(m) => {
                let current_size = m.len();
                let current_mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                // Size must match
                if current_size != self.source_size {
                    return false;
                }

                // Mtime must match (if both available)
                if let (Some(current), Some(stored)) = (current_mtime, self.source_mtime) {
                    if current != stored {
                        return false;
                    }
                }

                true
            }
            Err(_) => false,
        }
    }

    /// Add LOD level info to the manifest
    pub fn add_lod(&mut self, level: u32, height: u32, interval_seconds: f64, count: u32) {
        self.lod_levels.push(LodInfo {
            level,
            height,
            interval_seconds,
            count,
        });
    }
}

/// Manager for clip thumbnails with LOD and caching
///
/// Generates and manages multi-resolution thumbnails for timeline display.
/// Thumbnails are stored on disk alongside proxy frames.
pub struct ThumbnailManager {
    /// FFmpeg instance for thumbnail generation
    ffmpeg: FFmpeg,
}

impl ThumbnailManager {
    /// Create a new thumbnail manager
    pub fn new() -> Result<Self, FFmpegError> {
        Ok(Self {
            ffmpeg: FFmpeg::new()?,
        })
    }

    /// Get the thumbnails directory for an asset
    pub fn get_thumbnails_dir(&self, proxy_dir: &Path) -> PathBuf {
        proxy_dir.join("thumbnails")
    }

    /// Get the LOD directory for an asset
    pub fn get_lod_dir(&self, proxy_dir: &Path, lod: ThumbnailLod) -> PathBuf {
        self.get_thumbnails_dir(proxy_dir).join(lod.dir_name())
    }

    /// Get the manifest path for an asset
    pub fn get_manifest_path(&self, proxy_dir: &Path) -> PathBuf {
        self.get_thumbnails_dir(proxy_dir).join("manifest.json")
    }

    /// Check if thumbnails are valid (source hasn't changed)
    ///
    /// Returns true if thumbnails exist and manifest matches source file.
    pub fn is_cache_valid(&self, proxy_dir: &Path, source_path: &Path) -> bool {
        let manifest_path = self.get_manifest_path(proxy_dir);

        if !manifest_path.exists() {
            return false;
        }

        match std::fs::read_to_string(&manifest_path) {
            Ok(json) => match serde_json::from_str::<ThumbnailManifest>(&json) {
                Ok(manifest) => manifest.is_valid_for(source_path),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    /// Load manifest from disk
    pub fn load_manifest(&self, proxy_dir: &Path) -> Option<ThumbnailManifest> {
        let manifest_path = self.get_manifest_path(proxy_dir);
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    /// Save manifest to disk
    fn save_manifest(
        &self,
        proxy_dir: &Path,
        manifest: &ThumbnailManifest,
    ) -> Result<(), std::io::Error> {
        let manifest_path = self.get_manifest_path(proxy_dir);
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(manifest_path, json)
    }

    /// Generate all LOD levels for an asset
    ///
    /// Should be called during or after proxy generation.
    /// Creates LOD0 and LOD1 thumbnail directories with JPEG images.
    pub fn generate_all_lods(
        &self,
        source_path: &Path,
        proxy_dir: &Path,
        duration: Duration,
    ) -> Result<(), FFmpegError> {
        // Create thumbnails directory
        let thumb_dir = self.get_thumbnails_dir(proxy_dir);
        std::fs::create_dir_all(&thumb_dir).map_err(FFmpegError::Io)?;

        // Create manifest
        let mut manifest = ThumbnailManifest::new(source_path, duration);

        // Generate LOD0 (40px, every 10 seconds)
        let lod0_count = self.generate_lod(source_path, proxy_dir, ThumbnailLod::Lod0, duration)?;
        manifest.add_lod(0, 40, 10.0, lod0_count);

        // Generate LOD1 (80px, every 2 seconds)
        let lod1_count = self.generate_lod(source_path, proxy_dir, ThumbnailLod::Lod1, duration)?;
        manifest.add_lod(1, 80, 2.0, lod1_count);

        // Save manifest
        self.save_manifest(proxy_dir, &manifest)
            .map_err(|e| FFmpegError::ExecutionFailed(format!("Failed to save manifest: {}", e)))?;

        tracing::info!(
            "Generated thumbnails for {:?}: LOD0={} LOD1={}",
            source_path.file_name().unwrap_or_default(),
            lod0_count,
            lod1_count
        );

        Ok(())
    }

    /// Generate thumbnails for a single LOD level
    fn generate_lod(
        &self,
        source_path: &Path,
        proxy_dir: &Path,
        lod: ThumbnailLod,
        _duration: Duration,
    ) -> Result<u32, FFmpegError> {
        if lod == ThumbnailLod::Lod2 {
            return Ok(0); // LOD2 uses proxy frames
        }

        let lod_dir = self.get_lod_dir(proxy_dir, lod);
        std::fs::create_dir_all(&lod_dir).map_err(FFmpegError::Io)?;

        let height = lod.height();
        let interval = lod.interval_seconds();
        let fps = 1.0 / interval;

        // Output pattern
        let output_pattern = lod_dir.join("thumb_%04d.jpg");

        // qscale for JPEG quality (2 = high quality, 5 = medium)
        let qscale = if lod == ThumbnailLod::Lod0 { "4" } else { "3" };

        // Build FFmpeg command
        use std::process::{Command, Stdio};

        let status = Command::new(self.ffmpeg.ffmpeg_path())
            .args(["-i"])
            .arg(source_path)
            .args([
                "-vf",
                &format!("fps={},scale=-1:{}", fps, height),
                "-qscale:v",
                qscale,
                "-y",
            ])
            .arg(&output_pattern)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(FFmpegError::Io)?;

        if !status.success() {
            return Err(FFmpegError::ExecutionFailed(format!(
                "FFmpeg thumbnail generation failed with code {:?}",
                status.code()
            )));
        }

        // Count generated thumbnails
        let count = std::fs::read_dir(&lod_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).count())
            .unwrap_or(0) as u32;

        Ok(count)
    }

    /// Get paths to all thumbnails for a specific LOD level
    ///
    /// Returns vec of (timestamp_seconds, path) tuples.
    pub fn get_thumbnail_paths(
        &self,
        proxy_dir: &Path,
        lod: ThumbnailLod,
        duration: Duration,
    ) -> Vec<(f64, PathBuf)> {
        if lod == ThumbnailLod::Lod2 {
            return Vec::new(); // LOD2 uses proxy frames
        }

        let lod_dir = self.get_lod_dir(proxy_dir, lod);
        let interval = lod.interval_seconds();
        let duration_secs = duration.as_secs_f64();

        let expected_count = (duration_secs / interval).ceil() as u32;
        let mut results = Vec::with_capacity(expected_count as usize);

        for i in 1..=expected_count {
            let timestamp = (i as f64 - 1.0) * interval;
            let path = lod_dir.join(format!("thumb_{:04}.jpg", i));
            if path.exists() {
                results.push((timestamp, path));
            }
        }

        results
    }

    /// Delete thumbnail cache for an asset
    pub fn delete_thumbnails(&self, proxy_dir: &Path) -> Result<(), std::io::Error> {
        let thumb_dir = self.get_thumbnails_dir(proxy_dir);
        if thumb_dir.exists() {
            std::fs::remove_dir_all(&thumb_dir)?;
            tracing::debug!("Deleted thumbnail cache: {:?}", thumb_dir);
        }
        Ok(())
    }
}

impl Default for ThumbnailManager {
    fn default() -> Self {
        Self::new().expect("Failed to create ThumbnailManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_for_zoom() {
        assert_eq!(get_lod_for_zoom(5.0), ThumbnailLod::Lod0);
        assert_eq!(get_lod_for_zoom(10.0), ThumbnailLod::Lod1);
        assert_eq!(get_lod_for_zoom(30.0), ThumbnailLod::Lod1);
        assert_eq!(get_lod_for_zoom(50.0), ThumbnailLod::Lod2);
        assert_eq!(get_lod_for_zoom(100.0), ThumbnailLod::Lod2);
    }

    #[test]
    fn test_lod_properties() {
        assert_eq!(ThumbnailLod::Lod0.height(), 40);
        assert_eq!(ThumbnailLod::Lod0.interval_seconds(), 10.0);
        assert_eq!(ThumbnailLod::Lod1.height(), 80);
        assert_eq!(ThumbnailLod::Lod1.interval_seconds(), 2.0);
    }

    #[test]
    fn test_manifest_lod_tracking() {
        let mut manifest = ThumbnailManifest {
            source_mtime: Some(1234567890),
            source_size: 1000000,
            duration_seconds: 60.0,
            lod_levels: Vec::new(),
        };

        manifest.add_lod(0, 40, 10.0, 6);
        manifest.add_lod(1, 80, 2.0, 30);

        assert_eq!(manifest.lod_levels.len(), 2);
        assert_eq!(manifest.lod_levels[0].count, 6);
        assert_eq!(manifest.lod_levels[1].count, 30);
    }
}
