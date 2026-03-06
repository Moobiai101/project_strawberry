# Project Strawberry: AI-Native Creative Engine
## System Design Document v1.0

> **Classification:** Internal Engineering Document  
> **Authors:** Founding Engineering Team  
> **Last Updated:** 2026-01-18  
> **Status:** APPROVED FOR DEVELOPMENT

> [!IMPORTANT]
> **THIS IS NOT AN MVP.** This is a production-grade AI-native creative engine that users will depend on as their primary creative tool. Every line of code must be shippable quality. No workarounds, no shortcuts, no technical debt.

**Code Name:** Project Strawberry (final name TBD)  
**Target Platform:** Windows (primary)  
**Export Targets:** All major formats (MP4, WebM, ProRes, etc.)

---

## Executive Summary

Project Strawberry is a production-grade, AI-native creative engine for video editing, motion graphics, color grading, VFX, and animation. The architecture is built on three pillars:

1. **ECS is King** — Every feature is an Entity-Component-System pattern
2. **AI-Native** — Every action is an AI-invocable command from ground up
3. **UI-First Validation** — Every feature ships with UI to test immediately

**Target:** Production-ready v1.0 in **18 months** (6 phases × 3 months each)

### Current Progress (as of 2026-01-21)

| Phase | Status | Weeks Completed |
|-------|--------|-----------------|
| **Phase 0: Foundation** | ✅ COMPLETE | 12/12 weeks |
| **Phase 1: Core Editing** | 🔄 IN PROGRESS | 5/12 weeks |
| Phase 2: Effects & Color | ⏳ Not Started | 0/12 weeks |
| Phase 3: Animation & Motion | ⏳ Not Started | 0/12 weeks |
| Phase 4: AI Integration | ⏳ Not Started | 0/12 weeks |
| Phase 5: Production Polish | ⏳ Not Started | 0/12 weeks |

**Recent Completions (Phase 1 Week 1-5):**
- Timeline components: `Timeline`, `Track`, `Clip`, `TrackClip`, `ClipMetadata`
- Timeline commands: `CreateTimelineCommand`, `CreateTrackCommand`, `AddClipCommand`, `MoveClipCommand`, `DeleteClipCommand`
- Selection commands: `SelectClipCommand`, `ClearSelectionCommand`, `ToggleSelectionCommand`
- Track commands: `SetTrackMutedCommand`, `SetTrackSoloCommand`, `SetTrackLockedCommand`
- UI: `TimelineWidget` with ruler, clip blocks, selection, drag-to-move, playhead click-to-seek, Ctrl+scroll zoom
- **CapCut-style UI redesign (2026-01-19)**:
  - Removed track headers (M/S/L buttons) for cleaner CapCut look
  - Reduced clip height (60px → 35px) for compact clips
  - Added `TimelineViewState` resource for zoom/scroll/playhead follow mode
  - Implemented centered playhead auto-scroll during playback
  - Empty state: styled "Drag videos or images here" drop zone pill
- **CapCut-style timeline interactions (2026-01-21)**:
  - Centered playhead mode: playhead fixed at center, clips scroll during scrub/playback
  - Scrub from anywhere: drag on ruler, empty area, or clips to scrub
  - Frame-snapped smooth scrolling: mousewheel/trackpad seeks in frame increments
  - Visible trim handles: white bars on clip edges with ResizeHorizontal cursor
  - DragOrigin tracking: distinguishes Ruler, Empty, ClipBody, TrimEdge drags
  - Auto-scroll during move/trim: timeline scrolls when dragging near edges (120px zone)
  - Clip selection works in centered mode: click on clips always selects them

- **Ripple Delete (2026-01-22)**:
  - `RippleDeleteCommand` implemented with gap removal
  - Added "Ripple Delete" to context menu (UI fallback for shortcut issues)
  - Shortcuts: `Alt+Delete`, `Shift+Backspace`
- **Stable UUID Migration (2026-01-24)**:
  - Migrated `Move\`, `Trim\`, `Slip\`, `Slide`, `Split`, `RippleDelete` to use stable UUIDs
  - Ensures robust undo/redo even if entity IDs change
- **Advanced Editing (2026-01-24)**:
  - `SlipClipCommand`: Shift media in/out points without moving clip (UUID-aware)
  - `SlideClipCommand`: Move clip while adjusting neighbors (UUID-aware)

**Next Up (Phase 1 Week 8):**
- **Multi-track Compositing**: Layer ordering, opacity blending
- **Transitions System**: Dissolve, wipe, push shaders
- **Project Save/Load**

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Core Design Principles](#2-core-design-principles)
3. [Technology Stack](#3-technology-stack)
4. [Phase 0: Foundation](#4-phase-0-foundation-months-1-3)
5. [Phase 1: Core Editing](#5-phase-1-core-editing-months-4-6)
6. [Phase 2: Effects & Color](#6-phase-2-effects--color-months-7-9)
7. [Phase 3: Animation & Motion](#7-phase-3-animation--motion-months-10-12)
8. [Phase 4: AI Integration](#8-phase-4-ai-integration-months-13-15)
9. [Phase 5: Production Polish](#9-phase-5-production-polish-months-16-18)
10. [API Design for AI Control](#10-api-design-for-ai-control)
11. [Testing Strategy](#11-testing-strategy)
12. [Future Enhancements](#12-future-enhancements-competitive-edge-features)
13. [Risk Mitigation](#13-risk-mitigation)

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER / AI AGENT                                │
│                                    │                                        │
│                                    ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         COMMAND LAYER                                 │  │
│  │  Every action is a Command. No direct state mutation allowed.         │  │
│  │  Commands are: Undoable, Serializable, AI-Invocable                   │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                    │                                        │
│                                    ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                           ECS WORLD                                   │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐     │  │
│  │  │  Entities   │ │ Components  │ │  Systems    │ │  Resources  │     │  │
│  │  │  (Clips,    │ │ (Data only, │ │ (Logic,     │ │ (Shared     │     │  │
│  │  │   Layers,   │ │  no logic)  │ │  queries    │ │  state)     │     │  │
│  │  │   Effects)  │ │             │ │  over ECS)  │ │             │     │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘     │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                    │                                        │
│                                    ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         SERVICE LAYER                                 │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐     │  │
│  │  │   Video     │ │   Audio     │ │   Render    │ │   Asset     │     │  │
│  │  │   Decode    │ │   Engine    │ │   Graph     │ │   Manager   │     │  │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘     │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                    │                                        │
│                                    ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                           GPU LAYER                                   │  │
│  │                    wgpu (Vulkan/Metal/DX12)                           │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Core Design Principles

### 2.1 The Command Pattern (CRITICAL)

**Every user action and AI action goes through the same Command interface:**

```rust
/// Every action in the system implements this trait
pub trait Command: Send + Sync + 'static {
    /// Unique identifier for this command type
    fn id(&self) -> CommandId;
    
    /// Human-readable description (for AI and undo history)
    fn description(&self) -> String;
    
    /// Execute the command, returning data needed for undo
    fn execute(&self, world: &mut World) -> Result<UndoData, CommandError>;
    
    /// Reverse the command using the undo data
    fn undo(&self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError>;
    
    /// Serialize for AI invocation and project save
    fn serialize(&self) -> serde_json::Value;
    
    /// Deserialize from AI request or project load
    fn deserialize(data: serde_json::Value) -> Result<Self, CommandError> where Self: Sized;
}
```

**Why this matters for AI:**
- AI sends: `{"command": "split_clip", "clip_id": "abc123", "at_frame": 120}`
- Same code path as user clicking "Split" button
- Full undo support
- Complete action history for AI context

### 2.2 ECS Purity Rules

| Rule | Rationale |
|------|-----------|
| Components are **data only** | No methods beyond accessors |
| Systems are **stateless functions** | Query ECS, process, write back |
| No globals | Use ECS Resources instead |
| Entity references via IDs | Never hold `&Entity` across frames |
| Events for cross-system communication | Decouple systems |

### 2.3 UI-Feature Coupling

```
┌─────────────────────────────────────────┐
│  Feature Implementation Checklist:      │
│                                         │
│  □ Component(s) defined                 │
│  □ System(s) implemented                │
│  □ Command(s) created                   │
│  □ UI widget connected                  │
│  □ AI schema documented                 │
│  □ Tests written                        │
│  □ Feature flag added                   │
│                                         │
│  No feature ships without ALL boxes ✓   │
└─────────────────────────────────────────┘
```

### 2.4 AI-ECS Architecture (CRITICAL)

**The AI Agent does NOT directly mutate ECS.** It operates through the same Command layer as the UI.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        AI AGENT ARCHITECTURE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         AI SIDECAR PROCESS                          │   │
│  │  (Separate process or thread - isolates AI latency from main loop)  │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                  │   │
│  │  │   Context   │  │    LLM      │  │   Action    │                  │   │
│  │  │   Builder   │  │  Inference  │  │   Parser    │                  │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘                  │   │
│  │         │                │                │                          │   │
│  └─────────┼────────────────┼────────────────┼──────────────────────────┘   │
│            │                │                │                              │
│            ▼                ▼                ▼                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      MESSAGE BUS (IPC)                              │   │
│  │              (Channels: tokio::mpsc or gRPC)                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│            │                                 │                              │
│            ▼                                 ▼                              │
│  ┌─────────────────────┐          ┌─────────────────────┐                  │
│  │   QUERY INTERFACE   │          │  COMMAND INTERFACE  │                  │
│  │  (Read-only access) │          │  (Same as UI uses)  │                  │
│  │                     │          │                     │                  │
│  │  • Project state    │          │  • Execute command  │                  │
│  │  • Timeline data    │          │  • Undo/Redo        │                  │
│  │  • Clip metadata    │          │  • Batch commands   │                  │
│  │  • AI annotations   │          │                     │                  │
│  └──────────┬──────────┘          └──────────┬──────────┘                  │
│             │                                │                              │
│             ▼                                ▼                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         ECS WORLD                                   │   │
│  │           (AI reads via queries, writes via commands)               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Principles:**

| Principle | Implementation |
|-----------|----------------|
| AI never mutates directly | All writes go through `CommandQueue` |
| AI queries are read-only | Snapshot of ECS state serialized to JSON |
| AI runs in sidecar | Separate async task, doesn't block main loop |
| Same code path as user | `SplitClipCommand` from AI = from button click |
| Full auditability | Every AI action logged in command history |

**AI Query API (Read-Only):**
```rust
/// AI can query the ECS world through these interfaces
pub trait AIQueryable {
    /// Get serializable snapshot of entity
    fn to_ai_context(&self) -> serde_json::Value;
}

/// Query service that AI sidecar uses
pub struct AIQueryService {
    world: Arc<RwLock<World>>,
}

impl AIQueryService {
    /// Get current timeline state
    pub fn get_timeline_context(&self) -> TimelineContext {
        let world = self.world.read();
        let mut context = TimelineContext::default();
        
        for (entity, clip, transform) in world.query::<(Entity, &Clip, &ClipTransform)>().iter(&world) {
            context.clips.push(ClipContext {
                id: entity.to_bits().to_string(),
                name: clip.name.clone(),
                start_frame: clip.timeline_start.as_secs_f64() * 30.0,
                duration_frames: (clip.source_out - clip.source_in).as_secs_f64() * 30.0,
                transform: transform.to_ai_context(),
                // ... more fields
            });
        }
        
        context
    }
    
    /// Get recently executed commands (for AI to understand user intent)
    pub fn get_command_history(&self, limit: usize) -> Vec<CommandSummary> {
        // Returns last N commands for context
    }
}
```

**AI Command Interface (Write via Commands):**
```rust
/// AI submits commands through this interface
pub struct AICommandInterface {
    command_queue: Sender<Box<dyn Command>>,
    command_registry: Arc<CommandRegistry>,
}

impl AICommandInterface {
    /// AI sends a command request as JSON
    pub fn submit(&self, request: AICommandRequest) -> Result<CommandId, AIError> {
        // 1. Validate the command exists
        let command_def = self.command_registry.get(&request.command_type)?;
        
        // 2. Deserialize parameters
        let command = command_def.deserialize(request.params)?;
        
        // 3. Optionally require user confirmation for destructive commands
        if command_def.requires_confirmation && !request.user_confirmed {
            return Err(AIError::ConfirmationRequired(command));
        }
        
        // 4. Submit to queue (same queue UI uses)
        let id = CommandId::new();
        self.command_queue.send(command)?;
        
        Ok(id)
    }
} // THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

### 2.5 AI Context Management Strategy

> [!IMPORTANT]
> Based on research from Anthropic's Claude Code, Cursor, and academic papers on context engineering (2024-2025), we use a **hybrid approach** combining structured context, embeddings, and hierarchical summarization.

**The "Context Rot" Problem:**
Even with 200K token context windows, performance degrades when we dump everything. Quality > quantity.

**Our Strategy: Tiered Context Architecture**

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    AI CONTEXT TIERS                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TIER 1: Always Present (< 2K tokens)                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • System prompt (role, capabilities, constraints)                   │   │
│  │  • Project summary (name, resolution, duration, track count)         │   │
│  │  • Current selection (what user is working on)                       │   │
│  │  • Playhead position                                                 │   │
│  │  • Recent user actions (last 5 commands)                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TIER 2: Dynamically Retrieved (< 8K tokens)                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • Clips near playhead (±30 seconds)                                 │   │
│  │  • Selected entity full details                                      │   │
│  │  • Active track clips                                                │   │
│  │  • Effect parameters of selected clip                                │   │
│  │  • AI annotations for visible clips                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TIER 3: Retrieved via Semantic Search (< 4K tokens)                        │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • Clips matching user's query (via embeddings)                      │   │
│  │  • Similar operations from command history                           │   │
│  │  • Relevant templates/presets                                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TIER 4: Summarized/Compressed (< 2K tokens)                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • Full timeline summary (auto-generated)                            │   │
│  │  • Scene descriptions for all clips                                  │   │
│  │  • User's editing patterns/preferences                               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TOTAL TARGET: < 16K tokens (even with 200K context, less is better)        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Implementation Components:**

```rust
/// Context builder that assembles tiered context for AI
pub struct AIContextBuilder {
    query_service: AIQueryService,
    embedding_index: EmbeddingIndex,
    summarizer: Summarizer,
}

impl AIContextBuilder {
    pub fn build_context(&self, user_query: &str, world: &World) -> AIContext {
        let mut context = AIContext::new();
        
        // Tier 1: Always present
        context.add_tier1(self.build_project_summary(world));
        context.add_tier1(self.get_current_selection(world));
        context.add_tier1(self.get_recent_commands(world, 5));
        
        // Tier 2: Dynamic based on focus
        let playhead = world.resource::<Playhead>();
        context.add_tier2(self.get_clips_near_playhead(world, playhead, Duration::from_secs(30)));
        context.add_tier2(self.get_selected_entity_details(world));
        
        // Tier 3: Semantic search if user query is complex
        if !user_query.is_empty() {
            let relevant = self.embedding_index.search(user_query, 5);
            context.add_tier3(relevant);
        }
        
        // Tier 4: Pre-computed summaries
        context.add_tier4(self.summarizer.get_timeline_summary(world));
        
        context
    }
}

/// Embedding index for semantic search over project content
pub struct EmbeddingIndex {
    embeddings: HashMap<EntityId, Vec<f32>>,
    model: EmbeddingModel,  // e.g., all-MiniLM-L6-v2 via ort
}

impl EmbeddingIndex {
    /// Index a clip's content for semantic search
    pub fn index_clip(&mut self, entity: Entity, clip: &Clip, annotations: &AIAnnotation) {
        let text = format!(
            "{} {} {}",
            clip.name,
            annotations.description,
            annotations.tags.join(" ")
        );
        let embedding = self.model.embed(&text);
        self.embeddings.insert(entity.to_bits(), embedding);
    }
    
    /// Search for clips semantically similar to query
    pub fn search(&self, query: &str, top_k: usize) -> Vec<EntityId> {
        let query_embedding = self.model.embed(query);
        // Cosine similarity search
        let mut scores: Vec<_> = self.embeddings.iter()
            .map(|(id, emb)| (*id, cosine_similarity(&query_embedding, emb)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.into_iter().take(top_k).map(|(id, _)| id).collect()
    }
} // THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

**Why This Works:**

| Approach | Problem It Solves |
|----------|-------------------|
| Tiered context | Prioritizes relevance, avoids context rot |
| Structured JSON | AI knows exactly what each field means |
| Embeddings (Tier 3) | Finds relevant content in large projects |
| Summaries (Tier 4) | Compresses long histories into key facts |
| Command history | AI understands user intent from recent actions |
| Selection focus | AI knows what user is working on right now |

**Dependencies for AI Context:**
```toml
# Embedding model
ort = "2"                    # MIT - ONNX Runtime
fastembed = "0.3"            # MIT - Easy embedding models

# Vector similarity
simsimd = "0.4"              # Apache 2.0 - Fast similarity
```

### 2.6 AI Testability from Phase 0

> [!IMPORTANT]  
> AI is NOT a Phase 4 bolt-on. Every phase must include AI testing criteria.

**AI Testing Strategy (Progressive):**

| Phase | What AI Can Do | Test Coverage |
|-------|---------------|---------------|
| **Phase 0** | Query project state, control playback | `POST /command/seek` works |
| **Phase 1** | Add/remove/edit clips, transitions | AI can create rough cut |
| **Phase 2** | Apply effects, adjust colors | AI can apply effect stack |
| **Phase 3** | Create animations, control Rive | AI can animate text |
| **Phase 4** | Natural language, auto-edit | Full NLU + complex tasks |
| **Phase 5** | Production reliability | AI handles edge cases |

**Phase 0 AI Infrastructure:**
```rust
// From Phase 0, we have:
strawberry-ai/
├── src/
│   ├── lib.rs
│   ├── query_service.rs      // Read-only ECS queries
│   ├── command_interface.rs  // Submit commands
│   ├── context_builder.rs    // Build AI context
│   └── api/
│       ├── mod.rs
│       ├── http.rs           // REST API for external AI
│       └── local.rs          // In-process AI
```

**Phase 0 AI Tests:**
```rust
#[test]
fn ai_can_query_project_state() {
    let world = setup_test_world();
    let query_service = AIQueryService::new(&world);
    
    let state = query_service.get_project_context();
    
    assert_eq!(state.name, "Test Project");
    assert_eq!(state.resolution, [1920, 1080]);
}

#[test]
fn ai_can_execute_seek_command() {
    let (world, command_interface) = setup_test_world_with_ai();
    
    let request = AICommandRequest {
        command_type: "seek".to_string(),
        params: json!({ "frame": 100 }),
        user_confirmed: true,
    };
    
    let result = command_interface.submit(request);
    assert!(result.is_ok());
    
    // Verify playhead moved
    let playhead = world.resource::<Playhead>();
    assert_eq!(playhead.current_frame, 100);
}
```

---

## 3. Technology Stack

### 3.1 Core Dependencies (Complete List)

```toml
[dependencies]
# === ECS Foundation (from Bevy) ===
bevy_ecs = "0.15"           # MIT - Entity Component System
bevy_app = "0.15"           # MIT - Application lifecycle
bevy_tasks = "0.15"         # MIT - Async task pool
bevy_time = "0.15"          # MIT - Time management
bevy_hierarchy = "0.15"     # MIT - Parent/child relationships
bevy_reflect = "0.15"       # MIT - Runtime reflection
bevy_state = "0.15"         # MIT - State machines

# === Graphics (Core) ===
wgpu = "23"                 # MIT - GPU abstraction (Vulkan/Metal/DX12)
vello = "0.3"               # MIT - GPU 2D vector rendering
kurbo = "0.11"              # MIT - 2D geometry (beziers, shapes)
peniko = "0.3"              # MIT - 2D styling (gradients, brushes)
tiny-skia = "0.11"          # BSD - CPU 2D rasterizer (fallback)
image = "0.25"              # MIT - Image decode/encode
imageproc = "0.25"          # MIT - Image processing filters
resvg = "0.43"              # MPL 2.0 - SVG rendering
usvg = "0.43"               # MPL 2.0 - SVG parsing

# === Video ===
ffmpeg-next = "7"           # Apache 2.0 - FFmpeg bindings
mp4 = "0.14"                # MIT - MP4 container parsing
matroska = "0.23"           # MIT - MKV/WebM parsing
rav1e = "0.7"               # BSD - AV1 encoder

# === Audio ===
cpal = "0.15"               # Apache 2.0 - Audio I/O
symphonia = "0.5"           # MPL 2.0 - Audio decode (MP3, FLAC, WAV)
rubato = "0.15"             # MIT - High-quality resampling
dasp = "0.11"               # MIT - Audio DSP primitives
fundsp = "0.18"             # MIT - Audio processing nodes
hound = "3.5"               # MIT - WAV read/write

# === Text ===
cosmic-text = "0.12"        # MIT - Advanced text layout
fontdb = "0.21"             # MIT - Font discovery
ab_glyph = "0.2"            # Apache 2.0 - Font parsing

# === Math ===
glam = "0.29"               # MIT - Fast vector/matrix math
nalgebra = "0.33"           # Apache 2.0 - Advanced linear algebra

# === Animation ===
splines = "4"               # BSD - Bezier/Catmull-Rom curves
ezing = "0.1"               # MIT - Easing functions
spring-solver = "0.1"       # MIT - Physics spring animation
rive-rs = "0.1"             # MIT - Rive runtime

# === 3D Graphics ===
gltf = "1.4"                # MIT - glTF 2.0 loader
obj = "0.10"                # Apache 2.0 - OBJ mesh loader
meshopt = "0.2"             # MIT - Mesh optimization
ozz-animation-rs = "0.10"   # MIT - Skeletal animation

# === Color Science ===
palette = "0.7"             # MIT - Color math (RGB, HSL, Lab)
colorgrad = "0.6"           # MIT - Color gradients

# === Motion Tracking ===
opencv = "0.92"             # Apache 2.0 - OpenCV bindings
ndarray = "0.16"            # MIT - N-dimensional arrays

# === Serialization ===
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bincode = "1"               # MIT - Binary serialization
ron = "0.8"                 # MIT - Rust Object Notation
toml = "0.8"                # MIT - Config files

# === Async/Parallel ===
tokio = { version = "1", features = ["full"] }
rayon = "1.10"              # MIT - CPU parallelization
parking_lot = "0.12"        # MIT - Fast mutexes
crossbeam = "0.8"           # MIT - Lock-free structures

# === AI/ML ===
ort = "2"                   # MIT - ONNX Runtime
candle-core = "0.7"         # MIT - Rust ML framework
fastembed = "0.3"           # MIT - Text embeddings
async-openai = "0.24"       # MIT - OpenAI API
whisper-rs = "0.13"         # MIT - Speech recognition
llama-cpp-rs = "0.4"        # MIT - Local LLM (optional)

# === Networking ===
tonic = "0.12"              # MIT - gRPC
reqwest = "0.12"            # MIT - HTTP client

# === UI ===
egui = "0.29"               # MIT - Immediate mode GUI
eframe = "0.29"             # MIT - egui framework
egui-wgpu = "0.29"          # MIT - wgpu integration
```

### 3.2 External References (For Learning Only)

> [!WARNING]
> These are GPL/LGPL projects. Study the **concepts and algorithms**, never copy code.

#### Phase 0-1 References (Video Playback & Timeline)

| Project | What to Study | Key Files/Concepts |
|---------|---------------|-------------------|
| **Kdenlive** | Timeline model, clip representation | `src/timeline/`, track/clip data structures |
| **OpenShot** | libopenshot timeline, keyframe system | `src/Timeline.cpp`, `src/Keyframe.cpp` |
| **FFmpeg** | Filter graph, frame handling | `libavfilter/`, decode pipeline |
| **MLT Framework** | Producer/filter/consumer pattern | Core architecture concepts |

#### Phase 2 References (Effects & Color)

| Project | What to Study | Key Files/Concepts |
|---------|---------------|-------------------|
| **Blender Compositor** | Node-based compositing | `source/blender/compositor/` |
| **Natron** | OpenFX plugin hosting, node graph | `Engine/`, effect parameter model |
| **DaVinci Resolve** | Color science concepts | Study via tutorials (proprietary) |
| **Lygia Shaders** | MIT shader library | Direct use: blur, color, distort |
| **Shadertoy** | Effect algorithms (CC0/MIT) | Study + clean room implement |

#### Phase 3 References (Animation & Motion)

| Project | What to Study | Key Files/Concepts |
|---------|---------------|-------------------|
| **Blender Armature** | IK/FK, bone constraints | `source/blender/blenkernel/` |
| **Synfig** | 2D skeletal animation | `synfig-core/src/modules/` |
| **Rive** | State machines (MIT, use directly) | `rive-rs` crate |
| **Lottie** | After Effects animation format | `lottie` crate (MIT) |
| **ozz-animation** | Skeletal animation runtime | `ozz-animation-rs` (MIT) |

#### Phase 4 References (AI)

| Project | What to Study | Key Files/Concepts |
|---------|---------------|-------------------|
| **Whisper.cpp** | Speech recognition | `whisper-rs` (MIT) |
| **Segment Anything** | Object segmentation | ONNX model via `ort` |
| **CLIP** | Image understanding | Embeddings via `candle` |

### 3.3 Shader Algorithm Sources (MIT/CC0)

| Source | URL | What It Provides |
|--------|-----|-----------------|
| **Lygia** | lygia.xyz | Comprehensive shader library |
| **The Book of Shaders** | thebookofshaders.com | Algorithm explanations |
| **GPU Gems** | developer.nvidia.com | Advanced GPU techniques |
| **Shadertoy** | shadertoy.com | 100K+ shader examples |

---

### 3.4 UI Framework Decision

> [!IMPORTANT]
> We choose **egui** as the foundation, but we **maximize reuse** by:
> 1. Using existing MIT crates directly
> 2. Studying Qt-based NLEs (Olive, Kdenlive) for complex widget architecture
> 3. Building professional implementations inspired by proven designs

**Framework:** egui + wgpu  
**Strategy:** Port and improve, not build from scratch

### 3.5 UI Reuse Strategy

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    UI REUSE STRATEGY                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  TIER 1: USE DIRECTLY (MIT/Apache — Zero work)                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • egui_tiles      — Docking/tiling panel system                    │   │
│  │  • egui_dock       — Alternative docking                            │   │
│  │  • egui_plot       — Curve plotting (for graph editor base)         │   │
│  │  • egui_node_graph — Node editor (compositor, effects)              │   │
│  │  • egui_extras     — Tables, syntax highlighting                    │   │
│  │  • egui-file-dialog — File browser                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TIER 2: STUDY + PORT CONCEPTS (from Qt NLEs — GPL, clean room)            │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  OLIVE EDITOR (olive-editor/olive) — Modern NLE in Qt/C++           │   │
│  │  ├── Timeline architecture                                          │   │
│  │  ├── Clip rendering with thumbnails                                 │   │
│  │  ├── Transition overlays                                            │   │
│  │  ├── Node-based compositor                                          │   │
│  │  └── Keyframe system                                                │   │
│  │                                                                      │   │
│  │  KDENLIVE (KDE/kdenlive) — Mature NLE in Qt/C++                     │   │
│  │  ├── Track header design                                            │   │
│  │  ├── Effect stack UI                                                │   │
│  │  ├── Transition picker                                              │   │
│  │  └── Color scope widgets                                            │   │
│  │                                                                      │   │
│  │  NATRON (NatronGitHub/Natron) — Node compositor in Qt               │   │
│  │  ├── Node graph interaction                                         │   │
│  │  ├── Curve editor (bezier handles)                                  │   │
│  │  └── Parameter inspector                                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  TIER 3: BUILD CUSTOM (no good reference exists)                           │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • AI Chat Panel — novel interaction model                          │   │
│  │  • AI Suggestion Cards — our unique feature                         │   │
│  │  • Rive State Machine Inspector — custom for Rive                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.6 Widget Source Matrix

| Widget | Source | License | Approach |
|--------|--------|---------|----------|
| **Docking** | `egui_tiles` | MIT | Direct use |
| **File Browser** | `egui-file-dialog` | MIT | Direct use |
| **Node Editor** | `egui_node_graph` | MIT | Direct use |
| **Curve Plot** | `egui_plot` | MIT | Use as base, extend |
| **Timeline** | Study Olive/Kdenlive | GPL (concepts) | Clean room implementation |
| **Track Headers** | Study Kdenlive | GPL (concepts) | Clean room implementation |
| **Clip Thumbnails** | Study Olive | GPL (concepts) | Clean room implementation |
| **Transition Picker** | Study Kdenlive | GPL (concepts) | Clean room implementation |
| **Color Wheels** | Study DaVinci (concepts) | N/A | Clean room implementation |
| **Curves Editor** | Study Natron | GPL (concepts) | Clean room, use egui_plot base |
| **Waveform/Scopes** | Study Kdenlive | GPL (concepts) | Clean room + GPU compute |
| **Effect Inspector** | Study Kdenlive/Natron | GPL (concepts) | Clean room implementation |
| **AI Chat Panel** | Novel | — | Build from scratch |
| **Suggestion Cards** | Novel | — | Build from scratch |

### 3.7 Study Sources (GPL — Concepts Only)

> [!WARNING]
> These are GPL. **Study the architecture and algorithms, never copy code.**

#### Olive Editor (Most Modern NLE Architecture)
```
olive-editor/olive/
├── app/
│   ├── widget/
│   │   ├── timelinewidget/     ← STUDY: Track/clip rendering
│   │   │   ├── timelinewidget.cpp
│   │   │   ├── clipblock.cpp    ← How clips are rendered
│   │   │   └── transitionblock.cpp
│   │   ├── nodeparam/          ← STUDY: Parameter UI
│   │   └── curvewidget/        ← STUDY: Keyframe curves
│   └── node/                   ← STUDY: Node architecture
```

#### Kdenlive (Mature Feature Set)
```
KDE/kdenlive/
├── src/
│   ├── timeline2/
│   │   ├── view/               ← STUDY: Timeline rendering
│   │   │   ├── qml/            ← Track/clip QML
│   │   │   └── clipitem.cpp
│   ├── effects/                ← STUDY: Effect stack UI
│   ├── scopes/                 ← STUDY: Waveform, histogram
│   └── colortools/             ← STUDY: Color picker widgets
```

#### Natron (Node Compositor)
```
NatronGitHub/Natron/
├── Gui/
│   ├── NodeGraph.cpp           ← STUDY: Node graph interaction
│   ├── CurveWidget.cpp         ← STUDY: Bezier curve editing
│   └── DopeSheet.cpp           ← STUDY: Keyframe overview
```

### 3.8 UI Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        UI ARCHITECTURE                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      egui Application Layer                         │   │
│  │  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐           │   │
│  │  │  Menu Bar │ │  Toolbar  │ │  Panels   │ │  Dialogs  │           │   │
│  │  └───────────┘ └───────────┘ └───────────┘ └───────────┘           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │              Custom Widgets (strawberry-ui crate)                   │   │
│  │                                                                      │   │
│  │  Ported from egui crates:     Inspired by Qt NLEs:                  │   │
│  │  ┌─────────────┐              ┌─────────────┐ ┌─────────────┐       │   │
│  │  │ egui_tiles  │              │  Timeline   │ │  Color      │       │   │
│  │  │ (docking)   │              │  (Olive)    │ │  (DaVinci)  │       │   │
│  │  └─────────────┘              └─────────────┘ └─────────────┘       │   │
│  │  ┌─────────────┐              ┌─────────────┐ ┌─────────────┐       │   │
│  │  │egui_node_   │              │  Curves     │ │  Scopes     │       │   │
│  │  │graph        │              │  (Natron)   │ │  (Kdenlive) │       │   │
│  │  └─────────────┘              └─────────────┘ └─────────────┘       │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                 Viewport (pure wgpu, egui overlay)                  │   │
│  │      Direct GPU rendering: video, effects, 3D, vector graphics      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.9 Time Savings Estimate

| Widget | Build from scratch | With reuse strategy |
|--------|-------------------|---------------------|
| Docking | 2 weeks | **0 (egui_tiles)** |
| File Browser | 1 week | **0 (egui-file-dialog)** |
| Node Editor | 3 weeks | **0 (egui_node_graph)** |
| Timeline | 6 weeks | **3 weeks (study Olive + build)** |
| Curves | 2 weeks | **1 week (egui_plot + extend)** |
| Color Wheels | 2 weeks | **1.5 weeks (study DaVinci)** |
| Scopes | 2 weeks | **1 week (study Kdenlive)** |
| **Total** | **18 weeks** | **~6.5 weeks** |

### 3.10 Custom Widget Roadmap

| Widget | Complexity | Phase | Source Strategy |
|--------|------------|-------|-----------------|
| **Viewport** | High | 0 | wgpu + egui overlay |
| **Playback Controls** | Low | 0 | egui built-ins |
| **Panel Docking** | Low | 0 | `egui_tiles` (MIT) |
| **Timeline** | Very High | 1 | Study Olive → clean room |
| **Track Headers** | Medium | 1 | Study Kdenlive → clean room |
| **Asset Browser** | Medium | 1 | `egui-file-dialog` + custom grid |
| **Transition Picker** | Medium | 1 | Study Kdenlive → clean room |
| **Effect Stack** | Medium | 2 | Study Kdenlive → clean room |
| **Effect Inspector** | Medium | 2 | Study Natron → clean room |
| **Color Wheels** | High | 2 | Study DaVinci → clean room |
| **RGB Curves** | High | 2 | `egui_plot` + Natron concepts |
| **Histogram** | Medium | 2 | GPU compute → egui texture |
| **Waveform/Vectorscope** | High | 2 | Study Kdenlive → GPU compute |
| **Graph Editor** | Very High | 3 | `egui_plot` + Natron concepts |
| **Node Editor** | High | 3 | `egui_node_graph` (MIT) |
| **Text Editor** | Medium | 3 | egui built-ins + cosmic-text |
| **Template Browser** | Medium | 3 | Custom grid |
| **AI Chat Panel** | Medium | 4 | Novel — build from scratch |
| **Suggestion Cards** | Low | 4 | Novel — build from scratch |

---

## 4. Phase 0: Foundation (Months 1-3)

### 4.0 Phase Goal
**Deliverable:** A window that displays a video frame with basic playback controls.  
**AI Capability:** AI can query project state and control playback.
**UI Deliverable:** Window, viewport, playback controls, panel layout skeleton.

### 4.1 Week-by-Week Breakdown

#### Weeks 1-2: Project Bootstrap + UI Shell

**Core Tasks:**
- [x] Initialize Cargo workspace structure ✅
- [ ] Set up CI/CD (GitHub Actions)
- [ ] Configure linting (clippy), formatting (rustfmt)
- [ ] Create feature flag system
- [x] Set up logging (tracing) ✅

**UI Tasks:**
- [x] Set up eframe + egui with wgpu backend ✅
- [x] Create custom dark theme (`strawberry_theme`) ✅
- [x] Implement basic panel layout (top bar, left panel, center, right panel, bottom) ✅
- [x] Add menu bar skeleton (File, Edit, View, Help) ✅
- [x] Create docking/panel resize system ✅ — `strawberry-app/src/docking.rs` with egui_tiles

**Dependencies for this phase:**
```toml
# Core
bevy_ecs = "0.15"
bevy_app = "0.15"
bevy_tasks = "0.15"

# UI
egui = "0.29"
eframe = "0.29"
egui-wgpu = "0.29"

# Graphics
wgpu = "23"
```

**Directory Structure:**
```
project-strawberry/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── strawberry-core/          # ECS, Commands, Events
│   ├── strawberry-video/         # Video decode/encode
│   ├── strawberry-audio/         # Audio engine
│   ├── strawberry-render/        # GPU rendering
│   ├── strawberry-ui/            # UI components ← egui widgets here
│   ├── strawberry-ai/            # AI integration
│   └── strawberry-app/           # Main application
├── assets/                       # Test assets
├── shaders/                      # WGSL shaders
└── docs/                         # Documentation
```

**UI Layout (Target for Week 2):**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│  File   Edit   View   Help                              Project: Untitled   │
├───────────────┬─────────────────────────────────────────┬───────────────────┤
│               │                                         │                   │
│   Asset       │               Viewport                  │    Inspector      │
│   Browser     │         (empty placeholder)             │    Panel          │
│               │                                         │                   │
│               │                                         │                   │
├───────────────┴─────────────────────────────────────────┴───────────────────┤
│                            Timeline (placeholder)                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

**UI Milestone:** Window with panel layout, menu bar, and dark theme renders.

---

#### Weeks 3-4: ECS Core + Command System

**Tasks:**
- [x] Implement `Command` trait ✅
- [x] Create `CommandQueue` resource ✅
- [x] Implement `UndoStack` with redo support ✅
- [x] Create `CommandRegistry` for AI discovery ✅
- [x] Build first command: `CreateProject` ✅
- [x] Build playback commands: `Play`, `Pause`, `Seek`, `SetSpeed` ✅

**Components:**
```rust
// Core project components
#[derive(Component)]
pub struct Project {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

#[derive(Component)]
pub struct ProjectSettings {
    pub resolution: UVec2,
    pub frame_rate: FrameRate,
    pub color_space: ColorSpace,
}
```

**First Command:**
```rust
pub struct CreateProjectCommand {
    pub name: String,
    pub resolution: UVec2,
    pub frame_rate: FrameRate,
}

impl Command for CreateProjectCommand {
    fn execute(&self, world: &mut World) -> Result<UndoData, CommandError> {
        let entity = world.spawn((
            Project { 
                name: self.name.clone(),
                created_at: Utc::now(),
                modified_at: Utc::now(),
            },
            ProjectSettings {
                resolution: self.resolution,
                frame_rate: self.frame_rate,
                color_space: ColorSpace::Srgb,
            },
        )).id();
        
        Ok(UndoData::EntityCreated(entity))
    }
    // ...
}
```

**UI Milestone:** "New Project" dialog → creates project → shows in title bar.

---

#### Weeks 5-6: Video Decode Service + Viewport Widget

**Dependencies:**
- FFmpeg CLI subprocess (via `which` crate for discovery)

**Core Tasks:**
- [x] Create `VideoDecodeService` (not ECS, service layer) ✅ — `strawberry-video/src/service.rs`
- [x] Implement async frame decoding (sync for Phase 0, async) ✅
- [x] Create frame cache with LRU eviction ✅ — `strawberry-video/src/cache.rs`
- [x] Handle seek with keyframe alignment ✅ — FFmpeg `-ss` flag
- [x] Create `VideoAsset` component ✅
- [x] Create `VideoFrame` decode output struct ✅
- [x] Create `VideoMetadata` with ffprobe JSON parsing ✅

**UI Tasks:**
- [x] Create `ViewportWidget` (custom egui widget) ✅ — `strawberry-ui/src/widgets/viewport.rs`
- [x] Implement wgpu texture render to egui ✅ — `TextureManager` in `strawberry-render`
- [x] Add drag-and-drop file import ✅ — with visual drop zone
- [x] Show loading indicator during decode ✅ — spinner animation
- [x] Display video metadata in status bar ✅ — resolution, fps, duration, codec
- [x] Create `ViewportState` resource ✅ — zoom, pan, aspect ratio
- [x] Create `ViewportSystem` ✅ — syncs playhead to video service

**Components:**
```rust
#[derive(Component)]
pub struct VideoAsset {
    pub path: PathBuf,
    pub duration: Duration,
    pub frame_count: u64,
    pub frame_rate: FrameRate,
    pub resolution: UVec2,
    pub codec: String,
}

#[derive(Component)]
pub struct VideoFrame {
    pub frame_number: u64,
    pub texture_id: TextureId,  // GPU texture handle
    pub timestamp: Duration,
}
```

**UI Milestone:** Drag video file → displays first frame in viewport with resolution info. ✅ **ACHIEVED**

---

#### Weeks 7-8: Basic Render Pipeline

**Dependencies:**
- `wgpu` (MIT) — GPU abstraction
- `vello` (MIT) — 2D rendering

**Tasks:**
- [x] Create `RenderGraph` structure ✅ — `strawberry-render/src/graph.rs` with RenderNode trait
- [x] Implement `ViewportNode` for preview ✅ — VideoDecodeNode + TransformNode in graph.rs
- [x] Create `CompositeNode` for layer blending ✅ — CompositeNode in graph.rs (Phase 0: passthrough)
- [x] Build texture upload from video frames ✅ — `TextureManager`
- [x] Implement basic transform (position, scale, rotation) ✅ — `Transform2D` component + 5 commands

**Render Graph:**
```
┌─────────────────────────────────────────┐
│              Render Graph               │
├─────────────────────────────────────────┤
│                                         │
│  [VideoDecodeNode] ──► [TransformNode]  │
│                              │          │
│                              ▼          │
│  [UIOverlayNode] ──────► [CompositeNode]│
│                              │          │
│                              ▼          │
│                        [PresentNode]    │
│                                         │
└─────────────────────────────────────────┘
```

**UI Milestone:** Video frame renders in viewport with transform controls visible.

---

#### Weeks 9-10: Playback Engine + Transport Controls

**Core Tasks:**
- [x] Create `Playhead` resource ✅ — `strawberry-core/src/resources/mod.rs`
- [x] Implement `PlaybackSystem` ✅ — `playhead_advance_system` in `strawberry-core/src/systems/playback.rs`
- [x] Handle frame timing with vsync ✅ — `PlaybackClock` resource with delta tracking
- [x] Create playback commands: Play, Pause, Seek, SetSpeed ✅ — `strawberry-core/src/commands/playback.rs`

**UI Tasks:**
- [x] Create `TransportBar` widget (play, pause, stop, prev frame, next frame) ✅ — basic controls in `strawberry-app/src/main.rs`
- [x] Create `Scrubber` widget (seekable progress bar) ✅ — timeline scrubber implemented
- [x] Add timecode display (HH:MM:SS:FF) ✅ — in playback controls
- [ ] Add playback speed dropdown (0.25x, 0.5x, 1x, 2x, 4x)
- [x] Add keyboard shortcuts (Space = play/pause, J/K/L = reverse/pause/forward) ✅ — Space for play/pause

**Resources:**
```rust
#[derive(Resource)]
pub struct Playhead {
    pub current_frame: u64,
    pub current_time: Duration,
    pub is_playing: bool,
    pub playback_speed: f32,
    pub loop_mode: LoopMode,
}

#[derive(Resource)]
pub struct PlaybackState {
    pub buffered_frames: RangeInclusive<u64>,
    pub decode_ahead: u64,
}
```

**Commands:**
```rust
pub struct PlayCommand;
pub struct PauseCommand;
pub struct SeekCommand { pub frame: u64 }
pub struct SetSpeedCommand { pub speed: f32 }
```

**UI Milestone:** Play/Pause button works. Scrubber shows position. Keyboard shortcuts work. Framerate stable.

---

#### Weeks 11-12: Audio Playback

**Dependencies:**
- `cpal` (Apache 2.0) — Audio I/O ✅ — using `cpal` for output
- FFmpeg subprocess for audio decode (consistent with video architecture) ✅

**Tasks:**
- [x] Create `AudioPlaybackService` ✅ — `strawberry-audio/src/playback.rs`
- [x] Sync audio to video playhead ✅ — `audio_sync_system` syncs to playhead, 30ms drift detection
- [x] Handle sample rate conversion ✅ — via FFmpeg (`-ar` flag)
- [x] Create `AudioAsset` component ✅ — `AudioMetadata` struct with sample_rate, channels, duration
- [x] Implement volume control command ✅ — `set_volume()` method with atomic storage

**Components:**
```rust
#[derive(Component)]
pub struct AudioAsset {
    pub path: PathBuf,
    pub duration: Duration,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Component)]
pub struct AudioClip {
    pub asset: Entity,  // Reference to AudioAsset
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
}
```

**UI Milestone:** Video plays with synced audio. Volume slider works.

---

### 4.2 Phase 0 Exit Criteria

| Criterion | Validation | Status |
|-----------|------------|--------|
| Video plays in viewport | Manual test | ✅ **DONE** — NVDEC hw accel, backpressure buffer, timestamp sync |
| Audio synced to video | < 50ms drift test | ✅ **DONE** — 30ms drift detection, auto-resync |
| Play/Pause/Seek work | Automated UI test | ✅ **DONE** — Play, Pause, Seek commands functional |
| Commands are undoable | Undo test | ✅ **DONE** (17 tests passing) |
| AI can query state | API test: `GET /project/state` | ✅ **DONE** (4 tests passing) |
| AI can control playback | API test: `POST /command/seek` | ✅ **DONE** |
| 60 FPS sustained | Performance benchmark | ✅ **DONE** — maintained with buffer backpressure |
| Memory stable | No leaks over 1 hour playback | ⚠️ Not yet tested |

---

## 5. Phase 1: Core Editing (Months 4-6)

### 5.0 Phase Goal
**Deliverable:** Full timeline with multi-track editing, trim, split, transitions.  
**AI Capability:** AI can assemble rough cuts, split clips, add transitions.
**UI Deliverable:** Timeline widget, track headers, asset browser, transition picker.

### 5.1 Week-by-Week Breakdown

#### Weeks 1-2: Timeline Data Model + Timeline Widget Foundation ✅ DONE

**UI Tasks (Critical - This is the heart of the editor):**
- [x] Create `TimelineWidget` (custom egui widget) ✅ `strawberry-ui/src/widgets/timeline.rs`
- [x] Implement ruler with timecode/frames toggle ✅
- [x] Create track lanes with vertical scrolling ✅
- [x] Add horizontal zoom (mouse wheel) ✅ MIN_ZOOM=5, MAX_ZOOM=500
- [x] Implement playhead indicator line ✅

**ECS Tasks:**
- [x] Create `Timeline`, `Track`, `Clip` components ✅ `strawberry-core/src/components/timeline.rs`
- [x] Create timeline commands ✅ `strawberry-core/src/commands/timeline.rs`
  - [x] CreateTimelineCommand, CreateTrackCommand, AddClipCommand
  - [x] MoveClipCommand (with from_bits constructor)
  - [x] DeleteClipCommand

**Components:**
```rust
#[derive(Component)]
pub struct Timeline {
    pub name: String,
    pub duration: Duration,
}

#[derive(Component)]
pub struct Track {
    pub index: u32,
    pub track_type: TrackType,
    pub name: String,
    pub locked: bool,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Component)]
pub struct Clip {
    pub track: Entity,          // Parent track
    pub source_asset: Entity,   // Video/Audio/Image asset
    pub source_in: Duration,    // In point in source
    pub source_out: Duration,   // Out point in source
    pub timeline_start: Duration, // Position on timeline
}

#[derive(Component)]
pub struct ClipTransform {
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    pub anchor: Vec2,
    pub opacity: f32,
}
```

**Commands:**
```rust
pub struct CreateTrackCommand { pub track_type: TrackType, pub name: String }
pub struct AddClipCommand { pub track: Entity, pub asset: Entity, pub at: Duration }
pub struct MoveClipCommand { pub clip: Entity, pub to_track: Entity, pub at: Duration }
pub struct DeleteClipCommand { pub clip: Entity }
```

**UI Milestone:** Timeline panel shows tracks. Can add empty tracks.

---

#### Weeks 3-4: Timeline UI Widget (Study Olive/Kdenlive) ✅ DONE

**Study References:**
- `olive-editor/olive/app/widget/timelinewidget/` — Clip rendering, track layout ✅ Studied
- `KDE/kdenlive/src/timeline2/view/` — Track headers, zoom behavior ✅ Studied

**UI Tasks:**
- [x] Build timeline ruler (frames/timecode toggle) ✅
- [x] Create `TrackHeader` widget (name, mute, solo, lock, color) ✅ M/S/L buttons implemented
- [x] Implement `ClipBlock` widget with thumbnails ✅
- [x] Add clip selection (single, multi with Shift/Ctrl) ✅ SelectClipCommand
- [x] Implement drag-to-move clips ✅ ClipDragState + ghost preview + MoveClipCommand
- [x] Add playhead dragging ✅ Click-to-seek
- [x] Implement zoom with Ctrl+scroll ✅ MIN_ZOOM=5, MAX_ZOOM=500

**New Commands Created:**
- [x] SelectClipCommand (exclusive/additive) ✅ `strawberry-core/src/commands/selection.rs`
- [x] ClearSelectionCommand, ToggleSelectionCommand ✅
- [x] SetTrackMutedCommand, SetTrackSoloCommand, SetTrackLockedCommand ✅ `strawberry-core/src/commands/track.rs`

**UI Features:**
```
┌─────────────────────────────────────────────────────────────────┐
│ 00:00:00:00    00:00:05:00    00:00:10:00    00:00:15:00        │ ← Ruler
├─────────────────────────────────────────────────────────────────┤
│ V1 [M][S][L]  │▓▓▓▓▓▓▓▓▓▓▓▓▓│         │▓▓▓▓▓▓▓▓▓│              │ ← Video Track
├─────────────────────────────────────────────────────────────────┤
│ V2 [M][S][L]  │    │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│                        │ ← Video Track
├─────────────────────────────────────────────────────────────────┤
│ A1 [M][S][L]  │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│            │ ← Audio Track
├─────────────────────────────────────────────────────────────────┤
│                              ▼                                   │ ← Playhead
└─────────────────────────────────────────────────────────────────┘
```

**UI Milestone:** Full timeline with clips, thumbnails, drag/select. Looks professional. ✅ ACHIEVED

---

#### Weeks 5-6: Clip Editing Operations

**Status: ✅ COMPLETE**


**Commands (Implemented ✅):**
```rust
// TrimClipCommand - IMPLEMENTED with UUID support
pub struct TrimClipCommand { 
    pub clip: Entity, // Legacy, uses clip_uuid internally
    pub side: TrimSide, 
    pub new_value: Duration,
    // ... clip_uuid: Option<Uuid>
}

// SplitClipCommand - IMPLEMENTED with UUID support
pub struct SplitClipCommand { 
    pub clip: Entity,
    pub at: Duration,
    // ... clip_uuid, created_clip_uuid
}

// RippleDeleteCommand - IMPLEMENTED
pub struct RippleDeleteCommand { 
    pub clip: Entity,
    // ... stored_uuid
}

// SlipClipCommand - IMPLEMENTED (UUID-aware)
pub struct SlipClipCommand { 
    pub clip: Entity, 
    pub offset: Duration,
    // ... clip_uuid
}

// SlideClipCommand - IMPLEMENTED (UUID-aware)
pub struct SlideClipCommand { 
    pub clip: Entity, 
    pub delta: f64,
    // ... clip_uuid, previous_clip_uuid, next_clip_uuid
}
```

**UI Features (Implemented ✅):**
- CapCut-style centered playhead: Playhead fixed at center, clips scroll during scrub/playback
- Visible trim handles on clip edges with ResizeHorizontal cursor
- **Ripple Delete**: Context menu option + shortcuts (`Alt+Del`, `Shift+Bksp`)
- DragOrigin tracking: distinguishes Ruler, Empty, ClipBody, TrimEdge drags
- Auto-scroll during move/trim: timeline scrolls when dragging near edges (120px zone)
- Frame-snapped smooth scrolling for mousewheel/trackpad
- Scrub from anywhere: drag on ruler, empty area, or clips to scrub

**UI Milestone:** Drag clip edges shows trim handles. Split at playhead works. ✅ ACHIEVED (UI side)

---

#### Weeks 7-8: Multi-track Compositing

**Tasks:**
- [ ] Implement layer ordering (z-index from track order)
- [ ] Create per-clip transform controls
- [ ] Add opacity blending
- [ ] Implement solo/mute functionality

**Render Graph Update:**
```
[Track N Clips] ──► [Transform] ──┐
                                  │
[Track N-1 Clips] ► [Transform] ──┼──► [Composite] ──► [Output]
                                  │
[Track 1 Clips] ──► [Transform] ──┘
```

**UI Milestone:** Multiple video layers composite. Transform handles in viewport.

---

#### Weeks 9-10: Transitions System

**Dependencies:**
- Shaders from Lygia (MIT) for transition math

**Components:**
```rust
#[derive(Component)]
pub struct Transition {
    pub clip_a: Entity,
    pub clip_b: Entity,
    pub transition_type: TransitionType,
    pub duration: Duration,
    pub easing: EasingType,
}

pub enum TransitionType {
    Dissolve,
    Wipe { direction: Direction, softness: f32 },
    Push { direction: Direction },
    Zoom { zoom_in: bool },
    // ... more
}
```

**Shaders (WGSL):**
```
shaders/
├── transitions/
│   ├── dissolve.wgsl
│   ├── wipe.wgsl
│   ├── push.wgsl
│   ├── zoom.wgsl
│   ├── iris.wgsl
│   └── ... (20+ transition shaders)
```

**Commands:**
```rust
pub struct AddTransitionCommand { 
    pub between: (Entity, Entity), 
    pub transition_type: TransitionType,
    pub duration: Duration,
}
pub struct ChangeTransitionTypeCommand { pub transition: Entity, pub new_type: TransitionType }
pub struct RemoveTransitionCommand { pub transition: Entity }
```

**UI Milestone:** Drag transition from library to clip boundary. Preview in real-time.

---

#### Weeks 11-12: Import/Export Foundation

**Tasks:**
- [ ] Project save/load (RON or bincode format)
- [ ] Asset import (video, audio, images)
- [ ] Basic export (single codec, fixed settings)
- [ ] Progress reporting for long operations

**Project Format:**
```rust
#[derive(Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: Version,
    pub settings: ProjectSettings,
    pub assets: Vec<AssetReference>,
    pub timeline: TimelineData,
    pub command_history: Vec<SerializedCommand>,  // For AI context
}
```

**UI Milestone:** Save/Load project. Export to MP4.

---

### 5.2 Phase 1 Exit Criteria

| Criterion | Validation |
|-----------|------------|
| Multi-track timeline functional | 5 video + 5 audio tracks test |
| Trim/Split/Delete work | Automated edit test |
| Transitions render correctly | Visual regression test |
| Ripple edit works | Automated test |
| Save/Load project | Round-trip test |
| Export produces valid video | FFprobe validation |
| AI can add clips to timeline | API test |
| AI can create rough cut | End-to-end test |

---

## 6. Phase 2: Effects & Color (Months 7-9)

### 6.0 Phase Goal
**Deliverable:** Full effect stack, color grading, GPU-accelerated processing.  
**AI Capability:** AI can apply effects, suggest color grades, auto-correct.
**UI Deliverable:** Effect stack panel, color wheels, curves editor, histogram/waveform scopes.

### 6.1 Week-by-Week Breakdown

#### Weeks 1-2: Effect System Architecture + Effect Stack UI

**Study References:**
- `KDE/kdenlive/src/effects/` — Effect stack drag/reorder
- `NatronGitHub/Natron/Gui/DockablePanel.cpp` — Effect parameter inspector

**Core Tasks:**
- [ ] Implement `EffectStack` component
- [ ] Create `EffectProcessSystem`
- [ ] Build GPU shader dispatch

**UI Tasks:**
- [ ] Create `EffectStackPanel` (reorderable list)
- [ ] Add enable/disable toggle per effect
- [ ] Implement drag-to-reorder effects
- [ ] Create "Add Effect" browser popup
- [ ] Add effect parameter inspector

**Components:**
```rust
#[derive(Component)]
pub struct EffectStack {
    pub effects: Vec<Entity>,  // Ordered list of effect entities
    pub enabled: bool,
}

#[derive(Component)]
pub struct Effect {
    pub effect_type: EffectType,
    pub enabled: bool,
}

// Each effect type has its own component
#[derive(Component)]
pub struct BlurEffect {
    pub radius: f32,
    pub quality: BlurQuality,
}

#[derive(Component)]
pub struct ColorAdjustEffect {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
}
```

**Effect Processing System:**
```rust
fn effect_process_system(
    effects: Query<(&EffectStack, &Children)>,
    blur_effects: Query<&BlurEffect>,
    color_effects: Query<&ColorAdjustEffect>,
    gpu: Res<GpuContext>,
) {
    for (stack, children) in effects.iter() {
        if !stack.enabled { continue; }
        
        let mut current_texture = input_texture;
        
        for &effect_entity in &stack.effects {
            if let Ok(blur) = blur_effects.get(effect_entity) {
                current_texture = gpu.dispatch_blur(current_texture, blur);
            }
            if let Ok(color) = color_effects.get(effect_entity) {
                current_texture = gpu.dispatch_color_adjust(current_texture, color);
            }
            // ... more effect types
        }
    }
}
```

**UI Milestone:** Inspector shows effect stack. Can add/remove/reorder effects.

---

#### Weeks 3-4: Effect Library (40+ Effects)

**Shader Sources:**
- Lygia (MIT) — blur, color, distortion
- Shadertoy (CC0/MIT) — creative effects
- GPU Gems (public algorithms)

**Effect Categories:**

| Category | Effects | Shader Count |
|----------|---------|--------------|
| Blur | Gaussian, Box, Motion, Radial, Zoom, Tilt-Shift | 6 |
| Sharpen | Unsharp Mask, High Pass, Clarity | 3 |
| Color | Brightness, Contrast, Saturation, Vibrance, Exposure | 5 |
| Stylize | Posterize, Threshold, Halftone, Mosaic, Pixelate | 5 |
| Distort | Wave, Ripple, Twirl, Spherize, Bulge, Displacement | 6 |
| Light | Glow, Bloom, Lens Flare, Vignette, Light Rays | 5 |
| Noise | Film Grain, Add Noise, Denoise | 3 |
| Keying | Chroma Key, Luma Key | 2 |
| Time | Echo, Motion Trail | 2 |
| Artistic | Glitch, VHS, CRT, Duotone | 4 |

**UI Milestone:** Effect browser panel. Drag effect to clip. Real-time preview.

---

#### Weeks 5-6: Color Grading System (Study DaVinci/Kdenlive)

**Study References:**
- DaVinci Resolve tutorials — Color wheel interaction patterns
- `KDE/kdenlive/src/colortools/` — Color picker widgets
- `KDE/kdenlive/src/scopes/` — Histogram, waveform, vectorscope

**Core Tasks:**
- [ ] Implement color wheel shader
- [ ] Create curves evaluation system
- [ ] Build LUT loader (3D texture)

**UI Tasks:**
- [ ] Create `ColorWheelWidget` (lift/gamma/gain/offset)
- [ ] Build `CurvesEditor` widget (use `egui_plot` as base)
- [ ] Add `HistogramWidget` (GPU compute → egui texture)
- [ ] Add `WaveformWidget` (GPU compute)
- [ ] Add `VectorscopeWidget` (GPU compute)
- [ ] Create LUT browser with previews

**Components:**
```rust
#[derive(Component)]
pub struct ColorWheels {
    pub lift: Vec3,    // Shadows
    pub gamma: Vec3,   // Midtones
    pub gain: Vec3,    // Highlights
    pub offset: Vec3,  // Overall
}

#[derive(Component)]
pub struct CurvesAdjust {
    pub master: AnimationCurve,
    pub red: AnimationCurve,
    pub green: AnimationCurve,
    pub blue: AnimationCurve,
}

#[derive(Component)]
pub struct LUTReference {
    pub lut_asset: Entity,
    pub intensity: f32,
}

#[derive(Component)]
pub struct HSLAdjust {
    pub hue_shift: f32,
    pub saturation: f32,
    pub lightness: f32,
}
```

**UI Milestone:** Professional color panel with wheels, curves, scopes. LUT browser works.

---

#### Weeks 7-8: GPU Compute Pipeline

**Tasks:**
- [ ] Optimize effect chain to minimize GPU passes
- [ ] Implement effect fusion (combine simple effects into one shader)
- [ ] Add GPU memory management
- [ ] Create shader hot-reload for development
- [ ] Profile and optimize bottlenecks

**Optimizations:**
```rust
// Effect fusion: combine multiple simple color ops into one shader
fn optimize_effect_chain(effects: &[Effect]) -> OptimizedChain {
    let mut fusable_color_ops = Vec::new();
    let mut chain = Vec::new();
    
    for effect in effects {
        if effect.is_simple_color_op() && fusable_color_ops.len() < 8 {
            fusable_color_ops.push(effect.clone());
        } else {
            if !fusable_color_ops.is_empty() {
                chain.push(OptimizedEffect::FusedColor(fusable_color_ops.clone()));
                fusable_color_ops.clear();
            }
            chain.push(OptimizedEffect::Single(effect.clone()));
        }
    }
    
    OptimizedChain { effects: chain }
}
```

**UI Milestone:** Effects maintain 60fps with 5+ effects on 4K.

---

#### Weeks 9-10: Masks & Qualifiers

**Components:**
```rust
#[derive(Component)]
pub struct Mask {
    pub path: BezierPath,  // Vector mask
    pub feather: f32,
    pub invert: bool,
    pub expansion: f32,
}

#[derive(Component)]
pub struct QualifierMask {
    pub hue_range: RangeInclusive<f32>,
    pub saturation_range: RangeInclusive<f32>,
    pub luminance_range: RangeInclusive<f32>,
    pub softness: f32,
}

#[derive(Component)]
pub struct EffectMask {
    pub mask_type: MaskType,
    pub mask_entity: Entity,
}
```

**UI Milestone:** Draw vector masks on viewport. HSL qualifier picker. Mask preview mode.

---

#### Weeks 11-12: AI Color Suggestions

**Dependencies:**
- `ort` (MIT) — ONNX inference
- Color analysis models

**Features:**
- Auto white balance
- Auto exposure
- Match color between shots
- Suggest LUT based on mood
- Scene-based color correction

**AI Commands:**
```rust
pub struct AutoWhiteBalanceCommand { pub clip: Entity }
pub struct MatchColorCommand { pub source: Entity, pub target: Entity }
pub struct SuggestLUTCommand { pub clip: Entity, pub mood: String }
```

**UI Milestone:** "Auto Color" button. "Match Color" tool. AI color suggestions panel.

---

### 6.2 Phase 2 Exit Criteria

| Criterion | Validation |
|-----------|------------|
| 40+ effects available | Effect library test |
| Color wheels functional | Manual grading test |
| LUTs apply correctly | Visual comparison |
| GPU performance stable | 4K 60fps with effects |
| Masks work | Draw/animate mask test |
| AI color match works | Side-by-side test |

---

## 7. Phase 3: Animation & Motion (Months 10-12)

### 7.0 Phase Goal
**Deliverable:** Full keyframe animation, motion graphics, text animation, Rive integration.  
**AI Capability:** AI can create motion graphics, animate text, control Rive state machines.
**UI Deliverable:** Graph editor (keyframe curves), text editor, template browser, Rive inspector.

### 7.1 Week-by-Week Breakdown

#### Weeks 1-2: Keyframe System + Graph Editor Foundation

**Study References:**
- `NatronGitHub/Natron/Gui/CurveWidget.cpp` — Bezier curve editing with handles
- `olive-editor/olive/app/widget/curvewidget/` — Keyframe curve interaction
- `NatronGitHub/Natron/Gui/DopeSheet.cpp` — Keyframe overview

**Core Tasks:**
- [ ] Implement `AnimationCurve` with bezier support
- [ ] Create `KeyframeEvalSystem`
- [ ] Add easing presets

**UI Tasks:**
- [ ] Create `GraphEditorWidget` (use `egui_plot` as base)
- [ ] Add keyframe handles with bezier drag
- [ ] Implement multi-curve display
- [ ] Add keyframe selection/multi-select
- [ ] Create easing preset dropdown
- [ ] Add "Add Keyframe" button on inspectable properties

**Dependencies:**
- `splines` (BSD) — Curve evaluation
- `ezing` (MIT) — Easing functions

**Components:**
```rust
#[derive(Component)]
pub struct Animatable {
    pub property_path: String,  // e.g., "transform.position.x"
}

#[derive(Component)]
pub struct AnimationCurves {
    pub curves: HashMap<String, AnimationCurve>,
}

pub struct AnimationCurve {
    pub keyframes: Vec<Keyframe>,
    pub interpolation: InterpolationType,
}

pub struct Keyframe {
    pub time: Duration,
    pub value: f32,
    pub in_tangent: Vec2,
    pub out_tangent: Vec2,
    pub easing: EasingType,
}
```

**Systems:**
```rust
fn keyframe_evaluation_system(
    time: Res<Playhead>,
    mut animated: Query<(&AnimationCurves, &mut ClipTransform)>,
) {
    for (curves, mut transform) in animated.iter_mut() {
        if let Some(curve) = curves.get("position.x") {
            transform.position.x = curve.evaluate(time.current_time);
        }
        // ... other properties
    }
}
```

**UI Milestone:** Keyframe indicator on timeline. Add keyframe button. Interpolation menu.

---

#### Weeks 3-4: Graph Editor

**UI Features:**
- Curve display with bezier handles
- Multi-curve editing
- Keyframe selection/manipulation
- Value graph vs. speed graph
- Presets (bounce, elastic, etc.)

```
┌─────────────────────────────────────────────────────────────────┐
│ Graph Editor                                            [×]     │
├─────────────────────────────────────────────────────────────────┤
│  │                                              ●───────        │
│  │                              ●──────────●                    │
│  │               ●─────────────                                 │
│  │  ●───────────                                                │
│  ├──────────────────────────────────────────────────────────────│
│  └─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┼───────  │
│      00:00  00:01  00:02  00:03  00:04  00:05  00:06  00:07      │
├─────────────────────────────────────────────────────────────────┤
│ [Position X] [Position Y] [Scale] [Rotation] [Opacity]         │
└─────────────────────────────────────────────────────────────────┘
```

**UI Milestone:** Graph editor opens. Can manipulate curves. Preview updates in real-time.

---

#### Weeks 5-6: Text System

**Dependencies:**
- `cosmic-text` (MIT) — Text layout and shaping

**Components:**
```rust
#[derive(Component)]
pub struct TextLayer {
    pub content: String,
    pub style: TextStyle,
}

#[derive(Component)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub color: Color,
    pub stroke: Option<StrokeStyle>,
    pub shadow: Option<ShadowStyle>,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub alignment: TextAlignment,
}

#[derive(Component)]
pub struct TextAnimation {
    pub animation_type: TextAnimationType,
    pub per_character: bool,
    pub stagger: Duration,
}

pub enum TextAnimationType {
    TypeWriter,
    FadeIn,
    SlideIn { direction: Direction },
    Scale,
    Blur,
    Custom { keyframes: AnimationCurves },
}
```

**UI Milestone:** Text tool. Font selector. Text animations preview. Rich text editing.

---

#### Weeks 7-8: Motion Graphics Templates

**Components:**
```rust
#[derive(Component)]
pub struct Template {
    pub name: String,
    pub category: TemplateCategory,
    pub thumbnail: TextureId,
}

#[derive(Component)]
pub struct TemplateInstance {
    pub template: Entity,
    pub overrides: HashMap<String, Value>,
}

#[derive(Component)]
pub struct TemplateSlot {
    pub slot_type: SlotType,
    pub name: String,
    pub default_value: Value,
}

pub enum SlotType {
    Text,
    Image,
    Color,
    Number,
    Duration,
}
```

**Template Categories:**
- Lower thirds
- Title cards
- Transitions
- Callouts
- Social media (YouTube end screen, etc.)
- Intros/Outros

**UI Milestone:** Template browser. Drag template to timeline. Edit template slots in inspector.

---

#### Weeks 9-10: Rive Integration

**Dependencies:**
- `rive-rs` (MIT) — Rive runtime

**Components:**
```rust
#[derive(Component)]
pub struct RiveAsset {
    pub file_path: PathBuf,
    pub artboard: String,
}

#[derive(Component)]
pub struct RiveInstance {
    pub runtime: RiveRuntime,
    pub current_animation: String,
    pub state_machine: Option<String>,
}

#[derive(Component)]
pub struct RiveInputs {
    pub inputs: HashMap<String, RiveInputValue>,
}

pub enum RiveInputValue {
    Bool(bool),
    Number(f64),
    Trigger,
}
```

**AI Integration:**
```rust
pub struct SetRiveInputCommand {
    pub instance: Entity,
    pub input_name: String,
    pub value: RiveInputValue,
}

pub struct TriggerRiveStateCommand {
    pub instance: Entity,
    pub state_name: String,
}
```

**UI Milestone:** Import .riv files. Rive state machine controls. Preview Rive animations.

---

#### Weeks 11-12: Motion Paths & Expression System

**Components:**
```rust
#[derive(Component)]
pub struct MotionPath {
    pub path: BezierPath,
    pub orient_to_path: bool,
    pub auto_orient_offset: f32,
}

#[derive(Component)]
pub struct Expression {
    pub code: String,
    pub target_property: String,
    pub dependencies: Vec<String>,
}
```

**Expression Language (Rhai-based):**
```javascript
// Example expression: wiggle
let time = project.time;
let frequency = 2.0;
let amplitude = 10.0;
sin(time * frequency * PI * 2.0) * amplitude

// Example: loopOut
let duration = this.out - this.in;
let local_time = (project.time - this.in) % duration;
evaluate_keyframes(local_time)
```

**UI Milestone:** Motion path tool. Expression editor with autocomplete. Live preview.

---

### 7.2 Phase 3 Exit Criteria

| Criterion | Validation |
|-----------|------------|
| Keyframes animate all properties | Comprehensive animation test |
| Graph editor functional | Curve manipulation test |
| Text renders correctly | Multi-language text test |
| Templates work | Template round-trip test |
| Rive integration complete | State machine test |
| Expressions evaluate | Expression test suite |
| AI can animate elements | AI animation test |

---

## 8. Phase 4: AI Integration (Months 13-15)

### 8.0 Phase Goal
**Deliverable:** Full AI agent integration — auto-edit, voice commands, smart suggestions.  
**AI Capability:** AI is a co-editor, can perform complex multi-step tasks.
**UI Deliverable:** AI chat panel, suggestion cards, voice input indicator, AI status overlay.

### 8.1 Week-by-Week Breakdown

#### Weeks 1-2: AI Agent Architecture + Chat Panel UI

**Note:** This is the novel UI — no direct GPL reference to study. Build from first principles.

**Core Tasks:**
- [ ] Implement `AIAgent` resource
- [ ] Create context builder
- [ ] Build command submission interface

**UI Tasks:**
- [ ] Create `AIChatPanel` widget
- [ ] Add conversation history display
- [ ] Implement streaming response display
- [ ] Add context indicator (what AI sees)
- [ ] Create command preview before execution
- [ ] Add "Cancel" button for in-progress operations

**Components:**
```rust
#[derive(Resource)]
pub struct AIAgent {
    pub context_window: Vec<ContextItem>,
    pub pending_suggestions: Vec<Suggestion>,
    pub conversation_history: Vec<Message>,
}

#[derive(Component)]
pub struct AIAnnotation {
    pub entity: Entity,
    pub description: String,
    pub tags: Vec<String>,
    pub confidence: f32,
}
```

**Agent Loop:**
```
┌─────────────────────────────────────────────────────────────────┐
│                        AI Agent Loop                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  User Input ──► Intent Parser ──► Action Planner ──► Commands   │
│                      │                   │              │       │
│                      ▼                   ▼              ▼       │
│               [Scene Context]    [Available Actions]  [Execute] │
│               [Command History]  [Constraints]        [Undo]    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**UI Milestone:** AI chat panel. AI can respond to queries about project.

---

#### Weeks 3-4: Scene Understanding

**Dependencies:**
- `ort` (MIT) — ONNX inference
- Vision models (CLIP, etc.)

**Features:**
- Detect scene content (people, objects, text)
- Identify shot types (close-up, wide, etc.)
- Detect faces and expressions
- Identify audio content (speech, music, silence)

**Components:**
```rust
#[derive(Component)]
pub struct SceneAnalysis {
    pub objects: Vec<DetectedObject>,
    pub faces: Vec<DetectedFace>,
    pub shot_type: ShotType,
    pub dominant_colors: Vec<Color>,
    pub motion_level: f32,
}

#[derive(Component)]
pub struct AudioAnalysis {
    pub speech_segments: Vec<TimeRange>,
    pub music_segments: Vec<TimeRange>,
    pub silence_segments: Vec<TimeRange>,
    pub beats: Vec<Duration>,
    pub loudness_curve: Vec<f32>,
}
```

**UI Milestone:** Scene analysis panel. Object detection overlay. Audio waveform with markers.

---

#### Weeks 5-6: Natural Language Commands

**Dependencies:**
- `async-openai` (MIT) — OpenAI API
- `llama-cpp-rs` (MIT) — Local LLM option

**Command Mapping:**
```rust
pub struct NLCommandParser {
    pub command_registry: CommandRegistry,
    pub intent_classifier: IntentClassifier,
}

impl NLCommandParser {
    pub async fn parse(&self, input: &str, context: &ProjectContext) -> Result<Vec<Box<dyn Command>>> {
        // 1. Classify intent
        let intent = self.intent_classifier.classify(input).await?;
        
        // 2. Extract entities (clip names, timestamps, etc.)
        let entities = self.extract_entities(input, context).await?;
        
        // 3. Map to commands
        let commands = self.map_to_commands(intent, entities)?;
        
        Ok(commands)
    }
}
```

**Example Inputs → Commands:**
| Natural Language | Generated Commands |
|-----------------|-------------------|
| "Split this clip at 5 seconds" | `SplitClipCommand { at: 5s }` |
| "Add a dissolve between these clips" | `AddTransitionCommand { type: Dissolve }` |
| "Make this clip 20% slower" | `SetSpeedCommand { speed: 0.8 }` |
| "Remove the background" | `ApplyEffectCommand { type: BackgroundRemoval }` |
| "Color match to the previous shot" | `MatchColorCommand { source, target }` |

**UI Milestone:** Voice/text input. AI executes commands. Confirmation dialog for destructive ops.

---

#### Weeks 7-8: Auto-Edit Features

**Features:**
- Auto-cut to music beats
- Smart trim (remove silence)
- Jump cut generator
- Highlight reel creator
- Scene-based assembly

**Commands:**
```rust
pub struct AutoCutToBeatCommand {
    pub audio_track: Entity,
    pub target_clips: Vec<Entity>,
}

pub struct SmartTrimCommand {
    pub clips: Vec<Entity>,
    pub remove_silence: bool,
    pub silence_threshold: f32,
    pub min_silence_duration: Duration,
}

pub struct CreateHighlightReelCommand {
    pub source_clips: Vec<Entity>,
    pub target_duration: Duration,
    pub style: HighlightStyle,
}
```

**UI Milestone:** "Auto-edit" menu. Beat sync tool. Highlight reel wizard.

---

#### Weeks 9-10: Smart Suggestions

**Features:**
- Transition suggestions based on content
- Effect recommendations
- Pacing analysis
- Color grade suggestions
- Audio level recommendations

**Components:**
```rust
#[derive(Component)]
pub struct Suggestion {
    pub suggestion_type: SuggestionType,
    pub description: String,
    pub command: Box<dyn Command>,
    pub confidence: f32,
    pub preview: Option<TextureId>,
}

#[derive(Resource)]
pub struct SuggestionEngine {
    pub active_suggestions: Vec<Entity>,
    pub dismissed: HashSet<SuggestionId>,
}
```

**UI Milestone:** Suggestion cards. One-click apply. Dismiss/snooze options.

---

#### Weeks 11-12: Speech-to-Text & Auto-Captions

**Dependencies:**
- `whisper-rs` (MIT) — Speech recognition

**Components:**
```rust
#[derive(Component)]
pub struct Transcription {
    pub language: String,
    pub segments: Vec<TranscriptionSegment>,
}

pub struct TranscriptionSegment {
    pub text: String,
    pub start: Duration,
    pub end: Duration,
    pub confidence: f32,
    pub words: Vec<TimedWord>,
}

#[derive(Component)]
pub struct CaptionTrack {
    pub style: CaptionStyle,
    pub segments: Vec<Entity>,  // References to caption entities
}
```

**UI Milestone:** Transcribe button. Edit captions in timeline. Caption style editor.

---

### 8.2 Phase 4 Exit Criteria

| Criterion | Validation |
|-----------|------------|
| AI understands project context | Context query test |
| Natural language commands work | 50 command test suite |
| Auto-edit features functional | Auto-edit quality test |
| Suggestions are relevant | Suggestion relevance test |
| Transcription accurate | WER < 10% on test set |
| AI can create complete edits | End-to-end editing test |

---

## 9. Phase 5: Production Polish (Months 16-18)

### 9.0 Phase Goal
**Deliverable:** Production-ready v1.0 with polish, performance, and stability.  
**AI Capability:** AI is reliable, fast, and helpful in production workflows.

### 9.1 Week-by-Week Breakdown

#### Weeks 1-4: Performance Optimization

**Tasks:**
- [ ] Profile and optimize hot paths
- [ ] Implement GPU memory pooling
- [ ] Optimize video decode pipeline
- [ ] Add proxy workflow
- [ ] Implement background rendering
- [ ] Cache optimization

**Targets:**
| Metric | Target |
|--------|--------|
| 4K 60fps playback | Sustained, no drops |
| Effect chain (5 effects) | < 8ms per frame |
| Project load time | < 2s for 100 clip project |
| Memory usage | < 4GB for typical project |
| Export speed | ≥ Real-time for 1080p |

---

#### Weeks 5-8: Stability & Testing

**Tasks:**
- [ ] Crash reporting integration
- [ ] Automated UI testing
- [ ] Fuzz testing for file formats
- [ ] Memory leak detection
- [ ] Stress testing (1000+ clips)
- [ ] Cross-platform testing (Windows, macOS, Linux)

**Test Coverage Targets:**
| Area | Coverage |
|------|----------|
| Core ECS | 90% |
| Commands | 95% |
| Effects | 80% |
| UI | 70% |
| AI | 60% |

---

#### Weeks 9-10: UI Polish

**Tasks:**
- [ ] Keyboard shortcuts (comprehensive)
- [ ] Dark/Light theme
- [ ] Responsive layouts
- [ ] Accessibility (screen readers)
- [ ] Onboarding flow
- [ ] Tooltips and help

---

#### Weeks 11-12: Launch Preparation

**Tasks:**
- [ ] Documentation
- [ ] Tutorial videos
- [ ] Marketing website
- [ ] Beta program
- [ ] Crash reporting setup
- [ ] Analytics integration
- [ ] License system (if applicable)

---

### 9.2 Phase 5 Exit Criteria

| Criterion | Validation |
|-----------|------------|
| No P0/P1 bugs | Bug triage complete |
| Performance targets met | Benchmark suite passes |
| Test coverage achieved | Coverage report |
| Docs complete | Doc review |
| Beta feedback addressed | Feedback triage |

---

## 10. API Design for AI Control

### 10.1 Command Schema

Every command is AI-invocable via JSON:

```json
{
  "command": "split_clip",
  "params": {
    "clip_id": "clip_abc123",
    "at_frame": 120
  },
  "context": {
    "confirmation_required": false,
    "preview_requested": true
  }
}
```

### 10.2 Query API

AI can query project state:

```json
// Request
{
  "query": "get_clips",
  "filters": {
    "track": "V1",
    "time_range": { "start": 0, "end": 600 }
  }
}

// Response
{
  "clips": [
    {
      "id": "clip_abc123",
      "name": "Interview_01.mp4",
      "start_frame": 0,
      "end_frame": 300,
      "effects": ["color_correct", "blur"],
      "ai_annotations": {
        "content": "Person speaking, indoor, medium shot",
        "faces": [{ "id": "face_1", "expression": "neutral" }]
      }
    }
  ]
}
```

### 10.3 Event Stream

AI receives real-time events:

```json
{
  "event": "clip_selected",
  "data": {
    "clip_id": "clip_abc123",
    "selection_method": "user_click"
  }
}

{
  "event": "playhead_moved",
  "data": {
    "frame": 150,
    "triggered_by": "user_scrub"
  }
}
```

---

## 11. Testing Strategy

### 11.1 Test Pyramid

```
                    ┌─────────────┐
                    │   E2E (10%) │  ← Full workflow tests
                   ─┴─────────────┴─
                  ┌─────────────────┐
                  │ Integration (30%)│  ← Multi-system tests
                 ─┴─────────────────┴─
                ┌─────────────────────┐
                │    Unit (60%)       │  ← Component/System tests
               ─┴─────────────────────┴─
```

### 11.2 Test Types

| Type | Purpose | Tools |
|------|---------|-------|
| Unit | Test components/systems in isolation | `cargo test` |
| Integration | Test feature workflows | Custom harness |
| Visual Regression | Ensure renders don't change | Image diff |
| Performance | Maintain speed | Criterion benchmarks |
| Fuzz | Find edge case bugs | cargo-fuzz |
| E2E | Full user workflows | UI automation |

---

## 12. Future Enhancements (Competitive Edge Features)

> [!IMPORTANT]
> These enhancements go beyond basic functionality to create a **professional-grade, AI-native editor** that competes with Adobe Premiere, DaVinci Resolve, and CapCut.

### 12.1 Video Playback Engine (Phase 1-2)

| Feature | Priority | Description |
|---------|----------|-------------|
| **Media Cache on Disk** | High | Pre-decode frames to SSD (like Premiere's Media Cache) for instant scrubbing |
| **Proxy Workflows** | High | Auto-generate lower-res editing copies, link for export |
| **Preview Renders** | Medium | Pre-compute heavy effects to green-bar segments |
| **GPU Texture Pool** | Medium | Reuse textures instead of reallocating, reduce GPU memory churn |
| **Variable Speed Playback** | Medium | 0.25x - 4x playback with pitch-corrected audio |
| **Reverse Playback** | Low | Backward scrubbing with buffered frame cache |

### 12.2 Audio Engine Enhancements (Phase 1-2)

| Feature | Priority | Description |
|---------|----------|-------------|
| **Waveform Visualization** | High | GPU-rendered audio waveforms on timeline clips |
| **Multi-track Audio** | High | Separate audio tracks with independent controls |
| **Audio Peak Normalization** | Medium | Auto-level audio tracks |
| **Ducking/Sidechain** | Medium | Auto-lower background when speech detected |
| **VST3/CLAP Plugin Support** | Low | External audio effects |

### 12.3 Performance Optimizations (Phase 2-3)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PERFORMANCE ARCHITECTURE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐ │
│  │  ASYNC DECODE       │  │  GPU COMPUTE        │  │  SMART CACHING      │ │
│  │  ─────────────      │  │  ───────────        │  │  ─────────────      │ │
│  │  • Tokio runtime    │  │  • NVENC export     │  │  • LRU frame cache  │ │
│  │  • Thread pool      │  │  • GPU effects      │  │  • SSD media cache  │ │
│  │  • Priority queue   │  │  • Compute shaders  │  │  • Memory mapping   │ │
│  └─────────────────────┘  └─────────────────────┘  └─────────────────────┘ │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

| Feature | Impact | Complexity |
|---------|--------|-----------|
| **NVENC/NVDEC Full Pipeline** | 10x export speed | Medium |
| **Compute Shader Effects** | 5x effect speed | High |
| **Multi-GPU Support** | 2x render speed | Medium |
| **Memory-Mapped File I/O** | Faster large file reads | Low |
| **Incremental Render** | Only re-render changed segments | High |

### 12.4 UI/UX Competitive Features (Phase 1-3)

| Feature | Inspiration | Description |
|---------|-------------|-------------|
| **Magnetic Timeline** | Final Cut Pro | Clips snap and ripple intelligently |
| **Multicam Sync** | Premiere, Resolve | Audio-sync multiple camera angles |
| **Color Scopes** | DaVinci Resolve | Parade, Waveform, Vectorscope |
| **Motion Tracking** | After Effects | Point and planar tracking |
| **Text Presets** | CapCut | One-click animated titles |
| **Background Removal** | CapCut, Runway | AI-powered green screen without green screen |
| **Speed Ramps** | CapCut | Bezier-controlled speed curves |

### 12.5 AI-Native Competitive Advantages (Phase 4-5)

> [!TIP]
> These are the features that will **differentiate** Project Strawberry from every other editor.

| Feature | Description | Why It Wins |
|---------|-------------|-------------|
| **Voice-to-Edit** | "Remove the um's and pauses" → Auto-edit | Fastest rough cut in industry |
| **Smart Auto-Reframe** | AI tracks subjects for vertical/square crops | One-click TikTok/Reels export |
| **Content-Aware Transitions** | AI picks transitions based on content | Better than random transitions |
| **Auto B-Roll** | AI suggests stock footage based on speech | Built-in stock integration |
| **Semantic Search** | "Find the part where she laughs" | Natural timeline navigation |
| **Style Transfer** | Apply visual style from reference video | One-click color matching |
| **Auto Subtitles** | Whisper-powered transcription | Native SRT/VTT export |
| **Script Sync** | Align video to script/teleprompter | Script → timeline selection |

### 12.6 ECS Architecture Improvements (Ongoing)

| Improvement | Description |
|-------------|-------------|
| **Hierarchical Undo** | Group related commands, undo entire operation |
| **Selective Undo** | Undo specific action without undoing later ones |
| **Command Batching** | Batch small edits into single undo step |
| **Parallel Systems** | More ECS systems run in parallel |
| **Component Archetype Optimization** | Optimal memory layout for common patterns |
| **Hot Reload** | Modify effects/scripts without restart |

### 12.7 Export & Distribution (Phase 3-5)

| Feature | Priority | Description |
|---------|----------|-------------|
| **One-Click Social Export** | High | Optimized presets for YouTube, TikTok, Instagram |
| **Hardware Encoding** | High | NVENC H.264/H.265/AV1 |
| **Background Rendering** | Medium | Export while continuing to edit |
| **Cloud Renders** | Low | Offload to cloud for 4K+ exports |
| **Direct Upload** | Medium | Publish directly to platforms |

---

## 13. Risk Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| FFmpeg bindings break | High | Medium | Pin version, have fallback |
| wgpu compatibility issues | High | Low | Test on multiple GPUs |
| AI API costs too high | Medium | Medium | Local LLM fallback |
| Performance regression | High | Medium | CI benchmarks with alerts |
| Scope creep | High | High | Strict phase gates |
| Key person dependency | High | Medium | Document everything, pair program |

---

## Appendix A: Full Dependency Matrix

[See Section 3.1 for core dependencies]

## Appendix B: Feature Flag Registry

| Flag | Description | Phase |
|------|-------------|-------|
| `video_decode` | Video decode functionality | 0 |
| `audio_playback` | Audio playback | 0 |
| `timeline` | Timeline editing | 1 |
| `transitions` | Transition effects | 1 |
| `effects` | Effect system | 2 |
| `color_grade` | Color grading | 2 |
| `animation` | Keyframe animation | 3 |
| `rive` | Rive integration | 3 |
| `ai_agent` | AI agent | 4 |
| `auto_edit` | Auto-edit features | 4 |

---

## Document Sign-Off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Tech Lead | [Your Name] | 2026-01-12 | ✓ |
| Product | | | |
| Engineering | | | |

---

**END OF DOCUMENT**
