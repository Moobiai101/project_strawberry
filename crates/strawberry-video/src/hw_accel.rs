//! # Hardware Acceleration Detection
//!
//! Detects available GPU hardware encoders and decoders for proxy generation.
//!
//! ## Why GPU Acceleration for Proxies?
//!
//! NVENC/QSV/AMF are **dedicated encoder ASICs** — separate silicon from the
//! GPU's 3D rendering cores. Using them for proxy generation has **zero
//! contention** with the wgpu compositor pipeline. This is how Premiere Pro
//! and DaVinci Resolve handle background encoding.
//!
//! ## Encoder Fallback Chain
//!
//! 1. `h264_nvenc` (NVIDIA) — most common, ~10x faster than CPU
//! 2. `h264_qsv` (Intel QuickSync) — available on most Intel CPUs
//! 3. `h264_amf` (AMD AMF) — AMD Radeon GPUs
//! 4. CPU software fallback — `libx264` with `-threads 1` + `IDLE_PRIORITY`
//!
//! ## Decoder Acceleration
//!
//! For the decode (input) side of proxy generation, we use `-hwaccel auto`
//! which lets FFmpeg pick the best available: NVDEC → DXVA2 → VAAPI → CPU.

use std::path::Path;
use std::process::{Command, Stdio};

/// Available hardware encoder backends
///
/// Each variant maps to an FFmpeg encoder name. The enum is ordered by
/// preference — NVENC is preferred over QSV, QSV over AMF, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwEncoder {
    /// NVIDIA NVENC (dedicated ASIC, no GPU compute contention)
    Nvenc,
    /// Intel QuickSync Video (integrated GPU encoder)
    Qsv,
    /// AMD Advanced Media Framework (dedicated encoder on Radeon GPUs)
    Amf,
    /// CPU software encoding (fallback — runs at IDLE_PRIORITY with -threads 1)
    Cpu,
}

impl HwEncoder {
    /// Get the FFmpeg encoder name for video proxy encoding
    ///
    /// Returns the `-c:v` argument value for FFmpeg.
    pub fn ffmpeg_encoder_name(&self) -> &'static str {
        match self {
            HwEncoder::Nvenc => "h264_nvenc",
            HwEncoder::Qsv => "h264_qsv",
            HwEncoder::Amf => "h264_amf",
            HwEncoder::Cpu => "libx264",
        }
    }

    /// Get the FFmpeg hwaccel flag for GPU-accelerated decoding
    ///
    /// Used as `-hwaccel <value>` to offload decode from CPU.
    /// Returns `None` for CPU-only (no hwaccel flag needed).
    pub fn ffmpeg_hwaccel_decode(&self) -> Option<&'static str> {
        match self {
            HwEncoder::Nvenc => Some("cuda"),
            HwEncoder::Qsv => Some("qsv"),
            HwEncoder::Amf => Some("auto"),
            HwEncoder::Cpu => None,
        }
    }

    /// Check if this is a hardware-accelerated encoder (not CPU)
    pub fn is_hardware(&self) -> bool {
        !matches!(self, HwEncoder::Cpu)
    }

    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            HwEncoder::Nvenc => "NVIDIA NVENC",
            HwEncoder::Qsv => "Intel QuickSync",
            HwEncoder::Amf => "AMD AMF",
            HwEncoder::Cpu => "CPU (software)",
        }
    }
}

/// Result of hardware encoder detection
///
/// Stored on `FFmpeg` struct after initialization. Contains the best
/// available encoder and the full list of all detected encoders.
#[derive(Debug, Clone)]
pub struct HwAccelCapabilities {
    /// Best available encoder (first in fallback chain)
    pub best_encoder: HwEncoder,
    /// All detected hardware encoders (may be empty if CPU-only)
    pub available_encoders: Vec<HwEncoder>,
}

impl Default for HwAccelCapabilities {
    fn default() -> Self {
        Self {
            best_encoder: HwEncoder::Cpu,
            available_encoders: Vec::new(),
        }
    }
}

/// Detect available hardware encoders by probing FFmpeg
///
/// Runs `ffmpeg -encoders` and checks for known HW encoder names.
/// This is called once at FFmpeg initialization — the result is cached.
///
/// ## Performance
///
/// This takes ~50ms (one FFmpeg subprocess call). Only called once at startup.
pub fn detect_hw_encoders(ffmpeg_path: &Path) -> HwAccelCapabilities {
    let encoder_output: String = match Command::new(ffmpeg_path)
        .args(["-hide_banner", "-encoders"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
        Err(e) => {
            tracing::warn!("Failed to query FFmpeg encoders: {}", e);
            return HwAccelCapabilities::default();
        }
    };

    let mut available: Vec<HwEncoder> = Vec::new();

    // Check each HW encoder in preference order
    // FFmpeg encoder list format: " V..... h264_nvenc  NVIDIA NVENC H.264 encoder"
    let encoder_checks: &[(&str, HwEncoder)] = &[
        ("h264_nvenc", HwEncoder::Nvenc),
        ("h264_qsv", HwEncoder::Qsv),
        ("h264_amf", HwEncoder::Amf),
    ];

    for &(encoder_name, hw_encoder) in encoder_checks {
        if encoder_output.contains(encoder_name) {
            tracing::info!(
                "HW encoder detected: {} ({})",
                hw_encoder.display_name(),
                encoder_name
            );
            available.push(hw_encoder);
        }
    }

    let best_encoder: HwEncoder = available.first().copied().unwrap_or(HwEncoder::Cpu);

    tracing::info!(
        "Best proxy encoder: {} ({} HW encoder(s) available)",
        best_encoder.display_name(),
        available.len()
    );

    HwAccelCapabilities {
        best_encoder,
        available_encoders: available,
    }
}

/// Build FFmpeg hwaccel decode arguments for the given encoder
///
/// Returns the command-line arguments to enable hardware-accelerated
/// decoding. Called by proxy generation to offload source decode from CPU.
///
/// ## Example
///
/// For NVENC: `["-hwaccel", "cuda", "-hwaccel_output_format", "nv12"]`
/// For CPU: `[]` (empty — no hwaccel flags)
pub fn build_hwaccel_decode_args(encoder: HwEncoder) -> Vec<&'static str> {
    match encoder.ffmpeg_hwaccel_decode() {
        Some(hwaccel) => vec!["-hwaccel", hwaccel],
        None => Vec::new(),
    }
}

/// Build FFmpeg encoder arguments for proxy video encoding
///
/// Returns command-line arguments for the `-c:v` encoder selection
/// and any encoder-specific tuning parameters.
///
/// ## Encoder-Specific Settings
///
/// - **NVENC**: `-preset p4 -tune ll` (fast, low-latency preset)
/// - **QSV**: `-preset faster` (balanced speed/quality)
/// - **AMF**: `-quality speed` (prioritize encoding speed)
/// - **CPU**: `-preset ultrafast -threads 1` (minimal CPU impact)
pub fn build_encoder_args(encoder: HwEncoder) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-c:v".to_string(),
        encoder.ffmpeg_encoder_name().to_string(),
    ];

    match encoder {
        HwEncoder::Nvenc => {
            args.extend([
                "-preset".to_string(),
                "p4".to_string(), // Fast preset (lower = faster)
                "-tune".to_string(),
                "ll".to_string(), // Low-latency tune
                "-rc".to_string(),
                "vbr".to_string(), // Variable bitrate
                "-cq".to_string(),
                "28".to_string(), // Quality level (lower = better, 28 is good for proxy)
            ]);
        }
        HwEncoder::Qsv => {
            args.extend([
                "-preset".to_string(),
                "faster".to_string(), // Balance speed/quality
                "-global_quality".to_string(),
                "28".to_string(),
            ]);
        }
        HwEncoder::Amf => {
            args.extend([
                "-quality".to_string(),
                "speed".to_string(), // Prioritize speed
                "-rc".to_string(),
                "cqp".to_string(),
                "-qp_i".to_string(),
                "28".to_string(),
                "-qp_p".to_string(),
                "28".to_string(),
            ]);
        }
        HwEncoder::Cpu => {
            args.extend([
                "-preset".to_string(),
                "ultrafast".to_string(), // Minimal CPU impact
                "-crf".to_string(),
                "28".to_string(), // Constant quality
                "-threads".to_string(),
                "1".to_string(), // Single thread — don't starve the main app
            ]);
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hw_encoder_names() {
        assert_eq!(HwEncoder::Nvenc.ffmpeg_encoder_name(), "h264_nvenc");
        assert_eq!(HwEncoder::Qsv.ffmpeg_encoder_name(), "h264_qsv");
        assert_eq!(HwEncoder::Amf.ffmpeg_encoder_name(), "h264_amf");
        assert_eq!(HwEncoder::Cpu.ffmpeg_encoder_name(), "libx264");
    }

    #[test]
    fn test_hw_encoder_is_hardware() {
        assert!(HwEncoder::Nvenc.is_hardware());
        assert!(HwEncoder::Qsv.is_hardware());
        assert!(HwEncoder::Amf.is_hardware());
        assert!(!HwEncoder::Cpu.is_hardware());
    }

    #[test]
    fn test_build_encoder_args_cpu_has_threads_1() {
        let args: Vec<String> = build_encoder_args(HwEncoder::Cpu);
        assert!(args.contains(&"-threads".to_string()));
        assert!(args.contains(&"1".to_string()));
    }

    #[test]
    fn test_build_hwaccel_decode_args_nvenc() {
        let args: Vec<&str> = build_hwaccel_decode_args(HwEncoder::Nvenc);
        assert_eq!(args, vec!["-hwaccel", "cuda"]);
    }

    #[test]
    fn test_build_hwaccel_decode_args_cpu_is_empty() {
        let args: Vec<&str> = build_hwaccel_decode_args(HwEncoder::Cpu);
        assert!(args.is_empty());
    }

    #[test]
    fn test_default_capabilities() {
        let caps: HwAccelCapabilities = HwAccelCapabilities::default();
        assert_eq!(caps.best_encoder, HwEncoder::Cpu);
        assert!(caps.available_encoders.is_empty());
    }
}
