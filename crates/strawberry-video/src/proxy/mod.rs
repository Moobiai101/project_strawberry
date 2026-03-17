//! # Proxy Manager
//!
//! Manages generation of edit-friendly proxy files for video assets.
//!
//! ## Why Proxies?
//!
//! H.264/H.265 use temporal compression (I/P/B frames in GOPs).
//! To decode frame N, you might need to decode frames N-15 through N.
//! This makes scrubbing slow (~100-200ms per seek).
//!
//! Proxies solve this by converting to intra-frame codecs:
//! - **JPEG Image Sequence**: ~2-5ms per frame (instant scrubbing!)
//! - **DNxHR LB**: ~50ms per frame (smaller but still needs decode)
//!
//! ## Module Structure
//!
//! - `mod.rs` — Types, config, manager core, job tracking, accessors
//! - `generation.rs` — FFmpeg proxy generation (GPU-accelerated, IDLE priority)
//! - `process_control.rs` — OS-level suspend/resume of FFmpeg processes
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  ProxyManager                                               │
//! │  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐   │
//! │  │ ProxyJob    │     │ ProxyJob    │     │ ProxyJob    │   │
//! │  │ asset_1     │     │ asset_2     │     │ asset_3     │   │
//! │  └─────────────┘     └─────────────┘     └─────────────┘   │
//! │         ↓                   ↓                   ↓          │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │  Background Threads (std::thread::spawn)            │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │         ↓                   ↓                   ↓          │
//! │  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐   │
//! │  │ FFmpeg      │     │ FFmpeg      │     │ FFmpeg      │   │
//! │  │ GPU/IDLE    │     │ GPU/IDLE    │     │ GPU/IDLE    │   │
//! │  └─────────────┘     └─────────────┘     └─────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod generation;
mod process_control;

use crate::ffmpeg::{FFmpeg, FFmpegError};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Compute a deterministic content-based hash for proxy cache keying.
///
/// Uses file path (canonicalized), file size, and modification time to produce
/// a 32-character hex string. Same file → same hash → same proxy directory.
/// This is how Premiere Pro and DaVinci Resolve handle proxy caching.
///
/// ## Why not just file path?
///
/// If the user re-encodes the same file (same path, different content), the
/// mtime and size will change, producing a new hash → new proxy. This ensures
/// stale proxies are never served for modified source files.
pub fn compute_proxy_content_hash(source_path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();

    // Use canonical path to handle symlinks and relative paths
    let canonical: PathBuf = std::fs::canonicalize(source_path)
        .unwrap_or_else(|_| source_path.to_path_buf());
    canonical.hash(&mut hasher);

    // Include file size and mtime for content-change detection
    if let Ok(metadata) = std::fs::metadata(source_path) {
        metadata.len().hash(&mut hasher);
        if let Ok(mtime) = metadata.modified() {
            if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                duration.as_secs().hash(&mut hasher);
            }
        }
    }

    format!("{:016x}", hasher.finish())
}

/// Status of a single proxy generation job
#[derive(Debug, Clone)]
pub enum ProxyJobStatus {
    /// Job is queued but not yet started
    Queued,
    /// Job is currently running
    Running { progress: f32 },
    /// Job completed successfully
    Completed,
    /// Job failed with error message
    Failed { error: String },
    /// Job was cancelled
    Cancelled,
}

impl ProxyJobStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ProxyJobStatus::Running { .. })
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, ProxyJobStatus::Completed)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, ProxyJobStatus::Failed { .. })
    }

    pub fn progress(&self) -> Option<f32> {
        match self {
            ProxyJobStatus::Running { progress } => Some(*progress),
            ProxyJobStatus::Completed => Some(1.0),
            _ => None,
        }
    }
}

/// A single proxy generation job
#[derive(Debug, Clone)]
pub struct ProxyJob {
    pub asset_id: Uuid,
    pub source_path: PathBuf,
    pub proxy_path: PathBuf,
    pub status: ProxyJobStatus,
    pub source_duration: Duration,
    pub is_image_sequence: bool,
    pub frame_rate: f64,
}

impl ProxyJob {
    pub fn new(
        asset_id: Uuid,
        source_path: PathBuf,
        proxy_path: PathBuf,
        source_duration: Duration,
        is_image_sequence: bool,
        frame_rate: f64,
    ) -> Self {
        Self {
            asset_id,
            source_path,
            proxy_path,
            status: ProxyJobStatus::Queued,
            source_duration,
            is_image_sequence,
            frame_rate,
        }
    }
}

/// Result sent from background proxy generation
#[derive(Debug, Clone)]
pub enum ProxyResult {
    /// Progress update
    Progress { asset_id: Uuid, progress: f32 },
    /// Generation completed successfully
    Completed { asset_id: Uuid, proxy_path: PathBuf },
    /// Generation failed
    Failed { asset_id: Uuid, error: String },
}

/// Configuration for proxy generation
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Target height for proxies (width auto-calculated from aspect ratio)
    pub target_height: u32,
    /// JPEG quality for image sequence proxies (0-100)
    pub jpeg_quality: u32,
    /// Maximum number of concurrent proxy generation jobs
    pub max_concurrent_jobs: usize,
    /// Directory to store proxy files
    pub cache_dir: PathBuf,
    /// Whether to generate image sequences (true) or video proxies (false)
    pub use_image_sequences: bool,
    /// Whether to generate LOD thumbnails after proxy
    pub generate_thumbnails: bool,
    /// Maximum age of proxy cache entries in days before auto-cleanup
    pub max_cache_age_days: u32,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        // Default proxy cache directory
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("strawberry")
            .join("proxies");

        Self {
            target_height: 720,
            jpeg_quality: 85,
            max_concurrent_jobs: 1, // Sequential: avoids GPU/CPU/disk contention
            cache_dir,
            use_image_sequences: true,
            generate_thumbnails: true,
            max_cache_age_days: 7,
        }
    }
}

/// Manager for proxy generation jobs
///
/// Handles queuing, running, and monitoring proxy generation.
/// Thread-safe via internal locking.
///
/// ## Process Control
///
/// Active FFmpeg subprocess PIDs are tracked in `active_proxy_pids`.
/// This enables suspend/resume during playback to eliminate I/O contention.
pub struct ProxyManager {
    /// FFmpeg instance for spawning encode processes
    pub(crate) ffmpeg: Arc<FFmpeg>,
    /// Active proxy generation jobs (keyed by asset UUID)
    pub(crate) jobs: Arc<RwLock<HashMap<Uuid, ProxyJob>>>,
    /// Channel sender for proxy results
    pub(crate) result_tx: mpsc::Sender<ProxyResult>,
    /// Channel receiver for proxy results (wrapped in Mutex for Sync)
    result_rx: std::sync::Mutex<mpsc::Receiver<ProxyResult>>,
    /// Configuration
    pub(crate) config: ProxyConfig,
    /// Active FFmpeg process PIDs for suspend/resume during playback
    pub(crate) active_proxy_pids: Arc<std::sync::Mutex<Vec<u32>>>,
}

impl ProxyManager {
    /// Create a new ProxyManager with default configuration
    pub fn new() -> Result<Self, FFmpegError> {
        Self::with_config(ProxyConfig::default())
    }

    /// Create with custom configuration
    ///
    /// Runs stale cache cleanup on startup (removes proxy directories
    /// older than `max_cache_age_days`).
    pub fn with_config(config: ProxyConfig) -> Result<Self, FFmpegError> {
        let ffmpeg = Arc::new(FFmpeg::new()?);

        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&config.cache_dir).map_err(|e| FFmpegError::Io(e))?;

        // Clean up stale proxy directories on startup
        Self::cleanup_stale_proxies(&config.cache_dir, config.max_cache_age_days);

        let (tx, rx) = mpsc::channel();

        Ok(Self {
            ffmpeg,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            result_tx: tx,
            result_rx: std::sync::Mutex::new(rx),
            config,
            active_proxy_pids: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }

    /// Remove proxy directories older than `max_age_days` from the cache.
    ///
    /// Called once at startup. Scans the proxies directory for subdirectories
    /// and removes any whose last modification time exceeds the threshold.
    /// Logs total directories removed and bytes freed.
    fn cleanup_stale_proxies(cache_dir: &Path, max_age_days: u32) {
        let max_age = std::time::Duration::from_secs(max_age_days as u64 * 24 * 60 * 60);
        let now = std::time::SystemTime::now();
        let mut removed_count: u32 = 0;
        let mut freed_bytes: u64 = 0;

        let entries = match std::fs::read_dir(cache_dir) {
            Ok(entries) => entries,
            Err(_) => return, // No cache dir yet — nothing to clean
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Check modification time: prefer .last_used marker (written on cache hit),
            // fall back to directory mtime
            let marker: PathBuf = path.join(".last_used");
            let mtime = if marker.exists() {
                match marker.metadata().and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => continue,
                }
            } else {
                match path.metadata().and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => continue,
                }
            };

            let age = match now.duration_since(mtime) {
                Ok(age) => age,
                Err(_) => continue, // Clock skew — skip
            };

            if age > max_age {
                // Calculate size before deleting
                let dir_size: u64 = Self::dir_size_bytes(&path);
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    tracing::warn!("Failed to clean stale proxy {:?}: {}", path, e);
                    continue;
                }
                removed_count += 1;
                freed_bytes += dir_size;
            }
        }

        if removed_count > 0 {
            tracing::info!(
                "Proxy cache cleanup: removed {} stale director{} ({:.1} MB freed, threshold: {} days)",
                removed_count,
                if removed_count == 1 { "y" } else { "ies" },
                freed_bytes as f64 / (1024.0 * 1024.0),
                max_age_days,
            );
        }
    }

    /// Calculate total size of a directory recursively (for cleanup logging)
    fn dir_size_bytes(path: &Path) -> u64 {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let p = e.path();
                        if p.is_dir() {
                            Self::dir_size_bytes(&p)
                        } else {
                            e.metadata().map(|m| m.len()).unwrap_or(0)
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Queue a proxy generation job for an asset.
    ///
    /// ## Cache Reuse
    ///
    /// Uses a **content-based hash** (file path + size + mtime) for the proxy
    /// directory name. Same video file → same hash → same directory. If a valid
    /// proxy already exists on disk, skips FFmpeg entirely and immediately
    /// marks the job as Completed.
    ///
    /// Returns the path where the proxy will be (or already is) stored.
    pub fn queue_proxy(
        &self,
        asset_id: Uuid,
        source_path: &Path,
        source_duration: Duration,
        frame_rate: f64,
    ) -> PathBuf {
        // Content-based directory key: same file → same proxy directory
        let content_hash: String = compute_proxy_content_hash(source_path);
        let asset_dir: PathBuf = self.config.cache_dir.join(&content_hash);
        let _ = std::fs::create_dir_all(&asset_dir);

        // Determine proxy path based on configuration
        let (proxy_path, is_image_sequence) = if self.config.use_image_sequences {
            (asset_dir.clone(), true)
        } else {
            (asset_dir.join("proxy.mov"), false)
        };

        // Check if a valid proxy already exists on disk (cache HIT)
        if self.is_proxy_cache_valid(&proxy_path, source_path, is_image_sequence) {
            tracing::info!(
                "Proxy cache HIT for asset {} ({}) — reusing existing proxy",
                &asset_id.to_string()[..8],
                source_path.file_name().unwrap_or_default().to_string_lossy(),
            );

            // Register as already-completed job
            let mut job = ProxyJob::new(
                asset_id,
                source_path.to_path_buf(),
                proxy_path.clone(),
                source_duration,
                is_image_sequence,
                frame_rate,
            );
            job.status = ProxyJobStatus::Completed;
            self.jobs.write().insert(asset_id, job);

            // Send completion immediately so VideoAssetManager picks it up
            let _ = self.result_tx.send(ProxyResult::Completed {
                asset_id,
                proxy_path: proxy_path.clone(),
            });

            // Touch the directory mtime so cleanup doesn't evict active proxies
            if let Ok(metadata) = std::fs::metadata(&asset_dir) {
                let _ = std::fs::set_permissions(&asset_dir, metadata.permissions());
                // On most OS, opening and closing a file in the dir updates atime.
                // But for mtime, we write a small marker file.
                let marker: PathBuf = asset_dir.join(".last_used");
                let _ = std::fs::write(&marker, b"");
            }

            return proxy_path;
        }

        // Cache MISS — generate proxy
        let job = ProxyJob::new(
            asset_id,
            source_path.to_path_buf(),
            proxy_path.clone(),
            source_duration,
            is_image_sequence,
            frame_rate,
        );

        self.jobs.write().insert(asset_id, job.clone());

        tracing::info!(
            "Proxy cache MISS for asset {} ({}, {}p, {:.1}fps - {})",
            &asset_id.to_string()[..8],
            source_path.file_name().unwrap_or_default().to_string_lossy(),
            self.config.target_height,
            frame_rate,
            if is_image_sequence { "JPEG sequence" } else { "DNxHR LB" },
        );

        self.spawn_proxy_task(job);
        proxy_path
    }

    /// Check if an existing proxy on disk is still valid for the given source.
    ///
    /// For image sequences: checks that frame_00001.jpg exists AND the
    /// thumbnail manifest validates against the source file (size + mtime).
    /// For video proxies: checks that proxy.mov exists.
    fn is_proxy_cache_valid(
        &self,
        proxy_path: &Path,
        source_path: &Path,
        is_image_sequence: bool,
    ) -> bool {
        if is_image_sequence {
            // Must have at least the first frame
            let first_frame: PathBuf = proxy_path.join("frame_00001.jpg");
            if !first_frame.exists() {
                return false;
            }
            // Validate via thumbnail manifest (has source size + mtime check)
            let thumb_manifest: PathBuf = proxy_path.join("thumbnails").join("manifest.json");
            if thumb_manifest.exists() {
                if let Ok(json) = std::fs::read_to_string(&thumb_manifest) {
                    if let Ok(manifest) = serde_json::from_str::<crate::thumbnail::ThumbnailManifest>(&json) {
                        return manifest.is_valid_for(source_path);
                    }
                }
            }
            // No manifest but frames exist — still valid (manifest may not have been generated)
            true
        } else {
            proxy_path.exists() && proxy_path.is_file()
        }
    }

    /// Poll for completed proxy results
    ///
    /// Call this periodically in the main update loop.
    /// Returns a list of results to process.
    pub fn poll_results(&self) -> Vec<ProxyResult> {
        let mut results = Vec::new();

        // Lock the receiver (Mutex required because Receiver is !Sync)
        if let Ok(rx) = self.result_rx.lock() {
            while let Ok(result) = rx.try_recv() {
                results.push(result);
            }
        }

        results
    }

    /// Get job status for an asset
    pub fn get_job_status(&self, asset_id: &Uuid) -> Option<ProxyJobStatus> {
        self.jobs.read().get(asset_id).map(|j| j.status.clone())
    }

    /// Get proxy path for an asset (if exists and completed)
    pub fn get_proxy_path(&self, asset_id: &Uuid) -> Option<PathBuf> {
        self.jobs.read().get(asset_id).and_then(|j| {
            if j.status.is_completed() && j.proxy_path.exists() {
                Some(j.proxy_path.clone())
            } else {
                None
            }
        })
    }

    /// Check if a proxy exists for an asset
    pub fn has_proxy(&self, asset_id: &Uuid) -> bool {
        self.get_proxy_path(asset_id).is_some()
    }

    /// Cancel a pending or running job
    pub fn cancel_job(&self, asset_id: &Uuid) {
        let mut jobs = self.jobs.write();
        if let Some(job) = jobs.get_mut(asset_id) {
            job.status = ProxyJobStatus::Cancelled;
            tracing::info!(
                "Cancelled proxy job for asset {}",
                &asset_id.to_string()[..8]
            );
        }
        // Note: This doesn't stop an already-running FFmpeg process
        // For that, we'd need to track the process handle
    }

    /// Get all active jobs (queued or running)
    pub fn active_jobs(&self) -> Vec<ProxyJob> {
        self.jobs
            .read()
            .values()
            .filter(|j| {
                matches!(
                    j.status,
                    ProxyJobStatus::Queued | ProxyJobStatus::Running { .. }
                )
            })
            .cloned()
            .collect()
    }

    /// Get count of active jobs
    pub fn active_job_count(&self) -> usize {
        self.jobs
            .read()
            .values()
            .filter(|j| {
                matches!(
                    j.status,
                    ProxyJobStatus::Queued | ProxyJobStatus::Running { .. }
                )
            })
            .count()
    }

    /// Clear completed jobs from memory
    pub fn clear_completed(&self) {
        let mut jobs = self.jobs.write();
        jobs.retain(|_, j| {
            !matches!(
                j.status,
                ProxyJobStatus::Completed | ProxyJobStatus::Failed { .. }
            )
        });
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &Path {
        &self.config.cache_dir
    }

    /// Get the path to a specific frame in an image sequence proxy
    ///
    /// For INSTANT scrubbing: calculates frame number from timestamp and returns
    /// the path to the JPEG file. No decoding needed - just load the image!
    ///
    /// Returns None if:
    /// - Asset doesn't have a completed proxy
    /// - Proxy is not an image sequence
    /// - Frame file doesn't exist
    pub fn get_frame_path(&self, asset_id: &Uuid, time_seconds: f64) -> Option<PathBuf> {
        let jobs = self.jobs.read();
        let job = jobs.get(asset_id)?;

        // Must be completed image sequence
        if !job.status.is_completed() || !job.is_image_sequence {
            return None;
        }

        // Calculate frame number (1-indexed for ffmpeg output)
        // FFmpeg outputs frame_00001.jpg, frame_00002.jpg, etc.
        let frame_number = (time_seconds * job.frame_rate).round() as u32 + 1;

        // Build frame path
        let frame_filename = format!("frame_{:05}.jpg", frame_number);
        let frame_path = job.proxy_path.join(&frame_filename);

        // Only return if file exists
        if frame_path.exists() {
            Some(frame_path)
        } else {
            None
        }
    }

    /// Check if an asset has an image sequence proxy
    pub fn is_image_sequence(&self, asset_id: &Uuid) -> bool {
        self.jobs
            .read()
            .get(asset_id)
            .map(|j| j.is_image_sequence && j.status.is_completed())
            .unwrap_or(false)
    }

    /// Delete the proxy folder for an asset
    ///
    /// Call this when an asset is deleted from the project to clean up disk space.
    /// Also removes the job from the internal tracking.
    ///
    /// Returns Ok(()) if deletion succeeded or folder didn't exist.
    pub fn delete_proxy(&self, asset_id: &Uuid) -> Result<(), std::io::Error> {
        // Get proxy path before removing from jobs
        let proxy_path = {
            let jobs = self.jobs.read();
            jobs.get(asset_id).map(|j| j.proxy_path.clone())
        };

        // Remove from job tracking
        {
            let mut jobs = self.jobs.write();
            jobs.remove(asset_id);
        }

        // Delete the proxy folder/file on disk
        if let Some(path) = proxy_path {
            if path.exists() {
                if path.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                    tracing::info!("Deleted proxy folder: {:?}", path);
                } else {
                    std::fs::remove_file(&path)?;
                    tracing::info!("Deleted proxy file: {:?}", path);
                }
            }
        }

        Ok(())
    }

    /// Get the proxy directory for a source file (for storing thumbnails alongside frames)
    ///
    /// Uses content-based hash: same file → same directory.
    /// Returns the path even if proxy is not yet complete.
    pub fn get_proxy_dir(&self, source_path: &Path) -> PathBuf {
        let content_hash: String = compute_proxy_content_hash(source_path);
        self.config.cache_dir.join(content_hash)
    }
}

impl Default for ProxyManager {
    fn default() -> Self {
        Self::new().expect("Failed to create ProxyManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_config_default() {
        let config = ProxyConfig::default();
        assert_eq!(config.target_height, 720);
        assert_eq!(config.max_concurrent_jobs, 1);
        assert!(config.cache_dir.ends_with("proxies"));
    }

    #[test]
    fn test_proxy_job_status() {
        let status = ProxyJobStatus::Running { progress: 0.5 };
        assert!(status.is_running());
        assert_eq!(status.progress(), Some(0.5));

        let completed = ProxyJobStatus::Completed;
        assert!(completed.is_completed());
        assert_eq!(completed.progress(), Some(1.0));
    }
}
