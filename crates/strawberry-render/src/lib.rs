//! # Strawberry Render
//!
//! GPU render pipeline using wgpu and Vello.
//!
//! ## Modules
//!
//! - `graph`: GPU render graph (RenderNode, RenderGraph)
//! - `traverser`: Timeline node traverser (NodeTraverser, CompositorOutput)
//! - `compositor`: Timeline compositor system
//! - `systems`: ECS systems for viewport/audio sync
//! - `texture_manager`: GPU texture upload and caching
//! - `thumbnail_texture_manager`: Thumbnail texture GPU upload

pub mod compositor;
pub mod compositor_cache;
pub mod graph;
pub mod pipeline;
pub mod systems;
pub mod texture_manager;
pub mod thumbnail_texture_manager;
pub mod traverser;

pub use compositor::{compositor_system, VideoAssetManager};
pub use compositor_cache::{calculate_layer_hash, CompositorCache};
pub use graph::{
    create_default_render_graph, CompositeNode, PresentNode, RenderContext, RenderError,
    RenderGraph, RenderNode, RenderResources, RenderTexture, TransformNode, VideoDecodeNode,
};
pub use pipeline::RenderPipeline;
pub use systems::{
    audio_mixer_sync_system, viewport_update_system, AudioMixerResource, VideoServiceResource,
};
pub use texture_manager::TextureManager;
pub use thumbnail_texture_manager::{ThumbnailKey, ThumbnailTextureManager};
pub use traverser::{
    ActiveLayersResource, CompositorLayer, CompositorOutput, NodeTraverser, TraverserResource,
};
