//! # Command Executor System
//!
//! System that processes the command queue each frame.

use bevy_ecs::prelude::*;

use crate::commands::{CommandId, UndoEntry};
use crate::events::CommandExecutedEvent;
use crate::resources::{CommandQueue, UndoRedoStack};

/// System that processes all pending commands from the queue
///
/// This runs each frame and:
/// 1. Takes all commands from the queue
/// 2. Executes each command
/// 3. Pushes successful commands to the undo stack
/// 4. Emits CommandExecutedEvent for each command
pub fn execute_commands_system(world: &mut World) {
    // Take all pending commands
    let commands = {
        let queue = world.resource::<CommandQueue>();
        queue.take_all()
    };

    if commands.is_empty() {
        return;
    }

    for mut command in commands {
        let id = CommandId::new();
        let name = command.name();

        match command.execute(world) {
            Ok(undo_data) => {
                // Only push to undo stack if command is undoable
                // (Editing commands like Delete, Split, Move are undoable)
                // (Playback commands like Seek, Play, Pause are NOT undoable)
                if command.is_undoable() {
                    let mut undo_stack = world.resource_mut::<UndoRedoStack>();
                    undo_stack.push(UndoEntry {
                        id,
                        command,
                        undo_data,
                    });
                }

                // Emit event
                world.send_event(CommandExecutedEvent {
                    command_id: id,
                    command_name: name,
                    success: true,
                });
            }
            Err(err) => {
                tracing::error!("Command '{}' failed: {}", name, err);

                // Emit failure event
                world.send_event(CommandExecutedEvent {
                    command_id: id,
                    command_name: name,
                    success: false,
                });
            }
        }
    }
}

/// System to perform undo
pub fn undo_system(world: &mut World) {
    let entry = {
        let mut stack = world.resource_mut::<UndoRedoStack>();
        stack.pop_for_undo()
    };

    if let Some(mut entry) = entry {
        let name = entry.command.name();

        match entry.command.undo(world, entry.undo_data.clone()) {
            Ok(()) => {
                tracing::info!("Undid '{}'", name);

                // Push to redo stack
                let mut stack = world.resource_mut::<UndoRedoStack>();
                stack.push_to_redo(entry);
            }
            Err(err) => {
                tracing::error!("Undo failed for '{}': {}", name, err);
            }
        }
    }
}

/// System to perform redo
pub fn redo_system(world: &mut World) {
    let entry = {
        let mut stack = world.resource_mut::<UndoRedoStack>();
        stack.pop_for_redo()
    };

    if let Some(mut entry) = entry {
        let name = entry.command.name();

        match entry.command.execute(world) {
            Ok(undo_data) => {
                tracing::info!("Redid '{}'", name);

                // Update undo data and push back to undo stack
                entry.undo_data = undo_data;
                let mut stack = world.resource_mut::<UndoRedoStack>();
                stack.push(entry);
            }
            Err(err) => {
                tracing::error!("Redo failed for '{}': {}", name, err);
            }
        }
    }
}
