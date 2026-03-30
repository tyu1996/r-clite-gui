# Milestone 1: Fluid Text Movement — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add word navigation, soft word wrap, and reflow paragraph to both the TUI and GUI frontends.

**Architecture:** Word boundary logic lives in `Buffer`; cursor movement and soft-wrap state live in `EditorCore`; both frontends (TUI via `ui.rs`/`editor.rs`, GUI via `gui/mod.rs`) read `ViewSnapshot::soft_wrap` and dispatch new `Command` variants. `Config` supplies `word_wrap` and `wrap_column` defaults.

**Tech Stack:** Rust 2024, ropey (rope data structure), crossterm (TUI), egui/eframe (GUI)

---

## Task 1: Buffer word boundary helpers

**Spec ref:** §2 (word boundary definition)

**Files:**
- Modify: `src/buffer.rs`

- [ ] **Step 1: Write failing tests**

Add at the bottom of `src/buffer.rs` inside a `#[cfg(test)]` block (or existing test block):

```rust
#[cfg(test)]
mod word_boundary_tests {
    use super::*;

    // "hello world foo" — positions:
    // h=0,e=1,l=2,l=3,o=4, =5,w=6,o=7,r=8,l=9,d=10, =11,f=12,o=13,o=14 (len=15)
    fn buf() -> Buffer { Buffer::from_content("hello world foo".to_string()) }

    #[test]
    fn next_word_from_line_start() {
        assert_eq!(buf().next_word_start(0), 6); // "hello" → "world"
    }

    #[test]
    fn next_word_from_middle_of_word() {
        assert_eq!(buf().next_word_start(2), 6); // inside "hello" → "world"
    }

    #[test]
    fn next_word_from_word_boundary() {
        assert_eq!(buf().next_word_start(6), 12); // "world" → "foo"
    }

    #[test]
    fn next_word_at_end() {
        assert_eq!(buf().next_word_start(15), 15); // at end → stays at end
    }

    #[test]
    fn prev_word_from_end() {
        assert_eq!(buf().prev_word_start(15), 12); // end → start of "foo"
    }

    #[test]
    fn prev_word_from_start_of_word() {
        assert_eq!(buf().prev_word_start(12), 6); // "foo" → "world"
    }

    #[test]
    fn prev_word_from_middle_of_word() {
        assert_eq!(buf().prev_word_start(8), 6); // inside "world" → start of "world"
    }

    #[test]
    fn prev_word_at_zero() {
        assert_eq!(buf().prev_word_start(0), 0);
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test word_boundary
```

Expected: compile error — `next_word_start` and `prev_word_start` not defined.

- [ ] **Step 3: Add `is_word_char` helper and both methods to `Buffer`**

Add after the `char_at_offset` method in `src/buffer.rs`:

```rust
/// Returns `true` if `c` is a word character (`[a-zA-Z0-9_]`).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Returns the char offset of the start of the next word after `pos`.
///
/// Skips any word chars at `pos` (end of current word), then skips non-word
/// chars, landing at the first char of the next word. Returns `len_chars()`
/// if there is no next word.
pub fn next_word_start(&self, pos: usize) -> usize {
    let len = self.rope.len_chars();
    let mut i = pos;
    // Skip current word characters
    while i < len && is_word_char(self.rope.char(i)) {
        i += 1;
    }
    // Skip non-word characters
    while i < len && !is_word_char(self.rope.char(i)) {
        i += 1;
    }
    i
}

/// Returns the char offset of the start of the word to the left of `pos`.
///
/// Steps back over non-word chars, then back over word chars, landing at
/// the first char of that word. Returns `0` if already at the start.
pub fn prev_word_start(&self, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    // Step back over non-word chars
    while i > 0 && !is_word_char(self.rope.char(i)) {
        i -= 1;
    }
    // Step back over word chars
    while i > 0 && is_word_char(self.rope.char(i - 1)) {
        i -= 1;
    }
    i
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```
cargo test word_boundary
```

Expected: 8 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/buffer.rs
git commit -m "feat(buffer): add next_word_start and prev_word_start helpers"
```

---

## Task 2: New Command variants and TUI key bindings

**Spec ref:** §2 (commands + TUI bindings)

**Files:**
- Modify: `src/keymap.rs`

- [ ] **Step 1: Add 6 new variants to `Command`**

In `src/keymap.rs`, add after the `SelectAll` variant and before `None`:

```rust
/// Move cursor left by one word.
MoveWordLeft,
/// Move cursor right by one word.
MoveWordRight,
/// Delete from cursor back to the start of the previous word.
DeleteWordLeft,
/// Delete from cursor forward to the start of the next word.
DeleteWordRight,
/// Toggle soft word wrap on/off.
ToggleSoftWrap,
/// Reflow the current paragraph to the configured wrap column.
ReflowParagraph,
```

- [ ] **Step 2: Add TUI key bindings**

In the `map()` function in `src/keymap.rs`, add the `alt` variable alongside `ctrl`, then add new match arms. The full updated `map()` function:

```rust
pub fn map(event: KeyEvent) -> Command {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = event.modifiers.contains(KeyModifiers::ALT);

    match event.code {
        // Arrow keys
        KeyCode::Up => Command::MoveUp,
        KeyCode::Down => Command::MoveDown,
        KeyCode::Left => Command::MoveLeft,
        KeyCode::Right => Command::MoveRight,

        // Emacs-style navigation aliases
        KeyCode::Char('p') if ctrl => Command::MoveUp,
        KeyCode::Char('n') if ctrl => Command::MoveDown,
        KeyCode::Char('b') if ctrl => Command::MoveLeft,
        KeyCode::Char('f') if ctrl => Command::MoveRight,

        // Line navigation
        KeyCode::Home => Command::MoveLineStart,
        KeyCode::End => Command::MoveLineEnd,

        // Page navigation
        KeyCode::PageUp => Command::PageUp,
        KeyCode::PageDown => Command::PageDown,

        // Word navigation (Ctrl+arrow on Win/Linux; terminals report Option as ALT on macOS)
        KeyCode::Left if ctrl || alt => Command::MoveWordLeft,
        KeyCode::Right if ctrl || alt => Command::MoveWordRight,

        // Word delete
        KeyCode::Backspace if ctrl || alt => Command::DeleteWordLeft,
        KeyCode::Delete if ctrl || alt => Command::DeleteWordRight,

        // Quit
        KeyCode::Char('q') if ctrl => Command::Quit,

        // Reflow paragraph: Alt+Q (Option+Q on macOS — must come before Ctrl+Q)
        KeyCode::Char('q') if alt => Command::ReflowParagraph,

        // Save / Save As
        KeyCode::Char('s') if ctrl && event.modifiers.contains(KeyModifiers::SHIFT) => {
            Command::SaveAs
        }
        KeyCode::Char('S') if ctrl => Command::SaveAs,
        KeyCode::Char('s') if ctrl => Command::Save,
        KeyCode::Char('o') if ctrl => Command::Open,

        // Undo / Redo
        KeyCode::Char('z') if ctrl => Command::Undo,
        KeyCode::Char('y') if ctrl => Command::Redo,

        // Find
        KeyCode::Char('f') if ctrl => Command::Find,

        // Toggle line numbers
        KeyCode::Char('l') if ctrl => Command::ToggleLineNumbers,

        // Soft wrap toggle: Ctrl+Shift+W (or Option+W terminals may send ALT+W)
        KeyCode::Char('w') if ctrl && event.modifiers.contains(KeyModifiers::SHIFT) => {
            Command::ToggleSoftWrap
        }
        KeyCode::Char('W') if ctrl => Command::ToggleSoftWrap,

        // Editing
        KeyCode::Enter => Command::InsertNewline,
        KeyCode::Tab => Command::InsertTab,
        KeyCode::Backspace => Command::Backspace,
        KeyCode::Delete => Command::DeleteChar,

        // Printable characters (no modifier, or shift only for uppercase/symbols)
        KeyCode::Char(ch) if !ctrl => Command::InsertChar(ch),

        _ => Command::None,
    }
}
```

- [ ] **Step 3: Verify it compiles**

```
cargo build 2>&1 | head -30
```

Expected: compile warnings about unused variants (they aren't handled yet) but no errors.

- [ ] **Step 4: Commit**

```bash
git add src/keymap.rs
git commit -m "feat(keymap): add word nav, soft wrap, and reflow paragraph commands"
```

---

## Task 3: Config additions

**Spec ref:** §5 (config summary)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add fields to `Config` struct and `Default` impl**

In `src/config.rs`, update the struct and default:

```rust
pub struct Config {
    pub tab_width: usize,
    pub line_numbers: bool,
    pub theme: String,
    /// Whether soft word wrap is enabled on startup.
    pub word_wrap: bool,
    /// Column width used by the ReflowParagraph command.
    pub wrap_column: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_width: 4,
            line_numbers: true,
            theme: "dark".to_string(),
            word_wrap: true,
            wrap_column: 80,
        }
    }
}
```

- [ ] **Step 2: Parse the new keys in `Config::load()`**

Inside the `match key {` block in `Config::load()`, add after the `"theme"` arm:

```rust
"word_wrap" => match val {
    "true" => cfg.word_wrap = true,
    "false" => cfg.word_wrap = false,
    _ => warnings.push(format!("config: invalid word_wrap '{}'", val)),
},
"wrap_column" => match val.parse::<usize>() {
    Ok(n) if n > 0 => cfg.wrap_column = n,
    _ => warnings.push(format!("config: invalid wrap_column '{}'", val)),
},
```

- [ ] **Step 3: Verify it compiles**

```
cargo build 2>&1 | head -20
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add word_wrap and wrap_column settings"
```

---

## Task 4: EditorCore word movement

**Spec ref:** §2 (MoveWordLeft/Right, DeleteWordLeft/Right)

**Files:**
- Modify: `src/core/editor_core.rs`

- [ ] **Step 1: Write failing tests**

In the `#[cfg(test)]` module at the bottom of `src/core/editor_core.rs`, add:

```rust
mod word_movement_tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::config::Config;

    fn make_core(text: &str) -> EditorCore {
        EditorCore::new(Buffer::from_content(text.to_string()), Config::default())
    }

    fn vp() -> ViewportMetrics { ViewportMetrics { rows: 24, cols: 80 } }

    #[test]
    fn move_word_right_from_start() {
        let mut core = make_core("hello world");
        core.apply_command(Command::MoveWordRight, vp()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 6));
    }

    #[test]
    fn move_word_right_from_last_word() {
        let mut core = make_core("hello world");
        core.cursor_col = 6;
        core.apply_command(Command::MoveWordRight, vp()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 11));
    }

    #[test]
    fn move_word_left_from_end() {
        let mut core = make_core("hello world");
        core.cursor_col = 11;
        core.apply_command(Command::MoveWordLeft, vp()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 6));
    }

    #[test]
    fn move_word_left_from_start_of_word() {
        let mut core = make_core("hello world");
        core.cursor_col = 6;
        core.apply_command(Command::MoveWordLeft, vp()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 0));
    }

    #[test]
    fn delete_word_left_removes_previous_word() {
        let mut core = make_core("hello world");
        core.cursor_col = 11;
        core.apply_command(Command::DeleteWordLeft, vp()).unwrap();
        assert_eq!(core.buffer().line(0), "hello ");
        assert_eq!(core.cursor_col, 6);
    }

    #[test]
    fn delete_word_right_removes_next_word() {
        let mut core = make_core("hello world");
        // cursor at 0; next_word_start(0) = 6 (skips "hello" then space → "world")
        core.apply_command(Command::DeleteWordRight, vp()).unwrap();
        assert_eq!(core.buffer().line(0), " world");
    }

    #[test]
    fn delete_word_left_is_undoable() {
        let mut core = make_core("hello world");
        core.cursor_col = 11;
        core.apply_command(Command::DeleteWordLeft, vp()).unwrap();
        core.apply_command(Command::Undo, vp()).unwrap();
        assert_eq!(core.buffer().line(0), "hello world");
        assert_eq!(core.cursor_col, 11);
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test word_movement
```

Expected: compile error — new Command variants not matched in `apply_command`.

- [ ] **Step 3: Add the four movement methods and wire into `apply_command`**

Add these private methods to the `impl EditorCore` block in `src/core/editor_core.rs`:

```rust
fn move_word_right(&mut self, viewport: ViewportMetrics) {
    let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
    let new_pos = self.buffer.next_word_start(pos);
    let (row, col) = self.buffer.offset_to_row_col(new_pos);
    self.cursor_row = row;
    self.cursor_col = col;
    self.desired_col = col;
    self.last_insert_at = None;
    self.scroll_to_cursor(viewport);
}

fn move_word_left(&mut self, viewport: ViewportMetrics) {
    let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
    let new_pos = self.buffer.prev_word_start(pos);
    let (row, col) = self.buffer.offset_to_row_col(new_pos);
    self.cursor_row = row;
    self.cursor_col = col;
    self.desired_col = col;
    self.last_insert_at = None;
    self.scroll_to_cursor(viewport);
}

fn delete_word_left(&mut self, viewport: ViewportMetrics) {
    let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
    if pos == 0 {
        return;
    }
    let new_pos = self.buffer.prev_word_start(pos);
    let len = pos - new_pos;
    let text: String = self.buffer.rope.slice(new_pos..pos).into();
    let cursor_before = (self.cursor_row, self.cursor_col);
    self.buffer.raw_delete(new_pos, len);
    let (row, col) = self.buffer.offset_to_row_col(new_pos);
    self.cursor_row = row;
    self.cursor_col = col;
    self.desired_col = col;
    self.push_undo(
        vec![EditOp::Delete { pos: new_pos, text }],
        cursor_before,
        (row, col),
    );
    self.last_insert_at = None;
    self.redo_stack.clear();
    self.scroll_to_cursor(viewport);
}

fn delete_word_right(&mut self, viewport: ViewportMetrics) {
    let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
    let end_pos = self.buffer.next_word_start(pos);
    if end_pos == pos {
        return;
    }
    let delete_len = end_pos - pos;
    let text: String = self.buffer.rope.slice(pos..end_pos).into();
    let cursor_before = (self.cursor_row, self.cursor_col);
    self.buffer.raw_delete(pos, delete_len);
    self.push_undo(
        vec![EditOp::Delete { pos, text }],
        cursor_before,
        (self.cursor_row, self.cursor_col),
    );
    self.last_insert_at = None;
    self.redo_stack.clear();
    self.scroll_to_cursor(viewport);
}
```

In `apply_command`, add the four new arms before `Command::None`:

```rust
Command::MoveWordLeft => {
    self.move_word_left(viewport);
    None
}
Command::MoveWordRight => {
    self.move_word_right(viewport);
    None
}
Command::DeleteWordLeft => {
    self.delete_word_left(viewport);
    None
}
Command::DeleteWordRight => {
    self.delete_word_right(viewport);
    None
}
Command::ToggleSoftWrap => {
    // Implemented in Task 5
    None
}
Command::ReflowParagraph => {
    // Implemented in Task 7
    None
}
```

- [ ] **Step 4: Run tests**

```
cargo test word_movement
```

Expected: 7 tests PASS.

- [ ] **Step 5: Run full test suite**

```
cargo test
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/core/editor_core.rs
git commit -m "feat(core): implement word navigation and word delete commands"
```

---

## Task 5: Soft wrap state in EditorCore

**Spec ref:** §3 (soft wrap state, `visual_rows_for`, cursor movement, `scroll_to_cursor`)

**Files:**
- Modify: `src/core/editor_core.rs`

- [ ] **Step 1: Write failing tests**

Add to the test module in `src/core/editor_core.rs`:

```rust
mod soft_wrap_tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::config::Config;

    fn make_core_wrap(text: &str) -> EditorCore {
        let mut cfg = Config::default();
        cfg.word_wrap = true;
        cfg.wrap_column = 80;
        EditorCore::new(Buffer::from_content(text.to_string()), cfg)
    }

    fn vp_narrow() -> ViewportMetrics { ViewportMetrics { rows: 24, cols: 15 } }
    // text_width for cols=15, show_line_numbers=true (gutter=1+1=2) = 13

    #[test]
    fn soft_wrap_on_by_default() {
        let core = make_core_wrap("hello");
        assert!(core.soft_wrap);
    }

    #[test]
    fn toggle_disables_soft_wrap() {
        let mut core = make_core_wrap("hello");
        core.apply_command(Command::ToggleSoftWrap, vp_narrow()).unwrap();
        assert!(!core.soft_wrap);
    }

    #[test]
    fn visual_rows_for_short_line() {
        let core = make_core_wrap("hello");
        // line_len=5, wrap_width=13 → 1 visual row
        assert_eq!(core.visual_rows_for(0, 13), 1);
    }

    #[test]
    fn visual_rows_for_wrapped_line() {
        // 20-char line, wrap_width=13 → ceil(20/13) = 2
        let core = make_core_wrap("abcdefghijklmnopqrst");
        assert_eq!(core.visual_rows_for(0, 13), 2);
    }

    #[test]
    fn move_down_within_wrapped_line() {
        // 20-char line, wrap_width=13; cursor starts at col 0
        let mut core = make_core_wrap("abcdefghijklmnopqrst");
        core.apply_command(Command::MoveDown, vp_narrow()).unwrap();
        // Should move to col 13 (next visual row within same buffer row)
        assert_eq!((core.cursor_row, core.cursor_col), (0, 13));
    }

    #[test]
    fn move_up_within_wrapped_line() {
        let mut core = make_core_wrap("abcdefghijklmnopqrst");
        core.cursor_col = 13; // second visual row
        core.apply_command(Command::MoveUp, vp_narrow()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 0));
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test soft_wrap
```

Expected: compile errors — `soft_wrap` field not found.

- [ ] **Step 3: Add `soft_wrap` and `wrap_column` to `EditorCore`**

In the `EditorCore` struct, add two fields after `selection_end`:

```rust
pub soft_wrap: bool,
wrap_column: usize,
```

In `EditorCore::new()`, initialize them from config:

```rust
soft_wrap: config.word_wrap,
wrap_column: config.wrap_column,
```

- [ ] **Step 4: Add `visual_rows_for` and `cursor_visual_row` helpers**

Add to `impl EditorCore`:

```rust
/// How many visual rows buffer row `r` occupies when wrapped at `wrap_width`.
pub fn visual_rows_for(&self, buffer_row: usize, wrap_width: usize) -> usize {
    if wrap_width == 0 {
        return 1;
    }
    let line_len = self.buffer.line_len(buffer_row);
    if line_len == 0 {
        return 1;
    }
    (line_len + wrap_width - 1) / wrap_width
}

/// Visual row index of the cursor (summing visual rows for all preceding buffer rows).
fn cursor_visual_row(&self, wrap_width: usize) -> usize {
    if wrap_width == 0 {
        return self.cursor_row;
    }
    let rows_before: usize = (0..self.cursor_row)
        .map(|r| self.visual_rows_for(r, wrap_width))
        .sum();
    rows_before + self.cursor_col / wrap_width
}
```

- [ ] **Step 5: Update `scroll_to_cursor` to handle soft wrap**

Replace the existing `scroll_to_cursor` body with:

```rust
fn scroll_to_cursor(&mut self, viewport: ViewportMetrics) {
    if self.soft_wrap {
        let wrap_width =
            viewport.text_width(self.show_line_numbers, self.buffer.line_count());
        if wrap_width > 0 {
            let visual_row = self.cursor_visual_row(wrap_width);
            if visual_row < self.scroll_offset {
                self.scroll_offset = visual_row;
            } else if viewport.rows > 0
                && visual_row >= self.scroll_offset + viewport.rows
            {
                self.scroll_offset = visual_row - viewport.rows + 1;
            }
            // No horizontal scrolling when soft wrap is on
            self.col_offset = 0;
            return;
        }
    }
    // Original behaviour (soft wrap off)
    if self.cursor_row < self.scroll_offset {
        self.scroll_offset = self.cursor_row;
    } else if viewport.rows > 0
        && self.cursor_row >= self.scroll_offset + viewport.rows
    {
        self.scroll_offset = self.cursor_row - viewport.rows + 1;
    }
    let text_width =
        viewport.text_width(self.show_line_numbers, self.buffer.line_count());
    if self.cursor_col < self.col_offset {
        self.col_offset = self.cursor_col;
    } else if text_width > 0 && self.cursor_col >= self.col_offset + text_width {
        self.col_offset = self.cursor_col - text_width + 1;
    }
}
```

- [ ] **Step 6: Update `move_up` and `move_down` for visual rows**

Replace `move_up`:

```rust
fn move_up(&mut self, viewport: ViewportMetrics) {
    if self.soft_wrap {
        let wrap_width =
            viewport.text_width(self.show_line_numbers, self.buffer.line_count());
        if wrap_width > 0 {
            let visual_row_in_line = self.cursor_col / wrap_width;
            if visual_row_in_line > 0 {
                // Move up within the same buffer row
                let visual_col = self.cursor_col % wrap_width;
                let new_start = (visual_row_in_line - 1) * wrap_width;
                let line_len = self.buffer.line_len(self.cursor_row);
                self.cursor_col = (new_start + visual_col).min(line_len);
                self.desired_col = self.cursor_col;
                self.last_insert_at = None;
                self.scroll_to_cursor(viewport);
                return;
            } else if self.cursor_row > 0 {
                // Move to last visual row of previous buffer row
                let visual_col = self.cursor_col % wrap_width;
                self.cursor_row -= 1;
                let prev_len = self.buffer.line_len(self.cursor_row);
                let last_vr_start = (prev_len / wrap_width) * wrap_width;
                // If prev_len is exactly divisible, last_vr_start == prev_len (empty
                // last visual row); step back one visual row.
                let last_vr_start = if last_vr_start == prev_len && prev_len > 0 {
                    last_vr_start.saturating_sub(wrap_width)
                } else {
                    last_vr_start
                };
                self.cursor_col = (last_vr_start + visual_col).min(prev_len);
                self.desired_col = self.cursor_col;
                self.last_insert_at = None;
                self.scroll_to_cursor(viewport);
                return;
            }
        }
    }
    // Original behaviour
    if self.cursor_row > 0 {
        self.cursor_row -= 1;
        self.clamp_col_to_line();
        self.scroll_to_cursor(viewport);
    }
    self.last_insert_at = None;
}
```

Replace `move_down`:

```rust
fn move_down(&mut self, viewport: ViewportMetrics) {
    if self.soft_wrap {
        let wrap_width =
            viewport.text_width(self.show_line_numbers, self.buffer.line_count());
        if wrap_width > 0 {
            let line_len = self.buffer.line_len(self.cursor_row);
            let visual_row_in_line = self.cursor_col / wrap_width;
            let next_vr_start = (visual_row_in_line + 1) * wrap_width;
            if next_vr_start < line_len {
                // Another visual row exists in the same buffer row
                let visual_col = self.cursor_col % wrap_width;
                self.cursor_col = (next_vr_start + visual_col).min(line_len);
                self.desired_col = self.cursor_col;
                self.last_insert_at = None;
                self.scroll_to_cursor(viewport);
                return;
            } else if self.cursor_row + 1 < self.buffer.line_count() {
                // Move to first visual row of next buffer row
                let visual_col = self.cursor_col % wrap_width;
                self.cursor_row += 1;
                let next_len = self.buffer.line_len(self.cursor_row);
                self.cursor_col = visual_col.min(next_len);
                self.desired_col = self.cursor_col;
                self.last_insert_at = None;
                self.scroll_to_cursor(viewport);
                return;
            }
        }
    }
    // Original behaviour
    if self.cursor_row + 1 < self.buffer.line_count() {
        self.cursor_row += 1;
        self.clamp_col_to_line();
        self.scroll_to_cursor(viewport);
    }
    self.last_insert_at = None;
}
```

- [ ] **Step 7: Wire `ToggleSoftWrap` into `apply_command`**

Replace the placeholder stub added in Task 4:

```rust
Command::ToggleSoftWrap => {
    self.soft_wrap = !self.soft_wrap;
    // Reset horizontal scroll when disabling wrap
    if !self.soft_wrap {
        self.col_offset = 0;
    }
    self.scroll_to_cursor(viewport);
    None
}
```

- [ ] **Step 8: Add `soft_wrap` to `ViewSnapshot`**

In the `ViewSnapshot` struct, add:

```rust
pub soft_wrap: bool,
```

In `EditorCore::snapshot()`, include it:

```rust
soft_wrap: self.soft_wrap,
```

- [ ] **Step 9: Run tests**

```
cargo test soft_wrap
```

Expected: all soft_wrap tests PASS.

- [ ] **Step 10: Run full suite**

```
cargo test
```

Expected: all tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/core/editor_core.rs
git commit -m "feat(core): add soft wrap state, visual row helpers, and wrap-aware cursor movement"
```

---

## Task 6: TUI soft wrap rendering

**Spec ref:** §3 (TUI rendering, cursor position)

**Files:**
- Modify: `src/ui.rs`
- Modify: `src/editor.rs`

- [ ] **Step 1: Add `soft_wrap` to `RenderState`**

In `src/ui.rs`, add to `RenderState`:

```rust
pub soft_wrap: bool,
```

- [ ] **Step 2: Pass `soft_wrap` from snapshot in `editor.rs`**

In `src/editor.rs`, inside the `RenderState { ... }` literal, add:

```rust
soft_wrap: snapshot.soft_wrap,
```

- [ ] **Step 3: Update `Ui::render()` for soft wrap**

The current render loop in `src/ui.rs` iterates `screen_row in 0..rows` with `file_row = screen_row + scroll_offset`. With soft wrap we need a visual-row map.

Replace the section from `for screen_row in 0..rows {` through the final `cursor::MoveTo` call with the following. The key change is: build a `visual_map: Vec<(usize, usize)>` mapping visual row index → `(buffer_row, col_start_in_line)`, then use it in the rendering loop.

Find the line in `render()` that reads:
```rust
for screen_row in 0..rows {
    let file_row = screen_row + scroll_offset;
```

Replace the entire loop (up to and not including the status bar rendering) with:

```rust
// Build visual-row → (buffer_row, col_start) map when soft wrap is on.
// When soft wrap is off this is a 1-1 map: visual_row == buffer_row, col_start == 0.
let wrap_width = if state.soft_wrap { text_width } else { 0 };
let visual_map: Vec<(usize, usize)> = if wrap_width > 0 {
    let mut map = Vec::new();
    for row in 0..line_count {
        let line_len = buffer.line_len(row);
        let num_vr = if line_len == 0 {
            1
        } else {
            (line_len + wrap_width - 1) / wrap_width
        };
        for vr in 0..num_vr {
            map.push((row, vr * wrap_width));
        }
    }
    map
} else {
    (0..line_count).map(|r| (r, 0)).collect()
};

for screen_row in 0..rows {
    let visual_row = screen_row + scroll_offset;
    stdout.queue(Clear(ClearType::CurrentLine))?;

    let resolved = if wrap_width > 0 {
        visual_map.get(visual_row).copied()
    } else {
        let file_row = visual_row;
        if file_row < line_count {
            Some((file_row, col_offset))
        } else {
            None
        }
    };

    if let Some((file_row, col_start)) = resolved {
        // Gutter: show line number on first visual row of a buffer row, blank on continuations.
        if gutter > 0 {
            if col_start == 0 {
                let num = format!("{:>width$} ", file_row + 1, width = gutter - 1);
                stdout.queue(SetForegroundColor(Color::DarkGrey))?;
                stdout.queue(Print(&num))?;
                stdout.queue(ResetColor)?;
            } else {
                stdout.queue(Print(" ".repeat(gutter)))?;
            }
        }

        let line = buffer.line(file_row);
        // Only re-compute highlight spans at the start of each buffer row.
        // For continuation visual rows we reuse the same spans, just advance col_start.
        let (spans, next_block) =
            highlight::highlight_line(&line, file_ext, in_block_comment, &self.theme);
        // Only advance in_block_comment at the end of each buffer row (col_start == 0 means
        // this is the first — or only — visual row for file_row).
        if col_start == 0 {
            in_block_comment = next_block;
        }

        let line_char_start = buffer.char_offset_for(file_row, 0);
        let col_end = col_start + text_width;
        let mut char_pos = 0usize;
        let mut printed = 0usize;

        // Peer cursor map (collab feature unchanged — still uses absolute col)
        #[cfg(feature = "collab")]
        let peer_cursor_cols: std::collections::HashMap<usize, Color> = {
            let mut map = std::collections::HashMap::new();
            let line_char_end = line_char_start + buffer.line_len(file_row);
            for (idx, (_peer, &offset)) in peer_cursors.iter().enumerate() {
                if offset >= line_char_start && offset <= line_char_end {
                    let col = offset - line_char_start;
                    let color = PEER_COLORS[idx % PEER_COLORS.len()];
                    map.insert(col, color);
                }
            }
            map
        };

        for span in &spans {
            for ch in span.text.chars() {
                if char_pos < col_start {
                    char_pos += 1;
                    continue;
                }
                if char_pos >= col_end || printed >= text_width {
                    break;
                }
                // Copy the existing per-character rendering block verbatim here.
                // The ONLY difference from the original loop is the skip/stop bounds:
                //   Original: skip if char_pos < col_offset; stop if printed >= text_width
                //   Wrapped:  skip if char_pos < col_start;  stop if char_pos >= col_end
                // Everything else — abs_offset calculation, search_match highlight,
                // peer_cursor highlight, Color32 selection, and stdout.queue(Print(ch)) —
                // is identical. Copy it from the current loop body unchanged.
                let abs_offset = line_char_start + char_pos;
                char_pos += 1;
                printed += 1;
            }
            if printed >= text_width {
                break;
            }
        }
    } else {
        // Beyond-EOF tilde row
        if gutter > 0 {
            stdout.queue(Print(" ".repeat(gutter)))?;
        }
        stdout.queue(SetForegroundColor(Color::DarkCyan))?;
        stdout.queue(Print("~"))?;
        stdout.queue(ResetColor)?;
    }
    stdout.queue(Print("\r\n"))?;
}
```

**Note to implementer:** The comment `// [existing search_match and peer_cursor highlighting code goes here verbatim]` means: copy the exact existing per-character rendering block from the original loop unchanged. Only the outer col range check changes (from `col_offset`/`text_width` to `col_start`/`col_end`).

- [ ] **Step 4: Fix the terminal cursor position for soft wrap**

After the main loop, find the line that moves the terminal cursor to the editor cursor position:

```rust
stdout.queue(cursor::MoveTo(
    (gutter + cursor_col.saturating_sub(col_offset)) as u16,
    (cursor_row.saturating_sub(scroll_offset)) as u16,
))?;
```

Replace it with:

```rust
let (cursor_screen_row, cursor_screen_col) = if wrap_width > 0 {
    // Find the visual row of the cursor in the visual_map
    let cursor_col_start = (cursor_col / wrap_width) * wrap_width;
    let vrow = visual_map
        .iter()
        .position(|&(r, cs)| r == cursor_row && cs == cursor_col_start)
        .unwrap_or(cursor_row);
    let vcol = cursor_col % wrap_width;
    (vrow.saturating_sub(scroll_offset), gutter + vcol)
} else {
    (
        cursor_row.saturating_sub(scroll_offset),
        gutter + cursor_col.saturating_sub(col_offset),
    )
};
stdout.queue(cursor::MoveTo(
    cursor_screen_col as u16,
    cursor_screen_row as u16,
))?;
```

- [ ] **Step 5: Build and smoke-test manually**

```
cargo build
cargo run -- src/main.rs
```

Open a file, verify:
- Long lines now wrap visually (soft wrap on by default)
- Ctrl+Shift+W toggles wrap off (long lines truncated again)
- Line numbers show only on first visual row of each buffer row

- [ ] **Step 6: Run full test suite**

```
cargo test
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/ui.rs src/editor.rs
git commit -m "feat(tui): implement soft wrap rendering and cursor positioning"
```

---

## Task 7: Reflow paragraph

**Spec ref:** §4 (ReflowParagraph behaviour, paragraph boundary, undo)

**Files:**
- Modify: `src/core/editor_core.rs`

- [ ] **Step 1: Write failing tests**

Add to the test module in `src/core/editor_core.rs`:

```rust
mod reflow_tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::config::Config;

    fn vp() -> ViewportMetrics { ViewportMetrics { rows: 24, cols: 80 } }

    fn make_core(text: &str, wrap_col: usize) -> EditorCore {
        let mut cfg = Config::default();
        cfg.wrap_column = wrap_col;
        EditorCore::new(Buffer::from_content(text.to_string()), cfg)
    }

    #[test]
    fn reflow_wraps_long_line() {
        let long = "This is a long line that should be reflowed at forty chars";
        let mut core = make_core(long, 40);
        core.apply_command(Command::ReflowParagraph, vp()).unwrap();
        let result = core.buffer().line(0);
        assert!(result.chars().count() <= 40, "first line too long: {:?}", result);
        // All original words still present
        let full: String = (0..core.buffer().line_count())
            .map(|r| core.buffer().line(r))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(full.contains("reflowed"));
    }

    #[test]
    fn reflow_respects_blank_line_boundary() {
        let text = "first paragraph\n\nsecond paragraph";
        let mut core = make_core(text, 80);
        // cursor is on first paragraph
        core.apply_command(Command::ReflowParagraph, vp()).unwrap();
        // second paragraph untouched
        let last = core.buffer().line(core.buffer().line_count() - 1);
        assert_eq!(last, "second paragraph");
    }

    #[test]
    fn reflow_is_undoable() {
        let long = "word1 word2 word3 word4 word5";
        let mut core = make_core(long, 10);
        core.apply_command(Command::ReflowParagraph, vp()).unwrap();
        core.apply_command(Command::Undo, vp()).unwrap();
        assert_eq!(core.buffer().line(0), long);
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test reflow
```

Expected: tests fail (ReflowParagraph is a no-op).

- [ ] **Step 3: Add `word_wrap_to` free function and `reflow_paragraph` method**

Add to `src/core/editor_core.rs` (outside `impl EditorCore`, e.g. near the top with other helpers):

```rust
/// Word-wrap `text` to `width` columns, inserting newlines at word boundaries.
/// Returns the reflowed string. A `width` of 0 returns the text unchanged.
fn word_wrap_to(text: &str, width: usize) -> String {
    if width == 0 {
        return text.to_string();
    }
    let mut result = String::new();
    let mut line_len = 0usize;
    for word in text.split_whitespace() {
        let wlen = word.chars().count();
        if result.is_empty() {
            result.push_str(word);
            line_len = wlen;
        } else if line_len + 1 + wlen <= width {
            result.push(' ');
            result.push_str(word);
            line_len += 1 + wlen;
        } else {
            result.push('\n');
            result.push_str(word);
            line_len = wlen;
        }
    }
    result
}
```

Add to `impl EditorCore`:

```rust
fn reflow_paragraph(&mut self, viewport: ViewportMetrics) {
    // 1. Find paragraph start: scan upward to blank line or buffer start.
    let mut start_row = self.cursor_row;
    while start_row > 0 && !self.buffer.line(start_row - 1).trim().is_empty() {
        start_row -= 1;
    }

    // 2. Find paragraph end: scan downward to blank line or buffer end.
    let mut end_row = self.cursor_row;
    let line_count = self.buffer.line_count();
    while end_row + 1 < line_count
        && !self.buffer.line(end_row + 1).trim().is_empty()
    {
        end_row += 1;
    }

    // 3. Join lines into one string (single space between lines).
    let joined: String = (start_row..=end_row)
        .map(|r| self.buffer.line(r))
        .collect::<Vec<_>>()
        .join(" ");

    // 4. Re-wrap to wrap_column.
    let reflowed = word_wrap_to(&joined, self.wrap_column);

    // 5. Compute char offsets of the paragraph in the buffer.
    let start_pos = self.buffer.char_offset_for(start_row, 0);
    let end_col = self.buffer.line_len(end_row);
    let end_pos = self.buffer.char_offset_for(end_row, end_col);

    // 6. Replace the paragraph as a single undo entry.
    let old_text: String = self.buffer.rope.slice(start_pos..end_pos).into();
    let cursor_before = (self.cursor_row, self.cursor_col);

    self.buffer.raw_delete(start_pos, end_pos - start_pos);
    self.buffer.raw_insert(start_pos, &reflowed);

    let new_len = reflowed.chars().count();
    let (new_row, new_col) = self.buffer.offset_to_row_col(start_pos + new_len);

    self.push_undo(
        vec![
            EditOp::Delete { pos: start_pos, text: old_text },
            EditOp::Insert { pos: start_pos, text: reflowed },
        ],
        cursor_before,
        (new_row, new_col),
    );

    self.cursor_row = new_row;
    self.cursor_col = new_col;
    self.desired_col = new_col;
    self.last_insert_at = None;
    if !self.redo_stack.is_empty() {
        self.save_depth = None;
    }
    self.redo_stack.clear();
    self.scroll_to_cursor(viewport);
}
```

- [ ] **Step 4: Wire `ReflowParagraph` into `apply_command`**

Replace the placeholder stub from Task 4:

```rust
Command::ReflowParagraph => {
    self.reflow_paragraph(viewport);
    None
}
```

- [ ] **Step 5: Run reflow tests**

```
cargo test reflow
```

Expected: 3 tests PASS.

- [ ] **Step 6: Run full suite**

```
cargo test
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/core/editor_core.rs
git commit -m "feat(core): implement reflow paragraph command"
```

---

## Task 8: GUI keybindings and soft wrap rendering

**Spec ref:** §2 (GUI bindings, macOS vs Win/Linux), §3 (GUI soft wrap via LayoutJob)

**Files:**
- Modify: `src/gui/mod.rs`

- [ ] **Step 1: Add word navigation to the key event handler**

In `handle_event()` in `src/gui/mod.rs`, in the `Key::ArrowLeft` / `Key::ArrowRight` block (currently plain movement), add before those arms:

```rust
// Word navigation: Option+Arrow on macOS, Ctrl+Arrow on Win/Linux
Key::ArrowLeft if modifiers.alt => {
    return self.apply_command(ctx, Command::MoveWordLeft);
}
Key::ArrowRight if modifiers.alt => {
    return self.apply_command(ctx, Command::MoveWordRight);
}
#[cfg(not(target_os = "macos"))]
Key::ArrowLeft if modifiers.ctrl => {
    return self.apply_command(ctx, Command::MoveWordLeft);
}
#[cfg(not(target_os = "macos"))]
Key::ArrowRight if modifiers.ctrl => {
    return self.apply_command(ctx, Command::MoveWordRight);
}
// Word delete
Key::Backspace if modifiers.alt || modifiers.ctrl => {
    return self.apply_command(ctx, Command::DeleteWordLeft);
}
Key::Delete if modifiers.alt || modifiers.ctrl => {
    return self.apply_command(ctx, Command::DeleteWordRight);
}
```

Place these arms **before** the existing `Key::ArrowLeft => ...` arm so they take priority.

- [ ] **Step 2: Add `ToggleSoftWrap` and `ReflowParagraph` to `map_shortcut`**

In `map_shortcut()` in `src/gui/mod.rs`, add to the match block:

```rust
// Soft wrap: Cmd+Shift+W (macOS) or Ctrl+Shift+W (Win/Linux)
Key::W if modifiers.shift => Some(Command::ToggleSoftWrap),
```

For `ReflowParagraph` (Alt+Q / Option+Q), it uses `alt` not `command/ctrl`, so handle it outside `map_shortcut`. In `handle_event()`, before the `map_shortcut` call, add:

```rust
// Reflow paragraph: Option+Q (macOS) / Alt+Q (Win/Linux)
if let egui::Event::Key { key: Key::Q, pressed: true, modifiers, .. } = &event {
    if modifiers.alt {
        return self.apply_command(ctx, Command::ReflowParagraph);
    }
}
```

Add this block near the top of `handle_event()` before the main `match event {`.

- [ ] **Step 3: Apply soft wrap in the GUI text rendering**

In `render_editor()` in `src/gui/mod.rs`, find where each line's `LayoutJob` is constructed and rendered. Read `self.core.snapshot().soft_wrap` to decide whether to wrap.

The egui text area already has a fixed width. When soft_wrap is on, do **not** set `egui::TextWrapMode::Extend` (or equivalent no-wrap setting); let egui's default wrap behaviour apply within the available width. When soft_wrap is off, set the layout job or painter to extend (no wrap).

The exact call depends on how the current `render_editor` constructs its `LayoutJob`. Look for a call like:

```rust
layout_job.wrap = egui::text::TextWrapping { ... };
```

or a `ui.label(...)` / `painter.galley(...)` call. Apply:

```rust
let snap = self.core.snapshot();
// soft wrap: leave egui's default wrapping in place when on;
// disable it (extend) when off.
if !snap.soft_wrap {
    layout_job.wrap = egui::text::TextWrapping {
        max_width: f32::INFINITY,
        ..Default::default()
    };
}
```

If `render_editor` currently uses `egui::ScrollArea` with horizontal scroll, gate horizontal scroll on `!snap.soft_wrap` as well.

- [ ] **Step 4: Build the GUI binary and smoke-test**

```
cargo build --bin rcte_gui
cargo run --bin rcte_gui -- src/main.rs
```

Verify:
- Option+Left/Right (macOS) or Ctrl+Left/Right (Win/Linux) jumps by word
- Option+Backspace deletes a word left
- Cmd+Shift+W (macOS) or Ctrl+Shift+W (Win/Linux) toggles wrap
- Alt+Q (Option+Q) reflows the paragraph under cursor
- Long lines wrap visually by default

- [ ] **Step 5: Run full test suite**

```
cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/gui/mod.rs
git commit -m "feat(gui): add OS-aware word nav, soft wrap, and reflow keybindings"
```

---

## Self-Review Checklist (do not skip)

Before declaring done, verify:

- [ ] `cargo test` passes with zero failures
- [ ] `cargo build` produces no warnings about unmatched `Command` variants (all 6 are handled in `apply_command`)
- [ ] Soft wrap default is `true` — opening a file with a long line wraps immediately
- [ ] Ctrl+Shift+W / Cmd+Shift+W toggles wrap off and back on (both TUI and GUI)
- [ ] Alt+Q / Option+Q reflows a multi-sentence paragraph correctly
- [ ] Undo after reflow restores original text
- [ ] Undo after word-delete restores deleted text
- [ ] Word navigation crosses line boundaries (Ctrl+Right at end of one line moves to start of first word on next line)
