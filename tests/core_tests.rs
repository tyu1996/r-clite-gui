use r_clite::{
    buffer::Buffer,
    config::Config,
    core::{EditorCore, ViewportMetrics},
    keymap::Command,
};

fn vp() -> ViewportMetrics {
    ViewportMetrics { rows: 20, cols: 80 }
}

fn make_core(content: &str) -> EditorCore {
    EditorCore::new(Buffer::from_content(content.to_string()), Config::default())
}

#[allow(dead_code)]
fn type_str(core: &mut EditorCore, s: &str) {
    for ch in s.chars() {
        core.apply_command(Command::InsertChar(ch), vp()).unwrap();
    }
}

// ── Cursor movement ──────────────────────────────────────────────────────────

#[test]
fn move_right_wraps_to_next_line_at_eol() {
    let mut core = make_core("ab\ncd\n");
    core.apply_command(Command::MoveLineEnd, vp()).unwrap(); // cursor at (0, 2)
    core.apply_command(Command::MoveRight, vp()).unwrap(); // wrap
    let snap = core.snapshot();
    assert_eq!(snap.cursor_row, 1);
    assert_eq!(snap.cursor_col, 0);
}

#[test]
fn move_left_wraps_to_prev_line_at_line_start() {
    let mut core = make_core("ab\ncd\n");
    core.apply_command(Command::MoveDown, vp()).unwrap(); // row 1, col 0
    core.apply_command(Command::MoveLeft, vp()).unwrap(); // wrap to end of row 0
    let snap = core.snapshot();
    assert_eq!(snap.cursor_row, 0);
    assert_eq!(snap.cursor_col, 2); // "ab" is 2 chars
}

#[test]
fn move_up_and_down_basic() {
    let mut core = make_core("ab\ncd\n");
    core.apply_command(Command::MoveDown, vp()).unwrap();
    assert_eq!(core.snapshot().cursor_row, 1);
    core.apply_command(Command::MoveUp, vp()).unwrap();
    assert_eq!(core.snapshot().cursor_row, 0);
}

#[test]
fn move_up_at_first_row_is_noop() {
    let mut core = make_core("ab\ncd\n");
    core.apply_command(Command::MoveUp, vp()).unwrap();
    assert_eq!(core.snapshot().cursor_row, 0);
}

#[test]
fn move_down_at_last_row_is_noop() {
    // "ab\ncd\n" → 3 lines: "ab", "cd", "" (trailing empty)
    let mut core = make_core("ab\ncd\n");
    core.apply_command(Command::MoveDown, vp()).unwrap(); // → row 1
    core.apply_command(Command::MoveDown, vp()).unwrap(); // → row 2 (empty)
    core.apply_command(Command::MoveDown, vp()).unwrap(); // stays at 2
    assert_eq!(core.snapshot().cursor_row, 2);
}

#[test]
fn move_line_start_and_end() {
    let mut core = make_core("hello\n");
    core.apply_command(Command::MoveLineEnd, vp()).unwrap();
    assert_eq!(core.snapshot().cursor_col, 5); // "hello" = 5 chars
    core.apply_command(Command::MoveLineStart, vp()).unwrap();
    assert_eq!(core.snapshot().cursor_col, 0);
}

#[test]
fn page_down_advances_cursor_row() {
    // 30 lines — enough to scroll
    let content: String = (0..30).map(|i| format!("line{}\n", i)).collect();
    let mut core = make_core(&content);
    core.apply_command(Command::PageDown, vp()).unwrap();
    assert!(core.snapshot().cursor_row >= 1);
}

#[test]
fn page_up_reduces_row_after_page_down() {
    let content: String = (0..30).map(|i| format!("line{}\n", i)).collect();
    let mut core = make_core(&content);
    core.apply_command(Command::PageDown, vp()).unwrap();
    let row_after = core.snapshot().cursor_row;
    core.apply_command(Command::PageUp, vp()).unwrap();
    assert!(core.snapshot().cursor_row < row_after);
}
