use egui_kittest::Harness;
use r_clite::{buffer::Buffer, config::Config, gui::GuiApp};

fn make_harness(content: &str) -> Harness<'_, GuiApp> {
    let buffer = Buffer::from_content(content.to_string());
    let mut harness = Harness::new_eframe(|_cc| GuiApp::new(buffer, Config::default(), None));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness
}

/// Build an egui Key event (pressed=true, no modifiers).
fn plain_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        pressed: true,
        modifiers: egui::Modifiers::default(),
        repeat: false,
        physical_key: None,
    }
}

/// Build a Ctrl+key event (pressed=true).
fn ctrl_key(key: egui::Key) -> egui::Event {
    let mut mods = egui::Modifiers::default();
    mods.ctrl = true;
    egui::Event::Key {
        key,
        pressed: true,
        modifiers: mods,
        repeat: false,
        physical_key: None,
    }
}

/// Compute the pixel origin of the text area (top-left of char at row 0, col 0).
/// Uses the known constants from gui/mod.rs: PANEL_PADDING=12.0, char_width=8.0, row_height=18.0.
fn editor_text_origin(harness: &Harness<GuiApp>) -> egui::Pos2 {
    let rect = harness
        .state()
        .editor_rect()
        .expect("editor_rect is None — make sure harness.run_steps(2) was called");
    let line_count = harness.state().core().buffer().line_count();
    let show_ln = harness.state().core().snapshot().show_line_numbers;
    let gutter_chars = if show_ln {
        line_count.to_string().len() + 1
    } else {
        0
    };
    let gutter_px = gutter_chars as f32 * 8.0; // char_width constant in gui/mod.rs
    egui::Pos2::new(
        rect.left() + 12.0 + gutter_px, // PANEL_PADDING = 12.0
        rect.top() + 12.0,
    )
}

// ── Initialization ───────────────────────────────────────────────────────────

#[test]
fn initial_buffer_content_is_preserved() {
    let harness = make_harness("hello\nworld\n");
    assert_eq!(harness.state().core().buffer().line(0), "hello");
    assert_eq!(harness.state().core().buffer().line(1), "world");
}

#[test]
fn empty_buffer_starts_with_one_line() {
    let harness = make_harness("");
    assert_eq!(harness.state().core().buffer().line_count(), 1);
}

// ── Text input ───────────────────────────────────────────────────────────────

#[test]
fn text_event_inserts_chars_into_buffer() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("hello".to_owned()));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line(0), "hello");
}

#[test]
fn backspace_key_removes_last_typed_char() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("ab".to_owned()));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness.event(plain_key(egui::Key::Backspace));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line(0), "a");
}

#[test]
fn enter_key_splits_line() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("ab".to_owned()));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness.event(plain_key(egui::Key::Enter));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line_count(), 2);
}

// ── Cursor navigation ────────────────────────────────────────────────────────

#[test]
fn arrow_right_advances_cursor_col() {
    let mut harness = make_harness("hello\n");
    harness.event(plain_key(egui::Key::ArrowRight));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_col, 1);
}

#[test]
fn arrow_down_advances_cursor_row() {
    let mut harness = make_harness("line1\nline2\n");
    harness.event(plain_key(egui::Key::ArrowDown));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_row, 1);
}

#[test]
fn home_and_end_keys_move_to_line_boundaries() {
    let mut harness = make_harness("hello\n");
    harness.event(plain_key(egui::Key::End));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_col, 5);
    harness.event(plain_key(egui::Key::Home));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_col, 0);
}

// ── Mouse interaction ────────────────────────────────────────────────────────

#[test]
fn click_at_row_0_positions_cursor_at_row_0() {
    let mut harness = make_harness("hello\nworld\n");
    let origin = editor_text_origin(&harness);
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + 0.5, origin.y + 0.5),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_row, 0);
}

#[test]
fn click_at_second_row_y_positions_cursor_at_row_1() {
    let mut harness = make_harness("hello\nworld\n");
    let origin = editor_text_origin(&harness);
    let row_height = 18.0_f32; // hardcoded in mouse_pos_to_row_col
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + 0.5, origin.y + row_height + 0.5),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().snapshot().cursor_row, 1);
}

#[test]
fn drag_across_chars_creates_selection() {
    let mut harness = make_harness("hello world\n");
    let origin = editor_text_origin(&harness);
    let char_width = 8.0_f32;
    // Press at col 0
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + 0.5, origin.y + 0.5),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    // Move to col 5
    harness.event(egui::Event::PointerMoved(egui::Pos2::new(
        origin.x + char_width * 5.0,
        origin.y + 0.5,
    )));
    // Release
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + char_width * 5.0, origin.y + 0.5),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().has_selection());
}

#[test]
fn click_without_drag_clears_selection() {
    let mut harness = make_harness("hello world\n");
    // Establish a selection via Ctrl+A
    harness.event(ctrl_key(egui::Key::A));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().has_selection());

    // Click without dragging — sets selection_start == selection_end → not selected
    let origin = editor_text_origin(&harness);
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + 0.5, origin.y + 0.5),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    harness.event(egui::Event::PointerButton {
        pos: egui::Pos2::new(origin.x + 0.5, origin.y + 0.5),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(!harness.state().core().has_selection());
}

// ── Hotkey integration ───────────────────────────────────────────────────────

#[test]
fn ctrl_z_undoes_typed_text() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("hello".to_owned()));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line(0), "hello");

    harness.event(ctrl_key(egui::Key::Z));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line(0), "");
}

#[test]
fn ctrl_y_redoes_undone_edit() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("hello".to_owned()));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness.event(ctrl_key(egui::Key::Z)); // undo
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness.event(ctrl_key(egui::Key::Y)); // redo
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(harness.state().core().buffer().line(0), "hello");
}

#[test]
fn ctrl_f_activates_search_mode() {
    let mut harness = make_harness("hello\n");
    harness.event(ctrl_key(egui::Key::F));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().snapshot().search.is_some());
}

#[test]
fn ctrl_q_sets_should_quit_on_clean_buffer() {
    // Buffer::from_content marks dirty=false; no edits → clean
    let mut harness = make_harness("hello\n");
    harness.event(ctrl_key(egui::Key::Q));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().snapshot().should_quit);
}

#[test]
fn ctrl_l_toggles_line_numbers() {
    let mut harness = make_harness("");
    let initial = harness.state().core().snapshot().show_line_numbers;
    harness.event(ctrl_key(egui::Key::L));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert_eq!(
        harness.state().core().snapshot().show_line_numbers,
        !initial
    );
}

#[test]
fn ctrl_a_selects_all_text() {
    let mut harness = make_harness("hello world\n");
    harness.event(ctrl_key(egui::Key::A));
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().has_selection());
}

#[test]
fn ctrl_x_cuts_selected_text_from_buffer() {
    let mut harness = make_harness("hello world\n");
    harness.event(ctrl_key(egui::Key::A)); // select all
    harness.run_steps(2); // two frames: initial layout + repaint flush
    harness.event(ctrl_key(egui::Key::X)); // cut
    harness.run_steps(2); // two frames: initial layout + repaint flush
    // The selected text is deleted from the buffer regardless of clipboard availability
    assert_eq!(harness.state().core().buffer().rope.to_string(), "");
}

#[test]
fn shift_arrow_right_extends_selection() {
    let mut harness = make_harness("hello\n");
    let mut mods = egui::Modifiers::default();
    mods.shift = true;
    harness.event(egui::Event::Key {
        key: egui::Key::ArrowRight,
        pressed: true,
        modifiers: mods,
        repeat: false,
        physical_key: None,
    });
    harness.run_steps(2); // two frames: initial layout + repaint flush
    assert!(harness.state().core().has_selection());
}
