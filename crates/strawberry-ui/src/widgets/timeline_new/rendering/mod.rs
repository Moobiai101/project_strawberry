//! Timeline rendering functions.
//!
//! Contains all drawing logic for the timeline widget.
//!
//! ## Modules
//!
//! - `colors`: Color palette constants
//! - `ruler`: Time ruler drawing
//! - `tracks`: Track lane backgrounds
//! - `playhead`: Playhead indicator

pub mod colors;
pub mod playhead;
pub mod ruler;
pub mod tracks;

// Re-exports for convenience
pub use colors::*;
pub use playhead::{
    draw_playhead, time_to_x, x_to_time, PLAYHEAD_GRAB_ZONE, PLAYHEAD_HEAD_SIZE, PLAYHEAD_WIDTH,
};
pub use ruler::{calculate_tick_interval, draw_ruler, format_timecode};
pub use tracks::{calculate_total_track_height, draw_track_lanes, track_height, track_y_position};
