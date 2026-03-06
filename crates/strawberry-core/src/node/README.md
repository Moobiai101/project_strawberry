# Node System

> Production-grade node graph architecture for timeline compositing.
> Based on Olive Editor's architecture, adapted for Rust/Bevy ECS.

## Purpose

The node system provides a unified architecture for:
- **Timeline rendering**: Clips and tracks as nodes
- **Effects**: Blur, color correction, keying
- **Generators**: Solid, text, shapes
- **Transitions**: Dissolve, wipe, fade

## Data Flow

```
┌──────────────────────────────────────────────────────────┐
│                    RENDER PIPELINE                       │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  Playhead.current_time                                   │
│          │                                               │
│          ▼                                               │
│  ┌─────────────────┐                                    │
│  │  TimelineNode   │ (queries all TrackNodes)           │
│  └────────┬────────┘                                    │
│           │                                              │
│           ▼                                              │
│  ┌─────────────────┐                                    │
│  │   TrackNode     │ (queries ClipNodes at time)        │
│  └────────┬────────┘                                    │
│           │                                              │
│           ▼                                              │
│  ┌─────────────────┐                                    │
│  │   ClipNode      │ (timeline_to_source + asset ref)   │
│  └────────┬────────┘                                    │
│           │                                              │
│           ▼                                              │
│  ┌─────────────────┐                                    │
│  │ VideoAssetMgr   │ (decode frame from source time)    │
│  └────────┬────────┘                                    │
│           │                                              │
│           ▼                                              │
│      NodeValue::Texture                                  │
│           │                                              │
│           ▼                                              │
│  NodeTraverser.resolve_jobs() → GPU texture              │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

## Key Structs

| Struct | File | Description |
|--------|------|-------------|
| `Node` trait | `mod.rs` | Base interface for all nodes |
| `NodeCategory` | `mod.rs` | Categorization for node menus |
| `NodeValue` | `value.rs` | Typed values flowing between nodes |
| `NodeValueType` | `value.rs` | Type discriminant for NodeValue |
| `InputDefinition` | `input.rs` | Describes a node input (type, default, keyframable) |
| `ClipNode` | `timeline/clip.rs` | Node wrapper around ECS Clip component |
| `TrackNode` | `timeline/track.rs` | Node wrapper around ECS Track component |

## Dependencies

- `strawberry-video`: For `VideoAssetManager` (footage decoding)
- `uuid`: For stable node IDs
- `glam`: For vector/matrix types
- `bevy_ecs`: For ECS integration

## Module Structure

```
node/
├── README.md          ← You are here
├── mod.rs             ← Node trait, NodeCategory, re-exports
├── value.rs           ← NodeValue enum, NodeValueType
├── input.rs           ← InputDefinition, NodeInput
└── timeline/
    ├── README.md
    ├── mod.rs
    ├── clip.rs        ← ClipNode implementation
    └── track.rs       ← TrackNode implementation
```

## Example: ClipNode

```rust
impl Node for ClipNode {
    fn id(&self) -> &'static str { "org.strawberry.clip" }
    fn category(&self) -> NodeCategory { NodeCategory::Timeline }
    
    fn process(
        &self,
        inputs: &NodeValueRow,
        globals: &NodeGlobals,
    ) -> NodeValueTable {
        // 1. Get timeline time from globals
        let timeline_time = globals.time_range.start;
        
        // 2. Convert to source time using Clip component
        let source_time = self.clip.timeline_to_source(timeline_time)?;
        
        // 3. Request frame from VideoAssetManager
        let frame = inputs.get("footage")?;
        
        // 4. Return texture
        table.push(NodeValue::Texture(frame.texture_id));
        table
    }
}
```

## AI Agent Notes

- **400-line limit**: Each file stays under 400 lines
- **Explicit types**: All function signatures use explicit return types
- **Command pattern**: Node mutations go through Commands
- **StableId**: All nodes have UUIDs, not raw ECS Entity IDs
