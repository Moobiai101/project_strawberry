//! # Render Graph
//!
//! A directed acyclic graph (DAG) for organizing render operations.
//!
//! ## Architecture (Based on Bevy + Adobe Premiere Research)
//!
//! The render graph consists of:
//! - **Nodes**: Individual render operations (decode, transform, composite, present)
//! - **Edges**: Dependencies between nodes (implicit via execution order)
//! - **Resources**: Textures and buffers passed between nodes
//!
//! ## Phase 0 Pipeline
//! ```text
//! [VideoDecodeNode] → [TransformNode] → [CompositeNode] → [PresentNode]
//! ```
//!
//! ## Usage
//! ```ignore
//! let mut graph = RenderGraph::new();
//! graph.add_node(Box::new(VideoDecodeNode::new()));
//! graph.add_node(Box::new(TransformNode::new()));
//! graph.add_node(Box::new(PresentNode::new()));
//! graph.execute(&mut context)?;
//! ```

use bevy_ecs::prelude::*;
use std::fmt::Debug;
use thiserror::Error;
use wgpu;

// ============================================================================
// ERRORS
// ============================================================================

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("Node execution failed: {0}")]
    NodeFailed(String),

    #[error("Missing resource: {0}")]
    MissingResource(String),

    #[error("GPU error: {0}")]
    GpuError(String),
}

// ============================================================================
// RENDER CONTEXT
// ============================================================================

/// Context passed to each render node during execution
///
/// Contains all resources needed for rendering (device, queue, textures, etc.)
pub struct RenderContext<'a> {
    /// wgpu device reference
    pub device: &'a wgpu::Device,
    /// wgpu queue reference
    pub queue: &'a wgpu::Queue,
    /// Current frame's output texture
    pub output_texture: Option<&'a wgpu::TextureView>,
    /// Current viewport size
    pub viewport_size: (u32, u32),
    /// Shared render resources (textures, buffers)
    pub resources: RenderResources,
}

/// Shared resources between render nodes
#[derive(Default)]
pub struct RenderResources {
    /// Named texture slots for passing between nodes
    pub textures: std::collections::HashMap<String, RenderTexture>,
    /// Transform matrix for current frame
    pub transform_matrix: Option<glam::Mat3>,
    /// Opacity for compositing
    pub opacity: f32,
}

impl RenderResources {
    pub fn new() -> Self {
        Self {
            textures: std::collections::HashMap::new(),
            transform_matrix: None,
            opacity: 1.0,
        }
    }

    /// Store a texture for later nodes to use
    pub fn set_texture(&mut self, name: impl Into<String>, texture: RenderTexture) {
        self.textures.insert(name.into(), texture);
    }

    /// Get a texture by name
    pub fn get_texture(&self, name: &str) -> Option<&RenderTexture> {
        self.textures.get(name)
    }
}

/// A texture in the render pipeline
pub struct RenderTexture {
    pub texture_id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

// ============================================================================
// RENDER NODE TRAIT
// ============================================================================

/// Trait for render graph nodes
///
/// Each node represents a single render operation that can:
/// - Read from previous nodes via RenderResources
/// - Write to RenderResources for following nodes
/// - Execute GPU commands
pub trait RenderNode: Send + Sync + Debug {
    /// Human-readable node name
    fn name(&self) -> &'static str;

    /// Execute this node
    ///
    /// Nodes should:
    /// 1. Read any required inputs from context.resources
    /// 2. Perform their rendering work
    /// 3. Write outputs to context.resources
    fn execute(&mut self, world: &World, context: &mut RenderContext) -> Result<(), RenderError>;

    /// Optional: Check if node should run this frame
    fn should_run(&self, _world: &World) -> bool {
        true
    }
}

// ============================================================================
// RENDER GRAPH (ECS Resource)
// ============================================================================

/// The main render graph - an ECS Resource
///
/// Manages a list of render nodes that execute in order.
/// For Phase 0, we use simple ordered execution.
/// Phase 2+ will add true DAG with parallel execution.
#[derive(Resource)]
pub struct RenderGraph {
    /// Ordered list of nodes to execute
    nodes: Vec<Box<dyn RenderNode>>,
    /// Whether the graph has been built
    built: bool,
}

impl Default for RenderGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderGraph {
    /// Create a new empty render graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            built: false,
        }
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, node: Box<dyn RenderNode>) {
        tracing::debug!("Added render node: {}", node.name());
        self.nodes.push(node);
        self.built = false;
    }

    /// Execute all nodes in order
    pub fn execute(
        &mut self,
        world: &World,
        context: &mut RenderContext,
    ) -> Result<(), RenderError> {
        for node in &mut self.nodes {
            if node.should_run(world) {
                tracing::trace!("Executing render node: {}", node.name());
                node.execute(world, context)?;
            }
        }
        Ok(())
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Clear all nodes
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.built = false;
    }
}

impl Debug for RenderGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderGraph")
            .field("node_count", &self.nodes.len())
            .field("built", &self.built)
            .finish()
    }
}

// ============================================================================
// BASIC RENDER NODES (Phase 0)
// ============================================================================

/// Video decode node - currently a passthrough (decode happens elsewhere)
///
/// In Phase 0, video decode is handled by VideoService.
/// This node's job is to get the current frame texture and put it in resources.
#[derive(Debug)]
pub struct VideoDecodeNode;

impl VideoDecodeNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VideoDecodeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderNode for VideoDecodeNode {
    fn name(&self) -> &'static str {
        "Video Decode"
    }

    fn execute(&mut self, _world: &World, _context: &mut RenderContext) -> Result<(), RenderError> {
        // Phase 0: Video frames are uploaded via TextureManager
        // This node is a passthrough - the texture is already available
        Ok(())
    }
}

/// Transform node - applies 2D transform to the current frame
#[derive(Debug)]
pub struct TransformNode {
    /// Cached transform matrix
    transform_matrix: Option<glam::Mat3>,
}

impl TransformNode {
    pub fn new() -> Self {
        Self {
            transform_matrix: None,
        }
    }

    /// Set the current transform matrix
    pub fn set_transform(&mut self, matrix: glam::Mat3) {
        self.transform_matrix = Some(matrix);
    }
}

impl Default for TransformNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderNode for TransformNode {
    fn name(&self) -> &'static str {
        "Transform"
    }

    fn execute(&mut self, _world: &World, context: &mut RenderContext) -> Result<(), RenderError> {
        // Store transform in resources for composite node
        if let Some(matrix) = self.transform_matrix {
            context.resources.transform_matrix = Some(matrix);
        }
        Ok(())
    }
}

/// Composite node - blends layers together
///
/// Phase 0: Single layer passthrough
/// Phase 2+: Multi-layer blending with blend modes
#[derive(Debug)]
pub struct CompositeNode {
    /// Background color (RGBA)
    pub background_color: [f32; 4],
}

impl CompositeNode {
    pub fn new() -> Self {
        Self {
            background_color: [0.0, 0.0, 0.0, 1.0], // Black
        }
    }
}

impl Default for CompositeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderNode for CompositeNode {
    fn name(&self) -> &'static str {
        "Composite"
    }

    fn execute(&mut self, _world: &World, _context: &mut RenderContext) -> Result<(), RenderError> {
        // Phase 0: Single layer, no compositing needed
        // The viewport displays the video frame directly via egui
        Ok(())
    }
}

/// Present node - final output to screen
#[derive(Debug)]
pub struct PresentNode;

impl PresentNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PresentNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderNode for PresentNode {
    fn name(&self) -> &'static str {
        "Present"
    }

    fn execute(&mut self, _world: &World, _context: &mut RenderContext) -> Result<(), RenderError> {
        // Phase 0: egui handles presentation
        // This node is a marker for the end of the pipeline
        Ok(())
    }
}

// ============================================================================
// FACTORY FUNCTION
// ============================================================================

/// Create the default Phase 0 render graph
pub fn create_default_render_graph() -> RenderGraph {
    let mut graph = RenderGraph::new();

    graph.add_node(Box::new(VideoDecodeNode::new()));
    graph.add_node(Box::new(TransformNode::new()));
    graph.add_node(Box::new(CompositeNode::new()));
    graph.add_node(Box::new(PresentNode::new()));

    tracing::info!(
        "Created default render graph with {} nodes",
        graph.node_count()
    );
    graph
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_graph_creation() {
        let graph = RenderGraph::new();
        assert_eq!(graph.node_count(), 0);
    }

    #[test]
    fn test_add_nodes() {
        let mut graph = RenderGraph::new();
        graph.add_node(Box::new(VideoDecodeNode::new()));
        graph.add_node(Box::new(TransformNode::new()));
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn test_default_graph() {
        let graph = create_default_render_graph();
        assert_eq!(graph.node_count(), 4);
    }

    #[test]
    fn test_transform_node_matrix() {
        let mut node = TransformNode::new();
        let matrix = glam::Mat3::from_scale(glam::Vec2::new(2.0, 2.0));
        node.set_transform(matrix);
        assert!(node.transform_matrix.is_some());
    }
}
