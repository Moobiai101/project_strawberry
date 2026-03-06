//! # Node Values
//!
//! Type-safe values that flow between nodes in the graph.
//!
//! ## Design Rationale
//!
//! Rather than using trait objects everywhere, we use an enum for values.
//! This provides:
//! - Compile-time type checking
//! - Easy serialization (serde)
//! - Pattern matching for GPU job dispatch

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// NODE VALUE TYPE
// ============================================================================

/// Type discriminant for [`NodeValue`].
///
/// Used for input validation and UI generation without needing
/// the actual value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeValueType {
    /// No value / void
    None,
    /// Boolean (checkbox in UI)
    Boolean,
    /// Integer number
    Integer,
    /// Floating point number
    Float,
    /// 2D vector (position, scale)
    Vec2,
    /// 3D vector (color RGB, position XYZ)
    Vec3,
    /// 4D vector (color RGBA)
    Vec4,
    /// 3x3 matrix (2D transforms)
    Matrix3,
    /// 4x4 matrix (3D transforms)
    Matrix4,
    /// Color with alpha (RGBA, 0.0-1.0)
    Color,
    /// GPU texture reference
    Texture,
    /// Audio samples buffer
    Samples,
    /// Reference to a video/image file
    Footage,
    /// Reference to another node
    NodeRef,
    /// Selection from predefined options
    Combo,
    /// File path
    Path,
    /// Duration / time value
    Duration,
}

impl NodeValueType {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Boolean => "Boolean",
            Self::Integer => "Integer",
            Self::Float => "Float",
            Self::Vec2 => "Vector 2D",
            Self::Vec3 => "Vector 3D",
            Self::Vec4 => "Vector 4D",
            Self::Matrix3 => "Matrix 3x3",
            Self::Matrix4 => "Matrix 4x4",
            Self::Color => "Color",
            Self::Texture => "Texture",
            Self::Samples => "Audio",
            Self::Footage => "Footage",
            Self::NodeRef => "Node Reference",
            Self::Combo => "Choice",
            Self::Path => "File Path",
            Self::Duration => "Duration",
        }
    }
}

// ============================================================================
// TEXTURE HANDLE
// ============================================================================

/// Handle to a GPU texture.
///
/// This is an opaque reference that can be resolved by the texture manager.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextureHandle {
    /// Unique identifier for this texture
    pub id: u64,
    /// Optional egui texture ID (if already uploaded)
    pub egui_id: Option<egui::TextureId>,
    /// Texture dimensions
    pub dimensions: glam::UVec2,
}

impl TextureHandle {
    /// Create a new texture handle
    pub fn new(id: u64, dimensions: glam::UVec2) -> Self {
        Self {
            id,
            egui_id: None,
            dimensions,
        }
    }

    /// Create with an egui texture ID
    pub fn with_egui_id(id: u64, egui_id: egui::TextureId, dimensions: glam::UVec2) -> Self {
        Self {
            id,
            egui_id: Some(egui_id),
            dimensions,
        }
    }
}

// ============================================================================
// FOOTAGE REFERENCE
// ============================================================================

/// Reference to a video or image file.
///
/// Used by ClipNode to reference its source media.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FootageRef {
    /// UUID of the asset in the asset manager
    pub asset_uuid: Uuid,
    /// Original file path (for display and re-linking)
    pub path: std::path::PathBuf,
}

impl FootageRef {
    /// Create a new footage reference
    pub fn new(asset_uuid: Uuid, path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            asset_uuid,
            path: path.into(),
        }
    }
}

// ============================================================================
// AUDIO SAMPLES
// ============================================================================

/// Audio samples buffer.
///
/// Placeholder for audio processing - will be expanded in audio phase.
#[derive(Debug, Clone)]
pub struct AudioSamples {
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Interleaved f32 samples
    pub data: Arc<Vec<f32>>,
}

impl AudioSamples {
    /// Create new audio samples
    pub fn new(sample_rate: u32, channels: u8, data: Vec<f32>) -> Self {
        Self {
            sample_rate,
            channels,
            data: Arc::new(data),
        }
    }

    /// Get number of samples per channel
    pub fn sample_count(&self) -> usize {
        self.data.len() / self.channels as usize
    }
}

// ============================================================================
// NODE VALUE
// ============================================================================

/// A typed value that flows between nodes.
///
/// This enum contains all possible value types that can be passed
/// as inputs or outputs of nodes.
///
/// Note: Some variants (Texture, Samples) are not serializable and will
/// be skipped during serialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum NodeValue {
    /// No value / void
    None,
    /// Boolean value
    Boolean(bool),
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// 2D vector
    Vec2(glam::Vec2),
    /// 3D vector
    Vec3(glam::Vec3),
    /// 4D vector
    Vec4(glam::Vec4),
    /// 3x3 matrix
    Matrix3(glam::Mat3),
    /// 4x4 matrix
    Matrix4(glam::Mat4),
    /// RGBA color (0.0-1.0 range)
    Color([f32; 4]),
    /// GPU texture (not serializable - transient)
    #[serde(skip)]
    Texture(TextureHandle),
    /// Audio samples (not serializable - transient)
    #[serde(skip)]
    Samples(AudioSamples),
    /// Footage reference
    Footage(FootageRef),
    /// Node reference (UUID)
    NodeRef(Uuid),
    /// Combo selection (index)
    Combo(usize),
    /// File path
    Path(std::path::PathBuf),
    /// Duration value
    Duration(Duration),
}

impl NodeValue {
    /// Get the type of this value
    pub fn value_type(&self) -> NodeValueType {
        match self {
            Self::None => NodeValueType::None,
            Self::Boolean(_) => NodeValueType::Boolean,
            Self::Integer(_) => NodeValueType::Integer,
            Self::Float(_) => NodeValueType::Float,
            Self::Vec2(_) => NodeValueType::Vec2,
            Self::Vec3(_) => NodeValueType::Vec3,
            Self::Vec4(_) => NodeValueType::Vec4,
            Self::Matrix3(_) => NodeValueType::Matrix3,
            Self::Matrix4(_) => NodeValueType::Matrix4,
            Self::Color(_) => NodeValueType::Color,
            Self::Texture(_) => NodeValueType::Texture,
            Self::Samples(_) => NodeValueType::Samples,
            Self::Footage(_) => NodeValueType::Footage,
            Self::NodeRef(_) => NodeValueType::NodeRef,
            Self::Combo(_) => NodeValueType::Combo,
            Self::Path(_) => NodeValueType::Path,
            Self::Duration(_) => NodeValueType::Duration,
        }
    }

    /// Check if this value is None
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Try to get as boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as integer
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Try to get as Vec2
    pub fn as_vec2(&self) -> Option<glam::Vec2> {
        match self {
            Self::Vec2(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as texture handle
    pub fn as_texture(&self) -> Option<&TextureHandle> {
        match self {
            Self::Texture(t) => Some(t),
            _ => None,
        }
    }

    /// Try to get as footage reference
    pub fn as_footage(&self) -> Option<&FootageRef> {
        match self {
            Self::Footage(f) => Some(f),
            _ => None,
        }
    }

    /// Try to get as duration
    pub fn as_duration(&self) -> Option<Duration> {
        match self {
            Self::Duration(d) => Some(*d),
            _ => None,
        }
    }
}

impl Default for NodeValue {
    fn default() -> Self {
        Self::None
    }
}

// ============================================================================
// NODE VALUE TABLE
// ============================================================================

/// Output table from node processing.
///
/// Nodes push their output values to this table. Typically contains
/// a single texture, but can hold multiple values for multi-output nodes.
#[derive(Debug, Clone, Default)]
pub struct NodeValueTable {
    values: Vec<NodeValue>,
}

impl NodeValueTable {
    /// Create an empty table
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a value to the table
    pub fn push(&mut self, value: NodeValue) {
        self.values.push(value);
    }

    /// Get value at index
    pub fn get(&self, index: usize) -> Option<&NodeValue> {
        self.values.get(index)
    }

    /// Get the first value (most common case)
    pub fn first(&self) -> Option<&NodeValue> {
        self.values.first()
    }

    /// Get the first texture in the table
    pub fn first_texture(&self) -> Option<&TextureHandle> {
        self.values.iter().find_map(|v| v.as_texture())
    }

    /// Check if table is empty
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get number of values
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Clear all values
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Iterate over values
    pub fn iter(&self) -> impl Iterator<Item = &NodeValue> {
        self.values.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_value_type() {
        let val = NodeValue::Float(3.14);
        assert_eq!(val.value_type(), NodeValueType::Float);
        assert_eq!(val.as_float(), Some(3.14));
    }

    #[test]
    fn test_node_value_table() {
        let mut table = NodeValueTable::new();
        table.push(NodeValue::Float(1.0));
        table.push(NodeValue::Boolean(true));

        assert_eq!(table.len(), 2);
        assert_eq!(table.first().unwrap().as_float(), Some(1.0));
    }

    #[test]
    fn test_int_to_float_coercion() {
        let val = NodeValue::Integer(42);
        assert_eq!(val.as_float(), Some(42.0));
    }
}
