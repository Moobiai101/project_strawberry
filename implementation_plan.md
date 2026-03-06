# Implementation Plan: Compositor Engine (Phase 1)

## 🚀 Implementation Progress

> **Last Updated:** 2026-02-02

### Phase 1: Node Foundation ✅ COMPLETE

| Task | Status | Files |
|------|--------|-------|
| Node trait + NodeValue | ✅ Done | `node/mod.rs`, `node/value.rs` |
| InputDefinition | ✅ Done | `node/input.rs` |
| ClipNode + ClipAssetRef | ✅ Done | `node/timeline/clip.rs` |
| TrackNode | ✅ Done | `node/timeline/track.rs` |
| NodeTraverser | ✅ Done | `traverser.rs` |
| compositor_system | ✅ Done | `compositor.rs` |
| VideoAssetManager | ✅ Done | `compositor.rs` |
| AddClipCommand with asset_uuid | ✅ Done | `commands/timeline.rs` |
| App Integration | ✅ Done | `strawberry-app/src/main.rs` |

### Phase 1: Viewport Rendering ✅ COMPLETE

| Task | Status | Notes |
|------|--------|-------|
| Viewport shows frames | ✅ Done | compositor_system now updates ViewportState |

### Phase 1.5: Timeline Polish ✅ COMPLETE (2026-02-02)

| Task | Status | Files |
|------|--------|-------|
| Filmstrip thumbnail pre-generation | ✅ Done | `strawberry-video/src/thumbnail.rs` |
| Filmstrip persistent disk cache | ✅ Done | `thumbnails/` directory per asset |
| Multi-LOD filmstrip loading | ✅ Done | 5 LOD levels for smooth zoom |
| FilmstripTextureCache (egui) | ✅ Done | `filmstrip.rs` |
| CapCut-style edge-to-edge rendering | ✅ Done | Fixed aspect ratio + seamless tiling |
| ClipAssetRef in DeleteClipCommand | ✅ Done | Proper undo restores video+filmstrip |
| ClipAssetRef in RazorToolCommand | ✅ Done | Split clips retain filmstrip |
| UUID-based entity lookup in undo | ✅ Done | `SplitClipCommand`, `RazorToolCommand` |
| StableIdRegistry for persist across undo/redo | ✅ Done | All commands use UUID lookup |

> [!TIP]
> **Key Architecture Win**: Commands now use `StableIdRegistry` for entity lookup instead of raw entity bits. This matches Olive's approach and ensures undo/redo works correctly even with 1000+ clips.

---

### Phase 2: Production-Grade Performance 🚧 NEXT

**Goal**: Scale to 1000+ clips, overlays, text, VFX without lag

| Task | Priority | Estimate | Notes |
|------|----------|----------|-------|
| **Timeline Virtualization** | 🔴 Critical | 2 days | Only render visible clips (viewport culling) |
| **Clip Pooling/Recycling** | 🔴 Critical | 1 day | Reuse clip entities during scroll |
| **GPU-Batched Compositing** | 🟠 High | 3 days | Render multiple clips in single draw call |
| **Background Filmstrip Worker** | 🟠 High | 1 day | Async thumbnail generation off main thread |
| **Shader-Based Blending** | 🟠 High | 2 days | Alpha-over compositing on GPU |
| **Frame Request Coalescing** | 🟡 Medium | 1 day | Dedupe identical frame requests |
| **Texture Atlas for Thumbnails** | 🟡 Medium | 2 days | Single large texture, UV-addressed |
| **Level-of-Detail for Clips** | 🟡 Medium | 1 day | Show simpler UI at low zoom |

### Phase 3: Effects & Text 🔮 PLANNED

| Task | Priority | Notes |
|------|----------|-------|
| TextNode (cosmic-text) | 🔴 Critical | GPU-accelerated text rendering |
| BlurNode | 🟠 High | Gaussian blur with GPU shader |
| ColorCorrectionNode | 🟠 High | HSL, curves, LUT support |
| TransitionNodes | 🟠 High | Dissolve, wipe, slide |
| KeyframeSystem | 🔴 Critical | Per-parameter animation curves |

### Phase 4: VFX & Motion Graphics 🔮 PLANNED

| Task | Notes |
|------|-------|
| Gizmo System | Draggable handles in viewport |
| TransformNode + gizmos | Position, rotation, scale, anchor |
| CornerPinNode | 4-point perspective distortion |
| MaskNode | Bezier masks with feathering |
| Motion Tracking | Track points, planar tracking |

### Phase 5: 3D & AI 🔮 FUTURE

| Task | Notes |
|------|-------|
| 3D Layers | Z-depth, camera |
| AI Upscaling | ESRGAN/Real-ESRGAN integration |
| AI Object Removal | LaMa/ProPainter integration |
| AI Auto-Captioning | Whisper integration |

---

## 12. README Reminder Checklist


For each phase, create these README files:

### Phase 1
- [x] `crates/strawberry-core/src/node/README.md`
- [ ] `crates/strawberry-core/src/node/timeline/README.md`
- [ ] `crates/strawberry-render/src/README.md` (update)
- [ ] `crates/strawberry-video/README.md` (update)

### Phase 2
- [ ] `crates/strawberry-render/src/shaders/README.md`
- [ ] `crates/strawberry-core/src/node/blend/README.md`

### Phase 3
- [ ] `crates/strawberry-core/src/node/generator/README.md`
- [ ] `crates/strawberry-core/src/node/filter/README.md`
- [ ] `crates/strawberry-core/src/node/color/README.md`

### Phase 4
- [ ] `crates/strawberry-ui/src/gizmo/README.md`
- [ ] `crates/strawberry-core/src/node/distort/README.md`

### Phase 5
- [ ] `crates/strawberry-core/src/node/ai/README.md`
- [ ] `crates/strawberry-core/src/node/tracking/README.md`
- [ ] `crates/strawberry-core/src/node/threed/README.md`

---

## Appendix: Key Code References

### Olive Time Remapping (`research/rendering/node/block/clip/clip.cpp:155-198`)
The `SequenceToMediaTime` function is the gold standard for time conversion.

### Olive Job Resolution (`research/rendering/node/traverser.cpp:351-471`)
`ResolveJobs` dispatches FootageJob/ShaderJob to GPU. Our `NodeTraverser` follows this pattern.

### Alpha-Over Shader (`research/rendering/shaders/alphaover.frag`)
Standard GLSL alpha compositing: `base * (1.0 - blend.a) + blend`

## Executive Summary

This plan details the implementation of a production-grade **Compositor Engine** for Project Strawberry, based on reverse-engineering Olive Editor (node-based) and Kdenlive (MLT-based) architectures. The goal is to fix the viewport desync issue by replacing the current "single video player" model with a proper NLE timeline compositor.

---

## 1. Problem Analysis

### Current Architecture (Broken)
```
┌─────────────────────────────────────────────────────────────┐
│                       CURRENT FLOW                          │
├─────────────────────────────────────────────────────────────┤
│  Playhead.current_time ──►  VideoService.current_video      │
│                              │                              │
│                              ▼                              │
│                         request_frame(timestamp)            │
│                              │                              │
│                              ▼                              │
│                       ViewportState.texture                 │
└─────────────────────────────────────────────────────────────┘
```
**Issue**: `VideoService` holds ONE video. Timeline clips are IGNORED.

### Target Architecture (Industry Standard)
```
┌─────────────────────────────────────────────────────────────┐
│                       TARGET FLOW                           │
├─────────────────────────────────────────────────────────────┤
│  Playhead.current_time                                      │
│        │                                                    │
│        ▼                                                    │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              COMPOSITOR SYSTEM                       │   │
│  │  1. Query all Tracks (sorted by z-index)            │   │
│  │  2. For each Track:                                 │   │
│  │     a. Find Clips overlapping playhead              │   │
│  │     b. Calculate source_time:                       │   │
│  │        source_time = (playhead - clip.start)        │   │
│  │                      * speed + clip.source_in       │   │
│  │     c. Request frame from VideoAssetManager         │   │
│  │  3. Blend frames (alpha-over, bottom to top)        │   │
│  │  4. Return final composited texture                 │   │
│  └─────────────────────────────────────────────────────┘   │
│        │                                                    │
│        ▼                                                    │
│  ViewportState.texture                                      │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Research Findings

### 2.1 Olive Editor Architecture

| Component | Purpose | Our Equivalent |
|-----------|---------|----------------|
| `NodeTraverser` | Walks node graph, generates value tables | `CompositorGraph` |
| `RenderProcessor` | Executes GPU jobs (ShaderJob, FootageJob) | `RenderExecutor` |
| `ClipBlock.SequenceToMediaTime()` | Maps timeline→source time with speed/reverse | `Clip.timeline_to_source()` |
| `Renderer.Blit()` | GPU texture blitting with shaders | `wgpu` render pass |
| `alphaover.frag` | Standard alpha compositing shader | WGSL equivalent |

**Key Insight**: Olive's `GenerateTable()` recursively walks the node graph and collects "jobs" (FootageJob, ShaderJob). These jobs are resolved by `RenderProcessor.ResolveJobs()` which does the actual GPU work.

### 2.2 Kdenlive Architecture

| Component | Purpose | Our Equivalent |
|-----------|---------|----------------|
| `Mlt::Producer` | Represents a media source | `VideoAsset` component |
| `Mlt::Consumer` | Renders frames to display | `viewport_update_system` |
| `VideoWidget` | Qt widget that displays frames from consumer | `egui::Image` |
| MLT's `cut()` | Creates a sub-clip from a producer | `Clip.source_in/source_out` |

**Key Insight**: Kdenlive delegates ALL rendering to the MLT framework. We cannot use MLT directly (Rust), but our architecture should follow the same Producer→Consumer model.

### 2.3 Critical Time Remapping Logic (from Olive `clip.cpp`)

```rust
/// Convert timeline time to source media time
/// Handles: speed, reverse, media_in offset, loop mode
fn sequence_to_media_time(
    &self,
    timeline_time: Duration,
    flags: TimeFlags,
) -> Duration {
    let mut media_time = timeline_time;

    // Handle reverse playback
    if self.reverse && !flags.contains(IGNORE_REVERSE) {
        media_time = self.duration() - media_time;
    }

    // Handle speed
    if !flags.contains(IGNORE_SPEED) {
        if self.speed == 0.0 {
            media_time = Duration::ZERO; // Freeze frame
        } else if self.speed != 1.0 {
            media_time = Duration::from_secs_f64(
                media_time.as_secs_f64() * self.speed
            );
        }
    }

    // Add source in-point offset
    media_time + self.source_in
}
```

---

## 3. Proposed Changes

### Phase 1A: Multi-Asset Video Service

#### [MODIFY] `crates/strawberry-video/src/service.rs`

Transform `VideoService` into `VideoAssetManager`:

```rust
/// Manages multiple loaded video assets
/// Each asset is identified by its StableId (UUID)
pub struct VideoAssetManager {
    /// Decoder for random-access frames
    decoder: VideoDecoder,
    /// Frame cache (shared across all assets)
    cache: Arc<FrameCache>,
    /// Loaded video metadata, keyed by asset UUID
    loaded_assets: HashMap<Uuid, LoadedVideo>,
    /// Active playback decoders (for playing clips)
    playback_decoders: HashMap<Uuid, PipeDecoder>,
    /// Async decode worker
    worker: DecodeWorker,
    result_rx: mpsc::Receiver<DecodeResult>,
}

impl VideoAssetManager {
    /// Load an asset and return its metadata
    pub fn load_asset(&mut self, uuid: Uuid, path: &Path) 
        -> Result<VideoMetadata, DecodeError>;
    
    /// Request a frame for a specific asset at a source timestamp
    pub fn request_frame(
        &self,
        asset_uuid: Uuid,
        source_time: Duration,
    ) -> FrameStatus;
    
    /// Start playback mode for an asset
    pub fn start_playback(
        &mut self,
        asset_uuid: Uuid,
        start_time: Duration,
    ) -> Result<(), DecodeError>;
    
    /// Stop playback for an asset
    pub fn stop_playback(&mut self, asset_uuid: Uuid);
}
```

---

### Phase 1B: Compositor System

#### [NEW] `crates/strawberry-render/src/compositor.rs`

```rust
//! # Compositor System
//!
//! ECS system that composites timeline clips into a single output texture.
//! Implements the industry-standard "bottom-to-top alpha-over" pattern.

use bevy_ecs::prelude::*;
use strawberry_core::{
    components::{Clip, Track, TrackType, ClipAssetRef, Transform2D, StableId},
    resources::{Playhead, ViewportState, StableIdRegistry},
};
use strawberry_video::service::VideoAssetManager;

/// Resource holding the composited frame for the current playhead position
#[derive(Resource, Default)]
pub struct CompositorOutput {
    /// The final composited texture ID
    pub texture_id: Option<egui::TextureId>,
    /// Resolution of the output
    pub resolution: glam::UVec2,
    /// Frame number that was composited
    pub frame: u64,
}

/// Main compositor system - runs every frame
///
/// ## Algorithm (matches Olive/Premiere):
/// 1. Get playhead time
/// 2. Query all visible tracks, sorted by index (ascending = bottom first)
/// 3. For each track, find overlapping clips
/// 4. For each clip, calculate source time and request frame
/// 5. Composite frames bottom-to-top with alpha-over blending
pub fn compositor_system(
    playhead: Res<Playhead>,
    tracks: Query<(Entity, &Track, &StableId)>,
    clips: Query<(&Clip, &ClipAssetRef, &Parent, Option<&Transform2D>, &StableId)>,
    registry: Res<StableIdRegistry>,
    mut video_manager: ResMut<VideoAssetManagerResource>,
    texture_manager: Res<TextureManager>,
    mut compositor_output: ResMut<CompositorOutput>,
) {
    let timeline_time = playhead.current_time;
    
    // Collect and sort tracks by index (bottom to top)
    let mut sorted_tracks: Vec<_> = tracks.iter().collect();
    sorted_tracks.sort_by_key(|(_, track, _)| track.index);
    
    // Storage for layers to composite
    let mut layers: Vec<CompositorLayer> = Vec::new();
    
    for (track_entity, track, _) in sorted_tracks {
        // Skip muted/invisible tracks
        if track.muted || !track.visible {
            continue;
        }
        
        // Find clips on this track that overlap the playhead
        for (clip, asset_ref, parent, transform, stable_id) in clips.iter() {
            if parent.get() != track_entity {
                continue;
            }
            
            // Check if clip overlaps playhead
            if !clip.contains_time(timeline_time) {
                continue;
            }
            
            // Calculate source time using Olive's algorithm
            let relative_time = timeline_time - clip.timeline_start;
            let source_time = clip.source_in + relative_time;
            // TODO: Add speed/reverse handling in Phase 2
            
            // Request frame from asset manager
            let frame_status = video_manager.0.request_frame(
                asset_ref.asset_uuid,
                source_time,
            );
            
            if let FrameStatus::Ready(frame) = frame_status {
                // Queue texture upload
                let key = FrameKey::new(
                    asset_ref.asset_uuid,
                    source_time,
                    frame.width,
                    frame.height,
                );
                texture_manager.queue_upload(key.clone(), (*frame).clone());
                
                if let Some(tex_id) = texture_manager.get_texture(&key) {
                    layers.push(CompositorLayer {
                        texture_id: tex_id,
                        transform: transform.cloned().unwrap_or_default(),
                        opacity: transform.map(|t| t.opacity).unwrap_or(1.0),
                        z_index: track.index,
                    });
                }
            }
        }
    }
    
    // For Phase 1: Just use the topmost layer
    // Phase 2 will implement GPU blending
    if let Some(top_layer) = layers.last() {
        compositor_output.texture_id = Some(top_layer.texture_id);
        compositor_output.frame = playhead.current_frame;
    } else {
        compositor_output.texture_id = None;
    }
}

/// A single layer to be composited
struct CompositorLayer {
    texture_id: egui::TextureId,
    transform: Transform2D,
    opacity: f32,
    z_index: u32,
}
```

---

### Phase 1C: Component Additions

#### [MODIFY] `crates/strawberry-core/src/components/mod.rs`

Add reference linking clips to video assets:

```rust
/// Links a Clip entity to its source video asset
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct ClipAssetRef {
    /// UUID of the source video asset
    pub asset_uuid: Uuid,
    /// Original filename (for display)
    pub filename: String,
}
```

---

### Phase 1D: Viewport Integration

#### [MODIFY] `crates/strawberry-render/src/systems.rs`

Update `viewport_update_system` to consume compositor output:

```rust
/// Updated viewport system that reads from Compositor instead of VideoService
pub fn viewport_update_system(
    compositor_output: Res<CompositorOutput>,
    mut viewport: ResMut<ViewportState>,
) {
    if let Some(texture_id) = compositor_output.texture_id {
        viewport.set_frame(compositor_output.frame, texture_id);
    }
}
```

---

## 4. Implementation Order

| Step | File | Estimate | Dependency |
|------|------|----------|------------|
| 1 | `ClipAssetRef` component | 30 min | None |
| 2 | `VideoAssetManager` refactor | 2 hours | Step 1 |
| 3 | `CompositorOutput` resource | 15 min | None |
| 4 | `compositor_system` | 3 hours | Steps 1, 2, 3 |
| 5 | Update `viewport_update_system` | 30 min | Step 4 |
| 6 | Update `AddClipCommand` to create `ClipAssetRef` | 1 hour | Step 1 |
| 7 | Integration testing | 2 hours | All |

**Total Estimate**: ~9 hours

---

## 5. Verification Plan

### 5.1 Automated Tests

```bash
# Unit test compositor logic
cargo test -p strawberry-render -- compositor

# Integration test with mock clips
cargo test -p strawberry-app -- integration::compositor
```

### 5.2 Manual Verification

1. Start Project Strawberry
2. Import two different video files (A and B)
3. Create two tracks (Track 1 at bottom, Track 2 on top)
4. Place Clip A on Track 1 at 0:00
5. Place Clip B on Track 2 at 0:02 (overlapping)
6. Scrub playhead over the overlap region

**Success Criteria**:
- At 0:01 → Viewport shows Clip A
- At 0:03 → Viewport shows Clip B (topmost)
- At 0:05 (after B ends) → Viewport shows Clip A again

---

## 6. Honest Assessment: Is Phase 1 Enough?

> [!CAUTION]
> **Phase 1 is a QUICK FIX, not a production-grade architecture.**

After deep analysis of Olive's 1350-line `Node` base class and full node ecosystem, I must be honest:

| Feature | Phase 1 Plan | Olive's Architecture | Production Need |
|---------|--------------|---------------------|-----------------|
| Multi-track compositing | ✅ Yes | ✅ Yes | ✅ |
| Time remapping | ✅ Yes | ✅ Yes | ✅ |
| GPU blending | ❌ Later | ✅ Yes | ✅ |
| Effect nodes (blur, color) | ❌ No | ✅ Yes | ✅ |
| Node graph connections | ❌ No | ✅ Yes | ✅ |
| Shader generation per-node | ❌ No | ✅ Yes | ✅ |
| Gizmo system (motion tracking handles) | ❌ No | ✅ Yes | ✅ |
| Keyframe tracks per parameter | ❌ No | ✅ Yes | ✅ |
| AI/VFX plugin hooks | ❌ No | ⚠️ Extensible | ✅ |

**Conclusion**: Phase 1 fixes the immediate viewport sync issue, but **will NOT** support VFX, CGI, motion tracking, or 3D without a major rewrite.

---

## 7. Production-Grade Architecture (Full Solution)

To truly compete with CapCut/Premiere/Olive, we need a **Node Graph** architecture where:
- **Everything is a Node**: Clips, Effects, Generators, Outputs, Transforms
- **Nodes connect via Inputs/Outputs**: Like Blender's node editor or Nuke
- **Each node can generate shaders**: GPU-accelerated processing
- **Gizmos provide UI interaction**: For motion tracking, corner pins, etc.

### 7.1 Core Node System

#### [NEW] `crates/strawberry-core/src/node/mod.rs`

```rust
//! # Node Graph System
//!
//! Production-grade node architecture based on Olive Editor.
//! Every processing unit is a Node with typed inputs and outputs.

use bevy_ecs::prelude::*;
use uuid::Uuid;
use std::collections::HashMap;

/// Categories that nodes can belong to (from Olive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    Output,      // Viewer, Export
    Generator,   // Solid, Text, Noise, Shape
    Math,        // Add, Multiply, Clamp
    Keying,      // Chroma key, Luma key
    Filter,      // Blur, Sharpen, Edge detect
    Color,       // Color correct, LUT, HSV
    Time,        // Time remap, Freeze
    Timeline,    // Track, Clip
    Transition,  // Dissolve, Wipe
    Distort,     // Transform, Corner pin, Wave
    Project,     // Project settings
    AI,          // Custom: AI effects, upscaling
    Tracking,    // Custom: Motion tracking
    ThreeD,      // Custom: 3D layers
}

/// Base trait for all nodes
pub trait Node: Send + Sync {
    /// Unique identifier for this node type (e.g., "org.strawberry.blur")
    fn id(&self) -> &'static str;
    
    /// Human-readable name
    fn name(&self) -> &'static str;
    
    /// Category for menu organization
    fn category(&self) -> NodeCategory;
    
    /// Define inputs this node accepts
    fn inputs(&self) -> Vec<InputDefinition>;
    
    /// Generate WGSL shader code for this node (if GPU-based)
    fn shader_code(&self) -> Option<ShaderCode>;
    
    /// Process values and produce output (called by traverser)
    fn process(&self, inputs: &NodeValueRow, globals: &NodeGlobals) 
        -> NodeValueTable;
    
    /// Optional: Create gizmos for viewport interaction
    fn create_gizmos(&self) -> Vec<Box<dyn Gizmo>> {
        vec![]
    }
}

/// Definition of a node input
#[derive(Debug, Clone)]
pub struct InputDefinition {
    pub id: String,
    pub name: String,
    pub data_type: NodeValueType,
    pub default_value: NodeValue,
    pub keyframable: bool,
    pub connectable: bool,
}

/// Types of values that can flow between nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeValueType {
    None,
    Boolean,
    Integer,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Matrix3,
    Matrix4,
    Color,
    Texture,
    Samples,  // Audio
    Footage,  // Video file reference
}

/// A value passed between nodes
#[derive(Debug, Clone)]
pub enum NodeValue {
    None,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Vec2(glam::Vec2),
    Vec3(glam::Vec3),
    Vec4(glam::Vec4),
    Matrix3(glam::Mat3),
    Matrix4(glam::Mat4),
    Color([f32; 4]),
    Texture(TextureHandle),
    Samples(AudioSamples),
    Footage(FootageRef),
}

/// Shader code generated by a node
#[derive(Debug, Clone)]
pub struct ShaderCode {
    pub vertex: Option<String>,
    pub fragment: String,
}
```

### 7.2 Node Graph Traverser

#### [NEW] `crates/strawberry-render/src/traverser.rs`

```rust
//! Node Graph Traverser (mirrors Olive's NodeTraverser)
//!
//! Walks the node graph, generates value tables, and resolves jobs.

use strawberry_core::node::*;

pub struct NodeTraverser {
    video_params: VideoParams,
    audio_params: AudioParams,
    /// Cache to prevent reprocessing the same node/time combo
    value_cache: HashMap<(NodeId, TimeRange), NodeValueTable>,
}

impl NodeTraverser {
    /// Generate the output table for a node at a given time
    pub fn generate_table(
        &mut self,
        node: &dyn Node,
        range: TimeRange,
    ) -> NodeValueTable {
        // Check cache first
        let key = (node.id(), range);
        if let Some(cached) = self.value_cache.get(&key) {
            return cached.clone();
        }
        
        // Generate database of input values
        let database = self.generate_database(node, range);
        
        // Generate row from database
        let row = self.generate_row(&database, node, range);
        
        // Let node process and produce output
        let globals = NodeGlobals::new(self.video_params, self.audio_params, range);
        let table = node.process(&row, &globals);
        
        // Cache and return
        self.value_cache.insert(key, table.clone());
        table
    }
    
    /// Resolve GPU jobs (ShaderJob, FootageJob) into actual textures
    pub fn resolve_jobs(&mut self, value: &mut NodeValue, renderer: &Renderer) {
        // Similar to Olive's ResolveJobs
        // Dispatches to ProcessShader, ProcessVideoFootage, etc.
    }
}
```

### 7.3 Built-in Node Examples

#### Generator: Solid Color
```rust
pub struct SolidNode {
    color: [f32; 4],
}

impl Node for SolidNode {
    fn id(&self) -> &'static str { "org.strawberry.solid" }
    fn name(&self) -> &'static str { "Solid" }
    fn category(&self) -> NodeCategory { NodeCategory::Generator }
    
    fn inputs(&self) -> Vec<InputDefinition> {
        vec![InputDefinition {
            id: "color".into(),
            name: "Color".into(),
            data_type: NodeValueType::Color,
            default_value: NodeValue::Color([0.0, 0.0, 0.0, 1.0]),
            keyframable: true,
            connectable: true,
        }]
    }
    
    fn shader_code(&self) -> Option<ShaderCode> {
        Some(ShaderCode {
            vertex: None,
            fragment: r#"
                @fragment
                fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
                    return uniforms.color;
                }
            "#.into(),
        })
    }
    
    fn process(&self, inputs: &NodeValueRow, globals: &NodeGlobals) -> NodeValueTable {
        // Create a ShaderJob for the solid color
        let mut table = NodeValueTable::new();
        table.push(NodeValue::Texture(
            ShaderJob::new("solid", &[("color", inputs.get("color"))])
        ));
        table
    }
}
```

#### Effect: Gaussian Blur
```rust
pub struct BlurNode;

impl Node for BlurNode {
    fn id(&self) -> &'static str { "org.strawberry.blur" }
    fn name(&self) -> &'static str { "Blur" }
    fn category(&self) -> NodeCategory { NodeCategory::Filter }
    
    fn inputs(&self) -> Vec<InputDefinition> {
        vec![
            InputDefinition::texture("input", "Input"),
            InputDefinition::float("radius", "Radius", 10.0),
            InputDefinition::combo("method", "Method", &["Gaussian", "Box", "Directional"]),
        ]
    }
    
    fn shader_code(&self) -> Option<ShaderCode> {
        // See research/rendering/shaders/blur.frag for reference
        Some(ShaderCode::from_file("blur.wgsl"))
    }
}
```

### 7.4 Gizmo System (for Motion Tracking, Corner Pin, etc.)

#### [NEW] `crates/strawberry-ui/src/gizmo/mod.rs`

```rust
//! Gizmo System for viewport interaction
//!
//! Gizmos are draggable UI elements that control node parameters.
//! Used for: Transform handles, Corner pins, Bezier paths, Tracking points

pub trait Gizmo: Send + Sync {
    /// Draw the gizmo in the viewport
    fn draw(&self, ctx: &mut GizmoContext);
    
    /// Handle mouse interaction
    fn on_drag_start(&mut self, pos: Vec2, time: Duration);
    fn on_drag_move(&mut self, delta: Vec2, modifiers: Modifiers);
    fn on_drag_end(&mut self) -> UndoCommand;
    
    /// Which node input does this gizmo control?
    fn target_input(&self) -> NodeInput;
}

/// Point gizmo (for transform position, tracking points)
pub struct PointGizmo {
    position: Vec2,
    size: f32,
    color: [f32; 4],
    target: NodeInput,
}

/// Rect gizmo (for corner pin, crop)
pub struct RectGizmo {
    corners: [Vec2; 4],
    target: NodeInput,
}

/// Path gizmo (for bezier masks, motion paths)
pub struct PathGizmo {
    points: Vec<BezierPoint>,
    closed: bool,
    target: NodeInput,
}
```

---

## 8. Foundation vs Rewrite Analysis

> [!IMPORTANT]
> **Will Phase 1 be a foundation or require a full rewrite?**

After reviewing `agent.md` rules, here's an honest breakdown:

### What Survives from Phase 1 ✅ (Foundation)
| Component | Reusability | Why |
|-----------|-------------|-----|
| `VideoAssetManager` | ✅ 100% | Stateless service, nodes will call it |
| `ClipAssetRef` component | ✅ 100% | Becomes data inside `ClipNode` |
| Time remapping logic | ✅ 100% | Core algorithm, reusable anywhere |
| `CompositorOutput` resource | ✅ 80% | Minor changes for node-based output |

### What Gets Replaced ❌ (Rewrite)
| Component | Replacement | Why |
|-----------|-------------|-----|
| `compositor_system` | `NodeTraverser` | ECS system → Graph traversal |
| Direct clip queries | `ClipNode.process()` | Timeline→Node abstraction |

### Verdict: ~60% Foundation, ~40% Rewrite

**Recommendation**: Design Phase 1 with the Node architecture in mind, even if we don't have a visual node graph yet. This way Phase 1 IS the foundation.

---

## 9. Revised Phased Approach (Node-First)

> [!TIP]
> **New Strategy**: Build the Node foundation in Phase 1, but with a simplified "linear traverser" (no visual node graph UI). Add the graph UI later.

### Phase 1 (Foundation) - 1.5 Weeks
**Goal**: Fix viewport sync with node-compatible architecture.

| Deliverable | Location | Lines | README Required |
|-------------|----------|-------|-----------------|
| `Node` trait | `strawberry-core/src/node/mod.rs` | ~150 | ✅ `node/README.md` |
| `NodeValue` types | `strawberry-core/src/node/value.rs` | ~100 | - |
| `NodeInput` | `strawberry-core/src/node/input.rs` | ~80 | - |
| `ClipNode` impl | `strawberry-core/src/node/timeline/clip.rs` | ~200 | ✅ `node/timeline/README.md` |
| `TrackNode` impl | `strawberry-core/src/node/timeline/track.rs` | ~150 | - |
| `VideoAssetManager` | `strawberry-video/src/asset_manager.rs` | ~300 | ✅ `service/README.md` update |
| `NodeTraverser` (simplified) | `strawberry-render/src/traverser.rs` | ~200 | ✅ `render/README.md` |
| Viewport integration | `strawberry-render/src/viewport.rs` | ~100 | - |

> [!NOTE]
> **AI Context Rule**: Every new module directory MUST contain a `README.md` explaining purpose, data flow, and key structs.

### Phase 2 (GPU Blending) - 1 Week
**Goal**: Alpha-over compositing on GPU.

| Deliverable | Location | Lines | README Required |
|-------------|----------|-------|-----------------|
| `alphaover.wgsl` | `strawberry-render/src/shaders/alphaover.wgsl` | ~50 | ✅ `shaders/README.md` |
| `ShaderJob` execution | `strawberry-render/src/executor.rs` | ~200 | - |
| `BlendNode` | `strawberry-core/src/node/blend/mod.rs` | ~150 | ✅ `node/blend/README.md` |

### Phase 3 (Effect Nodes) - 2 Weeks
**Goal**: Basic effects (blur, color).

| Deliverable | Location | Lines | README Required |
|-------------|----------|-------|-----------------|
| `SolidNode` | `strawberry-core/src/node/generator/solid.rs` | ~80 | ✅ `node/generator/README.md` |
| `TextNode` | `strawberry-core/src/node/generator/text.rs` | ~150 | - |
| `BlurNode` | `strawberry-core/src/node/filter/blur.rs` | ~120 | ✅ `node/filter/README.md` |
| `ColorCorrectionNode` | `strawberry-core/src/node/color/correction.rs` | ~180 | ✅ `node/color/README.md` |

### Phase 4 (Gizmo System) - 2 Weeks
**Goal**: Viewport interaction handles.

| Deliverable | Location | Lines | README Required |
|-------------|----------|-------|-----------------|
| `Gizmo` trait | `strawberry-ui/src/gizmo/mod.rs` | ~100 | ✅ `gizmo/README.md` |
| `PointGizmo` | `strawberry-ui/src/gizmo/point.rs` | ~150 | - |
| `RectGizmo` | `strawberry-ui/src/gizmo/rect.rs` | ~180 | - |
| `PathGizmo` | `strawberry-ui/src/gizmo/path.rs` | ~200 | - |
| `TransformNode` + gizmo | `strawberry-core/src/node/distort/transform.rs` | ~250 | ✅ `node/distort/README.md` |

### Phase 5 (AI/VFX/3D) - Ongoing
**Goal**: Plugin architecture for advanced features.

| Deliverable | Location | Lines | README Required |
|-------------|----------|-------|-----------------|
| `NodeRegistry` (dynamic loading) | `strawberry-core/src/node/registry.rs` | ~200 | - |
| AI upscaling hook | `strawberry-core/src/node/ai/upscale.rs` | ~150 | ✅ `node/ai/README.md` |
| Motion tracking integration | `strawberry-core/src/node/tracking/mod.rs` | ~300 | ✅ `node/tracking/README.md` |
| 3D layer support | `strawberry-core/src/node/threed/mod.rs` | ~400 | ✅ `node/threed/README.md` |

---

## 10. AI-Native Standards Compliance

Per `agent.md` and `/agent` workflow, ALL code must follow these rules:

### 10.1 The 400-Line Soft Limit
Every file in the plan is designed to stay under 400 lines. When approaching the limit:
```
node/mod.rs (approaching 400 lines)
  → Split into:
    node/mod.rs          (~50 lines, re-exports)
    node/trait.rs        (~150 lines, Node trait)
    node/value.rs        (~100 lines, NodeValue enum)
    node/input.rs        (~80 lines, InputDefinition)
```

### 10.2 README.md Requirements
**Every new module directory MUST have a README.md** with:
1. **Purpose**: Single-sentence description
2. **Data Flow**: How data moves through this module
3. **Key Structs**: List of main types with one-line descriptions
4. **Dependencies**: What this module imports from other modules

Example `strawberry-core/src/node/README.md`:
```markdown
# Node System

> Production-grade node graph architecture for compositing.

## Data Flow
```
Timeline → TrackNode → ClipNode → NodeTraverser → GPU Jobs → Texture
```

## Key Structs
- `Node`: Base trait for all processing units
- `NodeValue`: Typed values flowing between nodes
- `NodeTraverser`: Walks graph and resolves GPU jobs

## Dependencies
- `strawberry-video`: For footage decoding
- `uuid`: For stable node IDs
```

### 10.3 Separation of Concerns
| Crate | Contains | Never Contains |
|-------|----------|----------------|
| `strawberry-core` | Node trait, value types, logic | `egui`, `wgpu`, UI code |
| `strawberry-render` | NodeTraverser, GPU execution | Business logic |
| `strawberry-ui` | Gizmos, viewport widget | Node processing logic |
| `strawberry-video` | VideoAssetManager | ECS code, UI code |

### 10.4 Command Pattern for Node Operations
All node manipulations must use Commands:
```rust
pub struct AddNodeCommand {
    pub node_uuid: Uuid,
    pub node_type: String,
    pub parent_uuid: Uuid,
    pub position: Vec2,
}

pub struct ConnectNodesCommand {
    pub from_node: Uuid,
    pub from_output: String,
    pub to_node: Uuid,
    pub to_input: String,
}

pub struct SetNodeInputCommand {
    pub node_uuid: Uuid,
    pub input_id: String,
    pub value: NodeValue,
}
```

### 10.5 Explicit Types (No Inference)
```rust
// ❌ Bad
let table = traverser.generate_table(node, range);

// ✅ Good
let table: NodeValueTable = traverser.generate_table(node, range);
```

---

## 11. Decision Point (Updated)

> [!IMPORTANT]
> **Choose your path:**
>
> **Option A (RECOMMENDED)**: Node-First Foundation
> - Build Node trait + ClipNode + simplified traverser in Phase 1
> - Fixes viewport sync AND creates foundation
> - ~1.5 weeks, 60% more work than quick fix
> - **No rewrite needed later**
>
> **Option B**: Quick Fix (Original Phase 1)
> - Fastest path to working viewport
> - ~1 week
> - **Requires 40% rewrite in Phase 2**

I strongly recommend **Option A** based on your rules:
- Rule 3: "NEVER implement workarounds or temporary fixes"
- The quick fix IS a temporary fix

---

