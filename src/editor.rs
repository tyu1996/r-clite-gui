// Core editor state and main event loop.
//
// Owns the Buffer, Terminal, and Ui instances and drives
// the read-key → update-state → render cycle.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event};

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
            Command::None => {}
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
