//! # Clip Audio Entry
//!
//! Per-clip audio state for the mixer. Holds the decoded audio data
//! (kira `StaticSoundData`) and the active playback handle.

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::PlaybackState;
use kira::Tween;

/// Per-clip audio state tracked by `KiraMixerService`.
///
/// Each imported clip with audio gets one `ClipAudioEntry`.
/// The `sound_data` is loaded once on import (zero-copy clone via Arc).
/// The `handle` is created each time the clip starts playing and
/// moved to `stopping_handles` when stopped (NOT dropped immediately).
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

    /// Handles that have been told to stop but not yet confirmed by kira.
    ///
    /// CRITICAL: In kira, dropping a `StaticSoundHandle` before the audio
    /// thread processes the stop command orphans the sound — it plays
    /// forever. We keep handles here until `state() == Stopped`.
    stopping_handles: Vec<StaticSoundHandle>,
}

impl ClipAudioEntry {
    /// Create a new entry from decoded sound data (no playback yet).
    pub fn new(sound_data: StaticSoundData) -> Self {
        Self {
            sound_data,
            handle: None,
            stopping_handles: Vec::new(),
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

    /// Stop playback: tell kira to stop, then park the handle.
    ///
    /// The handle is NOT dropped — it's moved to `stopping_handles`
    /// and cleaned up by `cleanup_stopped()` once kira confirms the
    /// sound has actually stopped in its audio thread.
    pub fn stop(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop(Tween::default());
            self.stopping_handles.push(handle);
        }
    }

    /// Clean up handles that kira has confirmed as stopped.
    ///
    /// Called each frame. Only drops handles whose `state() == Stopped`.
    /// This prevents orphaning sounds in kira's audio thread.
    pub fn cleanup_stopped(&mut self) {
        self.stopping_handles.retain(|handle| {
            let state = handle.state();
            // Keep handles that haven't finished stopping yet
            state != PlaybackState::Stopped
        });
    }
}
