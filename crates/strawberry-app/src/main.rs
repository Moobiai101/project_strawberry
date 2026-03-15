//! # Project Strawberry
//!
//! AI-Native Creative Engine for video editing, motion graphics, and animation.
//!
//! ## This is NOT an MVP
//!
//! This is a production-grade tool that users will depend on daily.
//! Every line of code must be shippable quality.

mod docking;
mod viewport_renderer;

use anyhow::Result;
use bevy_ecs::prelude::*;
use docking::{DockingState, VideoMetadataCache};
use std::path::PathBuf;
use std::sync::mpsc;
use strawberry_audio::KiraMixerService;
use strawberry_core::commands::{
    AddClipCommand, CreateTimelineCommand, DeleteClipCommand, PauseCommand, PlayCommand,
    RippleDeleteCommand, SeekCommand, SetSpeedCommand, SplitClipCommand, StopCommand,
};
use strawberry_core::components::{ActiveTimeline, Clip, TrackType};
use strawberry_core::resources::{
    CommandQueue, MasterAudioState, Playhead, ScrubState, ThumbnailTextureCache, UndoRedoStack,
    ViewportState,
};
use strawberry_core::systems::{
    execute_commands_system, playhead_advance_system, timeline_duration_sync_system, PlaybackClock,
};
use strawberry_render::{
    audio_mixer_sync_system, compositor_system, ActiveLayersResource, AudioMixerResource,
    CompositorCache, CompositorOutput, TextureManager, ThumbnailTextureManager, TraverserResource,
    VideoAssetManager,
};
use strawberry_ui::StrawberryTheme;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "strawberry=info,wgpu=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Project Strawberry");

    // Run the application
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Project Strawberry"),
        // Request the GPU backend
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Project Strawberry",
        options,
        Box::new(|cc| Ok(Box::new(StrawberryApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}

/// Video metadata for status bar display
#[derive(Default, Clone)]
struct VideoMetadataDisplay {
    path: Option<PathBuf>,
    resolution: Option<(u32, u32)>,
    frame_rate: Option<f64>,
    duration: Option<std::time::Duration>,
    codec: Option<String>,
}

/// Pending clip to add after timeline is created
#[derive(Clone)]
struct PendingClip {
    name: String,
    duration: std::time::Duration,
    /// UUID of the loaded video asset in VideoAssetManager
    asset_uuid: uuid::Uuid,
    #[allow(dead_code)]
    path: PathBuf,
}

/// Main application state
struct StrawberryApp {
    /// ECS World
    world: World,
    /// Main update schedule
    schedule: Schedule,
    /// Pending file to open (for drag-drop deferred loading)
    pending_file: Option<PathBuf>,
    /// Pending clip to add after timeline is created
    pending_clip: Option<PendingClip>,
    /// Current video metadata for status bar
    video_metadata: VideoMetadataDisplay,
    /// Docking state for panel layout
    docking: DockingState,
    /// Receiver for completed audio extractions (polled each frame)
    audio_extraction_rx: mpsc::Receiver<AudioExtractionResult>,
    /// Sender cloned into each spawned audio extraction thread
    audio_extraction_tx: mpsc::Sender<AudioExtractionResult>,
    /// Receiver for completed background asset imports (polled each frame)
    asset_import_rx: mpsc::Receiver<AssetImportResult>,
    /// Sender cloned into each spawned asset import thread
    asset_import_tx: mpsc::Sender<AssetImportResult>,
}

impl StrawberryApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply custom theme
        StrawberryTheme::apply(&cc.egui_ctx);

        let mut world = World::new();
        let mut schedule = Schedule::default();

        // Initialize resources
        world.insert_resource(Playhead::default());
        world.insert_resource(PlaybackClock::default());
        world.insert_resource(ViewportState::default());
        world.insert_resource(TextureManager::new());
        world.insert_resource(CommandQueue::new());
        world.insert_resource(UndoRedoStack::default());
        world.insert_resource(ActiveTimeline::default());
        world.insert_resource(strawberry_core::resources::Selection::default());
        world.insert_resource(strawberry_core::resources::TimelineViewState::default());
        world.insert_resource(strawberry_core::resources::StableIdRegistry::default());
        world.insert_resource(strawberry_core::resources::ScrubState::default());
        world.insert_resource(strawberry_core::resources::AssetRegistry::new());
        world.insert_resource(strawberry_core::resources::TimelineViewport::default());
        world.insert_resource(strawberry_core::resources::CollisionSettings::default());
        world.insert_resource(strawberry_core::gizmo::gizmo_state::GizmoState::new());
        world.insert_resource(strawberry_core::resources::ProjectSettings::default());

        // Initialize Compositor Resources (Node-based architecture)
        world.insert_resource(VideoAssetManager::default());
        world.insert_resource(TraverserResource::default());
        world.insert_resource(ActiveLayersResource::default());
        world.insert_resource(CompositorOutput::default());
        world.insert_resource(CompositorCache::default());

        // Initialize Thumbnail Resources (for filmstrip rendering)
        world.insert_resource(ThumbnailTextureCache::default());
        world.insert_resource(ThumbnailTextureManager::new());

        // Initialize Audio Mixer (kira-based multi-track mixer)
        world.insert_resource(MasterAudioState::default());
        match KiraMixerService::new() {
            Ok(mixer) => {
                world.insert_resource(AudioMixerResource(mixer));
            }
            Err(e) => {
                tracing::error!("Failed to initialize KiraMixerService: {}", e);
                // Continue without audio — the sync system will gracefully skip
            }
        }

        // Add systems - ORDER MATTERS!
        // 1. Execute commands first (process user input)
        schedule.add_systems(execute_commands_system);
        // 2. Sync timeline duration from clip endpoints (before playhead advance)
        schedule.add_systems(timeline_duration_sync_system);
        // 3. Advance playhead (uses synced duration for bounds checking)
        schedule.add_systems(playhead_advance_system);
        // 4. Compositor renders timeline content using NodeTraverser
        schedule.add_systems(compositor_system);
        // 5. Audio mixer sync — uses traverser layers to play active clips
        schedule.add_systems(audio_mixer_sync_system);

        // Configure wgpu if available
        if let Some(render_state) = &cc.wgpu_render_state {
            tracing::info!("WGPU init: adapter={:?}", render_state.adapter.get_info());
        }

        let (audio_tx, audio_rx) = mpsc::channel::<AudioExtractionResult>();
        let (import_tx, import_rx) = mpsc::channel::<AssetImportResult>();

        Self {
            world,
            schedule,
            pending_file: None,
            pending_clip: None,
            video_metadata: VideoMetadataDisplay::default(),
            docking: DockingState::new(),
            audio_extraction_rx: audio_rx,
            audio_extraction_tx: audio_tx,
            asset_import_rx: import_rx,
            asset_import_tx: import_tx,
        }
    }

    /// Handle file open (from dialog or drag-drop)
    ///
    /// Loads the video into VideoAssetManager and creates timeline/clip.
    fn open_file(&mut self, path: PathBuf) {
        // Load asset into VideoAssetManager
        let load_result = {
            let mut asset_manager = self.world.resource_mut::<VideoAssetManager>();
            asset_manager.load_asset(&path)
        };

        match load_result {
            Ok(asset_uuid) => {
                tracing::info!("Loaded asset with UUID: {}", asset_uuid);

                // Get metadata from the service we just loaded
                let metadata = {
                    let asset_manager = self.world.resource::<VideoAssetManager>();
                    asset_manager
                        .get_service(&asset_uuid)
                        .and_then(|s| s.current_video().cloned())
                };

                if let Some(loaded) = metadata {
                    // Update viewport state
                    {
                        let mut viewport = self.world.resource_mut::<ViewportState>();
                        viewport.reset();
                        viewport.loaded_asset = Some(1); // Placeholder
                        viewport.source_resolution =
                            glam::UVec2::new(loaded.metadata.width, loaded.metadata.height);
                    }

                    // Register asset in AssetRegistry (for Media Browser)
                    {
                        use strawberry_core::resources::{AssetMetadata, RegisteredAsset};
                        let filename = path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Untitled".to_string());
                        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let asset_metadata = AssetMetadata::video(
                            filename,
                            loaded.metadata.duration,
                            loaded.metadata.width,
                            loaded.metadata.height,
                            loaded.metadata.frame_rate,
                            loaded.metadata.codec.clone(),
                            loaded.metadata.frame_count,
                            file_size,
                        );
                        let registered_asset =
                            RegisteredAsset::new(asset_uuid, path.clone(), asset_metadata);
                        let mut registry = self
                            .world
                            .resource_mut::<strawberry_core::resources::AssetRegistry>();
                        registry.register(registered_asset);
                    }

                    // Reset playhead
                    {
                        let mut playhead = self.world.resource_mut::<Playhead>();
                        playhead.is_playing = false;
                        playhead.current_time = std::time::Duration::ZERO;
                        playhead.current_frame = 0;
                    }

                    // Update playback clock with video timing
                    {
                        let mut clock = self.world.resource_mut::<PlaybackClock>();
                        clock.set_timeline(loaded.metadata.duration, loaded.metadata.frame_rate);
                    }

                    // Spawn background audio extraction (non-blocking).
                    // Audio will be registered when extraction completes.
                    if self.world.contains_resource::<AudioMixerResource>() {
                        let open_file_uuid = {
                            let asset_manager = self.world.resource::<VideoAssetManager>();
                            asset_manager.active_asset()
                        };
                        if let Some(uuid) = open_file_uuid {
                            spawn_audio_extraction(
                                uuid,
                                path.clone(),
                                self.audio_extraction_tx.clone(),
                            );
                        }
                    }
                    // Store metadata for status bar
                    self.video_metadata = VideoMetadataDisplay {
                        path: Some(path.clone()),
                        resolution: Some((loaded.metadata.width, loaded.metadata.height)),
                        frame_rate: Some(loaded.metadata.frame_rate),
                        duration: Some(loaded.metadata.duration),
                        codec: Some(loaded.metadata.codec.clone()),
                    };

                    tracing::info!(
                        "Loaded video: {}x{} @ {:.2}fps, {:.2}s",
                        loaded.metadata.width,
                        loaded.metadata.height,
                        loaded.metadata.frame_rate,
                        loaded.metadata.duration.as_secs_f64()
                    );

                    // ========================================================
                    // AUTO-CREATE TIMELINE + TRACK + CLIP (like real NLEs)
                    // ========================================================
                    let has_timeline = self
                        .world
                        .get_resource::<ActiveTimeline>()
                        .map(|at| at.get().is_some())
                        .unwrap_or(false);

                    if !has_timeline {
                        tracing::info!(
                            "No timeline exists - creating timeline with video track..."
                        );

                        let file_name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Untitled".to_string());

                        let command_queue = self.world.resource::<CommandQueue>();

                        // Determine FrameRate from video metadata
                        let frame_rate = match loaded.metadata.frame_rate as u32 {
                            24 => strawberry_core::components::FrameRate::Fps24,
                            25 => strawberry_core::components::FrameRate::Fps25,
                            30 => strawberry_core::components::FrameRate::Fps30,
                            50 => strawberry_core::components::FrameRate::Fps50,
                            60 => strawberry_core::components::FrameRate::Fps60,
                            _ => {
                                let fps_rounded =
                                    (loaded.metadata.frame_rate * 1000.0).round() as u32;
                                strawberry_core::components::FrameRate::Custom(fps_rounded, 1000)
                            }
                        };

                        // Create timeline command
                        command_queue.submit(Box::new(CreateTimelineCommand::new(
                            format!("{} Timeline", file_name),
                            frame_rate,
                        )));

                        tracing::info!("Timeline creation command submitted");

                        // Store pending clip with asset UUID for later attachment
                        self.pending_clip = Some(PendingClip {
                            name: file_name.clone(),
                            duration: loaded.metadata.duration,
                            asset_uuid,
                            path: path.clone(),
                        });
                    }
                } else {
                    tracing::error!("Asset loaded but no metadata available");
                }
            }
            Err(e) => {
                tracing::error!("Failed to load video: {}", e);
                self.video_metadata = VideoMetadataDisplay::default();
            }
        }
    }

    /// Show file open dialog
    fn show_open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mov", "avi", "mkv", "webm"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.open_file(path);
        }
    }

    /// Show import media dialog
    ///
    /// Opens a native file dialog that allows selecting MULTIPLE media files.
    /// Each selected file is imported in a background thread to avoid blocking
    /// the UI during FFmpeg probe (~200-500ms per file).
    fn show_import_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Import Media")
            .add_filter("Video", &["mp4", "mov", "avi", "mkv", "webm", "mxf", "mts"])
            .add_filter("Audio", &["mp3", "wav", "aac", "flac", "ogg"])
            .add_filter("Image", &["png", "jpg", "jpeg", "tiff", "exr", "dpx"])
            .add_filter("All Files", &["*"])
            .pick_files()
        {
            for path in paths {
                self.import_media(path);
            }
        }
    }

    /// Import media file to MediaBrowser (non-blocking, background thread).
    ///
    /// Spawns a background thread that:
    /// 1. Creates VideoService + loads metadata (blocking FFmpeg probe)
    /// 2. Sends the result back via mpsc channel
    ///
    /// The main thread picks up results in `poll_asset_imports()` each frame
    /// and registers the service in VideoAssetManager + AssetRegistry.
    fn import_media(&mut self, path: PathBuf) {
        // Check if already loaded or already being loaded
        {
            let mut asset_manager = self.world.resource_mut::<VideoAssetManager>();
            if !asset_manager.mark_import_pending(&path) {
                tracing::info!("Skipping duplicate import for: {:?}", path);
                return;
            }
        }

        let uuid: uuid::Uuid = uuid::Uuid::new_v4();
        let result_tx = self.asset_import_tx.clone();
        let import_path: PathBuf = path.clone();

        tracing::info!(
            "Starting background import for: {:?} (uuid: {})",
            path.file_name().unwrap_or_default(),
            &uuid.to_string()[..8]
        );

        // Spawn background thread for FFmpeg probe (200-500ms per file)
        std::thread::spawn(move || {
            // Create VideoService and load metadata (blocking FFmpeg probe)
            let mut service: strawberry_video::service::VideoService =
                match strawberry_video::service::VideoService::new() {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = result_tx.send(AssetImportResult {
                            uuid,
                            path: import_path,
                            outcome: AssetImportOutcome::Failed {
                                error: format!("VideoService init failed: {}", e),
                            },
                        });
                        return;
                    }
                };

            let metadata: strawberry_video::VideoMetadata = match service.load(&import_path) {
                Ok(m) => m,
                Err(e) => {
                    let _ = result_tx.send(AssetImportResult {
                        uuid,
                        path: import_path,
                        outcome: AssetImportOutcome::Failed {
                            error: format!("FFmpeg probe failed: {}", e),
                        },
                    });
                    return;
                }
            };

            let _ = result_tx.send(AssetImportResult {
                uuid,
                path: import_path,
                outcome: AssetImportOutcome::Success { service, metadata },
            });
        });
    }

    /// Poll for completed background asset imports (called each frame).
    ///
    /// Drains the import result channel and for each completed import:
    /// 1. Registers the VideoService in VideoAssetManager
    /// 2. Registers the asset in AssetRegistry (for Media Browser)
    /// 3. Spawns audio extraction (non-blocking)
    fn poll_asset_imports(&mut self) {
        while let Ok(result) = self.asset_import_rx.try_recv() {
            match result.outcome {
                AssetImportOutcome::Success { service, metadata } => {
                    let uuid: uuid::Uuid = result.uuid;
                    let path: PathBuf = result.path;

                    // Register pre-loaded service in VideoAssetManager
                    {
                        let mut asset_manager = self.world.resource_mut::<VideoAssetManager>();
                        asset_manager.register_loaded_service(
                            uuid,
                            service,
                            path.clone(),
                            metadata.duration,
                            metadata.frame_rate,
                        );
                    }

                    // Register in AssetRegistry (for Media Browser display)
                    {
                        use strawberry_core::resources::{AssetMetadata, RegisteredAsset};
                        let filename: String = path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Untitled".to_string());
                        let file_size: u64 = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let asset_metadata: AssetMetadata = AssetMetadata::video(
                            filename.clone(),
                            metadata.duration,
                            metadata.width,
                            metadata.height,
                            metadata.frame_rate,
                            metadata.codec.clone(),
                            metadata.frame_count,
                            file_size,
                        );
                        let registered_asset: RegisteredAsset =
                            RegisteredAsset::new(uuid, path.clone(), asset_metadata);
                        let mut registry = self
                            .world
                            .resource_mut::<strawberry_core::resources::AssetRegistry>();
                        registry.register(registered_asset);
                    }

                    // Spawn background audio extraction (non-blocking)
                    if self.world.contains_resource::<AudioMixerResource>() {
                        spawn_audio_extraction(
                            uuid,
                            path.clone(),
                            self.audio_extraction_tx.clone(),
                        );
                    }

                    tracing::info!(
                        "Background import complete: {} ({}x{} @ {:.2}fps, {:.2}s)",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        metadata.width,
                        metadata.height,
                        metadata.frame_rate,
                        metadata.duration.as_secs_f64()
                    );
                }
                AssetImportOutcome::Failed { error } => {
                    // Clear pending state so user can retry
                    {
                        let mut asset_manager = self.world.resource_mut::<VideoAssetManager>();
                        asset_manager.clear_import_pending(&result.path);
                    }
                    tracing::error!(
                        "Failed to import {:?}: {}",
                        result.path.file_name().unwrap_or_default(),
                        error
                    );
                }
            }
        }
    }
}

impl eframe::App for StrawberryApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Poll for completed audio extractions (non-blocking)
        self.poll_audio_extractions();

        // Poll for completed background asset imports (non-blocking)
        self.poll_asset_imports();

        // Sync proxy completions from VideoAssetManager → AssetRegistry
        self.sync_proxy_completions_to_registry();

        // DEBUG: Check global input state
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Delete) {
                tracing::debug!("Global: DELETE pressed. Modifiers: {:?}", i.modifiers);
                if i.modifiers.shift {
                    tracing::debug!("Global: SHIFT+DELETE pressed");
                }
            }
        });

        // Process any pending file (from drag-drop)
        if let Some(path) = self.pending_file.take() {
            self.open_file(path);
        }

        // =====================================================================
        // PREEMPTIVE SCRUB DETECTION (Fixes one-frame delay bug)
        // =====================================================================
        // The UI (docking.rs) updates ScrubState AFTER schedule.run(), but the
        // compositor needs the value BEFORE it runs. To fix this, we preemptively
        // detect if the pointer is being dragged and update ScrubState here.
        //
        // This is a coarse check - the detailed "can_scrub" logic lives in
        // timeline.rs, but for compositor optimization purposes, detecting any
        // primary button drag is sufficient.
        let (pointer_dragging, primary_down, any_down) = ctx.input(|i| {
            (
                i.pointer.is_decidedly_dragging(),
                i.pointer.primary_down(),
                i.pointer.any_down(),
            )
        });

        {
            let mut scrub_state = self.world.resource_mut::<ScrubState>();
            // Use primary_down instead of is_decidedly_dragging for immediate response
            if primary_down && !scrub_state.is_scrubbing {
                scrub_state.start_scrub();
            } else if !primary_down && scrub_state.is_scrubbing {
                // Only end if timeline also says not scrubbing (handled in docking)
                // Don't end here to avoid race condition
            }
            // Note: Let docking.rs handle scrub end to use timeline's proper detection
        }

        // Run ECS systems
        self.schedule.run(&mut self.world);

        // Process pending clip - add to V1 track if timeline now exists
        if self.pending_clip.is_some() {
            // Check if timeline now exists
            let timeline_exists = self
                .world
                .get_resource::<ActiveTimeline>()
                .map(|at| at.get().is_some())
                .unwrap_or(false);

            if timeline_exists {
                // Find V1 track (first video track)
                use bevy_hierarchy::Children;
                use strawberry_core::components::Track;

                let mut v1_track_entity: Option<bevy_ecs::entity::Entity> = None;

                if let Some(active_timeline) = self.world.get_resource::<ActiveTimeline>() {
                    if let Some(timeline_entity) = active_timeline.get() {
                        if let Some(children) = self.world.get::<Children>(timeline_entity) {
                            for &child in children.iter() {
                                if let Some(track) = self.world.get::<Track>(child) {
                                    if track.track_type == TrackType::Video && track.index == 0 {
                                        v1_track_entity = Some(child);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                if let (Some(pending), Some(track_entity)) =
                    (self.pending_clip.take(), v1_track_entity)
                {
                    // Get current playhead position for clip placement
                    // This enables collision detection when clips overlap
                    let playhead_position: std::time::Duration = self
                        .world
                        .get_resource::<strawberry_core::resources::Playhead>()
                        .map(|p| p.current_time)
                        .unwrap_or(std::time::Duration::ZERO);

                    tracing::info!(
                        "Adding clip '{}' (asset: {}) at playhead position {:.2}s",
                        pending.name,
                        &pending.asset_uuid.to_string()[..8],
                        playhead_position.as_secs_f64()
                    );

                    // We need an asset entity - for now create a placeholder
                    // In production, we'd create a VideoAsset entity first
                    let asset_entity = self.world.spawn(()).id();

                    let command_queue = self.world.resource::<CommandQueue>();
                    command_queue.submit(Box::new(
                        // Place clip at playhead position - collision detection will
                        // automatically create a new track if there's overlap
                        AddClipCommand::new(track_entity, asset_entity, playhead_position)
                            .with_name(pending.name.clone())
                            .with_source_range(std::time::Duration::ZERO, pending.duration)
                            .with_asset_uuid(pending.asset_uuid, pending.name),
                        // Note: auto_sequence disabled to use playhead position
                    ));

                    tracing::info!(
                        "AddClipCommand submitted at playhead position (collision detection enabled)"
                    );
                }
            }
        }

        // Process GPU uploads and reclaim staging buffers
        if let Some(render_state) = frame.wgpu_render_state() {
            let texture_manager = self.world.resource::<TextureManager>();

            // Advance LRU frame counter (was NEVER called — broke slot eviction)
            texture_manager.advance_frame();

            let mut renderer = render_state.renderer.write();
            texture_manager.process_uploads(
                &render_state.device,
                &render_state.queue,
                &mut renderer,
            );

            // NON-BLOCKING staging buffer reclamation.
            //
            // Each queue.write_texture() allocates a wgpu staging buffer that persists
            // until device.poll() confirms the GPU finished the copy. Without this call,
            // staging buffers are NEVER freed — accumulating to 44GB+ over 3 minutes of
            // continuous playback, causing a fatal OOM crash.
            //
            // Maintain::Poll is non-blocking (<0.1ms) — it checks a GPU fence value and
            // frees any completed staging buffers. This is what Unity/Unreal/Godot do.
            // Maintain::Wait would block the CPU, killing parallelism.
            render_state.device.poll(eframe::wgpu::Maintain::Poll);
        }

        // --- UI ---

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        // TODO: CreateProjectCommand
                        ui.close_menu();
                    }
                    if ui
                        .add(egui::Button::new("Open...").shortcut_text("Ctrl+O"))
                        .clicked()
                    {
                        ui.close_menu();
                        self.show_open_dialog();
                    }
                    if ui
                        .add(egui::Button::new("Import Media...").shortcut_text("Ctrl+I"))
                        .clicked()
                    {
                        ui.close_menu();
                        self.show_import_dialog();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Fit to Viewport").clicked() {
                        self.world.resource_mut::<ViewportState>().fit_to_viewport();
                        ui.close_menu();
                    }
                });

                // Spacer
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Show current file name in menu bar
                    if let Some(path) = &self.video_metadata.path {
                        if let Some(name) = path.file_name() {
                            ui.label(egui::RichText::new(name.to_string_lossy()).weak());
                        }
                    }
                });
            });
        });

        // Status bar at very bottom
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Video metadata
                    if let Some((w, h)) = self.video_metadata.resolution {
                        ui.label(format!("{}×{}", w, h));
                        ui.separator();
                    }

                    if let Some(fps) = self.video_metadata.frame_rate {
                        ui.label(format!("{:.2} fps", fps));
                        ui.separator();
                    }

                    if let Some(dur) = self.video_metadata.duration {
                        let secs = dur.as_secs();
                        let mins = secs / 60;
                        let hours = mins / 60;
                        ui.label(format!(
                            "Duration: {:02}:{:02}:{:02}",
                            hours,
                            mins % 60,
                            secs % 60
                        ));
                        ui.separator();
                    }

                    if let Some(codec) = &self.video_metadata.codec {
                        ui.label(codec);
                        ui.separator();
                    }

                    // Playhead info
                    let playhead = self.world.resource::<Playhead>();
                    ui.label(format!(
                        "Frame {} | {:.2}s",
                        playhead.current_frame,
                        playhead.current_time.as_secs_f64()
                    ));

                    // GPU info
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // VideoAssetManager is always available (default)
                        ui.label(
                            egui::RichText::new("●")
                                .color(egui::Color32::GREEN)
                                .size(10.0),
                        );
                        ui.label("Compositor Ready");
                    });
                });
            });

        // --- Dockable Panels (Viewport, Timeline, Inspector) ---
        // Convert video metadata for docking
        let video_meta = VideoMetadataCache {
            path: self.video_metadata.path.clone(),
            resolution: self.video_metadata.resolution,
            frame_rate: self.video_metadata.frame_rate,
            duration: self.video_metadata.duration,
            codec: self.video_metadata.codec.clone(),
        };

        // Show docking UI (contains Viewport, Timeline, Inspector)
        let docking_response = self.docking.show(ctx, &mut self.world, &video_meta);

        // Handle dropped files from docking
        if let Some(path) = docking_response.dropped_files.into_iter().next() {
            self.pending_file = Some(path);
        }

        // Handle import button click from MediaBrowser
        if docking_response.import_requested {
            self.show_import_dialog();
        }

        // Handle keyboard shortcuts for file operations
        {
            // Ctrl+O = Open file
            if ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.ctrl) {
                self.show_open_dialog();
            }

            // Ctrl+I = Import media
            if ctx.input(|i| i.key_pressed(egui::Key::I) && i.modifiers.ctrl) {
                self.show_import_dialog();
            }
        }

        // Handle keyboard shortcuts for playback (outside of widgets)
        {
            let frame_rate = self.video_metadata.frame_rate.unwrap_or(30.0);
            let (is_playing, current_frame, playback_speed) = {
                let playhead = self.world.resource::<Playhead>();
                (
                    playhead.is_playing,
                    playhead.current_frame,
                    playhead.playback_speed,
                )
            };
            let command_queue = self.world.resource::<CommandQueue>();

            // Space = toggle play/pause
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                if is_playing {
                    command_queue.submit(Box::new(PauseCommand));
                } else {
                    command_queue.submit(Box::new(PlayCommand));
                }
            }

            // Home = stop (reset to beginning)
            if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                command_queue.submit(Box::new(StopCommand::with_frame_rate(frame_rate)));
            }

            // J/K/L shuttle controls
            if ctx.input(|i| i.key_pressed(egui::Key::J)) {
                let new_speed = if playback_speed > 0.0 {
                    -1.0
                } else {
                    (playback_speed - 1.0).clamp(-4.0, 4.0)
                };
                command_queue.submit(Box::new(SetSpeedCommand::new(new_speed)));
                if !is_playing {
                    command_queue.submit(Box::new(PlayCommand));
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::K)) {
                command_queue.submit(Box::new(PauseCommand));
            }

            if ctx.input(|i| i.key_pressed(egui::Key::L)) {
                let new_speed = if playback_speed < 0.0 {
                    1.0
                } else {
                    (playback_speed + 1.0).clamp(-4.0, 4.0)
                };
                command_queue.submit(Box::new(SetSpeedCommand::new(new_speed)));
                if !is_playing {
                    command_queue.submit(Box::new(PlayCommand));
                }
            }

            // Arrow keys for frame stepping
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                let target = current_frame.saturating_sub(1);
                command_queue.submit(Box::new(SeekCommand::new(target, frame_rate)));
            }

            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let target = current_frame.saturating_add(1);
                command_queue.submit(Box::new(SeekCommand::new(target, frame_rate)));
            }

            // S = split selected clips at playhead
            if ctx.input(|i| i.key_pressed(egui::Key::S)) && !ctx.input(|i| i.modifiers.ctrl) {
                let playhead_time = self.world.resource::<Playhead>().current_time;
                let selection = self
                    .world
                    .resource::<strawberry_core::resources::Selection>()
                    .clone();

                // Get selected clip entities that contain the playhead
                let mut clips_to_split: Vec<bevy_ecs::entity::Entity> = Vec::new();

                for &clip_bits in selection.selected.iter() {
                    let clip_entity = bevy_ecs::entity::Entity::from_bits(clip_bits);
                    if let Some(clip) = self.world.get::<Clip>(clip_entity) {
                        // Check if playhead is inside this clip's range
                        if clip.contains_time(playhead_time) {
                            clips_to_split.push(clip_entity);
                        }
                    }
                }

                // Submit split commands for each clip
                for clip_entity in clips_to_split {
                    command_queue
                        .submit(Box::new(SplitClipCommand::new(clip_entity, playhead_time)));
                    tracing::info!("Split clip at {:?}", playhead_time);
                }
            }

            // Delete = delete selected clips (non-ripple)
            if ctx.input(|i| i.key_pressed(egui::Key::Delete) && !i.modifiers.shift) {
                let selection = self
                    .world
                    .resource::<strawberry_core::resources::Selection>()
                    .clone();

                if !selection.is_empty() {
                    for &clip_bits in selection.selected.iter() {
                        let clip_entity = bevy_ecs::entity::Entity::from_bits(clip_bits);
                        // Only delete if it's a clip (verify entity has Clip component)
                        if self.world.get::<Clip>(clip_entity).is_some() {
                            command_queue.submit(Box::new(DeleteClipCommand::new(clip_entity)));
                            tracing::info!("Deleted clip {:?}", clip_entity);
                        }
                    }
                }
            }

            // Shift+Delete = ripple delete selected clips
            // Ripple delete removes clips AND shifts subsequent clips to fill the gap
            if ctx.input(|i| i.key_pressed(egui::Key::Delete) && i.modifiers.shift) {
                tracing::debug!("Shift+Delete pressed - ripple delete handler");
                let selection = self
                    .world
                    .resource::<strawberry_core::resources::Selection>()
                    .clone();

                tracing::debug!("Selection contains {} items", selection.selected.len());

                if !selection.is_empty() {
                    for &clip_bits in selection.selected.iter() {
                        let clip_entity = bevy_ecs::entity::Entity::from_bits(clip_bits);
                        // Only ripple delete if it's a clip
                        if self.world.get::<Clip>(clip_entity).is_some() {
                            command_queue.submit(Box::new(RippleDeleteCommand::new(clip_entity)));
                            tracing::info!("Ripple deleted clip {:?}", clip_entity);
                        } else {
                            tracing::warn!(
                                "Entity {:?} is not a Clip, skipping ripple delete",
                                clip_entity
                            );
                        }
                    }
                } else {
                    tracing::debug!("Selection is empty, nothing to ripple delete");
                }
            }

            // Ctrl+Z = Undo
            if ctx.input(|i| i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && !i.modifiers.shift)
            {
                let mut undo_stack = self.world.resource_mut::<UndoRedoStack>();
                if undo_stack.can_undo() {
                    if let Some(mut entry) = undo_stack.pop_for_undo() {
                        let undo_data = std::mem::replace(
                            &mut entry.undo_data,
                            strawberry_core::commands::UndoData::None,
                        );
                        drop(undo_stack); // Release borrow before calling undo

                        if let Err(e) = entry.command.undo(&mut self.world, undo_data) {
                            tracing::error!("Undo failed: {}", e);
                        } else {
                            tracing::info!("Undid: {}", entry.command.name());
                            // Push to redo stack
                            let mut undo_stack = self.world.resource_mut::<UndoRedoStack>();
                            undo_stack.push_to_redo(entry);
                        }
                    }
                }
            }

            // Ctrl+Shift+Z = Redo
            if ctx.input(|i| i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && i.modifiers.shift) {
                let mut undo_stack = self.world.resource_mut::<UndoRedoStack>();
                if undo_stack.can_redo() {
                    if let Some(mut entry) = undo_stack.pop_for_redo() {
                        drop(undo_stack); // Release borrow before calling execute

                        match entry.command.execute(&mut self.world) {
                            Ok(new_undo_data) => {
                                entry.undo_data = new_undo_data;
                                tracing::info!("Redid: {}", entry.command.name());

                                // NOTE: Entity ID remapping is no longer needed here.
                                // Commands now use UUID-based lookup via StableIdRegistry,
                                // so entity IDs are resolved at execution time.

                                // Push back to undo stack (without clearing redo stack)
                                let mut undo_stack = self.world.resource_mut::<UndoRedoStack>();
                                undo_stack.push_from_redo(entry);
                            }
                            Err(e) => {
                                tracing::error!("Redo failed: {}", e);
                            }
                        }
                    }
                }
            }
        }

        // Request continuous repaint during playback
        if self.world.resource::<Playhead>().is_playing {
            ctx.request_repaint();
        }
    }
}

// ============================================================================
// ASYNC ASSET IMPORT
// ============================================================================

/// Result sent from a background asset import thread to the main thread.
///
/// The background thread runs FFmpeg probe to get metadata, then sends this
/// result via an mpsc channel. The main thread polls the channel each frame.
struct AssetImportResult {
    /// Asset UUID assigned before spawning the thread
    uuid: uuid::Uuid,
    /// Original source file path
    path: PathBuf,
    /// Import outcome
    outcome: AssetImportOutcome,
}

/// Possible outcomes of a background asset import
enum AssetImportOutcome {
    /// Successfully loaded VideoService with metadata
    Success {
        service: strawberry_video::service::VideoService,
        metadata: strawberry_video::VideoMetadata,
    },
    /// Import failed
    Failed { error: String },
}

// ============================================================================
// ASYNC AUDIO EXTRACTION
// ============================================================================

/// Result sent from a background audio extraction thread to the main thread.
///
/// The background thread runs FFmpeg to extract audio, then sends this result
/// via an mpsc channel. The main thread polls the channel each frame.
struct AudioExtractionResult {
    /// Asset UUID this extraction belongs to
    asset_uuid: uuid::Uuid,
    /// Extraction outcome
    outcome: AudioExtractionOutcome,
}

/// Possible outcomes of a background audio extraction
enum AudioExtractionOutcome {
    /// Audio successfully extracted to this WAV path
    Success { wav_path: PathBuf },
    /// Source has no audio stream (not an error)
    NoAudioStream,
    /// Extraction failed
    Failed { error: String },
}

/// Spawn a background thread to extract audio from a media file.
///
/// This is the non-blocking counterpart of the old `extract_and_register_audio()`.
/// FFmpeg runs on a background thread via `std::thread::spawn`. When complete,
/// the result is sent back via the provided `mpsc::Sender`.
///
/// The main thread polls `audio_extraction_rx` each frame to pick up results.
fn spawn_audio_extraction(
    asset_uuid: uuid::Uuid,
    source_path: PathBuf,
    result_tx: mpsc::Sender<AudioExtractionResult>,
) {
    std::thread::spawn(move || {
        // 1. Initialize FFmpeg
        let ffmpeg = match strawberry_video::ffmpeg::FFmpeg::new() {
            Ok(f) => f,
            Err(e) => {
                let _ = result_tx.send(AudioExtractionResult {
                    asset_uuid,
                    outcome: AudioExtractionOutcome::Failed {
                        error: format!("FFmpeg not available: {}", e),
                    },
                });
                return;
            }
        };

        // 2. Check if source has audio at all
        if !ffmpeg.has_audio_stream(&source_path) {
            let _ = result_tx.send(AudioExtractionResult {
                asset_uuid,
                outcome: AudioExtractionOutcome::NoAudioStream,
            });
            return;
        }

        // 3. Compute cached WAV path
        let audio_cache_dir: PathBuf = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("strawberry")
            .join("audio_cache");

        let wav_filename: String = format!("{}.wav", asset_uuid.simple());
        let wav_path: PathBuf = audio_cache_dir.join(&wav_filename);

        // 4. Extract if not already cached
        if !wav_path.exists() {
            if let Err(e) = ffmpeg.extract_audio_to_wav(&source_path, &wav_path) {
                let _ = result_tx.send(AudioExtractionResult {
                    asset_uuid,
                    outcome: AudioExtractionOutcome::Failed {
                        error: format!("{}", e),
                    },
                });
                return;
            }
        }

        // 5. Send success back to main thread
        let _ = result_tx.send(AudioExtractionResult {
            asset_uuid,
            outcome: AudioExtractionOutcome::Success { wav_path },
        });
    });
}

impl StrawberryApp {
    /// Poll for completed audio extractions and register them with kira.
    ///
    /// Called once per frame at the start of `update()`. Non-blocking — uses
    /// `try_recv()` to drain all completed extractions without waiting.
    fn poll_audio_extractions(&mut self) {
        while let Ok(result) = self.audio_extraction_rx.try_recv() {
            match result.outcome {
                AudioExtractionOutcome::Success { wav_path } => {
                    // Update asset registry
                    {
                        let mut registry = self
                            .world
                            .resource_mut::<strawberry_core::resources::AssetRegistry>();
                        if let Some(asset) = registry.get_mut(&result.asset_uuid) {
                            asset.set_audio_ready(wav_path.clone());
                        }
                    }

                    // Register the extracted WAV with kira mixer
                    if self.world.contains_resource::<AudioMixerResource>() {
                        let mut mixer = self.world.resource_mut::<AudioMixerResource>();
                        match mixer.register_clip_audio(result.asset_uuid, &wav_path) {
                            Ok(()) => {
                                tracing::info!(
                                    "Audio ready for clip {} (via background extraction)",
                                    result.asset_uuid
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to register extracted audio: {}", e);
                            }
                        }
                    }
                }
                AudioExtractionOutcome::NoAudioStream => {
                    let mut registry = self
                        .world
                        .resource_mut::<strawberry_core::resources::AssetRegistry>();
                    if let Some(asset) = registry.get_mut(&result.asset_uuid) {
                        asset.set_no_audio_stream();
                    }
                    tracing::info!(
                        "No audio stream in clip {} — marked as ready",
                        result.asset_uuid
                    );
                }
                AudioExtractionOutcome::Failed { error } => {
                    let mut registry = self
                        .world
                        .resource_mut::<strawberry_core::resources::AssetRegistry>();
                    if let Some(asset) = registry.get_mut(&result.asset_uuid) {
                        asset.set_audio_failed(error.clone());
                    }
                    tracing::warn!(
                        "Audio extraction failed for {}: {}",
                        result.asset_uuid,
                        error
                    );
                }
            }
        }
    }

    /// Sync proxy completion status from VideoAssetManager to AssetRegistry.
    ///
    /// VideoAssetManager tracks proxy generation internally but doesn't have
    /// access to AssetRegistry (separate ECS Resource). This method bridges
    /// the gap by draining completed proxy IDs each frame and updating the
    /// registry so `is_ready_for_timeline()` reflects actual readiness.
    fn sync_proxy_completions_to_registry(&mut self) {
        // Drain completed proxies from VideoAssetManager
        let completions: Vec<(uuid::Uuid, PathBuf)> = {
            let mut asset_manager = self.world.resource_mut::<VideoAssetManager>();
            asset_manager.drain_proxy_completions()
        };

        if completions.is_empty() {
            return;
        }

        // Update AssetRegistry for each completed proxy
        let mut registry = self
            .world
            .resource_mut::<strawberry_core::resources::AssetRegistry>();
        for (asset_uuid, proxy_path) in completions {
            if let Some(asset) = registry.get_mut(&asset_uuid) {
                asset.set_proxy_ready(proxy_path);
                tracing::info!(
                    "AssetRegistry: proxy status set to Ready for {}",
                    &asset_uuid.to_string()[..8]
                );
            }
        }
    }
}
