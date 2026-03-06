//! # Proxy Manager
//!
//! Manages generation of edit-friendly proxy files for video assets.
//!
//! ## Why Proxies?
//!
//! H.264/H.265 use temporal compression (I/P/B frames in GOPs).
//! To seek to frame 150, the decoder must decode from the nearest I-frame,
//! which can take 30-100ms per seek. This makes scrubbing nearly impossible.
//!
//! **Solution**: Generate intra-frame codec proxies (DNxHR LB).
//! Every frame is a standalone image → instant seeking.
//!
//! ## Proxy Codec Selection
//!
//! We use **DNxHR LB** (Low Bandwidth):
//! - Intra-frame: Every frame is standalone
//! - Cross-platform: Works on Windows (unlike ProRes encoding)
//! - Small-ish files: ~1/4 size of ProRes 422
//! - Professional: Used by DaVinci Resolve, Avid, etc.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      ProxyManager                           │
//! │                                                             │
//! │  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐   │
//! │  │ ProxyJob    │     │ ProxyJob    │     │ ProxyJob    │   │
//! │  │ asset_1     │     │ asset_2     │     │ asset_3     │   │
//! │  └─────────────┘     └─────────────┘     └─────────────┘   │
//! │         ↓                   ↓                   ↓          │
//! │  ┌────────────────────────────────────────────────────┐    │
//! │  │              Background Thread Pool                 │    │
//! │  │              (tokio::spawn_blocking)                │    │
//! │  └────────────────────────────────────────────────────┘    │
//! │         ↓                   ↓                   ↓          │
//! │  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐   │
//! │  │ ffmpeg      │     │ ffmpeg      │     │ ffmpeg      │   │
//! │  │ -c:v dnxhd  │     │ -c:v dnxhd  │     │ -c:v dnxhd  │   │
//! │  └─────────────┘     └─────────────┘     └─────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

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
    /// Job is actively running (progress 0.0 to 1.0)
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
    /// UUID of the asset this proxy is for
    pub asset_id: Uuid,
    /// Source video path
    pub source_path: PathBuf,
    /// Output proxy path (directory for image sequence, file for video proxy)
    pub proxy_path: PathBuf,
    /// Current status
    pub status: ProxyJobStatus,
    /// Source video duration (for progress calculation)
    pub source_duration: Duration,
    /// Whether this is an image sequence (true) or video proxy (false)
    pub is_image_sequence: bool,
    /// Frame rate of the source video (needed for image sequence lookup)
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
    /// Directory to store proxy files
    pub cache_dir: PathBuf,
    /// Target height for proxy (width is auto-calculated)
    pub target_height: u32,
    /// Maximum concurrent proxy jobs
    pub max_concurrent_jobs: usize,
    /// Use image sequence (JPEG frames) instead of video proxy
    /// - true: Generate JPEG frames for instant scrubbing (~2ms per frame load)
    /// - false: Generate DNxHR video for smaller disk usage (~50ms per frame decode)
    pub use_image_sequence: bool,
    /// JPEG quality for image sequence (1-100, higher = larger files + better quality)
    pub jpeg_quality: u32,
    /// Generate LOD thumbnails alongside proxy
    /// - true: Auto-generate LOD0 (40px) and LOD1 (80px) thumbnails
    /// - false: Skip thumbnail generation (for faster proxy-only workflow)
    pub generate_thumbnails: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        // Default cache directory in user's app data
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("strawberry")
            .join("proxies");

        Self {
            cache_dir,
            target_height: 720,
            max_concurrent_jobs: 2,    // Don't overwhelm the system
            use_image_sequence: true,  // Default to instant scrubbing!
            jpeg_quality: 85,          // Good balance of quality vs file size
            generate_thumbnails: true, // Auto-generate thumbnails for filmstrip
        }
    }
}

/// Manager for proxy generation jobs
///
/// Handles queuing, running, and monitoring proxy generation.
/// Thread-safe via internal locking.
pub struct ProxyManager {
    /// Configuration
    config: ProxyConfig,
    /// Active and completed jobs (thread-safe)
    jobs: Arc<RwLock<HashMap<Uuid, ProxyJob>>>,
    /// FFmpeg wrapper
    ffmpeg: Arc<FFmpeg>,
    /// Channel sender (cloned for each background thread)
    result_tx: mpsc::Sender<ProxyResult>,
    /// Channel receiver (for polling results) - uses Mutex because Receiver is !Sync
    result_rx: Arc<std::sync::Mutex<mpsc::Receiver<ProxyResult>>>,
}

impl ProxyManager {
    /// Create a new ProxyManager with default configuration
    pub fn new() -> Result<Self, FFmpegError> {
        Self::with_config(ProxyConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: ProxyConfig) -> Result<Self, FFmpegError> {
        // Ensure cache directory exists
        std::fs::create_dir_all(&config.cache_dir).map_err(|e| {
            FFmpegError::ExecutionFailed(format!("Failed to create cache dir: {}", e))
        })?;

        let ffmpeg = FFmpeg::new()?;
        let (result_tx, result_rx) = mpsc::channel();

        Ok(Self {
            config,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            ffmpeg: Arc::new(ffmpeg),
            result_tx,
            result_rx: Arc::new(std::sync::Mutex::new(result_rx)),
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
        let use_image_sequence = self.config.use_image_sequence;

        // Generate proxy path (directory for image sequence, file for video)
        let proxy_path = if use_image_sequence {
            // Image sequence: asset_id directory containing frame_%05d.jpg
            let proxy_dir = self
                .config
                .cache_dir
                .join(asset_id.to_string().replace("-", ""));
            // Create directory for frames
            if let Err(e) = std::fs::create_dir_all(&proxy_dir) {
                tracing::error!("Failed to create proxy directory: {}", e);
            }
            proxy_dir
        } else {
            // Video proxy: single .mov file
            let proxy_filename = format!("{}_proxy.mov", asset_id.to_string().replace("-", ""));
            self.config.cache_dir.join(&proxy_filename)
        };

        // Create job
        let job = ProxyJob::new(
            asset_id,
            source_path.to_path_buf(),
            proxy_path.clone(),
            source_duration,
            use_image_sequence,
            frame_rate,
        );

        // Store job
        {
            let mut jobs = self.jobs.write();
            jobs.insert(asset_id, job.clone());
        }

        tracing::info!(
            "Queued {} proxy for asset {} -> {:?}",
            if use_image_sequence {
                "image sequence"
            } else {
                "video"
            },
            &asset_id.to_string()[..8],
            proxy_path
        );

        // Spawn background task
        self.spawn_proxy_task(job);

        proxy_path
    }

    /// Spawn a background thread to generate the proxy
    ///
    /// Uses std::thread::spawn instead of tokio::spawn because eframe
    /// apps don't run inside a Tokio runtime.
    ///
    /// After proxy generation completes, also generates LOD thumbnails
    /// for timeline filmstrip display (if generate_thumbnails is enabled).
    fn spawn_proxy_task(&self, job: ProxyJob) {
        let ffmpeg = self.ffmpeg.clone();
        let jobs = self.jobs.clone();
        let result_tx = self.result_tx.clone();
        let target_height = self.config.target_height;
        let jpeg_quality = self.config.jpeg_quality;
        let generate_thumbnails = self.config.generate_thumbnails;

        // Clone values we need in the spawned thread
        let asset_id = job.asset_id;
        let source_path = job.source_path.clone();
        let proxy_path = job.proxy_path.clone();
        let source_duration = job.source_duration;
        let is_image_sequence = job.is_image_sequence;

        // Use std::thread::spawn for background work (no Tokio runtime required)
        std::thread::spawn(move || {
            // Update status to running
            {
                let mut jobs_guard = jobs.write();
                if let Some(j) = jobs_guard.get_mut(&asset_id) {
                    j.status = ProxyJobStatus::Running { progress: 0.0 };
                }
            }

            // Run FFmpeg (blocking - this is a background thread)
            let result = if is_image_sequence {
                Self::generate_image_sequence_blocking(
                    &ffmpeg,
                    &source_path,
                    &proxy_path,
                    target_height,
                    jpeg_quality,
                )
            } else {
                Self::generate_video_proxy_blocking(
                    &ffmpeg,
                    &source_path,
                    &proxy_path,
                    target_height,
                )
            };

            // Handle result
            match result {
                Ok(()) => {
                    // Generate thumbnails if enabled
                    if generate_thumbnails {
                        if let Err(e) = Self::generate_thumbnails_blocking(
                            &source_path,
                            &proxy_path,
                            source_duration,
                        ) {
                            tracing::warn!("Thumbnail generation failed (non-fatal): {}", e);
                        }
                    }

                    // Mark as completed
                    let mut jobs_guard = jobs.write();
                    if let Some(j) = jobs_guard.get_mut(&asset_id) {
                        j.status = ProxyJobStatus::Completed;
                    }
                    let _ = result_tx.send(ProxyResult::Completed {
                        asset_id,
                        proxy_path,
                    });
                }
                Err(e) => {
                    // FFmpeg error
                    let error = e.to_string();
                    let mut jobs_guard = jobs.write();
                    if let Some(j) = jobs_guard.get_mut(&asset_id) {
                        j.status = ProxyJobStatus::Failed {
                            error: error.clone(),
                        };
                    }
                    let _ = result_tx.send(ProxyResult::Failed { asset_id, error });
                }
            }
        });
    }

    /// Generate LOD thumbnails for timeline filmstrip display
    ///
    /// Called after proxy generation completes. Generates:
    /// - LOD0 (40px, every 10 seconds) for zoomed-out view
    /// - LOD1 (80px, every 2 seconds) for normal editing view
    fn generate_thumbnails_blocking(
        source_path: &Path,
        proxy_path: &Path,
        duration: Duration,
    ) -> Result<(), FFmpegError> {
        use crate::thumbnail::ThumbnailManager;

        let thumb_manager = ThumbnailManager::new()?;
        thumb_manager.generate_all_lods(source_path, proxy_path, duration)?;
        Ok(())
    }

    /// Blocking function to generate VIDEO proxy using FFmpeg (DNxHR LB)
    ///
    /// This is slower for scrubbing (~50ms per frame) but uses less disk space.
    fn generate_video_proxy_blocking(
        ffmpeg: &FFmpeg,
        source: &Path,
        output: &Path,
        target_height: u32,
    ) -> Result<(), FFmpegError> {
        use std::process::{Command, Stdio};

        tracing::info!(
            "Starting video proxy generation: {:?} -> {:?}",
            source,
            output
        );

        // Build FFmpeg command for DNxHR LB proxy
        // DNxHR LB = Low Bandwidth, intra-frame codec
        let status = Command::new(ffmpeg.ffmpeg_path())
            .args(["-i"])
            .arg(source)
            .args([
                "-c:v",
                "dnxhd",
                "-profile:v",
                "dnxhr_lb", // Low Bandwidth (intra-frame!)
                "-vf",
                &format!("scale=-2:{}", target_height), // Maintain aspect ratio
                "-c:a",
                "pcm_s16le", // PCM audio (fast decode)
                "-y",        // Overwrite output
            ])
            .arg(output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(|e| FFmpegError::Io(e))?;

        if status.success() {
            tracing::info!("Video proxy generation completed: {:?}", output);
            Ok(())
        } else {
            Err(FFmpegError::ExecutionFailed(format!(
                "FFmpeg exited with code {:?}",
                status.code()
            )))
        }
    }

    /// Blocking function to generate IMAGE SEQUENCE proxy using FFmpeg
    ///
    /// Generates JPEG frames: frame_00001.jpg, frame_00002.jpg, etc.
    /// This enables INSTANT scrubbing (~2-5ms per frame load, no decode needed!)
    fn generate_image_sequence_blocking(
        ffmpeg: &FFmpeg,
        source: &Path,
        output_dir: &Path,
        target_height: u32,
        jpeg_quality: u32,
    ) -> Result<(), FFmpegError> {
        use std::process::{Command, Stdio};

        tracing::info!(
            "Starting image sequence proxy generation: {:?} -> {:?}",
            source,
            output_dir
        );

        // Output pattern: frame_00001.jpg, frame_00002.jpg, etc.
        let output_pattern = output_dir.join("frame_%05d.jpg");

        // Build FFmpeg command for JPEG image sequence
        // -qscale:v 2-5 = JPEG quality (2 = high quality, 31 = low quality)
        // qscale roughly maps to JPEG quality: 2 ≈ 95%, 3 ≈ 90%, 5 ≈ 80%
        let qscale = match jpeg_quality {
            90..=100 => "2",
            80..=89 => "3",
            70..=79 => "4",
            60..=69 => "5",
            _ => "6",
        };

        let status = Command::new(ffmpeg.ffmpeg_path())
            .args(["-i"])
            .arg(source)
            .args([
                "-vf",
                &format!("scale=-2:{}", target_height), // Maintain aspect ratio
                "-qscale:v",
                qscale,
                "-y", // Overwrite output
            ])
            .arg(&output_pattern)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(|e| FFmpegError::Io(e))?;

        if status.success() {
            // Count generated frames
            let frame_count = std::fs::read_dir(output_dir)
                .map(|entries| entries.filter_map(|e| e.ok()).count())
                .unwrap_or(0);
            tracing::info!(
                "Image sequence proxy completed: {} frames in {:?}",
                frame_count,
                output_dir
            );
            Ok(())
        } else {
            Err(FFmpegError::ExecutionFailed(format!(
                "FFmpeg image sequence generation exited with code {:?}",
                status.code()
            )))
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
