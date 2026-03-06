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

#[cfg(feature = "collab")]
use crate::collab::{CollabEvent, CollabHandle, CollabRole, OpKind};

const TRANSIENT_MESSAGE_DURATION: Duration = Duration::from_secs(5);
const STARTUP_HINT_DELAY: Duration = Duration::from_secs(5);
const OPEN_CONFIRM_DURATION: Duration = Duration::from_secs(3);

// Undo / Redo

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

struct SearchState {
    query: String,
    /// Char offset of the current highlighted match, if any.
    current_match: Option<usize>,
    /// Cursor and scroll position when search was invoked (restored on Esc).
    saved_cursor: (usize, usize),
    saved_scroll: usize,
    saved_col_offset: usize,
}

/// The core editor.
pub struct Editor {
    buffer: Buffer,
    _guard: RawModeGuard,
    ui: Ui,
    config: Config,

    cursor_row: usize,
    cursor_col: usize,
    /// The column the user is "trying" to be at (preserved across vertical moves).
    desired_col: usize,

    /// Index of the first visible line (vertical scroll).
    scroll_offset: usize,
    /// Index of the first visible column (horizontal scroll).
    col_offset: usize,

    quit_count: u8,
    last_quit_at: Option<Instant>,

    transient_message: Option<(String, Instant)>,
    persistent_message: Option<String>,
    started_at: Instant,

    prompt: Option<PromptState>,
    last_open_request_at: Option<Instant>,

    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    /// Time of the last single-char insert (for grouping).
    last_insert_at: Option<Instant>,

    search: Option<SearchState>,

    /// Undo stack depth at the time of the last successful save.
    save_depth: Option<usize>,

    should_quit: bool,

    #[cfg(feature = "collab")]
    collab: Option<CollabHandle>,
    /// Last cursor char-offset sent to peers (avoids redundant cursor msgs).
    #[cfg(feature = "collab")]
    last_sent_cursor: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptKind {
    SaveAs,
}

struct PromptState {
    kind: PromptKind,
    input: String,
}

impl Editor {
    /// Construct a new editor for the given buffer and enter raw mode.
    #[cfg(not(feature = "collab"))]
    pub fn new(buffer: Buffer, config: Config) -> Result<Self> {
        Self::new_inner(buffer, config)
    }

    /// Construct a new editor for the given buffer and enter raw mode.
    #[cfg(feature = "collab")]
    pub fn new(buffer: Buffer, config: Config, collab: Option<CollabHandle>) -> Result<Self> {
        let mut ed = Self::new_inner(buffer, config)?;
        ed.collab = collab;
        Ok(ed)
    }

    fn new_inner(buffer: Buffer, config: Config) -> Result<Self> {
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
            transient_message: None,
            persistent_message: None,
            started_at: Instant::now(),
            prompt: None,
            last_open_request_at: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_insert_at: None,
            search: None,
            save_depth: Some(0),
            should_quit: false,
            #[cfg(feature = "collab")]
            collab: None,
            #[cfg(feature = "collab")]
            last_sent_cursor: None,
        })
    }

    /// Set a startup message (e.g., config warnings). Called before `run`.
    pub fn set_startup_message(&mut self, msg: String) {
        self.set_message(msg);
    }

    /// Run the editor event loop until the user quits.
    pub fn run(&mut self) -> Result<()> {
        self.set_persistent_message("Ctrl+Q quit  Ctrl+O open  Ctrl+S save  Ctrl+F find".to_string());

        loop {
            if let Ok((w, h)) = crate::terminal::size() {
                self.ui.width = w as usize;
                self.ui.height = h as usize;
            }

            // Apply any pending remote collaboration events.
            #[cfg(feature = "collab")]
            self.apply_collab_events();

            // Send cursor position to peers when it changes.
            #[cfg(feature = "collab")]
            {
                let offset = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
                if Some(offset) != self.last_sent_cursor {
                    if let Some(h) = &self.collab {
                        h.send_cursor(offset);
                    }
                    self.last_sent_cursor = Some(offset);
                }
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

            #[cfg(feature = "collab")]
            let collab_status = self.collab.as_ref().map(|h| h.status_str());
            #[cfg(feature = "collab")]
            let peer_cursors = self.collab.as_ref().map(|h| h.peer_cursors()).unwrap_or_default();

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
                    #[cfg(feature = "collab")]
                    collab_status: collab_status.as_deref(),
                    #[cfg(feature = "collab")]
                    peer_cursors: &peer_cursors,
                },
            )?;

            if self.should_quit {
                break;
            }

            // Use poll with a short timeout so we can process collab events
            // even when no keys are pressed.
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key)?;
                }
            }
        }

        Ok(())
    }

    /// Drain and apply all pending collaboration events.
    #[cfg(feature = "collab")]
    fn apply_collab_events(&mut self) {
        loop {
            let ev = match self.collab.as_mut() {
                Some(h) => h.try_recv(),
                None => break,
            };
            match ev {
                Some(CollabEvent::Edit { kind, pos, text, peer: _, rev: _ }) => {
                    // Apply the remote edit to the local buffer (no undo entry).
                    match kind {
                        OpKind::Insert => {
                            self.buffer.raw_insert(pos, &text);
                            // Shift cursor if it's at or past the insertion point.
                            let cursor_offset = self
                                .buffer
                                .char_offset_for(self.cursor_row, self.cursor_col);
                            if cursor_offset >= pos {
                                let len = text.chars().count();
                                let new_offset = cursor_offset + len;
                                let (r, c) = self.buffer.offset_to_row_col(new_offset);
                                self.cursor_row = r;
                                self.cursor_col = c;
                                self.desired_col = c;
                            }
                        }
                        OpKind::Delete => {
                            let len = text.chars().count();
                            let cursor_offset = self
                                .buffer
                                .char_offset_for(self.cursor_row, self.cursor_col);
                            self.buffer.raw_delete(pos, len);
                            // Adjust cursor if affected.
                            if cursor_offset > pos {
                                let new_offset = if cursor_offset >= pos + len {
                                    cursor_offset - len
                                } else {
                                    pos
                                };
                                let (r, c) = self.buffer.offset_to_row_col(new_offset);
                                self.cursor_row = r;
                                self.cursor_col = c;
                                self.desired_col = c;
                            }
                        }
                    }
                    // Clear dirty flag on host (guests don't save).
                    self.scroll_to_cursor();
                }
                Some(CollabEvent::FullSync { content, rev: _ }) => {
                    // Replace entire buffer content.
                    self.buffer = crate::buffer::Buffer::from_content(content);
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                    self.desired_col = 0;
                    self.scroll_offset = 0;
                    self.col_offset = 0;
                    self.undo_stack.clear();
                    self.redo_stack.clear();
                }
                Some(CollabEvent::LocalConfirm { .. })
                | Some(CollabEvent::PeersChanged { .. })
                | Some(CollabEvent::PeerCursor { .. })
                | Some(CollabEvent::ConnectionStatus { .. }) => {
                    // State is updated in the shared CollabState; just re-render.
                }
                None => break,
            }
        }
    }

    fn set_message(&mut self, msg: String) {
        self.transient_message = Some((msg, Instant::now()));
    }

    fn set_persistent_message(&mut self, msg: String) {
        self.persistent_message = Some(msg);
    }

    fn current_message(&self) -> Option<&str> {
        match &self.transient_message {
            Some((text, posted_at)) if posted_at.elapsed() < TRANSIENT_MESSAGE_DURATION => {
                Some(text.as_str())
            }
            _ if self.started_at.elapsed() < STARTUP_HINT_DELAY => None,
            _ => self.persistent_message.as_deref(),
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if self.prompt.is_some() {
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
            Command::Open => self.handle_open()?,
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

    /// Send a local insert op to the collab layer (no-op if not in collab mode).
    #[cfg(feature = "collab")]
    fn collab_send_insert(&self, pos: usize, text: String) {
        if let Some(h) = &self.collab {
            h.send_insert(pos, text);
        }
    }

    /// Send a local delete op to the collab layer (no-op if not in collab mode).
    #[cfg(feature = "collab")]
    fn collab_send_delete(&self, pos: usize, text: String) {
        if let Some(h) = &self.collab {
            h.send_delete(pos, text);
        }
    }

    fn handle_insert_char(&mut self, ch: char) {
        let pos = self.buffer.char_offset_for(self.cursor_row, self.cursor_col);
        self.buffer.insert_char(self.cursor_row, self.cursor_col, ch);
        self.cursor_col += 1;
        self.desired_col = self.cursor_col;

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
        #[cfg(feature = "collab")]
        self.collab_send_insert(pos, ch.to_string());
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
        #[cfg(feature = "collab")]
        self.collab_send_delete(del_pos, ch.to_string());
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
        #[cfg(feature = "collab")]
        self.collab_send_delete(pos, ch.to_string());
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
        #[cfg(feature = "collab")]
        self.collab_send_insert(pos, "\n".to_string());
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
            vec![EditOp::Insert { pos, text: spaces.clone() }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        #[cfg(feature = "collab")]
        self.collab_send_insert(pos, spaces);
        self.scroll_to_cursor();
    }

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

    fn handle_save(&mut self) -> Result<()> {
        #[cfg(feature = "collab")]
        if let Some(h) = &self.collab {
            if matches!(h.role, CollabRole::Guest { .. }) {
                self.set_message("Only the host can save.".to_string());
                return Ok(());
            }
        }
        if self.buffer.path.is_none() {
            self.start_save_prompt();
        } else {
            self.do_save();
        }
        Ok(())
    }

    fn start_save_prompt(&mut self) {
        self.prompt = Some(PromptState {
            kind: PromptKind::SaveAs,
            input: String::new(),
        });
        self.set_prompt_message();
    }

    fn do_save(&mut self) {
        match self.buffer.save() {
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

    fn handle_open(&mut self) -> Result<()> {
        #[cfg(feature = "collab")]
        {
            if self.collab.is_some() {
                self.set_message("Open is unavailable during collaboration.".to_string());
                return Ok(());
            }
        }

        if self.buffer.is_dirty() {
            let now = Instant::now();
            let within_window = self
                .last_open_request_at
                .map(|t| now.duration_since(t) < OPEN_CONFIRM_DURATION)
                .unwrap_or(false);

            if !within_window {
                self.last_open_request_at = Some(now);
                self.set_message(
                    "WARNING: File has unsaved changes. Press Ctrl+O again to choose a file."
                        .to_string(),
                );
                return Ok(());
            }
        }

        self.last_open_request_at = None;
        self.pick_and_open_file()?;
        Ok(())
    }

    fn pick_and_open_file(&mut self) -> Result<()> {
        let initial_dir = self
            .buffer
            .path
            .as_ref()
            .and_then(|path| path.parent());

        let selected = crate::terminal::suspend(|| crate::file_picker::pick_open_file(initial_dir))?;

        match selected {
            Some(path) if path.is_file() => self.open_path(path),
            Some(_) => self.set_message("Open error: Selected path is not a file.".to_string()),
            None => self.set_message("Open cancelled.".to_string()),
        }

        Ok(())
    }

    fn open_path(&mut self, path: PathBuf) {
        match Buffer::open(path) {
            Ok(buffer) => {
                let name = buffer.display_name();
                self.replace_buffer(buffer);
                self.set_message(format!("Opened {}", name));
            }
            Err(e) => {
                self.set_message(format!("Open error: {:#}", e));
            }
        }
    }

    fn replace_buffer(&mut self, buffer: Buffer) {
        self.buffer = buffer;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.desired_col = 0;
        self.scroll_offset = 0;
        self.col_offset = 0;
        self.search = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.save_depth = Some(0);
        self.last_insert_at = None;
        self.last_open_request_at = None;
        self.quit_count = 0;
        self.last_quit_at = None;
        #[cfg(feature = "collab")]
        {
            self.last_sent_cursor = None;
        }
        self.scroll_to_cursor();
    }

    fn handle_prompt_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let (kind, input) = match self.prompt.as_ref() {
                    Some(prompt) => (prompt.kind, prompt.input.clone()),
                    None => return Ok(()),
                };

                match kind {
                    PromptKind::SaveAs => {
                        self.prompt = None;
                        if input.is_empty() {
                            self.set_message("Save cancelled.".to_string());
                        } else {
                            match self.buffer.save_to(PathBuf::from(input)) {
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
                }
            }
            KeyCode::Esc => {
                let cancel_message = match self.prompt.as_ref().map(|prompt| prompt.kind) {
                    Some(PromptKind::SaveAs) => "Save cancelled.".to_string(),
                    None => return Ok(()),
                };
                self.prompt = None;
                self.set_message(cancel_message);
            }
            KeyCode::Backspace => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.input.pop();
                }
                self.set_prompt_message();
            }
            KeyCode::Char(ch) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.input.push(ch);
                }
                self.set_prompt_message();
            }
            _ => {}
        }
        Ok(())
    }

    fn set_prompt_message(&mut self) {
        let message = match self.prompt.as_ref() {
            Some(prompt) => match prompt.kind {
                PromptKind::SaveAs => format!("Save as: {}", prompt.input),
            },
            None => return,
        };
        self.set_message(message);
    }

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

    fn handle_quit(&mut self) -> Result<()> {
        #[cfg(feature = "collab")]
        {
            if let Some(h) = &self.collab {
                // Guests disconnect and exit immediately — no unsaved-changes guard.
                if matches!(h.role, CollabRole::Guest { .. }) {
                    self.should_quit = true;
                    return Ok(());
                }
                if matches!(h.role, CollabRole::Host { .. }) && h.peer_count() > 0 {
                    let now = Instant::now();
                    let within_window = self
                        .last_quit_at
                        .map(|t| now.duration_since(t) < Duration::from_secs(3))
                        .unwrap_or(false);
                    if within_window && self.quit_count >= 1 {
                        self.should_quit = true;
                        return Ok(());
                    }
                    self.quit_count = 1;
                    self.last_quit_at = Some(now);
                    self.set_message(
                        "Disconnect all peers and quit? Press Ctrl+Q again to confirm.".to_string(),
                    );
                    return Ok(());
                }
            }
        }

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
