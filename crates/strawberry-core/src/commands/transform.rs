//! # Transform Commands
//!
//! AI-invokable commands for manipulating Transform2D components.
//!
//! ## Usage (AI/UI)
//! ```ignore
//! // Set position
//! command_queue.push(Box::new(SetPositionCommand {
//!     entity: clip_entity,
//!     position: Vec2::new(100.0, 50.0),
//! }));
//!
//! // Set scale
//! command_queue.push(Box::new(SetScaleCommand {
//!     entity: clip_entity,
//!     scale: Vec2::new(1.5, 1.5),
//! }));
//! ```

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::{Command, CommandError, UndoData};
use crate::components::Transform2D;
use crate::impl_serializable_command;

// ============================================================================
// SET POSITION COMMAND
// ============================================================================

/// Command to set the position of an entity's Transform2D
///
/// ## AI Schema
/// ```json
/// {
///   "command": "set_position",
///   "entity": 12345,
///   "x": 100.0,
///   "y": 50.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPositionCommand {
    /// Target entity (as u64 for serialization)
    pub entity_bits: u64,
    /// New position
    pub position: Vec2,
    /// Stored old position for undo
    #[serde(skip)]
    old_position: Option<Vec2>,
}

impl SetPositionCommand {
    pub fn new(entity: Entity, position: Vec2) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            position,
            old_position: None,
        }
    }
}

impl Command for SetPositionCommand {
    fn name(&self) -> &'static str {
        "Set Position"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        // Get current transform
        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "Entity {} not found or missing Transform2D",
                self.entity_bits
            ))
        })?;

        // Store old value for undo
        self.old_position = Some(transform.position);
        let old_pos = transform.position;

        // Apply new position
        transform.position = self.position;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "position".to_string(),
            old_value: serde_json::json!([old_pos.x, old_pos.y]),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(old_pos) = self.old_position {
            let entity = Entity::from_bits(self.entity_bits);
            let mut query = world.query::<&mut Transform2D>();
            let mut transform = query.get_mut(world, entity).map_err(|_| {
                CommandError::EntityNotFound(format!("Entity {} not found", self.entity_bits))
            })?;
            transform.position = old_pos;
        }
        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        // Merge consecutive position changes on the same entity (for dragging)
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetPositionCommand>() {
            other_cmd.entity_bits == self.entity_bits
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        // Keep our target position (the final one), but preserve the original old_position
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetPositionCommand>() {
            // Keep the original old_position from the first command
            if self.old_position.is_none() {
                self.old_position = other_cmd.old_position;
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET SCALE COMMAND
// ============================================================================

/// Command to set the scale of an entity's Transform2D
///
/// ## AI Schema
/// ```json
/// {
///   "command": "set_scale",
///   "entity": 12345,
///   "scale_x": 1.5,
///   "scale_y": 1.5
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetScaleCommand {
    /// Target entity
    pub entity_bits: u64,
    /// New scale
    pub scale: Vec2,
    /// Stored old scale for undo
    #[serde(skip)]
    old_scale: Option<Vec2>,
}

impl SetScaleCommand {
    pub fn new(entity: Entity, scale: Vec2) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            scale,
            old_scale: None,
        }
    }
}

impl Command for SetScaleCommand {
    fn name(&self) -> &'static str {
        "Set Scale"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "Entity {} not found or missing Transform2D",
                self.entity_bits
            ))
        })?;

        self.old_scale = Some(transform.scale);
        let old = transform.scale;
        transform.scale = self.scale;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "scale".to_string(),
            old_value: serde_json::json!([old.x, old.y]),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(old) = self.old_scale {
            let entity = Entity::from_bits(self.entity_bits);
            let mut query = world.query::<&mut Transform2D>();
            let mut transform = query.get_mut(world, entity).map_err(|_| {
                CommandError::EntityNotFound(format!("Entity {} not found", self.entity_bits))
            })?;
            transform.scale = old;
        }
        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetScaleCommand>() {
            other_cmd.entity_bits == self.entity_bits
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetScaleCommand>() {
            if self.old_scale.is_none() {
                self.old_scale = other_cmd.old_scale;
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET ROTATION COMMAND
// ============================================================================

/// Command to set the rotation of an entity's Transform2D
///
/// ## AI Schema
/// ```json
/// {
///   "command": "set_rotation",
///   "entity": 12345,
///   "rotation_degrees": 45.0
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetRotationCommand {
    /// Target entity
    pub entity_bits: u64,
    /// New rotation in radians
    pub rotation: f32,
    /// Stored old rotation for undo
    #[serde(skip)]
    old_rotation: Option<f32>,
}

impl SetRotationCommand {
    pub fn new(entity: Entity, rotation_radians: f32) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            rotation: rotation_radians,
            old_rotation: None,
        }
    }

    pub fn from_degrees(entity: Entity, degrees: f32) -> Self {
        Self::new(entity, degrees.to_radians())
    }
}

impl Command for SetRotationCommand {
    fn name(&self) -> &'static str {
        "Set Rotation"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "Entity {} not found or missing Transform2D",
                self.entity_bits
            ))
        })?;

        self.old_rotation = Some(transform.rotation);
        let old = transform.rotation;
        transform.rotation = self.rotation;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "rotation".to_string(),
            old_value: serde_json::json!(old),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(old) = self.old_rotation {
            let entity = Entity::from_bits(self.entity_bits);
            let mut query = world.query::<&mut Transform2D>();
            let mut transform = query.get_mut(world, entity).map_err(|_| {
                CommandError::EntityNotFound(format!("Entity {} not found", self.entity_bits))
            })?;
            transform.rotation = old;
        }
        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetRotationCommand>() {
            other_cmd.entity_bits == self.entity_bits
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<SetRotationCommand>() {
            if self.old_rotation.is_none() {
                self.old_rotation = other_cmd.old_rotation;
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET OPACITY COMMAND
// ============================================================================

/// Command to set the opacity of an entity's Transform2D
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetOpacityCommand {
    pub entity_bits: u64,
    pub opacity: f32,
    #[serde(skip)]
    old_opacity: Option<f32>,
}

impl SetOpacityCommand {
    pub fn new(entity: Entity, opacity: f32) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            opacity: opacity.clamp(0.0, 1.0),
            old_opacity: None,
        }
    }
}

impl Command for SetOpacityCommand {
    fn name(&self) -> &'static str {
        "Set Opacity"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "Entity {} not found or missing Transform2D",
                self.entity_bits
            ))
        })?;

        self.old_opacity = Some(transform.opacity);
        let old = transform.opacity;
        transform.opacity = self.opacity;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "opacity".to_string(),
            old_value: serde_json::json!(old),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(old) = self.old_opacity {
            let entity = Entity::from_bits(self.entity_bits);
            let mut query = world.query::<&mut Transform2D>();
            let mut transform = query.get_mut(world, entity).map_err(|_| {
                CommandError::EntityNotFound(format!("Entity {} not found", self.entity_bits))
            })?;
            transform.opacity = old;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// SET ANCHOR COMMAND
// ============================================================================

/// Command to set the anchor point of an entity's Transform2D
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetAnchorCommand {
    pub entity_bits: u64,
    pub anchor: Vec2,
    #[serde(skip)]
    old_anchor: Option<Vec2>,
}

impl SetAnchorCommand {
    pub fn new(entity: Entity, anchor: Vec2) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            anchor,
            old_anchor: None,
        }
    }
}

impl Command for SetAnchorCommand {
    fn name(&self) -> &'static str {
        "Set Anchor"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = Entity::from_bits(self.entity_bits);

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "Entity {} not found or missing Transform2D",
                self.entity_bits
            ))
        })?;

        self.old_anchor = Some(transform.anchor);
        let old = transform.anchor;
        transform.anchor = self.anchor;

        Ok(UndoData::PropertyChanged {
            entity_bits: self.entity_bits,
            property: "anchor".to_string(),
            old_value: serde_json::json!([old.x, old.y]),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        if let Some(old) = self.old_anchor {
            let entity = Entity::from_bits(self.entity_bits);
            let mut query = world.query::<&mut Transform2D>();
            let mut transform = query.get_mut(world, entity).map_err(|_| {
                CommandError::EntityNotFound(format!("Entity {} not found", self.entity_bits))
            })?;
            transform.anchor = old;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// AI/JSON SERIALIZABLE IMPLEMENTATIONS
// ============================================================================

impl_serializable_command!(SetPositionCommand, "set_position");
impl_serializable_command!(SetScaleCommand, "set_scale");
impl_serializable_command!(SetRotationCommand, "set_rotation");
impl_serializable_command!(SetOpacityCommand, "set_opacity");
impl_serializable_command!(SetAnchorCommand, "set_anchor");

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    fn create_test_world() -> (World, Entity) {
        let mut world = World::new();
        let entity = world.spawn(Transform2D::default()).id();
        (world, entity)
    }

    #[test]
    fn test_set_position_command() {
        let (mut world, entity) = create_test_world();

        let mut cmd = SetPositionCommand::new(entity, Vec2::new(100.0, 50.0));
        let result = cmd.execute(&mut world);
        assert!(result.is_ok());

        let transform = world.get::<Transform2D>(entity).unwrap();
        assert_eq!(transform.position, Vec2::new(100.0, 50.0));
    }

    #[test]
    fn test_set_position_undo() {
        let (mut world, entity) = create_test_world();

        let mut cmd = SetPositionCommand::new(entity, Vec2::new(100.0, 50.0));
        let undo_data = cmd.execute(&mut world).unwrap();

        // Verify position changed
        assert_eq!(
            world.get::<Transform2D>(entity).unwrap().position,
            Vec2::new(100.0, 50.0)
        );

        // Undo
        cmd.undo(&mut world, undo_data).unwrap();

        // Verify position restored
        assert_eq!(
            world.get::<Transform2D>(entity).unwrap().position,
            Vec2::ZERO
        );
    }

    #[test]
    fn test_set_scale_command() {
        let (mut world, entity) = create_test_world();

        let mut cmd = SetScaleCommand::new(entity, Vec2::new(2.0, 2.0));
        cmd.execute(&mut world).unwrap();

        let transform = world.get::<Transform2D>(entity).unwrap();
        assert_eq!(transform.scale, Vec2::new(2.0, 2.0));
    }

    #[test]
    fn test_set_rotation_from_degrees() {
        let (mut world, entity) = create_test_world();

        let mut cmd = SetRotationCommand::from_degrees(entity, 90.0);
        cmd.execute(&mut world).unwrap();

        let transform = world.get::<Transform2D>(entity).unwrap();
        assert!((transform.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }
}
