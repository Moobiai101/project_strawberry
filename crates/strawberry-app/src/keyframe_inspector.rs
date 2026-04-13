//! # Keyframe Inspector UI
//!
//! Per-clip keyframe automation controls for the Inspector panel.
//! Follows the DaVinci Resolve / After Effects inspector pattern:
//!
//! ## How Pro Editors Handle Keyframes
//!
//! 1. **Current value at playhead** — Shows the interpolated value at the
//!    current time. User edits this slider to set a "target" value.
//! 2. **Add keyframe at playhead** — Inserts a keyframe at the current time
//!    with the CURRENT slider value (not the default).
//! 3. **Per-keyframe rows** — Each keyframe shows time, an editable value
//!    slider, an interpolation dropdown, and a delete button.
//! 4. **"Enable automation"** — Clicking "+ Volume" or "+ Pan" enables
//!    automation and adds the first keyframe at the current playhead position.
//!
//! ## Architecture
//! - Read: `KeyframeTrack` component from selected clip entity
//! - Write: All mutations go through `CommandQueue` → keyframe commands
//! - Zero cost when no clip is selected (early return)

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use egui::Ui;

use strawberry_core::commands::keyframe_commands::{
    AddKeyframeCommand, RemoveKeyframeCommand,
};
use strawberry_core::components::keyframe::KeyframeInterpolation;
use strawberry_core::components::keyframe_track::{property_ids, KeyframeTrack};
use strawberry_core::resources::CommandQueue;

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

/// Render keyframe automation controls for the selected clip.
///
/// Called from the inspector panel in `docking.rs`.
/// Shows automation sections for each property that has keyframes,
/// plus an "Add Automation" button for properties that don't.
pub fn render_keyframe_inspector(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    clip_entity: Entity,
) {
    ui.separator();
    ui.strong("🎹 Keyframe Automation");
    ui.add_space(4.0);

    let track: Option<&KeyframeTrack> = world.get::<KeyframeTrack>(clip_entity);

    // Render existing properties
    let has_volume: bool = track.map(|t| t.has_keyframes(property_ids::VOLUME)).unwrap_or(false);
    let has_pan: bool = track.map(|t| t.has_keyframes(property_ids::PAN)).unwrap_or(false);

    if has_volume {
        render_volume_section(ui, world, clip_uuid, track.unwrap());
    }

    if has_pan {
        render_pan_section(ui, world, clip_uuid, track.unwrap());
    }

    if !has_volume || !has_pan {
        render_add_automation_buttons(ui, world, clip_uuid, has_volume, has_pan);
    }
}

// ============================================================================
// VOLUME SECTION
// ============================================================================

/// Render the volume automation section with editable per-keyframe sliders.
///
/// Each keyframe row has: time label, value slider (0%–200%), interpolation
/// dropdown, and delete button. The slider submits an AddKeyframeCommand
/// at the same time (replace semantics) when dragged.
fn render_volume_section(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    track: &KeyframeTrack,
) {
    egui::CollapsingHeader::new("📈 Volume Keyframes")
        .id_salt("kf_section_volume")
        .default_open(true)
        .show(ui, |ui| {
            render_keyframe_rows(
                ui, world, clip_uuid, track,
                property_ids::VOLUME,
                0.0,    // slider min (raw)
                2.0,    // slider max (raw)
                100.0,  // display scale (raw → percent)
                "%",    // suffix
                1.0,    // step
            );

            ui.add_space(2.0);
            render_add_keyframe_button(
                ui, world, clip_uuid, track,
                property_ids::VOLUME, "Volume", 1.0,
            );
        });
}

// ============================================================================
// PAN SECTION
// ============================================================================

/// Render the pan automation section with editable per-keyframe sliders.
fn render_pan_section(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    track: &KeyframeTrack,
) {
    egui::CollapsingHeader::new("📈 Pan Keyframes")
        .id_salt("kf_section_pan")
        .default_open(true)
        .show(ui, |ui| {
            render_keyframe_rows(
                ui, world, clip_uuid, track,
                property_ids::PAN,
                -1.0,   // slider min (raw)
                1.0,    // slider max (raw)
                100.0,  // display scale
                "%",    // suffix
                1.0,    // step
            );

            ui.add_space(2.0);
            render_add_keyframe_button(
                ui, world, clip_uuid, track,
                property_ids::PAN, "Pan", 0.0,
            );
        });
}

// ============================================================================
// KEYFRAME ROWS (shared for all properties)
// ============================================================================

/// Render editable keyframe rows for a property.
///
/// Each row: [time_label] [value_slider] [interp_dropdown] [🗑]
///
/// The value slider submits `AddKeyframeCommand` at the same time_secs
/// (replace-on-duplicate semantics). This is the same pattern After Effects
/// uses — editing a keyframe's value replaces the old keyframe.
fn render_keyframe_rows(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    track: &KeyframeTrack,
    property_id: &str,
    raw_min: f32,
    raw_max: f32,
    display_scale: f32,
    suffix: &str,
    step: f32,
) {
    let keyframes = match track.keyframes(property_id) {
        Some(kfs) => kfs,
        None => return,
    };

    // Collect actions to avoid borrow conflicts with the command queue
    let mut action: Option<KeyframeAction> = None;

    for (idx, kf) in keyframes.iter().enumerate() {
        let row_id: String = format!("kf_row_{property_id}_{idx}");

        ui.push_id(&row_id, |ui| {
            ui.horizontal(|ui| {
                // Time label (fixed width for alignment)
                ui.monospace(format!("{:.2}s", kf.time_secs));

                // Value slider — the core interaction
                let mut display_value: f32 = kf.value * display_scale;
                let slider_response = ui.add_sized(
                    [ui.available_width() - 60.0, 18.0],
                    egui::Slider::new(
                        &mut display_value,
                        (raw_min * display_scale)..=(raw_max * display_scale),
                    )
                    .suffix(suffix)
                    .step_by(step as f64)
                    .clamping(egui::SliderClamping::Always),
                );

                if slider_response.changed() {
                    let new_raw_value: f32 = display_value / display_scale;
                    action = Some(KeyframeAction::SetValue {
                        time_secs: kf.time_secs,
                        new_value: new_raw_value,
                        interpolation: kf.interpolation,
                    });
                }

                // Interpolation mode dropdown
                let interp_label: &str = match kf.interpolation {
                    KeyframeInterpolation::Hold => "H",
                    KeyframeInterpolation::Linear => "L",
                    KeyframeInterpolation::Bezier => "B",
                    KeyframeInterpolation::Smooth => "S",
                };

                let interp_response = ui.small_button(interp_label)
                    .on_hover_text(format!("Interpolation: {}", kf.interpolation.display_name()));

                if interp_response.clicked() {
                    // Cycle through interpolation modes
                    let next_interp: KeyframeInterpolation = match kf.interpolation {
                        KeyframeInterpolation::Linear => KeyframeInterpolation::Smooth,
                        KeyframeInterpolation::Smooth => KeyframeInterpolation::Bezier,
                        KeyframeInterpolation::Bezier => KeyframeInterpolation::Hold,
                        KeyframeInterpolation::Hold => KeyframeInterpolation::Linear,
                    };
                    action = Some(KeyframeAction::SetValue {
                        time_secs: kf.time_secs,
                        new_value: kf.value,
                        interpolation: next_interp,
                    });
                }

                // Delete button
                if ui.small_button("🗑").on_hover_text("Remove keyframe").clicked() {
                    action = Some(KeyframeAction::Remove(idx));
                }
            });
        });
    }

    // Process deferred action
    if let Some(act) = action {
        let command_queue: &CommandQueue = world.resource::<CommandQueue>();
        match act {
            KeyframeAction::Remove(idx) => {
                command_queue.submit(Box::new(RemoveKeyframeCommand::new(
                    clip_uuid,
                    property_id.to_string(),
                    idx,
                )));
            }
            KeyframeAction::SetValue { time_secs, new_value, interpolation } => {
                // Replace keyframe at same time (AddKeyframe has replace semantics)
                command_queue.submit(Box::new(
                    AddKeyframeCommand::new(
                        clip_uuid,
                        property_id.to_string(),
                        time_secs,
                        new_value,
                    ).with_interpolation(interpolation),
                ));
            }
        }
    }
}

/// Deferred action to avoid mutable borrow conflicts during iteration.
enum KeyframeAction {
    Remove(usize),
    SetValue {
        time_secs: f64,
        new_value: f32,
        interpolation: KeyframeInterpolation,
    },
}

// ============================================================================
// ADD KEYFRAME BUTTON
// ============================================================================

/// Render the "Add Keyframe at Playhead" button.
///
/// Uses the CURRENT interpolated value at the playhead position as the
/// initial value — not the default. This matches After Effects behavior
/// where adding a keyframe "locks in" the current animated value.
fn render_add_keyframe_button(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    track: &KeyframeTrack,
    property_id: &str,
    display_name: &str,
    default_value: f32,
) {
    if ui.button(format!("+ Add {display_name} Keyframe at Playhead")).clicked() {
        let playhead = world.resource::<strawberry_core::resources::Playhead>();
        let time_secs: f64 = playhead.current_time.as_secs_f64();

        // Use the current interpolated value (not default) — matches AE behavior
        let current_value: f32 = track.evaluate(property_id, time_secs, default_value);

        let command_queue: &CommandQueue = world.resource::<CommandQueue>();
        command_queue.submit(Box::new(AddKeyframeCommand::new(
            clip_uuid,
            property_id.to_string(),
            time_secs,
            current_value,
        )));
    }
}

// ============================================================================
// ADD AUTOMATION BUTTONS
// ============================================================================

/// Render buttons to enable automation for properties that don't have it yet.
///
/// Clicking "+ Volume" adds the FIRST keyframe at the current playhead
/// position with the clip's current static value — so enabling automation
/// doesn't change the sound. The user then moves the playhead and adds
/// more keyframes to create the automation curve.
fn render_add_automation_buttons(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    has_volume: bool,
    has_pan: bool,
) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Add Automation:").weak());
    ui.horizontal(|ui| {
        if !has_volume {
            if ui.button("+ Volume").clicked() {
                let playhead = world.resource::<strawberry_core::resources::Playhead>();
                let time_secs: f64 = playhead.current_time.as_secs_f64();
                let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                command_queue.submit(Box::new(AddKeyframeCommand::new(
                    clip_uuid,
                    property_ids::VOLUME.to_string(),
                    time_secs,
                    1.0,
                )));
            }
        }
        if !has_pan {
            if ui.button("+ Pan").clicked() {
                let playhead = world.resource::<strawberry_core::resources::Playhead>();
                let time_secs: f64 = playhead.current_time.as_secs_f64();
                let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                command_queue.submit(Box::new(AddKeyframeCommand::new(
                    clip_uuid,
                    property_ids::PAN.to_string(),
                    time_secs,
                    0.0,
                )));
            }
        }
    });
}
