//! # Transport Controls Widget
//!
//! Production-grade transport bar with play/pause, stop, scrubber, and timecode display.
//!
//! ## Architecture
//!
//! This widget is a **pure view** - it reads state and returns responses.
//! All mutations go through Commands, maintaining ECS purity.

use std::time::Duration;

/// Response from the transport widget
#[derive(Debug, Default)]
pub struct TransportResponse {
    /// User requested play/pause toggle
    pub toggle_play: bool,
    /// User requested stop (reset to beginning)
    pub stop: bool,
    /// User requested seek to a specific frame
    pub seek_to_frame: Option<u64>,
    /// User requested speed change
    pub set_speed: Option<f32>,
    /// User requested step forward one frame
    pub step_forward: bool,
    /// User requested step backward one frame
    pub step_backward: bool,
}

impl TransportResponse {
    /// Check if any action was requested
    pub fn has_action(&self) -> bool {
        self.toggle_play
            || self.stop
            || self.seek_to_frame.is_some()
            || self.set_speed.is_some()
            || self.step_forward
            || self.step_backward
    }
}

/// Transport controls widget with play/pause, stop, scrubber, and timecode
///
/// ## Usage
///
/// ```ignore
/// let response = TransportWidget::new(&playhead, frame_rate, total_duration)
///     .show(ui);
///
/// if response.toggle_play {
///     command_queue.submit(if playhead.is_playing {
///         Box::new(PauseCommand)
///     } else {
///         Box::new(PlayCommand)
///     });
/// }
/// ```
pub struct TransportWidget {
    /// Current time position
    current_time: Duration,
    /// Current frame number
    current_frame: u64,
    /// Total duration of the media
    total_duration: Duration,
    /// Total number of frames
    total_frames: u64,
    /// Whether playback is active
    is_playing: bool,
    /// Current playback speed
    playback_speed: f32,
    /// Frame rate for timecode display
    frame_rate: f64,
}

impl TransportWidget {
    /// Create a new transport widget from playback state
    pub fn new(
        current_time: Duration,
        current_frame: u64,
        total_duration: Duration,
        is_playing: bool,
        playback_speed: f32,
        frame_rate: f64,
    ) -> Self {
        let total_frames = (total_duration.as_secs_f64() * frame_rate) as u64;
        Self {
            current_time,
            current_frame,
            total_duration,
            total_frames,
            is_playing,
            playback_speed,
            frame_rate,
        }
    }

    /// Show the transport widget and return user interactions
    pub fn show(&self, ui: &mut egui::Ui) -> TransportResponse {
        let mut response = TransportResponse::default();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // Step backward button
            if ui.button("⏮").on_hover_text("Step backward (←)").clicked() {
                response.step_backward = true;
            }

            // Play/Pause button
            let play_text = if self.is_playing { "⏸" } else { "▶" };
            let play_hover = if self.is_playing {
                "Pause (Space)"
            } else {
                "Play (Space)"
            };
            if ui.button(play_text).on_hover_text(play_hover).clicked() {
                response.toggle_play = true;
            }

            // Stop button
            if ui.button("⏹").on_hover_text("Stop (Home)").clicked() {
                response.stop = true;
            }

            // Step forward button
            if ui.button("⏭").on_hover_text("Step forward (→)").clicked() {
                response.step_forward = true;
            }

            ui.separator();

            // Timecode display
            let timecode = self.format_timecode(self.current_time);
            ui.label(
                egui::RichText::new(&timecode)
                    .monospace()
                    .size(14.0)
                    .color(egui::Color32::from_rgb(200, 200, 200)),
            );

            ui.label("/");

            let total_timecode = self.format_timecode(self.total_duration);
            ui.label(
                egui::RichText::new(&total_timecode)
                    .monospace()
                    .size(14.0)
                    .color(egui::Color32::from_rgb(150, 150, 150)),
            );

            // Note: Scrubber removed - users should scrub on the timeline ruler instead

            // Speed dropdown
            ui.separator();
            self.show_speed_selector(ui, &mut response);
        });

        response
    }

    /// Format time as SMPTE timecode HH:MM:SS:FF
    fn format_timecode(&self, time: Duration) -> String {
        let total_secs = time.as_secs_f64();
        let hours = (total_secs / 3600.0).floor() as u32;
        let minutes = ((total_secs % 3600.0) / 60.0).floor() as u32;
        let seconds = (total_secs % 60.0).floor() as u32;

        // Calculate frame within the current second
        let frame_in_second = ((total_secs % 1.0) * self.frame_rate).floor() as u32;

        format!(
            "{:02}:{:02}:{:02}:{:02}",
            hours, minutes, seconds, frame_in_second
        )
    }

    /// Show the scrubber (seekable progress bar)
    /// Returns Some(progress) if user clicked/dragged to a new position
    fn show_scrubber(&self, ui: &mut egui::Ui, progress: f32, width: f32) -> Option<f32> {
        let height = 16.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());

        let mut new_progress = None;

        // Handle interaction
        if response.clicked() || response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let relative_x = (pos.x - rect.left()).clamp(0.0, width);
                new_progress = Some(relative_x / width);
            }
        }

        // Draw background
        let painter = ui.painter_at(rect);
        let rounding = 4.0;

        // Track background
        painter.rect_filled(rect, rounding, egui::Color32::from_rgb(40, 40, 45));

        // Progress fill
        let progress_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * progress.clamp(0.0, 1.0), height),
        );
        painter.rect_filled(
            progress_rect,
            rounding,
            egui::Color32::from_rgb(100, 149, 237), // Cornflower blue
        );

        // Playhead indicator
        let playhead_x = rect.left() + rect.width() * progress.clamp(0.0, 1.0);
        let playhead_rect = egui::Rect::from_center_size(
            egui::pos2(playhead_x, rect.center().y),
            egui::vec2(4.0, height + 4.0),
        );
        painter.rect_filled(playhead_rect, 2.0, egui::Color32::WHITE);

        // Hover effect
        if response.hovered() {
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 149, 237)),
            );
        }

        new_progress
    }

    /// Show speed selector dropdown
    fn show_speed_selector(&self, ui: &mut egui::Ui, response: &mut TransportResponse) {
        let speed_text = format!("{:.1}x", self.playback_speed);

        egui::ComboBox::from_id_salt("speed_selector")
            .selected_text(speed_text)
            .width(60.0)
            .show_ui(ui, |ui| {
                for &speed in &[0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
                    let label = format!("{:.2}x", speed);
                    if ui
                        .selectable_label((self.playback_speed - speed as f32).abs() < 0.01, label)
                        .clicked()
                    {
                        response.set_speed = Some(speed as f32);
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timecode_formatting() {
        let widget = TransportWidget::new(
            Duration::from_secs(3661), // 1h 1m 1s
            0,
            Duration::from_secs(7200),
            false,
            1.0,
            30.0,
        );

        let timecode = widget.format_timecode(Duration::from_secs(3661));
        assert_eq!(timecode, "01:01:01:00");
    }

    #[test]
    fn test_timecode_with_frames() {
        let widget = TransportWidget::new(
            Duration::from_secs_f64(1.5), // 1.5 seconds at 30fps = frame 15
            45,
            Duration::from_secs(60),
            false,
            1.0,
            30.0,
        );

        // 1.5 seconds = 1 second + 15 frames at 30fps
        let timecode = widget.format_timecode(Duration::from_secs_f64(1.5));
        assert_eq!(timecode, "00:00:01:15");
    }

    #[test]
    fn test_response_has_action() {
        let mut response = TransportResponse::default();
        assert!(!response.has_action());

        response.toggle_play = true;
        assert!(response.has_action());
    }
}
