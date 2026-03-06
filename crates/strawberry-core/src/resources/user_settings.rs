//! # User Settings Resource
//!
//! Persistent user preferences for the application.
//! Includes proxy generation settings, UI preferences, and paths.
//!
//! ## Persistence
//!
//! Settings are serialized to JSON and stored in the user's config directory.
//! The file is loaded on startup and saved when settings change.
//!
//! ## Architecture
//!
//! This is a pure data resource - no UI code. The settings panel
//! in strawberry-ui reads and writes to this resource.

use bevy_ecs::system::Resource;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Proxy resolution preset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyResolution {
    /// 480p (854x480) - fastest, smallest files
    Res480p,
    /// 720p (1280x720) - good balance
    Res720p,
    /// 1080p (1920x1080) - high quality, larger files
    Res1080p,
    /// Original resolution - no scaling
    Original,
}

impl ProxyResolution {
    /// Get pixel dimensions (width, height)
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            ProxyResolution::Res480p => Some((854, 480)),
            ProxyResolution::Res720p => Some((1280, 720)),
            ProxyResolution::Res1080p => Some((1920, 1080)),
            ProxyResolution::Original => None,
        }
    }

    /// Human-readable label
    pub fn label(&self) -> &'static str {
        match self {
            ProxyResolution::Res480p => "480p (Fast)",
            ProxyResolution::Res720p => "720p (Balanced)",
            ProxyResolution::Res1080p => "1080p (Quality)",
            ProxyResolution::Original => "Original",
        }
    }
}

impl Default for ProxyResolution {
    fn default() -> Self {
        ProxyResolution::Res720p
    }
}

/// JPEG quality for proxy image sequences
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyQuality {
    /// ~70% quality - smaller files, faster
    Low,
    /// ~85% quality - good balance (default)
    Medium,
    /// ~95% quality - best quality, larger files
    High,
}

impl ProxyQuality {
    /// Get JPEG quality value (0-100)
    pub fn jpeg_quality(&self) -> u8 {
        match self {
            ProxyQuality::Low => 70,
            ProxyQuality::Medium => 85,
            ProxyQuality::High => 95,
        }
    }

    /// Human-readable label
    pub fn label(&self) -> &'static str {
        match self {
            ProxyQuality::Low => "Low (Fast)",
            ProxyQuality::Medium => "Medium",
            ProxyQuality::High => "High",
        }
    }
}

impl Default for ProxyQuality {
    fn default() -> Self {
        ProxyQuality::Medium
    }
}

/// Proxy generation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySettings {
    /// Target resolution for proxies
    pub resolution: ProxyResolution,
    /// JPEG quality for image sequence
    pub quality: ProxyQuality,
    /// Maximum concurrent proxy generation jobs
    pub max_concurrent_jobs: u8,
    /// Auto-generate proxies on import
    pub auto_generate: bool,
    /// Custom proxy storage directory (None = use project directory)
    pub custom_proxy_dir: Option<PathBuf>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            resolution: ProxyResolution::default(),
            quality: ProxyQuality::default(),
            max_concurrent_jobs: 2,
            auto_generate: true,
            custom_proxy_dir: None,
        }
    }
}

/// UI/Editor preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    /// Timeline zoom level (pixels per second)
    pub timeline_zoom: f32,
    /// Show filmstrip thumbnails on clips
    pub show_filmstrip: bool,
    /// Show waveforms on audio clips
    pub show_waveforms: bool,
    /// Snap to playhead when moving clips
    pub snap_to_playhead: bool,
    /// Snap to other clips
    pub snap_to_clips: bool,
    /// UI scale factor (1.0 = 100%)
    pub ui_scale: f32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            timeline_zoom: 100.0,
            show_filmstrip: true,
            show_waveforms: true,
            snap_to_playhead: true,
            snap_to_clips: true,
            ui_scale: 1.0,
        }
    }
}

/// All user settings combined
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    /// Proxy generation settings
    pub proxy: ProxySettings,
    /// UI preferences
    pub ui: UiSettings,
    /// Recent projects (for quick access menu)
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
    /// Maximum recent projects to keep
    #[serde(default = "default_max_recent")]
    pub max_recent_projects: usize,
}

fn default_max_recent() -> usize {
    10
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            proxy: ProxySettings::default(),
            ui: UiSettings::default(),
            recent_projects: Vec::new(),
            max_recent_projects: 10,
        }
    }
}

impl UserSettings {
    /// Create new settings with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a project to recent projects list
    pub fn add_recent_project(&mut self, path: PathBuf) {
        // Remove if already in list
        self.recent_projects.retain(|p| p != &path);
        // Add to front
        self.recent_projects.insert(0, path);
        // Trim to max
        if self.recent_projects.len() > self.max_recent_projects {
            self.recent_projects.truncate(self.max_recent_projects);
        }
    }

    /// Load settings from a file
    pub fn load_from_file(path: &PathBuf) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read settings file: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))
    }

    /// Save settings to a file
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings dir: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        std::fs::write(path, content).map_err(|e| format!("Failed to write settings file: {}", e))
    }

    /// Get the default settings file path
    ///
    /// Uses APPDATA on Windows, ~/.config on Unix
    pub fn default_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA")
                .ok()
                .map(|p| PathBuf::from(p).join("Strawberry").join("settings.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME").ok().map(|p| {
                PathBuf::from(p)
                    .join(".config")
                    .join("strawberry")
                    .join("settings.json")
            })
        }
    }

    /// Load from default path, or return defaults if not found
    pub fn load_or_default() -> Self {
        if let Some(path) = Self::default_path() {
            Self::load_from_file(&path).unwrap_or_else(|e| {
                tracing::warn!("Could not load settings: {}", e);
                Self::default()
            })
        } else {
            Self::default()
        }
    }

    /// Save to default path
    pub fn save(&self) -> Result<(), String> {
        if let Some(path) = Self::default_path() {
            self.save_to_file(&path)
        } else {
            Err("Could not determine settings path".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_resolution_dimensions() {
        assert_eq!(ProxyResolution::Res720p.dimensions(), Some((1280, 720)));
        assert_eq!(ProxyResolution::Original.dimensions(), None);
    }

    #[test]
    fn test_proxy_quality_values() {
        assert_eq!(ProxyQuality::Low.jpeg_quality(), 70);
        assert_eq!(ProxyQuality::Medium.jpeg_quality(), 85);
        assert_eq!(ProxyQuality::High.jpeg_quality(), 95);
    }

    #[test]
    fn test_recent_projects() {
        let mut settings = UserSettings::new();

        settings.add_recent_project(PathBuf::from("/project1"));
        settings.add_recent_project(PathBuf::from("/project2"));
        settings.add_recent_project(PathBuf::from("/project1")); // Duplicate

        // Should have 2 projects, with project1 first
        assert_eq!(settings.recent_projects.len(), 2);
        assert_eq!(settings.recent_projects[0], PathBuf::from("/project1"));
    }

    #[test]
    fn test_serialization() {
        let settings = UserSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: UserSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.proxy.resolution, settings.proxy.resolution);
    }
}
