//! # Viewport & Audio Systems
//!
//! ## viewport_update_system (Legacy, unused)
//! Updates viewport state for a single VideoService — kept for reference.
//!
//! ## audio_mixer_sync_system
//! Multi-track audio sync using kira mixer. Reads active layers from
//! `ActiveLayersResource` (populated by compositor_system) for zero-cost
//! layer access — no redundant node-tree traversal.

use bevy_ecs::prelude::*;
use strawberry_core::resources::{Playhead, ViewportState};
use strawberry_video::service::{FrameStatus, VideoService};

use crate::texture_manager::TextureManager;
use std::ops::Deref;

/// Wrapper for VideoService to allow ECS resource storage
#[derive(Resource)]
pub struct VideoServiceResource(pub VideoService);

impl Deref for VideoServiceResource {
    type Target = VideoService;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// System to update viewport frame (legacy single-video path)
///
/// Uses two decoding strategies:
/// - **Playback mode**: PipeDecoder streams frames continuously at 30fps+
/// - **Random access mode**: Subprocess decoder for scrubbing/seeking
pub fn viewport_update_system(
    playhead: Res<Playhead>,
    mut viewport: ResMut<ViewportState>,
    mut video_service: ResMut<VideoServiceResource>,
    texture_manager: Res<TextureManager>,
) {
    video_service.0.poll_results();

    if video_service.current_video().is_none() {
        if viewport.loaded_asset.is_some() {
            viewport.reset();
        }
        return;
    }

    let is_playing = playhead.is_playing;
    let is_in_playback_mode = video_service.0.is_playback_mode();

    if is_playing && !is_in_playback_mode {
        if let Err(e) = video_service.0.start_playback(playhead.current_time) {
            tracing::error!("Failed to start playback mode: {}", e);
        }
    } else if !is_playing && is_in_playback_mode {
        video_service.0.stop_playback();
    }

    if is_playing && video_service.0.is_playback_mode() {
        let target_time = playhead.current_time;

        if let Some(frame) = video_service.0.request_playback_frame_for_time(target_time) {
            let key = strawberry_video::cache::FrameKey::new(
                video_service.current_video().unwrap().path.clone(),
                target_time,
                frame.width,
                frame.height,
            );

            texture_manager.queue_upload(key.clone(), (*frame).clone());

            if let Some(texture_id) = texture_manager.get_current_texture() {
                viewport.set_frame(playhead.current_frame, texture_id);
                viewport.source_resolution = glam::UVec2::new(frame.width, frame.height);
            }
        }
    } else {
        let timestamp = playhead.current_time;
        match video_service.request_frame(timestamp) {
            FrameStatus::Ready(frame) => {
                let key = strawberry_video::cache::FrameKey::new(
                    video_service.current_video().unwrap().path.clone(),
                    timestamp,
                    frame.width,
                    frame.height,
                );

                texture_manager.queue_upload(key.clone(), (*frame).clone());

                if let Some(texture_id) = texture_manager.get_texture(&key) {
                    viewport.set_frame(playhead.current_frame, texture_id);
                    viewport.source_resolution = glam::UVec2::new(frame.width, frame.height);
                }
            }
            FrameStatus::Loading => {
                if viewport.current_texture.is_none() {
                    viewport.set_loading(true);
                }
            }
            FrameStatus::Error(e) => {
                viewport.set_error(e);
            }
        }

        video_service.buffer_around(timestamp);
    }
}

// ============================================================================
// Multi-Track Audio Mixer Integration (kira-based)
// ============================================================================

use std::collections::HashSet;
use strawberry_audio::KiraMixerService;
use strawberry_core::components::{AudioClip, StableId};
use strawberry_core::resources::MasterAudioState;
use uuid::Uuid;

use crate::traverser::ActiveLayersResource;

/// ECS resource wrapping [`KiraMixerService`] for multi-track audio mixing.
///
/// Replaces the old single-stream `AudioServiceResource`.
/// Initialize in main.rs: `world.insert_resource(AudioMixerResource(mixer))`.
#[derive(Resource)]
pub struct AudioMixerResource(pub KiraMixerService);

impl Deref for AudioMixerResource {
    type Target = KiraMixerService;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AudioMixerResource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Multi-track audio mixer sync system.
///
/// Reads active layers from `ActiveLayersResource` (written by compositor_system)
/// instead of calling `collect_layers()` redundantly. This eliminates a full
/// node-tree traversal per frame — a critical optimization.
///
/// ## Performance
///
/// - Reads `ActiveLayersResource` (O(1) — just a Vec reference)
/// - Only updates kira handles when playback state requires it
/// - No I/O or heavy computation in the hot path
pub fn audio_mixer_sync_system(
    playhead: Res<Playhead>,
    active_layers: Res<ActiveLayersResource>,
    master_audio: Res<MasterAudioState>,
    mut mixer_resource: ResMut<AudioMixerResource>,
    audio_clips: Query<(&StableId, &AudioClip)>,
) {
    let mixer: &mut KiraMixerService = &mut mixer_resource.0;

    // Apply master volume + mute
    mixer.set_master_volume(master_audio.master_volume);
    mixer.set_master_muted(master_audio.master_muted);

    // Use layers already computed by compositor_system (no redundant traversal)
    let mut active_asset_uuids: HashSet<Uuid> = HashSet::new();

    for layer in &active_layers.layers {
        let asset_uuid: Uuid = layer.asset_uuid;

        if !mixer.has_clip(&asset_uuid) {
            continue;
        }

        active_asset_uuids.insert(asset_uuid);

        if playhead.is_playing {
            let source_time_secs: f64 = layer.source_time.as_secs_f64();
            if let Err(e) = mixer.play_clip_at_offset(&asset_uuid, source_time_secs) {
                tracing::warn!("Audio mixer: failed to play clip {}: {}", asset_uuid, e);
            }

            let effective_speed: f64 = layer.clip_speed as f64 * playhead.playback_speed as f64;
            mixer.set_clip_playback_rate(&asset_uuid, effective_speed);
        }

        let (volume, pan, muted) = find_audio_clip_params(&audio_clips, layer.clip_uuid);
        mixer.update_clip_params(&asset_uuid, volume, pan, muted);
    }

    // Stop clips that left the playhead
    let registered: Vec<Uuid> = mixer.registered_uuids();
    for uuid in registered {
        if !active_asset_uuids.contains(&uuid) {
            mixer.stop_clip(&uuid);
        }
    }

    if !playhead.is_playing {
        mixer.pause_all();
    }

    // GC handles that kira has confirmed as stopped.
    // This must run every frame to prevent orphaned sounds.
    mixer.cleanup_stopped_handles();
}

/// Find AudioClip volume/pan/mute for a clip by its stable UUID.
///
/// Returns (volume, pan, muted). Defaults to (1.0, 0.0, false) if
/// no AudioClip component is attached to the clip entity.
fn find_audio_clip_params(
    audio_clips: &Query<(&StableId, &AudioClip)>,
    clip_uuid: Uuid,
) -> (f32, f32, bool) {
    for (stable_id, audio_clip) in audio_clips.iter() {
        if stable_id.uuid() == clip_uuid {
            return (audio_clip.volume, audio_clip.pan, audio_clip.muted);
        }
    }
    (1.0, 0.0, false)
}
