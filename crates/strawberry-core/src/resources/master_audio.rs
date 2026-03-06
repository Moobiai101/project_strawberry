//! # Master Audio State Resource
//!
//! Global audio mixer state — master volume, mute, and solo.

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};

/// Global master audio state for the mixer.
///
/// Controls master volume and mute independently of per-clip settings.
/// Updated via the Audio Mixer panel (UI) and read by `audio_mixer_sync_system`.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct MasterAudioState {
    /// Master volume (linear 0.0–1.0, where 1.0 = unity gain = 0 dB)
    pub master_volume: f32,
    /// Whether the master bus is muted
    pub master_muted: bool,
}

impl Default for MasterAudioState {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            master_muted: false,
        }
    }
}
