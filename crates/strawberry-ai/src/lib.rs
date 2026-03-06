//! # Strawberry AI
//!
//! AI sidecar for natural language commands and intelligent assistance.
//!
//! ## Architecture (from SYSTEM_DESIGN.md)
//!
//! The AI operates through the same Command layer as the UI:
//! - AI reads via `AIQueryService` (read-only ECS access)
//! - AI writes via `AICommandInterface` (submits commands to CommandQueue)
//! - AI runs in a sidecar (separate async task, doesn't block main loop)
//!
//! ## Phase 0 Capabilities
//!
//! - Query project state
//! - Control playback (play, pause, seek, set speed)
//! - Create projects

pub mod agent;
pub mod command_interface;
pub mod context;
pub mod query;

pub use agent::*;
pub use command_interface::*;
pub use query::*;
