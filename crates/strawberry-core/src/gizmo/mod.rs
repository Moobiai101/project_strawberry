//! # Viewport Gizmo Engine
//!
//! Pure math and data structures for the viewport transform gizmo system.
//! This module provides the "brain" behind viewport interactions — coordinate
//! transforms, handle hit-testing, and smart-guide snapping.
//!
//! ## Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────┐
//! │  strawberry-ui (viewport)                         │
//! │  ├── gizmo_overlay.rs  (rendering)                │
//! │  └── gizmo_input.rs    (mouse/keyboard routing)   │
//! │          │                                        │
//! │          ▼ calls into                             │
//! │  strawberry-core (this module)                    │
//! │  ├── coordinate_transform.rs  (screen ↔ scene)    │
//! │  ├── hit_testing.rs           (handle detection)  │
//! │  ├── snapping.rs              (smart guides)      │
//! │  └── gizmo_state.rs           (interaction state) │
//! └───────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Concepts
//!
//! - **Screen space**: Pixel coordinates in the egui viewport widget
//! - **Scene space**: Coordinates in the project canvas (e.g., 1920×1080)
//! - **GizmoHandle**: Which part of the bounding box the user clicked
//! - **SnapEngine**: Finds alignment snaps to canvas edges, centers, and other elements
//!
//! ## Design Notes
//!
//! Inspired by Olive Editor's `viewerdisplay.cpp` coordinate pipeline
//! (`GenerateGizmoTransform → ScreenToScenePoint → TryGizmoPress → DragStart/Move/End`),
//! but extended with CapCut/Canva features (smart guides, distance indicators,
//! rotation snapping, aspect-ratio lock) that Olive doesn't have.
//!
//! All code here is pure math — no egui, no winit, no UI dependencies.
//! This allows comprehensive unit testing without a running application.

pub mod coordinate_transform;
pub mod gizmo_state;
pub mod hit_testing;
pub mod snapping;
