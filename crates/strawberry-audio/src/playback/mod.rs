//! # Audio Playback Service
//!
//! Production-grade audio playback with lock-free ring buffer architecture.
//!
//! ## Architecture (Adobe Premiere / CapCut Style)
//!
//! ```text
//! Main Thread          Fill Thread           Audio Thread (cpal)
//! ───────────          ───────────           ──────────────────
//! set_speed()──atom──→ rubato resample ──→   drain ring (lock-free)
//! seek()  ───atom───→ direct copy     ──→   apply volume
//! play()  ───atom───→ (HeapProd)             (HeapCons)
//! ```
//!
//! - **Video as Master Clock**: Audio follows video playhead position
//! - **Lock-free SPSC Ring Buffer**: `ringbuf` between fill thread and cpal
//! - **Zero mutexes on audio thread**: Callback only drains ring buffer
//! - **Drift Correction**: Monitor drift, resync if > threshold

mod fill_thread;

use crate::ffmpeg_audio::{AudioDecodeError, AudioMetadata, FFmpegAudioDecoder};
use parking_lot::RwLock;
use ringbuf::traits::*;
use ringbuf::HeapRb;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Audio playback errors
#[derive(Error, Debug)]
pub enum AudioError {
    #[error("Playback error: {0}")]
    Playback(String),

    #[error("Device error: {0}")]
    Device(String),

    #[error("Decode error: {0}")]
    Decode(#[from] AudioDecodeError),

    #[error("No audio loaded")]
    NoAudioLoaded,
}

/// Audio playback state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Sync statistics for monitoring
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncStats {
    pub audio_position_samples: u64,
    pub target_position_samples: u64,
    pub drift_ms: f64,
    pub resync_count: u64,
    pub device_latency_ms: f64,
}

/// Loaded audio asset info
#[derive(Debug, Clone)]
pub struct LoadedAudio {
    pub path: PathBuf,
    pub metadata: AudioMetadata,
}

// ── Constants ──

/// Ring buffer capacity in samples.
/// ~170ms at 48kHz stereo — enough for smooth playback.
const RING_CAPACITY_SAMPLES: usize = 8192 * 2;

/// Drift threshold for sync correction (50ms — imperceptible to humans).
const DRIFT_THRESHOLD_MS: f64 = 50.0;

/// Warning threshold for catastrophic desync.
const CATASTROPHIC_DRIFT_MS: f64 = 500.0;

// ── Shared State ──

/// Lock-free shared state between main thread, fill thread, and audio callback.
///
/// All fields are atomic — no mutexes needed for thread communication.
pub(crate) struct SharedState {
    /// Current playback position in frames (written by callback)
    pub(crate) position_frames: AtomicU64,
    /// Pending seek flag (main→fill thread)
    pub(crate) needs_seek: AtomicBool,
    /// Seek target in frames (main→fill thread)
    pub(crate) seek_target_frames: AtomicU64,
    /// Whether audio is playing
    pub(crate) is_playing: AtomicBool,
    /// Volume (f32 bits), main→callback
    volume: std::sync::atomic::AtomicU32,
    /// Speed (f32 bits), main→fill thread
    speed_bits: std::sync::atomic::AtomicU32,
    /// Sample rate (immutable after init)
    pub(crate) sample_rate: u32,
    /// Resync counter (diagnostics)
    pub(crate) resync_count: AtomicU64,
    /// Source position as tracked by fill thread (fill→callback)
    pub(crate) fill_position: AtomicU64,
    /// Incremented on seek — callback checks to flush stale ring samples
    pub(crate) seek_generation: AtomicU64,
    /// Stop signal for fill thread (main→fill)
    pub(crate) stop_fill: AtomicBool,
}

impl SharedState {
    fn new(sample_rate: u32) -> Self {
        Self {
            position_frames: AtomicU64::new(0),
            needs_seek: AtomicBool::new(false),
            seek_target_frames: AtomicU64::new(0),
            is_playing: AtomicBool::new(false),
            volume: std::sync::atomic::AtomicU32::new(1.0f32.to_bits()),
            speed_bits: std::sync::atomic::AtomicU32::new(1.0f32.to_bits()),
            sample_rate,
            resync_count: AtomicU64::new(0),
            fill_position: AtomicU64::new(0),
            seek_generation: AtomicU64::new(0),
            stop_fill: AtomicBool::new(false),
        }
    }

    pub(crate) fn get_volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn set_volume(&self, vol: f32) {
        self.volume
            .store(vol.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn get_speed(&self) -> f32 {
        f32::from_bits(self.speed_bits.load(Ordering::Relaxed))
    }

    fn set_speed(&self, speed: f32) {
        self.speed_bits
            .store(speed.clamp(0.25, 3.0).to_bits(), Ordering::Relaxed);
    }

    fn request_seek(&self, target_frame: u64) {
        self.seek_target_frames
            .store(target_frame, Ordering::SeqCst);
        self.needs_seek.store(true, Ordering::SeqCst);
    }
}

// ── Stream Wrapper ──

/// Wrapper to make cpal::Stream Send+Sync for ECS resource storage.
///
/// # Safety
/// Stream is only touched from main thread. Audio callback uses atomics.
#[allow(dead_code)]
struct SendSyncStream(cpal::Stream);
unsafe impl Send for SendSyncStream {}
unsafe impl Sync for SendSyncStream {}

// ── Audio Playback Service ──

/// Production-grade audio playback service (ECS-agnostic).
///
/// ## Usage
///
/// ```ignore
/// let mut service = AudioPlaybackService::new()?;
/// service.load(&audio_path)?;
/// service.play();
/// service.sync_to(video_playhead_time); // every frame
/// ```
pub struct AudioPlaybackService {
    decoder: FFmpegAudioDecoder,
    loaded_audio: Option<LoadedAudio>,
    audio_buffer: Arc<RwLock<Vec<f32>>>,
    shared_state: Arc<SharedState>,
    _stream: Option<SendSyncStream>,
    device_latency_secs: f64,
    state: PlaybackState,
    current_speed: f32,
    /// Fill thread handle — joined on drop or reload
    fill_thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioPlaybackService {
    /// Create a new audio playback service.
    pub fn new() -> Result<Self, AudioError> {
        let decoder = FFmpegAudioDecoder::new()?;
        Ok(Self {
            decoder,
            loaded_audio: None,
            audio_buffer: Arc::new(RwLock::new(Vec::new())),
            shared_state: Arc::new(SharedState::new(48000)),
            _stream: None,
            device_latency_secs: 0.0,
            state: PlaybackState::Stopped,
            current_speed: 1.0,
            fill_thread_handle: None,
        })
    }

    /// Load an audio file for playback.
    pub fn load(&mut self, path: &Path) -> Result<AudioMetadata, AudioError> {
        // Stop old fill thread + stream before creating new ones
        self.stop_fill_thread();
        if self._stream.is_some() {
            self.shared_state.is_playing.store(false, Ordering::Relaxed);
            self._stream = None;
            std::thread::sleep(Duration::from_millis(50));
        }

        let metadata = self.decoder.probe(path)?;
        tracing::info!(
            "Loading audio: {} ({}Hz, {} ch, {:.2}s)",
            path.display(),
            metadata.sample_rate,
            metadata.channels,
            metadata.duration.as_secs_f64()
        );

        let samples = self.decoder.decode_full(path)?;
        *self.audio_buffer.write() = samples.data;

        self.shared_state = Arc::new(SharedState::new(metadata.sample_rate));
        self.loaded_audio = Some(LoadedAudio {
            path: path.to_path_buf(),
            metadata: metadata.clone(),
        });

        self.init_audio_stream(metadata.sample_rate, metadata.channels)?;
        Ok(metadata)
    }

    /// Stop and join the fill thread (if running).
    fn stop_fill_thread(&mut self) {
        self.shared_state.stop_fill.store(true, Ordering::SeqCst);
        if let Some(handle) = self.fill_thread_handle.take() {
            handle.join().ok();
        }
    }

    /// Initialize cpal stream + fill thread with lock-free ring buffer.
    fn init_audio_stream(&mut self, sample_rate: u32, channels: u16) -> Result<(), AudioError> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::Device("No audio output device".into()))?;

        tracing::info!("Audio device: {}", device.name().unwrap_or_default());

        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let channel_count: usize = channels as usize;

        // ── Create lock-free ring buffer ──
        let ring: HeapRb<f32> = HeapRb::new(RING_CAPACITY_SAMPLES);
        let (producer, mut consumer) = ring.split();

        // ── Spawn background fill thread ──
        let fill_handle = fill_thread::spawn_fill_thread(
            Arc::clone(&self.shared_state),
            Arc::clone(&self.audio_buffer),
            producer,
            sample_rate,
            channel_count,
        );
        self.fill_thread_handle = Some(fill_handle);

        // ── Build cpal stream (trivial callback — just drains ring) ──
        let shared_state = Arc::clone(&self.shared_state);
        let mut local_seek_gen: u64 = 0;

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Lock-free audio callback — ONLY drains ring buffer.
                    // No rubato, no mutexes, no allocations.

                    let is_playing: bool = shared_state.is_playing.load(Ordering::Relaxed);
                    if !is_playing {
                        data.fill(0.0);
                        return;
                    }

                    // Check for seek — flush stale ring samples
                    let gen: u64 = shared_state.seek_generation.load(Ordering::SeqCst);
                    if gen != local_seek_gen {
                        while consumer.try_pop().is_some() {}
                        local_seek_gen = gen;
                    }

                    let volume: f32 = shared_state.get_volume();

                    // Drain ring → output (lock-free pop_slice)
                    let popped: usize = consumer.pop_slice(data);
                    // Apply volume to drained samples
                    for sample in data[..popped].iter_mut() {
                        *sample *= volume;
                    }
                    // Silence any underrun
                    for sample in data[popped..].iter_mut() {
                        *sample = 0.0;
                    }

                    // Update position for sync_to() drift checking
                    let fill_pos: u64 = shared_state.fill_position.load(Ordering::Relaxed);
                    let ring_samples: u64 = consumer.occupied_len() as u64;
                    let ring_frames: u64 = ring_samples / channel_count as u64;
                    let speed: f32 = shared_state.get_speed();
                    let ring_source_frames: u64 = (ring_frames as f64 * speed as f64) as u64;
                    let played_pos: u64 = fill_pos.saturating_sub(ring_source_frames);
                    shared_state
                        .position_frames
                        .store(played_pos, Ordering::Relaxed);
                },
                |err| {
                    tracing::error!("Audio stream error: {}", err);
                },
                None,
            )
            .map_err(|e| AudioError::Device(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::Playback(e.to_string()))?;

        self.device_latency_secs = 0.020;
        self._stream = Some(SendSyncStream(stream));
        Ok(())
    }

    /// Start playback.
    pub fn play(&mut self) {
        if self.loaded_audio.is_some() {
            self.shared_state.is_playing.store(true, Ordering::Relaxed);
            self.state = PlaybackState::Playing;
        }
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.shared_state.is_playing.store(false, Ordering::Relaxed);
        self.state = PlaybackState::Paused;
    }

    /// Resume from paused state without seeking.
    pub fn resume(&mut self) {
        if self.state == PlaybackState::Paused {
            self.shared_state.is_playing.store(true, Ordering::Relaxed);
            self.state = PlaybackState::Playing;
        } else if self.state == PlaybackState::Stopped {
            self.play();
        }
    }

    /// Stop playback and reset to beginning.
    pub fn stop(&mut self) {
        self.shared_state.is_playing.store(false, Ordering::Relaxed);
        self.shared_state.request_seek(0);
        self.state = PlaybackState::Stopped;
    }

    /// Sync audio to video playhead position (drift correction).
    ///
    /// Called every frame. Compares audio callback position with video
    /// playhead and triggers atomic seek if drift exceeds threshold.
    pub fn sync_to(&mut self, video_time: Duration) {
        if self.loaded_audio.is_none() || self.state != PlaybackState::Playing {
            return;
        }

        let sample_rate: f64 = self.shared_state.sample_rate as f64;
        let audio_pos: u64 = self.shared_state.position_frames.load(Ordering::Relaxed);
        let audio_time: f64 = audio_pos as f64 / sample_rate;
        let video_time: f64 = video_time.as_secs_f64();
        let drift_ms: f64 = (audio_time - video_time).abs() * 1000.0;

        if drift_ms > CATASTROPHIC_DRIFT_MS {
            tracing::warn!(
                "A/V drift: {:.1}ms (audio={:.2}s, video={:.2}s) — resyncing",
                drift_ms,
                audio_time,
                video_time
            );
            self.seek_to(Duration::from_secs_f64(video_time));
        } else if drift_ms > DRIFT_THRESHOLD_MS {
            let target: u64 = (video_time * sample_rate) as u64;
            self.shared_state.request_seek(target);
        }
    }

    /// Set playback speed (0.1 - 10.0).
    pub fn set_speed(&mut self, speed: f32) {
        let clamped: f32 = speed.clamp(0.25, 3.0);
        self.shared_state.set_speed(clamped);
        self.current_speed = clamped;
    }

    /// Get current playback speed.
    pub fn speed(&self) -> f32 {
        self.current_speed
    }

    /// Seek to a specific time.
    pub fn seek_to(&mut self, time: Duration) {
        if self.loaded_audio.is_none() {
            return;
        }
        let sample_rate = self.shared_state.sample_rate;
        let frame = (time.as_secs_f64() * sample_rate as f64) as u64;
        self.shared_state.request_seek(frame);
    }

    /// Set master volume (0.0 - 1.0).
    pub fn set_volume(&mut self, volume: f32) {
        self.shared_state.set_volume(volume);
    }

    /// Get current volume.
    pub fn volume(&self) -> f32 {
        self.shared_state.get_volume()
    }

    /// Get playback state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Get sync statistics for debugging.
    pub fn sync_stats(&self) -> SyncStats {
        let audio_pos = self.shared_state.position_frames.load(Ordering::Relaxed);
        let target_pos = self.shared_state.seek_target_frames.load(Ordering::Relaxed);
        let sample_rate = self.shared_state.sample_rate as f64;
        let drift_frames = audio_pos as i64 - target_pos as i64;
        let drift_ms = (drift_frames as f64 / sample_rate) * 1000.0;

        SyncStats {
            audio_position_samples: audio_pos,
            target_position_samples: target_pos,
            drift_ms,
            resync_count: self.shared_state.resync_count.load(Ordering::Relaxed),
            device_latency_ms: self.device_latency_secs * 1000.0,
        }
    }

    /// Get loaded audio info.
    pub fn loaded_audio(&self) -> Option<&LoadedAudio> {
        self.loaded_audio.as_ref()
    }
}

impl Drop for AudioPlaybackService {
    fn drop(&mut self) {
        self.stop_fill_thread();
    }
}

impl Default for AudioPlaybackService {
    fn default() -> Self {
        Self::new().expect("Failed to initialize audio")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_service_creation() {
        let service = AudioPlaybackService::new();
        if let Ok(svc) = service {
            assert_eq!(svc.state(), PlaybackState::Stopped);
        }
    }

    #[test]
    fn test_volume_clamping() {
        let state = SharedState::new(48000);
        state.set_volume(0.5);
        assert!((state.get_volume() - 0.5).abs() < 0.001);
        state.set_volume(1.5);
        assert!((state.get_volume() - 1.0).abs() < 0.001);
        state.set_volume(-0.5);
        assert!((state.get_volume() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_sync_stats_default() {
        let stats = SyncStats::default();
        assert_eq!(stats.drift_ms, 0.0);
        assert_eq!(stats.resync_count, 0);
    }
}
