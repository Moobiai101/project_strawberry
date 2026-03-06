//! # Audio Mixer Module
//!
//! Production-grade multi-track audio mixer powered by **kira**.
//!
//! ## Architecture
//!
//! ```text
//! strawberry-render (ECS)           strawberry-audio/mixer
//! ────────────────────              ──────────────────────
//! audio_mixer_sync_system ──────→   KiraMixerService
//!   reads: Playhead, Clip,            wraps: kira::AudioManager
//!          AudioClip                  manages: HashMap<Uuid, ClipAudioEntry>
//!   writes: volume/pan/mute           each clip → StaticSoundData + Handle
//!                                     master bus → main track volume
//! ```
//!
//! ## Key Types
//!
//! - [`KiraMixerService`] — The main mixer. Manages `AudioManager`, registers/plays clips.
//! - [`ClipAudioEntry`] — Per-clip state: `StaticSoundData` + optional `StaticSoundHandle`.
//!
//! ## Usage
//!
//! ```ignore
//! let mut mixer = KiraMixerService::new()?;
//! mixer.register_clip_audio(asset_uuid, &path)?;
//! mixer.play_clip_at_offset(&asset_uuid, 2.5)?; // play from 2.5s
//! mixer.update_clip_params(&asset_uuid, -6.0, 0.0, false); // -6dB, center, not muted
//! ```

pub mod clip_sound;
pub mod mixer_service;

pub use clip_sound::ClipAudioEntry;
pub use mixer_service::KiraMixerService;
