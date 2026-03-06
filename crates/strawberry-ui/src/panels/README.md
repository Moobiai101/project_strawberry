# Panels Module

UI panels for the Project Strawberry editor.

## Overview

This module contains dock-able panels that provide different views and functionality within the editor. Panels are integrated into the `egui_tiles`-based docking system in `strawberry-app`.

## Panels

### Inspector (`inspector.rs`)
- Shows properties of selected items
- Displays video metadata

### Media Browser (`media_browser.rs`)
- **Purpose**: Browse and manage imported project assets
- **Views**: Grid (with thumbnails) and List (with details)
- **Features**:
  - Search filter
  - Adjustable thumbnail size
  - Proxy status indicators (color-coded dots)
  - Context menu (Delete, Regenerate Proxy, Copy Path)
  - Drag-to-timeline support (via response)
  - Double-click to add to timeline

## Data Flow

```
AssetRegistry (ECS Resource)
        ↓
  MediaBrowserPanel.show()
        ↓
  MediaBrowserResponse
        ↓
  docking.rs handler → Commands
```

## Integration

Panels are rendered by `TreeBehavior` in `strawberry-app/src/docking.rs`. Each panel:
1. Gets state from egui memory (for persistence across frames)
2. Reads from ECS resources
3. Returns a response struct with user actions
4. Response is handled to dispatch Commands

## Adding New Panels

1. Create `panels/my_panel.rs`
2. Add `pub mod my_panel;` to `panels/mod.rs`
3. Add `PaneType::MyPanel` variant in `docking.rs`
4. Add `render_my_panel()` in `TreeBehavior`
5. Update `create_default_layout()` if needed
