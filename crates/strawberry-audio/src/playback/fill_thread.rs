//! # Audio Fill Thread
//!
//! Background thread that feeds the lock-free ring buffer with
//! processed audio samples. This is the "producer" side of the
//! Premiere-style architecture.
//!
//! ## Architecture
//!
//! - Fill thread owns the `ringbuf::HeapProd` (producer)
//! - cpal callback owns the `ringbuf::HeapCons` (consumer)
//! - Zero mutexes between them — fully lock-free
//! - Fill thread does ALL heavy work (rubato resampling)
//! - cpal callback is trivial (just drains ring buffer)
//!
//! ## Thread Lifecycle
//!
//! Spawned by `init_audio_stream()`, stopped via `stop_fill` atomic.
//! The thread sleeps when not playing or when the ring is full.

use crate::resampler::{ResamplerConfig, SpeedResampler, RUBATO_CHUNK_SIZE};
use parking_lot::RwLock;
use ringbuf::traits::*;
use ringbuf::HeapProd;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::SharedState;

/// How many frames to copy per fill iteration in bypass mode.
const BYPASS_CHUNK_FRAMES: usize = RUBATO_CHUNK_SIZE;

/// Sleep when ring buffer is full (ms).
const RING_FULL_SLEEP_MS: u64 = 1;

/// Sleep when not playing or no audio loaded (ms).
const IDLE_SLEEP_MS: u64 = 5;

/// Spawn the background audio fill thread.
///
/// Continuously produces audio samples into the ring buffer:
/// - Bypass mode (speed ≈ 1.0): direct copy from source
/// - Resample mode (speed ≠ 1.0): rubato FFT sinc interpolation
///
/// Returns the `JoinHandle` for cleanup on drop.
pub(crate) fn spawn_fill_thread(
    shared_state: Arc<SharedState>,
    audio_buffer: Arc<RwLock<Vec<f32>>>,
    mut producer: HeapProd<f32>,
    sample_rate: u32,
    channels: usize,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("audio-fill".to_string())
        .spawn(move || {
            run_fill_loop(
                &shared_state,
                &audio_buffer,
                &mut producer,
                sample_rate,
                channels,
            );
            tracing::debug!("Audio fill thread stopped");
        })
        .expect("Failed to spawn audio fill thread")
}

/// Main fill loop — runs until `stop_fill` is set.
fn run_fill_loop(
    shared_state: &SharedState,
    audio_buffer: &RwLock<Vec<f32>>,
    producer: &mut HeapProd<f32>,
    sample_rate: u32,
    channels: usize,
) {
    let config: ResamplerConfig = ResamplerConfig {
        channels,
        sample_rate,
    };
    let mut resampler: SpeedResampler = match SpeedResampler::new(config, 1.0) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Fill thread: resampler init failed: {}", e);
            return;
        }
    };

    // Pre-allocate output buffer for rubato (no allocs in the hot loop)
    let mut rubato_output: Vec<f32> = vec![0.0f32; RUBATO_CHUNK_SIZE * channels];
    let mut last_speed: f32 = 1.0;

    loop {
        // ── Stop signal ──
        if shared_state.stop_fill.load(Ordering::Relaxed) {
            break;
        }

        // ── Idle when not playing ──
        if !shared_state.is_playing.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
            continue;
        }

        // ── Handle pending seek ──
        if shared_state.needs_seek.swap(false, Ordering::SeqCst) {
            let target: u64 = shared_state.seek_target_frames.load(Ordering::SeqCst);
            resampler.seek_to(target as usize);
            // Increment generation so callback flushes stale ring samples
            shared_state.seek_generation.fetch_add(1, Ordering::SeqCst);
            shared_state.fill_position.store(target, Ordering::Relaxed);
            shared_state
                .position_frames
                .store(target, Ordering::Relaxed);
            shared_state.resync_count.fetch_add(1, Ordering::Relaxed);
            // Skip this fill iteration — give the callback time to flush
            // stale ring samples before we push fresh audio.
            continue;
        }

        // ── Handle speed change ──
        let speed: f32 = shared_state.get_speed();
        if (speed - last_speed).abs() > 0.001 {
            resampler.set_speed(speed);
            last_speed = speed;
            // Flush ring — old-speed samples would corrupt position tracking.
            // The callback uses `speed` to convert ring frames → source frames.
            // Mixing old-speed and new-speed samples in the ring gives wrong
            // position → drift detection → seek loop → silence.
            let pos: u64 = resampler.source_position() as u64;
            shared_state.seek_generation.fetch_add(1, Ordering::SeqCst);
            shared_state.fill_position.store(pos, Ordering::Relaxed);
            shared_state.position_frames.store(pos, Ordering::Relaxed);
            // Skip fill — let callback flush stale ring first
            continue;
        }

        // ── Check ring buffer space ──
        let chunk_samples: usize = RUBATO_CHUNK_SIZE * channels;
        if producer.vacant_len() < chunk_samples {
            thread::sleep(Duration::from_millis(RING_FULL_SLEEP_MS));
            continue;
        }

        // ── Read source audio ──
        let buffer = audio_buffer.read();
        if buffer.is_empty() {
            drop(buffer);
            thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
            continue;
        }

        let total_source_frames: usize = buffer.len() / channels;

        if resampler.is_bypass() {
            // ================================================
            // BYPASS (speed ≈ 1.0) — direct copy to ring
            // ================================================
            fill_bypass(
                &mut resampler,
                &buffer,
                producer,
                channels,
                total_source_frames,
            );
        } else {
            // ================================================
            // RESAMPLE (speed ≠ 1.0) — rubato → ring
            // ================================================
            fill_resample(
                &mut resampler,
                &buffer,
                producer,
                channels,
                &mut rubato_output,
            );
        }

        // Drop read lock before updating atomics
        drop(buffer);

        // Update fill position for callback's position tracking
        shared_state
            .fill_position
            .store(resampler.source_position() as u64, Ordering::Relaxed);
    }
}

/// Fill ring buffer in bypass mode (direct sample copy).
fn fill_bypass(
    resampler: &mut SpeedResampler,
    source: &[f32],
    producer: &mut HeapProd<f32>,
    channels: usize,
    total_source_frames: usize,
) {
    let pos: usize = resampler.source_position();
    let frames_available: usize = total_source_frames.saturating_sub(pos);
    let frames_to_copy: usize = BYPASS_CHUNK_FRAMES.min(frames_available);

    if frames_to_copy == 0 {
        // Source exhausted — push silence chunk
        push_silence(producer, BYPASS_CHUNK_FRAMES * channels);
        return;
    }

    let start_sample: usize = pos * channels;
    let end_sample: usize = start_sample + frames_to_copy * channels;
    let pushed: usize = producer.push_slice(&source[start_sample..end_sample]);
    let frames_pushed: usize = pushed / channels;
    resampler.set_position(pos + frames_pushed);
}

/// Fill ring buffer via rubato resampling.
fn fill_resample(
    resampler: &mut SpeedResampler,
    source: &[f32],
    producer: &mut HeapProd<f32>,
    channels: usize,
    rubato_output: &mut [f32],
) {
    let produced: usize = resampler.process_chunk(source, rubato_output);

    if produced == 0 {
        // Source exhausted
        push_silence(producer, RUBATO_CHUNK_SIZE * channels);
        return;
    }

    let samples_to_push: usize = produced * channels;
    producer.push_slice(&rubato_output[..samples_to_push]);
}

/// Push silence samples into the ring buffer.
fn push_silence(producer: &mut HeapProd<f32>, count: usize) {
    // Use a small stack buffer to avoid heap alloc
    const SILENCE_BUF_SIZE: usize = 2048;
    let silence: [f32; SILENCE_BUF_SIZE] = [0.0; SILENCE_BUF_SIZE];

    let mut remaining: usize = count;
    while remaining > 0 {
        let chunk: usize = remaining.min(SILENCE_BUF_SIZE);
        producer.push_slice(&silence[..chunk]);
        remaining -= chunk;
    }
}
