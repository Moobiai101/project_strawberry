//! Custom theme for Project Strawberry

use egui::{Color32, Style, Visuals};

/// CapCut-inspired minimalist dark theme
pub struct StrawberryTheme;

impl StrawberryTheme {
    /// Apply the CapCut minimalist theme to egui
    pub fn apply(ctx: &egui::Context) {
        let mut style = Style::default();

        // Dark theme base
        style.visuals = Visuals::dark();

        // CapCut style - pure black backgrounds
        style.visuals.window_fill = Color32::from_rgb(13, 13, 13); // #0d0d0d
        style.visuals.panel_fill = Color32::from_rgb(18, 18, 18); // #121212
        style.visuals.faint_bg_color = Color32::from_rgb(24, 24, 24); // #181818

        // White selection/accents
        style.visuals.selection.bg_fill = Color32::from_rgb(92, 107, 192); // Muted indigo
        style.visuals.selection.stroke.color = Color32::from_rgb(255, 255, 255);
        style.visuals.hyperlink_color = Color32::from_rgb(140, 140, 140); // Gray

        // Button styling - dark with white accents
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 30);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 45, 45);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(92, 107, 192); // Indigo active

        // Subtle dark borders
        style.visuals.widgets.inactive.bg_stroke.color = Color32::from_rgb(40, 40, 40);
        style.visuals.widgets.hovered.bg_stroke.color = Color32::from_rgb(80, 80, 80);

        ctx.set_style(style);
    }
}
