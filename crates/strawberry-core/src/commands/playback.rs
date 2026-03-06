//! # Playback Commands
//!
//! Commands for controlling playback.

use crate::commands::{Command, CommandError, UndoData};
use crate::impl_serializable_command;
use crate::resources::{LoopMode, Playhead};
use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;

// ============================================================================
// PLAY COMMAND
// ============================================================================

/// Start playback
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayCommand;

impl Command for PlayCommand {
    fn name(&self) -> &'static str {
        "Play"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();
        let was_playing = playhead.is_playing;
        playhead.is_playing = true;

        Ok(UndoData::PropertyChanged {
            entity_bits: 0, // Resource, not entity
            property: "is_playing".to_string(),
            old_value: serde_json::json!(was_playing),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();
            playhead.is_playing = old_value.as_bool().unwrap_or(false);
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Playback commands are not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// PAUSE COMMAND
// ============================================================================

/// Pause playback
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PauseCommand;

impl Command for PauseCommand {
    fn name(&self) -> &'static str {
        "Pause"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();
        let was_playing = playhead.is_playing;
        playhead.is_playing = false;

        Ok(UndoData::PropertyChanged {
            entity_bits: 0,
            property: "is_playing".to_string(),
            old_value: serde_json::json!(was_playing),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();
            playhead.is_playing = old_value.as_bool().unwrap_or(false);
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Playback commands are not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// SEEK COMMAND
// ============================================================================

/// Seek to a specific frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeekCommand {
    /// Target frame number
    pub frame: u64,
    /// Frame rate (needed for time calculation)
    pub frame_rate: f64,
}

impl SeekCommand {
    pub fn new(frame: u64, frame_rate: f64) -> Self {
        Self { frame, frame_rate }
    }

    pub fn to_time(time: Duration, frame_rate: f64) -> Self {
        let frame = (time.as_secs_f64() * frame_rate) as u64;
        Self { frame, frame_rate }
    }
}

impl Command for SeekCommand {
    fn name(&self) -> &'static str {
        "Seek"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();
        let old_frame = playhead.current_frame;
        let old_time = playhead.current_time;

        playhead.seek_to_frame(self.frame, self.frame_rate);

        Ok(UndoData::PropertyChanged {
            entity_bits: 0,
            property: "position".to_string(),
            old_value: serde_json::json!({
                "frame": old_frame,
                "time_secs": old_time.as_secs_f64(),
            }),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();
            if let Some(frame) = old_value.get("frame").and_then(|v| v.as_u64()) {
                playhead.seek_to_frame(frame, self.frame_rate);
            }
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn can_merge(&self, other: &dyn Command) -> bool {
        // Merge consecutive seeks (e.g., during scrubbing)
        other.as_any().downcast_ref::<SeekCommand>().is_some()
    }

    fn merge(&mut self, other: Box<dyn Command>) {
        if let Some(seek) = other.as_any().downcast_ref::<SeekCommand>() {
            self.frame = seek.frame;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Seek commands are not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// SET SPEED COMMAND
// ============================================================================

/// Set playback speed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSpeedCommand {
    pub speed: f32,
}

impl SetSpeedCommand {
    pub fn new(speed: f32) -> Self {
        Self {
            speed: speed.clamp(0.1, 10.0),
        }
    }
}

impl Command for SetSpeedCommand {
    fn name(&self) -> &'static str {
        "Set Speed"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();
        let old_speed = playhead.playback_speed;
        playhead.playback_speed = self.speed;

        Ok(UndoData::PropertyChanged {
            entity_bits: 0,
            property: "playback_speed".to_string(),
            old_value: serde_json::json!(old_speed),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();
            playhead.playback_speed = old_value.as_f64().unwrap_or(1.0) as f32;
            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Playback commands are not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// SET LOOP MODE COMMAND
// ============================================================================

/// Set loop mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLoopModeCommand {
    pub mode: LoopMode,
    #[serde(with = "crate::resources::duration_option_serde")]
    pub in_point: Option<Duration>,
    #[serde(with = "crate::resources::duration_option_serde")]
    pub out_point: Option<Duration>,
}

impl SetLoopModeCommand {
    pub fn none() -> Self {
        Self {
            mode: LoopMode::None,
            in_point: None,
            out_point: None,
        }
    }

    pub fn timeline() -> Self {
        Self {
            mode: LoopMode::Timeline,
            in_point: None,
            out_point: None,
        }
    }

    pub fn in_out(in_point: Duration, out_point: Duration) -> Self {
        Self {
            mode: LoopMode::InOut,
            in_point: Some(in_point),
            out_point: Some(out_point),
        }
    }
}

impl Command for SetLoopModeCommand {
    fn name(&self) -> &'static str {
        "Set Loop Mode"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();

        let old_mode = playhead.loop_mode;
        let old_in = playhead.loop_in;
        let old_out = playhead.loop_out;

        playhead.loop_mode = self.mode;
        playhead.loop_in = self.in_point;
        playhead.loop_out = self.out_point;

        Ok(UndoData::PropertyChanged {
            entity_bits: 0,
            property: "loop".to_string(),
            old_value: serde_json::json!({
                "mode": old_mode,
                "in": old_in.map(|d| d.as_secs_f64()),
                "out": old_out.map(|d| d.as_secs_f64()),
            }),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();

            if let Some(mode) = old_value.get("mode") {
                playhead.loop_mode = serde_json::from_value(mode.clone()).unwrap_or_default();
            }
            if let Some(in_secs) = old_value.get("in").and_then(|v| v.as_f64()) {
                playhead.loop_in = Some(Duration::from_secs_f64(in_secs));
            } else {
                playhead.loop_in = None;
            }
            if let Some(out_secs) = old_value.get("out").and_then(|v| v.as_f64()) {
                playhead.loop_out = Some(Duration::from_secs_f64(out_secs));
            } else {
                playhead.loop_out = None;
            }

            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Playback commands are not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// STOP COMMAND
// ============================================================================

/// Stop playback and reset to beginning
///
/// This combines pause + seek to start, commonly used in video editors.
/// Unlike just pausing, stop resets the playhead to frame 0.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StopCommand {
    /// Frame rate for seeking (default 30.0 if not provided)
    frame_rate: f64,
}

impl StopCommand {
    pub fn new() -> Self {
        Self { frame_rate: 30.0 }
    }

    pub fn with_frame_rate(frame_rate: f64) -> Self {
        Self { frame_rate }
    }
}

impl Command for StopCommand {
    fn name(&self) -> &'static str {
        "Stop"
    }

    fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
        let mut playhead = world.resource_mut::<Playhead>();

        // Store old state for undo
        let was_playing = playhead.is_playing;
        let old_frame = playhead.current_frame;
        let old_time = playhead.current_time;

        // Stop and reset to beginning
        playhead.is_playing = false;
        playhead.current_frame = 0;
        playhead.current_time = Duration::ZERO;

        Ok(UndoData::PropertyChanged {
            entity_bits: 0,
            property: "stop".to_string(),
            old_value: serde_json::json!({
                "is_playing": was_playing,
                "frame": old_frame,
                "time_secs": old_time.as_secs_f64(),
            }),
        })
    }

    fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> {
        if let UndoData::PropertyChanged { old_value, .. } = undo_data {
            let mut playhead = world.resource_mut::<Playhead>();

            if let Some(was_playing) = old_value.get("is_playing").and_then(|v| v.as_bool()) {
                playhead.is_playing = was_playing;
            }
            if let Some(frame) = old_value.get("frame").and_then(|v| v.as_u64()) {
                playhead.seek_to_frame(frame, self.frame_rate);
            }

            Ok(())
        } else {
            Err(CommandError::InvalidOperation(
                "Invalid undo data".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Stop command is not undoable (navigation, not editing)
    fn is_undoable(&self) -> bool {
        false
    }
}

// ============================================================================
// AI/JSON SERIALIZABLE IMPLEMENTATIONS
// ============================================================================

impl_serializable_command!(PlayCommand, "play");
impl_serializable_command!(PauseCommand, "pause");
impl_serializable_command!(StopCommand, "stop");
impl_serializable_command!(SeekCommand, "seek");
impl_serializable_command!(SetSpeedCommand, "set_speed");
impl_serializable_command!(SetLoopModeCommand, "set_loop_mode");

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_world() -> World {
        let mut world = World::new();
        world.insert_resource(Playhead::default());
        world
    }

    #[test]
    fn test_play_pause() {
        let mut world = setup_world();

        let mut play = PlayCommand;
        play.execute(&mut world).unwrap();
        assert!(world.resource::<Playhead>().is_playing);

        let mut pause = PauseCommand;
        pause.execute(&mut world).unwrap();
        assert!(!world.resource::<Playhead>().is_playing);
    }

    #[test]
    fn test_seek() {
        let mut world = setup_world();

        let mut seek = SeekCommand::new(100, 30.0);
        seek.execute(&mut world).unwrap();

        let playhead = world.resource::<Playhead>();
        assert_eq!(playhead.current_frame, 100);
    }

    #[test]
    fn test_set_speed() {
        let mut world = setup_world();

        let mut cmd = SetSpeedCommand::new(2.0);
        cmd.execute(&mut world).unwrap();

        assert_eq!(world.resource::<Playhead>().playback_speed, 2.0);
    }
}
