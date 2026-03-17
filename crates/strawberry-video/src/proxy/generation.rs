//! # Proxy Generation
//!
//! Blocking FFmpeg functions for generating video proxies and image sequences.
//!
//! ## GPU Acceleration
//!
//! Uses the best available HW encoder (NVENC → QSV → AMF → CPU fallback).
//! HW encoders use dedicated ASICs — zero contention with the wgpu compositor.
//!
//! ## Process Control
//!
//! All FFmpeg subprocesses use `.spawn()` instead of `.status()` to get `Child`
//! handles with PIDs. PIDs are tracked in `active_proxy_pids` for suspend/resume
//! during playback.
//!
//! ## Priority
//!
//! On Windows, FFmpeg runs at `IDLE_PRIORITY_CLASS` (0x40) so it only uses CPU
//! when the system has spare capacity — zero impact on playback performance.

use crate::ffmpeg::{FFmpeg, FFmpegError};
use crate::hw_accel::{self, HwEncoder};
use std::path::Path;
use std::time::Duration;

use super::{ProxyJob, ProxyJobStatus, ProxyResult};

impl super::ProxyManager {
    /// Spawn a background thread to generate the proxy
    ///
    /// Uses std::thread::spawn instead of tokio::spawn because eframe
    /// apps don't run inside a Tokio runtime.
    ///
    /// ## GPU Acceleration
    ///
    /// The best available HW encoder (NVENC/QSV/AMF) is passed to the
    /// generation functions. HW encoders use dedicated ASICs — no contention
    /// with the wgpu render pipeline. CPU fallback uses IDLE_PRIORITY_CLASS.
    ///
    /// ## Process Control
    ///
    /// The FFmpeg subprocess PID is stored in `active_proxy_pids` so that
    /// playback can suspend/resume proxy processes to eliminate I/O contention.
    pub(crate) fn spawn_proxy_task(&self, job: ProxyJob) {
        let ffmpeg = self.ffmpeg.clone();
        let jobs = self.jobs.clone();
        let result_tx = self.result_tx.clone();
        let target_height = self.config.target_height;
        let jpeg_quality = self.config.jpeg_quality;
        let generate_thumbnails = self.config.generate_thumbnails;
        let active_pids = self.active_proxy_pids.clone();
        let hw_encoder: HwEncoder = ffmpeg.hw_caps().best_encoder;

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

            tracing::info!(
                "Proxy generation starting for asset {} using {} encoder",
                &asset_id.to_string()[..8],
                hw_encoder.display_name(),
            );

            // Run FFmpeg (blocking - this is a background thread)
            let result = if is_image_sequence {
                Self::generate_image_sequence_blocking(
                    &ffmpeg,
                    &source_path,
                    &proxy_path,
                    target_height,
                    jpeg_quality,
                    hw_encoder,
                    &active_pids,
                )
            } else {
                Self::generate_video_proxy_blocking(
                    &ffmpeg,
                    &source_path,
                    &proxy_path,
                    target_height,
                    hw_encoder,
                    &active_pids,
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

    /// Blocking function to generate VIDEO proxy using FFmpeg
    ///
    /// Uses the best available hardware encoder (NVENC/QSV/AMF) with CPU fallback.
    /// The FFmpeg subprocess is spawned at IDLE_PRIORITY_CLASS on Windows so it
    /// only runs when the system has spare capacity.
    ///
    /// ## Process Control
    ///
    /// Uses `.spawn()` instead of `.status()` to get a `Child` handle with PID.
    /// The PID is stored in `active_proxy_pids` for suspend/resume during playback.
    fn generate_video_proxy_blocking(
        ffmpeg: &FFmpeg,
        source: &Path,
        output: &Path,
        target_height: u32,
        hw_encoder: HwEncoder,
        active_pids: &std::sync::Mutex<Vec<u32>>,
    ) -> Result<(), FFmpegError> {
        use std::process::{Command, Stdio};

        tracing::info!(
            "Starting video proxy generation ({} encoder): {:?} -> {:?}",
            hw_encoder.display_name(),
            source,
            output
        );

        let mut cmd = Command::new(ffmpeg.ffmpeg_path());

        // Add HW-accelerated decode flags (offloads source decoding from CPU)
        let hwaccel_args: Vec<&str> = hw_accel::build_hwaccel_decode_args(hw_encoder);
        for arg in &hwaccel_args {
            cmd.arg(arg);
        }

        cmd.args(["-i"]).arg(source);

        // Add encoder-specific arguments (NVENC/QSV/AMF/CPU)
        let encoder_args: Vec<String> = hw_accel::build_encoder_args(hw_encoder);
        for arg in &encoder_args {
            cmd.arg(arg);
        }

        // Scale and audio settings
        cmd.args([
            "-vf",
            &format!("scale=-2:{}", target_height),
            "-c:a",
            "pcm_s16le",
            "-y",
        ])
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

        // Windows: BELOW_NORMAL_PRIORITY_CLASS (0x4000) — reduced priority
        // but still makes real progress. IDLE (0x40) was too aggressive —
        // it only runs when literally nothing else needs CPU, causing
        // 7-min videos to take 1-2 min. BELOW_NORMAL matches what
        // Premiere Pro and DaVinci Resolve use for background encodes.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
        }

        // Spawn subprocess (get handle for PID tracking)
        let mut child = cmd.spawn().map_err(FFmpegError::Io)?;

        // Track PID for suspend/resume during playback
        let pid: u32 = child.id();
        if let Ok(mut pids) = active_pids.lock() {
            pids.push(pid);
            tracing::debug!("Tracking proxy FFmpeg PID {} (video proxy)", pid);
        }

        // Wait for completion (blocking — this is a background thread)
        let status = child.wait().map_err(FFmpegError::Io)?;

        // Remove PID from tracking
        if let Ok(mut pids) = active_pids.lock() {
            pids.retain(|&p| p != pid);
        }

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
    ///
    /// ## GPU Acceleration
    ///
    /// When NVENC is available, uses a fully GPU-accelerated pipeline:
    /// 1. GPU decode (NVDEC)
    /// 2. GPU scale (scale_cuda) — keeps frames in VRAM, 10x faster than CPU
    /// 3. hwdownload — transfers only the small 720p frame (not full-res)
    /// 4. CPU mjpeg encode with -threads 4
    ///
    /// For other HW encoders or CPU fallback, uses CPU scaling with -threads 4.
    ///
    /// ## Process Control
    ///
    /// Uses `.spawn()` for PID tracking and BELOW_NORMAL priority on Windows.
    fn generate_image_sequence_blocking(
        ffmpeg: &FFmpeg,
        source: &Path,
        output_dir: &Path,
        target_height: u32,
        jpeg_quality: u32,
        hw_encoder: HwEncoder,
        active_pids: &std::sync::Mutex<Vec<u32>>,
    ) -> Result<(), FFmpegError> {
        use std::process::{Command, Stdio};

        tracing::info!(
            "Starting image sequence proxy ({} pipeline): {:?} -> {:?}",
            hw_encoder.display_name(),
            source,
            output_dir
        );

        // Output pattern: frame_00001.jpg, frame_00002.jpg, etc.
        let output_pattern = output_dir.join("frame_%05d.jpg");

        // Map JPEG quality percentage to FFmpeg qscale
        // qscale roughly maps: 2 ≈ 95%, 3 ≈ 90%, 5 ≈ 80%
        let qscale: &str = match jpeg_quality {
            90..=100 => "2",
            80..=89 => "3",
            70..=79 => "4",
            60..=69 => "5",
            _ => "6",
        };

        let mut cmd = Command::new(ffmpeg.ffmpeg_path());

        // Build GPU-optimized or CPU pipeline based on encoder type.
        //
        // NVENC path (fastest):
        //   -hwaccel cuda -hwaccel_output_format cuda → keep decoded frames in GPU VRAM
        //   -vf "scale_cuda=-2:720,hwdownload,format=nv12" → scale ON GPU, then download
        //   Only the small 720p frame is transferred GPU→CPU (vs full-res before)
        //
        // CPU/other path:
        //   -hwaccel auto → best available decode acceleration
        //   -vf "scale=-2:720" → CPU scale
        //   -threads 4 → parallel JPEG encoding
        let vf_string: String;
        if hw_encoder == HwEncoder::Nvenc {
            cmd.args([
                "-hwaccel", "cuda",
                "-hwaccel_output_format", "cuda",
            ]);
            // GPU scale + download. format=nv12 ensures mjpeg gets a compatible pixel format.
            vf_string = format!(
                "scale_cuda=-2:{},hwdownload,format=nv12",
                target_height
            );
        } else {
            // Non-NVENC: use generic hwaccel decode with CPU scale
            let hwaccel_args: Vec<&str> = hw_accel::build_hwaccel_decode_args(hw_encoder);
            for arg in &hwaccel_args {
                cmd.arg(arg);
            }
            vf_string = format!("scale=-2:{}", target_height);
        }

        cmd.args(["-i"])
            .arg(source)
            .args([
                "-vf", &vf_string,
                "-qscale:v", qscale,
                "-threads", "4",  // Parallel JPEG encoding
                "-y",
            ])
            .arg(&output_pattern)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        // Windows: BELOW_NORMAL_PRIORITY_CLASS — proxy won't starve playback
        // but still makes real progress (see generate_video_proxy_blocking)
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
        }

        // Spawn subprocess with PID tracking
        let mut child = cmd.spawn().map_err(FFmpegError::Io)?;

        let pid: u32 = child.id();
        if let Ok(mut pids) = active_pids.lock() {
            pids.push(pid);
            tracing::debug!("Tracking proxy FFmpeg PID {} (image sequence)", pid);
        }

        let status = child.wait().map_err(FFmpegError::Io)?;

        // Remove PID from tracking
        if let Ok(mut pids) = active_pids.lock() {
            pids.retain(|&p| p != pid);
        }

        if status.success() {
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
}
