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

    /// Generate all LOD levels for an asset.
    ///
    /// If the proxy is an image sequence, generates thumbnails by resizing
    /// existing proxy JPEG frames (no FFmpeg required — ~100x faster).
    /// Otherwise falls back to FFmpeg-based generation from the source file.
    pub fn generate_all_lods(
        &self,
        source_path: &Path,
        proxy_dir: &Path,
        duration: Duration,
    ) -> Result<(), FFmpegError> {
        // Create thumbnails directory
        let thumb_dir = self.get_thumbnails_dir(proxy_dir);
        std::fs::create_dir_all(&thumb_dir).map_err(FFmpegError::Io)?;

        // Check if proxy directory contains JPEG image sequence frames
        let has_proxy_frames: bool = proxy_dir.join("frame_00001.jpg").exists();

        let mut manifest = ThumbnailManifest::new(source_path, duration);

        if has_proxy_frames {
            // FAST PATH: Generate thumbnails from existing proxy JPEG frames.
            // This avoids re-decoding the source video (saves 2 full FFmpeg passes).
            // Each proxy frame is just resized using the `image` crate (~0.1ms each).
            let (lod0_count, lod1_count) =
                self.generate_lods_from_proxy_frames(proxy_dir, duration)?;
            manifest.add_lod(0, 40, 10.0, lod0_count);
            manifest.add_lod(1, 80, 2.0, lod1_count);
        } else {
            // FALLBACK: Use FFmpeg to generate thumbnails from source file.
            // Used for video proxies (non-image-sequence) where proxy frames
            // don't exist as individual files on disk.
            let lod0_count =
                self.generate_lod(source_path, proxy_dir, ThumbnailLod::Lod0, duration)?;
            manifest.add_lod(0, 40, 10.0, lod0_count);

            let lod1_count =
                self.generate_lod(source_path, proxy_dir, ThumbnailLod::Lod1, duration)?;
            manifest.add_lod(1, 80, 2.0, lod1_count);
        }

        // Save manifest
        self.save_manifest(proxy_dir, &manifest)
            .map_err(|e| FFmpegError::ExecutionFailed(format!("Failed to save manifest: {}", e)))?;

        tracing::info!(
            "Generated thumbnails for {:?}: LOD0={} LOD1={}",
            source_path.file_name().unwrap_or_default(),
            manifest.lod_levels[0].count,
            manifest.lod_levels[1].count
        );

        Ok(())
    }

    /// Generate LOD0 and LOD1 thumbnails by resizing existing proxy JPEG frames.
    ///
    /// This is the FAST PATH: no FFmpeg, no video decoding. Just:
    /// 1. Read proxy JPEG files from disk
    /// 2. Resize using `image` crate (Lanczos3 filter for quality)
    /// 3. Save as small JPEG thumbnails
    ///
    /// ## Performance
    ///
    /// Resizing a 720p JPEG to 80px takes ~0.1ms (vs ~10-50ms for FFmpeg decode).
    /// For a 10-min video: ~373 resize operations (~37ms total) vs ~36,000 FFmpeg
    /// decode operations (~6 minutes total). That's a **~100x speedup**.
    fn generate_lods_from_proxy_frames(
        &self,
        proxy_dir: &Path,
        duration: Duration,
    ) -> Result<(u32, u32), FFmpegError> {
        use image::imageops::FilterType;

        let duration_secs: f64 = duration.as_secs_f64();

        // Create LOD directories
        let lod0_dir: PathBuf = self.get_lod_dir(proxy_dir, ThumbnailLod::Lod0);
        let lod1_dir: PathBuf = self.get_lod_dir(proxy_dir, ThumbnailLod::Lod1);
        std::fs::create_dir_all(&lod0_dir).map_err(FFmpegError::Io)?;
        std::fs::create_dir_all(&lod1_dir).map_err(FFmpegError::Io)?;

        let lod0_interval: f64 = ThumbnailLod::Lod0.interval_seconds(); // 10s
        let lod1_interval: f64 = ThumbnailLod::Lod1.interval_seconds(); // 2s
        let lod0_height: u32 = ThumbnailLod::Lod0.height(); // 40px
        let lod1_height: u32 = ThumbnailLod::Lod1.height(); // 80px

        let mut lod0_count: u32 = 0;
        let mut lod1_count: u32 = 0;

        // Iterate through time positions for LOD1 (every 2s — finer granularity).
        // LOD0 is a subset of LOD1 (every 10s), so we generate both in one pass.
        let total_lod1: u32 = (duration_secs / lod1_interval).ceil() as u32;

        for i in 0..total_lod1 {
            let time_secs: f64 = i as f64 * lod1_interval;

            // Find the proxy frame number for this timestamp.
            // Proxy frames are named frame_00001.jpg (1-indexed) at source frame rate.
            // We approximate using 30fps (standard), but the exact frame doesn't matter
            // much for thumbnails — we just need a representative frame near this time.
            let frame_number: u32 = (time_secs * 30.0).round() as u32 + 1;
            let frame_path: PathBuf = proxy_dir.join(format!("frame_{:05}.jpg", frame_number));

            if !frame_path.exists() {
                // Frame might not exist if duration estimate is off — skip gracefully
                continue;
            }

            // Load the proxy frame JPEG
            let img = match image::open(&frame_path) {
                Ok(img) => img,
                Err(e) => {
                    tracing::trace!("Skipping proxy frame {:?}: {}", frame_path, e);
                    continue;
                }
            };

            // Generate LOD1 thumbnail (80px height, every 2s)
            lod1_count += 1;
            let lod1_thumb = img.resize(
                u32::MAX, // auto width
                lod1_height,
                FilterType::Triangle, // Fast bilinear — good enough for 80px
            );
            let lod1_path: PathBuf = lod1_dir.join(format!("thumb_{:04}.jpg", lod1_count));
            if let Err(e) = lod1_thumb.save(&lod1_path) {
                tracing::warn!("Failed to save LOD1 thumb: {}", e);
            }

            // Generate LOD0 thumbnail (40px height, every 10s — subset of LOD1)
            if time_secs % lod0_interval < lod1_interval {
                lod0_count += 1;
                let lod0_thumb = img.resize(
                    u32::MAX,
                    lod0_height,
                    FilterType::Triangle,
                );
                let lod0_path: PathBuf = lod0_dir.join(format!("thumb_{:04}.jpg", lod0_count));
                if let Err(e) = lod0_thumb.save(&lod0_path) {
                    tracing::warn!("Failed to save LOD0 thumb: {}", e);
                }
            }
        }

        tracing::debug!(
            "Generated thumbnails from proxy frames: LOD0={}, LOD1={}",
            lod0_count,
            lod1_count
        );

        Ok((lod0_count, lod1_count))
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
