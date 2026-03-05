// Core editor state and main event loop.
//
// Owns the Buffer, Terminal, and Ui instances and drives
// the read-key → update-state → render cycle.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};

use crate::buffer::Buffer;
use crate::keymap::{self, Command};
use crate::terminal::RawModeGuard;
use crate::ui::Ui;

/// The core editor.
///
/// Holds all persistent state for an editing session: the text buffer, cursor
/// position, viewport scroll offset, and the active terminal guard.
pub struct Editor {
    /// The text buffer being edited.
    buffer: Buffer,
    /// RAII guard — restores raw mode and alternate screen on drop.
    _guard: RawModeGuard,
    /// Rendering engine.
    ui: Ui,

    // ── Cursor ────────────────────────────────────────────────────────────────
    /// Current cursor row (0-indexed, clamped to `[0, line_count - 1]`).
    cursor_row: usize,
    /// Current cursor column (0-indexed, clamped to the line length).
    cursor_col: usize,
    /// The column the user is "trying" to be at.
    ///
    /// When moving vertically to a shorter line the column is clamped, but
    /// `desired_col` keeps the original value so that moving back to a longer
    /// line restores the user's intended position.
    desired_col: usize,

    // ── Viewport ──────────────────────────────────────────────────────────────
    /// Index of the first line visible in the viewport (vertical scroll).
    scroll_offset: usize,

    // ── Quit guard ────────────────────────────────────────────────────────────
    /// Number of Ctrl+Q presses seen since the last unsaved-changes warning.
    quit_count: u8,
    /// Timestamp of the first Ctrl+Q press in the current quit sequence.
    last_quit_at: Option<Instant>,

    // ── Message bar ───────────────────────────────────────────────────────────
    /// Current message bar text and the time it was posted.
    message: Option<(String, Instant)>,

    // ── Save-As prompt ────────────────────────────────────────────────────────
    /// When `Some`, the editor is collecting a filename from the user.
    /// Keystrokes are redirected to building this string until Enter or Esc.
    save_prompt: Option<String>,

    /// Set to `true` when the event loop should exit after the next render.
    should_quit: bool,
}

impl Editor {
    /// Construct a new editor for the given buffer and enter raw mode.
    pub fn new(buffer: Buffer) -> Result<Self> {
        let guard = RawModeGuard::new()?;
        let (w, h) = crate::terminal::size()?;
        let ui = Ui::new(w as usize, h as usize);

        Ok(Self {
            buffer,
            _guard: guard,
            ui,
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
            scroll_offset: 0,
            quit_count: 0,
            last_quit_at: None,
            message: None,
            save_prompt: None,
            should_quit: false,
        })
    }

    /// Run the editor event loop until the user quits.
    pub fn run(&mut self) -> Result<()> {
        self.set_message("Ctrl+Q to quit".to_string());

        loop {
            // Refresh terminal dimensions each frame so the editor handles
            // terminal resize transparently.
            if let Ok((w, h)) = crate::terminal::size() {
                self.ui.width = w as usize;
                self.ui.height = h as usize;
            }

            self.ui.render(
                &self.buffer,
                self.cursor_row,
                self.cursor_col,
                self.scroll_offset,
                self.current_message(),
            )?;

            if self.should_quit {
                break;
            }

            if let Event::Key(key) = event::read()? {
                self.handle_key(key)?;
            }
        }

        Ok(())
    }

    // ── Message helpers ────────────────────────────────────────────────────────

    fn set_message(&mut self, msg: String) {
        self.message = Some((msg, Instant::now()));
    }

    /// Returns the message text if it is still within its 5-second display
    /// window, otherwise `None`.
    fn current_message(&self) -> Option<&str> {
        match &self.message {
            Some((text, posted_at)) if posted_at.elapsed() < Duration::from_secs(5) => {
                Some(text.as_str())
            }
            _ => None,
        }
    }

    // ── Input dispatch ─────────────────────────────────────────────────────────

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        // While collecting a Save-As filename, redirect all keystrokes.
        if self.save_prompt.is_some() {
            return self.handle_prompt_key(key);
        }

        match keymap::map(key) {
            Command::MoveUp => self.move_up(),
            Command::MoveDown => self.move_down(),
            Command::MoveLeft => self.move_left(),
            Command::MoveRight => self.move_right(),
            Command::MoveLineStart => self.move_line_start(),
            Command::MoveLineEnd => self.move_line_end(),
            Command::PageUp => self.page_up(),
            Command::PageDown => self.page_down(),
            Command::Quit => self.handle_quit()?,
            Command::InsertChar(ch) => self.handle_insert_char(ch),
            Command::Backspace => self.handle_backspace(),
            Command::DeleteChar => self.handle_delete(),
            Command::InsertNewline => self.handle_newline(),
            Command::InsertTab => self.handle_tab(),
            Command::Save => self.handle_save()?,
            Command::SaveAs => self.start_save_prompt(),
            Command::None => {}
        }
        Ok(())
    }

    // ── Editing ───────────────────────────────────────────────────────────────

    fn handle_insert_char(&mut self, ch: char) {
        self.buffer.insert_char(self.cursor_row, self.cursor_col, ch);
        self.cursor_col += 1;
        self.desired_col = self.cursor_col;
        self.scroll_to_cursor();
    }

    fn handle_backspace(&mut self) {
        let (new_row, new_col) =
            self.buffer.delete_char_before(self.cursor_row, self.cursor_col);
        self.cursor_row = new_row;
        self.cursor_col = new_col;
        self.desired_col = new_col;
        self.scroll_to_cursor();
    }

    fn handle_delete(&mut self) {
        self.buffer.delete_char_at(self.cursor_row, self.cursor_col);
        // Cursor stays at the same position; clamp in case line shrank.
        let line_len = self.buffer.line_len(self.cursor_row);
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
            self.desired_col = line_len;
        }
        self.scroll_to_cursor();
    }

    fn handle_newline(&mut self) {
        self.buffer.insert_newline(self.cursor_row, self.cursor_col);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.scroll_to_cursor();
    }

    fn handle_tab(&mut self) {
        const TAB_WIDTH: usize = 4;
        let spaces = " ".repeat(TAB_WIDTH);
        self.buffer.insert_str(self.cursor_row, self.cursor_col, &spaces);
        self.cursor_col += TAB_WIDTH;
        self.desired_col = self.cursor_col;
        self.scroll_to_cursor();
    }

    // ── Save ──────────────────────────────────────────────────────────────────

    fn handle_save(&mut self) -> Result<()> {
        if self.buffer.path.is_none() {
            // No path yet — behave like Save As.
            self.start_save_prompt();
        } else {
            self.do_save();
        }
        Ok(())
    }

    fn start_save_prompt(&mut self) {
        self.save_prompt = Some(String::new());
        self.set_message("Save as: ".to_string());
    }

    fn do_save(&mut self) {
        match self.buffer.save() {
            Ok(bytes) => {
                let name = self.buffer.display_name();
                self.set_message(format!("{} written — {} bytes", name, bytes));
            }
            Err(e) => {
                self.set_message(format!("Save error: {:#}", e));
            }
        }
    }

    // ── Save-As prompt ────────────────────────────────────────────────────────

    fn handle_prompt_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let filename = self.save_prompt.take().unwrap_or_default();
                if filename.is_empty() {
                    self.set_message("Save cancelled.".to_string());
                } else {
                    match self.buffer.save_to(PathBuf::from(filename)) {
                        Ok(bytes) => {
                            let name = self.buffer.display_name();
                            self.set_message(format!("{} written — {} bytes", name, bytes));
                        }
                        Err(e) => {
                            self.set_message(format!("Save error: {:#}", e));
                        }
                    }
                }
            }
            KeyCode::Esc => {
                self.save_prompt = None;
                self.set_message("Save cancelled.".to_string());
            }
            KeyCode::Backspace => {
                if let Some(ref mut s) = self.save_prompt {
                    s.pop();
                    let display = format!("Save as: {}", s);
                    self.set_message(display);
                }
            }
            KeyCode::Char(ch) => {
                if let Some(ref mut s) = self.save_prompt {
                    s.push(ch);
                    let display = format!("Save as: {}", s);
                    self.set_message(display);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Cursor movement ────────────────────────────────────────────────────────

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col_to_line();
            self.scroll_to_cursor();
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.buffer.line_count() {
            self.cursor_row += 1;
            self.clamp_col_to_line();
            self.scroll_to_cursor();
        }
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            // Wrap to end of previous line.
            self.cursor_row -= 1;
            self.cursor_col = self.buffer.line_len(self.cursor_row);
        }
        // Horizontal movement resets the desired column.
        self.desired_col = self.cursor_col;
        self.scroll_to_cursor();
    }

    fn move_right(&mut self) {
        let line_len = self.buffer.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.buffer.line_count() {
            // Wrap to start of next line.
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.desired_col = self.cursor_col;
        self.scroll_to_cursor();
    }

    fn move_line_start(&mut self) {
        self.cursor_col = 0;
        self.desired_col = 0;
        self.scroll_to_cursor();
    }

    fn move_line_end(&mut self) {
        self.cursor_col = self.buffer.line_len(self.cursor_row);
        self.desired_col = self.cursor_col;
        self.scroll_to_cursor();
    }

    fn page_up(&mut self) {
        let rows = self.ui.viewport_rows();
        self.cursor_row = self.cursor_row.saturating_sub(rows);
        self.clamp_col_to_line();
        self.scroll_to_cursor();
    }

    fn page_down(&mut self) {
        let rows = self.ui.viewport_rows();
        let last_line = self.buffer.line_count().saturating_sub(1);
        self.cursor_row = (self.cursor_row + rows).min(last_line);
        self.clamp_col_to_line();
        self.scroll_to_cursor();
    }

    // ── Clamping & scrolling ──────────────────────────────────────────────────

    /// Clamp `cursor_col` to the length of the current line while honouring
    /// `desired_col`.
    ///
    /// Used after vertical movement: the column is set to the smaller of the
    /// desired column and the length of the new line.
    fn clamp_col_to_line(&mut self) {
        let line_len = self.buffer.line_len(self.cursor_row);
        self.cursor_col = self.desired_col.min(line_len);
    }

    /// Adjust `scroll_offset` so the cursor row is always visible.
    fn scroll_to_cursor(&mut self) {
        let rows = self.ui.viewport_rows();
        if self.cursor_row < self.scroll_offset {
            self.scroll_offset = self.cursor_row;
        } else if rows > 0 && self.cursor_row >= self.scroll_offset + rows {
            self.scroll_offset = self.cursor_row - rows + 1;
        }
    }

    // ── Quit ──────────────────────────────────────────────────────────────────

    fn handle_quit(&mut self) -> Result<()> {
        if self.buffer.is_dirty() {
            let now = Instant::now();
            let within_window = self
                .last_quit_at
                .map(|t| now.duration_since(t) < Duration::from_secs(3))
                .unwrap_or(false);

            if within_window && self.quit_count >= 1 {
                // Second Ctrl+Q within 3 seconds — force quit.
                self.should_quit = true;
            } else {
                // First Ctrl+Q with dirty buffer — show warning.
                self.quit_count = 1;
                self.last_quit_at = Some(now);
                self.set_message(
                    "WARNING: File has unsaved changes. Press Ctrl+Q again to quit.".to_string(),
                );
            }
        } else {
            self.should_quit = true;
        }
        Ok(())
    }
}
