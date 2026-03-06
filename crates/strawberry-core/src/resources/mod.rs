//! # Resources
//!
//! Global ECS resources for Project Strawberry.

use bevy_ecs::entity::Entity;
use bevy_ecs::system::Resource;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::commands::{UndoEntry, UndoStack};

// Sub-modules
pub mod asset_registry;
pub mod collision;
pub mod insertion;
pub mod master_audio;
pub mod media_browser;
pub mod project_settings;
pub mod proxy_progress;
pub mod thumbnail_cache;
pub mod timeline_utils;
pub mod user_settings;
pub mod viewport_state;

// Re-exports from sub-modules
pub use asset_registry::{
    AssetMetadata, AssetRegistry, AssetType, AudioStatus, ProxyStatus, RegisteredAsset,
};
pub use collision::{
    ClipTrimAction, CollisionAction, CollisionInfo, CollisionMode, CollisionSettings,
};
pub use insertion::{
    duration_to_frame, find_first_video_track, frame_to_duration, resolve_insertion_position,
    snap_to_frame_boundary, InsertionIntent,
};
pub use master_audio::MasterAudioState;
pub use media_browser::{MediaBrowser, MediaBrowserAsset};
pub use project_settings::{AspectRatioPreset, ProjectSettings};
pub use proxy_progress::{AssetProxyProgress, ProxyProgress};
pub use thumbnail_cache::{LoadedThumbnails, ThumbnailTexture, ThumbnailTextureCache};
pub use timeline_utils::{
    calculate_track_content_duration, calculate_track_end_time, count_clips_on_track,
    get_clips_on_track_sorted,
};
pub use user_settings::{ProxyQuality, ProxyResolution, ProxySettings, UiSettings, UserSettings};
pub use viewport_state::TimelineViewport;

// ============================================================================
// STABLE ID REGISTRY - Core of the scalable undo/redo architecture
// ============================================================================

/// Registry that maps stable UUIDs to current ECS entities
///
/// This is the foundation of the undo/redo system. Unlike ECS entity IDs
/// which become invalid after despawn/respawn, UUIDs are stable forever.
///
/// ## How Commands Use This
/// ```ignore
/// // In command execute()
/// fn execute(&mut self, world: &mut World) -> Result<...> {
///     let registry = world.resource::<StableIdRegistry>();
///     let entity = registry.get(&self.clip_uuid)?;  // Always gets current entity
///     // Work with entity...
/// }
/// ```
///
/// ## Undo/Redo Lifecycle
/// 1. **Create**: Entity spawned → register(uuid, entity)
/// 2. **Delete/Undo**: Entity despawned → park(uuid) marks as None
/// 3. **Redo/Restore**: New entity spawned → restore(uuid, new_entity)
///
/// The UUID stays the same, only the entity ID changes.
#[derive(Resource, Debug, Clone, Default)]
pub struct StableIdRegistry {
    /// Map from UUID to current entity (None if parked/deleted)
    uuid_to_entity: HashMap<Uuid, Option<Entity>>,
    /// Reverse map from Entity to UUID (for quick lookups)
    entity_to_uuid: HashMap<Entity, Uuid>,
}

impl StableIdRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new entity with its stable ID
    ///
    /// Call this when spawning any timeline object (clip, track, timeline, effect)
    pub fn register(&mut self, uuid: Uuid, entity: Entity) {
        self.uuid_to_entity.insert(uuid, Some(entity));
        self.entity_to_uuid.insert(entity, uuid);
        tracing::trace!(
            "Registered entity {:?} with UUID {}",
            entity,
            &uuid.to_string()[..8]
        );
    }

    /// Get the current entity for a UUID
    ///
    /// Returns None if the UUID is not registered or is parked (deleted)
    pub fn get(&self, uuid: &Uuid) -> Option<Entity> {
        self.uuid_to_entity.get(uuid).copied().flatten()
    }

    /// Get the UUID for an entity
    ///
    /// Returns None if the entity is not registered
    pub fn get_uuid(&self, entity: Entity) -> Option<Uuid> {
        self.entity_to_uuid.get(&entity).copied()
    }

    /// Check if a UUID is registered (may or may not be active)
    pub fn contains(&self, uuid: &Uuid) -> bool {
        self.uuid_to_entity.contains_key(uuid)
    }

    /// Check if a UUID is active (registered and not parked)
    pub fn is_active(&self, uuid: &Uuid) -> bool {
        self.uuid_to_entity
            .get(uuid)
            .map(|e| e.is_some())
            .unwrap_or(false)
    }

    /// Park a UUID (mark as deleted but keep the mapping)
    ///
    /// Call this when deleting an entity or during undo.
    /// The UUID→None mapping is preserved so redo can restore it.
    pub fn park(&mut self, uuid: &Uuid) {
        if let Some(entry) = self.uuid_to_entity.get_mut(uuid) {
            if let Some(entity) = *entry {
                self.entity_to_uuid.remove(&entity);
            }
            *entry = None;
            tracing::trace!("Parked UUID {}", &uuid.to_string()[..8]);
        }
    }

    /// Restore a parked UUID with a new entity
    ///
    /// Call this when redoing a creation or undoing a deletion.
    /// The new entity may have a different ID than the original.
    pub fn restore(&mut self, uuid: Uuid, entity: Entity) {
        // Remove old entity mapping if exists
        if let Some(Some(old_entity)) = self.uuid_to_entity.get(&uuid) {
            self.entity_to_uuid.remove(old_entity);
        }

        self.uuid_to_entity.insert(uuid, Some(entity));
        self.entity_to_uuid.insert(entity, uuid);
        tracing::trace!(
            "Restored UUID {} to entity {:?}",
            &uuid.to_string()[..8],
            entity
        );
    }

    /// Unregister a UUID completely (for permanent deletion)
    ///
    /// Use sparingly - usually prefer `park()` to allow undo.
    pub fn unregister(&mut self, uuid: &Uuid) {
        if let Some(Some(entity)) = self.uuid_to_entity.remove(uuid) {
            self.entity_to_uuid.remove(&entity);
        }
    }

    /// Get all active entities (for debugging/iteration)
    pub fn active_entities(&self) -> impl Iterator<Item = (Uuid, Entity)> + '_ {
        self.uuid_to_entity
            .iter()
            .filter_map(|(uuid, entity)| entity.map(|e| (*uuid, e)))
    }

    /// Get count of registered UUIDs (including parked)
    pub fn len(&self) -> usize {
        self.uuid_to_entity.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.uuid_to_entity.is_empty()
    }
}

// ============================================================================
// PLAYBACK
// ============================================================================

/// Playhead state - current position in the timeline
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct Playhead {
    /// Current time position
    pub current_time: Duration,
    /// Current frame number
    pub current_frame: u64,
    /// Whether playback is active
    pub is_playing: bool,
    /// Playback speed multiplier (1.0 = normal, 0.5 = half, 2.0 = double)
    pub playback_speed: f32,
    /// Loop mode
    pub loop_mode: LoopMode,
    /// In point for loop (if loop_mode is InOut)
    pub loop_in: Option<Duration>,
    /// Out point for loop (if loop_mode is InOut)
    pub loop_out: Option<Duration>,
}

impl Default for Playhead {
    fn default() -> Self {
        Self {
            current_time: Duration::ZERO,
            current_frame: 0,
            is_playing: false,
            playback_speed: 1.0,
            loop_mode: LoopMode::None,
            loop_in: None,
            loop_out: None,
        }
    }
}

impl Playhead {
    /// Set playhead to a specific frame
    pub fn seek_to_frame(&mut self, frame: u64, frame_rate: f64) {
        self.current_frame = frame;
        self.current_time = Duration::from_secs_f64(frame as f64 / frame_rate);
    }

    /// Set playhead to a specific time
    pub fn seek_to_time(&mut self, time: Duration, frame_rate: f64) {
        self.current_time = time;
        self.current_frame = (time.as_secs_f64() * frame_rate) as u64;
    }

    /// Advance playhead by delta time
    pub fn advance(&mut self, delta: Duration, frame_rate: f64, timeline_duration: Duration) {
        let effective_delta =
            Duration::from_secs_f64(delta.as_secs_f64() * self.playback_speed as f64);

        let new_time = self.current_time + effective_delta;

        // Handle looping
        let new_time = match self.loop_mode {
            LoopMode::None => {
                if new_time >= timeline_duration {
                    self.is_playing = false;
                    timeline_duration
                } else {
                    new_time
                }
            }
            LoopMode::Timeline => {
                if new_time >= timeline_duration {
                    Duration::ZERO
                } else {
                    new_time
                }
            }
            LoopMode::InOut => {
                let in_point = self.loop_in.unwrap_or(Duration::ZERO);
                let out_point = self.loop_out.unwrap_or(timeline_duration);
                if new_time >= out_point {
                    in_point
                } else {
                    new_time
                }
            }
        };

        self.seek_to_time(new_time, frame_rate);
    }
}

/// Loop mode for playback
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LoopMode {
    /// No looping - stop at end
    #[default]
    None,
    /// Loop entire timeline
    Timeline,
    /// Loop between in/out points
    InOut,
}

/// Playback state - buffering and decode info
#[derive(Resource, Debug, Clone, Default)]
pub struct PlaybackState {
    /// Range of frames currently buffered
    pub buffered_frames: Option<RangeInclusive<u64>>,
    /// Number of frames to decode ahead
    pub decode_ahead: u64,
    /// Current decode status
    pub decode_status: DecodeStatus,
}

/// Status of video decode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecodeStatus {
    #[default]
    Idle,
    Decoding,
    Buffering,
    Error,
}

impl PlaybackState {
    /// Create with custom decode-ahead
    pub fn with_decode_ahead(decode_ahead: u64) -> Self {
        Self {
            buffered_frames: None,
            decode_ahead,
            decode_status: DecodeStatus::Idle,
        }
    }

    /// Check if a frame is buffered
    pub fn is_frame_buffered(&self, frame: u64) -> bool {
        self.buffered_frames
            .as_ref()
            .map(|range| range.contains(&frame))
            .unwrap_or(false)
    }
}

// ============================================================================
// SCRUBBING STATE
// ============================================================================

/// Scrubbing state - tracks when user is rapidly seeking the timeline
///
/// When scrubbing is active, the render pipeline uses optimized decode:
/// 1. Cancels stale decode requests (only latest position matters)
/// 2. Shows last decoded frame while new frame decodes
/// 3. Avoids loading spinner during rapid seeking
///
/// This dramatically improves timeline scrubbing responsiveness.
#[derive(Resource, Debug, Clone, Default)]
pub struct ScrubState {
    /// Whether the user is currently scrubbing (dragging playhead)
    pub is_scrubbing: bool,
    /// Last successfully decoded frame time (for fallback display)
    pub last_decoded_time: Option<Duration>,
}

impl ScrubState {
    /// Enter scrubbing mode
    pub fn start_scrub(&mut self) {
        self.is_scrubbing = true;
        tracing::trace!("Started scrubbing");
    }

    /// Exit scrubbing mode
    pub fn end_scrub(&mut self) {
        self.is_scrubbing = false;
        tracing::trace!("Ended scrubbing");
    }

    /// Update last decoded time (call when frame decodes successfully)
    pub fn set_last_decoded(&mut self, time: Duration) {
        self.last_decoded_time = Some(time);
    }
}

// ============================================================================
// SELECTION
// ============================================================================

/// Selection state
#[derive(Resource, Debug, Clone, Default)]
pub struct Selection {
    /// All selected entity IDs (as u64 for serialization)
    pub selected: HashSet<u64>,
    /// Primary selected entity (for multi-select operations)
    pub primary: Option<u64>,
}

impl Selection {
    /// Clear all selections
    pub fn clear(&mut self) {
        self.selected.clear();
        self.primary = None;
    }

    /// Select an entity
    pub fn select(&mut self, entity_bits: u64) {
        self.selected.insert(entity_bits);
        if self.primary.is_none() {
            self.primary = Some(entity_bits);
        }
    }

    /// Select only this entity (clear others)
    pub fn select_exclusive(&mut self, entity_bits: u64) {
        self.selected.clear();
        self.selected.insert(entity_bits);
        self.primary = Some(entity_bits);
    }

    /// Deselect an entity
    pub fn deselect(&mut self, entity_bits: u64) {
        self.selected.remove(&entity_bits);
        if self.primary == Some(entity_bits) {
            self.primary = self.selected.iter().next().copied();
        }
    }

    /// Toggle selection
    pub fn toggle(&mut self, entity_bits: u64) {
        if self.selected.contains(&entity_bits) {
            self.deselect(entity_bits);
        } else {
            self.select(entity_bits);
        }
    }

    /// Check if an entity is selected
    pub fn is_selected(&self, entity_bits: u64) -> bool {
        self.selected.contains(&entity_bits)
    }

    /// Get count of selected entities
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Check if selection is empty
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
}

// ============================================================================
// TRIM MODE
// ============================================================================

/// Trim mode type - determines how trim operations affect the timeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TrimModeType {
    /// Normal trim - no ripple, gaps allowed
    #[default]
    Normal,
    /// Ripple trim - shift subsequent clips to close/open gaps
    Ripple,
    /// Roll trim - adjust boundary between two adjacent clips
    Roll,
}

/// Current trim mode for the timeline
///
/// Controls how trim operations behave:
/// - **Normal**: Trim creates gaps or doesn't affect other clips
/// - **Ripple**: Subsequent clips shift to close/open gaps (magnetic timeline)
/// - **Roll**: Adjust boundary between adjacent clips (one shrinks, other grows)
#[derive(Resource, Debug, Clone, Default)]
pub struct TrimMode {
    /// Current trim mode
    pub mode: TrimModeType,
}

impl TrimMode {
    /// Create with specific mode
    pub fn with_mode(mode: TrimModeType) -> Self {
        Self { mode }
    }

    /// Check if in ripple mode
    pub fn is_ripple(&self) -> bool {
        self.mode == TrimModeType::Ripple
    }

    /// Check if in roll mode  
    pub fn is_roll(&self) -> bool {
        self.mode == TrimModeType::Roll
    }
}

// ============================================================================
// COMMAND QUEUE
// ============================================================================

/// Command queue resource
#[derive(Resource)]
pub struct CommandQueue {
    queue: Arc<RwLock<Vec<Box<dyn crate::commands::Command>>>>,
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Submit a command to be executed
    pub fn submit(&self, command: Box<dyn crate::commands::Command>) {
        self.queue.write().push(command);
    }

    /// Take all pending commands for execution
    pub fn take_all(&self) -> Vec<Box<dyn crate::commands::Command>> {
        std::mem::take(&mut *self.queue.write())
    }

    /// Check if there are pending commands
    pub fn is_empty(&self) -> bool {
        self.queue.read().is_empty()
    }

    /// Get number of pending commands
    pub fn len(&self) -> usize {
        self.queue.read().len()
    }
}

// ============================================================================
// UNDO/REDO STACK
// ============================================================================

/// Undo/Redo stack resource
#[derive(Resource)]
pub struct UndoRedoStack {
    inner: UndoStack,
}

impl Default for UndoRedoStack {
    fn default() -> Self {
        Self {
            inner: UndoStack::new(100), // 100 undo levels
        }
    }
}

impl UndoRedoStack {
    /// Create with custom max size
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            inner: UndoStack::new(max_size),
        }
    }

    pub fn push(&mut self, entry: UndoEntry) {
        self.inner.push(entry);
    }

    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }

    pub fn pop_for_undo(&mut self) -> Option<UndoEntry> {
        self.inner.pop_for_undo()
    }

    pub fn push_to_redo(&mut self, entry: UndoEntry) {
        self.inner.push_to_redo(entry);
    }

    pub fn pop_for_redo(&mut self) -> Option<UndoEntry> {
        self.inner.pop_for_redo()
    }

    pub fn undo_name(&self) -> Option<&'static str> {
        self.inner.undo_name()
    }

    pub fn redo_name(&self) -> Option<&'static str> {
        self.inner.redo_name()
    }

    /// Push a redone entry back to the undo stack (without clearing redo)
    pub fn push_from_redo(&mut self, entry: UndoEntry) {
        self.inner.push_from_redo(entry);
    }

    // NOTE: remap_entity_in_redo() has been removed as part of the UUID-based undo/redo refactor.
    // Commands now lookup entities by UUID via StableIdRegistry, making entity ID remapping unnecessary.
}

// ============================================================================
// KEYBOARD SHORTCUTS
// ============================================================================

/// Key modifiers for shortcut combinations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Ctrl/Command key
    pub ctrl: bool,
    /// Shift key
    pub shift: bool,
    /// Alt/Option key
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        ctrl: false,
        shift: true,
        alt: false,
    };
    pub const ALT: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: true,
    };
    pub const CTRL_SHIFT: Modifiers = Modifiers {
        ctrl: true,
        shift: true,
        alt: false,
    };

    pub fn any(&self) -> bool {
        self.ctrl || self.shift || self.alt
    }
}

/// A keyboard key identifier
///
/// Using string representation for serialization and flexibility.
/// Maps to egui::Key names (e.g., "A", "B", "Delete", "Space", "F1").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCode(pub String);

impl KeyCode {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Common keys
    pub const V: &'static str = "V";
    pub const B: &'static str = "B";
    pub const Y: &'static str = "Y";
    pub const U: &'static str = "U";
    pub const S: &'static str = "S";
    pub const M: &'static str = "M";
    pub const Z: &'static str = "Z";
    pub const DELETE: &'static str = "Delete";
    pub const SPACE: &'static str = "Space";
    pub const ESCAPE: &'static str = "Escape";
}

/// A keyboard shortcut combination (key + modifiers)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombination {
    pub key: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyCombination {
    pub fn new(key: impl Into<String>, modifiers: Modifiers) -> Self {
        Self {
            key: KeyCode::new(key),
            modifiers,
        }
    }

    /// Create a shortcut with no modifiers
    pub fn key(key: impl Into<String>) -> Self {
        Self::new(key, Modifiers::NONE)
    }

    /// Create Ctrl+key shortcut
    pub fn ctrl(key: impl Into<String>) -> Self {
        Self::new(key, Modifiers::CTRL)
    }

    /// Create Shift+key shortcut
    pub fn shift(key: impl Into<String>) -> Self {
        Self::new(key, Modifiers::SHIFT)
    }

    /// Create Ctrl+Shift+key shortcut
    pub fn ctrl_shift(key: impl Into<String>) -> Self {
        Self::new(key, Modifiers::CTRL_SHIFT)
    }

    /// Human-readable display (e.g., "Ctrl+Z", "Shift+Delete")
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.ctrl {
            parts.push("Ctrl");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        parts.push(&self.key.0);
        parts.join("+")
    }
}

/// Action triggered by a keyboard shortcut
#[derive(Debug, Clone)]
pub enum ShortcutAction {
    /// Execute a named command (dispatched via CommandQueue)
    Command(String),
    /// Switch to a tool mode
    ToolSwitch(String),
    /// Undo operation
    Undo,
    /// Redo operation
    Redo,
    /// Toggle play/pause
    TogglePlayback,
    /// Split at playhead
    SplitAtPlayhead,
    /// Delete selected clips
    DeleteSelected,
    /// Ripple delete selected clips
    RippleDeleteSelected,
    /// Add marker at playhead
    AddMarker,
}

impl ShortcutAction {
    /// Get human-readable name for the action
    pub fn display_name(&self) -> &'static str {
        match self {
            ShortcutAction::Command(_) => "Execute Command",
            ShortcutAction::ToolSwitch(_) => "Switch Tool",
            ShortcutAction::Undo => "Undo",
            ShortcutAction::Redo => "Redo",
            ShortcutAction::TogglePlayback => "Play/Pause",
            ShortcutAction::SplitAtPlayhead => "Split at Playhead",
            ShortcutAction::DeleteSelected => "Delete",
            ShortcutAction::RippleDeleteSelected => "Ripple Delete",
            ShortcutAction::AddMarker => "Add Marker",
        }
    }
}

/// Keyboard shortcut registry
///
/// Stores mappings from key combinations to actions.
/// This is a global ECS Resource for centralized shortcut management.
///
/// ## Default Shortcuts
///
/// | Key | Action |
/// |-----|--------|
/// | Ctrl+Z | Undo |
/// | Ctrl+Shift+Z | Redo |
/// | Space | Play/Pause |
/// | Delete | Delete selected |
/// | Shift+Delete | Ripple delete |
/// | S | Split at playhead |
/// | V | Pointer tool |
/// | B | Razor tool |
/// | Y | Slip tool |
/// | U | Slide tool |
/// | M | Add marker |
#[derive(Resource, Debug, Clone)]
pub struct KeyboardShortcuts {
    bindings: std::collections::HashMap<KeyCombination, ShortcutAction>,
}

impl Default for KeyboardShortcuts {
    fn default() -> Self {
        let mut shortcuts = Self {
            bindings: std::collections::HashMap::new(),
        };
        shortcuts.register_defaults();
        shortcuts
    }
}

impl KeyboardShortcuts {
    /// Create an empty shortcut registry
    pub fn new() -> Self {
        Self {
            bindings: std::collections::HashMap::new(),
        }
    }

    /// Register default shortcuts
    pub fn register_defaults(&mut self) {
        // Undo/Redo
        self.bind(KeyCombination::ctrl("Z"), ShortcutAction::Undo);
        self.bind(KeyCombination::ctrl_shift("Z"), ShortcutAction::Redo);

        // Playback
        self.bind(KeyCombination::key("Space"), ShortcutAction::TogglePlayback);

        // Editing
        self.bind(
            KeyCombination::key("Delete"),
            ShortcutAction::DeleteSelected,
        );
        self.bind(
            KeyCombination::shift("Delete"),
            ShortcutAction::RippleDeleteSelected,
        );
        self.bind(KeyCombination::key("S"), ShortcutAction::SplitAtPlayhead);
        self.bind(KeyCombination::key("M"), ShortcutAction::AddMarker);

        // Tool switching
        self.bind(
            KeyCombination::key("V"),
            ShortcutAction::ToolSwitch("Pointer".to_string()),
        );
        self.bind(
            KeyCombination::key("B"),
            ShortcutAction::ToolSwitch("Razor".to_string()),
        );
        self.bind(
            KeyCombination::key("Y"),
            ShortcutAction::ToolSwitch("Slip".to_string()),
        );
        self.bind(
            KeyCombination::key("U"),
            ShortcutAction::ToolSwitch("Slide".to_string()),
        );
    }

    /// Bind a key combination to an action
    pub fn bind(&mut self, combination: KeyCombination, action: ShortcutAction) {
        self.bindings.insert(combination, action);
    }

    /// Remove a binding
    pub fn unbind(&mut self, combination: &KeyCombination) {
        self.bindings.remove(combination);
    }

    /// Get action for a key combination
    pub fn get_action(&self, combination: &KeyCombination) -> Option<&ShortcutAction> {
        self.bindings.get(combination)
    }

    /// Get all bindings (for preferences UI)
    pub fn all_bindings(&self) -> impl Iterator<Item = (&KeyCombination, &ShortcutAction)> {
        self.bindings.iter()
    }

    /// Check for conflicting bindings
    pub fn has_conflict(&self, combination: &KeyCombination) -> bool {
        self.bindings.contains_key(combination)
    }

    /// Get display string for an action (if bound)
    pub fn shortcut_for_action(&self, action_name: &str) -> Option<String> {
        for (combo, action) in &self.bindings {
            if action.display_name() == action_name {
                return Some(combo.display());
            }
        }
        None
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playhead_advance() {
        let mut playhead = Playhead::default();
        playhead.is_playing = true;

        playhead.advance(Duration::from_millis(100), 30.0, Duration::from_secs(60));

        assert!(playhead.current_time > Duration::ZERO);
    }

    #[test]
    fn test_playhead_loop() {
        let mut playhead = Playhead::default();
        playhead.loop_mode = LoopMode::Timeline;
        playhead.current_time = Duration::from_secs(59);

        playhead.advance(Duration::from_secs(2), 30.0, Duration::from_secs(60));

        // Should loop back to start
        assert!(playhead.current_time < Duration::from_secs(1));
    }

    #[test]
    fn test_selection() {
        let mut selection = Selection::default();

        selection.select(1);
        selection.select(2);

        assert!(selection.is_selected(1));
        assert!(selection.is_selected(2));
        assert_eq!(selection.count(), 2);

        selection.deselect(1);
        assert!(!selection.is_selected(1));
        assert_eq!(selection.count(), 1);
    }

    #[test]
    fn test_keyboard_shortcuts_default() {
        let shortcuts = KeyboardShortcuts::default();

        // Verify default shortcuts exist
        let undo_combo = KeyCombination::ctrl("Z");
        assert!(shortcuts.get_action(&undo_combo).is_some());

        let space_combo = KeyCombination::key("Space");
        assert!(shortcuts.get_action(&space_combo).is_some());
    }

    #[test]
    fn test_keyboard_shortcuts_bind_unbind() {
        let mut shortcuts = KeyboardShortcuts::new();

        let combo = KeyCombination::key("X");
        assert!(shortcuts.get_action(&combo).is_none());

        shortcuts.bind(combo.clone(), ShortcutAction::DeleteSelected);
        assert!(shortcuts.get_action(&combo).is_some());

        shortcuts.unbind(&combo);
        assert!(shortcuts.get_action(&combo).is_none());
    }

    #[test]
    fn test_key_combination_display() {
        let ctrl_z = KeyCombination::ctrl("Z");
        assert_eq!(ctrl_z.display(), "Ctrl+Z");

        let shift_del = KeyCombination::shift("Delete");
        assert_eq!(shift_del.display(), "Shift+Delete");

        let ctrl_shift_z = KeyCombination::ctrl_shift("Z");
        assert_eq!(ctrl_shift_z.display(), "Ctrl+Shift+Z");
    }

    #[test]
    fn test_keyboard_shortcuts_conflict() {
        let shortcuts = KeyboardShortcuts::default();

        // Existing binding should be detected as conflict
        let ctrl_z = KeyCombination::ctrl("Z");
        assert!(shortcuts.has_conflict(&ctrl_z));

        // Non-existing binding should not be a conflict
        let alt_q = KeyCombination::new("Q", Modifiers::ALT);
        assert!(!shortcuts.has_conflict(&alt_q));
    }
}

// ============================================================================
// VIEWPORT
// ============================================================================

/// A single clip's texture data for per-clip GPU quad rendering.
///
/// Each visible clip at the current playhead time gets its own entry.
/// The viewport renders each as an independent quad with its own Transform2D.
///
/// ## Why Per-Clip Textures?
///
/// Professional NLEs (Canva, CapCut, DaVinci) render each clip independently
/// so that per-clip transforms (position, scale, rotation) are applied correctly.
/// CPU alpha-compositing into a single texture loses per-clip spatial transforms.
#[derive(Debug, Clone)]
pub struct ClipTextureLayer {
    /// egui texture ID for this clip's decoded frame
    pub texture_id: egui::TextureId,
    /// UUID of the source clip (used to resolve Transform2D from ECS)
    pub clip_uuid: uuid::Uuid,
    /// Native video frame resolution (for aspect-ratio fitting)
    pub source_resolution: glam::UVec2,
    /// Clip opacity (0.0 - 1.0)
    pub opacity: f32,
}

/// Viewport display state
///
/// Tracks what's currently displayed in the video preview viewport.
#[derive(Resource, Debug, Clone)]
pub struct ViewportState {
    /// Currently loaded video asset entity
    pub loaded_asset: Option<u64>,
    /// Current frame texture ID for display (topmost layer, backward compat)
    pub current_texture: Option<egui::TextureId>,
    /// Per-clip textures for multi-layer GPU quad rendering.
    /// Each entry represents one visible clip at the current playhead time.
    /// Ordered bottom-to-top (first = background, last = foreground).
    /// Empty when no clips are active or during loading.
    pub clip_textures: Vec<ClipTextureLayer>,
    /// Current frame number being displayed
    pub current_frame: u64,
    /// Zoom level (1.0 = fit to viewport)
    pub zoom: f32,
    /// Pan offset from center (in normalized coordinates)
    pub pan: glam::Vec2,
    /// Source video resolution
    pub source_resolution: glam::UVec2,
    /// Is currently loading a frame?
    pub is_loading: bool,
    /// Error message if load failed
    pub error: Option<String>,
    /// Is playhead in a gap between clips? (show black, not placeholder)
    /// When true AND current_texture is None, show black frame.
    /// When false AND current_texture is None, show "drop video here".
    pub in_gap: bool,
    /// Active clip's speed at the current playhead position.
    /// Written by the frame compositor, read by audio_sync_system.
    /// This propagates per-clip speed to the audio callback.
    pub active_clip_speed: f32,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            loaded_asset: None,
            current_texture: None,
            clip_textures: Vec::new(),
            current_frame: 0,
            zoom: 1.0,
            pan: glam::Vec2::ZERO,
            source_resolution: glam::UVec2::new(1920, 1080),
            is_loading: false,
            error: None,
            in_gap: false,
            active_clip_speed: 1.0,
        }
    }
}

impl ViewportState {
    /// Reset to no loaded asset
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
        if loading {
            self.error = None;
        }
    }

    /// Set error state
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.is_loading = false;
    }

    /// Update displayed frame
    pub fn set_frame(&mut self, frame: u64, texture_id: egui::TextureId) {
        self.current_frame = frame;
        self.current_texture = Some(texture_id);
        self.is_loading = false;
        self.error = None;
    }

    /// Calculate display rect maintaining aspect ratio
    pub fn calculate_display_rect(&self, available: egui::Rect) -> egui::Rect {
        let source_aspect = self.source_resolution.x as f32 / self.source_resolution.y as f32;
        let available_aspect = available.width() / available.height();

        let (display_width, display_height) = if source_aspect > available_aspect {
            // Source is wider - fit to width
            (available.width(), available.width() / source_aspect)
        } else {
            // Source is taller - fit to height
            (available.height() * source_aspect, available.height())
        };

        // Apply zoom
        let zoomed_width = display_width * self.zoom;
        let zoomed_height = display_height * self.zoom;

        // Center in available space with pan offset
        let center = available.center();
        let pan_pixels = egui::vec2(
            self.pan.x * available.width(),
            self.pan.y * available.height(),
        );

        egui::Rect::from_center_size(center + pan_pixels, egui::vec2(zoomed_width, zoomed_height))
    }

    /// Zoom in/out around a point
    pub fn zoom_at(&mut self, delta: f32, _point: glam::Vec2) {
        self.zoom = (self.zoom * (1.0 + delta)).clamp(0.1, 10.0);
    }

    /// Reset zoom and pan
    pub fn fit_to_viewport(&mut self) {
        self.zoom = 1.0;
        self.pan = glam::Vec2::ZERO;
    }
}

// ============================================================================
// TIMELINE VIEW STATE
// ============================================================================

/// Playhead follow mode during playback
///
/// Controls how the timeline scrolls to keep the playhead visible during playback.
/// Based on professional NLE behavior (Kdenlive, DaVinci, Premiere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlayheadFollowMode {
    /// No auto-follow - timeline stays static, playhead may go off-screen
    None,
    /// Auto-scroll when playhead reaches edge (~80% of visible area)
    /// Like Premiere Pro default behavior
    AutoScroll,
    /// Playhead stays centered, timeline scrolls under it
    /// Like DaVinci Resolve "Fixed Playhead" mode
    #[default]
    Centered,
}

/// Timeline view state
///
/// Tracks the current view state of the timeline panel: zoom level, scroll position,
/// and playhead follow behavior. This is a UI-only resource, not saved with project.
///
/// ## Playhead Follow Modes
///
/// - **None**: Timeline remains static during playback. May need manual scroll.
/// - **AutoScroll**: When playhead reaches ~80% of visible area, scroll to keep visible.
/// - **Centered** (default): Playhead stays centered, timeline scrolls under it.
#[derive(Resource, Debug, Clone)]
pub struct TimelineViewState {
    /// Current zoom level (pixels per second)
    /// MIN_ZOOM = 5.0, MAX_ZOOM = 500.0
    pub zoom: f32,
    /// Horizontal scroll offset in seconds
    pub scroll_offset: f64,
    /// Vertical scroll offset in pixels (for many tracks)
    pub scroll_y: f32,
    /// How the timeline follows the playhead during playback
    pub follow_mode: PlayheadFollowMode,
    /// Whether the timeline is currently being dragged/scrolled by user
    /// (suspends auto-follow temporarily)
    pub user_scrolling: bool,
}

impl Default for TimelineViewState {
    fn default() -> Self {
        Self {
            zoom: 100.0, // 100 pixels per second
            scroll_offset: 0.0,
            scroll_y: 0.0,
            follow_mode: PlayheadFollowMode::Centered, // User's preferred default
            user_scrolling: false,
        }
    }
}

impl TimelineViewState {
    /// Update scroll to keep playhead visible based on follow mode
    ///
    /// Call this during playback to auto-scroll the timeline.
    ///
    /// # Arguments
    /// - `playhead_time`: Current playhead position in seconds
    /// - `visible_width`: Width of the timeline content area in pixels
    ///
    /// Returns true if scroll was updated.
    pub fn follow_playhead(&mut self, playhead_time: f64, visible_width: f32) -> bool {
        if self.user_scrolling {
            return false; // User is manually scrolling, don't interfere
        }

        let visible_duration = visible_width as f64 / self.zoom as f64;
        let playhead_in_view = playhead_time - self.scroll_offset;

        match self.follow_mode {
            PlayheadFollowMode::None => false,

            PlayheadFollowMode::AutoScroll => {
                // Scroll when playhead reaches 80% of visible area
                let trigger_point = visible_duration * 0.8;
                if playhead_in_view > trigger_point {
                    // Scroll to put playhead at 20% from left
                    self.scroll_offset = playhead_time - (visible_duration * 0.2);
                    true
                } else if playhead_in_view < 0.0 {
                    // Playhead went backwards past left edge
                    self.scroll_offset = playhead_time;
                    true
                } else {
                    false
                }
            }

            PlayheadFollowMode::Centered => {
                // Keep playhead centered - scroll proportionally
                let center_offset = playhead_time - (visible_duration / 2.0);
                let new_scroll = center_offset.max(0.0);
                if (self.scroll_offset - new_scroll).abs() > 0.001 {
                    self.scroll_offset = new_scroll;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Apply zoom change, clamping to valid range
    pub fn set_zoom(&mut self, new_zoom: f32) {
        const MIN_ZOOM: f32 = 5.0;
        const MAX_ZOOM: f32 = 500.0;
        self.zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Zoom in/out by a factor, optionally around a specific time point
    pub fn zoom_by(&mut self, factor: f32, around_time: Option<f64>) {
        let old_zoom = self.zoom;
        self.set_zoom(old_zoom * factor);

        // Adjust scroll to keep the focal point in place
        if let Some(time) = around_time {
            let zoom_ratio = self.zoom / old_zoom;
            let time_from_scroll = time - self.scroll_offset;
            self.scroll_offset = time - (time_from_scroll / zoom_ratio as f64);
            self.scroll_offset = self.scroll_offset.max(0.0);
        }
    }
}

// ============================================================================
// SERDE HELPERS
// ============================================================================

/// Serde helper for Option<Duration> - serializes as Option<f64> (seconds)
pub mod duration_option_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.map(|d| d.as_secs_f64()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds: Option<f64> = Option::deserialize(deserializer)?;
        Ok(seconds.map(Duration::from_secs_f64))
    }
}
