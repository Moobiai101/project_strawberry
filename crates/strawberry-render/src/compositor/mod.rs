//! # Compositor Module
//!
//! Production-grade video compositing system for timeline playback.
//!
//! ## Architecture Overview
//!
//! The compositor bridges ECS timeline data with the viewport display:
//!
//! ```text
//! ┌─────────────────┐    ┌──────────────────────┐    ┌─────────────┐
//! │  ECS Timeline   │───▶│  compositor_system   │───▶│  Viewport   │
//! │  (Clips/Tracks) │    │  (Frame Collection)  │    │  (Display)  │
//! └─────────────────┘    └──────────────────────┘    └─────────────┘
//!                               │
//!                               ▼
//!                   ┌──────────────────────┐
//!                   │  VideoAssetManager   │
//!                   │  (Decode + Cache)    │
//!                   └──────────────────────┘
//! ```
//!
//! ## Module Structure
//!
//! - **video_asset_manager**: Manages video asset decode paths (streaming/random-access)
//! - **frame_compositor**: ECS system that collects layers and outputs frames
//!
//! ## Data Flow
//!
//! 1. `compositor_system` queries ECS for clips at current playhead time
//! 2. NodeTraverser builds layer stack (bottom-to-top track order)
//! 3. `VideoAssetManager` decodes frames via optimal path:
//!    - **Playback mode**: PipeDecoder streaming (~1ms/frame)
//!    - **Scrubbing mode**: JPEG cache + proxy loader (~2-5ms/frame)
//! 4. Frames uploaded to GPU via TextureManager
//! 5. ViewportState updated for egui display
//!
//! ## Performance Features
//!
//! - **CompositorCache**: Skips redundant work when playhead is idle
//! - **Frame LRU cache**: Instant re-access during scrubbing
//! - **Proxy generation**: Edit-friendly 540p proxies for responsive scrubbing
//!
//! ## Future (Phase 2B)
//!
//! Multi-layer alpha-over compositing for overlapping clips.

mod alpha_compositor;
mod frame_compositor;
mod video_asset_manager;

pub use alpha_compositor::AlphaCompositor;
pub use frame_compositor::compositor_system;
pub use video_asset_manager::VideoAssetManager;
