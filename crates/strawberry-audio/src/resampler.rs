//! # Speed-Aware Audio Resampler
//!
//! Wraps rubato's `FftFixedOut` for high-quality speed-dependent
//! resampling with SIMD-optimized FFT-based sinc interpolation.
//!
//! This module is intentionally simple — it only wraps rubato.
//! The ring buffer decoupling is handled by the fill thread
//! in `playback/fill_thread.rs`.

use rubato::{FftFixedOut, Resampler};

/// Internal chunk size for rubato processing.
/// rubato produces exactly this many output frames per `process_chunk()`.
pub const RUBATO_CHUNK_SIZE: usize = 512;

/// Configuration for the audio resampler.
#[derive(Debug, Clone)]
pub struct ResamplerConfig {
    /// Number of audio channels (typically 2 for stereo)
    pub channels: usize,
    /// Output sample rate (matches cpal device rate)
    pub sample_rate: u32,
}

impl Default for ResamplerConfig {
    fn default() -> Self {
        Self {
            channels: 2,
            sample_rate: 48000,
        }
    }
}

/// Speed-aware audio resampler using rubato FFT-based sinc interpolation.
///
/// ## Usage
///
/// ```ignore
/// let mut resampler = SpeedResampler::new(config, 1.0)?;
/// resampler.set_speed(2.5); // 2.5x speed
/// let frames = resampler.process_chunk(&source, &mut output);
/// ```
pub struct SpeedResampler {
    /// rubato FFT-based resampler (fixed output size)
    resampler: FftFixedOut<f32>,
    /// Number of channels
    channels: usize,
    /// Pre-allocated per-channel input buffer for rubato
    input_buffer: Vec<Vec<f32>>,
    /// Pre-allocated per-channel output buffer for rubato
    output_buffer: Vec<Vec<f32>>,
    /// Current speed ratio
    current_speed: f32,
    /// Sample rate
    sample_rate: u32,
    /// Current read position in source buffer (in frames).
    source_position: usize,
}

impl SpeedResampler {
    /// Create a new speed resampler.
    ///
    /// Pre-allocates all buffers for real-time safety.
    pub fn new(config: ResamplerConfig, initial_speed: f32) -> Result<Self, String> {
        let speed: f64 = initial_speed.max(0.25) as f64;
        let resample_ratio: f64 = 1.0 / speed;
        let target_rate: usize = ((config.sample_rate as f64 * resample_ratio) as usize).max(1);

        let resampler: FftFixedOut<f32> = FftFixedOut::new(
            config.sample_rate as usize,
            target_rate,
            RUBATO_CHUNK_SIZE,
            2, // sub-chunks (quality vs latency tradeoff)
            config.channels,
        )
        .map_err(|e| format!("Failed to create resampler: {}", e))?;

        let input_frames_needed: usize = resampler.input_frames_next();
        let input_buffer: Vec<Vec<f32>> = vec![vec![0.0f32; input_frames_needed]; config.channels];
        let output_buffer: Vec<Vec<f32>> = vec![vec![0.0f32; RUBATO_CHUNK_SIZE]; config.channels];

        Ok(Self {
            resampler,
            channels: config.channels,
            input_buffer,
            output_buffer,
            current_speed: initial_speed,
            sample_rate: config.sample_rate,
            source_position: 0,
        })
    }

    /// Update the playback speed, recreating the rubato resampler.
    ///
    /// Returns `true` if speed actually changed.
    pub fn set_speed(&mut self, new_speed: f32) -> bool {
        if (new_speed - self.current_speed).abs() < 0.001 {
            return false;
        }

        let speed: f64 = new_speed.max(0.25) as f64;
        let resample_ratio: f64 = 1.0 / speed;
        let target_rate: usize = ((self.sample_rate as f64 * resample_ratio) as usize).max(1);

        match FftFixedOut::new(
            self.sample_rate as usize,
            target_rate,
            RUBATO_CHUNK_SIZE,
            2,
            self.channels,
        ) {
            Ok(new_resampler) => {
                let input_needed: usize = new_resampler.input_frames_next();
                self.resampler = new_resampler;
                self.current_speed = new_speed;

                // Resize input buffers if new ratio needs more
                for ch in self.input_buffer.iter_mut() {
                    if ch.len() < input_needed {
                        ch.resize(input_needed, 0.0);
                    }
                }
                true
            }
            Err(e) => {
                tracing::warn!("Failed to update resampler speed to {}: {}", new_speed, e);
                false
            }
        }
    }

    /// Seek to a source frame position (full reset).
    ///
    /// Resets rubato internal filters to prevent stale audio artifacts.
    pub fn seek_to(&mut self, frame: usize) {
        self.source_position = frame;
        self.resampler.reset();
    }

    /// Update source position without resetting rubato state.
    ///
    /// Used by bypass mode for lightweight position advancement.
    pub fn set_position(&mut self, frame: usize) {
        self.source_position = frame;
    }

    /// Process one rubato chunk from source buffer.
    ///
    /// Reads `input_frames_next()` frames from source at `source_position`,
    /// runs rubato, writes interleaved output to `output`.
    /// Advances `source_position` by consumed frames.
    ///
    /// Returns number of output frames produced, or 0 if source exhausted.
    pub fn process_chunk(&mut self, source_buffer: &[f32], output: &mut [f32]) -> usize {
        let total_source_frames: usize = source_buffer.len() / self.channels;
        let input_needed: usize = self.resampler.input_frames_next();

        if self.source_position + input_needed > total_source_frames {
            return 0;
        }

        // De-interleave source into per-channel input buffers
        for ch in 0..self.channels {
            let ch_buf: &mut Vec<f32> = &mut self.input_buffer[ch];
            if ch_buf.len() < input_needed {
                ch_buf.resize(input_needed, 0.0);
            }
            for i in 0..input_needed {
                let src_idx: usize = (self.source_position + i) * self.channels + ch;
                ch_buf[i] = if src_idx < source_buffer.len() {
                    source_buffer[src_idx]
                } else {
                    0.0
                };
            }
        }

        // Ensure output buffers are sized
        for ch in self.output_buffer.iter_mut() {
            if ch.len() < RUBATO_CHUNK_SIZE {
                ch.resize(RUBATO_CHUNK_SIZE, 0.0);
            }
        }

        // Process through rubato (alloc-free after construction)
        match self
            .resampler
            .process_into_buffer(&self.input_buffer, &mut self.output_buffer, None)
        {
            Ok((_consumed, produced)) => {
                // Re-interleave into output
                let frames: usize = produced.min(output.len() / self.channels);
                for i in 0..frames {
                    for ch in 0..self.channels {
                        output[i * self.channels + ch] = self.output_buffer[ch][i];
                    }
                }
                self.source_position += input_needed;
                frames
            }
            Err(e) => {
                tracing::warn!("Resample error: {}", e);
                0
            }
        }
    }

    /// Get the current source position (in frames).
    pub fn source_position(&self) -> usize {
        self.source_position
    }

    /// Check if speed is effectively 1.0 (bypass mode).
    pub fn is_bypass(&self) -> bool {
        (self.current_speed - 1.0).abs() < 0.02
    }

    /// Get current speed.
    pub fn current_speed(&self) -> f32 {
        self.current_speed
    }

    /// Get the number of channels.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_creation_at_normal_speed() {
        let config: ResamplerConfig = ResamplerConfig {
            channels: 2,
            sample_rate: 48000,
        };
        let resampler = SpeedResampler::new(config, 1.0);
        assert!(resampler.is_ok());
        assert!(resampler.unwrap().is_bypass());
    }

    #[test]
    fn test_resampler_creation_at_double_speed() {
        let config: ResamplerConfig = ResamplerConfig {
            channels: 2,
            sample_rate: 48000,
        };
        let resampler = SpeedResampler::new(config, 2.0);
        assert!(resampler.is_ok());
        assert!(!resampler.unwrap().is_bypass());
    }

    #[test]
    fn test_speed_change() {
        let config: ResamplerConfig = ResamplerConfig {
            channels: 2,
            sample_rate: 48000,
        };
        let mut resampler: SpeedResampler = SpeedResampler::new(config, 1.0).unwrap();
        assert!(resampler.is_bypass());

        let changed: bool = resampler.set_speed(2.0);
        assert!(changed);
        assert!(!resampler.is_bypass());
    }

    #[test]
    fn test_seek_resets_position() {
        let config: ResamplerConfig = ResamplerConfig {
            channels: 2,
            sample_rate: 48000,
        };
        let mut resampler: SpeedResampler = SpeedResampler::new(config, 1.0).unwrap();

        resampler.seek_to(1000);
        assert_eq!(resampler.source_position(), 1000);
    }

    #[test]
    fn test_process_chunk_with_empty_source() {
        let config: ResamplerConfig = ResamplerConfig {
            channels: 2,
            sample_rate: 48000,
        };
        let mut resampler: SpeedResampler = SpeedResampler::new(config, 2.0).unwrap();

        let source: Vec<f32> = vec![];
        let mut output: Vec<f32> = vec![0.0; 1024];
        let produced: usize = resampler.process_chunk(&source, &mut output);
        assert_eq!(produced, 0);
    }
}
