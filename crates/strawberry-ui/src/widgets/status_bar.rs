//! # Status Bar Widget
//!
//! Displays proxy generation progress and app status at the bottom of the UI.
//!
//! ## Features
//!
//! - Animated progress bar during proxy generation
//! - Shows per-asset status messages
//! - Displays estimated time remaining
//! - "Complete" notification that fades after 3 seconds
//!
//! ## Usage
//!
//! ```ignore
//! let response = status_bar_widget(ui, &proxy_progress, &stats);
//! // Response contains any actions to take (if any)
//! ```

use egui::{Color32, Rect, Rounding, Sense, Ui, Vec2};

/// Response from status bar widget
#[derive(Default)]
pub struct StatusBarResponse {
    /// Whether the user clicked to cancel proxy generation
    pub cancel_generation: bool,
}

/// Status bar configuration
pub struct StatusBarConfig {
    /// Height of the status bar
    pub height: f32,
    /// Background color
    pub background: Color32,
    /// Progress bar color
    pub progress_color: Color32,
    /// Text color
    pub text_color: Color32,
    /// Progress bar background
    pub progress_bg: Color32,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            height: 24.0,
            background: Color32::from_rgb(30, 30, 30),
            progress_color: Color32::from_rgb(76, 175, 80), // Green
            text_color: Color32::from_rgb(200, 200, 200),
            progress_bg: Color32::from_rgb(50, 50, 50),
        }
    }
}

/// Render the status bar widget
///
/// Shows proxy generation progress if active, otherwise shows general status.
///
/// # Arguments
/// * `ui` - The egui UI context
/// * `progress_message` - Current status message to display
/// * `progress_value` - Progress from 0.0 to 1.0 (None if no progress bar)
/// * `is_active` - Whether proxy generation is in progress
/// * `recently_completed` - Whether generation just completed (for success message)
pub fn status_bar_widget(
    ui: &mut Ui,
    progress_message: &str,
    progress_value: Option<f32>,
    is_active: bool,
    recently_completed: bool,
    config: &StatusBarConfig,
) -> StatusBarResponse {
    let mut response = StatusBarResponse::default();

    let available_width = ui.available_width();
    let bar_rect = ui.allocate_space(Vec2::new(available_width, config.height));
    let rect = bar_rect.1;

    // Background
    ui.painter()
        .rect_filled(rect, Rounding::ZERO, config.background);

    // Content based on state
    if is_active {
        // Show progress bar
        response = render_progress_state(ui, rect, progress_message, progress_value, config);
    } else if recently_completed {
        // Show success message
        render_completed_state(ui, rect, progress_message, config);
    } else {
        // Show idle state
        render_idle_state(ui, rect, config);
    }

    response
}

/// Render progress bar and status during active generation
fn render_progress_state(
    ui: &mut Ui,
    rect: Rect,
    message: &str,
    progress: Option<f32>,
    config: &StatusBarConfig,
) -> StatusBarResponse {
    let mut response = StatusBarResponse::default();

    let progress_bar_width = 200.0;
    let progress_bar_height = 8.0;
    let padding = 8.0;

    // Progress bar background
    let progress_rect = Rect::from_min_size(
        egui::pos2(
            rect.max.x - progress_bar_width - padding,
            rect.center().y - progress_bar_height / 2.0,
        ),
        Vec2::new(progress_bar_width, progress_bar_height),
    );
    ui.painter().rect_filled(
        progress_rect,
        Rounding::same(progress_bar_height / 2.0),
        config.progress_bg,
    );

    // Progress bar fill
    if let Some(progress_val) = progress {
        let fill_width = progress_bar_width * progress_val.clamp(0.0, 1.0);
        let fill_rect = Rect::from_min_size(
            progress_rect.min,
            Vec2::new(fill_width, progress_bar_height),
        );
        ui.painter().rect_filled(
            fill_rect,
            Rounding::same(progress_bar_height / 2.0),
            config.progress_color,
        );
    }

    // Status message (left side)
    let text_pos = egui::pos2(rect.min.x + padding, rect.center().y);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        message,
        egui::FontId::proportional(12.0),
        config.text_color,
    );

    // Cancel button (right of progress bar)
    let cancel_rect = Rect::from_min_size(
        egui::pos2(
            rect.max.x - progress_bar_width - padding - 60.0,
            rect.center().y - 8.0,
        ),
        Vec2::new(50.0, 16.0),
    );

    let cancel_response = ui.allocate_rect(cancel_rect, Sense::click());

    let cancel_color = if cancel_response.hovered() {
        Color32::from_rgb(255, 100, 100)
    } else {
        Color32::from_rgb(150, 150, 150)
    };

    ui.painter().text(
        cancel_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Cancel",
        egui::FontId::proportional(11.0),
        cancel_color,
    );

    if cancel_response.clicked() {
        response.cancel_generation = true;
    }

    response
}

/// Render completed state (success message)
fn render_completed_state(ui: &mut Ui, rect: Rect, message: &str, _config: &StatusBarConfig) {
    let padding = 8.0;

    // Success icon + message
    let text_pos = egui::pos2(rect.min.x + padding, rect.center().y);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        message,
        egui::FontId::proportional(12.0),
        Color32::from_rgb(76, 175, 80), // Green for success
    );
}

/// Render idle state (no active progress)
fn render_idle_state(ui: &mut Ui, rect: Rect, _config: &StatusBarConfig) {
    let padding = 8.0;

    // Just show "Ready" or app version
    let text_pos = egui::pos2(rect.min.x + padding, rect.center().y);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        "Ready",
        egui::FontId::proportional(12.0),
        Color32::from_rgb(100, 100, 100),
    );
}

/// Simplified status bar that reads from ProxyProgress resource
///
/// Use this in docking.rs to render the status bar with proper resource access.
pub struct StatusBar {
    config: StatusBarConfig,
}

impl StatusBar {
    /// Create a new status bar with default config
    pub fn new() -> Self {
        Self {
            config: StatusBarConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: StatusBarConfig) -> Self {
        Self { config }
    }

    /// Render the status bar
    ///
    /// Call with data extracted from ProxyProgress resource.
    pub fn show(
        &self,
        ui: &mut Ui,
        message: &str,
        progress: Option<f32>,
        is_active: bool,
        recently_completed: bool,
    ) -> StatusBarResponse {
        status_bar_widget(
            ui,
            message,
            progress,
            is_active,
            recently_completed,
            &self.config,
        )
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bar_config_default() {
        let config = StatusBarConfig::default();
        assert_eq!(config.height, 24.0);
    }

    #[test]
    fn test_status_bar_creation() {
        let bar = StatusBar::new();
        assert_eq!(bar.config.height, 24.0);
    }

    #[test]
    fn test_response_default() {
        let response = StatusBarResponse::default();
        assert!(!response.cancel_generation);
    }
}
