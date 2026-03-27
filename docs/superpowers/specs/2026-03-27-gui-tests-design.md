# GUI Test Suite Design

**Date:** 2026-03-27
**Project:** r-clite (`rcte-gui`)
**Scope:** Automated test suite covering editor core logic and egui GUI integration

---

## Overview

A two-tier test suite for the `rcte-gui` binary, which is built on `eframe`/`egui` 0.33.

- **Tier 1:** `EditorCore` unit tests — fast, headless, no GUI dependency
- **Tier 2:** `egui_kittest` harness tests — simulate real egui events through `GuiApp`

Total target: ~52 tests.

---

## Architecture

The codebase has a clean separation:

- `src/core/editor_core.rs` — `EditorCore`: pure logic, `apply_command(Command, ViewportMetrics) → Result<Option<FrontendRequest>>` + `snapshot() → ViewSnapshot`
- `src/gui/mod.rs` — `GuiApp`: egui rendering + event dispatch, implements `eframe::App`

This separation makes two-tier testing natural: Tier 1 tests the logic directly; Tier 2 tests the GUI wiring.

---

## Dependencies

Add to `Cargo.toml`:

```toml
[dev-dependencies]
egui_kittest = { version = "0.33", features = ["eframe"] }
```

No display or GPU required. `egui_kittest` is fully headless by default.

---

## File Structure

```
tests/
  core_tests.rs    # Tier 1: EditorCore unit tests
  gui_tests.rs     # Tier 2: egui_kittest harness tests
```

Existing inline tests in `src/gui/mod.rs` (shortcut mapping, layout helpers) are left in place.

---

## Shared Test Helpers

Defined at the top of each test file (not a shared module):

```rust
fn vp() -> ViewportMetrics { ViewportMetrics { rows: 20, cols: 80 } }

fn make_core(content: &str) -> EditorCore {
    EditorCore::new(Buffer::from_content(content.to_string()), Config::default())
}

fn type_str(core: &mut EditorCore, s: &str) {
    for ch in s.chars() {
        core.apply_command(Command::InsertChar(ch), vp()).unwrap();
    }
}
```

---

## Tier 1 — EditorCore Unit Tests (`tests/core_tests.rs`)

### Cursor movement (8 tests)

| Test | Assertion |
|------|-----------|
| `move_right_wraps_at_eol` | At end of line 0, MoveRight → cursor_row=1, cursor_col=0 |
| `move_left_wraps_at_line_start` | At col 0 on line 1, MoveLeft → cursor_row=0, cursor_col=line_len |
| `move_up_basic` | On row 1, MoveUp → row=0 |
| `move_down_basic` | On row 0, MoveDown → row=1 |
| `move_up_at_top_is_noop` | Row 0, MoveUp → still row=0 |
| `move_down_at_bottom_is_noop` | Last row, MoveDown → stays |
| `move_line_start` | MoveLineStart → col=0 |
| `move_line_end` | MoveLineEnd → col=line_len |

### Text editing (7 tests)

| Test | Assertion |
|------|-----------|
| `insert_char_updates_buffer_and_cursor` | Insert 'x' → buffer contains 'x', cursor_col=1 |
| `insert_multiple_chars` | type_str("hello") → buffer starts with "hello" |
| `insert_newline_splits_line` | "ab" then Enter at col 1 → 2 lines: "a" and "b" |
| `insert_tab_expands_to_spaces` | InsertTab → inserts 4 spaces (default tab_width) |
| `backspace_mid_line` | type "ab", Backspace → buffer contains "a" |
| `backspace_at_line_start_joins_lines` | Two lines, cursor at start of line 1, Backspace → 1 line |
| `delete_char_mid_line` | cursor at col 0 on "abc", DeleteChar → "bc" |

### Undo / Redo (4 tests)

| Test | Assertion |
|------|-----------|
| `undo_insert_restores_content` | type "hello", Undo → buffer empty, cursor at 0,0 |
| `undo_delete_restores_content` | type "ab", Backspace, Undo → "ab" restored |
| `undo_then_redo` | type "x", Undo, Redo → "x" present again |
| `undo_past_empty_stack_is_noop` | Empty buffer, Undo → no panic, buffer unchanged |

### Search (4 tests)

| Test | Assertion |
|------|-----------|
| `find_activates_search_mode` | Command::Find → snapshot().search is Some |
| `search_finds_match` | Content "hello world", search "world" → search_match is Some |
| `search_no_match` | Content "hello", search "xyz" → search_match is None |
| `search_cancel_restores_cursor` | Move to row 1, start search, cancel → cursor returns to row 1 |

### Selection (4 tests)

| Test | Assertion |
|------|-----------|
| `selection_basic` | set_selection_start(0,0), set_selection_end(0,3) → get_selected_text()="hel" |
| `selection_across_lines` | select from (0,0) to (1,0) → selected text includes newline |
| `clear_selection` | After setting selection, clear_selection() → has_selection()=false |
| `empty_selection_start_equals_end` | start==(0,2), end==(0,2) → has_selection()=false |

### Quit (2 tests)

| Test | Assertion |
|------|-----------|
| `quit_dirty_buffer_requires_confirm` | Type something, Quit → should_quit=false, message shown |
| `quit_clean_buffer_immediate` | Untouched buffer, Quit → should_quit=true |

### Buffer / dirty flag (3 tests)

| Test | Assertion |
|------|-----------|
| `fresh_buffer_not_dirty` | Buffer::from_content → buffer().is_dirty()=false |
| `insert_marks_dirty` | InsertChar → buffer().is_dirty()=true |
| `line_count_after_newline` | "a\nb", line_count()=2; after InsertNewline on line 0 → 3 |

**Tier 1 total: 32 tests**

---

## Tier 2 — egui_kittest Harness Tests (`tests/gui_tests.rs`)

### Harness setup pattern

```rust
use egui_kittest::Harness;
use r_clite::{buffer::Buffer, config::Config, gui::GuiApp};

fn make_harness(content: &str) -> Harness<GuiApp> {
    let buffer = Buffer::from_content(content.to_string());
    let mut harness = Harness::new_eframe(|_cc| {
        GuiApp::new(buffer, Config::default(), None)
    });
    harness.run();
    harness
}
```

### Initialization (2 tests)

| Test | Assertion |
|------|-----------|
| `initial_buffer_content_preserved` | GuiApp created with "hello\n" → core.buffer().to_string() contains "hello" |
| `empty_buffer_has_one_line` | Empty content → core.buffer().line_count()=1 |

### Text input (3 tests)

| Test | Assertion |
|------|-----------|
| `text_event_inserts_into_buffer` | Event::Text("hello") + run() → buffer contains "hello" |
| `backspace_key_removes_char` | Type "ab", Key::Backspace + run() → buffer contains "a" |
| `enter_key_inserts_newline` | Type "ab", Key::Enter + run() → line_count=2 |

### Cursor navigation (3 tests)

| Test | Assertion |
|------|-----------|
| `arrow_right_advances_cursor` | Type "abc", ArrowRight×2 + run() → cursor_col advances (or wraps) |
| `arrow_down_advances_row` | "line1\nline2", ArrowDown + run() → cursor_row=1 |
| `home_end_keys` | ArrowEnd → cursor_col=line_len; Home → cursor_col=0 |

### Mouse interaction (4 tests)

| Test | Assertion |
|------|-----------|
| `click_positions_cursor` | After run() (editor_rect populated), PointerButton at row-1 y-coord → cursor_row=1 |
| `click_second_line` | Click at y = rect.top + PANEL_PADDING + 18.0 (row height) → cursor_row=1 |
| `drag_creates_selection` | PointerButton pressed, PointerMoved, PointerButton released → has_selection()=true |
| `click_clears_selection` | Set selection via drag, then click elsewhere → has_selection()=false |

Mouse events use raw `harness.event(egui::Event::PointerButton { pos, button, pressed, modifiers })`. The editor area origin is computed from known constants: `PANEL_PADDING=12.0`, `row_height=18.0`, `char_width=8.0`.

### Hotkey integration (8 tests)

| Hotkey | Test | Assertion |
|--------|------|-----------|
| `Ctrl+Z` | `undo_hotkey_reverts_text` | Type "abc", Ctrl+Z + run() → buffer reverted |
| `Ctrl+Y` | `redo_hotkey_reapplies_text` | Type, Undo, Ctrl+Y + run() → text restored |
| `Ctrl+F` | `find_hotkey_activates_search` | Ctrl+F + run() → snapshot().search is Some |
| `Ctrl+Q` | `quit_hotkey_on_clean_buffer` | Ctrl+Q + run() → snapshot().should_quit=true |
| `Ctrl+L` | `toggle_line_numbers_hotkey` | Ctrl+L + run() → show_line_numbers flips |
| `Ctrl+A` | `select_all_hotkey` | Type "hello", Ctrl+A + run() → core.has_selection()=true |
| `Ctrl+X` | `cut_hotkey_removes_selected_text` | Ctrl+A then Ctrl+X + run() → buffer is empty |
| `Shift+ArrowRight` | `shift_arrow_extends_selection` | Shift+ArrowRight + run() → has_selection()=true |

Hotkeys use `harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z)`.
On macOS, `egui::Modifiers::command` is used; tests set both `ctrl` and `command` to be cross-platform.

**Tier 2 total: 20 tests**

---

## Out of Scope

- `Ctrl+S` / `Ctrl+O` / `Ctrl+Shift+S` — trigger native file dialogs, untestable headlessly
- `Ctrl+V` — requires real clipboard, flaky in CI
- `Ctrl+C` — no state mutation to assert against
- Screenshot / pixel regression tests — fragile across platforms, not needed for a logic suite
- Collab features — separate feature flag, out of scope here

---

## Grand Total: ~52 tests
