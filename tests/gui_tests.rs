#![allow(dead_code)]

use egui_kittest::Harness;
use r_clite::{buffer::Buffer, config::Config, gui::GuiApp};

fn make_harness(content: &str) -> Harness<'_, GuiApp> {
    let buffer = Buffer::from_content(content.to_string());
    let mut harness = Harness::new_eframe(|_cc| GuiApp::new(buffer, Config::default(), None));
    harness.run_steps(2);
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
        .expect("editor_rect is None — make sure harness.run() was called");
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
    harness.run_steps(2);
    assert_eq!(harness.state().core().buffer().line(0), "hello");
}

#[test]
fn backspace_key_removes_last_typed_char() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("ab".to_owned()));
    harness.run_steps(2);
    harness.event(plain_key(egui::Key::Backspace));
    harness.run_steps(2);
    assert_eq!(harness.state().core().buffer().line(0), "a");
}

#[test]
fn enter_key_splits_line() {
    let mut harness = make_harness("");
    harness.event(egui::Event::Text("ab".to_owned()));
    harness.run_steps(2);
    harness.event(plain_key(egui::Key::Enter));
    harness.run_steps(2);
    assert_eq!(harness.state().core().buffer().line_count(), 2);
}
