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

// ── Text editing ─────────────────────────────────────────────────────────────

#[test]
fn insert_tab_expands_to_four_spaces() {
    let mut core = make_core("");
    core.apply_command(Command::InsertTab, vp()).unwrap();
    assert_eq!(core.buffer().line(0), "    "); // 4 spaces (default tab_width)
    assert_eq!(core.snapshot().cursor_col, 4);
}

#[test]
fn insert_newline_splits_line_at_cursor() {
    let mut core = make_core("abcd\n");
    // Move to col 2 (between 'b' and 'c')
    core.apply_command(Command::MoveRight, vp()).unwrap();
    core.apply_command(Command::MoveRight, vp()).unwrap();
    core.apply_command(Command::InsertNewline, vp()).unwrap();
    // "ab" on line 0, "cd" on line 1, "" on line 2
    assert_eq!(core.buffer().line_count(), 3);
    assert_eq!(core.buffer().line(0), "ab");
    assert_eq!(core.buffer().line(1), "cd");
    assert_eq!(core.snapshot().cursor_row, 1);
    assert_eq!(core.snapshot().cursor_col, 0);
}

// ── Dirty flag ───────────────────────────────────────────────────────────────

#[test]
fn fresh_buffer_is_not_dirty() {
    let core = make_core("some content");
    assert!(!core.buffer().is_dirty());
}

#[test]
fn insert_char_marks_buffer_dirty() {
    let mut core = make_core("");
    core.apply_command(Command::InsertChar('x'), vp()).unwrap();
    assert!(core.buffer().is_dirty());
}

#[test]
fn line_count_increases_after_newline_insertion() {
    let mut core = make_core("hello\n");
    assert_eq!(core.buffer().line_count(), 2); // "hello", ""
    core.apply_command(Command::InsertNewline, vp()).unwrap();
    assert_eq!(core.buffer().line_count(), 3);
}

// ── Search ───────────────────────────────────────────────────────────────────

#[test]
fn search_with_no_match_leaves_search_match_none() {
    let mut core = make_core("hello world\n");
    core.start_search();
    core.set_search_query("xyz".to_string(), vp());
    assert_eq!(core.snapshot().search_match, None);
}

// ── Selection ────────────────────────────────────────────────────────────────

#[test]
fn selection_across_lines_includes_newline() {
    let mut core = make_core("ab\ncd\n");
    // Select from start of row 0 to start of row 1 — covers "ab\n"
    core.set_selection_start(0, 0);
    core.set_selection_end(1, 0);
    let text = core.get_selected_text().unwrap();
    assert_eq!(text, "ab\n");
}

// ── Case-sensitive search ─────────────────────────────────────────────────────

#[test]
fn case_sensitive_search_finds_exact_match() {
    let mut core = make_core("Hello hello HELLO");
    core.apply_command(Command::Find, vp()).unwrap();
    core.toggle_case_sensitive(vp()); // enable case-sensitive
    core.set_search_query("HELLO".to_string(), vp());
    // Should find position 12 (uppercase HELLO), not 0 (Hello) or 6 (hello)
    assert_eq!(core.snapshot().search_match, Some((12, 5)));
}

#[test]
fn case_insensitive_search_finds_all_variants() {
    let mut core = make_core("Hello hello HELLO");
    core.apply_command(Command::Find, vp()).unwrap();
    // case_sensitive is false by default, so case-insensitive search works
    core.set_search_query("hello".to_string(), vp());
    // Finds first at position 0
    assert_eq!(core.snapshot().search_match, Some((0, 5)));
    core.search_next(vp());
    // Second at position 6
    assert_eq!(core.snapshot().search_match, Some((6, 5)));
    core.search_next(vp());
    // Wraps to 12
    assert_eq!(core.snapshot().search_match, Some((12, 5)));
}

#[test]
fn toggle_case_sensitive_switches_behavior() {
    let mut core = make_core("Hello");
    core.apply_command(Command::Find, vp()).unwrap();
    core.set_search_query("hello".to_string(), vp());
    assert!(core.snapshot().search_match.is_some()); // found
    core.toggle_case_sensitive(vp());
    assert_eq!(core.snapshot().search_match, None); // no match when case-sensitive
    core.toggle_case_sensitive(vp());
    assert!(core.snapshot().search_match.is_some()); // back to found
}

// ── Replace ───────────────────────────────────────────────────────────────────

#[test]
fn replace_one_replaces_current_match_and_advances() {
    let mut core = make_core("Hello world");
    core.apply_command(Command::Find, vp()).unwrap();
    core.set_search_query("world".to_string(), vp());
    assert_eq!(core.snapshot().search_match, Some((6, 5)));
    core.set_search_replacement_for_test("rust".to_string());
    core.apply_replace_one(vp());
    let content = core.buffer().content();
    assert!(content.contains("rust"));
    assert!(!content.contains("world"));
}

#[test]
fn replace_all_replaces_everything() {
    let mut core = make_core("foo foo foo");
    core.apply_command(Command::Find, vp()).unwrap();
    core.set_search_query("foo".to_string(), vp());
    core.set_search_replacement_for_test("bar".to_string());
    core.apply_replace_all(vp());
    let content = core.buffer().content();
    assert_eq!(content, "bar bar bar");
}
