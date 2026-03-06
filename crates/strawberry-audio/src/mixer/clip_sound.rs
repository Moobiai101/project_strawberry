//! # Clip Audio Entry
//!
//! Per-clip audio state for the mixer. Holds the decoded audio data
//! (kira `StaticSoundData`) and the active playback handle.

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::PlaybackState;

/// Per-clip audio state tracked by `KiraMixerService`.
///
/// Each imported clip with audio gets one `ClipAudioEntry`.
/// The `sound_data` is loaded once on import (zero-copy clone via Arc).
/// The `handle` is created each time the clip starts playing and
/// dropped when it stops.
#[derive(Debug)]
pub struct ClipAudioEntry {
    /// Decoded audio data — loaded once, cloned cheaply via Arc.
    ///
    /// kira's `StaticSoundData` stores frames as `Arc<[Frame]>`,
    /// so cloning is O(1) with no extra memory.
    pub sound_data: StaticSoundData,

    /// Active playback handle, if this clip is currently playing.
    ///
    /// `None` = clip is not at the playhead (silent).
    /// `Some(handle)` = clip is actively playing.
    pub handle: Option<StaticSoundHandle>,
}

impl ClipAudioEntry {
    /// Create a new entry from decoded sound data (no playback yet).
    pub fn new(sound_data: StaticSoundData) -> Self {
        Self {
            sound_data,
            handle: None,
        }
    }

    /// Check if this clip is currently playing.
    pub fn is_playing(&self) -> bool {
        self.handle
            .as_ref()
            .map(|h| h.state() == PlaybackState::Playing)
            .unwrap_or(false)
    }

    /// Check if this clip has an active handle (playing or paused).
    pub fn is_active(&self) -> bool {
        self.handle.is_some()
    }

    /// Drop the handle, stopping playback immediately.
    pub fn stop(&mut self) {
        self.handle = None;
    }
}
