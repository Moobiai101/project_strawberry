//! # Video Asset Manager
//!
//! Production-grade multi-asset video decode manager for timeline compositing.
//!
//! ## Responsibilities
//!
//! - **Asset loading**: Loads video files and creates VideoService instances
//! - **Dual decode paths**: Streaming (playback) vs random-access (scrubbing)
//! - **Proxy management**: Generates 540p JPEG proxies for instant scrubbing
//! - **Frame caching**: LRU cache for instant frame re-access
//!
//! ## Performance Hierarchy
//!
//! 1. **LRU Cache**: ~0.01ms - instant access for recently viewed frames
//! 2. **JPEG Proxy**: ~2-5ms - image sequence load from disk
//! 3. **Video Decode**: ~100ms - full decoder fallback

use bevy_ecs::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use strawberry_video::jpeg_cache::JpegFrameCache;
use strawberry_video::proxy::{ProxyManager, ProxyResult};
use strawberry_video::service::{FrameStatus, VideoService};

// ============================================================================
// VIDEO ASSET MANAGER
// ============================================================================

/// Manages multiple video assets for timeline compositing.
///
/// Provides two decode paths:
/// - **Playback mode**: Uses PipeDecoder for continuous streaming (~1ms/frame)
/// - **Random access mode**: Uses subprocess decoder for scrubbing (~100ms/frame)
///
/// Architecture: Single source of truth
/// - All assets stored in `services` HashMap (one VideoService per asset)
/// - `active_asset_uuid` tracks which asset is currently being played/displayed
/// - No duplicate instances - eliminates synchronization issues
#[derive(Resource)]
pub struct VideoAssetManager {
    /// Video services keyed by asset UUID (single source of truth)
    services: HashMap<Uuid, VideoService>,
    /// Path to UUID mapping (for loading)
    path_to_uuid: HashMap<PathBuf, Uuid>,
    /// Original source paths (for export, not scrubbing)
    source_paths: HashMap<Uuid, PathBuf>,
    /// Proxy paths keyed by asset UUID (used for scrubbing)
    proxy_paths: HashMap<Uuid, PathBuf>,
    /// Currently active asset UUID (the one being played/displayed in viewport)
    active_asset_uuid: Option<Uuid>,
    /// Whether playback mode is active (uses PipeDecoder)
    playback_active: bool,
    /// Proxy manager for generating edit-friendly proxies
    proxy_manager: Option<ProxyManager>,
    /// LRU cache for JPEG frames (instant re-access during scrubbing)
    frame_cache: JpegFrameCache,
    /// Buffer of asset UUIDs whose proxy just completed.
    /// Drained by main.rs to sync `AssetRegistry.proxy_status` to `Ready`.
    pending_proxy_completions: Vec<(Uuid, PathBuf)>,
    /// Paths currently being loaded in background threads (prevents duplicate imports)
    pending_imports: HashSet<PathBuf>,
}

impl Default for VideoAssetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoAssetManager {
    /// Create a new empty asset manager
    pub fn new() -> Self {
        // Try to create proxy manager (may fail if FFmpeg not found)
        let proxy_manager: Option<ProxyManager> = match ProxyManager::new() {
            Ok(pm) => {
                tracing::info!("ProxyManager initialized, cache dir: {:?}", pm.cache_dir());
                Some(pm)
            }
            Err(e) => {
                tracing::warn!(
                    "ProxyManager unavailable: {}. Proxy generation disabled.",
                    e
                );
                None
            }
        };

        // Create LRU cache for JPEG frames (128MB — holds ~60 proxy frames at 720p).
        // Doubled from 64MB to reduce cache misses during fast scrubbing.
        // At proxy resolution (1280×720 RGBA = ~3.5MB/frame), this holds ~36 frames.
        // The extra memory cost is negligible on modern systems.
        let frame_cache: JpegFrameCache = JpegFrameCache::new(128);

        Self {
            services: HashMap::new(),
            path_to_uuid: HashMap::new(),
            source_paths: HashMap::new(),
            proxy_paths: HashMap::new(),
            active_asset_uuid: None,
            playback_active: false,
            proxy_manager,
            frame_cache,
            pending_proxy_completions: Vec::new(),
            pending_imports: HashSet::new(),
        }
    }

    /// Load a video asset and return its UUID
    ///
    /// This will:
    /// 1. Create a VideoService for the asset (single source of truth)
    /// 2. Queue proxy generation (for instant scrubbing)
    /// 3. Set as active asset ONLY if no active asset exists yet
    /// 4. Return the asset UUID
    ///
    /// ## Multi-Asset Safety
    ///
    /// Importing a second video does NOT change the active asset. The compositor
    /// uses per-clip asset UUIDs from `ClipAssetRef`, so the active asset is only
    /// used as a fallback for metadata queries (e.g. `active_service()`).
    pub fn load_asset(&mut self, path: &std::path::Path) -> Result<Uuid, String> {
        // Check if already loaded - return existing UUID without changing active
        if let Some(uuid) = self.path_to_uuid.get(path) {
            return Ok(*uuid);
        }

        // Create new service for this asset (single instance per asset)
        let mut service: VideoService = VideoService::new().map_err(|e| e.to_string())?;
        let metadata = service.load(path).map_err(|e| e.to_string())?;

        let uuid: Uuid = Uuid::new_v4();
        self.path_to_uuid.insert(path.to_path_buf(), uuid);
        self.source_paths.insert(uuid, path.to_path_buf());
        self.services.insert(uuid, service);

        // Queue proxy generation (if proxy manager available)
        // Proxies use JPEG image sequences for instant scrubbing
        if let Some(ref proxy_manager) = self.proxy_manager {
            let proxy_path =
                proxy_manager.queue_proxy(uuid, path, metadata.duration, metadata.frame_rate);
            tracing::info!(
                "Queued proxy generation for asset {}: {:?}",
                &uuid.to_string()[..8],
                proxy_path
            );
        }

        // Only set active asset on FIRST load. Subsequent imports must not
        // steal the active slot — the compositor resolves assets by UUID,
        // not by "active" designation.
        if self.active_asset_uuid.is_none() {
            self.active_asset_uuid = Some(uuid);
        }

        Ok(uuid)
    }

    /// Register a pre-loaded VideoService created on a background thread.
    ///
    /// This is called by the main thread after a background import thread
    /// completes. The `VideoService` was created and loaded on a background
    /// thread to avoid blocking the UI during FFmpeg probe (~200-500ms).
    ///
    /// ## Arguments
    ///
    /// - `uuid`: The UUID assigned to this asset (generated before spawning the thread)
    /// - `service`: The fully loaded VideoService (with metadata already probed)
    /// - `path`: Original source file path
    /// - `duration`: Video duration from probe (needed for proxy queue)
    /// - `frame_rate`: Video frame rate from probe (needed for proxy queue)
    pub fn register_loaded_service(
        &mut self,
        uuid: Uuid,
        service: VideoService,
        path: PathBuf,
        duration: Duration,
        frame_rate: f64,
    ) {
        // Insert service into the manager
        self.path_to_uuid.insert(path.clone(), uuid);
        self.source_paths.insert(uuid, path.clone());
        self.services.insert(uuid, service);

        // Queue proxy generation (if proxy manager available)
        if let Some(ref proxy_manager) = self.proxy_manager {
            let proxy_path = proxy_manager.queue_proxy(uuid, &path, duration, frame_rate);
            tracing::info!(
                "Queued proxy generation for background-loaded asset {}: {:?}",
                &uuid.to_string()[..8],
                proxy_path
            );
        }

        // Only set active asset on FIRST load
        if self.active_asset_uuid.is_none() {
            self.active_asset_uuid = Some(uuid);
        }

        // Clear from pending imports
        self.pending_imports.remove(&path);

        tracing::info!(
            "Registered background-loaded asset {}: {:?}",
            &uuid.to_string()[..8],
            path
        );
    }

    /// Mark a path as currently being loaded in a background thread.
    ///
    /// Returns `false` if the path is already loading or already loaded.
    pub fn mark_import_pending(&mut self, path: &std::path::Path) -> bool {
        // Already loaded?
        if self.path_to_uuid.contains_key(path) {
            return false;
        }
        // Already being loaded in background?
        if self.pending_imports.contains(path) {
            return false;
        }
        self.pending_imports.insert(path.to_path_buf());
        true
    }

    /// Clear a path from pending imports (called on import failure)
    pub fn clear_import_pending(&mut self, path: &std::path::Path) {
        self.pending_imports.remove(path);
    }

    /// Check if a path is currently being loaded in a background thread
    pub fn is_import_pending(&self, path: &std::path::Path) -> bool {
        self.pending_imports.contains(path)
    }

    /// Get the currently active asset UUID
    pub fn active_asset(&self) -> Option<Uuid> {
        self.active_asset_uuid
    }

    /// Set the active asset (for playback/viewport display)
    pub fn set_active_asset(&mut self, uuid: Uuid) {
        if self.services.contains_key(&uuid) {
            self.active_asset_uuid = Some(uuid);
        } else {
            tracing::warn!("Attempted to set non-existent asset {} as active", uuid);
        }
    }

    /// Get a service for a specific asset
    pub fn get_service(&self, asset_uuid: &Uuid) -> Option<&VideoService> {
        self.services.get(asset_uuid)
    }

    /// Get mutable service for a specific asset
    pub fn get_service_mut(&mut self, asset_uuid: &Uuid) -> Option<&mut VideoService> {
        self.services.get_mut(asset_uuid)
    }

    /// Get the active service (the one being played/displayed in viewport)
    pub fn active_service(&self) -> Option<&VideoService> {
        self.active_asset_uuid
            .and_then(|uuid| self.services.get(&uuid))
    }

    /// Get mutable access to active service
    pub fn active_service_mut(&mut self) -> Option<&mut VideoService> {
        if let Some(uuid) = self.active_asset_uuid {
            self.services.get_mut(&uuid)
        } else {
            None
        }
    }

    // =========================================================================
    // PLAYBACK MODE CONTROL
    // =========================================================================

    /// Start playback mode for ALL loaded assets.
    ///
    /// Activates PipeDecoder streaming on every loaded VideoService so the
    /// compositor can request frames from ANY clip on the timeline, not just
    /// whichever video happens to be "active".
    ///
    /// ## Why ALL services?
    ///
    /// A multi-clip timeline may have clips from different source videos
    /// overlapping at the playhead. The compositor requests frames by asset
    /// UUID, so every service must be streaming during playback.
    pub fn start_playback(&mut self, start_time: Duration) -> Result<(), String> {
        if self.playback_active {
            return Ok(()); // Already in playback mode
        }

        if self.services.is_empty() {
            return Err("No assets loaded for playback".to_string());
        }

        tracing::info!(
            "VideoAssetManager: Starting playback mode at {:.2}s for {} service(s)",
            start_time.as_secs_f64(),
            self.services.len()
        );

        // Start playback on ALL loaded services so any clip can provide frames
        let mut errors: Vec<String> = Vec::new();
        for (uuid, service) in self.services.iter_mut() {
            if let Err(e) = service.start_playback(start_time) {
                tracing::warn!(
                    "Failed to start playback for asset {}: {}",
                    &uuid.to_string()[..8],
                    e
                );
                errors.push(format!("{}: {}", &uuid.to_string()[..8], e));
            }
        }

        self.playback_active = true;

        // NOTE: Proxy FFmpeg processes are NOT suspended during playback.
        // They run at BELOW_NORMAL_PRIORITY_CLASS, which lets the OS scheduler
        // automatically give priority to the playback pipeline while still
        // making progress on proxy generation. Suspending them was freezing
        // proxy work for the entire playback duration (72s+ wasted time).
        //
        // The suspend/resume APIs in process_control.rs remain available
        // for future use (e.g., during export where we DO want full GPU/CPU).

        // Log errors but don't fail — partial playback is better than none
        if !errors.is_empty() {
            tracing::warn!(
                "Playback started with {} error(s): {}",
                errors.len(),
                errors.join("; ")
            );
        }

        Ok(())
    }

    /// Stop playback mode for ALL services.
    ///
    /// Returns to random-access mode for scrubbing/seeking.
    ///
    /// ## Hold-Frame Bridge
    ///
    /// Before destroying each PipeDecoder, grabs its last decoded frame and
    /// inserts it into the JPEG LRU cache. This ensures the FIRST random-access
    /// request after pause gets an instant cache hit (~0.01ms) instead of
    /// triggering an async decode that takes 50-100ms — eliminating the black
    /// frame flash on the play→pause transition.
    ///
    /// Iterates `services` (one per unique video asset, NOT per clip), so this
    /// scales to 1000+ clips without performance issues.
    pub fn stop_playback(&mut self) {
        if !self.playback_active {
            return;
        }

        tracing::debug!("VideoAssetManager: Stopping playback mode");

        // CHANGE 3: Cache the last playback frame BEFORE destroying PipeDecoders.
        //
        // The PipeDecoder's internal buffer has the last decoded frame. Once
        // stop_playback() is called, the buffer is destroyed and the frame is
        // lost. Grabbing it now and inserting into the LRU cache means the
        // compositor's first random-access request will get an instant hit.
        let asset_uuids: Vec<Uuid> = self.services.keys().copied().collect();
        for asset_uuid in &asset_uuids {
            if let Some(service) = self.services.get(asset_uuid) {
                if let Some(last_frame) = service.request_latest_playback_frame() {
                    // Approximate the frame number from the frame's timestamp
                    let frame_number: u32 = (last_frame.timestamp.as_secs_f64() * 30.0) as u32;
                    let cache_key: (Uuid, u32) = (*asset_uuid, frame_number);
                    self.frame_cache.insert_video_frame(cache_key, &last_frame);
                    tracing::debug!(
                        "Cached last playback frame for asset {} (frame {})",
                        &asset_uuid.to_string()[..8],
                        frame_number
                    );
                }
            }
        }

        // Now stop playback on ALL services (mirrors start_playback symmetry)
        for service in self.services.values_mut() {
            service.stop_playback();
        }

        self.playback_active = false;

        // NOTE: No resume needed — proxy processes are no longer suspended
        // during playback. See start_playback() for rationale.
    }

    /// Check if playback mode is active
    pub fn is_playback_mode(&self) -> bool {
        self.playback_active
    }

    /// Set scrubbing mode on all services
    ///
    /// When scrubbing, services use optimized decode path that clears stale requests.
    pub fn set_scrubbing(&mut self, is_scrubbing: bool) {
        for service in self.services.values_mut() {
            service.set_scrubbing(is_scrubbing);
        }
    }

    // =========================================================================
    // FRAME REQUESTS
    // =========================================================================

    /// Request a frame from an asset at a specific source time
    ///
    /// Automatically selects the appropriate decode path:
    /// - Playback mode: Uses PipeDecoder streaming (fast)
    /// - Random access: Uses JPEG cache + proxy loader (instant)
    pub fn request_frame(&mut self, asset_uuid: Uuid, source_time: Duration) -> FrameStatus {
        if self.playback_active {
            self.request_frame_playback(asset_uuid, source_time)
        } else {
            self.request_frame_random_access(asset_uuid, source_time)
        }
    }

    /// Request a frame for a compositor layer, respecting time remapping.
    ///
    /// When a clip has time remapping active (speed ≠ 1.0, reverse, or loop
    /// mode), the PipeDecoder's sequential stream cannot serve the non-linear
    /// source timestamps. In that case we force the random-access decode path
    /// (JPEG proxy + LRU cache), which can seek to arbitrary source times.
    ///
    /// **Critical**: When routing to random-access during playback, we also
    /// stop the pipe decoder for that asset. The pipe decoder subprocess
    /// consumes CPU/memory and causes contention with JPEG proxy I/O,
    /// which is the root cause of the viewport freeze bug.
    ///
    /// Normal clips (no time remapping) use the standard path — pipe decoder
    /// during playback, random-access when paused/scrubbing.
    pub fn request_frame_for_layer(
        &mut self,
        asset_uuid: Uuid,
        source_time: Duration,
        needs_random_access: bool,
    ) -> FrameStatus {
        if needs_random_access {
            // Time remapping active — pipe decoder cannot serve non-linear timestamps.
            // Force random-access path which can seek to any source time.
            //
            // CRITICAL: Stop the pipe decoder for this asset if it's running.
            // The pipe decoder subprocess (FFmpeg) consumes CPU and competes for
            // I/O bandwidth with JPEG proxy file reads. This resource contention
            // causes the viewport freeze that was reported.
            if self.playback_active {
                if let Some(service) = self.services.get_mut(&asset_uuid) {
                    if service.is_playback_mode() {
                        tracing::info!(
                            "Stopping pipe decoder for asset {} (using random-access path)",
                            &asset_uuid.to_string()[..8]
                        );
                        service.stop_playback();
                    }
                }
            }
            self.request_frame_random_access(asset_uuid, source_time)
        } else if self.playback_active {
            self.request_frame_playback(asset_uuid, source_time)
        } else {
            self.request_frame_random_access(asset_uuid, source_time)
        }
    }

    /// Request frame using PipeDecoder streaming (for playback)
    ///
    /// Requests a frame from the specific asset's service. Never falls back to
    /// a different asset — showing a loading state is correct, showing the wrong
    /// video is a critical bug.
    fn request_frame_playback(&mut self, asset_uuid: Uuid, source_time: Duration) -> FrameStatus {
        if let Some(service) = self.services.get_mut(&asset_uuid) {
            if let Some(frame) = service.request_playback_frame_for_time(source_time) {
                return FrameStatus::Ready(frame);
            }
            // Service exists but buffer hasn't caught up yet — this is normal
            // during the first few frames of playback or after a pipe re-seek.
            FrameStatus::Loading
        } else {
            // Asset not loaded at all — this is an error, not a loading state
            FrameStatus::Error(format!("Asset {} not loaded for playback", asset_uuid))
        }
    }

    /// Request frame using LRU cache + JPEG proxy (for scrubbing)
    ///
    /// **Performance Hierarchy:**
    /// 1. LRU Cache check (~0.01ms) - INSTANT
    /// 2. JPEG proxy load (~2-5ms) + cache insert
    /// 3. Video decoder fallback (~100ms)
    fn request_frame_random_access(
        &mut self,
        asset_uuid: Uuid,
        source_time: Duration,
    ) -> FrameStatus {
        // =====================================================================
        // 1. CHECK LRU CACHE FIRST (0.01ms - instant!)
        // =====================================================================
        let frame_number = (source_time.as_secs_f64() * 30.0) as u32; // Approximate
        let cache_key = (asset_uuid, frame_number);

        if let Some(cached_frame) = self.frame_cache.get(&cache_key) {
            return FrameStatus::Ready(cached_frame.to_video_frame());
        }

        // =====================================================================
        // 2. TRY IMAGE SEQUENCE PROXY (2-5ms)
        // =====================================================================
        if let Some(ref proxy_manager) = self.proxy_manager {
            if proxy_manager.is_image_sequence(&asset_uuid) {
                let time_secs = source_time.as_secs_f64();
                if let Some(frame_path) = proxy_manager.get_frame_path(&asset_uuid, time_secs) {
                    if let Some(frame) =
                        strawberry_video::service::load_frame_from_jpeg(&frame_path)
                    {
                        // Insert into cache for next access
                        self.frame_cache.insert_video_frame(cache_key, &frame);
                        return FrameStatus::Ready(frame);
                    } else {
                        tracing::warn!(
                            "JPEG proxy file exists but failed to load: {:?}",
                            frame_path
                        );
                    }
                }
            }
        }

        // =====================================================================
        // 3. FALLBACK: Use video decoder (slower path)
        // =====================================================================
        if let Some(service) = self.services.get(&asset_uuid) {
            service.request_frame(source_time)
        } else {
            FrameStatus::Error(format!("Asset {} not loaded", asset_uuid))
        }
    }

    /// Poll all services for results AND poll proxy generation results.
    ///
    /// Also clears the per-service failed-request caches so that changed
    /// clip properties (speed, reverse, loop) produce different source_times
    /// that get fresh decode attempts on the next frame.
    pub fn poll_results(&mut self) {
        for service in self.services.values_mut() {
            // Clear failed timestamps from the previous frame so new
            // source_times (from changed properties) get fresh attempts.
            // Within this frame, any timestamp that fails will be added
            // back, preventing the infinite retry loop.
            service.clear_failed_requests();
            service.poll_results();
        }

        // Poll proxy generation results
        self.poll_proxy_results();
    }

    /// Poll for completed proxy generation results
    ///
    /// Updates proxy_paths when a proxy is ready, and reloads the service
    /// with the proxy file for instant scrubbing.
    fn poll_proxy_results(&mut self) {
        if let Some(ref proxy_manager) = self.proxy_manager {
            for result in proxy_manager.poll_results() {
                match result {
                    ProxyResult::Completed {
                        asset_id,
                        proxy_path,
                    } => {
                        tracing::info!(
                            "Proxy ready for asset {}: {:?}",
                            &asset_id.to_string()[..8],
                            proxy_path
                        );
                        self.proxy_paths.insert(asset_id, proxy_path.clone());
                        self.pending_proxy_completions
                            .push((asset_id, proxy_path.clone()));

                        // For VIDEO proxies (.mov), reload the service to use proxy.
                        // For IMAGE SEQUENCE proxies (directory of JPEGs), do NOT reload —
                        // image sequences are accessed frame-by-frame via
                        // proxy_manager.get_frame_path() in request_frame_random_access().
                        if !proxy_path.is_dir() {
                            if let Some(service) = self.services.get_mut(&asset_id) {
                                if let Err(e) = service.load(&proxy_path) {
                                    tracing::warn!(
                                        "Failed to load video proxy for asset {}: {}",
                                        &asset_id.to_string()[..8],
                                        e
                                    );
                                } else {
                                    tracing::info!(
                                        "Switched asset {} to use video proxy for scrubbing",
                                        &asset_id.to_string()[..8]
                                    );
                                }
                            }
                        }
                    }
                    ProxyResult::Progress { asset_id, progress } => {
                        tracing::debug!(
                            "Proxy progress for {}: {:.0}%",
                            &asset_id.to_string()[..8],
                            progress * 100.0
                        );
                    }
                    ProxyResult::Failed { asset_id, error } => {
                        tracing::error!(
                            "Proxy generation failed for asset {}: {}",
                            &asset_id.to_string()[..8],
                            error
                        );
                    }
                }
            }
        }
    }

    /// Check if a proxy is ready for an asset
    pub fn has_proxy(&self, asset_uuid: &Uuid) -> bool {
        self.proxy_paths.contains_key(asset_uuid)
    }

    /// Get the source path for an asset (for export - always use original)
    pub fn get_source_path(&self, asset_uuid: &Uuid) -> Option<&PathBuf> {
        self.source_paths.get(asset_uuid)
    }

    /// Drain completed proxy notifications.
    ///
    /// Called by main.rs each frame to sync `AssetRegistry.proxy_status` to `Ready`.
    /// Returns (asset_uuid, proxy_path) pairs for all newly-completed proxies
    /// since last drain.
    pub fn drain_proxy_completions(&mut self) -> Vec<(Uuid, PathBuf)> {
        std::mem::take(&mut self.pending_proxy_completions)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_asset_manager_new() {
        let manager = VideoAssetManager::new();
        assert!(manager.active_service().is_none());
        assert!(manager.active_asset().is_none());
        assert!(manager.services.is_empty());
    }
}
