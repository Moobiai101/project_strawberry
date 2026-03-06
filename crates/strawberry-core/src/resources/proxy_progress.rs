//! # Proxy Progress Resource
//!
//! Tracks progress of proxy generation across all assets.
//! Used by the status bar UI to display overall proxy status.
//!
//! ## Architecture
//!
//! This resource is read by the UI (status bar) and written by the
//! proxy generation system. It aggregates per-asset progress into
//! an overall completion percentage.
//!
//! ## Usage
//!
//! ```ignore
//! // In proxy system:
//! let mut progress = world.resource_mut::<ProxyProgress>();
//! progress.update_asset(asset_id, 0.5, "Encoding frame 150/300");
//!
//! // In status bar UI:
//! let progress = world.resource::<ProxyProgress>();
//! if progress.is_active() {
//!     draw_progress_bar(progress.overall_progress());
//! }
//! ```

use bevy_ecs::system::Resource;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Progress state for a single asset's proxy generation
#[derive(Debug, Clone)]
pub struct AssetProxyProgress {
    /// Current progress (0.0 to 1.0)
    pub progress: f32,
    /// Status message (e.g., "Encoding frame 150/300")
    pub status_message: String,
    /// When this asset started generating
    pub started_at: Instant,
    /// Last update time
    pub last_update: Instant,
}

impl AssetProxyProgress {
    /// Create new progress entry
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            progress: 0.0,
            status_message: "Starting...".to_string(),
            started_at: now,
            last_update: now,
        }
    }

    /// Update progress
    pub fn update(&mut self, progress: f32, message: impl Into<String>) {
        self.progress = progress.clamp(0.0, 1.0);
        self.status_message = message.into();
        self.last_update = Instant::now();
    }

    /// Get elapsed time since start
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Estimate time remaining based on current progress
    pub fn estimated_remaining(&self) -> Option<Duration> {
        if self.progress <= 0.0 || self.progress >= 1.0 {
            return None;
        }
        let elapsed = self.elapsed().as_secs_f64();
        let total_estimate = elapsed / self.progress as f64;
        let remaining = total_estimate - elapsed;
        Some(Duration::from_secs_f64(remaining.max(0.0)))
    }
}

impl Default for AssetProxyProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Global proxy progress tracker
///
/// Aggregates progress from all assets currently generating proxies.
/// The status bar reads this to show overall progress.
#[derive(Resource, Default)]
pub struct ProxyProgress {
    /// Per-asset progress
    assets_in_progress: HashMap<Uuid, AssetProxyProgress>,
    /// Total number of assets queued (including completed)
    total_queued: usize,
    /// Number of completed assets
    completed: usize,
    /// Last completion time (for showing "Proxy generation complete" briefly)
    last_completion: Option<Instant>,
}

impl ProxyProgress {
    /// Create empty progress tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking a new asset
    pub fn start_asset(&mut self, asset_uuid: Uuid) {
        self.assets_in_progress
            .insert(asset_uuid, AssetProxyProgress::new());
        self.total_queued += 1;
        tracing::debug!(
            "Proxy progress: started asset {}, queue: {}/{}",
            &asset_uuid.to_string()[..8],
            self.assets_in_progress.len(),
            self.total_queued
        );
    }

    /// Update progress for an asset
    pub fn update_asset(&mut self, asset_uuid: Uuid, progress: f32, message: impl Into<String>) {
        if let Some(entry) = self.assets_in_progress.get_mut(&asset_uuid) {
            entry.update(progress, message);
        }
    }

    /// Mark an asset as complete
    pub fn complete_asset(&mut self, asset_uuid: &Uuid) {
        if self.assets_in_progress.remove(asset_uuid).is_some() {
            self.completed += 1;
            tracing::debug!(
                "Proxy progress: completed asset {}, {}/{} done",
                &asset_uuid.to_string()[..8],
                self.completed,
                self.total_queued
            );

            // If all done, record completion time
            if self.assets_in_progress.is_empty() {
                self.last_completion = Some(Instant::now());
                tracing::info!(
                    "Proxy generation complete: {} assets processed",
                    self.completed
                );
            }
        }
    }

    /// Mark an asset as failed
    pub fn fail_asset(&mut self, asset_uuid: &Uuid, error: impl Into<String>) {
        if let Some(entry) = self.assets_in_progress.get_mut(asset_uuid) {
            entry.status_message = format!("Failed: {}", error.into());
        }
        // Keep in progress for a bit so the error is visible
    }

    /// Check if any proxy generation is active
    pub fn is_active(&self) -> bool {
        !self.assets_in_progress.is_empty()
    }

    /// Check if generation recently completed (within last 3 seconds)
    pub fn recently_completed(&self) -> bool {
        self.last_completion
            .map(|t| t.elapsed() < Duration::from_secs(3))
            .unwrap_or(false)
    }

    /// Get overall progress (0.0 to 1.0)
    ///
    /// Calculated as: completed_assets + sum(in_progress_percentages) / total
    pub fn overall_progress(&self) -> f32 {
        if self.total_queued == 0 {
            return 1.0;
        }

        let completed_weight: f32 = self.completed as f32;
        let in_progress_weight: f32 = self.assets_in_progress.values().map(|p| p.progress).sum();

        (completed_weight + in_progress_weight) / self.total_queued as f32
    }

    /// Get number of assets currently in progress
    pub fn active_count(&self) -> usize {
        self.assets_in_progress.len()
    }

    /// Get total queued count
    pub fn total_count(&self) -> usize {
        self.total_queued
    }

    /// Get completed count
    pub fn completed_count(&self) -> usize {
        self.completed
    }

    /// Get current status message for display
    ///
    /// Returns a summary like "Generating proxies (2/5)..." or per-asset status
    pub fn status_message(&self) -> String {
        if self.assets_in_progress.is_empty() {
            if self.recently_completed() {
                return format!("✓ {} proxies generated", self.completed);
            }
            return String::new();
        }

        // Get the most recently updated asset's status
        if let Some((_, entry)) = self
            .assets_in_progress
            .iter()
            .max_by_key(|(_, p)| p.last_update)
        {
            format!(
                "Generating proxies ({}/{})... {}",
                self.completed + 1,
                self.total_queued,
                entry.status_message
            )
        } else {
            format!(
                "Generating proxies ({}/{})...",
                self.completed + 1,
                self.total_queued
            )
        }
    }

    /// Clear all progress (call when switching projects)
    pub fn clear(&mut self) {
        self.assets_in_progress.clear();
        self.total_queued = 0;
        self.completed = 0;
        self.last_completion = None;
    }

    /// Get estimated time remaining for all assets
    pub fn estimated_total_remaining(&self) -> Option<Duration> {
        if self.assets_in_progress.is_empty() {
            return None;
        }

        // Average remaining time weighted by incomplete portion
        let total_remaining: f64 = self
            .assets_in_progress
            .values()
            .filter_map(|p| p.estimated_remaining())
            .map(|d| d.as_secs_f64())
            .sum();

        if total_remaining > 0.0 {
            Some(Duration::from_secs_f64(total_remaining))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_lifecycle() {
        let mut progress = ProxyProgress::new();
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();

        assert!(!progress.is_active());
        assert_eq!(progress.overall_progress(), 1.0);

        // Start first asset
        progress.start_asset(uuid1);
        assert!(progress.is_active());
        assert_eq!(progress.active_count(), 1);
        assert_eq!(progress.overall_progress(), 0.0);

        // Start second asset
        progress.start_asset(uuid2);
        assert_eq!(progress.active_count(), 2);
        assert_eq!(progress.total_count(), 2);

        // Update progress
        progress.update_asset(uuid1, 0.5, "50% done");
        assert!((progress.overall_progress() - 0.25).abs() < 0.01);

        // Complete first
        progress.complete_asset(&uuid1);
        assert_eq!(progress.completed_count(), 1);
        assert_eq!(progress.active_count(), 1);
        assert!((progress.overall_progress() - 0.5).abs() < 0.01);

        // Complete second
        progress.update_asset(uuid2, 1.0, "Done");
        progress.complete_asset(&uuid2);
        assert!(!progress.is_active());
        assert!(progress.recently_completed());
    }

    #[test]
    fn test_status_message() {
        let mut progress = ProxyProgress::new();
        let uuid = Uuid::new_v4();

        progress.start_asset(uuid);
        progress.update_asset(uuid, 0.3, "Encoding frame 90/300");

        let msg = progress.status_message();
        assert!(msg.contains("1/1"));
        assert!(msg.contains("Encoding frame"));
    }

    #[test]
    fn test_clear() {
        let mut progress = ProxyProgress::new();
        progress.start_asset(Uuid::new_v4());
        progress.start_asset(Uuid::new_v4());

        progress.clear();

        assert!(!progress.is_active());
        assert_eq!(progress.total_count(), 0);
        assert_eq!(progress.completed_count(), 0);
    }
}
