# Project Strawberry: Architecture Plan

> [!IMPORTANT]
> **THIS IS NOT AN MVP.** This is a production-grade AI-native creative engine for video editing, motion graphics, and animation. Users will use this as their primary creative tool. Every line of code must be shippable quality.

**Code Name:** Project Strawberry (final name TBD)  
**Target Platform:** Windows (primary)  
**Export Targets:** All major formats (MP4, WebM, ProRes, etc.)  
**Team:** Co-founders building together

---

## How UI, AI, User, ECS, and Engine Work Together

This document provides a detailed analysis of how all components interact in the AI-native creative engine.

---

## 1. High-Level Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                              PROJECT STRAWBERRY ARCHITECTURE                             │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                         │
│   ┌─────────────┐          ┌─────────────┐          ┌─────────────────────────────┐    │
│   │    USER     │          │     AI      │          │         UI (egui)           │    │
│   │  (Human)    │          │  (Sidecar)  │          │  Viewport, Timeline, etc.   │    │
│   └──────┬──────┘          └──────┬──────┘          └──────────────┬──────────────┘    │
│          │                        │                                │                    │
│          │ Clicks, Drags,         │ Natural Language,              │ Widget Events      │
│          │ Keyboard               │ Commands                       │                    │
│          │                        │                                │                    │
│          ▼                        ▼                                ▼                    │
│   ┌─────────────────────────────────────────────────────────────────────────────────┐  │
│   │                           COMMAND LAYER                                          │  │
│   │  ┌─────────────────────────────────────────────────────────────────────────┐    │  │
│   │  │                       Command Queue                                      │    │  │
│   │  │   [SplitClip] [AddEffect] [Seek] [Undo] [ApplyTransition] ...          │    │  │
│   │  └─────────────────────────────────────────────────────────────────────────┘    │  │
│   │                                    │                                             │  │
│   │                                    ▼                                             │  │
│   │  ┌─────────────────────────────────────────────────────────────────────────┐    │  │
│   │  │                       Undo/Redo Stack                                    │    │  │
│   │  └─────────────────────────────────────────────────────────────────────────┘    │  │
│   └─────────────────────────────────────────────────────────────────────────────────┘  │
│                                        │                                                │
│                                        ▼                                                │
│   ┌─────────────────────────────────────────────────────────────────────────────────┐  │
│   │                              ECS WORLD (bevy_ecs)                                │  │
│   │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐                  │  │
│   │  │   ENTITIES      │  │   COMPONENTS    │  │   RESOURCES     │                  │  │
│   │  │                 │  │                 │  │                 │                  │  │
│   │  │  • Clips        │  │  • Clip         │  │  • Playhead     │                  │  │
│   │  │  • Tracks       │  │  • Track        │  │  • Project      │                  │  │
│   │  │  • Effects      │  │  • Effect       │  │  • Selection    │                  │  │
│   │  │  • Transitions  │  │  • Transform    │  │  • AIContext    │                  │  │
│   │  │  • Assets       │  │  • ColorGrade   │  │  • CommandQueue │                  │  │
│   │  └─────────────────┘  └─────────────────┘  └─────────────────┘                  │  │
│   │                                                                                  │  │
│   │  ┌─────────────────────────────────────────────────────────────────────────┐    │  │
│   │  │                           SYSTEMS                                        │    │  │
│   │  │  PlaybackSystem, RenderSystem, EffectSystem, AnimationSystem, ...       │    │  │
│   │  └─────────────────────────────────────────────────────────────────────────┘    │  │
│   └─────────────────────────────────────────────────────────────────────────────────┘  │
│                                        │                                                │
│                                        ▼                                                │
│   ┌─────────────────────────────────────────────────────────────────────────────────┐  │
│   │                              ENGINE SERVICES                                     │  │
│   │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │  │
│   │  │   Video     │  │   Audio     │  │   Render    │  │   File      │            │  │
│   │  │   Decode    │  │   Playback  │  │   Pipeline  │  │   I/O       │            │  │
│   │  │  (ffmpeg)   │  │   (cpal)    │  │   (wgpu)    │  │             │            │  │
│   │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘            │  │
│   └─────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                         │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. The Command Layer: Single Source of Truth

> **CRITICAL PRINCIPLE**: All mutations to the ECS World go through Commands. No exceptions.

### Why Commands?

| Benefit | Explanation |
|---------|-------------|
| **Undo/Redo** | Every command knows how to reverse itself |
| **AI Parity** | AI uses same commands as user clicks |
| **Auditability** | Full history of all operations |
| **Serialization** | Commands can be saved/replayed |
| **Testing** | Commands can be tested in isolation |

### Command Flow

```
USER                        AI                          UI
  │                         │                           │
  │ "Split clip at 5s"      │ "Split this clip here"   │ [Split Button Click]
  │                         │                           │
  └────────┬────────────────┴────────────┬──────────────┘
           │                             │
           ▼                             ▼
     ┌─────────────────────────────────────────────┐
     │           Command Dispatcher                 │
     │                                              │
     │  1. Validate command                         │
     │  2. Check permissions                        │
     │  3. Create undo data                         │
     │  4. Execute on ECS World                     │
     │  5. Push to undo stack                       │
     │  6. Emit events                              │
     └─────────────────────────────────────────────┘
                         │
                         ▼
     ┌─────────────────────────────────────────────┐
     │              ECS World Mutated               │
     │                                              │
     │  • New entity created                        │
     │  • Components updated                        │
     │  • Resources modified                        │
     └─────────────────────────────────────────────┘
                         │
                         ▼
     ┌─────────────────────────────────────────────┐
     │           UI Reacts (Immediate Mode)         │
     │                                              │
     │  • Timeline redraws                          │
     │  • Viewport updates                          │
     │  • Inspector refreshes                       │
     └─────────────────────────────────────────────┘
```

---

## 3. USER → ENGINE Interaction

### How User Actions Become Commands

```rust
// Example: User clicks Split button
fn handle_split_button(ui: &mut egui::Ui, state: &AppState) {
    if ui.button("✂ Split").clicked() {
        // 1. Get current selection and playhead
        let selected_clip = state.selection.primary_clip();
        let playhead = state.playhead.current_time;
        
        // 2. Create command
        let command = SplitClipCommand {
            clip: selected_clip,
            at: playhead,
        };
        
        // 3. Submit to command queue (NOT directly to ECS)
        state.command_queue.submit(Box::new(command));
    }
} // THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

### User Interaction Types

| Interaction | Source | Command Generated |
|-------------|--------|-------------------|
| Click button | UI widget | Direct command |
| Drag clip | Mouse event | MoveClipCommand |
| Keyboard shortcut | Input system | Mapped to command |
| Drag trim handle | Mouse event | TrimClipCommand |
| Right-click menu | Context menu | Selected command |
| Slider change | Inspector | SetPropertyCommand |

---

## 4. AI → ENGINE Interaction

### AI Sidecar Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           AI SIDECAR (Separate Process/Thread)              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        Context Builder                               │   │
│  │                                                                      │   │
│  │  Tier 1: Always (selection, playhead, recent commands)              │   │
│  │  Tier 2: Dynamic (nearby clips, selected entity details)            │   │
│  │  Tier 3: Semantic (embeddings search for relevant content)          │   │
│  │  Tier 4: Summarized (timeline overview)                             │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        LLM Inference                                 │   │
│  │                                                                      │   │
│  │  • OpenAI API (async-openai)                                        │   │
│  │  • Local LLM (llama-cpp-rs)                                         │   │
│  │  • ONNX models (ort)                                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        Action Parser                                 │   │
│  │                                                                      │   │
│  │  LLM Output → Validated Commands                                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                              │
                              │ IPC (tokio::mpsc / gRPC)
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           MAIN ENGINE PROCESS                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────────────┐     ┌──────────────────────┐                     │
│  │   AI Query Service   │     │  AI Command Interface │                    │
│  │   (Read-Only)        │     │  (Write via Commands) │                    │
│  │                      │     │                      │                      │
│  │  get_timeline()      │     │  submit(command)     │                      │
│  │  get_selection()     │     │  submit_batch([...]) │                      │
│  │  get_clip(id)        │     │                      │                      │
│  └──────────┬───────────┘     └──────────┬───────────┘                     │
│             │                            │                                  │
│             │ JSON snapshot              │ Command objects                  │
│             ▼                            ▼                                  │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                           ECS WORLD                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### AI Read Path (Query Service)

```rust
/// AI queries ECS through read-only interface
impl AIQueryService {
    /// Returns JSON snapshot - AI never holds World reference
    pub fn get_timeline_context(&self) -> serde_json::Value {
        let world = self.world.read();
        
        json!({
            "playhead_frame": world.resource::<Playhead>().current_frame,
            "duration_frames": world.resource::<Project>().duration_frames,
            "tracks": self.serialize_tracks(&world),
            "selection": self.serialize_selection(&world),
            "recent_commands": self.get_recent_commands(&world, 5),
        })
    }
}// THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

### AI Write Path (Command Interface)

```rust
/// AI submits commands through same queue as UI
impl AICommandInterface {
    pub fn submit(&self, request: AICommandRequest) -> Result<CommandId> {
        // 1. Parse command type
        let command = match request.command_type.as_str() {
            "split_clip" => Box::new(SplitClipCommand {
                clip: Entity::from_bits(request.params["clip"].as_u64()?),
                at: Duration::from_secs_f64(request.params["at"].as_f64()?),
            }),
            "add_effect" => Box::new(AddEffectCommand {
                target: Entity::from_bits(request.params["target"].as_u64()?),
                effect_type: request.params["effect_type"].as_str()?.parse()?,
            }),
            // ... more command types
            _ => return Err(AIError::UnknownCommand),
        };
        
        // 2. Submit to SAME queue as UI
        self.command_queue.send(command)?;
        
        Ok(CommandId::new())
    }
}// THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

### Why AI Uses Same Command Path?

| Reason | Benefit |
|--------|---------|
| Same undo/redo | User can undo AI actions |
| Same validation | AI can't do invalid operations |
| Same audit log | AI actions tracked identically |
| Same permissions | AI respects same constraints |
| Testable | Test commands once, works for both |

---

## 5. UI → ECS Interaction

### Immediate Mode UI Reading ECS

```rust
/// UI reads ECS state every frame (immediate mode)
fn draw_timeline_panel(ui: &mut egui::Ui, world: &World) {
    // 1. Query ECS for tracks
    let tracks = world.query::<(Entity, &Track)>();
    
    for (entity, track) in tracks.iter(world) {
        // 2. Draw track header
        draw_track_header(ui, entity, track);
        
        // 3. Query clips for this track
        let clips = world.query_filtered::<(Entity, &Clip), With<OnTrack>>();
        
        for (clip_entity, clip) in clips.iter(world) {
            if clip.track == entity {
                // 4. Draw clip block
                draw_clip_block(ui, clip_entity, clip);
            }
        }
    }
}
```

### UI Writing to ECS (Always Via Commands)

```rust
/// UI NEVER mutates ECS directly
fn handle_clip_drag(
    response: &egui::Response,
    clip_entity: Entity,
    state: &mut AppState,
) {
    if response.drag_released() {
        let new_position = calculate_drop_position(response);
        
        // ✅ CORRECT: Submit command
        state.command_queue.submit(Box::new(MoveClipCommand {
            clip: clip_entity,
            to_track: new_position.track,
            at: new_position.time,
        }));
        
        // ❌ WRONG: Never do this
        // world.get_mut::<Clip>(clip_entity).unwrap().timeline_start = new_position.time;
    }
}
```

---

## 6. ECS ↔ Engine Services Interaction

### ECS Systems Call Engine Services

```rust
/// Systems are the bridge between ECS and Engine Services
fn playback_system(
    time: Res<Time>,
    mut playhead: ResMut<Playhead>,
    video_service: Res<VideoDecodeService>,
    audio_service: Res<AudioPlaybackService>,
    clips: Query<&Clip>,
) {
    if playhead.is_playing {
        // 1. Advance playhead
        playhead.current_time += time.delta();
        
        // 2. Request frames from video service
        for clip in clips.iter() {
            if clip.is_at_playhead(&playhead) {
                video_service.request_frame(clip.asset, playhead.current_time);
            }
        }
        
        // 3. Sync audio
        audio_service.sync_to(playhead.current_time);
    }
} // THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

### Engine Services Don't Know ECS

```rust
/// Video decode service is ECS-agnostic
pub struct VideoDecodeService {
    decoder: FFmpegDecoder,
    frame_cache: LruCache<FrameKey, GpuTexture>,
}

impl VideoDecodeService {
    /// Just decodes - doesn't know about entities
    pub async fn decode_frame(&self, path: &Path, time: Duration) -> GpuTexture {
        // Check cache
        if let Some(frame) = self.frame_cache.get(&(path, time)) {
            return frame.clone();
        }
        
        // Decode
        let frame = self.decoder.decode(path, time).await?;
        let texture = self.upload_to_gpu(frame);
        
        // Cache
        self.frame_cache.insert((path.to_owned(), time), texture.clone());
        
        texture
    }
} // THE ABOVE CODE IS FOR A VERY BASIC EXAMPLE, ACTUALL IMPELMETATION CODE IS GONNA BE ACCORDING TO ENGINE ARCHITECTURE
```

---

## 7. Complete Interaction Example: User Adds Effect

```
USER clicks "Add Blur" button
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ UI: handle_add_effect_button()                                 │
│                                                                │
│ 1. Get selected clip from ECS query                           │
│ 2. Create AddEffectCommand { clip, effect_type: Blur }        │
│ 3. Submit to command_queue                                     │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ COMMAND DISPATCHER: execute_commands()                         │
│                                                                │
│ 1. Pop AddEffectCommand from queue                            │
│ 2. Validate clip exists                                        │
│ 3. Create undo data (no effect existed)                       │
│ 4. Execute:                                                    │
│    - Spawn new entity with BlurEffect component               │
│    - Add to clip's EffectStack                                │
│ 5. Push to undo stack                                          │
│ 6. Emit EffectAddedEvent                                       │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ ECS WORLD (Now Contains):                                      │
│                                                                │
│ Entity(42): Clip                                               │
│   - EffectStack { effects: [Entity(99)] }                     │
│                                                                │
│ Entity(99): BlurEffect (NEW)                                   │
│   - BlurParams { radius: 10.0, quality: High }                │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ EFFECT SYSTEM: effect_process_system()                         │
│                                                                │
│ 1. Query clips with EffectStack                               │
│ 2. For each effect in stack:                                  │
│    - Get effect parameters                                     │
│    - Dispatch GPU shader                                       │
│ 3. BlurEffect → dispatch gaussian_blur.wgsl                   │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ RENDER PIPELINE: RenderGraph                                   │
│                                                                │
│ [VideoFrame] → [BlurShader] → [Composite] → [Present]         │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ UI (Next Frame): draw_effect_stack_panel()                     │
│                                                                │
│ 1. Query clip's EffectStack                                   │
│ 2. Now shows BlurEffect in list                               │
│ 3. Inspector shows blur radius slider                          │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
USER sees blurred clip in viewport + effect in panel
```

---

## 8. Complete Interaction Example: AI Adds Transition

```
USER says: "Add a dissolve between these two clips"
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ AI CHAT PANEL: on_user_message()                               │
│                                                                │
│ 1. Send to AI Sidecar via IPC                                 │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ AI SIDECAR: process_request()                                  │
│                                                                │
│ 1. Query ECS state via AIQueryService                         │
│    → Get selected clips: [Entity(10), Entity(11)]             │
│    → Get clip positions                                        │
│                                                                │
│ 2. Build context for LLM                                      │
│    {                                                           │
│      "selection": ["clip_10: interview.mp4", "clip_11: ...]   │
│      "user_request": "Add a dissolve between these two clips" │
│      "available_commands": ["AddTransitionCommand", ...]      │
│    }                                                           │
│                                                                │
│ 3. LLM generates action                                       │
│    → "AddTransitionCommand(clip_a: 10, clip_b: 11, Dissolve)" │
│                                                                │
│ 4. Parse and validate command                                  │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ AI COMMAND INTERFACE: submit()                                 │
│                                                                │
│ 1. Create AddTransitionCommand                                │
│ 2. Submit to command_queue (SAME queue as UI)                 │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
┌───────────────────────────────────────────────────────────────┐
│ COMMAND DISPATCHER: execute_commands()                         │
│                                                                │
│ (Same flow as user clicking "Add Transition")                  │
│                                                                │
│ 1. Validate clips exist and are adjacent                      │
│ 2. Create undo data                                            │
│ 3. Spawn Transition entity                                    │
│ 4. Push to undo stack                                          │
└───────────────────────────────────────────────────────────────┘
        │
        ▼
USER sees:
  - Dissolve transition on timeline
  - AI chat shows "Added dissolve transition ✓"
  - Can press Ctrl+Z to undo (same as any action)
```

---

## 9. Data Flow Summary

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            DATA FLOW RULES                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────┐                                                                │
│  │  USER   │ ─────► UI Event ─────► Command ─────► ECS ─────► Systems      │
│  └─────────┘                           │              ▲                     │
│                                        │              │                     │
│  ┌─────────┐                           │              │                     │
│  │   AI    │ ─────► NL Parse ──────────┘              │                     │
│  └─────────┘                                          │                     │
│                                                       │                     │
│  ┌─────────┐                                          │                     │
│  │   UI    │ ◄──────────────────── Read (Query) ──────┘                     │
│  └─────────┘                                                                │
│                                                                             │
│                                                                             │
│  WRITE PATH (User/AI → ECS):                                               │
│  ═══════════════════════════                                               │
│  User/AI → Intent → Command → Validate → Execute → Undo Stack → ECS       │
│                                                                             │
│  READ PATH (UI ← ECS):                                                     │
│  ═════════════════════                                                     │
│  UI ← Query ECS ← Systems ← Frame Update                                   │
│                                                                             │
│  AI READ PATH:                                                             │
│  ═════════════                                                             │
│  AI Sidecar ← JSON Snapshot ← AIQueryService ← Query ECS                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. Key Invariants

| Invariant | Enforcement |
|-----------|-------------|
| All writes go through Commands | UI/AI only submit commands, never mutate |
| Commands are reversible | Every command implements undo() |
| AI uses same code path as user | Same Command types, same queue |
| UI reads ECS every frame | Immediate mode ensures freshness |
| Engine services are ECS-agnostic | Services don't import bevy_ecs |
| Systems are the bridge | Only systems call engine services |

---

## 11. Crate Dependency Graph

```
strawberry-app (binary)
    │
    ├── strawberry-ui (egui widgets)
    │   └── strawberry-core (ECS, Commands)
    │
    ├── strawberry-ai (AI sidecar)
    │   └── strawberry-core
    │
    ├── strawberry-render (wgpu pipeline)
    │   └── strawberry-core
    │
    ├── strawberry-video (ffmpeg)
    │   └── (no ECS dependency)
    │
    └── strawberry-audio (cpal)
        └── (no ECS dependency)
```

**Note**: `strawberry-video` and `strawberry-audio` have NO dependency on ECS. They are pure services that systems call.

---

## 12. Thread/Process Model

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              THREAD MODEL                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  MAIN THREAD (60 FPS)                                                       │
│  ─────────────────────                                                      │
│  1. Poll input                                                              │
│  2. Execute commands                                                        │
│  3. Run ECS systems                                                         │
│  4. Draw UI (egui)                                                          │
│  5. Submit GPU work                                                         │
│  6. Present frame                                                           │
│                                                                             │
│  ASYNC TASK POOL (bevy_tasks)                                              │
│  ─────────────────────────────                                             │
│  • Video decode (background frames)                                        │
│  • Audio decode                                                            │
│  • File I/O                                                                │
│  • Export encoding                                                         │
│                                                                             │
│  AI SIDECAR (Separate)                                                     │
│  ─────────────────────                                                     │
│  • LLM inference (doesn't block main thread)                               │
│  • Scene analysis                                                          │
│  • Embedding computation                                                   │
│  • Communicates via channels                                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 13. Summary: How Everything Connects

| From | To | Mechanism | Data Flow |
|------|-----|-----------|-----------|
| **User** | Command Layer | UI events → Commands | Click → SplitClipCommand |
| **AI** | Command Layer | NL → Parse → Commands | "Split here" → SplitClipCommand |
| **Command** | ECS | execute() on World | Command mutates entities/components |
| **ECS** | UI | Query → draw() | UI reads and renders ECS state |
| **ECS** | AI | Query → JSON | AI reads snapshot of ECS state |
| **Systems** | Engine | Function calls | render_system → RenderPipeline |
| **Engine** | GPU | wgpu | Shaders execute on GPU |

**The Command Layer is the central nervous system.** Everything flows through it.
