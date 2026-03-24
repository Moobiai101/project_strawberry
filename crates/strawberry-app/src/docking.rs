//! # Docking System
//!
//! Provides a flexible, dockable panel layout using egui_tiles.

use bevy_ecs::prelude::*;
use egui_tiles::{TileId, Tiles, Tree};
use std::path::PathBuf;
use std::time::Duration;

use bevy_hierarchy::Children;
use strawberry_core::commands::{
    DeleteClipCommand, MoveClipCommand, PauseCommand, PlayCommand, RippleDeleteCommand,
    SeekCommand, SelectClipCommand, SetClipPropertyCommand, SetSpeedCommand, SetTrackLockedCommand,
    SetTrackMutedCommand, SetTrackSoloCommand, SplitClipCommand, StopCommand, TrimClipCommand,
    TrimSide,
};

use strawberry_core::components::timeline::{ClipLoopMode, ClipMetadata};
use strawberry_core::components::{ActiveTimeline, Clip, StableId, Timeline, Track, TrackType};
use strawberry_core::resources::{
    CommandQueue, Playhead, ProxyProgress, ScrubState, Selection, TimelineViewState, UserSettings,
    ViewportState,
};
use strawberry_ui::{
    panels::{MediaBrowserPanel, MediaBrowserResponse, SettingsPanel},
    ClipDragState, ClipInfo, FilmstripAssetTextures, StatusBar, TimelineTool, TimelineWidget,
    TrackInfo, TransportWidget, TrimDragState, TrimEdge, ViewportWidget,
};

// ============================================================================
// PANE TYPES
// ============================================================================

/// Types of panes that can be docked
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneType {
    Viewport,
    Timeline,
    Inspector,
    MediaBrowser,
}

impl PaneType {
    pub fn name(&self) -> &'static str {
        match self {
            PaneType::Viewport => "Viewport",
            PaneType::Timeline => "Timeline",
            PaneType::Inspector => "Inspector",
            PaneType::MediaBrowser => "Media Browser",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            PaneType::Viewport => "🎬",
            PaneType::Timeline => "📊",
            PaneType::Inspector => "⚙️",
            PaneType::MediaBrowser => "📁",
        }
    }
}

// ============================================================================
// DOCKING STATE
// ============================================================================

/// Docking state - holds the egui_tiles tree
pub struct DockingState {
    pub tree: Tree<PaneType>,
    /// Whether settings modal is open
    pub settings_open: bool,
    /// Settings panel state
    settings_panel: SettingsPanel,
}

impl Default for DockingState {
    fn default() -> Self {
        Self::new()
    }
}

impl DockingState {
    pub fn new() -> Self {
        Self {
            tree: create_default_layout(),
            settings_open: false,
            settings_panel: SettingsPanel::new(),
        }
    }

    /// Show the docking UI in a CentralPanel
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        world: &mut World,
        video_metadata: &VideoMetadataCache,
    ) -> DockingResponse {
        let mut response = DockingResponse::default();

        // Get proxy progress for status bar (read-only)
        let proxy_progress = world.get_resource::<ProxyProgress>();
        let (is_active, recently_completed, progress, message) =
            if let Some(progress) = proxy_progress {
                (
                    progress.is_active(),
                    progress.recently_completed(),
                    Some(progress.overall_progress()),
                    progress.status_message(),
                )
            } else {
                (false, false, None, String::new())
            };

        // Status bar at the bottom (with settings button)
        let status_bar = StatusBar::default();
        egui::TopBottomPanel::bottom("status_bar_panel")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Settings gear button on the left
                    if ui.button("⚙").on_hover_text("Settings").clicked() {
                        self.settings_open = true;
                    }
                    ui.separator();

                    // Status bar content fills remaining space
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            status_bar.show(ui, &message, progress, is_active, recently_completed);
                        });
                    });
                });
            });

        // =====================================================================
        // SETTINGS MODAL
        // =====================================================================
        if self.settings_open {
            let mut close_requested = false;

            egui::Window::new("⚙ Settings")
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    // Get mutable settings from world
                    if let Some(settings) = world.get_resource_mut::<UserSettings>() {
                        // Convert enums to u8 indices for the panel
                        let mut resolution_idx: u8 = match settings.proxy.resolution {
                            strawberry_core::resources::ProxyResolution::Res480p => 0,
                            strawberry_core::resources::ProxyResolution::Res720p => 1,
                            strawberry_core::resources::ProxyResolution::Res1080p => 2,
                            strawberry_core::resources::ProxyResolution::Original => 3,
                        };
                        let mut quality_idx: u8 = match settings.proxy.quality {
                            strawberry_core::resources::ProxyQuality::Low => 0,
                            strawberry_core::resources::ProxyQuality::Medium => 1,
                            strawberry_core::resources::ProxyQuality::High => 2,
                        };
                        let mut max_jobs: u8 = settings.proxy.max_concurrent_jobs;
                        let mut auto_generate: bool = settings.proxy.auto_generate;
                        let mut show_filmstrip: bool = settings.ui.show_filmstrip;
                        let mut show_waveforms: bool = settings.ui.show_waveforms;
                        let mut snap_to_playhead: bool = settings.ui.snap_to_playhead;
                        let mut snap_to_clips: bool = settings.ui.snap_to_clips;

                        let panel_response = self.settings_panel.show(
                            ui,
                            &mut resolution_idx,
                            &mut quality_idx,
                            &mut max_jobs,
                            &mut auto_generate,
                            &mut show_filmstrip,
                            &mut show_waveforms,
                            &mut snap_to_playhead,
                            &mut snap_to_clips,
                        );

                        // Write back changed values
                        let mut settings = world.resource_mut::<UserSettings>();
                        settings.proxy.resolution = match resolution_idx {
                            0 => strawberry_core::resources::ProxyResolution::Res480p,
                            1 => strawberry_core::resources::ProxyResolution::Res720p,
                            2 => strawberry_core::resources::ProxyResolution::Res1080p,
                            _ => strawberry_core::resources::ProxyResolution::Original,
                        };
                        settings.proxy.quality = match quality_idx {
                            0 => strawberry_core::resources::ProxyQuality::Low,
                            1 => strawberry_core::resources::ProxyQuality::Medium,
                            _ => strawberry_core::resources::ProxyQuality::High,
                        };
                        settings.proxy.max_concurrent_jobs = max_jobs;
                        settings.proxy.auto_generate = auto_generate;
                        settings.ui.show_filmstrip = show_filmstrip;
                        settings.ui.show_waveforms = show_waveforms;
                        settings.ui.snap_to_playhead = snap_to_playhead;
                        settings.ui.snap_to_clips = snap_to_clips;

                        // Save on close
                        if panel_response.close_requested {
                            close_requested = true;
                            if let Err(e) = settings.save() {
                                tracing::warn!("Failed to save settings: {}", e);
                            }
                        }
                    }
                });

            if close_requested {
                self.settings_open = false;
            }
        }

        let mut behavior = TreeBehavior {
            world,
            video_metadata,
            response: &mut response,
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            self.tree.ui(&mut behavior, ui);
        });

        response
    }
}

fn create_default_layout() -> Tree<PaneType> {
    let mut tiles = Tiles::default();

    let viewport = tiles.insert_pane(PaneType::Viewport);
    let timeline = tiles.insert_pane(PaneType::Timeline);
    let inspector = tiles.insert_pane(PaneType::Inspector);
    let media_browser = tiles.insert_pane(PaneType::MediaBrowser);

    // Tab the Media Browser with Inspector (bottom-right)
    let right_tabs = tiles.insert_tab_tile(vec![media_browser, inspector]);

    let bottom_row = tiles.insert_horizontal_tile(vec![timeline, right_tabs]);

    if let Some(egui_tiles::Tile::Container(container)) = tiles.get_mut(bottom_row) {
        if let egui_tiles::Container::Linear(linear) = container {
            linear.shares.set_share(timeline, 2.5);
            linear.shares.set_share(right_tabs, 1.0);
        }
    }

    let root = tiles.insert_vertical_tile(vec![viewport, bottom_row]);

    if let Some(egui_tiles::Tile::Container(container)) = tiles.get_mut(root) {
        if let egui_tiles::Container::Linear(linear) = container {
            linear.shares.set_share(viewport, 6.0);
            linear.shares.set_share(bottom_row, 4.0);
        }
    }

    Tree::new("strawberry_dock", root, tiles)
}

// ============================================================================
// VIDEO METADATA CACHE
// ============================================================================

#[derive(Default, Clone)]
pub struct VideoMetadataCache {
    pub path: Option<PathBuf>,
    pub resolution: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    pub duration: Option<Duration>,
    pub codec: Option<String>,
}

// ============================================================================
// DOCKING RESPONSE
// ============================================================================

#[derive(Default)]
pub struct DockingResponse {
    pub dropped_files: Vec<PathBuf>,
    /// Flag indicating user clicked Import button in MediaBrowser
    pub import_requested: bool,
}

// ============================================================================
// FILMSTRIP TEXTURE CACHE
// ============================================================================

/// Cache for filmstrip texture handles
///
/// Stores TextureHandle objects to keep GPU textures alive across frames.
/// Without this, textures are destroyed when handles are dropped.
#[derive(Default)]
struct FilmstripTextureCache {
    /// Map of texture_name -> TextureHandle
    /// We store handles to keep textures alive in GPU memory
    handles: std::collections::HashMap<String, egui::TextureHandle>,
    /// Access order for LRU eviction (texture_name -> last access timestamp)
    access_order: std::collections::HashMap<String, std::time::Instant>,
}

impl FilmstripTextureCache {
    /// Get the number of cached textures
    fn len(&self) -> usize {
        self.handles.len()
    }

    /// Mark a texture as recently accessed
    fn touch(&mut self, name: &str) {
        self.access_order
            .insert(name.to_string(), std::time::Instant::now());
    }

    /// Evict least recently used textures, keeping only those in `keep_assets`.
    ///
    /// This is called when the cache exceeds `max_cached` textures.
    /// Textures for visible clips (in `keep_assets`) are never evicted.
    ///
    /// ## Parameters
    ///
    /// - `keep_asset_uuids`: Asset UUIDs that should NOT be evicted (currently visible)
    /// - `max_cached`: Maximum textures to keep after eviction
    fn evict_lru_except(
        &mut self,
        keep_asset_uuids: &std::collections::HashSet<uuid::Uuid>,
        max_cached: usize,
    ) {
        if self.handles.len() <= max_cached {
            return;
        }

        // Build set of texture names to keep (those belonging to visible assets)
        let keep_prefixes: Vec<String> = keep_asset_uuids
            .iter()
            .map(|uuid| format!("filmstrip_{}", uuid))
            .collect();

        // Collect evictable textures sorted by access time (oldest first)
        let mut evictable: Vec<(String, std::time::Instant)> = self
            .access_order
            .iter()
            .filter(|(name, _)| !keep_prefixes.iter().any(|prefix| name.starts_with(prefix)))
            .map(|(name, time)| (name.clone(), *time))
            .collect();

        evictable.sort_by_key(|(_, time)| *time);

        // Evict oldest textures until we're under max_cached
        let num_to_evict = self.handles.len().saturating_sub(max_cached);
        for (name, _) in evictable.into_iter().take(num_to_evict) {
            self.handles.remove(&name);
            self.access_order.remove(&name);
            tracing::trace!("Evicted filmstrip texture: {}", name);
        }
    }
}

impl Clone for FilmstripTextureCache {
    fn clone(&self) -> Self {
        // Clone by reference - handles are Arc internally
        Self {
            handles: self.handles.clone(),
            access_order: self.access_order.clone(),
        }
    }
}

// ============================================================================
// TREE BEHAVIOR
// ============================================================================

struct TreeBehavior<'a> {
    world: &'a mut World,
    video_metadata: &'a VideoMetadataCache,
    response: &'a mut DockingResponse,
}

impl<'a> egui_tiles::Behavior<PaneType> for TreeBehavior<'a> {
    fn tab_title_for_pane(&mut self, pane: &PaneType) -> egui::WidgetText {
        format!("{} {}", pane.icon(), pane.name()).into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        pane: &mut PaneType,
    ) -> egui_tiles::UiResponse {
        match pane {
            PaneType::Viewport => self.render_viewport(ui),
            PaneType::Timeline => self.render_timeline(ui),
            PaneType::Inspector => self.render_inspector(ui),
            PaneType::MediaBrowser => self.render_media_browser(ui),
        }

        egui_tiles::UiResponse::None
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        28.0
    }
}

impl<'a> TreeBehavior<'a> {
    fn render_viewport(&mut self, ui: &mut egui::Ui) {
        // Check for dropped files
        let dropped_files: Vec<PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });

        if !dropped_files.is_empty() {
            self.response.dropped_files = dropped_files;
        }

        // Visual feedback for drag-and-drop
        let is_dragging = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

        if is_dragging {
            let rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(100, 150, 255, 50),
            );
            ui.painter().rect_stroke(
                rect.shrink(4.0),
                8.0,
                egui::Stroke::new(3.0, egui::Color32::from_rgb(100, 150, 255)),
            );
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Drop video file here")
                        .size(24.0)
                        .color(egui::Color32::WHITE),
                );
            });
        } else {
            // Get viewport state and project settings for resolution
            let viewport_state = self.world.resource::<ViewportState>();
            let project_settings = self
                .world
                .resource::<strawberry_core::resources::ProjectSettings>();

            // Use PROJECT resolution for the canvas (not native video resolution).
            // This is what enables Clipchamp-style instant aspect ratio switching.
            let project_resolution: glam::UVec2 = project_settings.project_resolution;
            let active_preset: strawberry_core::resources::AspectRatioPreset =
                project_settings.active_preset;

            // ================================================================
            // ASPECT RATIO TOOLBAR (Clipchamp-style "Size" dropdown)
            // ================================================================
            ui.horizontal(|ui| {
                let dropdown = strawberry_ui::AspectRatioDropdown::new(active_preset);
                let dropdown_response = dropdown.show(ui);

                if let Some(new_preset) = dropdown_response.selected_preset {
                    let command_queue = self.world.resource::<CommandQueue>();
                    command_queue.submit(Box::new(
                        strawberry_core::commands::SetProjectResolutionCommand::new(new_preset),
                    ));
                }

                // Show current resolution as info
                ui.label(
                    egui::RichText::new(format!(
                        "{}×{}",
                        project_resolution.x, project_resolution.y
                    ))
                    .size(11.0)
                    .color(egui::Color32::from_rgb(140, 140, 155)),
                );
            });

            // ================================================================
            // VIEWPORT RENDERING + GPU QUAD PREVIEW
            //
            // ViewportWidget ALWAYS gets the texture (prevents placeholder).
            // During an active gizmo drag, we paint a transformed GPU quad
            // ON TOP of the widget to show the live preview of the transform.
            // ================================================================

            let mut widget = ViewportWidget::new()
                .with_resolution(project_resolution.x, project_resolution.y)
                .with_zoom(viewport_state.zoom)
                .with_pan(viewport_state.pan)
                .with_loading(viewport_state.is_loading)
                .with_in_gap(viewport_state.in_gap);

            // Always give texture to widget (prevents "drop video here" placeholder)
            let texture_id: Option<egui::TextureId> = viewport_state.current_texture;
            let is_normal_display: bool = !viewport_state.is_loading
                && viewport_state.error.is_none()
                && !viewport_state.in_gap
                && texture_id.is_some();

            if let Some(tid) = texture_id {
                widget = widget.with_texture(tid);
            }

            // Skip widget rendering the texture when we are painting the quad
            widget = widget.with_skip_texture(is_normal_display);

            if let Some(err) = &viewport_state.error {
                widget = widget.with_error(Some(err.clone()));
            }

            // Cache display params using PROJECT resolution for gizmo + quad rendering
            let canvas_size: glam::Vec2 =
                glam::Vec2::new(project_resolution.x as f32, project_resolution.y as f32);
            let zoom: f32 = viewport_state.zoom;
            let pan: glam::Vec2 = viewport_state.pan;

            // Compute display_rect using the same logic as ViewportWidget.
            // This is the aspect-fitted area WHERE the video actually renders,
            // not the full response.rect (which includes letterbox areas).
            let widget_for_rect = ViewportWidget::new()
                .with_resolution(project_resolution.x, project_resolution.y)
                .with_zoom(viewport_state.zoom)
                .with_pan(viewport_state.pan);

            // Render viewport widget — this allocates the response + painter
            let response: egui::Response = ui.add(widget);

            let display_rect: egui::Rect = widget_for_rect.calculate_display_rect(response.rect);

            // ================================================================
            // LIVE PREVIEW: Paint per-clip transformed quads
            // Renders each visible clip with its own Transform2D
            // ================================================================
            self.render_all_clip_layers(ui, display_rect, canvas_size, zoom, pan);

            // ================================================================
            // GIZMO INTEGRATION — overlay + input handler
            // ================================================================
            self.render_gizmo_overlay(ui, &response, display_rect, canvas_size, zoom, pan);
        }
    }

    /// Paint all visible clip layers as GPU-accelerated transformed quads.
    ///
    /// Delegates to `viewport_renderer::render_all_clip_layers()`.
    fn render_all_clip_layers(
        &self,
        ui: &egui::Ui,
        display_rect: egui::Rect,
        canvas_size: glam::Vec2,
        zoom: f32,
        pan: glam::Vec2,
    ) {
        crate::viewport_renderer::render_all_clip_layers(
            &self.world,
            ui,
            display_rect,
            canvas_size,
            zoom,
            pan,
        );
    }

    /// Render gizmo overlay on top of the viewport and process gizmo input.
    ///
    /// Delegates to `viewport_renderer::render_gizmo_overlay()`.
    fn render_gizmo_overlay(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        display_rect: egui::Rect,
        canvas_size: glam::Vec2,
        zoom: f32,
        pan: glam::Vec2,
    ) {
        crate::viewport_renderer::render_gizmo_overlay(
            &mut self.world,
            ui,
            response,
            display_rect,
            canvas_size,
            zoom,
            pan,
        );
    }

    fn render_timeline(&mut self, ui: &mut egui::Ui) {
        let (current_time, current_frame, is_playing, playback_speed) = {
            let playhead = self.world.resource::<Playhead>();
            (
                playhead.current_time,
                playhead.current_frame,
                playhead.is_playing,
                playhead.playback_speed,
            )
        };

        let total_duration = self
            .video_metadata
            .duration
            .unwrap_or(Duration::from_secs(0));
        let frame_rate = self.video_metadata.frame_rate.unwrap_or(30.0);

        // Transport controls
        let transport = TransportWidget::new(
            current_time,
            current_frame,
            total_duration,
            is_playing,
            playback_speed,
            frame_rate,
        );
        let transport_response = transport.show(ui);

        // Pre-update viewport BEFORE filmstrip loading to avoid one-frame lag.
        // Without this, load_filmstrip_textures() uses stale viewport data from
        // the previous frame, which can exclude all clips and return empty textures.
        // Must happen before command_queue borrow to avoid borrow conflict.
        self.pre_update_timeline_viewport(ui.available_width());

        let command_queue = self.world.resource::<CommandQueue>();

        if transport_response.toggle_play {
            if is_playing {
                command_queue.submit(Box::new(PauseCommand));
            } else {
                command_queue.submit(Box::new(PlayCommand));
            }
        }

        if transport_response.stop {
            command_queue.submit(Box::new(StopCommand::with_frame_rate(frame_rate)));
        }

        if let Some(frame) = transport_response.seek_to_frame {
            command_queue.submit(Box::new(SeekCommand::new(frame, frame_rate)));
        }

        if let Some(speed) = transport_response.set_speed {
            command_queue.submit(Box::new(SetSpeedCommand::new(speed)));
        }

        if transport_response.step_forward {
            let target = current_frame.saturating_add(1);
            command_queue.submit(Box::new(SeekCommand::new(target, frame_rate)));
        }

        if transport_response.step_backward {
            let target = current_frame.saturating_sub(1);
            command_queue.submit(Box::new(SeekCommand::new(target, frame_rate)));
        }

        ui.separator();

        // Retrieve persisted drag/trim states from egui memory
        let drag_state: Option<ClipDragState> = ui.memory(|mem| {
            mem.data
                .get_temp::<ClipDragState>(egui::Id::new("timeline_drag_state"))
        });
        let trim_state: Option<TrimDragState> = ui.memory(|mem| {
            mem.data
                .get_temp::<TrimDragState>(egui::Id::new("timeline_trim_state"))
        });

        // Retrieve persisted tool state from egui memory
        let active_tool: TimelineTool = ui.memory(|mem| {
            mem.data
                .get_temp::<TimelineTool>(egui::Id::new("timeline_active_tool"))
                .unwrap_or_default()
        });

        // Build timeline clips ONCE for both filmstrip loading and widget building.
        // Previously this was called twice per frame — once here and once inside
        // build_timeline_widget() — duplicating ECS iteration over all entities.
        let clips = self.get_timeline_clips(
            self.world
                .get_resource::<ActiveTimeline>()
                .and_then(|a| a.get()),
        );

        // Build the timeline widget, reusing pre-computed clips
        let mut timeline_widget = self
            .build_timeline_widget_with_clips(frame_rate, clips.clone())
            .with_drag_state(drag_state)
            .with_trim_state(trim_state)
            .with_active_tool(active_tool);

        // Load filmstrip textures for visible clips (if thumbnails exist on disk)
        // Uses persistent cache to avoid per-frame disk I/O
        let filmstrip_textures = self.load_filmstrip_textures(ui.ctx(), &clips);
        if !filmstrip_textures.is_empty() {
            timeline_widget = timeline_widget.with_filmstrip_textures(filmstrip_textures);
        }

        // Attach waveform peak cache for audio waveform rendering.
        // Clone is cheap — WaveformCache wraps HashMap<Uuid, Arc<WaveformPeaks>>.
        if self
            .world
            .contains_resource::<strawberry_render::AudioMixerResource>()
        {
            let mixer = self
                .world
                .resource::<strawberry_render::AudioMixerResource>();
            let cache = mixer.0.waveform_cache().clone();
            timeline_widget = timeline_widget.with_waveform_cache(cache);
        }

        let timeline_response = timeline_widget.show(ui);

        // Handle timeline widget interactions
        if let Some(seek_time) = timeline_response.seek_to {
            // Convert Duration to frame and submit SeekCommand
            let seek_frame = (seek_time.as_secs_f64() * frame_rate).round() as u64;
            command_queue.submit(Box::new(SeekCommand::new(seek_frame, frame_rate)));
        }

        // Handle clip selection
        if let Some(entity_bits) = timeline_response.clicked_clip {
            tracing::info!(
                "SELECTION: clicked_clip received, entity_bits={}",
                entity_bits
            );
            // Check if Ctrl is held for multi-select
            let modifiers = ui.input(|i| i.modifiers);
            let exclusive = !modifiers.ctrl;

            if exclusive {
                command_queue.submit(Box::new(SelectClipCommand::exclusive(entity_bits)));
            } else {
                command_queue.submit(Box::new(SelectClipCommand::additive(entity_bits)));
            }
        }

        // Handle right-click context menu on clip
        if let Some((entity_bits, click_pos)) = timeline_response.context_menu_clip {
            // Store the right-clicked clip and position in memory for persistent menu
            ui.memory_mut(|mem| {
                mem.data.insert_temp(
                    egui::Id::new("clip_context_menu_state"),
                    (entity_bits, click_pos),
                );
            });
        }

        // Check if we should show the context menu
        let menu_state: Option<(u64, egui::Pos2)> = ui.memory(|mem| {
            mem.data
                .get_temp::<(u64, egui::Pos2)>(egui::Id::new("clip_context_menu_state"))
        });

        if let Some((entity_bits, click_pos)) = menu_state {
            let popup_id = egui::Id::new("clip_context_menu_popup");

            // Show a popup at the click position
            let response = egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(click_pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.set_min_width(140.0);

                            if ui.button("✂ Split at Playhead").clicked() {
                                let playhead_time = self.world.resource::<Playhead>().current_time;
                                let clip_entity = bevy_ecs::entity::Entity::from_bits(entity_bits);

                                command_queue.submit(Box::new(SplitClipCommand::new(
                                    clip_entity,
                                    playhead_time,
                                )));

                                // Close menu
                                ui.memory_mut(|mem| {
                                    mem.data.remove::<(u64, egui::Pos2)>(egui::Id::new(
                                        "clip_context_menu_state",
                                    ));
                                });
                            }

                            ui.separator();

                            // Add Ripple Delete button
                            if ui.button("🌊 Ripple Delete (Alt+Del)").clicked() {
                                let clip_entity = bevy_ecs::entity::Entity::from_bits(entity_bits);
                                command_queue
                                    .submit(Box::new(RippleDeleteCommand::new(clip_entity)));
                                tracing::info!(
                                    "Ripple delete triggered from menu for {:?}",
                                    clip_entity
                                );

                                // Close menu
                                ui.memory_mut(|mem| {
                                    mem.data.remove::<(u64, egui::Pos2)>(egui::Id::new(
                                        "clip_context_menu_state",
                                    ));
                                });
                            }

                            ui.separator();

                            if ui.button("🗑 Delete (Del)").clicked() {
                                let clip_entity = bevy_ecs::entity::Entity::from_bits(entity_bits);
                                command_queue.submit(Box::new(DeleteClipCommand::new(clip_entity)));

                                // Close menu
                                ui.memory_mut(|mem| {
                                    mem.data.remove::<(u64, egui::Pos2)>(egui::Id::new(
                                        "clip_context_menu_state",
                                    ));
                                });
                            }
                        });
                });

            // Close menu if clicked outside
            if ui.input(|i| i.pointer.any_click())
                && !response
                    .response
                    .rect
                    .contains(ui.input(|i| i.pointer.latest_pos().unwrap_or_default()))
            {
                ui.memory_mut(|mem| {
                    mem.data
                        .remove::<(u64, egui::Pos2)>(egui::Id::new("clip_context_menu_state"));
                });
            }
        }

        // Handle track button toggles
        if let Some((track_entity, current_muted)) = timeline_response.toggle_track_mute {
            // Toggle mute: set to opposite of current state
            command_queue.submit(Box::new(SetTrackMutedCommand::new(
                track_entity,
                !current_muted,
            )));
        }

        if let Some((track_entity, current_solo)) = timeline_response.toggle_track_solo {
            command_queue.submit(Box::new(SetTrackSoloCommand::new(
                track_entity,
                !current_solo,
            )));
        }

        if let Some((track_entity, current_locked)) = timeline_response.toggle_track_lock {
            command_queue.submit(Box::new(SetTrackLockedCommand::new(
                track_entity,
                !current_locked,
            )));
        }

        // Handle clip audio mute toggle from 🔊/🔇 icon click
        if let Some((clip_entity_bits, current_muted)) = timeline_response.toggle_clip_mute {
            let entity = bevy_ecs::entity::Entity::from_bits(clip_entity_bits);
            if let Some(stable_id) = self.world.get::<StableId>(entity) {
                let clip_uuid: uuid::Uuid = stable_id.uuid();
                command_queue.submit(Box::new(
                    strawberry_core::commands::SetClipAudioCommand::set_muted(
                        clip_uuid,
                        !current_muted,
                    ),
                ));
            }
        }

        // Handle clip move from drag-and-drop
        if let Some((clip_entity, target_track_entity, new_time_seconds)) =
            timeline_response.move_clip
        {
            command_queue.submit(Box::new(MoveClipCommand::from_bits(
                clip_entity,
                target_track_entity,
                new_time_seconds,
            )));
        }

        // Handle clip trim from drag
        if let Some((clip_entity_bits, edge, new_value)) = timeline_response.trim_clip {
            // Convert TrimEdge to TrimSide
            let side = match edge {
                TrimEdge::Left => TrimSide::In,
                TrimEdge::Right => TrimSide::Out,
            };

            // For Trim Out (right edge), new_value is the new duration
            // We need to calculate the new source_out from the clip
            let final_value = if side == TrimSide::Out {
                // Get the original clip's source_in to calculate new source_out
                let clip_entity = bevy_ecs::entity::Entity::from_bits(clip_entity_bits);
                if let Some(clip) = self.world.get::<Clip>(clip_entity) {
                    // new_source_out = source_in + new_duration
                    clip.source_in.as_secs_f64() + new_value
                } else {
                    new_value
                }
            } else {
                new_value
            };

            command_queue.submit(Box::new(TrimClipCommand::from_bits(
                clip_entity_bits,
                side,
                final_value,
            )));
        }

        // Store drag state in egui memory for persistence across frames
        // (This is the proper egui pattern for stateful widgets)
        if let Some(drag_state) = timeline_response.updated_drag_state {
            ui.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("timeline_drag_state"), drag_state);
            });
        } else if timeline_response.move_clip.is_some() {
            // Clear drag state after move completes
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<ClipDragState>(egui::Id::new("timeline_drag_state"));
            });
        } else {
            // No active drag state AND no move command — ensure stale state
            // is cleaned up (e.g. drag was cancelled or was a no-op).
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<ClipDragState>(egui::Id::new("timeline_drag_state"));
            });
        }

        // Store trim state in egui memory for persistence across frames
        if let Some(trim_state) = timeline_response.updated_trim_state {
            ui.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("timeline_trim_state"), trim_state);
            });
        } else {
            // No active trim state — always clear from memory.
            // Previously this was `else if trim_clip.is_some()` which left
            // stale ghost state when the trim was a no-op or cancelled.
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<TrimDragState>(egui::Id::new("timeline_trim_state"));
            });
        }

        // Handle zoom changes from Ctrl+scroll
        if let Some(new_zoom) = timeline_response.zoom_changed {
            if let Some(mut view_state) = self.world.get_resource_mut::<TimelineViewState>() {
                view_state.set_zoom(new_zoom);
            }
        }

        // Handle scroll changes from user drag/scroll
        if let Some((scroll_x, scroll_y)) = timeline_response.scroll_changed {
            if let Some(mut view_state) = self.world.get_resource_mut::<TimelineViewState>() {
                // scroll_x from widget is in pixels, convert to seconds by dividing by zoom
                // Note: scroll_x is the NEW scroll position, not a delta
                let scroll_seconds = (scroll_x / view_state.zoom) as f64;
                view_state.scroll_offset = scroll_seconds.max(0.0);
                view_state.scroll_y = scroll_y;
                view_state.user_scrolling = true; // Mark user is scrolling
            }
        } else {
            // User stopped scrolling
            if let Some(mut view_state) = self.world.get_resource_mut::<TimelineViewState>() {
                view_state.user_scrolling = false;
            }
        }

        // Auto-follow playhead during playback (Centered mode)
        let is_playing = self.world.resource::<Playhead>().is_playing;
        if is_playing {
            let playhead_time = self.world.resource::<Playhead>().current_time.as_secs_f64();
            // Use the actual timeline widget width for follow calculation
            let visible_width: f32 = timeline_response.response.rect.width();
            if let Some(mut view_state) = self.world.get_resource_mut::<TimelineViewState>() {
                view_state.follow_playhead(playhead_time, visible_width);
            }
        }

        // =====================================================================
        // UPDATE TIMELINE VIEWPORT (post-show, with accurate rect width)
        // =====================================================================
        // Refines the viewport with the actual rendered rect width from
        // timeline_response. The pre-update (before show) used ui.available_width()
        // which is approximate; this post-update stores the precise value for
        // track count and accurate culling on the next frame.
        {
            let view_state = self.world.resource::<TimelineViewState>();
            let zoom: f32 = view_state.zoom;
            let scroll_offset_secs: f64 = view_state.scroll_offset;

            // Get track count from timeline
            let timeline_entity = self
                .world
                .get_resource::<ActiveTimeline>()
                .and_then(|a| a.get());
            let track_count: u32 = self.get_timeline_tracks(timeline_entity).len() as u32;

            // Use actual rendered widget width (not hardcoded)
            let visible_width_px: f32 = timeline_response.response.rect.width();

            // Calculate scroll position in pixels (from seconds)
            let scroll_offset_px: f32 = (scroll_offset_secs * zoom as f64) as f32;

            // Update TimelineViewport resource with accurate dimensions
            if let Some(mut viewport) = self
                .world
                .get_resource_mut::<strawberry_core::resources::TimelineViewport>()
            {
                viewport.update_from_view_state(scroll_offset_px, zoom, visible_width_px);
                viewport.visible_track_start = 0;
                viewport.visible_track_end = track_count;
            }
        }

        // Handle tool switching via keyboard (V, B, Y, U)
        if let Some(new_tool) = timeline_response.tool_changed {
            ui.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("timeline_active_tool"), new_tool);
            });
            tracing::debug!("Tool switched to: {:?}", new_tool);
        }

        // Handle razor cut from timeline click
        if let Some((_timeline_bits, cut_time)) = timeline_response.razor_cut {
            let playhead_time = Duration::from_secs_f64(cut_time);

            // Get active timeline entity first
            let timeline_entity = self
                .world
                .get_resource::<ActiveTimeline>()
                .and_then(|a| a.get());

            // Find all clips that contain this time and split them
            let clips_at_time: Vec<Entity> = self
                .get_timeline_clips(timeline_entity)
                .iter()
                .filter_map(|clip| {
                    if clip.timeline_start.as_secs_f64() < cut_time
                        && (clip.timeline_start + clip.duration).as_secs_f64() > cut_time
                    {
                        Some(Entity::from_bits(clip.entity_bits))
                    } else {
                        None
                    }
                })
                .collect();

            // Get command queue and submit split commands
            let command_queue = self.world.resource::<CommandQueue>();
            for clip_entity in clips_at_time {
                command_queue.submit(Box::new(SplitClipCommand::new(clip_entity, playhead_time)));
                tracing::info!("Razor cut clip at {:?}", playhead_time);
            }
        }

        // Handle ripple delete from Shift+Delete
        if let Some(clip_entity_bits) = timeline_response.ripple_delete {
            tracing::debug!(
                "Docking: Received ripple_delete request from timeline widget for {}",
                clip_entity_bits
            );
            let clip_entity = Entity::from_bits(clip_entity_bits);
            let command_queue = self.world.resource::<CommandQueue>();
            command_queue.submit(Box::new(RippleDeleteCommand::new(clip_entity)));
            tracing::info!(
                "Docking: Submitted RippleDeleteCommand for {:?}",
                clip_entity
            );
        }

        // Update ScrubState for compositor optimization - MUST be at end
        // to avoid borrow conflicts with command_queue references above
        {
            let mut scrub_state = self.world.resource_mut::<ScrubState>();

            if timeline_response.is_scrubbing {
                if !scrub_state.is_scrubbing {
                    tracing::trace!("Docking: Starting scrub mode");
                    scrub_state.start_scrub();
                }
            } else {
                if scrub_state.is_scrubbing {
                    tracing::trace!("Docking: Ending scrub mode");
                    scrub_state.end_scrub();
                }
            }
        }
    }

    /// Build TimelineWidget from ECS state
    #[allow(dead_code)] // Backward-compat wrapper; main path uses build_timeline_widget_with_clips
    fn build_timeline_widget(&self, frame_rate: f64) -> TimelineWidget {
        let timeline_entity = self
            .world
            .get_resource::<ActiveTimeline>()
            .and_then(|a| a.get());
        let clips = self.get_timeline_clips(timeline_entity);
        self.build_timeline_widget_with_clips(frame_rate, clips)
    }

    /// Build TimelineWidget from ECS state with pre-computed clips.
    ///
    /// This avoids the double `get_timeline_clips()` call that was causing
    /// redundant ECS iteration every frame. The caller computes clips once
    /// and passes them here and to `load_filmstrip_textures()`.
    fn build_timeline_widget_with_clips(
        &self,
        frame_rate: f64,
        clips: Vec<ClipInfo>,
    ) -> TimelineWidget {
        // Get active timeline
        let active_timeline = self.world.get_resource::<ActiveTimeline>();
        let timeline_entity = active_timeline.and_then(|a| a.get());

        let (timeline_name, duration) = if let Some(entity) = timeline_entity {
            let clock_duration: Duration = self
                .world
                .get_resource::<strawberry_core::systems::PlaybackClock>()
                .map(|clock| clock.timeline_duration)
                .unwrap_or(Duration::from_secs(60));

            self.world
                .get::<Timeline>(entity)
                .map(|t| (t.name.clone(), clock_duration))
                .unwrap_or(("No Timeline".to_string(), clock_duration))
        } else {
            ("No Timeline".to_string(), Duration::from_secs(60))
        };

        // Build track info
        let tracks = self.get_timeline_tracks(timeline_entity);

        // Get playhead
        let playhead = self.world.resource::<Playhead>();

        // Get timeline view state (zoom, scroll, follow mode)
        let view_state = self.world.resource::<TimelineViewState>();

        TimelineWidget::new()
            .with_name(timeline_name)
            .with_frame_rate(frame_rate)
            .with_duration(duration)
            .with_tracks(tracks)
            .with_clips(clips)
            .with_playhead(playhead.current_time)
            .with_playing(playhead.is_playing)
            .with_zoom(view_state.zoom)
            .with_scroll(
                view_state.scroll_offset as f32 * view_state.zoom,
                view_state.scroll_y,
            )
    }

    /// Get track info from ECS
    fn get_timeline_tracks(&self, timeline_entity: Option<Entity>) -> Vec<TrackInfo> {
        let mut tracks = Vec::new();

        if let Some(timeline) = timeline_entity {
            // Get children of timeline (tracks)
            if let Some(children) = self.world.get::<Children>(timeline) {
                for &track_entity in children.iter() {
                    if let Some(track) = self.world.get::<Track>(track_entity) {
                        tracks.push(TrackInfo {
                            entity_bits: track_entity.to_bits(),
                            index: track.index,
                            name: track.name.clone(),
                            is_video: matches!(track.track_type, TrackType::Video),
                            muted: track.muted,
                            solo: track.solo,
                            locked: track.locked,
                            height: if matches!(track.track_type, TrackType::Audio) {
                                40.0
                            } else {
                                60.0
                            },
                        });
                    }
                }
            }
        }

        // Sort by index
        tracks.sort_by_key(|t| t.index);
        tracks
    }

    /// Get clip info from ECS
    fn get_timeline_clips(&self, timeline_entity: Option<Entity>) -> Vec<ClipInfo> {
        let mut clips = Vec::new();

        // Get selection resource for checking selected state
        let selection = self.world.get_resource::<Selection>();

        if let Some(timeline) = timeline_entity {
            // Get children of timeline (tracks)
            if let Some(track_children) = self.world.get::<Children>(timeline) {
                for &track_entity in track_children.iter() {
                    let track = match self.world.get::<Track>(track_entity) {
                        Some(t) => t,
                        None => continue,
                    };

                    // Get children of track (clips)
                    if let Some(clip_children) = self.world.get::<Children>(track_entity) {
                        for &clip_entity in clip_children.iter() {
                            if let Some(clip) = self.world.get::<Clip>(clip_entity) {
                                let entity_bits = clip_entity.to_bits();
                                let is_selected = selection
                                    .map(|s| s.is_selected(entity_bits))
                                    .unwrap_or(false);

                                // Try to get asset UUID from ClipAssetRef component (if present)
                                let asset_uuid = self
                                    .world
                                    .get::<strawberry_core::node::timeline::ClipAssetRef>(
                                        clip_entity,
                                    )
                                    .map(|asset_ref| asset_ref.asset_uuid);

                                // Calculate speed-adjusted duration for timeline display.
                                // At speed > 1.0, the clip visually shrinks on the timeline.
                                let raw_duration: Duration = clip.duration();
                                let effective_duration: Duration = self
                                    .world
                                    .get::<ClipMetadata>(clip_entity)
                                    .map(|meta| {
                                        let speed: f64 = meta.speed as f64;
                                        if speed <= 0.0
                                            || (speed - 1.0).abs() < f64::EPSILON
                                            || meta.loop_mode == ClipLoopMode::Loop
                                            || meta.loop_mode == ClipLoopMode::PingPong
                                        {
                                            raw_duration
                                        } else {
                                            Duration::from_secs_f64(
                                                raw_duration.as_secs_f64() / speed,
                                            )
                                        }
                                    })
                                    .unwrap_or(raw_duration);

                                clips.push(ClipInfo {
                                    entity_bits,
                                    name: clip.name.clone(),
                                    track_index: track.index,
                                    timeline_start: clip.timeline_start,
                                    duration: effective_duration,
                                    selected: is_selected,
                                    color: None,
                                    asset_uuid,
                                    source_in: clip.source_in,
                                    source_out: clip.source_out,
                                    // Audio state from AudioClip component (defaults if absent)
                                    audio_muted: self
                                        .world
                                        .get::<strawberry_core::components::AudioClip>(clip_entity)
                                        .map(|a| a.muted)
                                        .unwrap_or(false),
                                    audio_volume: self
                                        .world
                                        .get::<strawberry_core::components::AudioClip>(clip_entity)
                                        .map(|a| a.volume)
                                        .unwrap_or(1.0),
                                });
                            }
                        }
                    }
                }
            }
        }

        clips
    }

    fn render_inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        ui.separator();

        // Wrap all inspector content in a scroll area for dynamic resizing
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Use full available width for controls
                ui.set_min_width(ui.available_width());

        // --- Selected Clip Properties (Editable) ---
        // Read selection from the Selection resource (stores entity bits in a HashSet)
        let selected_clip_info: Option<(uuid::Uuid, String, Clip, ClipMetadata)> = {
            let selection = self.world.resource::<Selection>();
            // Use primary selection, or fall back to first entry in the set
            let selected_bits: Option<u64> = selection
                .primary
                .or_else(|| selection.selected.iter().next().copied());

            if let Some(bits) = selected_bits {
                let entity = bevy_ecs::entity::Entity::from_bits(bits);
                // Look up the clip components on this entity
                let clip_opt = self.world.get::<Clip>(entity);
                let stable_id_opt = self.world.get::<StableId>(entity);
                match (clip_opt, stable_id_opt) {
                    (Some(clip), Some(stable_id)) => {
                        let meta: ClipMetadata = self
                            .world
                            .get::<ClipMetadata>(entity)
                            .cloned()
                            .unwrap_or_default();
                        tracing::debug!(
                            "Inspector: clip {} speed={:.2} reverse={} loop={:?}",
                            &stable_id.uuid().to_string()[..8],
                            meta.speed,
                            meta.reverse,
                            meta.loop_mode,
                        );
                        Some((stable_id.uuid(), clip.name.clone(), clip.clone(), meta))
                    }
                    _ => None,
                }
            } else {
                None
            }
        };

        if let Some((clip_uuid, clip_name, clip, metadata)) = selected_clip_info {
            // Clip name
            ui.strong(&clip_name);
            ui.add_space(4.0);

            // Read-only clip info
            egui::Grid::new("clip_info_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Source In:");
                    ui.label(format!("{:.2}s", clip.source_in.as_secs_f64()));
                    ui.end_row();

                    ui.label("Source Out:");
                    ui.label(format!("{:.2}s", clip.source_out.as_secs_f64()));
                    ui.end_row();

                    ui.label("Duration:");
                    ui.label(format!("{:.2}s", clip.duration().as_secs_f64()));
                    ui.end_row();

                    ui.label("Timeline Start:");
                    ui.label(format!("{:.2}s", clip.timeline_start.as_secs_f64()));
                    ui.end_row();
                });

            ui.separator();
            ui.strong("Time Remapping");
            ui.add_space(4.0);

            // --- Speed Picker (YouTube-style) ---
            let mut current_speed: f32 = (metadata.speed * 100.0).round() / 100.0;

            // Current speed — large centered display
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!("{:.2}x", current_speed))
                        .size(22.0)
                        .strong(),
                );
            });
            ui.add_space(4.0);

            // Slider with −/+ buttons
            let mut speed_changed: bool = false;
            ui.horizontal(|ui| {
                if ui.small_button("−").clicked() {
                    current_speed = ((current_speed - 0.05) * 100.0).round() / 100.0;
                    current_speed = current_speed.max(0.25);
                    speed_changed = true;
                }
                let slider_response = ui.add(
                    egui::Slider::new(&mut current_speed, 0.25..=3.0)
                        .step_by(0.05)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always),
                );
                if slider_response.changed() {
                    current_speed = (current_speed * 100.0).round() / 100.0;
                    speed_changed = true;
                }
                if ui.small_button("+").clicked() {
                    current_speed = ((current_speed + 0.05) * 100.0).round() / 100.0;
                    current_speed = current_speed.min(3.0);
                    speed_changed = true;
                }
            });
            ui.add_space(4.0);

            // Preset buttons row
            let presets: &[(f32, &str)] = &[
                (0.25, "0.25x"),
                (0.5, "0.5x"),
                (0.75, "0.75x"),
                (1.0, "Normal"),
                (1.25, "1.25x"),
                (1.5, "1.5x"),
                (2.0, "2.0x"),
                (3.0, "3.0x"),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &(preset_speed, label) in presets {
                    let is_active: bool = (current_speed - preset_speed).abs() < 0.01;
                    let button = if is_active {
                        egui::Button::new(
                            egui::RichText::new(label)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(60, 120, 200))
                    } else {
                        egui::Button::new(label)
                    };
                    if ui.add(button).clicked() {
                        current_speed = preset_speed;
                        speed_changed = true;
                    }
                }
            });

            // Submit on any change
            if speed_changed {
                tracing::info!(
                    "Inspector: submitting speed={:.2} for clip {}",
                    current_speed,
                    &clip_uuid.to_string()[..8],
                );
                let command_queue = self.world.resource::<CommandQueue>();
                command_queue.submit(Box::new(SetClipPropertyCommand::set_speed(
                    clip_uuid,
                    current_speed,
                )));
            }

            // --- Reverse Checkbox ---
            let mut current_reverse: bool = metadata.reverse;
            let reverse_response = ui.checkbox(&mut current_reverse, "Reverse Playback");
            if reverse_response.changed() {
                let command_queue = self.world.resource::<CommandQueue>();
                command_queue.submit(Box::new(SetClipPropertyCommand::set_reverse(
                    clip_uuid,
                    current_reverse,
                )));
            }

            // --- Loop Mode Dropdown ---
            let current_loop_mode: ClipLoopMode = metadata.loop_mode;
            let loop_label: &str = match current_loop_mode {
                ClipLoopMode::None => "None",
                ClipLoopMode::Loop => "Loop",
                ClipLoopMode::PingPong => "Ping-Pong",
                ClipLoopMode::Clamp => "Clamp",
            };

            ui.horizontal(|ui| {
                ui.label("Loop Mode:");
                egui::ComboBox::from_id_salt("clip_loop_mode")
                    .selected_text(loop_label)
                    .show_ui(ui, |ui| {
                        let modes: [(ClipLoopMode, &str); 4] = [
                            (ClipLoopMode::None, "None — Hold last frame"),
                            (ClipLoopMode::Loop, "Loop — Seamless repeat"),
                            (ClipLoopMode::PingPong, "Ping-Pong — Oscillate"),
                            (ClipLoopMode::Clamp, "Clamp — Freeze at boundary"),
                        ];
                        for (mode, label) in &modes {
                            if ui
                                .selectable_value(&mut metadata.loop_mode.clone(), *mode, *label)
                                .clicked()
                            {
                                let command_queue = self.world.resource::<CommandQueue>();
                                command_queue.submit(Box::new(
                                    SetClipPropertyCommand::set_loop_mode(clip_uuid, *mode),
                                ));
                            }
                        }
                    });
            });

            // --- Per-Clip Audio Controls ---
            {
                let selected_entity = {
                    let selection = self.world.resource::<strawberry_core::resources::Selection>();
                    selection
                        .primary
                        .or_else(|| selection.selected.iter().next().copied())
                        .map(bevy_ecs::entity::Entity::from_bits)
                };
                if let Some(entity) = selected_entity {
                    crate::audio_inspector::render_clip_audio_controls(
                        ui,
                        &self.world,
                        clip_uuid,
                        entity,
                    );
                }
            }
        } else {
            ui.colored_label(
                egui::Color32::from_gray(128),
                "Select a clip to edit properties",
            );
        }

        ui.separator();

        // --- Master Audio Controls (always visible) ---
        crate::audio_inspector::render_master_audio_controls(ui, &self.world);

        ui.separator();

        // --- Video File Metadata (existing) ---
        if let Some(path) = &self.video_metadata.path {
            if let Some(name) = path.file_name() {
                ui.label(format!("File: {}", name.to_string_lossy()));
            }
        }

        if let Some((w, h)) = self.video_metadata.resolution {
            ui.label(format!("Resolution: {}×{}", w, h));
        }

        if let Some(fps) = self.video_metadata.frame_rate {
            ui.label(format!("Frame Rate: {:.2} fps", fps));
        }

        if let Some(dur) = self.video_metadata.duration {
            let secs = dur.as_secs();
            ui.label(format!(
                "Duration: {:02}:{:02}:{:02}",
                secs / 3600,
                (secs / 60) % 60,
                secs % 60
            ));
        }

        if let Some(codec) = &self.video_metadata.codec {
            ui.label(format!("Codec: {}", codec));
        }

        ui.separator();

        // Show playhead info
        let playhead = self.world.resource::<Playhead>();
        ui.label(format!("Frame: {}", playhead.current_frame));
        ui.label(format!("Time: {:.2}s", playhead.current_time.as_secs_f64()));
        ui.label(format!("Speed: {:.1}x", playhead.playback_speed));
        }); // end ScrollArea
    }

    /// Render the media browser panel
    fn render_media_browser(&mut self, ui: &mut egui::Ui) {
        use strawberry_core::resources::AssetRegistry;

        // Get or create panel state from egui memory
        let mut panel: MediaBrowserPanel = ui.memory(|mem| {
            mem.data
                .get_temp::<MediaBrowserPanel>(egui::Id::new("media_browser_panel"))
                .unwrap_or_default()
        });

        // Get the asset registry
        let registry = self.world.get_resource::<AssetRegistry>();

        // Show the panel
        let response: MediaBrowserResponse = if let Some(registry) = registry {
            panel.show(ui, registry)
        } else {
            // No registry, show empty state
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("📁 No assets loaded");
                ui.label("Drop media files to import");
            });
            MediaBrowserResponse::default()
        };

        // Store panel state back to memory
        ui.memory_mut(|mem| {
            mem.data
                .insert_temp(egui::Id::new("media_browser_panel"), panel);
        });

        // Handle response actions - dispatch commands

        // Import button clicked -> trigger file dialog via DockingResponse
        if response.import_clicked {
            self.response.import_requested = true;
            tracing::info!("Import button clicked, flagging for file dialog");
        }

        // Regular double-click: add at end of track
        if let Some(asset_id) = response.asset_to_add {
            use strawberry_core::commands::AddAssetToTimelineCommand;
            let command_queue = self.world.resource::<CommandQueue>();
            command_queue.submit(Box::new(AddAssetToTimelineCommand::new(asset_id)));
            tracing::info!(
                "Submitted AddAssetToTimelineCommand (end of track) for asset {}",
                &asset_id.to_string()[..8]
            );
        }

        // Shift+double-click: add at playhead position
        if let Some(asset_id) = response.add_at_playhead {
            use strawberry_core::commands::AddAssetToTimelineCommand;
            let command_queue = self.world.resource::<CommandQueue>();
            command_queue.submit(Box::new(AddAssetToTimelineCommand::at_playhead(asset_id)));
            tracing::info!(
                "Submitted AddAssetToTimelineCommand (at playhead) for asset {}",
                &asset_id.to_string()[..8]
            );
        }

        if let Some(asset_id) = response.delete_asset {
            use strawberry_core::commands::DeleteAssetCommand;
            let command_queue = self.world.resource::<CommandQueue>();
            command_queue.submit(Box::new(DeleteAssetCommand::new(asset_id)));
            tracing::info!(
                "Submitted DeleteAssetCommand for asset {}",
                &asset_id.to_string()[..8]
            );
        }
        if let Some(asset_id) = response.regenerate_proxy {
            use strawberry_core::commands::RegenerateProxyCommand;
            let command_queue = self.world.resource::<CommandQueue>();
            command_queue.submit(Box::new(RegenerateProxyCommand::new(asset_id)));
            tracing::info!(
                "Submitted RegenerateProxyCommand for asset {}",
                &asset_id.to_string()[..8]
            );
        }
    }

    /// Pre-update the TimelineViewport resource before filmstrip texture loading.
    ///
    /// This eliminates the one-frame viewport lag that caused filmstrip textures to
    /// fail loading. Without this, the viewport was only updated AFTER
    /// `timeline_widget.show()`, but `load_filmstrip_textures()` runs BEFORE `show()`.
    ///
    /// Uses `TimelineViewState` (zoom, scroll_offset) which is always current, and
    /// `available_width` from `ui.available_width()` as an approximate viewport width.
    /// The post-show update in the response handler refines this with the exact rect.
    fn pre_update_timeline_viewport(&mut self, available_width: f32) {
        let view_state = self.world.resource::<TimelineViewState>();
        let zoom: f32 = view_state.zoom;
        let scroll_offset_secs: f64 = view_state.scroll_offset;
        let scroll_offset_px: f32 = (scroll_offset_secs * zoom as f64) as f32;

        if let Some(mut viewport) = self
            .world
            .get_resource_mut::<strawberry_core::resources::TimelineViewport>()
        {
            viewport.update_from_view_state(scroll_offset_px, zoom, available_width);
        }
    }

    /// Load filmstrip textures for timeline clips
    ///
    /// Reads thumbnail JPEGs from proxy directories and uploads them as egui textures.
    /// Returns a map of asset_uuid -> FilmstripAssetTextures for passing to TimelineWidget.
    ///
    /// ## Performance Optimization
    ///
    /// Uses `TimelineViewport` to filter visible clips first. Only clips that overlap
    /// the visible time range will have their filmstrips loaded. This prevents memory
    /// explosion when the timeline has 1000+ clips.
    fn load_filmstrip_textures(
        &self,
        ctx: &egui::Context,
        clips: &[ClipInfo],
    ) -> std::collections::HashMap<uuid::Uuid, FilmstripAssetTextures> {
        use std::collections::{HashMap, HashSet};

        let mut result = HashMap::new();

        // =====================================================================
        // VIEWPORT FILTERING: Only load textures for visible clips
        // =====================================================================
        // Key optimization for 1000+ clips: only load filmstrip textures for
        // clips visible in the viewport, plus a pre-fetch margin for smooth
        // scrolling (professional NLE behavior).
        //
        // The pre-fetch margin ensures thumbnails are loaded slightly before
        // a clip scrolls into view, preventing visible pop-in.
        const FILMSTRIP_PREFETCH_MARGIN_SECS: f64 = 5.0;

        let viewport = self
            .world
            .get_resource::<strawberry_core::resources::TimelineViewport>();

        let visible_clips: Vec<&ClipInfo> = if let Some(vp) = viewport {
            // Expand viewport by pre-fetch margin for filmstrip loading
            let prefetch_start: f64 =
                (vp.visible_start_secs - FILMSTRIP_PREFETCH_MARGIN_SECS).max(0.0);
            let prefetch_end: f64 = vp.visible_end_secs + FILMSTRIP_PREFETCH_MARGIN_SECS;

            let all_visible: Vec<&ClipInfo> = clips
                .iter()
                .filter(|clip| {
                    let clip_start: f64 = clip.timeline_start.as_secs_f64();
                    let clip_end: f64 = clip_start + clip.duration.as_secs_f64();
                    clip_end > prefetch_start && clip_start < prefetch_end
                })
                .collect();

            // Safety fallback: if viewport excludes all clips but clips exist,
            // load all. Prevents blank filmstrips from edge cases like the first
            // frame after launch or after undo/redo operations.
            if all_visible.is_empty() && !clips.is_empty() {
                clips.iter().collect()
            } else {
                all_visible
            }
        } else {
            // No viewport resource available — load all clips
            clips.iter().collect()
        };

        // Collect unique asset UUIDs from visible (+ pre-fetched) clips
        let visible_asset_uuids: HashSet<uuid::Uuid> = visible_clips
            .iter()
            .filter_map(|clip| clip.asset_uuid)
            .collect();

        // Get proxy cache dir from config
        let cache_dir: std::path::PathBuf = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("strawberry")
            .join("proxies");

        // Get asset registry to resolve source paths for content-hash proxy directories
        let asset_registry = self
            .world
            .get_resource::<strawberry_core::resources::AssetRegistry>();

        for asset_id in &visible_asset_uuids {
            // Resolve proxy directory via content hash (matches ProxyManager storage)
            // Asset UUID → source_path (from registry) → content_hash → proxy directory
            let proxy_dir = if let Some(registry) = asset_registry {
                if let Some(asset) = registry.get(asset_id) {
                    let content_hash = strawberry_video::proxy::compute_proxy_content_hash(&asset.source_path);
                    cache_dir.join(content_hash)
                } else {
                    continue; // Asset not in registry — skip
                }
            } else {
                continue; // No registry — skip
            };
            let thumbnails_dir = proxy_dir.join("thumbnails");

            if !thumbnails_dir.exists() {
                continue;
            }

            // Load LOD0 (10 second interval, 40px height)
            let lod0_dir = thumbnails_dir.join("lod0");
            let lod0_textures = self.load_lod_textures(ctx, &lod0_dir, 10.0, &asset_id, 0);

            // Load LOD1 (2 second interval, 80px height)
            let lod1_dir = thumbnails_dir.join("lod1");
            let lod1_textures = self.load_lod_textures(ctx, &lod1_dir, 2.0, &asset_id, 1);

            if !lod0_textures.is_empty() || !lod1_textures.is_empty() {
                result.insert(
                    *asset_id, // Dereference since we're iterating over &HashSet
                    FilmstripAssetTextures {
                        lod0: lod0_textures,
                        lod1: lod1_textures,
                        thumb_width: 80.0, // LOD1 size
                        thumb_height: 80.0,
                        lod0_interval: 10.0,
                        lod1_interval: 2.0,
                    },
                );
            }
        }

        // =====================================================================
        // LRU EVICTION: Limit cache size to prevent memory explosion
        // =====================================================================
        // After loading visible clips, evict oldest off-screen textures
        // to keep GPU memory bounded. Max 786 textures (~50MB at 80x80 RGBA).
        const MAX_FILMSTRIP_TEXTURES: usize = 786;

        let cache_id = egui::Id::new("filmstrip_texture_cache");
        ctx.memory_mut(|mem| {
            if let Some(mut cache) = mem.data.get_temp::<FilmstripTextureCache>(cache_id) {
                if cache.len() > MAX_FILMSTRIP_TEXTURES {
                    cache.evict_lru_except(&visible_asset_uuids, MAX_FILMSTRIP_TEXTURES);
                    mem.data.insert_temp(cache_id, cache);
                }
            }
        });

        result
    }

    /// Load thumbnails from a LOD directory and upload to egui
    fn load_lod_textures(
        &self,
        ctx: &egui::Context,
        lod_dir: &std::path::Path,
        interval: f64,
        asset_id: &uuid::Uuid,
        lod_level: u8,
    ) -> Vec<(f64, egui::TextureId)> {
        let mut textures = Vec::new();

        // Get or create persistent texture cache from egui memory
        let cache_id = egui::Id::new("filmstrip_texture_cache");
        let mut cache: FilmstripTextureCache = ctx.memory(|mem| {
            mem.data
                .get_temp::<FilmstripTextureCache>(cache_id)
                .unwrap_or_default()
        });

        // PERFORMANCE FIX: Cache directory listing to avoid per-frame disk I/O
        // The key is (asset_id, lod_level) -> cached paths
        let dir_cache_key = format!("filmstrip_dir_{}_{}", asset_id, lod_level);

        // Check if we have a cached directory listing
        let dir_cache_id = egui::Id::new("filmstrip_dir_cache");
        let mut dir_cache: std::collections::HashMap<String, Vec<(u32, std::path::PathBuf)>> = ctx
            .memory(|mem| {
                mem.data
                    .get_temp::<std::collections::HashMap<String, Vec<(u32, std::path::PathBuf)>>>(
                        dir_cache_id,
                    )
                    .unwrap_or_default()
            });

        // Use cached directory listing or scan disk if not cached
        // IMPORTANT: Only cache non-empty results! If thumbnails aren't generated yet,
        // we want to re-check on next frame when they might be ready.
        let paths: Vec<(u32, std::path::PathBuf)> =
            if let Some(cached_paths) = dir_cache.get(&dir_cache_key) {
                // Cache hit - use cached paths
                cached_paths.clone()
            } else {
                // Cache miss - scan disk
                if !lod_dir.exists() {
                    // Directory doesn't exist - DON'T cache, it may appear later
                    return textures;
                }

                let Ok(entries) = std::fs::read_dir(lod_dir) else {
                    // Read failed - DON'T cache, may succeed later
                    return textures;
                };

                let mut scanned_paths: Vec<(u32, std::path::PathBuf)> = entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let path = e.path();
                        let name = path.file_stem()?.to_str()?;
                        if name.starts_with("thumb_") {
                            let num_str = name.strip_prefix("thumb_")?;
                            let num: u32 = num_str.parse().ok()?;
                            Some((num, path))
                        } else {
                            None
                        }
                    })
                    .collect();

                scanned_paths.sort_by_key(|(n, _)| *n);

                // ONLY cache if we found thumbnails - empty dirs may get populated later
                if !scanned_paths.is_empty() {
                    dir_cache.insert(dir_cache_key.clone(), scanned_paths.clone());
                    ctx.memory_mut(|mem| {
                        mem.data.insert_temp(dir_cache_id, dir_cache);
                    });
                }

                scanned_paths
            };

        for (idx, (_, path)) in paths.iter().enumerate() {
            // Calculate timestamp: thumb_0001 = 0s, thumb_0002 = interval, etc.
            let timestamp = idx as f64 * interval;

            // Generate stable texture name
            let texture_name = format!("filmstrip_{}_lod{}_{}", asset_id, lod_level, idx);

            // Check if already in cache - use separate lookup and touch to avoid borrow conflict
            let cached_id = cache.handles.get(&texture_name).map(|h| h.id());

            let texture_id = if let Some(id) = cached_id {
                // Touch for LRU tracking - mark as recently used
                cache.touch(&texture_name);
                id
            } else {
                // Load image from disk and upload to GPU
                if let Ok(img_data) = std::fs::read(path) {
                    if let Ok(img) = image::load_from_memory(&img_data) {
                        let rgba = img.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        let pixels = rgba.into_raw();

                        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                        let handle = ctx.load_texture(
                            &texture_name,
                            color_image,
                            egui::TextureOptions::LINEAR,
                        );
                        let id = handle.id();

                        // Store handle in cache to keep texture alive
                        cache.handles.insert(texture_name.clone(), handle);
                        // Touch for LRU tracking
                        cache.touch(&texture_name);

                        id
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            };

            textures.push((timestamp, texture_id));
        }

        // Store cache back to egui memory
        ctx.memory_mut(|mem| {
            mem.data.insert_temp(cache_id, cache);
        });

        textures
    }
}
