//! # Media Browser Panel
//!
//! UI panel for browsing and managing project assets.
//!
//! ## Features
//!
//! - **Asset Grid**: Visual grid of imported media assets with thumbnails
//! - **Proxy Status**: Shows proxy generation progress per asset
//! - **Drag-to-Timeline**: Drag assets from browser to timeline
//! - **Context Menu**: Right-click for asset actions (delete, regenerate proxy)
//!
//! ## Architecture
//!
//! This panel reads from `AssetRegistry` (ECS Resource) and displays
//! assets in a scrollable grid. Actions dispatch Commands to the queue.

use egui::{Color32, Rect, RichText, Rounding, Sense, Ui, Vec2};
use strawberry_core::resources::{
    AssetRegistry, AssetType, AudioStatus, ProxyStatus, RegisteredAsset,
};

// Re-export Uuid from our dependencies
pub use uuid::Uuid;

/// View mode for the media browser
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaBrowserViewMode {
    /// Grid view with thumbnails
    #[default]
    Grid,
    /// List view with details
    List,
}

/// Sorting options for assets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AssetSortOrder {
    /// Sort by name A-Z
    #[default]
    NameAsc,
    /// Sort by name Z-A
    NameDesc,
    /// Most recently added first
    DateDesc,
    /// Oldest first
    DateAsc,
    /// By type (Video, Audio, Image)
    Type,
}

/// Response from the media browser panel
#[derive(Debug, Clone, Default)]
pub struct MediaBrowserResponse {
    /// Asset that was double-clicked (add to timeline at end)
    pub asset_to_add: Option<Uuid>,
    /// Asset that was Shift+double-clicked (add at playhead)
    pub add_at_playhead: Option<Uuid>,
    /// Asset that is being dragged
    pub dragging_asset: Option<Uuid>,
    /// Asset selected for context menu
    pub context_menu_asset: Option<Uuid>,
    /// Request to delete an asset
    pub delete_asset: Option<Uuid>,
    /// Request to regenerate proxy for an asset
    pub regenerate_proxy: Option<Uuid>,
    /// User clicked the Import button
    pub import_clicked: bool,
}

/// Media browser panel for asset management
#[derive(Clone)]
pub struct MediaBrowserPanel {
    /// Current view mode
    pub view_mode: MediaBrowserViewMode,
    /// Sorting order
    pub sort_order: AssetSortOrder,
    /// Search filter text
    pub search_filter: String,
    /// Thumbnail size in grid mode (pixels)
    pub thumbnail_size: f32,
    /// Currently selected asset (if any)
    pub selected_asset: Option<Uuid>,
}

impl Default for MediaBrowserPanel {
    fn default() -> Self {
        Self {
            view_mode: MediaBrowserViewMode::Grid,
            sort_order: AssetSortOrder::NameAsc,
            search_filter: String::new(),
            thumbnail_size: 100.0,
            selected_asset: None,
        }
    }
}

impl MediaBrowserPanel {
    /// Create a new media browser panel
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the media browser panel
    ///
    /// Returns a response with any actions the user performed.
    pub fn show(&mut self, ui: &mut Ui, registry: &AssetRegistry) -> MediaBrowserResponse {
        let mut response = MediaBrowserResponse::default();

        // Toolbar (includes Import button)
        response.import_clicked = self.show_toolbar(ui);
        ui.separator();

        // Main content area
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.view_mode {
                MediaBrowserViewMode::Grid => {
                    self.show_grid_view(ui, registry, &mut response);
                }
                MediaBrowserViewMode::List => {
                    self.show_list_view(ui, registry, &mut response);
                }
            });

        response
    }

    /// Show the toolbar with import button, search, view mode, and sort options
    ///
    /// Returns true if Import button was clicked.
    fn show_toolbar(&mut self, ui: &mut Ui) -> bool {
        let mut import_clicked = false;

        ui.horizontal(|ui| {
            // Import button (prominent, left side)
            if ui
                .add(egui::Button::new("➕ Import").fill(Color32::from_rgb(60, 80, 120)))
                .clicked()
            {
                import_clicked = true;
            }

            ui.separator();

            // Search box
            ui.add(
                egui::TextEdit::singleline(&mut self.search_filter)
                    .hint_text("🔍 Search assets...")
                    .desired_width(120.0),
            );

            ui.separator();

            // View mode toggle
            if ui
                .selectable_label(self.view_mode == MediaBrowserViewMode::Grid, "⊞")
                .on_hover_text("Grid View")
                .clicked()
            {
                self.view_mode = MediaBrowserViewMode::Grid;
            }
            if ui
                .selectable_label(self.view_mode == MediaBrowserViewMode::List, "☰")
                .on_hover_text("List View")
                .clicked()
            {
                self.view_mode = MediaBrowserViewMode::List;
            }

            // Thumbnail size slider (grid mode only)
            if self.view_mode == MediaBrowserViewMode::Grid {
                ui.separator();
                ui.add(egui::Slider::new(&mut self.thumbnail_size, 60.0..=200.0).show_value(false));
            }
        });

        import_clicked
    }

    /// Show assets in a grid layout with thumbnails
    fn show_grid_view(
        &mut self,
        ui: &mut Ui,
        registry: &AssetRegistry,
        response: &mut MediaBrowserResponse,
    ) {
        let assets = self.filter_and_sort_assets(registry);

        if assets.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("📁 No assets imported")
                        .size(16.0)
                        .color(Color32::GRAY),
                );
                ui.add_space(10.0);
                ui.label("Drag and drop media files here or use File > Import");
            });
            return;
        }

        // Calculate grid layout
        let available_width = ui.available_width();
        let item_width = self.thumbnail_size + 10.0; // Padding
        let columns = ((available_width / item_width).floor() as usize).max(1);

        egui::Grid::new("asset_grid")
            .num_columns(columns)
            .spacing([8.0, 12.0])
            .show(ui, |ui| {
                for (i, asset) in assets.iter().enumerate() {
                    self.show_asset_card(ui, asset, response);

                    if (i + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    /// Show a single asset card in grid view
    fn show_asset_card(
        &mut self,
        ui: &mut Ui,
        asset: &RegisteredAsset,
        response: &mut MediaBrowserResponse,
    ) {
        let size = Vec2::new(self.thumbnail_size, self.thumbnail_size + 25.0);
        let is_selected = self.selected_asset == Some(asset.id);

        // Allocate space
        let (rect, card_response) = ui.allocate_exact_size(size, Sense::click_and_drag());

        if !ui.is_rect_visible(rect) {
            return;
        }

        let painter = ui.painter();

        // Background
        let bg_color = if is_selected {
            Color32::from_rgb(45, 65, 95)
        } else if card_response.hovered() {
            Color32::from_rgb(35, 35, 40)
        } else {
            Color32::from_rgb(25, 25, 28)
        };

        painter.rect_filled(rect, Rounding::same(6.0), bg_color);

        // Thumbnail area
        let thumb_rect = Rect::from_min_size(
            rect.min + Vec2::new(5.0, 5.0),
            Vec2::new(self.thumbnail_size - 10.0, self.thumbnail_size - 10.0),
        );

        // Draw placeholder or asset type icon
        let icon = match asset.metadata.asset_type {
            AssetType::Video => "🎬",
            AssetType::Audio => "🔊",
            AssetType::Image => "🖼",
            AssetType::ImageSequence => "🎞",
        };
        painter.text(
            thumb_rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(32.0),
            Color32::WHITE,
        );

        // Proxy status indicator
        let status_color = match asset.proxy_status {
            ProxyStatus::Ready => Color32::from_rgb(76, 175, 80), // Green
            ProxyStatus::Generating { .. } => Color32::from_rgb(255, 193, 7), // Yellow
            ProxyStatus::Failed { .. } => Color32::from_rgb(244, 67, 54), // Red
            ProxyStatus::NotStarted => Color32::from_rgb(158, 158, 158), // Gray
            ProxyStatus::Disabled => Color32::from_rgb(100, 100, 100), // Dark gray
        };

        let indicator_rect = Rect::from_min_size(
            rect.min + Vec2::new(rect.width() - 14.0, 4.0),
            Vec2::new(10.0, 10.0),
        );
        painter.circle_filled(indicator_rect.center(), 5.0, status_color);

        // Readiness overlay — dim unready assets and show spinner
        let is_ready: bool = asset.is_ready_for_timeline();
        if !is_ready {
            // Semi-transparent dark overlay
            painter.rect_filled(
                thumb_rect,
                Rounding::same(4.0),
                Color32::from_rgba_premultiplied(0, 0, 0, 140),
            );

            // Animated spinner (rotating dots)
            let time: f64 = ui.input(|i| i.time);
            let angle: f64 = time * 3.0; // 3 rad/sec rotation
            let center = thumb_rect.center();
            for dot_index in 0..8u32 {
                let dot_angle: f64 = angle + (dot_index as f64) * std::f64::consts::TAU / 8.0;
                let radius: f32 = 12.0;
                let dot_pos = egui::pos2(
                    center.x + radius * dot_angle.cos() as f32,
                    center.y + radius * dot_angle.sin() as f32,
                );
                let alpha: u8 = (255.0 * (dot_index as f32 / 8.0)) as u8;
                painter.circle_filled(
                    dot_pos,
                    2.5,
                    Color32::from_rgba_premultiplied(255, 255, 255, alpha),
                );
            }

            // "Preparing..." text below spinner
            painter.text(
                egui::pos2(center.x, center.y + 22.0),
                egui::Align2::CENTER_CENTER,
                "Preparing...",
                egui::FontId::proportional(10.0),
                Color32::from_rgb(180, 180, 180),
            );

            // Request continuous repaint for animation
            ui.ctx().request_repaint();
        }

        // Asset name
        let name = asset
            .source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");

        let name_rect = Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 22.0),
            egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
        );

        // Truncate name if needed
        let truncated_name = if name.len() > 15 {
            format!("{}...", &name[..12])
        } else {
            name.to_string()
        };

        painter.text(
            name_rect.center(),
            egui::Align2::CENTER_CENTER,
            truncated_name,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );

        // Handle interactions — BLOCKED for unready assets
        if card_response.clicked() {
            self.selected_asset = Some(asset.id);
        }

        // Handle double-click with Shift modifier detection
        // Only allow adding to timeline if asset is fully ready
        if is_ready && card_response.double_clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            if modifiers.shift {
                response.add_at_playhead = Some(asset.id);
            } else {
                response.asset_to_add = Some(asset.id);
            }
        }

        if is_ready && card_response.dragged() {
            response.dragging_asset = Some(asset.id);
        }

        // Context menu with timeline actions
        card_response.context_menu(|ui| {
            // Timeline actions — gated on readiness
            let add_enabled: bool = is_ready;
            if ui
                .add_enabled(add_enabled, egui::Button::new("▶ Add to Timeline"))
                .clicked()
            {
                response.asset_to_add = Some(asset.id);
                ui.close_menu();
            }
            if ui
                .add_enabled(add_enabled, egui::Button::new("⏸ Add at Playhead"))
                .clicked()
            {
                response.add_at_playhead = Some(asset.id);
                ui.close_menu();
            }

            ui.separator();

            // Utility actions
            if ui.button("📋 Copy Path").clicked() {
                ui.output_mut(|o| o.copied_text = asset.source_path.to_string_lossy().to_string());
                ui.close_menu();
            }
            if ui.button("🔄 Regenerate Proxy").clicked() {
                response.regenerate_proxy = Some(asset.id);
                ui.close_menu();
            }

            ui.separator();

            // Destructive actions
            if ui.button("🗑 Delete").clicked() {
                response.delete_asset = Some(asset.id);
                ui.close_menu();
            }
        });

        // Tooltip with full path
        if card_response.hovered() {
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new(asset.id), |ui| {
                ui.label(format!("Path: {}", asset.source_path.display()));
                ui.label(format!("Type: {:?}", asset.metadata.asset_type));
                match &asset.proxy_status {
                    ProxyStatus::Ready => ui.label("Proxy: Ready ✓"),
                    ProxyStatus::Generating { progress } => {
                        ui.label(format!("Proxy: Generating... {:.0}%", progress * 100.0))
                    }
                    ProxyStatus::Failed { error } => ui.label(format!("Proxy: Failed - {}", error)),
                    ProxyStatus::NotStarted => ui.label("Proxy: Not started"),
                    ProxyStatus::Disabled => ui.label("Proxy: Disabled"),
                };
                // Audio status
                match &asset.audio_status {
                    AudioStatus::Ready => ui.label("Audio: Ready ✓"),
                    AudioStatus::Extracting => ui.label("Audio: Extracting..."),
                    AudioStatus::NoAudioStream => ui.label("Audio: No audio stream"),
                    AudioStatus::Failed { error } => ui.label(format!("Audio: Failed - {}", error)),
                    AudioStatus::NotStarted => ui.label("Audio: Pending"),
                };
                if !is_ready {
                    ui.label(
                        RichText::new("⏳ Asset not ready for timeline")
                            .color(Color32::from_rgb(255, 193, 7)),
                    );
                }
            });
        }
    }

    /// Show assets in a detailed list view
    fn show_list_view(
        &mut self,
        ui: &mut Ui,
        registry: &AssetRegistry,
        response: &mut MediaBrowserResponse,
    ) {
        let assets = self.filter_and_sort_assets(registry);

        if assets.is_empty() {
            ui.label("No assets imported");
            return;
        }

        egui::Grid::new("asset_list")
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                // Header
                ui.strong("Name");
                ui.strong("Type");
                ui.strong("Proxy");
                ui.strong("Path");
                ui.end_row();

                for asset in assets {
                    let name = asset
                        .source_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown");

                    // Name column (clickable)
                    let label = ui.selectable_label(self.selected_asset == Some(asset.id), name);
                    if label.clicked() {
                        self.selected_asset = Some(asset.id);
                    }
                    if label.double_clicked() {
                        response.asset_to_add = Some(asset.id);
                    }

                    // Type column
                    let type_str = match asset.metadata.asset_type {
                        AssetType::Video => "🎬 Video",
                        AssetType::Audio => "🔊 Audio",
                        AssetType::Image => "🖼 Image",
                        AssetType::ImageSequence => "🎞 Sequence",
                    };
                    ui.label(type_str);

                    // Proxy status
                    let (status_text, status_color) = match &asset.proxy_status {
                        ProxyStatus::Ready => ("✓ Ready", Color32::from_rgb(76, 175, 80)),
                        ProxyStatus::Generating { .. } => {
                            ("⏳ Generating", Color32::from_rgb(255, 193, 7))
                        }
                        ProxyStatus::Failed { .. } => ("✗ Failed", Color32::from_rgb(244, 67, 54)),
                        ProxyStatus::NotStarted => ("○ Pending", Color32::GRAY),
                        ProxyStatus::Disabled => ("-  Disabled", Color32::from_rgb(100, 100, 100)),
                    };
                    ui.colored_label(status_color, status_text);

                    // Path (truncated)
                    let path = asset.source_path.to_string_lossy();
                    let path_short = if path.len() > 40 {
                        format!("...{}", &path[path.len() - 37..])
                    } else {
                        path.to_string()
                    };
                    ui.label(path_short);

                    ui.end_row();
                }
            });
    }

    /// Filter and sort assets based on current settings
    fn filter_and_sort_assets<'a>(&self, registry: &'a AssetRegistry) -> Vec<&'a RegisteredAsset> {
        let mut assets: Vec<_> = registry
            .assets()
            .filter(|a| {
                if self.search_filter.is_empty() {
                    return true;
                }
                let name = a
                    .source_path
                    .file_name()
                    .and_then(|n: &std::ffi::OsStr| n.to_str())
                    .unwrap_or("");
                name.to_lowercase()
                    .contains(&self.search_filter.to_lowercase())
            })
            .collect();

        // Sort
        match self.sort_order {
            AssetSortOrder::NameAsc => {
                assets.sort_by(|a, b| {
                    let name_a = a.source_path.file_name();
                    let name_b = b.source_path.file_name();
                    name_a.cmp(&name_b)
                });
            }
            AssetSortOrder::NameDesc => {
                assets.sort_by(|a, b| {
                    let name_a = a.source_path.file_name();
                    let name_b = b.source_path.file_name();
                    name_b.cmp(&name_a)
                });
            }
            AssetSortOrder::Type => {
                assets.sort_by_key(|a| match a.metadata.asset_type {
                    AssetType::Video => 0,
                    AssetType::Audio => 1,
                    AssetType::Image => 2,
                    AssetType::ImageSequence => 3,
                });
            }
            _ => {} // DateAsc/DateDesc would need timestamp field
        }

        assets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_browser_defaults() {
        let panel = MediaBrowserPanel::new();
        assert_eq!(panel.view_mode, MediaBrowserViewMode::Grid);
        assert_eq!(panel.thumbnail_size, 100.0);
        assert!(panel.search_filter.is_empty());
    }
}
