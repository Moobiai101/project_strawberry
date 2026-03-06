//! # Node System
//!
//! Production-grade node graph architecture for timeline compositing.
//!
//! ## Purpose
//!
//! Provides a unified interface for all processing units:
//! - Timeline elements (clips, tracks)
//! - Effects (blur, color correction)
//! - Generators (solid, text)
//! - Transitions (dissolve, wipe)
//!
//! ## Architecture
//!
//! Every processing unit implements the [`Node`] trait, allowing the
//! [`NodeTraverser`] to walk the graph and produce output textures.
//!
//! See `README.md` in this directory for full data flow diagrams.

pub mod input;
pub mod timeline;
pub mod value;

// Re-export core types
pub use input::{InputDefinition, InputFlags};
pub use value::{NodeValue, NodeValueTable, NodeValueType};

use std::time::Duration;
use uuid::Uuid;

// ============================================================================
// NODE CATEGORY
// ============================================================================

/// Categories for organizing nodes in menus and for semantic grouping.
///
/// Based on Olive Editor's node categories, extended for AI/VFX needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    /// Output nodes (Viewer, Export)
    Output,
    /// Content generators (Solid, Text, Noise, Shape)
    Generator,
    /// Mathematical operations (Add, Multiply, Clamp)
    Math,
    /// Keying effects (Chroma key, Luma key)
    Keying,
    /// Filter effects (Blur, Sharpen, Edge detect)
    Filter,
    /// Color correction (LUT, HSV, Curves)
    Color,
    /// Time manipulation (Remap, Freeze)
    Time,
    /// Timeline elements (Track, Clip)
    Timeline,
    /// Transition effects (Dissolve, Wipe)
    Transition,
    /// Distortion effects (Transform, Corner pin, Wave)
    Distort,
    /// Project-level settings
    Project,
    /// AI-powered effects (Upscaling, Style transfer)
    AI,
    /// Motion tracking nodes
    Tracking,
    /// 3D compositing
    ThreeD,
}

impl NodeCategory {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Generator => "Generator",
            Self::Math => "Math",
            Self::Keying => "Keying",
            Self::Filter => "Filter",
            Self::Color => "Color",
            Self::Time => "Time",
            Self::Timeline => "Timeline",
            Self::Transition => "Transition",
            Self::Distort => "Distort",
            Self::Project => "Project",
            Self::AI => "AI",
            Self::Tracking => "Tracking",
            Self::ThreeD => "3D",
        }
    }
}

// ============================================================================
// NODE GLOBALS
// ============================================================================

/// Global parameters passed to all nodes during processing.
///
/// Contains render settings and timing information that nodes need
/// to produce their output.
#[derive(Debug, Clone)]
pub struct NodeGlobals {
    /// Time range being rendered (start to end)
    pub time_range: TimeRange,
    /// Output video resolution
    pub video_resolution: glam::UVec2,
    /// Output frame rate
    pub frame_rate: f64,
    /// Pixel aspect ratio (usually 1.0)
    pub pixel_aspect_ratio: f64,
}

impl NodeGlobals {
    /// Create new globals for a render pass
    pub fn new(time_range: TimeRange, video_resolution: glam::UVec2, frame_rate: f64) -> Self {
        Self {
            time_range,
            video_resolution,
            frame_rate,
            pixel_aspect_ratio: 1.0,
        }
    }
}

impl Default for NodeGlobals {
    fn default() -> Self {
        Self {
            time_range: TimeRange::ZERO,
            video_resolution: glam::UVec2::new(1920, 1080),
            frame_rate: 30.0,
            pixel_aspect_ratio: 1.0,
        }
    }
}

// ============================================================================
// TIME RANGE
// ============================================================================

/// A range of time for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TimeRange {
    /// Start time (inclusive)
    pub start: Duration,
    /// End time (exclusive)
    pub end: Duration,
}

impl TimeRange {
    /// Zero-duration range starting at zero
    pub const ZERO: Self = Self {
        start: Duration::ZERO,
        end: Duration::ZERO,
    };

    /// Create a new time range
    pub fn new(start: Duration, end: Duration) -> Self {
        debug_assert!(start <= end, "TimeRange start must be <= end");
        Self { start, end }
    }

    /// Create a range for a single frame at the given time
    pub fn at(time: Duration, frame_duration: Duration) -> Self {
        Self {
            start: time,
            end: time + frame_duration,
        }
    }

    /// Get the duration of this range
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Check if this range contains a time point
    pub fn contains(&self, time: Duration) -> bool {
        time >= self.start && time < self.end
    }

    /// Check if this range overlaps with another
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }
}

// ============================================================================
// NODE VALUE ROW
// ============================================================================

/// A row of input values for node processing.
///
/// Maps input IDs to their resolved values at the current time.
#[derive(Debug, Clone, Default)]
pub struct NodeValueRow {
    values: std::collections::HashMap<String, NodeValue>,
}

impl NodeValueRow {
    /// Create an empty row
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value
    pub fn insert(&mut self, key: impl Into<String>, value: NodeValue) {
        self.values.insert(key.into(), value);
    }

    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<&NodeValue> {
        self.values.get(key)
    }

    /// Get a value, returning None variant if not found
    pub fn get_or_none(&self, key: &str) -> NodeValue {
        self.values.get(key).cloned().unwrap_or(NodeValue::None)
    }
}

// ============================================================================
// NODE TRAIT
// ============================================================================

/// Base trait for all processing nodes.
///
/// Every node in the system implements this trait, providing a unified
/// interface for the [`NodeTraverser`] to walk the graph and produce output.
///
/// # Implementation Notes
///
/// - [`Node::id`] must be globally unique (use reverse domain notation)
/// - [`Node::process`] should be pure - same inputs = same outputs
/// - Use [`NodeValue::Texture`] for deferred GPU work (ShaderJob)
pub trait Node: Send + Sync {
    /// Globally unique identifier for this node type.
    ///
    /// Use reverse domain notation, e.g., `"org.strawberry.blur"`.
    /// This is used for serialization and node registry.
    fn id(&self) -> &'static str;

    /// Human-readable name for display in UI.
    fn name(&self) -> &'static str;

    /// Category for menu organization.
    fn category(&self) -> NodeCategory;

    /// Define the inputs this node accepts.
    ///
    /// Called once during node registration to build the UI and
    /// validation logic.
    fn inputs(&self) -> Vec<InputDefinition>;

    /// Process input values and produce output.
    ///
    /// This is the core rendering function. Given a row of input values
    /// and global render parameters, produce an output table of values.
    ///
    /// # Arguments
    ///
    /// * `inputs` - Resolved input values at the current time
    /// * `globals` - Render parameters (time, resolution, etc.)
    ///
    /// # Returns
    ///
    /// A table of output values, typically containing a single texture.
    fn process(&self, inputs: &NodeValueRow, globals: &NodeGlobals) -> NodeValueTable;

    /// Get the stable UUID for this node instance.
    ///
    /// Used for serialization and undo/redo. Default implementation
    /// returns None; override for nodes that need persistence.
    fn stable_id(&self) -> Option<Uuid> {
        None
    }
}

// ============================================================================
// NODE REFERENCE
// ============================================================================

/// A reference to a node by its stable ID.
///
/// Used in Commands and serialization instead of raw pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeRef {
    /// UUID of the node
    pub uuid: Uuid,
}

impl NodeRef {
    /// Create a new node reference
    pub fn new(uuid: Uuid) -> Self {
        Self { uuid }
    }
}

impl From<Uuid> for NodeRef {
    fn from(uuid: Uuid) -> Self {
        Self { uuid }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_range_contains() {
        let range = TimeRange::new(Duration::from_secs(1), Duration::from_secs(5));

        assert!(!range.contains(Duration::from_millis(500)));
        assert!(range.contains(Duration::from_secs(1)));
        assert!(range.contains(Duration::from_secs(3)));
        assert!(!range.contains(Duration::from_secs(5)));
    }

    #[test]
    fn test_time_range_overlaps() {
        let a = TimeRange::new(Duration::from_secs(0), Duration::from_secs(5));
        let b = TimeRange::new(Duration::from_secs(3), Duration::from_secs(8));
        let c = TimeRange::new(Duration::from_secs(5), Duration::from_secs(10));

        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn test_node_category_display() {
        assert_eq!(NodeCategory::Timeline.display_name(), "Timeline");
        assert_eq!(NodeCategory::Filter.display_name(), "Filter");
    }
}
