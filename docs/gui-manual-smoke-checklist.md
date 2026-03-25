# GUI Manual Smoke Checklist

This checklist verifies the GUI MVP behaviors defined in the extension specs.

## Preconditions

- Build passes: `cargo test`
- Launch app: `cargo run --bin rcte-gui`
- Have a writable temp directory available.

## Smoke Steps

- [ ] **Launch and baseline UI**
  - Open `rcte-gui`.
  - Verify window opens with toolbar, editor viewport, status bar, and message bar.

- [ ] **Create and edit content**
  - Type `hello`, press `Enter`, type `world`.
  - Verify cursor movement with arrow keys, `Home`, `End`, `PageUp`, `PageDown`.

- [ ] **Backspace and Delete**
  - Use `Backspace` and `Delete` in the same line and across line boundaries.
  - Verify text updates as expected and cursor remains visible.

- [ ] **Undo / Redo**
  - Press `Ctrl+Z` then `Ctrl+Y`.
  - Verify text history is restored and re-applied.

- [ ] **Find next / previous**
  - Press `Ctrl+F`, enter a repeated token (for example `o`).
  - Press `Enter` or click `Next` to advance matches.
  - Click `Prev` to move to previous match.
  - Verify highlight and cursor jump to each match.

- [ ] **Open with unsaved-change guard**
  - Make an unsaved edit.
  - Press `Ctrl+O` once and verify warning appears.
  - Press `Ctrl+O` again within the confirmation window and verify file picker opens.

- [ ] **Save and Save As**
  - Press `Ctrl+S` on unnamed buffer and verify Save dialog appears.
  - Save to a new file and verify file exists with expected content.
  - Press `Ctrl+Shift+S` and save to a second filename.
  - Verify both files contain expected text.

- [ ] **Dirty indicator**
  - Modify the buffer after saving.
  - Verify modified indicator appears in status/title.
  - Save again and verify indicator clears.

- [ ] **Quit confirmation**
  - With unsaved changes, press `Ctrl+Q` once and verify warning.
  - Press `Ctrl+Q` again within confirmation window and verify app closes.
  - Repeat using window close button (`X`) to verify the same guard.

- [ ] **Config parity**
  - Set `~/.config/r-clite/config.toml` with non-default `tab_width`, `line_numbers`, and `theme`.
  - Relaunch `rcte-gui` and verify:
    - tab inserts configured number of spaces
    - line numbers reflect configured default visibility
    - colors reflect configured theme

## Shortcut Parity Quick Check

- [ ] `Ctrl+S`
- [ ] `Ctrl+Shift+S`
- [ ] `Ctrl+O`
- [ ] `Ctrl+Q`
- [ ] `Ctrl+F`
- [ ] `Ctrl+Z`
- [ ] `Ctrl+Y`
