# Timeline Nodes

> Node wrappers for timeline ECS components (Track, Clip).

## Purpose

These nodes bridge the ECS component system with the node graph architecture.
They wrap existing `Track` and `Clip` components, implementing the `Node` trait
so the `NodeTraverser` can render timelines correctly.

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    TIMELINE RENDERING                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  NodeTraverser                                              │
│       │                                                     │
│       ├──► TimelineNode                                     │
│       │         │                                           │
│       │         └──► Query all TrackNodes (sorted by index) │
│       │                    │                                │
│       │                    └──► For each TrackNode:         │
│       │                              │                      │
│       │                              └──► ClipNode          │
│       │                                     │               │
│       │                                     ├─ timeline_to_source()  │
│       │                                     └─ request_frame()       │
│       │                                                     │
│       └──► Alpha-over composite all layers                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Key Types

| Type | File | Description |
|------|------|-------------|
| `ClipNode` | `clip.rs` | Wraps `Clip` component, handles time remapping |
| `TrackNode` | `track.rs` | Wraps `Track` component, collects clips |

## Time Remapping

ClipNode implements the same algorithm as Olive's `ClipBlock::SequenceToMediaTime`:

```rust
fn timeline_to_source(&self, timeline_time: Duration) -> Option<Duration> {
    // 1. Check if time is within clip bounds
    if !self.clip.contains_time(timeline_time) {
        return None;
    }
    
    // 2. Calculate relative time from clip start
    let relative = timeline_time - self.clip.timeline_start;
    
    // 3. Apply speed (Phase 2)
    let scaled = if self.speed != 1.0 {
        Duration::from_secs_f64(relative.as_secs_f64() * self.speed)
    } else {
        relative
    };
    
    // 4. Apply reverse (Phase 2)
    let adjusted = if self.reverse {
        self.clip.duration() - scaled
    } else {
        scaled
    };
    
    // 5. Add source in-point
    Some(self.clip.source_in + adjusted)
}
```

## Integration with ECS

ClipNode and TrackNode read from ECS components but don't modify them.
All mutations go through Commands:
- `AddClipCommand` - Creates clip with `ClipAssetRef`
- `MoveClipCommand` - Updates `Clip.timeline_start`
- `TrimClipCommand` - Updates `Clip.source_in/source_out`
