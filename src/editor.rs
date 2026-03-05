// Core editor state and main event loop.
//
// Owns the Buffer, Terminal, and Ui instances and drives
// the read-key → update-state → render cycle.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};

use crate::buffer::Buffer;
use crate::config::Config;
use crate::keymap::{self, Command};
use crate::terminal::RawModeGuard;
use crate::ui::{RenderState, Ui};

// ── Undo / Redo ───────────────────────────────────────────────────────────────

/// A single reversible edit stored as rope char-offset operations.
#[derive(Debug, Clone)]
enum EditOp {
    /// Text was inserted at char offset `pos`. Reverse: delete `text.len_chars()`.
    Insert { pos: usize, text: String },
    /// Text was deleted from char offset `pos`. Reverse: insert `text` at `pos`.
    Delete { pos: usize, text: String },
}

/// One undo step (possibly a group of rapid char inserts).
#[derive(Debug, Clone)]
struct UndoEntry {
    /// The operation(s) that were applied (in order). Undo applies them in reverse.
    ops: Vec<EditOp>,
    /// Cursor position before the edit — restored on undo.
    cursor_before: (usize, usize),
    /// Cursor position after the edit — restored on redo.
    cursor_after: (usize, usize),
}

// ── Search state ──────────────────────────────────────────────────────────────

struct SearchState {
    query: String,
    /// Char offset of the current highlighted match, if any.
    current_match: Option<usize>,
    /// Cursor and scroll position when search was invoked (restored on Esc).
    saved_cursor: (usize, usize),
    saved_scroll: usize,
    saved_col_offset: usize,
}

// ── Editor ────────────────────────────────────────────────────────────────────

/// The core editor.
pub struct Editor {
    buffer: Buffer,
    _guard: RawModeGuard,
    ui: Ui,
    config: Config,

    // ── Cursor ────────────────────────────────────────────────────────────────
    cursor_row: usize,
    cursor_col: usize,
    /// The column the user is "trying" to be at (preserved across vertical moves).
    desired_col: usize,

    // ── Viewport ──────────────────────────────────────────────────────────────
    /// Index of the first visible line (vertical scroll).
    scroll_offset: usize,
    /// Index of the first visible column (horizontal scroll).
    col_offset: usize,

    // ── Quit guard ────────────────────────────────────────────────────────────
    quit_count: u8,
    last_quit_at: Option<Instant>,

    // ── Message bar ───────────────────────────────────────────────────────────
    message: Option<(String, Instant)>,

    // ── Save-As prompt ────────────────────────────────────────────────────────
    save_prompt: Option<String>,

    // ── Undo / Redo ───────────────────────────────────────────────────────────
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    /// Time of the last single-char insert (for grouping).
    last_insert_at: Option<Instant>,

    // ── Search ────────────────────────────────────────────────────────────────
    search: Option<SearchState>,

    // ── Save point (for undo-to-clean-state dirty-flag clearing) ─────────────
    /// Undo stack depth at the time of the last successful save.
    save_depth: Option<usize>,

    should_quit: bool,
}

impl Editor {
    /// Construct a new editor for the given buffer and enter raw mode.
    pub fn new(buffer: Buffer, config: Config) -> Result<Self> {
        let guard = RawModeGuard::new()?;
        let (w, h) = crate::terminal::size()?;
        let mut ui = Ui::new(w as usize, h as usize);
        ui.show_line_numbers = config.line_numbers;
        ui.theme = config.theme.clone();

        Ok(Self {
            buffer,
            _guard: guard,
            ui,
            config,
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
            scroll_offset: 0,
            col_offset: 0,
            quit_count: 0,
            last_quit_at: None,
            message: None,
            save_prompt: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_insert_at: None,
            search: None,
            save_depth: None,
            should_quit: false,
        })
    }

    /// Set a startup message (e.g., config warnings). Called before `run`.
    pub fn set_startup_message(&mut self, msg: String) {
        self.set_message(msg);
    }

    /// Run the editor event loop until the user quits.
    pub fn run(&mut self) -> Result<()> {
        self.set_message("Ctrl+Q to quit  Ctrl+S save  Ctrl+Z undo  Ctrl+F find".to_string());

        loop {
            if let Ok((w, h)) = crate::terminal::size() {
                self.ui.width = w as usize;
                self.ui.height = h as usize;
            }

            let file_ext = self
                .buffer
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .and_then(|e| e.to_str())
                .map(|s| s.to_string());

            let search_match = self.search.as_ref().and_then(|s| {
                s.current_match.map(|offset| (offset, s.query.chars().count()))
            });

            self.ui.render(
                &self.buffer,
                &RenderState {
                    cursor_row: self.cursor_row,
                    cursor_col: self.cursor_col,
                    scroll_offset: self.scroll_offset,
                    col_offset: self.col_offset,
                    message: self.current_message(),
                    search_match,
                    file_ext: file_ext.as_deref(),
                },
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
        if self.save_prompt.is_some() {
            return self.handle_prompt_key(key);
        }
        if self.search.is_some() {
            return self.handle_search_key(key);
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
            Command::Undo => self.handle_undo(),
            Command::Redo => self.handle_redo(),
            Command::Find => self.start_search(),
            Command::ToggleLineNumbers => {
                self.ui.show_line_numbers = !self.ui.show_line_numbers;
            }
            Command::None => {}
        }
        Ok(())
    }

    // ── Editing ───────────────────────────────────────────────────────────────

    fn handle_insert_char(&mut self, ch: char) {
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        self.buffer.insert_char(self.cursor_row, self.cursor_col, ch);
        self.cursor_col += 1;
        self.desired_col = self.cursor_col;

        // Undo grouping: merge with last entry if it's an adjacent insert and
        // was made within 1 second.
        let within_window = self
            .last_insert_at
            .map(|t| t.elapsed() < Duration::from_secs(1))
            .unwrap_or(false);
        let merged = if within_window {
            if let Some(entry) = self.undo_stack.last_mut() {
                if let Some(EditOp::Insert { pos: epos, text }) = entry.ops.last_mut() {
                    if *epos + text.chars().count() == pos {
                        text.push(ch);
                        entry.cursor_after = (self.cursor_row, self.cursor_col);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if !merged {
            self.push_undo(
                vec![EditOp::Insert { pos, text: ch.to_string() }],
                (self.cursor_row, self.cursor_col - 1),
                (self.cursor_row, self.cursor_col),
            );
        }

        self.last_insert_at = Some(Instant::now());
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor();
    }

    fn handle_backspace(&mut self) {
        // Capture the char before deleting.
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        if pos == 0 {
            return;
        }
        let del_pos = pos - 1;
        let ch = match self.buffer.char_at_offset(del_pos) {
            Some(c) => c,
            None => return,
        };
        let cursor_before = (self.cursor_row, self.cursor_col);
        let (new_row, new_col) =
            self.buffer.delete_char_before(self.cursor_row, self.cursor_col);
        self.cursor_row = new_row;
        self.cursor_col = new_col;
        self.desired_col = new_col;

        self.push_undo(
            vec![EditOp::Delete { pos: del_pos, text: ch.to_string() }],
            cursor_before,
            (new_row, new_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor();
    }

    fn handle_delete(&mut self) {
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        let ch = match self.buffer.char_at_offset(pos) {
            Some(c) => c,
            None => return,
        };
        let cursor_before = (self.cursor_row, self.cursor_col);
        self.buffer.delete_char_at(self.cursor_row, self.cursor_col);
        let line_len = self.buffer.line_len(self.cursor_row);
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
            self.desired_col = line_len;
        }

        self.push_undo(
            vec![EditOp::Delete { pos, text: ch.to_string() }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor();
    }

    fn handle_newline(&mut self) {
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        let cursor_before = (self.cursor_row, self.cursor_col);
        self.buffer.insert_newline(self.cursor_row, self.cursor_col);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.desired_col = 0;

        self.push_undo(
            vec![EditOp::Insert { pos, text: "\n".to_string() }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor();
    }

    fn handle_tab(&mut self) {
        let tab_width = self.config.tab_width;
        let spaces = " ".repeat(tab_width);
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        let cursor_before = (self.cursor_row, self.cursor_col);
        self.buffer.insert_str(self.cursor_row, self.cursor_col, &spaces);
        self.cursor_col += tab_width;
        self.desired_col = self.cursor_col;

        self.push_undo(
            vec![EditOp::Insert { pos, text: spaces }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor();
    }

    // ── Undo / Redo ───────────────────────────────────────────────────────────

    fn push_undo(&mut self, ops: Vec<EditOp>, cursor_before: (usize, usize), cursor_after: (usize, usize)) {
        self.undo_stack.push(UndoEntry { ops, cursor_before, cursor_after });
    }

    fn handle_undo(&mut self) {
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => {
                self.set_message("Nothing to undo.".to_string());
                return;
            }
        };

        // Apply reverse ops in reverse order.
        for op in entry.ops.iter().rev() {
            match op {
                EditOp::Insert { pos, text } => {
                    let len = text.chars().count();
                    self.buffer.raw_delete(*pos, len);
                }
                EditOp::Delete { pos, text } => {
                    self.buffer.raw_insert(*pos, text);
                }
            }
        }

        let (row, col) = entry.cursor_before;
        self.cursor_row = row.min(self.buffer.line_count().saturating_sub(1));
        self.cursor_col = col.min(self.buffer.line_len(self.cursor_row));
        self.desired_col = self.cursor_col;

        // Build a redo entry (reverse of the undo entry).
        let redo_ops: Vec<EditOp> = entry.ops.iter().map(|op| match op {
            EditOp::Insert { pos, text } => EditOp::Delete { pos: *pos, text: text.clone() },
            EditOp::Delete { pos, text } => EditOp::Insert { pos: *pos, text: text.clone() },
        }).collect();
        self.redo_stack.push(UndoEntry {
            ops: redo_ops,
            cursor_before: entry.cursor_after,
            cursor_after: entry.cursor_before,
        });

        self.last_insert_at = None;
        if Some(self.undo_stack.len()) == self.save_depth {
            self.buffer.clear_dirty();
        }
        self.scroll_to_cursor();
    }

    fn handle_redo(&mut self) {
        let entry = match self.redo_stack.pop() {
            Some(e) => e,
            None => {
                self.set_message("Nothing to redo.".to_string());
                return;
            }
        };

        for op in entry.ops.iter() {
            match op {
                EditOp::Insert { pos, text } => {
                    self.buffer.raw_insert(*pos, text);
                }
                EditOp::Delete { pos, text } => {
                    let len = text.chars().count();
                    self.buffer.raw_delete(*pos, len);
                }
            }
        }

        let (row, col) = entry.cursor_before;
        self.cursor_row = row.min(self.buffer.line_count().saturating_sub(1));
        self.cursor_col = col.min(self.buffer.line_len(self.cursor_row));
        self.desired_col = self.cursor_col;

        // Push back to undo stack.
        let undo_ops: Vec<EditOp> = entry.ops.iter().map(|op| match op {
            EditOp::Insert { pos, text } => EditOp::Delete { pos: *pos, text: text.clone() },
            EditOp::Delete { pos, text } => EditOp::Insert { pos: *pos, text: text.clone() },
        }).collect();
        self.undo_stack.push(UndoEntry {
            ops: undo_ops,
            cursor_before: entry.cursor_after,
            cursor_after: entry.cursor_before,
        });

        self.last_insert_at = None;
        if Some(self.undo_stack.len()) == self.save_depth {
            self.buffer.clear_dirty();
        }
        self.scroll_to_cursor();
    }

    // ── Save ──────────────────────────────────────────────────────────────────

    fn handle_save(&mut self) -> Result<()> {
        if self.buffer.path.is_none() {
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
                self.save_depth = Some(self.undo_stack.len());
                self.last_insert_at = None; // break current undo group at save boundary
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
                            self.save_depth = Some(self.undo_stack.len());
                            self.last_insert_at = None;
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

    // ── Search ────────────────────────────────────────────────────────────────

    fn start_search(&mut self) {
        self.search = Some(SearchState {
            query: String::new(),
            current_match: None,
            saved_cursor: (self.cursor_row, self.cursor_col),
            saved_scroll: self.scroll_offset,
            saved_col_offset: self.col_offset,
        });
        self.set_message("Search: ".to_string());
    }

    fn handle_search_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                // Cancel: restore original cursor position.
                if let Some(s) = self.search.take() {
                    self.cursor_row = s.saved_cursor.0;
                    self.cursor_col = s.saved_cursor.1;
                    self.scroll_offset = s.saved_scroll;
                    self.col_offset = s.saved_col_offset;
                }
                self.set_message("Search cancelled.".to_string());
            }
            KeyCode::Enter | KeyCode::Char('n') => {
                self.search_next();
            }
            KeyCode::Char('N') => {
                self.search_prev();
            }
            KeyCode::Backspace => {
                if let Some(ref mut s) = self.search {
                    s.query.pop();
                    let q = s.query.clone();
                    let msg = format!("Search: {}", q);
                    self.set_message(msg);
                    // Re-search from beginning with new query.
                    self.search_from_start();
                }
            }
            KeyCode::Char(ch) => {
                if let Some(ref mut s) = self.search {
                    s.query.push(ch);
                    let q = s.query.clone();
                    let msg = format!("Search: {}", q);
                    self.set_message(msg);
                }
                self.search_from_start();
            }
            _ => {}
        }
        Ok(())
    }

    fn search_from_start(&mut self) {
        let query = match &self.search {
            Some(s) => s.query.clone(),
            None => return,
        };
        let from = self
            .search
            .as_ref()
            .and_then(|s| s.current_match)
            .unwrap_or(0);
        if let Some(pos) = self.buffer.find_next(&query, from) {
            self.jump_to_match(pos);
        }
    }

    fn search_next(&mut self) {
        let (query, from) = match &self.search {
            Some(s) => {
                let from = s.current_match.map(|m| m + 1).unwrap_or(0);
                (s.query.clone(), from)
            }
            None => return,
        };
        if let Some(pos) = self.buffer.find_next(&query, from) {
            self.jump_to_match(pos);
        } else {
            self.set_message(format!("Search: {} (no more matches)", query));
        }
    }

    fn search_prev(&mut self) {
        let (query, from) = match &self.search {
            Some(s) => {
                let from = s.current_match.unwrap_or(0);
                (s.query.clone(), from)
            }
            None => return,
        };
        if let Some(pos) = self.buffer.find_prev(&query, from) {
            self.jump_to_match(pos);
        } else {
            self.set_message(format!("Search: {} (no more matches)", query));
        }
    }

    fn jump_to_match(&mut self, pos: usize) {
        if let Some(s) = &mut self.search {
            s.current_match = Some(pos);
        }
        let (row, col) = self.buffer.offset_to_row_col(pos);
        self.cursor_row = row;
        self.cursor_col = col;
        self.desired_col = col;
        self.scroll_to_cursor();
    }

    // ── Cursor movement ────────────────────────────────────────────────────────

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col_to_line();
            self.scroll_to_cursor();
        }
        self.last_insert_at = None;
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.buffer.line_count() {
            self.cursor_row += 1;
            self.clamp_col_to_line();
            self.scroll_to_cursor();
        }
        self.last_insert_at = None;
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.buffer.line_len(self.cursor_row);
        }
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    fn move_right(&mut self) {
        let line_len = self.buffer.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.buffer.line_count() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    fn move_line_start(&mut self) {
        self.cursor_col = 0;
        self.desired_col = 0;
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    fn move_line_end(&mut self) {
        self.cursor_col = self.buffer.line_len(self.cursor_row);
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    fn page_up(&mut self) {
        let rows = self.ui.viewport_rows();
        self.cursor_row = self.cursor_row.saturating_sub(rows);
        self.clamp_col_to_line();
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    fn page_down(&mut self) {
        let rows = self.ui.viewport_rows();
        let last_line = self.buffer.line_count().saturating_sub(1);
        self.cursor_row = (self.cursor_row + rows).min(last_line);
        self.clamp_col_to_line();
        self.last_insert_at = None;
        self.scroll_to_cursor();
    }

    // ── Clamping & scrolling ──────────────────────────────────────────────────

    fn clamp_col_to_line(&mut self) {
        let line_len = self.buffer.line_len(self.cursor_row);
        self.cursor_col = self.desired_col.min(line_len);
    }

    fn scroll_to_cursor(&mut self) {
        // Vertical scroll.
        let rows = self.ui.viewport_rows();
        if self.cursor_row < self.scroll_offset {
            self.scroll_offset = self.cursor_row;
        } else if rows > 0 && self.cursor_row >= self.scroll_offset + rows {
            self.scroll_offset = self.cursor_row - rows + 1;
        }

        // Horizontal scroll.
        let line_count = self.buffer.line_count();
        let text_width = self.ui.text_area_width(line_count);
        if self.cursor_col < self.col_offset {
            self.col_offset = self.cursor_col;
        } else if text_width > 0 && self.cursor_col >= self.col_offset + text_width {
            self.col_offset = self.cursor_col - text_width + 1;
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
                self.should_quit = true;
            } else {
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
