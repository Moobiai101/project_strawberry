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
            max_concurrent_jobs: 2,
            cache_dir,
            use_image_sequences: true,
            generate_thumbnails: true,
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
    pub fn with_config(config: ProxyConfig) -> Result<Self, FFmpegError> {
        let ffmpeg = Arc::new(FFmpeg::new()?);

        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&config.cache_dir).map_err(|e| FFmpegError::Io(e))?;

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

    /// Queue a proxy generation job for an asset
    ///
    /// Returns the path where the proxy will be written.
    pub fn queue_proxy(
        &self,
        asset_id: Uuid,
        source_path: &Path,
        source_duration: Duration,
        frame_rate: f64,
    ) -> PathBuf {
        // Create asset-specific directory: proxies/<asset_id_hex>/
        let asset_dir = self
            .config
            .cache_dir
            .join(asset_id.to_string().replace("-", ""));
        let _ = std::fs::create_dir_all(&asset_dir);

        // Determine proxy path based on configuration
        let (proxy_path, is_image_sequence) = if self.config.use_image_sequences {
            // Image sequence: proxies/<id>/ (frames will be frame_00001.jpg, etc.)
            (asset_dir.clone(), true)
        } else {
            // Video proxy: proxies/<id>/proxy.mov
            (asset_dir.join("proxy.mov"), false)
        };

        let job = ProxyJob::new(
            asset_id,
            source_path.to_path_buf(),
            proxy_path.clone(),
            source_duration,
            is_image_sequence,
            frame_rate,
        );

        // Store job
        {
            let mut jobs = self.jobs.write();
            jobs.insert(asset_id, job.clone());
        }

        tracing::info!(
            "Queued proxy job for asset {} ({}, {}p, {:.1}fps - {})",
            &asset_id.to_string()[..8],
            source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            self.config.target_height,
            frame_rate,
            if is_image_sequence {
                "JPEG sequence"
            } else {
                "DNxHR LB"
            },
        );

        // Spawn background generation
        self.spawn_proxy_task(job);

        proxy_path
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

    /// Get the proxy directory for an asset (for storing thumbnails alongside frames)
    ///
    /// Returns the proxy directory path even if proxy is not yet complete.
    pub fn get_proxy_dir(&self, asset_id: &Uuid) -> PathBuf {
        self.config
            .cache_dir
            .join(asset_id.to_string().replace("-", ""))
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
        assert_eq!(config.max_concurrent_jobs, 2);
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
