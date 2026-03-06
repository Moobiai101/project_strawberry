//! Timeline interaction handling.
//!
//! Contains modules for user input processing, hit testing, and movement validation.
//!
//! ## Modules
//!
//! - `tools`: TimelineTool enum for editing modes
//! - `hit_testing`: Hit test functions for clips/edges/gaps
//! - `validation`: 5-stage movement validation chain (frame snap, trim clamp, track clamp)

pub mod hit_testing;
pub mod tools;
pub mod validation;

// Re-exports
pub use hit_testing::{
    hit_test_clip, hit_test_clip_edge, hit_test_gap, ClipHit, GapHit, TrimHit, TRIM_HANDLE_WIDTH,
};
pub use tools::TimelineTool;
