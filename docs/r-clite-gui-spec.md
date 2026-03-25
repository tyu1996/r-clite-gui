# r-clite GUI Extension Spec

## 1. Objective
Build a new GUI-focused continuation of `r-clite` in repo `r-clite-gui`, while preserving the existing CLI editor behavior.

Primary goal: Ship a working GUI MVP that can open/edit/save files and uses the existing text-editing core logic where possible.

## 2. Repository + Fork Status
- Source repo: `https://github.com/tyu1996/r-clite`
- New repo: `https://github.com/tyu1996/r-clite-gui`
- Local remote added: `gui-origin -> https://github.com/tyu1996/r-clite-gui.git`

## 3. Current Code Entry Points and Extension Points

### 3.1 Binary target / startup flow
- `Cargo.toml` (`[[bin]]`): `rcte` at `src/main.rs`
- `src/main.rs`
  - `main()` handles top-level error output + exit code.
  - `run()` parses CLI args (`clap`), loads config, opens buffer, constructs `Editor`, calls `ed.run()`.

Why important:
- GUI should **not** replace this path initially. Add a second binary for GUI (`rcte-gui`) and keep CLI intact.

### 3.2 Command loop / editor core
- `src/editor.rs`
  - `Editor` struct is the central runtime state owner (buffer, cursor, scroll, undo/redo, search, prompt, messages, collab hooks).
  - `run()` is the read-input -> update-state -> render loop.
  - `handle_key()` is command dispatch.
  - Editing/search/navigation are implemented as methods on `Editor`.

Why important:
- This file currently mixes domain logic with terminal I/O assumptions. For GUI, extract reusable core state transitions from terminal-specific loop/input plumbing.

### 3.3 Rendering abstraction
- `src/ui.rs`
  - `RenderState` is the current frame contract passed from `Editor` to renderer.
  - `Ui::render()` converts `Buffer + RenderState` into terminal draw commands.

Why important:
- `RenderState` is the best seed for a renderer-agnostic snapshot contract. GUI renderer can consume a richer version of this model.

### 3.4 Input abstraction
- `src/keymap.rs`
  - `Command` enum defines editor actions.
  - `map(KeyEvent)` maps crossterm keys to commands.

Why important:
- Keep `Command` as cross-frontend action API.
- Add a GUI key mapping adapter to produce the same `Command` variants.

### 3.5 Text + persistence state model
- `src/buffer.rs`
  - Rope-backed text model (`ropey::Rope`) with cursor offset conversions, insert/delete primitives, search, dirty tracking, and save/open.

Why important:
- This is already frontend-agnostic and should be retained as-is for GUI MVP.

### 3.6 Terminal-specific boundary
- `src/terminal.rs`
  - Raw mode + alternate screen + terminal size.
- `src/file_picker.rs`
  - Native file picker wrappers (zenity/rfd), currently called via terminal suspend logic.

Why important:
- GUI path must avoid terminal raw-mode APIs.
- File picker logic can be reused directly through `rfd` in GUI flow.

## 4. GUI Library Decision

## Chosen library: `eframe` + `egui`

Rationale:
1. Architectural fit: current app already uses an immediate frame loop; egui is immediate-mode and maps naturally.
2. Rust-native, permissive licensing (MIT/Apache-2.0), no web stack required.
3. Fast path to MVP: one-process app with straightforward event/render/update integration.
4. Good ecosystem for custom drawing, which is needed for code-editor-like viewport behavior.

## Alternatives considered
- `iced`:
  - Pros: strong architecture, cross-platform, modern runtime.
  - Cons: upstream explicitly labels it experimental and learning curve is steeper; would require broader rewrite.
- `slint`:
  - Pros: strong cross-platform story and tooling.
  - Cons: license model is more complex than MIT/Apache baseline and UI DSL split adds integration overhead.
- `gtk4-rs`:
  - Pros: mature toolkit.
  - Cons: heavier native dependency surface and less aligned with current custom editor-style rendering model.

## 5. Target Architecture (Post-Refactor)

Keep a single workspace/repo for now, but split responsibilities:

- `src/core/` (new module group)
  - editor state machine and command handling (frontend-agnostic)
  - buffer/search/undo logic
  - render snapshot model
- `src/cli/` (or keep current files with minimal churn)
  - crossterm input adapter
  - terminal renderer
- `src/gui/` (new)
  - egui app wrapper
  - egui input adapter -> `Command`
  - egui renderer consuming core snapshot

Binary plan:
- Keep existing `rcte` binary unchanged for compatibility.
- Add new `rcte-gui` binary as GUI entry point.

## 6. Detailed Implementation Plan

### Phase 1: Core extraction (no behavior changes)
1. Introduce `EditorCore` (or similar) without terminal dependencies.
2. Move editing/search/navigation/save/open state transitions out of terminal loop code.
3. Keep `Command` enum as action API.
4. Add pure methods:
   - `apply_command(Command)`
   - `snapshot() -> EditorSnapshot`
5. Keep CLI adapter thin: read crossterm event -> `Command` -> `EditorCore` -> terminal render.

Deliverable:
- Existing CLI behavior still passes manual smoke checks.

### Phase 2: Add GUI binary skeleton
1. Add dependencies: `eframe`, `egui` (and reuse `rfd` for dialogs).
2. Create `src/bin/rcte-gui.rs`.
3. Implement `GuiApp` with:
   - app state: `EditorCore`
   - frame `update` method
   - menu/toolbar stubs (Open/Save/Save As)

Deliverable:
- `cargo run --bin rcte-gui` launches a window.

### Phase 3: GUI editor viewport MVP
1. Render visible lines, line numbers, cursor, status bar, message bar.
2. Map egui key input to existing `Command` variants.
3. Implement scrolling + cursor visibility logic using core offsets.
4. Integrate syntax highlighting spans (reuse/port `highlight.rs` output model).

Deliverable:
- User can type, navigate, search, undo/redo, and see status updates.

### Phase 4: File workflows + parity
1. Open file (native picker).
2. Save / Save As.
3. Dirty state in title/status and quit confirmation.
4. Config support reuse (`tab_width`, `line_numbers`, `theme`).

Deliverable:
- End-to-end open-edit-save works in GUI with parity to CLI MVP feature set.

### Phase 5: Testing and stabilization
1. Unit tests for core state transitions and cursor/offset invariants.
2. Snapshot-like tests for line layout math (viewport + scroll).
3. Manual smoke script for GUI interactions.
4. Keep CLI tests passing.

Deliverable:
- Repeatable quality gate for future GUI work.

## 7. Acceptance Criteria
- New binary `rcte-gui` exists and runs.
- GUI can open, edit, save plain text files.
- Core editor behavior (insert/delete/newline/tab/move/search/undo/redo) works in GUI.
- Existing `rcte` CLI remains functional.
- No terminal raw-mode code path is used by GUI.
- Documentation updated with run instructions for GUI binary.

## 8. Non-Goals
- Collaboration mode GUI integration.
- Multi-cursor editing beyond existing collab cursor visuals.
- Plugin system.
- LSP integration.
- Full IME/complex text shaping parity.

## 9. Risks and Mitigations
- Risk: domain logic remains coupled to terminal concerns.
  - Mitigation: enforce `EditorCore` without crossterm imports.
- Risk: viewport performance for large files.
  - Mitigation: render only visible rows; keep rope offset math incremental.
- Risk: input behavior drift between CLI and GUI.
  - Mitigation: shared `Command` action layer + shared core tests.
- Risk: feature creep.
  - Mitigation: enforce MVP scope and non-goals above.

## 10. Task Breakdown for Dev Team
1. Refactor owner: extract `EditorCore` + tests.
2. GUI owner: build `rcte-gui` eframe shell + input adapter.
3. Rendering owner: line layout, gutter, cursor, status/message rendering.
4. Integration owner: file dialogs, config, packaging docs.
5. QA owner: smoke checklist and regression verification.

## 11. Suggested First PR Sequence
1. PR-1: Core extraction only (no GUI).
2. PR-2: GUI shell + basic rendering of static buffer.
3. PR-3: Input mapping + editing commands.
4. PR-4: Open/save/search/undo parity + docs.
