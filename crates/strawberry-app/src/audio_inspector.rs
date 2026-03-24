//! # Audio Inspector Controls
//!
//! Extracted audio control UI for the Inspector panel to maintain the 400-line rule.
//!
//! Contains:
//! - `render_clip_audio_controls()` — Per-clip volume/pan/mute sliders
//! - `render_master_audio_controls()` — Master volume + mute (always visible)
//!
//! ## Performance
//! - Reads only the selected clip (O(1) entity lookup)
//! - Commands only submitted on `response.changed()` (no work when idle)
//! - Volume/pan commands use `can_merge()` for slider dragging
//!
//! ## Responsiveness
//! - Sliders take full available width via `ui.available_width()`
//! - Labels and controls adapt to panel size
//! - Works within parent ScrollArea for vertical overflow

use bevy_ecs::world::World;
use egui::Ui;

use strawberry_core::components::AudioClip;
use strawberry_core::components::audio::{AudioFade, FadeCurve};
use strawberry_core::resources::{CommandQueue, MasterAudioState};

/// Render per-clip audio controls in the inspector panel.
///
/// Shows:
/// - Volume slider (0%–200%, linear)
/// - Pan slider (-100% to +100%, left–right)
/// - Mute toggle checkbox
///
/// Only renders when a clip is selected. Submits `SetClipAudioCommand`
/// via `CommandQueue` on change.
pub fn render_clip_audio_controls(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    clip_entity: bevy_ecs::entity::Entity,
) {
    ui.separator();
    ui.strong("🎧 Audio");
    ui.add_space(4.0);

    // Read current audio state (default if no AudioClip component yet)
    let audio: AudioClip = world
        .get::<AudioClip>(clip_entity)
        .cloned()
        .unwrap_or_default();

    let mut volume_percent: f32 = audio.volume * 100.0;
    let mut pan_percent: f32 = audio.pan * 100.0;
    let mut muted: bool = audio.muted;

    // --- Volume slider (0%–200%) ---
    ui.horizontal(|ui| {
        ui.label("Vol:");
        let volume_response = ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Slider::new(&mut volume_percent, 0.0..=200.0)
                .suffix("%")
                .step_by(1.0)
                .clamping(egui::SliderClamping::Always),
        );
        if volume_response.changed() {
            let command_queue: &CommandQueue = world.resource::<CommandQueue>();
            command_queue.submit(Box::new(
                strawberry_core::commands::SetClipAudioCommand::set_volume(
                    clip_uuid,
                    volume_percent / 100.0,
                ),
            ));
        }
    });

    // --- Pan slider (-100% to +100%) ---
    ui.horizontal(|ui| {
        ui.label("Pan:");
        let pan_response = ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Slider::new(&mut pan_percent, -100.0..=100.0)
                .suffix("%")
                .step_by(1.0)
                .clamping(egui::SliderClamping::Always)
                .custom_formatter(|val, _| {
                    if val < -1.0 {
                        format!("L {:.0}%", val.abs())
                    } else if val > 1.0 {
                        format!("R {:.0}%", val)
                    } else {
                        "Center".to_string()
                    }
                }),
        );
        if pan_response.changed() {
            let command_queue: &CommandQueue = world.resource::<CommandQueue>();
            command_queue.submit(Box::new(
                strawberry_core::commands::SetClipAudioCommand::set_pan(
                    clip_uuid,
                    pan_percent / 100.0,
                ),
            ));
        }
    });

    // --- Mute checkbox ---
    let mute_response = ui.checkbox(&mut muted, "Mute Clip");
    if mute_response.changed() {
        let command_queue: &CommandQueue = world.resource::<CommandQueue>();
        command_queue.submit(Box::new(
            strawberry_core::commands::SetClipAudioCommand::set_muted(clip_uuid, muted),
        ));
    }

    // --- Fade Controls ---
    ui.add_space(4.0);
    ui.strong("🎵 Fades");
    ui.add_space(2.0);

    let fade: AudioFade = world
        .get::<AudioFade>(clip_entity)
        .cloned()
        .unwrap_or_default();

    let mut fade_in_ms: f32 = fade.fade_in.as_millis() as f32;
    let mut fade_out_ms: f32 = fade.fade_out.as_millis() as f32;
    let mut current_curve: FadeCurve = fade.curve;

    // Fade-in slider (0ms – 5000ms)
    ui.horizontal(|ui| {
        ui.label("In:");
        let fade_in_response = ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Slider::new(&mut fade_in_ms, 0.0..=5000.0)
                .suffix("ms")
                .step_by(10.0)
                .clamping(egui::SliderClamping::Always),
        );
        if fade_in_response.changed() {
            let command_queue: &CommandQueue = world.resource::<CommandQueue>();
            command_queue.submit(Box::new(
                strawberry_core::commands::SetClipFadeCommand::set_fade_in(
                    clip_uuid,
                    fade_in_ms as u32,
                ),
            ));
        }
    });

    // Fade-out slider (0ms – 5000ms)
    ui.horizontal(|ui| {
        ui.label("Out:");
        let fade_out_response = ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Slider::new(&mut fade_out_ms, 0.0..=5000.0)
                .suffix("ms")
                .step_by(10.0)
                .clamping(egui::SliderClamping::Always),
        );
        if fade_out_response.changed() {
            let command_queue: &CommandQueue = world.resource::<CommandQueue>();
            command_queue.submit(Box::new(
                strawberry_core::commands::SetClipFadeCommand::set_fade_out(
                    clip_uuid,
                    fade_out_ms as u32,
                ),
            ));
        }
    });

    // Curve dropdown
    ui.horizontal(|ui| {
        ui.label("Curve:");
        egui::ComboBox::from_id_salt("fade_curve_combo")
            .selected_text(current_curve.display_name())
            .show_ui(ui, |ui| {
                for &curve in FadeCurve::ALL {
                    if ui
                        .selectable_value(&mut current_curve, curve, curve.display_name())
                        .changed()
                    {
                        let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                        command_queue.submit(Box::new(
                            strawberry_core::commands::SetClipFadeCommand::set_curve(
                                clip_uuid,
                                current_curve,
                            ),
                        ));
                    }
                }
            });
    });

    // --- Loudness Normalization ---
    ui.add_space(4.0);
    ui.strong("📊 Normalize");
    ui.add_space(2.0);

    // LUFS preset dropdown
    let lufs_id = egui::Id::new("normalize_lufs_target");
    let mut target_lufs: f64 = ui.memory(|mem| {
        mem.data
            .get_temp::<f64>(lufs_id)
            .unwrap_or(strawberry_audio::loudness::targets::YOUTUBE_LUFS)
    });

    ui.horizontal(|ui| {
        ui.label("Target:");
        egui::ComboBox::from_id_salt("normalize_lufs_combo")
            .selected_text(format!("{:.0} LUFS", target_lufs))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut target_lufs,
                    strawberry_audio::loudness::targets::YOUTUBE_LUFS,
                    "-14 LUFS (YouTube / Spotify)",
                );
                ui.selectable_value(
                    &mut target_lufs,
                    strawberry_audio::loudness::targets::STREAMING_LUFS,
                    "-16 LUFS (Streaming)",
                );
                ui.selectable_value(
                    &mut target_lufs,
                    strawberry_audio::loudness::targets::BROADCAST_LUFS,
                    "-23 LUFS (Broadcast / EBU R128)",
                );
            });
    });

    // Persist selected LUFS target
    ui.memory_mut(|mem| mem.data.insert_temp(lufs_id, target_lufs));

    // Normalize button — measures LUFS and submits command
    if ui.button("⚡ Normalize Volume").clicked() {
        // Access AudioMixerResource to measure LUFS
        if let Some(mixer) = world
            .get_resource::<strawberry_render::AudioMixerResource>()
        {
            // Get asset UUID from ClipAssetRef component on the clip entity
            if let Some(asset_ref) = world.get::<strawberry_core::node::timeline::clip::ClipAssetRef>(clip_entity) {
                let asset_uuid: uuid::Uuid = asset_ref.asset_uuid;
                if let Some(current_lufs) = mixer.0.measure_clip_loudness(&asset_uuid) {
                    let gain: f32 =
                        strawberry_audio::compute_normalization_gain(current_lufs, target_lufs);

                    let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                    command_queue.submit(Box::new(
                        strawberry_core::commands::NormalizeAudioCommand::new(
                            clip_uuid,
                            gain,
                            target_lufs,
                        ),
                    ));
                } else {
                    tracing::warn!("No audio registered for asset {}", asset_uuid);
                }
            }
        }
    }
}

/// Render master audio controls at the bottom of the inspector.
///
/// Always visible regardless of clip selection.
/// Shows master volume slider and mute toggle.
pub fn render_master_audio_controls(ui: &mut Ui, world: &World) {
    ui.separator();

    egui::CollapsingHeader::new("🔊 Master Audio")
        .default_open(true)
        .show(ui, |ui| {
            let master: MasterAudioState = world.resource::<MasterAudioState>().clone();
            let mut master_volume_percent: f32 = master.master_volume * 100.0;
            let mut master_muted: bool = master.master_muted;

            // Master volume slider — fill full width
            ui.horizontal(|ui| {
                ui.label("Vol:");
                let vol_response = ui.add_sized(
                    [ui.available_width(), 18.0],
                    egui::Slider::new(&mut master_volume_percent, 0.0..=200.0)
                        .suffix("%")
                        .step_by(1.0)
                        .clamping(egui::SliderClamping::Always),
                );
                if vol_response.changed() {
                    let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                    command_queue.submit(Box::new(
                        strawberry_core::commands::SetMasterAudioCommand::set_volume(
                            master_volume_percent / 100.0,
                        ),
                    ));
                }
            });

            // Master mute
            let mute_response = ui.checkbox(&mut master_muted, "Mute All");
            if mute_response.changed() {
                let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                command_queue.submit(Box::new(
                    strawberry_core::commands::SetMasterAudioCommand::set_muted(master_muted),
                ));
            }
        });
}
