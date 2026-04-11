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
use std::time::Duration;
use strawberry_audio::KiraMixerService;
use strawberry_core::components::{AudioClip, Clip, StableId, Track, TrackAudio};
use strawberry_core::components::audio::{AudioFade, compute_fade_multiplier};
use strawberry_core::components::keyframe_track::KeyframeTrack;
use strawberry_core::resources::MasterAudioState;
use uuid::Uuid;

use crate::traverser::{ActiveLayersResource, CompositorLayer};

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
    audio_fades: Query<(&StableId, &AudioFade, &Clip)>,
    keyframe_tracks: Query<(&StableId, &KeyframeTrack, &Clip)>,
    clip_timings: Query<(&StableId, &Clip)>,
    track_query: Query<(&Track, Option<&TrackAudio>)>,
) {
    let mixer: &mut KiraMixerService = &mut mixer_resource.0;

    // Apply master volume + mute
    mixer.set_master_volume(master_audio.master_volume);
    mixer.set_master_muted(master_audio.master_muted);

    // Pre-compute: does any track have solo enabled?
    // If so, only clips on soloed tracks will play.
    let any_track_soloed: bool = track_query.iter().any(|(t, _)| t.solo);

    // Pre-compute crossfade multipliers for all active layers.
    // Only does work when 2+ clips share a track_index (overlap).
    let crossfade_multipliers: Vec<f32> = compute_crossfade_multipliers(
        &active_layers.layers,
        &clip_timings,
        playhead.current_time,
    );

    // Use layers already computed by compositor_system (no redundant traversal)
    let mut active_asset_uuids: HashSet<Uuid> = HashSet::new();

    for (layer_idx, layer) in active_layers.layers.iter().enumerate() {
        let asset_uuid: Uuid = layer.asset_uuid;

        // ---- Track-level audio filtering ----
        // Look up the parent track's audio state via entity bits
        // (propagated from TrackNode during traversal).
        let track_entity: Entity = Entity::from_bits(layer.track_entity_bits);
        let (track_muted, track_soloed, track_volume, track_pan): (bool, bool, f32, f32) =
            if let Ok((track, track_audio)) = track_query.get(track_entity) {
                let (vol, pan): (f32, f32) = track_audio
                    .map(|ta| (ta.volume, ta.pan))
                    .unwrap_or((1.0, 0.0));
                (track.muted, track.solo, vol, pan)
            } else {
                (false, false, 1.0, 0.0)
            };

        // Skip muted tracks
        if track_muted {
            // Stop any playing audio for this clip since track is muted
            if mixer.has_clip(&asset_uuid) {
                mixer.stop_clip(&asset_uuid);
            }
            continue;
        }

        // Solo logic: when any track is soloed, skip clips on non-soloed tracks
        if any_track_soloed && !track_soloed {
            if mixer.has_clip(&asset_uuid) {
                mixer.stop_clip(&asset_uuid);
            }
            continue;
        }

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

        let (clip_volume, clip_pan, clip_muted) = find_audio_clip_params(&audio_clips, layer.clip_uuid);

        // Apply fade multiplier (1.0 if no AudioFade component)
        let fade_multiplier: f32 =
            find_fade_multiplier(&audio_fades, layer.clip_uuid, layer.source_time);

        // Apply keyframe automation (1.0/0.0 if no KeyframeTrack or no keyframes)
        let (kf_volume, kf_pan): (f32, f32) =
            find_keyframe_automation(&keyframe_tracks, layer.clip_uuid, layer.source_time);

        // Apply crossfade multiplier (1.0 if no overlap on this track)
        let crossfade_mult: f32 = crossfade_multipliers
            .get(layer_idx)
            .copied()
            .unwrap_or(1.0);

        // Combine: clip × track × fade × keyframe automation × crossfade
        let effective_volume: f32 = clip_volume * track_volume * fade_multiplier * kf_volume * crossfade_mult;

        // Combine pan: clip + track + keyframe automation (additive, clamped)
        let effective_pan: f32 = (clip_pan + track_pan + kf_pan).clamp(-1.0, 1.0);

        mixer.update_clip_params(&asset_uuid, effective_volume, effective_pan, clip_muted);
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

/// Compute the fade volume multiplier for a clip.
///
/// Returns `1.0` if the clip has no `AudioFade` component (zero overhead).
/// Otherwise computes based on source time position within the clip.
///
/// ## Performance
/// - Iterates AudioFade query only for clips with the component (lazy insertion)
/// - Pure arithmetic in `compute_fade_multiplier` — no allocations
fn find_fade_multiplier(
    audio_fades: &Query<(&StableId, &AudioFade, &Clip)>,
    clip_uuid: Uuid,
    source_time: Duration,
) -> f32 {
    for (stable_id, fade, clip) in audio_fades.iter() {
        if stable_id.uuid() == clip_uuid {
            return compute_fade_multiplier(
                source_time,
                clip.source_in,
                clip.duration(),
                fade,
            );
        }
    }
    1.0 // No AudioFade → no attenuation
}

/// Find keyframe automation values for a clip.
///
/// Returns (volume_multiplier, pan_offset). Defaults to (1.0, 0.0)
/// when no KeyframeTrack is present (zero overhead for non-automated clips).
///
/// ## Performance
/// - Only iterates KeyframeTrack query for clips with the component (lazy insertion)
/// - `evaluate()` uses binary search O(log n) — typically <3 comparisons
fn find_keyframe_automation(
    keyframe_tracks: &Query<(&StableId, &KeyframeTrack, &Clip)>,
    clip_uuid: Uuid,
    source_time: Duration,
) -> (f32, f32) {
    use strawberry_core::components::keyframe_track::property_ids;

    for (stable_id, track, clip) in keyframe_tracks.iter() {
        if stable_id.uuid() == clip_uuid {
            // Compute clip-local time relative to source_in
            let clip_local_time_secs: f64 = source_time
                .saturating_sub(clip.source_in)
                .as_secs_f64();

            let volume: f32 = track.evaluate(property_ids::VOLUME, clip_local_time_secs, 1.0);
            let pan: f32 = track.evaluate(property_ids::PAN, clip_local_time_secs, 0.0);
            return (volume, pan);
        }
    }
    (1.0, 0.0) // No KeyframeTrack → passthrough
}

/// Compute crossfade volume multipliers for all active layers.
///
/// When two clips overlap on the same track, the **outgoing** clip (earlier start)
/// fades OUT and the **incoming** clip (later start) fades IN over the overlap region.
/// Uses **constant-power crossfade** (sqrt-based) — the industry standard that
/// prevents volume dips in the middle of the transition.
///
/// ## Performance
/// - Returns `Vec<f32>` of length `layers.len()`, all 1.0 if no overlaps
/// - Only iterates track-overlap pairs (typically 0-1 overlaps in normal editing)
/// - No allocations beyond the output Vec
fn compute_crossfade_multipliers(
    layers: &[CompositorLayer],
    clip_timings: &Query<(&StableId, &Clip)>,
    playhead_time: Duration,
) -> Vec<f32> {
    let layer_count: usize = layers.len();
    let mut multipliers: Vec<f32> = vec![1.0; layer_count];

    if layer_count < 2 {
        return multipliers; // No possible overlap with < 2 layers
    }

    // Check all pairs for same-track overlap
    for i in 0..layer_count {
        for j in (i + 1)..layer_count {
            if layers[i].track_index != layers[j].track_index {
                continue; // Different tracks, no crossfade
            }

            // Same track → these clips overlap at current playhead.
            // Determine which is outgoing (earlier start) vs incoming.
            let clip_i = find_clip_timing(clip_timings, layers[i].clip_uuid);
            let clip_j = find_clip_timing(clip_timings, layers[j].clip_uuid);

            let (clip_a, clip_b) = match (clip_i, clip_j) {
                (Some(a), Some(b)) => (a, b),
                _ => continue, // Can't determine timing, skip
            };

            // Identify outgoing (earlier start) and incoming (later start)
            let (out_idx, in_idx, out_clip, in_clip) = if clip_a.timeline_start <= clip_b.timeline_start {
                (i, j, clip_a, clip_b)
            } else {
                (j, i, clip_b, clip_a)
            };

            // Overlap region: from incoming.start to outgoing.end
            let overlap_start: Duration = in_clip.timeline_start;
            let overlap_end: Duration = out_clip.timeline_end();

            if overlap_end <= overlap_start {
                continue; // No actual overlap (shouldn't happen but defensive)
            }

            let overlap_duration_secs: f64 = overlap_end
                .saturating_sub(overlap_start)
                .as_secs_f64();

            if overlap_duration_secs <= 0.0 {
                continue;
            }

            // t = how far into the crossfade we are (0.0 = start, 1.0 = end)
            let elapsed_in_overlap: f64 = playhead_time
                .saturating_sub(overlap_start)
                .as_secs_f64();
            let t: f32 = (elapsed_in_overlap / overlap_duration_secs).clamp(0.0, 1.0) as f32;

            // Constant-power crossfade:
            //   outgoing clip: volume ramps DOWN via sqrt(1 - t)
            //   incoming clip: volume ramps UP via sqrt(t)
            // This preserves perceived loudness during the transition.
            multipliers[out_idx] = (1.0 - t).sqrt();
            multipliers[in_idx] = t.sqrt();
        }
    }

    multipliers
}

/// Find a clip's timeline timing by its UUID.
/// Returns a clone of the Clip component if found (cheap — just Duration copies).
fn find_clip_timing(
    clip_timings: &Query<(&StableId, &Clip)>,
    clip_uuid: Uuid,
) -> Option<Clip> {
    for (stable_id, clip) in clip_timings.iter() {
        if stable_id.uuid() == clip_uuid {
            return Some(clip.clone());
        }
    }
    None
}
