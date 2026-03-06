//! # Settings Panel
//!
//! User preferences panel for proxy generation and UI settings.
//!
//! ## Features
//!
//! - Proxy resolution selector (480p, 720p, 1080p, Original)
//! - JPEG quality selector (Low, Medium, High)
//! - Concurrent jobs slider
//! - Auto-generate toggle
//! - UI preferences (filmstrips, waveforms, snapping)
//!
//! ## Integration
//!
//! Panel reads/writes to `UserSettings` resource in strawberry-core.
//! Changes are automatically persisted when the panel is closed.

use egui::{Color32, Ui};

/// Response from settings panel
#[derive(Default)]
pub struct SettingsResponse {
    /// Whether settings were modified this frame
    pub settings_changed: bool,
    /// Whether user requested to close the panel
    pub close_requested: bool,
}

/// Settings panel state
#[derive(Clone)]
pub struct SettingsPanel {
    /// Current section being viewed
    pub current_section: SettingsSection,
}

/// Settings sections
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    Proxy,
    Timeline,
    General,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            current_section: SettingsSection::Proxy,
        }
    }
}

impl SettingsPanel {
    /// Create a new settings panel
    pub fn new() -> Self {
        Self::default()
    }

    /// Render the settings panel
    ///
    /// Takes mutable reference to settings so changes are applied directly.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        proxy_resolution: &mut u8, // 0=480p, 1=720p, 2=1080p, 3=Original
        proxy_quality: &mut u8,    // 0=Low, 1=Medium, 2=High
        max_concurrent_jobs: &mut u8,
        auto_generate: &mut bool,
        show_filmstrip: &mut bool,
        show_waveforms: &mut bool,
        snap_to_playhead: &mut bool,
        snap_to_clips: &mut bool,
    ) -> SettingsResponse {
        let mut response = SettingsResponse::default();

        ui.vertical(|ui| {
            // Header
            ui.horizontal(|ui| {
                ui.heading("⚙ Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        response.close_requested = true;
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Section tabs
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.current_section == SettingsSection::Proxy, "📹 Proxy")
                    .clicked()
                {
                    self.current_section = SettingsSection::Proxy;
                }
                if ui
                    .selectable_label(
                        self.current_section == SettingsSection::Timeline,
                        "🎬 Timeline",
                    )
                    .clicked()
                {
                    self.current_section = SettingsSection::Timeline;
                }
                if ui
                    .selectable_label(
                        self.current_section == SettingsSection::General,
                        "⚡ General",
                    )
                    .clicked()
                {
                    self.current_section = SettingsSection::General;
                }
            });
            ui.add_space(12.0);

            // Section content
            match self.current_section {
                SettingsSection::Proxy => {
                    if Self::show_proxy_section(
                        ui,
                        proxy_resolution,
                        proxy_quality,
                        max_concurrent_jobs,
                        auto_generate,
                    ) {
                        response.settings_changed = true;
                    }
                }
                SettingsSection::Timeline => {
                    if Self::show_timeline_section(
                        ui,
                        show_filmstrip,
                        show_waveforms,
                        snap_to_playhead,
                        snap_to_clips,
                    ) {
                        response.settings_changed = true;
                    }
                }
                SettingsSection::General => {
                    ui.label("General settings coming soon...");
                }
            }
        });

        response
    }

    /// Render proxy settings section
    fn show_proxy_section(
        ui: &mut Ui,
        resolution: &mut u8,
        quality: &mut u8,
        max_jobs: &mut u8,
        auto_generate: &mut bool,
    ) -> bool {
        let mut changed = false;

        ui.label("Proxy Generation Settings");
        ui.add_space(8.0);

        // Resolution dropdown
        egui::Grid::new("proxy_settings_grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                ui.label("Resolution:");
                let resolution_labels = [
                    "480p (Fast)",
                    "720p (Balanced)",
                    "1080p (Quality)",
                    "Original",
                ];
                egui::ComboBox::from_id_salt("proxy_resolution")
                    .selected_text(
                        *resolution_labels
                            .get(*resolution as usize)
                            .unwrap_or(&"Unknown"),
                    )
                    .show_ui(ui, |ui| {
                        for (i, label) in resolution_labels.iter().enumerate() {
                            if ui.selectable_value(resolution, i as u8, *label).changed() {
                                changed = true;
                            }
                        }
                    });
                ui.end_row();

                ui.label("Quality:");
                let quality_labels = ["Low (Fast)", "Medium", "High"];
                egui::ComboBox::from_id_salt("proxy_quality")
                    .selected_text(*quality_labels.get(*quality as usize).unwrap_or(&"Unknown"))
                    .show_ui(ui, |ui| {
                        for (i, label) in quality_labels.iter().enumerate() {
                            if ui.selectable_value(quality, i as u8, *label).changed() {
                                changed = true;
                            }
                        }
                    });
                ui.end_row();

                ui.label("Concurrent Jobs:");
                if ui.add(egui::Slider::new(max_jobs, 1..=4)).changed() {
                    changed = true;
                }
                ui.end_row();

                ui.label("Auto-generate on import:");
                if ui.checkbox(auto_generate, "").changed() {
                    changed = true;
                }
                ui.end_row();
            });

        ui.add_space(16.0);

        // Info box
        ui.group(|ui| {
            ui.colored_label(
                Color32::from_rgb(100, 100, 100),
                "💡 Lower resolution and quality = faster generation + smaller files",
            );
        });

        changed
    }

    /// Render timeline settings section
    fn show_timeline_section(
        ui: &mut Ui,
        show_filmstrip: &mut bool,
        show_waveforms: &mut bool,
        snap_to_playhead: &mut bool,
        snap_to_clips: &mut bool,
    ) -> bool {
        let mut changed = false;

        ui.label("Timeline Display");
        ui.add_space(8.0);

        egui::Grid::new("timeline_settings_grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                ui.label("Show filmstrip thumbnails:");
                if ui.checkbox(show_filmstrip, "").changed() {
                    changed = true;
                }
                ui.end_row();

                ui.label("Show audio waveforms:");
                if ui.checkbox(show_waveforms, "").changed() {
                    changed = true;
                }
                ui.end_row();
            });

        ui.add_space(16.0);
        ui.label("Snapping");
        ui.add_space(8.0);

        egui::Grid::new("snapping_settings_grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                ui.label("Snap to playhead:");
                if ui.checkbox(snap_to_playhead, "").changed() {
                    changed = true;
                }
                ui.end_row();

                ui.label("Snap to clips:");
                if ui.checkbox(snap_to_clips, "").changed() {
                    changed = true;
                }
                ui.end_row();
            });

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_panel_default() {
        let panel = SettingsPanel::new();
        assert_eq!(panel.current_section, SettingsSection::Proxy);
    }

    #[test]
    fn test_settings_response_default() {
        let response = SettingsResponse::default();
        assert!(!response.settings_changed);
        assert!(!response.close_requested);
    }
}
