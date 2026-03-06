//! # Gizmo Commands
//!
//! Composite commands for viewport gizmo interactions — the bridge between
//! the gizmo math engine (`gizmo/`) and the undo/redo stack.
//!
//! ## Why Composite Commands?
//!
//! When the user drags a corner handle, the resulting transform changes
//! position, scale, and possibly rotation simultaneously. Using three
//! separate atomic commands (`SetPositionCommand` + `SetScaleCommand` +
//! `SetRotationCommand`) would create 3 undo steps for what the user
//! perceives as one action. `GizmoDragCommand` stores the full before/after
//! `Transform2D` for a single Ctrl+Z.
//!
//! ## UUID-Based Entity Resolution
//!
//! Following the project's golden rule §2.4, all commands resolve entities
//! via `StableIdRegistry` (UUID → Entity) rather than raw `Entity` IDs.
//! This is critical because ECS Entity IDs become stale after undo/redo
//! cycles that involve despawn/respawn. The pattern is:
//!
//! 1. On first `execute()`: resolve entity, cache UUID from `StableId` component
//! 2. On `undo()`: resolve UUID → Entity via `StableIdRegistry`
//! 3. On redo: UUID is already cached, so lookup works even if Entity ID changed
//!
//! ## Commands
//!
//! - [`GizmoDragCommand`] — batches move+scale+rotation into one undo step
//! - [`FlipCommand`] — horizontal or vertical flip via scale negation

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid::Uuid;

use super::{Command, CommandError, UndoData};
use crate::components::{StableId, Transform2D};
use crate::impl_serializable_command;
use crate::resources::StableIdRegistry;

// ============================================================================
// ENTITY RESOLUTION HELPER
// ============================================================================

/// Resolve an entity from UUID (preferred) or fallback to raw entity bits.
///
/// This is the standard resolution pattern used across all UUID-aware commands.
/// UUID path is always preferred because Entity IDs become stale after undo/redo.
fn resolve_entity_from_uuid_or_bits(
    world: &World,
    target_uuid: Option<Uuid>,
    entity_bits_fallback: u64,
    context: &str,
) -> Result<Entity, CommandError> {
    if let Some(uuid) = target_uuid {
        world
            .resource::<StableIdRegistry>()
            .get(&uuid)
            .ok_or_else(|| {
                CommandError::EntityNotFound(format!(
                    "{}: UUID {} not found in StableIdRegistry",
                    context,
                    &uuid.to_string()[..8]
                ))
            })
    } else {
        Ok(Entity::from_bits(entity_bits_fallback))
    }
}

/// Cache the UUID from a resolved entity's StableId component.
///
/// Called on first execute to ensure subsequent undo/redo can find the entity
/// even if its raw Entity ID has changed.
fn cache_uuid_from_entity(world: &World, entity: Entity, uuid_slot: &mut Option<Uuid>) {
    if uuid_slot.is_none() {
        if let Some(stable_id) = world.get::<StableId>(entity) {
            *uuid_slot = Some(stable_id.uuid());
        }
    }
}

// ============================================================================
// GIZMO DRAG COMMAND
// ============================================================================

/// Composite command for a complete gizmo drag gesture.
///
/// Stores the full `Transform2D` before and after the drag, enabling
/// a single Ctrl+Z to revert the entire gesture regardless of whether
/// it changed position, scale, rotation, or all three.
///
/// ## AI Schema
/// ```json
/// {
///   "command": "gizmo_drag",
///   "target_uuid": "a1b2c3d4-...",
///   "transform": {
///     "position": [100.0, 50.0],
///     "scale": [1.5, 1.5],
///     "rotation": 0.785,
///     "anchor": [0.5, 0.5],
///     "opacity": 1.0
///   }
/// }
/// ```
///
/// ## Merge Behavior
///
/// During an active drag, multiple `GizmoDragCommand`s for the same entity
/// are merged: the final transform is updated but the original (pre-drag)
/// transform is preserved. This prevents flooding the undo stack during
/// a single drag gesture while ensuring a clean single undo step.
///
/// ## Undo/Redo Safety
///
/// The `target_uuid` persists through undo/redo cycles. On redo, the entity
/// is resolved via `StableIdRegistry` since the raw `Entity` ID may have
/// changed if the entity was despawned and respawned during undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GizmoDragCommand {
    /// Target entity bits (initial creation path, may become stale)
    pub entity_bits: u64,
    /// The desired final transform after the drag
    pub new_transform: Transform2D,

    // --- UUID for undo/redo robustness ---
    /// UUID of the target entity (populated on first execute, persists through undo/redo)
    #[serde(skip)]
    target_uuid: Option<Uuid>,

    // --- Undo data ---
    /// The entity's full Transform2D captured at drag start.
    /// On undo, this is restored in its entirety.
    #[serde(skip)]
    original_transform: Option<Transform2D>,
}

impl GizmoDragCommand {
    /// Create a new drag command from a resolved entity.
    ///
    /// The UUID is cached on first `execute()`. For pre-caching, use
    /// [`with_uuid`](Self::with_uuid) or [`with_original_transform`](Self::with_original_transform).
    pub fn new(entity: Entity, new_transform: Transform2D) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            new_transform,
            target_uuid: None,
            original_transform: None,
        }
    }

    /// Create a drag command with a pre-cached UUID.
    ///
    /// Preferred when the caller already has the UUID (e.g., from the
    /// gizmo state machine which resolved it at press time).
    pub fn with_uuid(target_uuid: Uuid, new_transform: Transform2D) -> Self {
        Self {
            entity_bits: 0, // Will be resolved from UUID
            new_transform,
            target_uuid: Some(target_uuid),
            original_transform: None,
        }
    }

    /// Create a drag command with a known original transform.
    ///
    /// Used when the gizmo state machine already captured the transform
    /// at drag start — more reliable than querying at execute time because
    /// the entity's transform may have been modified by intermediate
    /// preview updates during the drag gesture.
    pub fn with_original_transform(
        target_uuid: Uuid,
        new_transform: Transform2D,
        original_transform: Transform2D,
    ) -> Self {
        Self {
            entity_bits: 0,
            new_transform,
            target_uuid: Some(target_uuid),
            original_transform: Some(original_transform),
        }
    }
}

impl Command for GizmoDragCommand {
    fn name(&self) -> &'static str {
        "Gizmo Drag"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Step 1: Resolve entity (UUID preferred, bits fallback)
        let entity: Entity = resolve_entity_from_uuid_or_bits(
            world,
            self.target_uuid,
            self.entity_bits,
            "GizmoDragCommand",
        )?;

        // Step 2: Cache UUID for future undo/redo cycles
        cache_uuid_from_entity(world, entity, &mut self.target_uuid);

        // Step 3: Get mutable Transform2D
        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "GizmoDragCommand: Entity {:?} missing Transform2D component",
                entity
            ))
        })?;

        // Step 4: Capture original transform if not already set
        // (First execute captures it; redo reuses the cached value)
        if self.original_transform.is_none() {
            self.original_transform = Some(transform.clone());
        }

        let old_transform: Transform2D = transform.clone();

        // Step 5: Apply the full new transform (all fields replaced atomically)
        transform.position = self.new_transform.position;
        transform.scale = self.new_transform.scale;
        transform.rotation = self.new_transform.rotation;
        transform.anchor = self.new_transform.anchor;
        transform.opacity = self.new_transform.opacity;

        tracing::trace!(
            "GizmoDrag: pos ({:.1},{:.1})→({:.1},{:.1}), scale ({:.2},{:.2})→({:.2},{:.2}), rot {:.3}→{:.3}",
            old_transform.position.x, old_transform.position.y,
            self.new_transform.position.x, self.new_transform.position.y,
            old_transform.scale.x, old_transform.scale.y,
            self.new_transform.scale.x, self.new_transform.scale.y,
            old_transform.rotation, self.new_transform.rotation,
        );

        Ok(UndoData::PropertyChanged {
            entity_bits: entity.to_bits(),
            property: "transform2d_composite".to_string(),
            old_value: serde_json::json!({
                "position": [old_transform.position.x, old_transform.position.y],
                "scale": [old_transform.scale.x, old_transform.scale.y],
                "rotation": old_transform.rotation,
                "anchor": [old_transform.anchor.x, old_transform.anchor.y],
                "opacity": old_transform.opacity,
            }),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let original: Transform2D = self.original_transform.clone().ok_or_else(|| {
            CommandError::InvalidOperation(
                "GizmoDragCommand: No original transform stored — cannot undo".to_string(),
            )
        })?;

        // Resolve entity from UUID (robust against Entity ID changes after undo/redo)
        let entity: Entity = resolve_entity_from_uuid_or_bits(
            world,
            self.target_uuid,
            self.entity_bits,
            "GizmoDragCommand::undo",
        )?;

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "GizmoDragCommand::undo: Entity {:?} missing Transform2D",
                entity
            ))
        })?;

        // Restore the full original transform atomically
        transform.position = original.position;
        transform.scale = original.scale;
        transform.rotation = original.rotation;
        transform.anchor = original.anchor;
        transform.opacity = original.opacity;

        tracing::trace!(
            "GizmoDrag undo: restored to pos ({:.1},{:.1})",
            original.position.x,
            original.position.y,
        );

        Ok(())
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        if let Some(other_cmd) = other.as_any().downcast_ref::<GizmoDragCommand>() {
            // Merge by UUID if both have it (most robust)
            if let (Some(our_uuid), Some(their_uuid)) = (self.target_uuid, other_cmd.target_uuid) {
                return our_uuid == their_uuid;
            }
            // Fallback: merge by entity bits
            other_cmd.entity_bits == self.entity_bits
        } else {
            false
        }
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(other_cmd) = other.as_any().downcast_ref::<GizmoDragCommand>() {
            // `self` is the NEWEST command, `other` is the PREVIOUS command.
            // We keep self.new_transform (the latest position) and take
            // other's original_transform (the very first position in the chain).
            if self.original_transform.is_none() {
                self.original_transform = other_cmd.original_transform.clone();
            }
            // Also inherit UUID if we don't have one
            if self.target_uuid.is_none() {
                self.target_uuid = other_cmd.target_uuid;
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
// FLIP COMMAND
// ============================================================================

/// Axis for flip operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlipAxis {
    /// Flip horizontally (mirror left↔right, negates scale.x)
    Horizontal,
    /// Flip vertically (mirror top↔bottom, negates scale.y)
    Vertical,
}

/// Command to flip an element horizontally or vertically.
///
/// Implemented as scale negation on the appropriate axis — this is how
/// CapCut and Canva implement flipping. It preserves the element's
/// position and only mirrors the visual output.
///
/// ## Undo/Redo Safety
///
/// Stores the full old scale (both axes) before flipping, not just the
/// negated value. This handles edge cases where scale was already negative
/// (e.g., flip-flip-undo should restore the intermediate negative state).
///
/// ## AI Schema
/// ```json
/// {
///   "command": "flip",
///   "target_uuid": "a1b2c3d4-...",
///   "axis": "horizontal"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipCommand {
    /// Target entity bits (initial path, may become stale after undo/redo)
    pub entity_bits: u64,
    /// Which axis to flip
    pub axis: FlipAxis,

    // --- UUID for undo/redo robustness ---
    /// UUID of the target entity (cached on first execute)
    #[serde(skip)]
    target_uuid: Option<Uuid>,

    // --- Undo data ---
    /// Full old scale stored for undo (both axes, not just the flipped one)
    #[serde(skip)]
    old_scale: Option<Vec2>,
}

impl FlipCommand {
    /// Create a flip command from a resolved entity.
    pub fn new(entity: Entity, axis: FlipAxis) -> Self {
        Self {
            entity_bits: entity.to_bits(),
            axis,
            target_uuid: None,
            old_scale: None,
        }
    }

    /// Create a flip command with a pre-cached UUID.
    pub fn with_uuid(target_uuid: Uuid, axis: FlipAxis) -> Self {
        Self {
            entity_bits: 0,
            axis,
            target_uuid: Some(target_uuid),
            old_scale: None,
        }
    }
}

impl Command for FlipCommand {
    fn name(&self) -> &'static str {
        match self.axis {
            FlipAxis::Horizontal => "Flip Horizontal",
            FlipAxis::Vertical => "Flip Vertical",
        }
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        // Step 1: Resolve entity
        let entity: Entity = resolve_entity_from_uuid_or_bits(
            world,
            self.target_uuid,
            self.entity_bits,
            "FlipCommand",
        )?;

        // Step 2: Cache UUID
        cache_uuid_from_entity(world, entity, &mut self.target_uuid);

        // Step 3: Get mutable Transform2D
        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "FlipCommand: Entity {:?} missing Transform2D",
                entity
            ))
        })?;

        // Step 4: Store full old scale for undo
        self.old_scale = Some(transform.scale);
        let old_scale: Vec2 = transform.scale;

        // Step 5: Negate the appropriate scale axis
        match self.axis {
            FlipAxis::Horizontal => {
                transform.scale.x = -transform.scale.x;
            }
            FlipAxis::Vertical => {
                transform.scale.y = -transform.scale.y;
            }
        }

        tracing::trace!(
            "Flip {:?}: scale ({:.2},{:.2})→({:.2},{:.2})",
            self.axis,
            old_scale.x,
            old_scale.y,
            transform.scale.x,
            transform.scale.y,
        );

        Ok(UndoData::PropertyChanged {
            entity_bits: entity.to_bits(),
            property: "scale".to_string(),
            old_value: serde_json::json!([old_scale.x, old_scale.y]),
        })
    }

    fn undo(&mut self, world: &mut World, _undo_data: UndoData) -> Result<(), CommandError> {
        let old_scale: Vec2 = self.old_scale.ok_or_else(|| {
            CommandError::InvalidOperation(
                "FlipCommand: No old scale stored — cannot undo".to_string(),
            )
        })?;

        let entity: Entity = resolve_entity_from_uuid_or_bits(
            world,
            self.target_uuid,
            self.entity_bits,
            "FlipCommand::undo",
        )?;

        let mut query = world.query::<&mut Transform2D>();
        let mut transform = query.get_mut(world, entity).map_err(|_| {
            CommandError::EntityNotFound(format!(
                "FlipCommand::undo: Entity {:?} missing Transform2D",
                entity
            ))
        })?;

        transform.scale = old_scale;

        tracing::trace!(
            "Flip undo: restored scale ({:.2},{:.2})",
            old_scale.x,
            old_scale.y
        );

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
// SERIALIZABLE COMMAND REGISTRATION
// ============================================================================

impl_serializable_command!(GizmoDragCommand, "gizmo_drag");
impl_serializable_command!(FlipCommand, "flip");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    /// Spawn a test entity with Transform2D + StableId, register in StableIdRegistry.
    fn setup_test_world_with_uuid() -> (World, Entity, Uuid) {
        let mut world: World = World::new();
        world.insert_resource(StableIdRegistry::default());

        let uuid: Uuid = Uuid::new_v4();
        let entity: Entity = world
            .spawn((
                Transform2D {
                    position: Vec2::new(100.0, 50.0),
                    scale: Vec2::new(1.0, 1.0),
                    rotation: 0.0,
                    anchor: Vec2::splat(0.5),
                    opacity: 1.0,
                },
                StableId::from_uuid(uuid),
            ))
            .id();

        world
            .resource_mut::<StableIdRegistry>()
            .register(uuid, entity);

        (world, entity, uuid)
    }

    // --- GizmoDragCommand tests ---

    #[test]
    fn test_gizmo_drag_execute_and_undo() {
        let (mut world, entity, _uuid) = setup_test_world_with_uuid();

        let new_transform: Transform2D = Transform2D {
            position: Vec2::new(200.0, 150.0),
            scale: Vec2::new(2.0, 2.0),
            rotation: 0.5,
            anchor: Vec2::splat(0.5),
            opacity: 0.8,
        };

        let mut cmd: GizmoDragCommand = GizmoDragCommand::new(entity, new_transform);
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute should succeed");

        // Verify UUID was cached
        assert!(
            cmd.target_uuid.is_some(),
            "UUID should be cached on execute"
        );

        // Verify transform was applied
        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.position.x - 200.0).abs() < 0.001);
            assert!((t.position.y - 150.0).abs() < 0.001);
            assert!((t.scale.x - 2.0).abs() < 0.001);
            assert!((t.rotation - 0.5).abs() < 0.001);
            assert!((t.opacity - 0.8).abs() < 0.001);
        }

        // Undo
        cmd.undo(&mut world, undo_data)
            .expect("undo should succeed");

        // Verify full original transform was restored
        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.position.x - 100.0).abs() < 0.001);
            assert!((t.position.y - 50.0).abs() < 0.001);
            assert!((t.scale.x - 1.0).abs() < 0.001);
            assert!((t.rotation).abs() < 0.001);
            assert!((t.opacity - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_gizmo_drag_with_uuid_constructor() {
        let (mut world, _entity, uuid) = setup_test_world_with_uuid();

        let new_transform: Transform2D = Transform2D {
            position: Vec2::new(300.0, 250.0),
            scale: Vec2::new(3.0, 3.0),
            rotation: 1.57,
            anchor: Vec2::splat(0.5),
            opacity: 0.5,
        };

        // Use UUID-based constructor (entity_bits is 0, should resolve via UUID)
        let mut cmd: GizmoDragCommand = GizmoDragCommand::with_uuid(uuid, new_transform);
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute ok");

        // Verify it resolved and applied correctly
        {
            let entity_resolved: Entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
            let t: &Transform2D = world.get::<Transform2D>(entity_resolved).unwrap();
            assert!((t.position.x - 300.0).abs() < 0.001);
            assert!((t.scale.x - 3.0).abs() < 0.001);
        }

        // Undo should restore via UUID
        cmd.undo(&mut world, undo_data).expect("undo ok");
        {
            let entity_resolved: Entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
            let t: &Transform2D = world.get::<Transform2D>(entity_resolved).unwrap();
            assert!((t.position.x - 100.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_gizmo_drag_with_explicit_original() {
        let (mut world, _entity, uuid) = setup_test_world_with_uuid();

        let original: Transform2D = Transform2D {
            position: Vec2::new(100.0, 50.0),
            scale: Vec2::ONE,
            rotation: 0.0,
            anchor: Vec2::splat(0.5),
            opacity: 1.0,
        };

        let new_transform: Transform2D = Transform2D {
            position: Vec2::new(500.0, 400.0),
            scale: Vec2::new(4.0, 4.0),
            rotation: 3.14,
            anchor: Vec2::splat(0.5),
            opacity: 0.2,
        };

        let mut cmd: GizmoDragCommand =
            GizmoDragCommand::with_original_transform(uuid, new_transform, original.clone());
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute ok");

        // Undo should restore the explicitly provided original (not re-captured)
        cmd.undo(&mut world, undo_data).expect("undo ok");
        {
            let entity: Entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!(
                (t.position.x - 100.0).abs() < 0.001,
                "pos.x={}",
                t.position.x
            );
            assert!((t.opacity - 1.0).abs() < 0.001, "opacity={}", t.opacity);
        }
    }

    #[test]
    fn test_gizmo_drag_merge_preserves_original() {
        let (_world, entity, uuid) = setup_test_world_with_uuid();

        let original: Transform2D = Transform2D::from_position(100.0, 50.0);

        // First command in the chain (has the original transform)
        let first_cmd: GizmoDragCommand = GizmoDragCommand {
            entity_bits: entity.to_bits(),
            new_transform: Transform2D::from_position(110.0, 55.0),
            target_uuid: Some(uuid),
            original_transform: Some(original.clone()),
        };

        // Second command (newer — doesn't have original yet)
        let mut second_cmd: GizmoDragCommand = GizmoDragCommand {
            entity_bits: entity.to_bits(),
            new_transform: Transform2D::from_position(120.0, 60.0),
            target_uuid: Some(uuid),
            original_transform: None,
        };

        assert!(second_cmd.can_merge(&first_cmd));
        second_cmd.merge(Box::new(first_cmd));

        // After merge: newest command keeps its new_transform but inherits
        // the original_transform from the first command in the chain
        assert!(second_cmd.original_transform.is_some());
        assert!(
            (second_cmd.original_transform.as_ref().unwrap().position.x - 100.0).abs() < 0.001,
            "Should inherit first cmd's original"
        );
        assert!(
            (second_cmd.new_transform.position.x - 120.0).abs() < 0.001,
            "Should keep newest transform"
        );
    }

    #[test]
    fn test_gizmo_drag_merge_by_uuid() {
        let uuid: Uuid = Uuid::new_v4();

        let cmd_a: GizmoDragCommand = GizmoDragCommand {
            entity_bits: 0,
            new_transform: Transform2D::default(),
            target_uuid: Some(uuid),
            original_transform: None,
        };

        let cmd_b: GizmoDragCommand = GizmoDragCommand {
            entity_bits: 999, // Different bits, same UUID
            new_transform: Transform2D::default(),
            target_uuid: Some(uuid),
            original_transform: None,
        };

        // Should merge because UUIDs match (even though entity_bits differ)
        assert!(cmd_b.can_merge(&cmd_a));
    }

    // --- FlipCommand tests ---

    #[test]
    fn test_flip_horizontal_execute_and_undo() {
        let (mut world, entity, _uuid) = setup_test_world_with_uuid();

        let mut cmd: FlipCommand = FlipCommand::new(entity, FlipAxis::Horizontal);
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute ok");

        // UUID should be cached
        assert!(cmd.target_uuid.is_some());

        // scale.x should be negated
        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - (-1.0)).abs() < 0.001, "scale.x={}", t.scale.x);
            assert!((t.scale.y - 1.0).abs() < 0.001, "scale.y unchanged");
        }

        // Undo
        cmd.undo(&mut world, undo_data).expect("undo ok");
        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - 1.0).abs() < 0.001, "scale.x restored");
            assert!((t.scale.y - 1.0).abs() < 0.001, "scale.y unchanged");
        }
    }

    #[test]
    fn test_flip_vertical_execute_and_undo() {
        let (mut world, entity, _uuid) = setup_test_world_with_uuid();

        let mut cmd: FlipCommand = FlipCommand::new(entity, FlipAxis::Vertical);
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute ok");

        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - 1.0).abs() < 0.001, "scale.x unchanged");
            assert!((t.scale.y - (-1.0)).abs() < 0.001, "scale.y negated");
        }

        cmd.undo(&mut world, undo_data).expect("undo ok");
        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.y - 1.0).abs() < 0.001, "scale.y restored");
        }
    }

    #[test]
    fn test_flip_via_uuid_constructor() {
        let (mut world, _entity, uuid) = setup_test_world_with_uuid();

        let mut cmd: FlipCommand = FlipCommand::with_uuid(uuid, FlipAxis::Horizontal);
        let undo_data: UndoData = cmd.execute(&mut world).expect("execute ok");

        {
            let entity: Entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - (-1.0)).abs() < 0.001);
        }

        cmd.undo(&mut world, undo_data).expect("undo ok");
        {
            let entity: Entity = world.resource::<StableIdRegistry>().get(&uuid).unwrap();
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_double_flip_is_identity() {
        let (mut world, entity, _uuid) = setup_test_world_with_uuid();

        let mut cmd1: FlipCommand = FlipCommand::new(entity, FlipAxis::Horizontal);
        cmd1.execute(&mut world).expect("flip 1");

        let mut cmd2: FlipCommand = FlipCommand::new(entity, FlipAxis::Horizontal);
        cmd2.execute(&mut world).expect("flip 2");

        {
            let t: &Transform2D = world.get::<Transform2D>(entity).unwrap();
            assert!((t.scale.x - 1.0).abs() < 0.001, "double flip = identity");
        }
    }

    #[test]
    fn test_command_names() {
        let (_, entity, _uuid) = setup_test_world_with_uuid();

        let drag: GizmoDragCommand = GizmoDragCommand::new(entity, Transform2D::default());
        assert_eq!(drag.name(), "Gizmo Drag");

        let flip_h: FlipCommand = FlipCommand::new(entity, FlipAxis::Horizontal);
        assert_eq!(flip_h.name(), "Flip Horizontal");

        let flip_v: FlipCommand = FlipCommand::new(entity, FlipAxis::Vertical);
        assert_eq!(flip_v.name(), "Flip Vertical");
    }
}
