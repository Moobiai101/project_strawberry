---
trigger: always_on
---

1. YOU ARE MY COMPANY'S CO-FOUNDER, SO TREAT IT AS YOUR OWN STARTUP.
2. ALWAYS WRITE PRODUCTION GRADE CODE, AND ALWAYS PREFER QUALITY OR QUANTITY.
3. YOU MUST NEVER IMPEMENT ANY WORKAROUNDS OR TEMPORARY FIXES THAT COULD BREAK THE CODE LATER IN THE DEVELOPMENT PHASES, SO ALWAYS IMPLEMET PERMANENET FIXES
4. VERY CRITICAL-- ALWAYS CHECK WHAT IS IN THE CODEBASE FIRST AND UNDERSTAND ITS ARCHITECTURE FULLY BEFORE MAKING OR ADDING ANY FEATURES OR FIXING A BUG, NEVER START RIGHT AWAY, FIRST U MUST KNOW WHAT IS CURRENT STATE OF OUR RUST PROJECT AND SHOULD GET CONTEXT OF WHAT IS ALREADY THERE , SO THAT U DONT MAKE DUPLICATE FUNCTIONS, LOGICS OR EVEN FILES.

5. Also when u ARE implement things, dont think that it is gonna take time i should just take shortcut and implement a workaround, never EVER do that, it can cause issue in future


# Project Strawberry: AI-Native Engineering Standards

> **"The code is the prompt."**
> This project is built to be read, understood, and modified by AI Agents.

## 1. The Golden Rules of Context Efficiency

AI models have limited context windows (even "infinite" context degrades in reasoning quality). To maximize agent performance, we adhere to these strict rules:

### 1.1 The 400-Line Soft Limit
- **Rule:** No source file should exceed 400 lines of code.
- **Why:** Agents get lost in massive files. Small files mean specific context retrieval.
- **Action:** If a file grows >400 lines, refactor into sub-modules.
  - *Bad:* `timeline.rs` (3000 lines) with all logic.
  - *Good:* `timeline/mod.rs`, `timeline/rendering.rs`, `timeline/input.rs`.

AI agents operate under finite attention and reasoning depth. To preserve correctness, locality, and architectural clarity, the following rules are non-negotiable.

1. File Size Discipline
1.1 The 400-Line Soft Limit (Mandatory)

Rule

No source file should exceed 400 lines of code.

Rationale

Beyond ~400 lines, reasoning accuracy degrades sharply.

Smaller files enable precise retrieval, isolated reasoning, and safer refactors.

At >400 lines, refactoring is mandatory, though if it is mandatory to increase the limit u can do it upto 600 to 700 lines

Files must never exceed 600- 800 lines under any circumstance.

1.2 Pre-Implementation Reasoning Requirement (Critical)

Before writing code, the agent must explicitly reason:

“Is this feature likely to push the file beyond 400 lines?”

If NO: proceed normally.

If YES:

The agent must plan submodules first

The agent must name and define file responsibilities

Only then may implementation begin

Failure to plan before exceeding limits is considered an error.

### 1.2 strict Separation of Concerns (ECS)
- **Core (Logic/Data)**: `crates/strawberry-core`
  - *Must never import UI libraries (egui, winit).*
  - Contains: Components, Resources, Systems, Commands.
- **UI (Presentation)**: `crates/strawberry-ui`
  - *Must never contain business logic.*
  - Contains: Widgets, Input handling, View-only systems.
- **Why:** Agents can reason about "What happens" (Logic) separately from "How it looks" (UI).

### 1.3 The "Soul" File Pattern
- **Rule:** Every complex module (crate or major feature) must have a detailed `mod.rs` or `README.md` at its root.
- **Content:** strict high-level explanation of the module's purpose, data flow, and key structs.
- **Why:** Allows agents to understand a module without reading every child file.

u must edit the readme.md file evry time we make changes to that module.

## 2. Coding Patterns for AI Reliability

### 2.1 The Command Pattern (Immutable Law)
- **Rule:** ALL state mutations must happen via `Command` structs.
- **Implementation:**
  ```rust
  #[derive(Serialize, Deserialize)]
  pub struct MyActionCommand {
      pub target_uuid: Uuid, // Store UUID, not Entity
      // ...
  }
  
  impl Command for MyActionCommand {
      fn name(&self) -> &'static str { "My Action" }
      
      fn execute(&mut self, world: &mut World) -> Result<UndoData, CommandError> {
          // Resolve UUID to Entity at runtime
          let entity = world.resource::<StableIdRegistry>()
              .get(&self.target_uuid)
              .ok_or(CommandError::EntityNotFound(...))?;
              
          // Mutate...
          Ok(UndoData::...)
      }

      fn undo(&mut self, world: &mut World, undo_data: UndoData) -> Result<(), CommandError> { ... }
  }
  ```
- **Why:** 
  1. AI can generate JSON commands easily.
  2. Free Undo/Redo.
  3. Execution history provides "training data" for the agent's memory.

### 2.2 Explicit Typing Over Inference
- **Rule:** Use explicit types in function signatures and complex assignments.
- **Avoid:** `let x = something();` (if `something` is non-obvious)
- **Prefer:** `let x: MyType = something();`
- **Why:** drastically reduces hallucination by providing immediate type context in diffs and snippets.

### 2.3 Descriptive Naming (Verbose is Good)
- **Rule:** Variable/Function names should explain *what* and *why*.
- **Avoid:** `calc_val()`, `handle_it()`
- **Prefer:** `calculate_timeline_duration_frames()`, `handle_clip_selection_event()`
- **Why:** LLMs rely on semantic meaning. Ambiguous names lead to wrong assumptions.

### 2.4 Stable IDs (UUIDs)
- **Rule:** Never store raw `Entity` IDs in Commands or persistent structs. Use `Uuid`.
- **Reasoning:** ECS Entity IDs change after undo/redo (despawn/respawn). UUIDs remain constant.
- **Implementation:**
  - Attach `StableId` component to all persistent entities.
  - Use `StableIdRegistry` resource to map `Uuid` -> `Entity`.
  - When creating entities: `registry.register(uuid, entity)`.
  - When deleting entities: `registry.park(uuid)` (allows undo to restore same UUID).

## 3. Directory Structure Standards

```
crates/
  strawberry-core/       # The "Brain"
    src/
      components/        # Data-only structs
      systems/           # Logic (functions)
      commands/          # Actions (mutators)
  
  strawberry-ui/         # The "Face"
    src/
      widgets/           # Reusable UI elements
      panels/            # Structural layout
      
  strawberry-video/      # Service Layer (FFmpeg)
    # Stateless services only. No ECS code.
```

## 4. Documentation Strategy

### 4.1 "Context-First" Comments
- **Rule:** Doc comments (`///`) should explain *intent*, not just mechanics.
- **Why:** Helps the agent understand the "strategy" behind the code.

```rust
/// Calculates the pixel offset for the playhead.
/// 
/// We use a double-precision float to prevent jitter at high zoom levels
/// (deep zoom) which occurs with f32 rounding errors.
fn calculate_playhead_x(...)
```

## 5. Future Implementation Checklist

Before writing any new feature, the Agent must verify:
1. [ ] Is this logic or UI? (Put it in the right crate)
2. [ ] Does this require a new Command? (Never mutate World directly from UI)
3. [ ] Will this file be < 700 lines? (If not, plan sub-modules)
4. [ ] Are types explicit?
5. [ ] Is it covered by a test? (AI-generated tests are mandatory)

---
**Approved by:** Founder
**Last Updated:** 2026-01-24