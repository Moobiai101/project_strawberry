//! # Aspect Ratio Dropdown Widget
//!
//! Clipchamp-style "Size" dropdown for selecting viewport aspect ratio.
//!
//! ## Visual Design
//!
//! Matches Clipchamp's dropdown:
//! - Button shows "📐 Size ▾" with current preset indicator
//! - Dropdown panel lists all presets with:
//!   - Checkmark (✓) for the active preset
//!   - Bold label (e.g. "Wide 16:9")
//!   - Subtle subtitle (e.g. "YouTube and streaming sites")
//!
//! ## Usage
//!
//! ```ignore
//! let dropdown_response = AspectRatioDropdown::new(current_preset)
//!     .show(ui);
//!
//! if let Some(new_preset) = dropdown_response.selected_preset {
//!     command_queue.submit(Box::new(SetProjectResolutionCommand::new(new_preset)));
//! }
//! ```

use egui::{Align2, Color32, Id, Pos2, Rect, Rounding, Stroke, Ui, Vec2};
use strawberry_core::resources::AspectRatioPreset;

// ============================================================================
// DROPDOWN RESPONSE
// ============================================================================

/// Response from the aspect ratio dropdown widget.
///
/// Contains the newly selected preset, if the user clicked one.
#[derive(Debug, Default)]
pub struct AspectRatioDropdownResponse {
    /// The preset the user selected, if any (None = no change)
    pub selected_preset: Option<AspectRatioPreset>,
}

// ============================================================================
// DROPDOWN WIDGET
// ============================================================================

/// Clipchamp-style "Size" dropdown for aspect ratio selection.
///
/// Renders as a toolbar button that opens a floating dropdown panel
/// with all preset options. The active preset shows a checkmark.
pub struct AspectRatioDropdown {
    /// Currently active preset (shown as checked in the dropdown)
    current_preset: AspectRatioPreset,
}

impl AspectRatioDropdown {
    /// Create a new dropdown with the currently active preset.
    pub fn new(current_preset: AspectRatioPreset) -> Self {
        Self { current_preset }
    }

    /// Show the dropdown button and popup panel.
    ///
    /// Returns `AspectRatioDropdownResponse` with the selected preset if changed.
    pub fn show(&self, ui: &mut Ui) -> AspectRatioDropdownResponse {
        let mut response = AspectRatioDropdownResponse::default();
        let dropdown_id: Id = Id::new("aspect_ratio_dropdown_open");

        // Read current open state from egui memory
        let is_open: bool =
            ui.memory(|mem| mem.data.get_temp::<bool>(dropdown_id).unwrap_or(false));

        // ================================================================
        // TOOLBAR BUTTON: "📐 Size ▾"
        // ================================================================
        let button_text: String = format!("📐 Size ▾");
        let button_response = ui.add(
            egui::Button::new(
                egui::RichText::new(&button_text)
                    .size(12.0)
                    .color(Color32::from_rgb(200, 200, 210)),
            )
            .fill(if is_open {
                Color32::from_rgb(50, 50, 60)
            } else {
                Color32::from_rgb(35, 35, 45)
            })
            .stroke(Stroke::new(1.0, Color32::from_rgb(70, 70, 80)))
            .rounding(Rounding::same(4.0)),
        );

        // Toggle dropdown on click
        if button_response.clicked() {
            ui.memory_mut(|mem| {
                mem.data.insert_temp(dropdown_id, !is_open);
            });
        }

        // ================================================================
        // DROPDOWN PANEL
        // ================================================================
        if is_open {
            let button_rect: Rect = button_response.rect;

            let popup_response = egui::Area::new(Id::new("aspect_ratio_dropdown_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(button_rect.left(), button_rect.bottom() + 2.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(30, 30, 38))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 70)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::same(6.0))
                        .show(ui, |ui| {
                            ui.set_min_width(240.0);

                            for &preset in AspectRatioPreset::ALL_PRESETS {
                                let is_active: bool = preset == self.current_preset;

                                if self.render_preset_row(ui, preset, is_active) {
                                    response.selected_preset = Some(preset);
                                    // Close dropdown after selection
                                    ui.memory_mut(|mem| {
                                        mem.data.insert_temp(dropdown_id, false);
                                    });
                                }
                            }
                        });
                });

            // Close dropdown if user clicks outside the popup area
            let popup_rect: Rect = popup_response.response.rect;
            let combined_rect: Rect = Rect::from_min_max(button_rect.min, popup_rect.max);

            if ui.input(|i| i.pointer.any_click()) {
                let click_pos: Option<Pos2> = ui.input(|i| i.pointer.latest_pos());
                if let Some(pos) = click_pos {
                    if !combined_rect.contains(pos) {
                        ui.memory_mut(|mem| {
                            mem.data.insert_temp(dropdown_id, false);
                        });
                    }
                }
            }
        }

        response
    }

    /// Render a single preset row in the dropdown.
    ///
    /// Returns true if the row was clicked (user selected this preset).
    fn render_preset_row(&self, ui: &mut Ui, preset: AspectRatioPreset, is_active: bool) -> bool {
        let mut clicked: bool = false;

        // Interactive area for the entire row
        let desired_height: f32 = 36.0;

        let (response, painter) = ui.allocate_painter(
            Vec2::new(ui.available_width(), desired_height),
            egui::Sense::click(),
        );
        let rect: Rect = response.rect;

        // Hover highlight
        let bg_color: Color32 = if response.hovered() {
            Color32::from_rgb(45, 45, 55)
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, Rounding::same(4.0), bg_color);

        // Checkmark for active preset
        let check_x: f32 = rect.left() + 12.0;
        let text_y: f32 = rect.top() + 8.0;

        if is_active {
            painter.text(
                Pos2::new(check_x, text_y),
                Align2::LEFT_TOP,
                "✓",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(100, 200, 100),
            );
        }

        // Label (e.g. "Wide 16:9")
        let label_x: f32 = rect.left() + 30.0;
        let label_color: Color32 = if is_active {
            Color32::WHITE
        } else {
            Color32::from_rgb(200, 200, 210)
        };
        painter.text(
            Pos2::new(label_x, text_y),
            Align2::LEFT_TOP,
            preset.label(),
            egui::FontId::proportional(13.0),
            label_color,
        );

        // Subtitle (e.g. "YouTube and streaming sites")
        let subtitle_y: f32 = text_y + 15.0;
        painter.text(
            Pos2::new(label_x, subtitle_y),
            Align2::LEFT_TOP,
            preset.subtitle(),
            egui::FontId::proportional(10.0),
            Color32::from_rgb(120, 120, 135),
        );

        if response.clicked() {
            clicked = true;
        }

        clicked
    }
}
