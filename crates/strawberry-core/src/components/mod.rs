//! # Core Components
//!
//! ECS Components for Project Strawberry.

pub mod timeline;
pub use timeline::*;

pub mod audio;
pub use audio::*;

pub mod waveform;
pub use waveform::*;

pub mod track_audio;
pub use track_audio::*;

pub mod keyframe;
pub use keyframe::*;

pub mod keyframe_track;
pub use keyframe_track::*;

use bevy_ecs::component::Component;
use chrono::{DateTime, Utc};
use glam::{UVec2, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// STABLE ID - Production-grade entity identification for undo/redo
// ============================================================================

/// Stable identifier for timeline entities (clips, tracks, timelines, effects)
///
/// Unlike ECS entity IDs which become invalid after despawn/respawn cycles,
/// `StableId` uses UUID v4 which persists across undo/redo operations.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StableId(pub Uuid);

impl StableId {
    /// Create a new stable ID with a random UUID v4
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID (for deserialization/redo)
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the UUID
    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for StableId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for StableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0.to_string()[..8])
    }
}

// ============================================================================
// PROJECT
// ============================================================================

/// Project metadata component
///
/// Contains the project name and timestamps.
/// Settings are stored separately in `ProjectSettings`.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Human-readable project name
    pub name: String,
    /// When the project was created
    pub created_at: DateTime<Utc>,
    /// When the project was last modified  
    pub modified_at: DateTime<Utc>,
}

impl Default for Project {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            name: "Untitled".to_string(),
            created_at: now,
            modified_at: now,
        }
    }
}

impl Project {
    /// Create a new project with the given name
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            created_at: now,
            modified_at: now,
        }
    }

    /// Mark the project as modified
    pub fn touch(&mut self) {
        self.modified_at = Utc::now();
    }
}

/// Project technical settings
///
/// Resolution, frame rate, and color space.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// Output resolution in pixels
    pub resolution: UVec2,
    /// Frame rate for the project
    pub frame_rate: FrameRate,
    /// Color space for the project
    pub color_space: ColorSpace,
    /// Project duration (computed from timeline content)
    pub duration: Duration,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            resolution: UVec2::new(1920, 1080),
            frame_rate: FrameRate::Fps30,
            color_space: ColorSpace::Srgb,
            duration: Duration::from_secs(60),
        }
    }
}

// ============================================================================
// COLOR SPACE
// ============================================================================

/// Color space for the project
///
/// Determines how colors are interpreted and displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ColorSpace {
    /// Standard RGB (most common, web)
    #[default]
    Srgb,
    /// Rec. 709 (HD video standard)
    Rec709,
    /// Rec. 2020 (HDR, wide gamut)
    Rec2020,
    /// DCI-P3 (cinema)
    DciP3,
    /// ACEScg (VFX, linear)
    AcesCg,
}

impl ColorSpace {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::Rec709 => "Rec. 709",
            Self::Rec2020 => "Rec. 2020 (HDR)",
            Self::DciP3 => "DCI-P3",
            Self::AcesCg => "ACEScg",
        }
    }
}

// ============================================================================
// FRAME RATE
// ============================================================================

/// Frame rate presets
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FrameRate {
    /// 23.976 fps (cinema, NTSC film)
    Fps23_976,
    /// 24 fps (cinema)
    Fps24,
    /// 25 fps (PAL)
    Fps25,
    /// 29.97 fps (NTSC video)
    Fps29_97,
    /// 30 fps
    Fps30,
    /// 50 fps (PAL progressive)
    Fps50,
    /// 59.94 fps (NTSC progressive)
    Fps59_94,
    /// 60 fps
    Fps60,
    /// Custom frame rate (numerator / denominator)
    Custom(u32, u32),
}

impl Default for FrameRate {
    fn default() -> Self {
        Self::Fps30
    }
}

impl FrameRate {
    /// Convert to floating point fps
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Fps23_976 => 24000.0 / 1001.0,
            Self::Fps24 => 24.0,
            Self::Fps25 => 25.0,
            Self::Fps29_97 => 30000.0 / 1001.0,
            Self::Fps30 => 30.0,
            Self::Fps50 => 50.0,
            Self::Fps59_94 => 60000.0 / 1001.0,
            Self::Fps60 => 60.0,
            Self::Custom(num, den) => *num as f64 / *den as f64,
        }
    }

    /// Get the frame duration
    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.as_f64())
    }

    /// Convert a timestamp to frame number
    pub fn timestamp_to_frame(&self, timestamp: Duration) -> u64 {
        (timestamp.as_secs_f64() * self.as_f64()) as u64
    }

    /// Convert a frame number to timestamp
    pub fn frame_to_timestamp(&self, frame: u64) -> Duration {
        Duration::from_secs_f64(frame as f64 / self.as_f64())
    }

    /// Get display name for UI
    pub fn display_name(&self) -> String {
        match self {
            Self::Fps23_976 => "23.976".to_string(),
            Self::Fps24 => "24".to_string(),
            Self::Fps25 => "25".to_string(),
            Self::Fps29_97 => "29.97".to_string(),
            Self::Fps30 => "30".to_string(),
            Self::Fps50 => "50".to_string(),
            Self::Fps59_94 => "59.94".to_string(),
            Self::Fps60 => "60".to_string(),
            Self::Custom(num, den) => format!("{:.3}", *num as f64 / *den as f64),
        }
    }
}

// ============================================================================
// TRACK
// ============================================================================

/// Track component
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Track index (0 = bottom, higher = on top)
    pub index: u32,
    /// Type of content on this track
    pub track_type: TrackType,
    /// Human-readable name
    pub name: String,
    /// Whether the track is locked (no edits allowed)
    pub locked: bool,
    /// Whether the track is muted
    pub muted: bool,
    /// Solo mode (only this track plays)
    pub solo: bool,
    /// Track visibility (for video tracks)
    pub visible: bool,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            index: 0,
            track_type: TrackType::Video,
            name: "Track 1".to_string(),
            locked: false,
            muted: false,
            solo: false,
            visible: true,
        }
    }
}

/// Track type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrackType {
    #[default]
    Video,
    Audio,
    Text,
    Effect,
}

// ============================================================================
// CLIP
// ============================================================================

/// Clip component - the core editing unit
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    /// Human-readable name of the clip
    pub name: String,
    /// Source in point (start time in source media)
    pub source_in: Duration,
    /// Source out point (end time in source media)
    pub source_out: Duration,
    /// Position on timeline
    pub timeline_start: Duration,
}

impl Clip {
    /// Get clip duration
    pub fn duration(&self) -> Duration {
        self.source_out.saturating_sub(self.source_in)
    }

    /// Get timeline end position
    pub fn timeline_end(&self) -> Duration {
        self.timeline_start + self.duration()
    }

    /// Check if a timestamp is within this clip's timeline range
    pub fn contains_time(&self, time: Duration) -> bool {
        time >= self.timeline_start && time < self.timeline_end()
    }

    /// Convert a timeline time to source time
    pub fn timeline_to_source(&self, timeline_time: Duration) -> Option<Duration> {
        if !self.contains_time(timeline_time) {
            return None;
        }
        let offset = timeline_time.saturating_sub(self.timeline_start);
        Some(self.source_in + offset)
    }
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            name: "Untitled Clip".to_string(),
            source_in: Duration::ZERO,
            source_out: Duration::from_secs(10),
            timeline_start: Duration::ZERO,
        }
    }
}

/// Clip transform component
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct ClipTransform {
    /// Position offset (in pixels, from center)
    pub position: Vec2,
    /// Scale factor (1.0 = 100%)
    pub scale: Vec2,
    /// Rotation in radians
    pub rotation: f32,
    /// Anchor point (0.5, 0.5 = center)
    pub anchor: Vec2,
    /// Opacity (0.0 - 1.0)
    pub opacity: f32,
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
            anchor: Vec2::new(0.5, 0.5),
            opacity: 1.0,
        }
    }
}

// ============================================================================
// VIDEO ASSET
// ============================================================================

/// Video asset component - represents a video file
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct VideoAsset {
    /// Path to the video file
    pub path: PathBuf,
    /// Total duration
    pub duration: Duration,
    /// Total frame count
    pub frame_count: u64,
    /// Frame rate of the source
    pub frame_rate: FrameRate,
    /// Resolution in pixels
    pub resolution: UVec2,
    /// Video codec name
    pub codec: String,
    /// Has audio track
    pub has_audio: bool,
}

/// Video frame component - represents a decoded frame in GPU memory
#[derive(Component, Debug, Clone)]
pub struct VideoFrame {
    /// Frame number in the source
    pub frame_number: u64,
    /// egui texture ID for rendering
    pub texture_id: egui::TextureId,
    /// Timestamp in source media
    pub timestamp: Duration,
}

// ============================================================================
// AUDIO ASSET
// ============================================================================

/// Audio asset component - represents an audio file
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct AudioAsset {
    /// Path to the audio file
    pub path: PathBuf,
    /// Total duration
    pub duration: Duration,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Codec name
    pub codec: String,
}

/// Audio clip component - an audio segment on a track
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct AudioClip {
    /// Reference to the audio asset entity
    pub asset_entity: u64, // Using u64 for serialization (Entity::to_bits())
    /// Volume (0.0 - 1.0, can exceed for boost)
    pub volume: f32,
    /// Stereo pan (-1.0 = left, 0.0 = center, 1.0 = right)
    pub pan: f32,
    /// Muted
    pub muted: bool,
}

impl Default for AudioClip {
    fn default() -> Self {
        Self {
            asset_entity: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
        }
    }
}
// ============================================================================
// 2D TRANSFORM (Industry-Standard Affine Transform)
// ============================================================================

/// 2D Transform component for clips and layers
///
/// Implements industry-standard affine transform:
/// - Transform order: Translate → Rotate around anchor → Scale
/// - Anchor point (pivot) in normalized coordinates (0.5, 0.5 = center)
/// - All rotations in radians (like After Effects/Premiere internally)
///
/// ## Example
/// ```ignore
/// let transform = Transform2D {
///     position: Vec2::new(100.0, 50.0),  // Move 100px right, 50px down
///     scale: Vec2::new(1.5, 1.5),        // Scale to 150%
///     rotation: std::f32::consts::FRAC_PI_4,  // 45 degrees
///     anchor: Vec2::new(0.5, 0.5),       // Rotate around center
/// };
/// ```
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Transform2D {
    /// Translation offset in pixels from parent origin
    pub position: Vec2,
    /// Scale factor (1.0 = original size)
    pub scale: Vec2,
    /// Rotation in radians (counter-clockwise positive, like standard math)
    pub rotation: f32,
    /// Anchor point in normalized coordinates (0.0-1.0)
    /// (0.0, 0.0) = top-left, (0.5, 0.5) = center, (1.0, 1.0) = bottom-right
    pub anchor: Vec2,
    /// Opacity (0.0 = transparent, 1.0 = opaque)
    pub opacity: f32,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
            anchor: Vec2::splat(0.5), // Center anchor (industry standard)
            opacity: 1.0,
        }
    }
}

impl Transform2D {
    /// Create an identity transform (no change)
    pub fn identity() -> Self {
        Self::default()
    }

    /// Create a transform with only position
    pub fn from_position(x: f32, y: f32) -> Self {
        Self {
            position: Vec2::new(x, y),
            ..Self::default()
        }
    }

    /// Create a transform with position and scale
    pub fn from_position_scale(position: Vec2, scale: Vec2) -> Self {
        Self {
            position,
            scale,
            ..Self::default()
        }
    }

    /// Compute the 3x3 affine transformation matrix
    ///
    /// The matrix is computed as: T * R(anchor) * S * T(-anchor)
    /// Where T is translation, R is rotation around anchor, S is scale
    ///
    /// This matches the behavior of After Effects/Premiere:
    /// - Anchor point defines the center of rotation and scaling
    /// - Position moves the anchor point to that screen location
    pub fn to_matrix(&self, source_size: Vec2) -> glam::Mat3 {
        use glam::Mat3;

        // Calculate anchor offset in pixels
        let anchor_offset = self.anchor * source_size;

        // Build transform: Position → RotateAroundAnchor → Scale
        // Final transform = T(position) × T(anchor) × R(rotation) × S(scale) × T(-anchor)

        let translate_to_origin = Mat3::from_translation(-anchor_offset);
        let scale_matrix = Mat3::from_scale(self.scale);
        let rotate_matrix = Mat3::from_angle(self.rotation);
        let translate_from_origin = Mat3::from_translation(anchor_offset);
        let final_translation = Mat3::from_translation(self.position);

        // Apply in order: origin → scale → rotate → back → position
        final_translation
            * translate_from_origin
            * rotate_matrix
            * scale_matrix
            * translate_to_origin
    }

    /// Compute inverse matrix for hit testing
    pub fn inverse_matrix(&self, source_size: Vec2) -> Option<glam::Mat3> {
        let matrix = self.to_matrix(source_size);
        let det = matrix.determinant();
        if det.abs() > f32::EPSILON {
            Some(matrix.inverse())
        } else {
            None
        }
    }

    /// Transform a point from local to world coordinates
    pub fn transform_point(&self, point: Vec2, source_size: Vec2) -> Vec2 {
        let matrix = self.to_matrix(source_size);
        let p3 = matrix.transform_point2(point);
        p3
    }

    /// Transform a point from world to local coordinates (for hit testing)
    pub fn inverse_transform_point(&self, point: Vec2, source_size: Vec2) -> Option<Vec2> {
        self.inverse_matrix(source_size)
            .map(|m| m.transform_point2(point))
    }

    /// Interpolate between two transforms (for animation)
    pub fn lerp(&self, other: &Transform2D, t: f32) -> Transform2D {
        Transform2D {
            position: self.position.lerp(other.position, t),
            scale: self.scale.lerp(other.scale, t),
            rotation: self.rotation + (other.rotation - self.rotation) * t,
            anchor: self.anchor.lerp(other.anchor, t),
            opacity: self.opacity + (other.opacity - self.opacity) * t,
        }
    }

    /// Get rotation in degrees (for UI display)
    pub fn rotation_degrees(&self) -> f32 {
        self.rotation.to_degrees()
    }

    /// Set rotation from degrees
    pub fn set_rotation_degrees(&mut self, degrees: f32) {
        self.rotation = degrees.to_radians();
    }
}

// ============================================================================
// RENDER LAYER (Compositing Order & Blend Mode)
// ============================================================================

/// Render layer component for z-ordering and compositing
///
/// Controls the visual stacking order and blend mode for compositing.
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct RenderLayer {
    /// Z-index for sorting (higher = on top)
    pub z_index: i32,
    /// Blend mode for compositing
    pub blend_mode: BlendMode,
    /// Whether this layer is visible
    pub visible: bool,
    /// Whether this layer is locked (no editing)
    pub locked: bool,
    /// Solo mode (only show this layer)
    pub solo: bool,
}

impl Default for RenderLayer {
    fn default() -> Self {
        Self {
            z_index: 0,
            blend_mode: BlendMode::Normal,
            visible: true,
            locked: false,
            solo: false,
        }
    }
}

/// Blend modes for layer compositing
///
/// Based on industry-standard Photoshop/After Effects blend modes.
/// Phase 0 implements Normal only; others are prepared for Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum BlendMode {
    // === Normal Modes ===
    #[default]
    Normal,
    Dissolve,

    // === Darken Modes ===
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,

    // === Lighten Modes ===
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,

    // === Contrast Modes ===
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,

    // === Inversion Modes ===
    Difference,
    Exclusion,
    Subtract,
    Divide,

    // === Component Modes ===
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Dissolve => "Dissolve",
            BlendMode::Darken => "Darken",
            BlendMode::Multiply => "Multiply",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::DarkerColor => "Darker Color",
            BlendMode::Lighten => "Lighten",
            BlendMode::Screen => "Screen",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::LinearDodge => "Linear Dodge (Add)",
            BlendMode::LighterColor => "Lighter Color",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::HardMix => "Hard Mix",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }

    /// Get all blend modes for UI dropdown
    pub fn all() -> &'static [BlendMode] {
        &[
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::SoftLight,
            BlendMode::HardLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
        ]
    }
}

// ============================================================================
// SELECTION MARKERS
// ============================================================================

/// Marker component for selected entities
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Selected;

/// Marker for primary selection (among multiple selected)
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PrimarySelection;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_rate_conversion() {
        let fps30 = FrameRate::Fps30;
        assert_eq!(fps30.as_f64(), 30.0);
        assert_eq!(fps30.timestamp_to_frame(Duration::from_secs(1)), 30);
        assert_eq!(fps30.frame_to_timestamp(60), Duration::from_secs(2));
    }

    #[test]
    fn test_clip_time_conversion() {
        let clip = Clip {
            name: "Test".to_string(),
            source_in: Duration::from_secs(10),
            source_out: Duration::from_secs(20),
            timeline_start: Duration::from_secs(5),
        };

        assert_eq!(clip.duration(), Duration::from_secs(10));
        assert_eq!(clip.timeline_end(), Duration::from_secs(15));
        assert!(clip.contains_time(Duration::from_secs(10)));
        assert!(!clip.contains_time(Duration::from_secs(4)));

        let source_time = clip.timeline_to_source(Duration::from_secs(7));
        assert_eq!(source_time, Some(Duration::from_secs(12)));
    }

    #[test]
    fn test_ntsc_frame_rates() {
        let fps23_976 = FrameRate::Fps23_976;
        assert!((fps23_976.as_f64() - 23.976).abs() < 0.001);

        let fps29_97 = FrameRate::Fps29_97;
        assert!((fps29_97.as_f64() - 29.97).abs() < 0.001);
    }
}
