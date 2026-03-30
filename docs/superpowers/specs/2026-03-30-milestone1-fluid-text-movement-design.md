# Milestone 1: Fluid Text Movement — Design Spec

**Date:** 2026-03-30
**Status:** Approved
**Scope:** Word navigation, soft word wrap, hard wrap / reflow paragraph

---

## 1. Motivation

The editor currently lacks word-level movement and word wrap. Users cannot move or delete by word, and long lines require horizontal scrolling. These two gaps make the editor uncomfortable for daily prose and notes editing. This milestone closes both gaps entirely.

---

## 2. New Commands

Six new variants added to the `Command` enum in `src/keymap.rs`.

| Command | Windows / Linux | macOS |
|---|---|---|
| `MoveWordLeft` | Ctrl+Left | Option+Left |
| `MoveWordRight` | Ctrl+Right | Option+Right |
| `DeleteWordLeft` | Ctrl+Backspace | Option+Backspace |
| `DeleteWordRight` | Ctrl+Delete | Option+Delete |
| `ToggleSoftWrap` | Ctrl+Shift+W | Cmd+Shift+W |
| `ReflowParagraph` | Alt+Q | Option+Q |

### Word boundary definition

A word is a contiguous run of alphanumeric characters and underscores (`[a-zA-Z0-9_]`).

- `MoveWordRight` — skip non-word characters, then advance through the current word, landing at the start of the next word.
- `MoveWordLeft` — if at a word boundary, step back; then retreat through the current/previous word, landing at its start.
- `DeleteWordLeft` — delete from cursor back to the start of the previous word (same motion as `MoveWordLeft`, then delete the range).
- `DeleteWordRight` — delete from cursor forward to the start of the next word (same motion as `MoveWordRight`, then delete the range).

### Platform key mapping

TUI (`src/keymap.rs`): uses crossterm `KeyModifiers`. On macOS, terminals report Option as `ALT`, so TUI bindings are consistent across platforms without branching.

GUI (`src/gui/mod.rs`): branches on `#[cfg(target_os = "macos")]`, using `egui::Modifiers::ALT` (Option) for word nav and `egui::Modifiers::COMMAND | SHIFT` for `ToggleSoftWrap` on macOS; `egui::Modifiers::CTRL` variants on other platforms. This follows the existing OS-aware hotkey pattern in the codebase.

---

## 3. Soft Wrap

### State

- `soft_wrap: bool` added to `EditorCore`. Default: `true`.
- `ViewSnapshot` gains a `soft_wrap: bool` field so both frontends read it without re-querying the core.
- Config file (`~/.config/r-clite/config.toml`) gains a new key: `word_wrap = true`.
- `ToggleSoftWrap` flips the flag at runtime.

### Rendering

When `soft_wrap` is on, a buffer row that exceeds the viewport width is rendered across multiple **visual rows**. Neither frontend inserts newlines into the buffer.

- **TUI (`src/ui.rs`):** `Ui::render()` breaks long lines at `viewport_cols` characters, emitting multiple terminal rows per buffer row.
- **GUI (`src/gui/mod.rs`):** egui's `LayoutJob` wraps text natively; the GUI rendering change is minimal.

### Cursor movement

A helper is added to `EditorCore`:

```rust
fn visual_rows_for(buffer_row: usize, wrap_width: usize) -> usize
```

Returns how many visual rows the given buffer row occupies. Used by Up/Down movement to step through visual rows correctly when wrap is on. Scroll offset is stored and computed in visual rows when wrap is enabled.

---

## 4. Hard Wrap / Reflow Paragraph

### Behaviour

`ReflowParagraph` locates the **current paragraph** — the contiguous block of non-blank lines surrounding the cursor row — joins its lines into a single string, then re-wraps to `wrap_column` characters by inserting newlines at word boundaries. The paragraph in the buffer is replaced with the reflowed result as a **single undo entry**.

**Example** (`wrap_column = 40`):
```
Before:
This is a very long line that goes well beyond forty characters in length.

After:
This is a very long line that goes well
beyond forty characters in length.
```

### Paragraph boundary detection

Scan upward from the cursor row until a blank line or the buffer start is found. Scan downward similarly. The resulting row range is the reflow target.

### Config

```toml
wrap_column = 80   # column width used by ReflowParagraph
```

### Implementation

A single new method on `EditorCore`:

```rust
fn reflow_paragraph(&mut self)
```

Uses existing `raw_insert` / `raw_delete` primitives on `Buffer`, grouped into one `UndoEntry`. No changes required in `ui.rs` or `gui/mod.rs` beyond dispatching the `ReflowParagraph` command.

---

## 5. Config Summary

Two new keys in `~/.config/r-clite/config.toml`:

```toml
word_wrap   = true   # soft wrap on by default (bool)
wrap_column = 80     # column for ReflowParagraph (usize, > 0)
```

Both are added to `src/config.rs` alongside existing keys, with the same silent-default-on-missing behaviour.

---

## 6. Files Changed

| File | Change |
|---|---|
| `src/keymap.rs` | Add 6 `Command` variants; add TUI key bindings |
| `src/core/editor_core.rs` | Word boundary logic, soft wrap state + `visual_rows_for`, `reflow_paragraph` |
| `src/ui.rs` | Visual line rendering for TUI when soft wrap is on |
| `src/gui/mod.rs` | OS-aware GUI key bindings; egui LayoutJob soft wrap |
| `src/config.rs` | `word_wrap`, `wrap_column` fields |
| `src/buffer.rs` | Word boundary helpers (`next_word_start`, `prev_word_start`) as public methods, called by `EditorCore` |

---

## 7. Out of Scope

- Find & Replace (Milestone 2)
- Multiple files / tabs (Milestone 3)
- Syntax highlighting for non-Rust files
