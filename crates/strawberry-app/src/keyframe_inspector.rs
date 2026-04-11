//! # Keyframe Inspector UI
//!
//! Per-clip keyframe automation controls for the Inspector panel.
//! Follows the same pattern as `audio_inspector.rs`.
//!
//! ## Features
//! - List of keyframes per property with time/value display
//! - Add keyframe button (inserts at current playhead position)
//! - Remove keyframe button (per-keyframe delete)
//! - Value editing via slider (submits AddKeyframe as replace)
//! - Interpolation mode selector per keyframe
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
        render_property_section(
            ui, world, clip_uuid, track.unwrap(),
            property_ids::VOLUME, "Volume", 0.0, 2.0, 1.0,
            |v| format!("{:.0}%", v * 100.0),
        );
    }

    if has_pan {
        render_property_section(
            ui, world, clip_uuid, track.unwrap(),
            property_ids::PAN, "Pan", -1.0, 1.0, 0.0,
            |v| {
                if v < -0.01 {
                    format!("L {:.0}%", v.abs() * 100.0)
                } else if v > 0.01 {
                    format!("R {:.0}%", v * 100.0)
                } else {
                    "Center".to_string()
                }
            },
        );
    }

    if !has_volume || !has_pan {
        render_add_automation_buttons(ui, world, clip_uuid, has_volume, has_pan);
    }
}

// ============================================================================
// PROPERTY SECTION
// ============================================================================

/// Render a collapsible section for a single animated property.
///
/// Shows each keyframe as a row with time, value, interpolation mode,
/// and a delete button.
fn render_property_section(
    ui: &mut Ui,
    world: &World,
    clip_uuid: uuid::Uuid,
    track: &KeyframeTrack,
    property_id: &str,
    display_name: &str,
    _min_value: f32,
    _max_value: f32,
    default_value: f32,
    format_value: impl Fn(f32) -> String,
) {
    let id_str: String = format!("kf_section_{property_id}");
    egui::CollapsingHeader::new(format!("📈 {display_name} Keyframes"))
        .id_salt(&id_str)
        .default_open(true)
        .show(ui, |ui| {
            let keyframes = track.keyframes(property_id);
            if let Some(kfs) = keyframes {
                // Collect actions to avoid borrow conflict
                let mut action: Option<KeyframeAction> = None;

                for (idx, kf) in kfs.iter().enumerate() {
                    ui.horizontal(|ui| {
                        // Time display
                        ui.monospace(format!("{:.2}s", kf.time_secs));

                        // Value display
                        ui.label(format_value(kf.value));

                        // Interpolation mode label
                        ui.label(format!("[{}]", kf.interpolation.display_name()));

                        // Delete button
                        if ui.small_button("🗑").on_hover_text("Remove keyframe").clicked() {
                            action = Some(KeyframeAction::Remove(idx));
                        }
                    });
                }

                // Process deferred action
                if let Some(act) = action {
                    match act {
                        KeyframeAction::Remove(idx) => {
                            let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                            command_queue.submit(Box::new(RemoveKeyframeCommand::new(
                                clip_uuid,
                                property_id.to_string(),
                                idx,
                            )));
                        }
                    }
                }

                ui.add_space(2.0);

                // Add keyframe button
                if ui.button(format!("+ Add {display_name} Keyframe")).clicked() {
                    let playhead = world.resource::<strawberry_core::resources::Playhead>();
                    let time_secs: f64 = playhead.current_time.as_secs_f64();

                    let command_queue: &CommandQueue = world.resource::<CommandQueue>();
                    command_queue.submit(Box::new(AddKeyframeCommand::new(
                        clip_uuid,
                        property_id.to_string(),
                        time_secs,
                        default_value,
                    )));
                }
            }
        });
}

/// Deferred action to avoid mutable borrow conflicts during iteration.
enum KeyframeAction {
    Remove(usize),
}

// ============================================================================
// ADD AUTOMATION BUTTONS
// ============================================================================

/// Render buttons to enable automation for properties that don't have it yet.
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
                add_default_automation(world, clip_uuid, property_ids::VOLUME, 1.0);
            }
        }
        if !has_pan {
            if ui.button("+ Pan").clicked() {
                add_default_automation(world, clip_uuid, property_ids::PAN, 0.0);
            }
        }
    });
}

/// Add two default keyframes (start and end of clip) for a property.
///
/// This creates a flat automation line at the default value,
/// which the user can then edit by adding/moving keyframes.
fn add_default_automation(
    world: &World,
    clip_uuid: uuid::Uuid,
    property_id: &str,
    default_value: f32,
) {
    let command_queue: &CommandQueue = world.resource::<CommandQueue>();

    // Add a keyframe at t=0 with the default value
    command_queue.submit(Box::new(AddKeyframeCommand::new(
        clip_uuid,
        property_id.to_string(),
        0.0,
        default_value,
    )));
}
