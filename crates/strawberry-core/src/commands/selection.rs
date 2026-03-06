//! # Selection Commands
//!
//! Commands for selecting and deselecting entities in the timeline.
//! All commands are undoable and AI-invocable.

use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::commands::{Command, CommandError, UndoData};
use crate::resources::Selection;

// ============================================================================
// SELECT CLIP COMMAND
// ============================================================================

/// Command to select a clip entity
///
/// Supports both exclusive selection (clear others) and additive selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectClipCommand {
    /// Entity ID to select (as u64 for serialization)
    pub entity_bits: u64,
    /// If true, clear other selections first (exclusive mode)
    /// If false, add to existing selection (additive mode)
    pub exclusive: bool,
}

impl SelectClipCommand {
    /// Create a new exclusive selection command (select only this clip)
    pub fn exclusive(entity_bits: u64) -> Self {
        Self {
            entity_bits,
            exclusive: true,
        }
    }

    /// Create a new additive selection command (add to selection)
    pub fn additive(entity_bits: u64) -> Self {
        Self {
            entity_bits,
            exclusive: false,
        }
    }

    /// Create from JSON parameters (for AI invocation)
    pub fn from_params(entity_bits: u64, exclusive: bool) -> Self {
        Self {
            entity_bits,
            exclusive,
        }
    }
}

impl Command for SelectClipCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        // Store previous state for undo
        let previous_selection: Vec<u64> = selection.selected.iter().copied().collect();
        let previous_primary = selection.primary;

        if self.exclusive {
            selection.select_exclusive(self.entity_bits);
        } else {
            selection.select(self.entity_bits);
        }

        Ok(UndoData::Selection {
            previous_selected: previous_selection,
            previous_primary,
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        if let UndoData::Selection {
            previous_selected,
            previous_primary,
        } = undo_data
        {
            selection.selected.clear();
            for entity_bits in previous_selected {
                selection.selected.insert(entity_bits);
            }
            selection.primary = previous_primary;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Select Clip"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Selection changes are not undoable (like professional NLEs)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// CLEAR SELECTION COMMAND
// ============================================================================

/// Command to clear all selections
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClearSelectionCommand;

impl ClearSelectionCommand {
    pub fn new() -> Self {
        Self
    }
}

impl Command for ClearSelectionCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        // Store previous state for undo
        let previous_selection: Vec<u64> = selection.selected.iter().copied().collect();
        let previous_primary = selection.primary;

        selection.clear();

        Ok(UndoData::Selection {
            previous_selected: previous_selection,
            previous_primary,
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        if let UndoData::Selection {
            previous_selected,
            previous_primary,
        } = undo_data
        {
            for entity_bits in previous_selected {
                selection.selected.insert(entity_bits);
            }
            selection.primary = previous_primary;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Clear Selection"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Selection changes are not undoable
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// TOGGLE SELECTION COMMAND
// ============================================================================

/// Command to toggle selection of a clip (select if unselected, deselect if selected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleSelectionCommand {
    /// Entity ID to toggle
    pub entity_bits: u64,
}

impl ToggleSelectionCommand {
    pub fn new(entity_bits: u64) -> Self {
        Self { entity_bits }
    }
}

impl Command for ToggleSelectionCommand {
    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        // Store previous state for undo
        let previous_selection: Vec<u64> = selection.selected.iter().copied().collect();
        let previous_primary = selection.primary;

        selection.toggle(self.entity_bits);

        Ok(UndoData::Selection {
            previous_selected: previous_selection,
            previous_primary,
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        let mut selection = world.resource_mut::<Selection>();

        if let UndoData::Selection {
            previous_selected,
            previous_primary,
        } = undo_data
        {
            selection.selected.clear();
            for entity_bits in previous_selected {
                selection.selected.insert(entity_bits);
            }
            selection.primary = previous_primary;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Toggle Selection"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Selection changes are not undoable
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_world() -> World {
        let mut world = World::new();
        world.insert_resource(Selection::default());
        world
    }

    #[test]
    fn test_select_exclusive() {
        let mut world = setup_world();

        // First selection
        let mut cmd1 = SelectClipCommand::exclusive(100);
        let undo1 = cmd1.execute(&mut world).unwrap();

        {
            let selection = world.resource::<Selection>();
            assert!(selection.is_selected(100));
            assert_eq!(selection.count(), 1);
        }

        // Second exclusive selection should clear first
        let mut cmd2 = SelectClipCommand::exclusive(200);
        let _undo2 = cmd2.execute(&mut world).unwrap();

        {
            let selection = world.resource::<Selection>();
            assert!(!selection.is_selected(100));
            assert!(selection.is_selected(200));
            assert_eq!(selection.count(), 1);
        }

        // Undo first should not affect anything (second already cleared)
        cmd1.undo(&mut world, undo1).unwrap();
    }

    #[test]
    fn test_select_additive() {
        let mut world = setup_world();

        let mut cmd1 = SelectClipCommand::exclusive(100);
        let _undo1 = cmd1.execute(&mut world).unwrap();

        let mut cmd2 = SelectClipCommand::additive(200);
        let _undo2 = cmd2.execute(&mut world).unwrap();

        let selection = world.resource::<Selection>();
        assert!(selection.is_selected(100));
        assert!(selection.is_selected(200));
        assert_eq!(selection.count(), 2);
    }

    #[test]
    fn test_select_undo() {
        let mut world = setup_world();

        let mut cmd1 = SelectClipCommand::exclusive(100);
        let _undo1 = cmd1.execute(&mut world).unwrap();

        let mut cmd2 = SelectClipCommand::exclusive(200);
        let undo2 = cmd2.execute(&mut world).unwrap();

        // Undo second selection
        cmd2.undo(&mut world, undo2).unwrap();

        let selection = world.resource::<Selection>();
        assert!(selection.is_selected(100));
        assert!(!selection.is_selected(200));
    }

    #[test]
    fn test_clear_selection() {
        let mut world = setup_world();

        // Add some selections
        {
            let mut selection = world.resource_mut::<Selection>();
            selection.select(100);
            selection.select(200);
        }

        let mut clear_cmd = ClearSelectionCommand::new();
        let undo_data = clear_cmd.execute(&mut world).unwrap();

        {
            let selection = world.resource::<Selection>();
            assert!(selection.is_empty());
        }

        // Undo should restore
        clear_cmd.undo(&mut world, undo_data).unwrap();

        let selection = world.resource::<Selection>();
        assert!(selection.is_selected(100));
        assert!(selection.is_selected(200));
    }

    #[test]
    fn test_toggle_selection() {
        let mut world = setup_world();

        let mut cmd = ToggleSelectionCommand::new(100);

        // Toggle on
        let undo1 = cmd.execute(&mut world).unwrap();
        assert!(world.resource::<Selection>().is_selected(100));

        // Toggle off (new command instance)
        let mut cmd2 = ToggleSelectionCommand::new(100);
        let _undo2 = cmd2.execute(&mut world).unwrap();
        assert!(!world.resource::<Selection>().is_selected(100));

        // Undo first should have no effect (already toggled twice)
        cmd.undo(&mut world, undo1).unwrap();
    }
}
