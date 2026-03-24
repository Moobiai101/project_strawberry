//! # Kira Mixer Service
//!
//! Production-grade multi-track audio mixer wrapping kira's `AudioManager`.
//!
//! ## Responsibilities
//!
//! - Manage `AudioManager<DefaultBackend>` (one per app lifetime)
//! - Register/unregister clip audio via `StaticSoundData`
//! - Start/stop per-clip playback with seek offsets
//! - Update per-clip volume (dB), panning, and mute each frame
//! - Master volume control on the main track
//!
//! ## Threading Model
//!
//! kira runs its own audio thread internally. All methods here are called
//! from the main thread. kira handles lock-free communication to the
//! audio thread via command queues — no mutexes on the hot path.

use std::collections::HashMap;
use std::path::Path;

use kira::sound::static_sound::StaticSoundData;
use kira::sound::PlaybackPosition;
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Panning, Tween};
use uuid::Uuid;

use crate::mixer::clip_sound::ClipAudioEntry;
use crate::waveform::{WaveformCache, compute_peaks_from_frames, DEFAULT_PEAK_COUNT};

/// Errors that can occur in the mixer.
#[derive(Debug, thiserror::Error)]
pub enum MixerError {
    #[error("Failed to initialize audio manager: {0}")]
    InitFailed(String),

    #[error("Failed to load audio file: {0}")]
    LoadFailed(String),

    #[error("Clip not found: {0}")]
    ClipNotFound(Uuid),

    #[error("Playback error: {0}")]
    PlaybackError(String),
}

/// Production-grade multi-track audio mixer powered by kira.
///
/// ## Lifecycle
///
/// 1. `KiraMixerService::new()` — creates `AudioManager`
/// 2. `register_clip_audio(uuid, path)` — loads audio per clip
/// 3. `play_clip_at_offset(uuid, secs)` — starts clip at source offset
/// 4. `update_clip_params(uuid, vol, pan, mute)` — per-frame updates
/// 5. `stop_clip(uuid)` — stops clip when it leaves playhead
pub struct KiraMixerService {
    /// kira audio manager — owns the audio thread and mixer.
    #[allow(dead_code)]
    manager: AudioManager<DefaultBackend>,

    /// Master sub-track — all clip sounds are routed through this.
    /// Allows master volume control independent of per-clip volume.
    master_track: TrackHandle,

    /// Per-clip audio entries, keyed by asset UUID.
    clips: HashMap<Uuid, ClipAudioEntry>,

    /// Current master volume (linear 0.0–1.0).
    master_volume: f32,

    /// Whether master is muted.
    master_muted: bool,

    /// Pre-computed waveform peaks, auto-populated on clip registration.
    /// Keyed by asset UUID, accessed by UI renderer for waveform display.
    waveform_cache: WaveformCache,
}

/// Convert linear slider value (0.0–2.0) to kira Decibels using a
/// **squared power curve** for perceptually responsive volume control.
///
/// The slider percentage is squared before dB conversion, which makes
/// the full slider range feel more responsive to the user:
///
/// | Slider | Amplitude² | dB      | Perceived Effect        |
/// |--------|-----------|---------|-------------------------|
/// | 200%   | 4.0       | +12.0   | Very noticeable boost   |
/// | 150%   | 2.25      | +7.0    | Clear boost             |
/// | 100%   | 1.0       | 0.0     | Unity gain              |
/// | 50%    | 0.25      | -12.0   | Prominent reduction     |
/// | 25%    | 0.0625    | -24.1   | Very quiet              |
/// | 0%     | 0.0       | -60.0   | Silent                  |
///
/// Without the power curve, 50%→-6dB and 200%→+6dB, which are barely
/// noticeable changes to the human ear.
fn perceptual_to_decibels(linear: f32) -> Decibels {
    if linear <= 0.001 {
        Decibels::SILENCE
    } else {
        // Square the amplitude for a more responsive curve
        let amplitude_squared: f32 = linear * linear;
        Decibels(20.0 * amplitude_squared.log10())
    }
}

/// Convert our pan (-1.0 left, 0.0 center, 1.0 right)
/// to kira Panning (0.0 left, 0.5 center, 1.0 right).
fn pan_to_kira_panning(pan: f32) -> Panning {
    Panning((pan + 1.0) / 2.0)
}

impl KiraMixerService {
    /// Create a new mixer service.
    ///
    /// Initializes kira's `AudioManager` with default settings (cpal backend)
    /// and creates a master sub-track for volume control.
    pub fn new() -> Result<Self, MixerError> {
        let mut manager: AudioManager<DefaultBackend> =
            AudioManager::new(AudioManagerSettings::default())
                .map_err(|e| MixerError::InitFailed(format!("{e}")))?;

        // Create master sub-track — all clip sounds route through here.
        let master_track: TrackHandle = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|e| MixerError::InitFailed(e.to_string()))?;

        tracing::info!("KiraMixerService initialized (kira multi-track mixer)");

        Ok(Self {
            manager,
            master_track,
            clips: HashMap::with_capacity(16),
            master_volume: 1.0,
            master_muted: false,
            waveform_cache: WaveformCache::default(),
        })
    }

    // ── Clip Registration ──

    /// Register a clip's audio from a file path.
    ///
    /// Loads the entire file into memory as `StaticSoundData`
    /// (backed by `Arc<[Frame]>` — cloning is zero-copy).
    /// The clip is NOT played until `play_clip_at_offset` is called.
    pub fn register_clip_audio(&mut self, asset_uuid: Uuid, path: &Path) -> Result<(), MixerError> {
        if self.clips.contains_key(&asset_uuid) {
            tracing::debug!("Clip {} already registered, skipping", asset_uuid);
            return Ok(());
        }

        let sound_data: StaticSoundData = StaticSoundData::from_file(path)
            .map_err(|e| MixerError::LoadFailed(format!("{}: {}", path.display(), e)))?;

        tracing::info!(
            "Registered clip audio: {} ({:.2}s)",
            asset_uuid,
            sound_data.duration().as_secs_f64()
        );

        // Pre-compute waveform peaks from raw frame data (one-time O(n) pass).
        // Uses direct Arc<[Frame]> slice access — no per-call overhead.
        let peaks = compute_peaks_from_frames(
            &sound_data.frames,
            sound_data.sample_rate,
            DEFAULT_PEAK_COUNT,
        );
        self.waveform_cache.insert(asset_uuid, peaks);

        self.clips
            .insert(asset_uuid, ClipAudioEntry::new(sound_data));
        Ok(())
    }

    /// Unregister a clip's audio (stops playback and frees resources).
    pub fn unregister_clip_audio(&mut self, asset_uuid: &Uuid) {
        if let Some(mut entry) = self.clips.remove(asset_uuid) {
            entry.stop();
            self.waveform_cache.remove(asset_uuid);
            tracing::info!("Unregistered clip audio: {}", asset_uuid);
        }
    }

    /// Get a reference to the waveform peak cache.
    ///
    /// Used by UI renderer to draw waveforms on timeline clips.
    /// O(1) lookup per visible clip — no allocations in render path.
    pub fn waveform_cache(&self) -> &WaveformCache {
        &self.waveform_cache
    }

    /// Check if a clip is registered.
    pub fn has_clip(&self, asset_uuid: &Uuid) -> bool {
        self.clips.contains_key(asset_uuid)
    }

    /// Measure integrated loudness (LUFS) of a registered clip's audio.
    ///
    /// Uses EBU R128 / ITU-R BS.1770-4 standard via the `ebur128` crate.
    /// Reads directly from cached `StaticSoundData` frames — no disk I/O.
    ///
    /// ## Performance
    /// - One-shot call (NOT per-frame) — only invoked on explicit user action
    /// - O(n) single pass over frames via ebur128's internal block processing
    /// - Zero impact on playback hot path
    ///
    /// Returns `None` if the clip is not registered.
    pub fn measure_clip_loudness(&self, asset_uuid: &Uuid) -> Option<f64> {
        let entry: &ClipAudioEntry = self.clips.get(asset_uuid)?;
        let lufs: f64 = crate::loudness::measure_integrated_lufs(
            &entry.sound_data.frames,
            entry.sound_data.sample_rate,
        );
        Some(lufs)
    }

    // ── Playback Control ──

    /// Start or re-seek a clip's playback at the given source-time offset.
    ///
    /// If the clip already has an active handle, this seeks it.
    /// If not, this plays a new instance on the master track with
    /// `start_position` set to the source offset.
    ///
    /// `offset_secs` is the clip's source time (NOT timeline time).
    /// The caller (audio_mixer_sync_system) converts timeline→source time
    /// using `Clip::timeline_to_source()`.
    pub fn play_clip_at_offset(
        &mut self,
        asset_uuid: &Uuid,
        offset_secs: f64,
    ) -> Result<(), MixerError> {
        let entry: &mut ClipAudioEntry = self
            .clips
            .get_mut(asset_uuid)
            .ok_or(MixerError::ClipNotFound(*asset_uuid))?;

        if entry.is_active() {
            // Handle exists (playing or paused) — resume + seek if needed
            if let Some(ref mut handle) = entry.handle {
                // CRITICAL: Always resume in case the handle was paused by pause_all().
                // Without this, paused handles stay silent forever because is_active()
                // returns true (handle exists) but audio never starts playing again.
                handle.resume(Tween::default());

                let current_pos: f64 = handle.position();
                let drift: f64 = (current_pos - offset_secs).abs();

                // Only seek if drift exceeds 100ms (avoids jitter)
                if drift > 0.1 {
                    handle.seek_to(offset_secs);
                    tracing::debug!(
                        "Seek clip {}: {:.2}s → {:.2}s (drift={:.0}ms)",
                        asset_uuid,
                        current_pos,
                        offset_secs,
                        drift * 1000.0
                    );
                }
            }
        } else {
            // Not playing — start on master track with start_position.
            // Routing: track.play() routes sound output through that track.
            let sound_data: StaticSoundData = entry
                .sound_data
                .start_position(PlaybackPosition::Seconds(offset_secs));

            let handle = self
                .master_track
                .play(sound_data)
                .map_err(|e: kira::PlaySoundError<()>| MixerError::PlaybackError(e.to_string()))?;

            entry.handle = Some(handle);
            tracing::debug!("Started clip {} at {:.2}s", asset_uuid, offset_secs);
        }

        Ok(())
    }

    /// Stop a clip that is no longer at the playhead.
    pub fn stop_clip(&mut self, asset_uuid: &Uuid) {
        if let Some(entry) = self.clips.get_mut(asset_uuid) {
            if entry.is_active() {
                entry.stop();
                tracing::debug!("Stopped clip {}", asset_uuid);
            }
        }
    }

    /// Stop all currently playing clips.
    pub fn stop_all_clips(&mut self) {
        for entry in self.clips.values_mut() {
            entry.stop();
        }
    }

    /// Clean up stopped handles across all clips.
    ///
    /// Called each frame from `audio_mixer_sync_system`. Drops handles
    /// only after kira's audio thread has confirmed `Stopped` state.
    pub fn cleanup_stopped_handles(&mut self) {
        for entry in self.clips.values_mut() {
            entry.cleanup_stopped();
        }
    }

    // ── Per-Clip Parameters ──

    /// Update a clip's volume, panning, and mute state.
    ///
    /// Called each frame from `audio_mixer_sync_system`.
    pub fn update_clip_params(
        &mut self,
        asset_uuid: &Uuid,
        volume_linear: f32,
        pan: f32,
        muted: bool,
    ) {
        if let Some(entry) = self.clips.get_mut(asset_uuid) {
            if let Some(ref mut handle) = entry.handle {
                // Use -55dB for mute instead of SILENCE (-60dB).
                // Kira treats -60dB as the silence threshold and may
                // internally stop the sound, preventing unmute recovery.
                // -55dB is inaudible but keeps the handle alive.
                let db: Decibels = if muted {
                    Decibels(-55.0)
                } else {
                    perceptual_to_decibels(volume_linear)
                };
                handle.set_volume(db, Tween::default());
                handle.set_panning(pan_to_kira_panning(pan), Tween::default());
            }
        }
    }

    // ── Master Controls ──

    /// Set the master volume (linear 0.0–2.0, where 1.0 = unity gain).
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 2.0);
        self.apply_master_volume();
    }

    /// Set master mute state.
    pub fn set_master_muted(&mut self, muted: bool) {
        self.master_muted = muted;
        self.apply_master_volume();
    }

    /// Get current master volume.
    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    fn apply_master_volume(&mut self) {
        let db: Decibels = if self.master_muted {
            Decibels(-55.0) // Keep above SILENCE threshold (-60dB)
        } else {
            perceptual_to_decibels(self.master_volume)
        };
        self.master_track.set_volume(db, Tween::default());
    }

    // ── Transport ──

    /// Pause all active clip playback.
    pub fn pause_all(&mut self) {
        for entry in self.clips.values_mut() {
            if let Some(ref mut handle) = entry.handle {
                handle.pause(Tween::default());
            }
        }
    }

    /// Resume all paused clips.
    pub fn resume_all(&mut self) {
        for entry in self.clips.values_mut() {
            if let Some(ref mut handle) = entry.handle {
                handle.resume(Tween::default());
            }
        }
    }

    /// Set playback speed for all active clips.
    pub fn set_playback_speed(&mut self, speed: f64) {
        for entry in self.clips.values_mut() {
            if let Some(ref mut handle) = entry.handle {
                handle.set_playback_rate(speed, Tween::default());
            }
        }
    }

    /// Set playback speed for a specific active clip.
    ///
    /// Used by `audio_mixer_sync_system` to apply per-clip speed
    /// (from ClipMetadata.speed * playhead.playback_speed).
    pub fn set_clip_playback_rate(&mut self, asset_uuid: &Uuid, speed: f64) {
        if let Some(entry) = self.clips.get_mut(asset_uuid) {
            if let Some(ref mut handle) = entry.handle {
                handle.set_playback_rate(speed, Tween::default());
            }
        }
    }

    /// Get the number of registered clips.
    pub fn registered_clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Get the number of currently active (playing) clips.
    pub fn active_clip_count(&self) -> usize {
        self.clips.values().filter(|e| e.is_active()).count()
    }

    /// Get all registered clip UUIDs.
    ///
    /// Used by `audio_mixer_sync_system` to find clips that left the playhead
    /// and need to be stopped.
    pub fn registered_uuids(&self) -> Vec<Uuid> {
        self.clips.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_to_decibels() {
        let db: Decibels = linear_to_decibels(1.0);
        assert!((db.0 - 0.0).abs() < 0.001, "1.0 linear = 0 dB");

        let db: Decibels = linear_to_decibels(0.5);
        assert!((db.0 - (-6.02)).abs() < 0.1, "0.5 linear ≈ -6 dB");

        let db: Decibels = linear_to_decibels(0.0);
        assert_eq!(db.0, -80.0, "0.0 linear = -80 dB (silence)");
    }

    #[test]
    fn test_pan_conversion() {
        let p: Panning = pan_to_kira_panning(-1.0);
        assert!((p.0 - 0.0).abs() < 0.001, "Our -1.0 = kira 0.0 (left)");

        let p: Panning = pan_to_kira_panning(0.0);
        assert!((p.0 - 0.5).abs() < 0.001, "Our 0.0 = kira 0.5 (center)");

        let p: Panning = pan_to_kira_panning(1.0);
        assert!((p.0 - 1.0).abs() < 0.001, "Our 1.0 = kira 1.0 (right)");
    }
}
