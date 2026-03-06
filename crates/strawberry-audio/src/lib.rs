//! # Strawberry Audio
//!
//! ECS-agnostic audio playback, decode, and multi-track mixing service.
//!
//! ## Architecture
//!
//! - **kira** for production-grade multi-track mixing (volume, pan, effects)
//! - FFmpeg subprocess for audio decoding (consistent with video)
//! - cpal for real-time audio output (via kira)
//! - rubato for production-grade sample rate conversion (speed changes)
//! - Lock-free ring buffer for smooth playback
//! - Drift detection and automatic resync
//!
//! ## Mixer (NEW)
//!
//! The `mixer` module provides `KiraMixerService` — a multi-track mixer
//! that can play N clips simultaneously with per-clip volume, pan, and mute.
//! See [`mixer`] module docs for architecture details.

pub mod ffmpeg_audio;
pub mod mixer;
pub mod playback;
pub mod resampler;

pub use ffmpeg_audio::{AudioDecodeError, AudioMetadata, AudioSamples, FFmpegAudioDecoder};
pub use mixer::{ClipAudioEntry, KiraMixerService};
pub use playback::{AudioError, AudioPlaybackService, LoadedAudio, PlaybackState, SyncStats};
pub use resampler::{ResamplerConfig, SpeedResampler};
