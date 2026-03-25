# r-clite-gui: GUI Extension Spec

Date: 2026-03-23  
Repo baseline: `tyu1996/r-clite`  
Target repo: `tyu1996/r-clite-gui`

## Agent 1: Fork / Repo Preparation

### Outcome
- New GitHub repo verified: https://github.com/tyu1996/r-clite-gui
- Local remote configured and synchronized:
  - `gui -> git@github.com:tyu1996/r-clite-gui.git`
- Push status: `git push gui --all` and `git push gui --tags` both succeeded (`Everything up-to-date`).

### Notes
- GitHub API fork call on an already-owned source repo returned the original repo URL, so repo creation + push synchronization was used to produce the fork-equivalent under the same owner.

---

## Agent 2: Code Entry Points and Extension Points

## Runtime entry point
- [`src/main.rs`](/Users/brian/Documents/projects/r-clite/src/main.rs:42)
  - `main()` delegates to `run()`.
- [`src/main.rs`](/Users/brian/Documents/projects/r-clite/src/main.rs:49)
  - `run()` parses CLI, loads config, constructs `Buffer`, then constructs `Editor`, then calls `Editor::run()`.

## Core loop entry point
- [`src/editor.rs`](/Users/brian/Documents/projects/r-clite/src/editor.rs:171)
  - `Editor::run()` is the app loop: resize handling, optional collab event apply, render, input poll/read, command dispatch.
- [`src/editor.rs`](/Users/brian/Documents/projects/r-clite/src/editor.rs:336)
  - `handle_key()` maps input to domain commands through `keymap::map` and mutates editor state.

## Command model / input abstraction
- [`src/keymap.rs`](/Users/brian/Documents/projects/r-clite/src/keymap.rs:14)
  - `Command` enum is already a good frontend-agnostic action model.
- [`src/keymap.rs`](/Users/brian/Documents/projects/r-clite/src/keymap.rs:64)
  - Current mapper is terminal-event-specific (`crossterm::KeyEvent`), should be generalized for GUI event input.

## Rendering abstraction candidate
- [`src/ui.rs`](/Users/brian/Documents/projects/r-clite/src/ui.rs:20)
  - `RenderState` is close to a backend-neutral view model.
- [`src/ui.rs`](/Users/brian/Documents/projects/r-clite/src/ui.rs:93)
  - `Ui::render()` currently emits terminal draw commands and should become one backend implementation (`TerminalRenderer`).

## Terminal coupling to isolate
- [`src/editor.rs`](/Users/brian/Documents/projects/r-clite/src/editor.rs:129)
  - `RawModeGuard` and terminal size are initialized inside editor constructor.
- [`src/editor.rs`](/Users/brian/Documents/projects/r-clite/src/editor.rs:236)
  - Input polling/reading directly uses `crossterm::event`.
- [`src/terminal.rs`](/Users/brian/Documents/projects/r-clite/src/terminal.rs:25)
  - Raw mode / alternate screen management is terminal-specific and should remain in CLI frontend only.

## Editor/document model to preserve
- [`src/buffer.rs`](/Users/brian/Documents/projects/r-clite/src/buffer.rs:16)
  - `Buffer` + `ropey` gives robust Unicode-safe editing primitives.
- [`src/buffer.rs`](/Users/brian/Documents/projects/r-clite/src/buffer.rs:160)
  - Search primitives (`find_next`, `find_prev`) are reusable for GUI.
- [`src/highlight.rs`](/Users/brian/Documents/projects/r-clite/src/highlight.rs:38)
  - Syntax highlighting API already returns spans suitable for GUI text painting.

## OS picker integration
- [`src/file_picker.rs`](/Users/brian/Documents/projects/r-clite/src/file_picker.rs:12)
  - Linux `zenity` path.
- [`src/file_picker.rs`](/Users/brian/Documents/projects/r-clite/src/file_picker.rs:50)
  - Non-Linux uses `rfd` (already aligns with GUI use).

---

## Agent 3: GUI Library Decision

## Candidate comparison (based on architecture fit + current ecosystem)

### 1) `egui` / `eframe` (Recommended)
- Fit:
  - Immediate-mode model maps well to current `Editor::run` + per-frame `RenderState` pattern.
  - Easy custom painting for code viewport, cursor layer, search highlight, gutter.
  - Fast path to MVP with desktop support.
- Risks:
  - Built-in text editing widgets are not sufficient for a full code editor; custom editor viewport is required.
  - Need careful virtualization for very large files.
- Sources:
  - egui repo: https://github.com/emilk/egui

### 2) `iced`
- Fit:
  - Strong architecture and message/update separation.
  - Good cross-platform direction.
- Risks:
  - Larger rewrite toward Elm-style update/view architecture.
  - Custom code editor component still needed.
- Sources:
  - iced repo: https://github.com/iced-rs/iced
  - iced book: https://book.iced.rs

### 3) `Slint`
- Fit:
  - Strong native rendering and declarative UI.
  - Good for polished desktop app shells.
- Risks:
  - Integrating a high-performance custom code-editor surface is more specialized.
  - Web target trade-offs documented by Slint (canvas-based, limited web accessibility).
- Sources:
  - Slint docs: https://docs.slint.dev
  - Web platform trade-offs: https://docs.slint.dev/latest/docs/slint/guide/platforms/web/

### 4) `Dioxus` (Desktop)
- Fit:
  - Strong cross-platform story and productive DX.
- Risks:
  - More web-style component model than needed for this editor-first desktop use case.
  - Additional adaptation layer to reach low-level editor viewport behavior.
- Sources:
  - Dioxus crate docs: https://docs.rs/dioxus/latest/dioxus/

## Primary recommendation
- Choose `egui` + `eframe` for `r-clite-gui`.

## Why this is the best fit now
- Minimal conceptual mismatch with current loop/render architecture.
- Lowest migration risk to preserve existing buffer/edit/search/undo semantics.
- Fastest path to deliver usable GUI while keeping a single Rust codebase.

## Main risks to actively manage
- Rendering performance for large files (must virtualize visible rows).
- Keyboard parity with terminal behavior (must normalize shortcut mapping).
- Avoiding backend coupling regression (must isolate terminal-specific code).

---

## Detailed Dev Spec

## Product scope
Deliver a desktop GUI variant of `r-clite` with feature parity for core editing flows:
- open/save/save-as
- cursor navigation
- insert/delete/newline/tab
- undo/redo
- find next/prev
- line numbers
- dirty state indicator

Not in scope for first GUI session:
- collaboration UI parity
- minimap, split panes, plugin system

## Architecture target

### 1) Split into core + frontends
Create a backend-neutral editor core module and two frontends:
- `core` (domain): buffer mutations, command handling, cursor/scroll state, search, undo/redo, status messages.
- `cli` frontend: existing crossterm terminal renderer/input.
- `gui` frontend: new egui renderer/input.

Suggested module layout:
- `src/core/mod.rs`
- `src/core/editor_core.rs`
- `src/frontends/cli/*`
- `src/frontends/gui/*`

### 2) Introduce frontend-agnostic input command API
- Keep/expand [`Command`](/Users/brian/Documents/projects/r-clite/src/keymap.rs:14) as the canonical action enum.
- Add two mapping adapters:
  - `cli_keymap`: `crossterm::KeyEvent -> Command`
  - `gui_keymap`: `egui::Event -> Command` (or shortcut-level translation)

### 3) Introduce render model separate from terminal output
- Preserve and extend [`RenderState`](/Users/brian/Documents/projects/r-clite/src/ui.rs:20) into a renderer-neutral `ViewState`.
- Core produces `ViewState`; frontends consume it.

### 4) Keep text model unchanged
- Reuse [`Buffer`](/Users/brian/Documents/projects/r-clite/src/buffer.rs:16) + `ropey` as-is.
- Reuse highlight API from [`highlight_line`](/Users/brian/Documents/projects/r-clite/src/highlight.rs:38), replacing `crossterm::Color` with a frontend-neutral color token enum if needed.

## GUI implementation plan (`egui`/`eframe`)

### Phase 1: Bootstrapping
1. Add GUI binary target in `Cargo.toml`:
   - `[[bin]] name = "rcte-gui" path = "src/bin/rcte_gui.rs"`
2. Add dependencies:
   - `eframe`, `egui` (version chosen by dev at implementation time)
3. Implement minimal app window with:
   - title
   - status bar
   - placeholder central editor panel

Acceptance:
- `cargo run --bin rcte-gui` opens window on macOS/Linux/Windows dev machines.

### Phase 2: Core extraction without behavior change
1. Move editing state from `Editor` into `EditorCore`.
2. Keep CLI behavior intact by adapting existing editor loop to call `EditorCore`.
3. Add smoke tests for:
   - insert/backspace/delete/newline
   - undo/redo
   - find next/prev

Acceptance:
- Existing CLI behavior remains unchanged.
- `cargo test` passes.

### Phase 3: GUI viewport + input parity
1. Build custom code viewport in egui:
   - virtualized visible lines only
   - gutter with line numbers
   - cursor rendering
   - search highlight and syntax spans
2. Map keyboard shortcuts to `Command` parity with CLI.
3. Hook file open/save/save-as (reuse existing file picker logic where possible).

Acceptance:
- GUI can open/edit/save text files with parity to CLI core flows.
- Shortcut parity validated manually: Ctrl+S, Ctrl+Shift+S, Ctrl+O, Ctrl+Q, Ctrl+F, Ctrl+Z, Ctrl+Y.

### Phase 4: Performance and UX hardening
1. Add incremental repaint/viewport-only rendering safeguards.
2. Ensure large file responsiveness target:
   - Open 10k-line file under acceptable interactive latency.
3. Add startup config support parity (theme, tab width, line numbers).

Acceptance:
- No major input lag on 10k-line editing scenario.
- Config values apply in GUI.

## Non-functional requirements
- Cross-platform desktop (macOS, Linux, Windows).
- No data loss on normal close path (dirty confirmation required).
- Unicode correctness preserved (keep ropey-based offsets).

## Risks and mitigations
- Risk: command drift between CLI and GUI.
  - Mitigation: shared `Command` enum and shared core reducer paths.
- Risk: large-file rendering slowdown.
  - Mitigation: strict viewport virtualization and profiling before polish.
- Risk: terminal concerns leaking into core.
  - Mitigation: no `crossterm` types in core modules.

## Deliverables expected
1. New GUI binary scaffold and dependency wiring.
2. Core/editor extraction PR with no CLI regressions.
3. Initial GUI editor viewport with basic editing parity.
4. Updated README section documenting:
   - `rcte` (CLI)
   - `rcte-gui` (GUI)

## Done criteria (session-level)
- `rcte-gui` launches and edits/saves a file end-to-end.
- Core logic shared by CLI and GUI (no duplicated editing logic).
- Tests cover core editing operations independent of frontend.

