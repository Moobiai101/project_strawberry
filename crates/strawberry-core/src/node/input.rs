//! # Node Inputs
//!
//! Definitions for node input parameters.
//!
//! Each node declares its inputs using [`InputDefinition`], which describes
//! the input's type, default value, and behavior.

use super::value::{NodeValue, NodeValueType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// INPUT FLAGS
// ============================================================================

bitflags::bitflags! {
    /// Flags controlling input behavior.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct InputFlags: u32 {
        /// Input cannot be connected to other nodes (value only)
        const NOT_CONNECTABLE = 0b00000001;
        /// Input cannot be keyframed
        const NOT_KEYFRAMABLE = 0b00000010;
        /// Input is hidden from UI
        const HIDDEN = 0b00000100;
        /// Input is read-only
        const READ_ONLY = 0b00001000;
        /// Input should be displayed as a slider
        const SLIDER = 0b00010000;
        /// Input should be displayed as a color picker
        const COLOR_PICKER = 0b00100000;
    }
}

impl Default for InputFlags {
    fn default() -> Self {
        Self::empty()
    }
}

// ============================================================================
// INPUT DEFINITION
// ============================================================================

/// Definition of a node input parameter.
///
/// Describes everything needed to render UI and validate connections
/// for a node input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDefinition {
    /// Unique identifier for this input (e.g., "radius", "color")
    pub id: String,
    /// Display name for UI
    pub name: String,
    /// Data type this input accepts
    pub data_type: NodeValueType,
    /// Default value when not connected or keyframed
    pub default_value: NodeValue,
    /// Behavioral flags
    pub flags: InputFlags,
    /// Minimum value (for numeric types)
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    pub max: Option<f64>,
    /// Options for Combo type
    pub combo_options: Vec<String>,
    /// Tooltip / description
    pub description: String,
}

impl InputDefinition {
    /// Create a new input definition
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        data_type: NodeValueType,
        default_value: NodeValue,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            data_type,
            default_value,
            flags: InputFlags::empty(),
            min: None,
            max: None,
            combo_options: Vec::new(),
            description: String::new(),
        }
    }

    // ========================================================================
    // Convenience constructors
    // ========================================================================

    /// Create a boolean input
    pub fn boolean(id: impl Into<String>, name: impl Into<String>, default: bool) -> Self {
        Self::new(
            id,
            name,
            NodeValueType::Boolean,
            NodeValue::Boolean(default),
        )
    }

    /// Create an integer input
    pub fn integer(id: impl Into<String>, name: impl Into<String>, default: i64) -> Self {
        Self::new(
            id,
            name,
            NodeValueType::Integer,
            NodeValue::Integer(default),
        )
    }

    /// Create a float input
    pub fn float(id: impl Into<String>, name: impl Into<String>, default: f64) -> Self {
        Self::new(id, name, NodeValueType::Float, NodeValue::Float(default))
    }

    /// Create a Vec2 input
    pub fn vec2(id: impl Into<String>, name: impl Into<String>, default: glam::Vec2) -> Self {
        Self::new(id, name, NodeValueType::Vec2, NodeValue::Vec2(default))
    }

    /// Create a color input
    pub fn color(id: impl Into<String>, name: impl Into<String>, default: [f32; 4]) -> Self {
        Self::new(id, name, NodeValueType::Color, NodeValue::Color(default))
            .with_flags(InputFlags::COLOR_PICKER)
    }

    /// Create a texture input
    pub fn texture(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(id, name, NodeValueType::Texture, NodeValue::None)
    }

    /// Create a footage input
    pub fn footage(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(id, name, NodeValueType::Footage, NodeValue::None)
    }

    /// Create a combo (dropdown) input
    pub fn combo(
        id: impl Into<String>,
        name: impl Into<String>,
        options: &[&str],
        default_index: usize,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            data_type: NodeValueType::Combo,
            default_value: NodeValue::Combo(default_index),
            flags: InputFlags::NOT_CONNECTABLE,
            min: None,
            max: None,
            combo_options: options.iter().map(|s| s.to_string()).collect(),
            description: String::new(),
        }
    }

    /// Create a duration input
    pub fn duration(
        id: impl Into<String>,
        name: impl Into<String>,
        default: std::time::Duration,
    ) -> Self {
        Self::new(
            id,
            name,
            NodeValueType::Duration,
            NodeValue::Duration(default),
        )
    }

    // ========================================================================
    // Builder methods
    // ========================================================================

    /// Add flags
    pub fn with_flags(mut self, flags: InputFlags) -> Self {
        self.flags |= flags;
        self
    }

    /// Set minimum value
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Set maximum value
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Set min and max range
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    // ========================================================================
    // Query methods
    // ========================================================================

    /// Check if this input is connectable
    pub fn is_connectable(&self) -> bool {
        !self.flags.contains(InputFlags::NOT_CONNECTABLE)
    }

    /// Check if this input is keyframable
    pub fn is_keyframable(&self) -> bool {
        !self.flags.contains(InputFlags::NOT_KEYFRAMABLE)
    }

    /// Check if this input is hidden
    pub fn is_hidden(&self) -> bool {
        self.flags.contains(InputFlags::HIDDEN)
    }
}

// ============================================================================
// NODE INPUT
// ============================================================================

/// Reference to a specific input on a specific node.
///
/// Used for connections and keyframe targeting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeInput {
    /// UUID of the node
    pub node_uuid: Uuid,
    /// ID of the input on that node
    pub input_id: String,
    /// Element index for array inputs (-1 for non-array)
    pub element: i32,
}

impl NodeInput {
    /// Create a new node input reference
    pub fn new(node_uuid: Uuid, input_id: impl Into<String>) -> Self {
        Self {
            node_uuid,
            input_id: input_id.into(),
            element: -1,
        }
    }

    /// Create with element index (for array inputs)
    pub fn with_element(node_uuid: Uuid, input_id: impl Into<String>, element: i32) -> Self {
        Self {
            node_uuid,
            input_id: input_id.into(),
            element,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_definition_builders() {
        let input = InputDefinition::float("radius", "Radius", 10.0)
            .with_range(0.0, 100.0)
            .with_description("Blur radius in pixels");

        assert_eq!(input.id, "radius");
        assert_eq!(input.min, Some(0.0));
        assert_eq!(input.max, Some(100.0));
        assert!(input.is_connectable());
        assert!(input.is_keyframable());
    }

    #[test]
    fn test_combo_input() {
        let input =
            InputDefinition::combo("method", "Method", &["Gaussian", "Box", "Directional"], 0);

        assert_eq!(input.combo_options.len(), 3);
        assert!(!input.is_connectable()); // Combos are not connectable by default
    }

    #[test]
    fn test_input_flags() {
        let input =
            InputDefinition::float("speed", "Speed", 1.0).with_flags(InputFlags::NOT_KEYFRAMABLE);

        assert!(!input.is_keyframable());
        assert!(input.is_connectable());
    }
}
