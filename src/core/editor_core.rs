use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::buffer::Buffer;
use crate::config::Config;
use crate::keymap::Command;

const TRANSIENT_MESSAGE_DURATION: Duration = Duration::from_secs(5);
const STARTUP_HINT_DELAY: Duration = Duration::from_secs(5);
const OPEN_CONFIRM_DURATION: Duration = Duration::from_secs(3);
const QUIT_CONFIRM_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
enum EditOp {
    Insert { pos: usize, text: String },
    Delete { pos: usize, text: String },
}

#[derive(Debug, Clone)]
struct UndoEntry {
    ops: Vec<EditOp>,
    cursor_before: (usize, usize),
    cursor_after: (usize, usize),
}

struct SearchState {
    query: String,
    current_match: Option<usize>,
    saved_cursor: (usize, usize),
    saved_scroll: usize,
    saved_col_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchStateSnapshot {
    pub query: String,
    pub current_match: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewSnapshot {
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub col_offset: usize,
    pub message: Option<String>,
    pub search_match: Option<(usize, usize)>,
    pub file_ext: Option<String>,
    pub show_line_numbers: bool,
    pub theme: String,
    pub should_quit: bool,
    pub search: Option<SearchStateSnapshot>,
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
    pub soft_wrap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportMetrics {
    pub rows: usize,
    pub cols: usize,
}

impl ViewportMetrics {
    fn gutter_width(self, show_line_numbers: bool, line_count: usize) -> usize {
        if !show_line_numbers {
            return 0;
        }
        line_count.to_string().len() + 1
    }

    fn text_width(self, show_line_numbers: bool, line_count: usize) -> usize {
        self.cols
            .saturating_sub(self.gutter_width(show_line_numbers, line_count))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendRequest {
    OpenFilePicker { initial_dir: Option<PathBuf> },
    SaveFilePicker { suggested_path: Option<PathBuf> },
}

pub struct EditorCore {
    buffer: Buffer,
    tab_width: usize,
    show_line_numbers: bool,
    theme: String,
    cursor_row: usize,
    cursor_col: usize,
    desired_col: usize,
    scroll_offset: usize,
    col_offset: usize,
    quit_count: u8,
    last_quit_at: Option<Instant>,
    last_open_request_at: Option<Instant>,
    transient_message: Option<(String, Instant)>,
    persistent_message: Option<String>,
    started_at: Instant,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    last_insert_at: Option<Instant>,
    search: Option<SearchState>,
    save_depth: Option<usize>,
    should_quit: bool,
    // Selection state for copy/cut operations
    selection_start: Option<(usize, usize)>,
    selection_end: Option<(usize, usize)>,
    pub soft_wrap: bool,
    wrap_column: usize,
}

impl EditorCore {
    pub fn new(buffer: Buffer, config: Config) -> Self {
        Self {
            buffer,
            tab_width: config.tab_width,
            show_line_numbers: config.line_numbers,
            theme: config.theme,
            cursor_row: 0,
            cursor_col: 0,
            desired_col: 0,
            scroll_offset: 0,
            col_offset: 0,
            quit_count: 0,
            last_quit_at: None,
            last_open_request_at: None,
            transient_message: None,
            persistent_message: None,
            started_at: Instant::now(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_insert_at: None,
            search: None,
            save_depth: Some(0),
            should_quit: false,
            selection_start: None,
            selection_end: None,
            soft_wrap: config.word_wrap,
            wrap_column: config.wrap_column,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn show_line_numbers(&self) -> bool {
        self.show_line_numbers
    }

    pub fn tab_width(&self) -> usize {
        self.tab_width
    }

    pub fn theme(&self) -> &str {
        &self.theme
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn snapshot(&self) -> ViewSnapshot {
        let file_ext = self
            .buffer
            .path
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .map(str::to_string);

        let search_match = self.search.as_ref().and_then(|state| {
            state
                .current_match
                .map(|offset| (offset, state.query.chars().count()))
        });

        ViewSnapshot {
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            scroll_offset: self.scroll_offset,
            col_offset: self.col_offset,
            message: self.current_message().map(str::to_string),
            search_match,
            file_ext,
            show_line_numbers: self.show_line_numbers,
            theme: self.theme.clone(),
            should_quit: self.should_quit,
            search: self.search.as_ref().map(|state| SearchStateSnapshot {
                query: state.query.clone(),
                current_match: state.current_match,
            }),
            selection_start: self.selection_start,
            selection_end: self.selection_end,
            soft_wrap: self.soft_wrap,
        }
    }

    pub fn set_startup_message(&mut self, msg: String) {
        self.set_message(msg);
    }

    pub fn set_message(&mut self, msg: String) {
        self.transient_message = Some((msg, Instant::now()));
    }

    pub fn set_persistent_message(&mut self, msg: String) {
        self.persistent_message = Some(msg);
    }

    // Selection methods
    pub fn has_selection(&self) -> bool {
        self.selection_text_range().is_some()
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
    }

    pub fn set_selection_start(&mut self, row: usize, col: usize) {
        self.selection_start = Some((row, col));
        self.selection_end = Some((row, col));
    }

    pub fn set_selection_end(&mut self, row: usize, col: usize) {
        self.selection_end = Some((row, col));
    }

    /// Get the selected text, if any. Returns (start_pos, end_pos, text)
    pub fn get_selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        Some((start, end))
    }

    /// Normalize selection so start <= end
    fn normalize_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let (start, end) = self.get_selection_range()?;
        // Compare by row first, then by column
        if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let (start_offset, end_offset) = self.selection_text_range()?;
        let text: String = self.buffer.rope.slice(start_offset..end_offset).into();
        Some(text)
    }

    fn selection_text_range(&self) -> Option<(usize, usize)> {
        let (start, end) = self.normalize_selection()?;
        let start_offset = self.buffer.char_offset_for(start.0, start.1);
        let end_offset = self.buffer.char_offset_for(end.0, end.1);
        if start_offset < end_offset {
            Some((start_offset, end_offset))
        } else {
            None
        }
    }

    fn selected_text_and_bounds(&self) -> Option<(usize, usize, String, (usize, usize))> {
        let (start, _end) = self.normalize_selection()?;
        let (start_offset, end_offset) = self.selection_text_range()?;
        let text: String = self.buffer.rope.slice(start_offset..end_offset).into();
        Some((start_offset, end_offset, text, start))
    }

    /// Set cursor position directly (for mouse clicks)
    pub fn set_cursor_position(&mut self, row: usize, col: usize, viewport: ViewportMetrics) {
        let max_row = self.buffer.line_count().saturating_sub(1);
        self.cursor_row = row.min(max_row);
        let line_len = self.buffer.line_len(self.cursor_row);
        self.cursor_col = col.min(line_len);
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    /// Set scroll offset directly (for mouse wheel scrolling)
    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    pub fn apply_command(
        &mut self,
        command: Command,
        viewport: ViewportMetrics,
    ) -> Result<Option<FrontendRequest>> {
        let request = match command {
            Command::MoveUp => {
                self.move_up(viewport);
                None
            }
            Command::MoveDown => {
                self.move_down(viewport);
                None
            }
            Command::MoveLeft => {
                self.move_left(viewport);
                None
            }
            Command::MoveRight => {
                self.move_right(viewport);
                None
            }
            Command::MoveLineStart => {
                self.move_line_start(viewport);
                None
            }
            Command::MoveLineEnd => {
                self.move_line_end(viewport);
                None
            }
            Command::PageUp => {
                self.page_up(viewport);
                None
            }
            Command::PageDown => {
                self.page_down(viewport);
                None
            }
            Command::Quit => {
                self.handle_quit();
                None
            }
            Command::InsertChar(ch) => {
                self.handle_insert_char(ch, viewport);
                None
            }
            Command::Backspace => {
                self.handle_backspace(viewport);
                None
            }
            Command::DeleteChar => {
                self.handle_delete(viewport);
                None
            }
            Command::InsertNewline => {
                self.handle_newline(viewport);
                None
            }
            Command::InsertTab => {
                self.handle_tab(viewport);
                None
            }
            Command::Save => self.handle_save()?,
            Command::SaveAs => Some(FrontendRequest::SaveFilePicker {
                suggested_path: self.buffer.path.clone(),
            }),
            Command::Open => self.request_open(),
            Command::Undo => {
                self.handle_undo(viewport);
                None
            }
            Command::Redo => {
                self.handle_redo(viewport);
                None
            }
            Command::Find => {
                self.start_search();
                None
            }
            Command::ToggleLineNumbers => {
                self.show_line_numbers = !self.show_line_numbers;
                self.scroll_to_cursor(viewport);
                None
            }
            Command::Copy => {
                // Selection text is retrieved by GUI for clipboard
                None
            }
            Command::Paste => {
                // Text is inserted by GUI after retrieving from clipboard
                None
            }
            Command::Cut => {
                // Selection is cut by GUI (get text, then delete)
                None
            }
            Command::SelectAll => {
                self.handle_select_all();
                None
            }
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
                self.soft_wrap = !self.soft_wrap;
                // Reset horizontal scroll when disabling wrap
                if !self.soft_wrap {
                    self.col_offset = 0;
                }
                self.scroll_to_cursor(viewport);
                None
            }
            Command::ReflowParagraph => {
                // Implemented in Task 7
                None
            }
            Command::None => None,
        };

        Ok(request)
    }

    pub fn open_path(&mut self, path: PathBuf, viewport: ViewportMetrics) {
        match Buffer::open(path) {
            Ok(buffer) => {
                let name = buffer.display_name();
                self.replace_buffer(buffer, viewport);
                self.set_message(format!("Opened {}", name));
            }
            Err(err) => self.set_message(format!("Open error: {:#}", err)),
        }
    }

    pub fn replace_with_content(&mut self, content: String, viewport: ViewportMetrics) {
        self.replace_buffer(Buffer::from_content(content), viewport);
    }

    pub fn save_to_path(&mut self, path: PathBuf) {
        match self.buffer.save_to(path) {
            Ok(bytes) => {
                let name = self.buffer.display_name();
                self.set_message(format!("{} written - {} bytes", name, bytes));
                self.save_depth = Some(self.undo_stack.len());
                self.last_insert_at = None;
            }
            Err(err) => self.set_message(format!("Save error: {:#}", err)),
        }
    }

    pub fn start_search(&mut self) {
        self.search = Some(SearchState {
            query: String::new(),
            current_match: None,
            saved_cursor: (self.cursor_row, self.cursor_col),
            saved_scroll: self.scroll_offset,
            saved_col_offset: self.col_offset,
        });
        self.set_message("Search: ".to_string());
    }

    pub fn is_search_active(&self) -> bool {
        self.search.is_some()
    }

    pub fn set_search_query(&mut self, query: String, viewport: ViewportMetrics) {
        if let Some(search) = self.search.as_mut() {
            search.query = query.clone();
        } else {
            self.start_search();
            if let Some(search) = self.search.as_mut() {
                search.query = query.clone();
            }
        }
        self.set_message(format!("Search: {}", query));
        self.search_from_start(viewport);
    }

    pub fn append_search_char(&mut self, ch: char, viewport: ViewportMetrics) {
        let mut query = self
            .search
            .as_ref()
            .map(|search| search.query.clone())
            .unwrap_or_default();
        query.push(ch);
        self.set_search_query(query, viewport);
    }

    pub fn pop_search_char(&mut self, viewport: ViewportMetrics) {
        if let Some(search) = self.search.as_ref() {
            let mut query = search.query.clone();
            query.pop();
            self.set_search_query(query, viewport);
        }
    }

    pub fn search_next(&mut self, viewport: ViewportMetrics) {
        let (query, from) = match &self.search {
            Some(search) => (
                search.query.clone(),
                search.current_match.map(|offset| offset + 1).unwrap_or(0),
            ),
            None => return,
        };

        if let Some(pos) = self.buffer.find_next(&query, from) {
            self.jump_to_match(pos, viewport);
        } else {
            self.set_message(format!("Search: {} (no more matches)", query));
        }
    }

    pub fn search_prev(&mut self, viewport: ViewportMetrics) {
        let (query, from) = match &self.search {
            Some(search) => (search.query.clone(), search.current_match.unwrap_or(0)),
            None => return,
        };

        if let Some(pos) = self.buffer.find_prev(&query, from) {
            self.jump_to_match(pos, viewport);
        } else {
            self.set_message(format!("Search: {} (no more matches)", query));
        }
    }

    pub fn cancel_search(&mut self) {
        if let Some(search) = self.search.take() {
            self.cursor_row = search.saved_cursor.0;
            self.cursor_col = search.saved_cursor.1;
            self.desired_col = self.cursor_col;
            self.scroll_offset = search.saved_scroll;
            self.col_offset = search.saved_col_offset;
        }
        self.set_message("Search cancelled.".to_string());
    }

    pub fn apply_remote_insert(&mut self, pos: usize, text: &str, viewport: ViewportMetrics) {
        let cursor_offset = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        self.buffer.raw_insert(pos, text);
        if cursor_offset >= pos {
            let new_offset = cursor_offset + text.chars().count();
            let (row, col) = self.buffer.offset_to_row_col(new_offset);
            self.cursor_row = row;
            self.cursor_col = col;
            self.desired_col = col;
        }
        self.scroll_to_cursor(viewport);
    }

    pub fn apply_remote_delete(&mut self, pos: usize, text: &str, viewport: ViewportMetrics) {
        let len = text.chars().count();
        let cursor_offset = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        self.buffer.raw_delete(pos, len);
        if cursor_offset > pos {
            let new_offset = if cursor_offset >= pos + len {
                cursor_offset - len
            } else {
                pos
            };
            let (row, col) = self.buffer.offset_to_row_col(new_offset);
            self.cursor_row = row;
            self.cursor_col = col;
            self.desired_col = col;
        }
        self.scroll_to_cursor(viewport);
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

    fn request_open(&mut self) -> Option<FrontendRequest> {
        if self.buffer.is_dirty() {
            let now = Instant::now();
            let within_window = self
                .last_open_request_at
                .map(|instant| now.duration_since(instant) < OPEN_CONFIRM_DURATION)
                .unwrap_or(false);

            if !within_window {
                self.last_open_request_at = Some(now);
                self.set_message(
                    "WARNING: File has unsaved changes. Press Ctrl+O again to choose a file."
                        .to_string(),
                );
                return None;
            }
        }

        self.last_open_request_at = None;
        Some(FrontendRequest::OpenFilePicker {
            initial_dir: self.current_directory().map(Path::to_path_buf),
        })
    }

    fn handle_save(&mut self) -> Result<Option<FrontendRequest>> {
        if self.buffer.path.is_none() {
            return Ok(Some(FrontendRequest::SaveFilePicker {
                suggested_path: None,
            }));
        }

        match self.buffer.save() {
            Ok(bytes) => {
                let name = self.buffer.display_name();
                self.set_message(format!("{} written - {} bytes", name, bytes));
                self.save_depth = Some(self.undo_stack.len());
                self.last_insert_at = None;
            }
            Err(err) => {
                self.set_message(format!("Save error: {:#}", err));
            }
        }

        Ok(None)
    }

    fn current_directory(&self) -> Option<&Path> {
        self.buffer.path.as_deref().and_then(Path::parent)
    }

    fn handle_insert_char(&mut self, ch: char, viewport: ViewportMetrics) {
        if self.has_selection() {
            self.replace_selection_with_text(&ch.to_string(), viewport);
            return;
        }

        let pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        self.buffer
            .insert_char(self.cursor_row, self.cursor_col, ch);
        self.cursor_col += 1;
        self.desired_col = self.cursor_col;

        let within_window = self
            .last_insert_at
            .map(|instant| instant.elapsed() < Duration::from_secs(1))
            .unwrap_or(false);
        let merged = if within_window {
            if let Some(entry) = self.undo_stack.last_mut() {
                if let Some(EditOp::Insert {
                    pos: entry_pos,
                    text,
                }) = entry.ops.last_mut()
                {
                    if *entry_pos + text.chars().count() == pos {
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
                vec![EditOp::Insert {
                    pos,
                    text: ch.to_string(),
                }],
                (self.cursor_row, self.cursor_col - 1),
                (self.cursor_row, self.cursor_col),
            );
        }

        self.last_insert_at = Some(Instant::now());
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }

    fn handle_backspace(&mut self, viewport: ViewportMetrics) {
        if self.has_selection() {
            self.delete_selection_with_undo(viewport);
            return;
        }

        let pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        if pos == 0 {
            return;
        }

        let delete_pos = pos - 1;
        let ch = match self.buffer.char_at_offset(delete_pos) {
            Some(ch) => ch,
            None => return,
        };
        let cursor_before = (self.cursor_row, self.cursor_col);
        let (new_row, new_col) = self
            .buffer
            .delete_char_before(self.cursor_row, self.cursor_col);
        self.cursor_row = new_row;
        self.cursor_col = new_col;
        self.desired_col = new_col;

        self.push_undo(
            vec![EditOp::Delete {
                pos: delete_pos,
                text: ch.to_string(),
            }],
            cursor_before,
            (new_row, new_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }

    fn handle_delete(&mut self, viewport: ViewportMetrics) {
        if self.has_selection() {
            self.delete_selection_with_undo(viewport);
            return;
        }

        let pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        let ch = match self.buffer.char_at_offset(pos) {
            Some(ch) => ch,
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
            vec![EditOp::Delete {
                pos,
                text: ch.to_string(),
            }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }

    fn handle_newline(&mut self, viewport: ViewportMetrics) {
        if self.has_selection() {
            self.replace_selection_with_text("\n", viewport);
            return;
        }

        let pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        let cursor_before = (self.cursor_row, self.cursor_col);
        self.buffer.insert_newline(self.cursor_row, self.cursor_col);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.desired_col = 0;

        self.push_undo(
            vec![EditOp::Insert {
                pos,
                text: "\n".to_string(),
            }],
            cursor_before,
            (self.cursor_row, self.cursor_col),
        );
        self.last_insert_at = None;
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }

    fn handle_tab(&mut self, viewport: ViewportMetrics) {
        if self.has_selection() {
            let spaces = " ".repeat(self.tab_width);
            self.replace_selection_with_text(&spaces, viewport);
            return;
        }

        let spaces = " ".repeat(self.tab_width);
        let pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        let cursor_before = (self.cursor_row, self.cursor_col);
        self.buffer
            .insert_str(self.cursor_row, self.cursor_col, &spaces);
        self.cursor_col += self.tab_width;
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
        self.scroll_to_cursor(viewport);
    }

    fn push_undo(
        &mut self,
        ops: Vec<EditOp>,
        cursor_before: (usize, usize),
        cursor_after: (usize, usize),
    ) {
        self.undo_stack.push(UndoEntry {
            ops,
            cursor_before,
            cursor_after,
        });
    }

    fn handle_undo(&mut self, viewport: ViewportMetrics) {
        let entry = match self.undo_stack.pop() {
            Some(entry) => entry,
            None => {
                self.set_message("Nothing to undo.".to_string());
                return;
            }
        };

        for op in entry.ops.iter().rev() {
            match op {
                EditOp::Insert { pos, text } => {
                    self.buffer.raw_delete(*pos, text.chars().count());
                }
                EditOp::Delete { pos, text } => self.buffer.raw_insert(*pos, text),
            }
        }

        let (row, col) = entry.cursor_before;
        self.cursor_row = row.min(self.buffer.line_count().saturating_sub(1));
        self.cursor_col = col.min(self.buffer.line_len(self.cursor_row));
        self.desired_col = self.cursor_col;

        self.redo_stack.push(entry);

        self.last_insert_at = None;
        if Some(self.undo_stack.len()) == self.save_depth {
            self.buffer.clear_dirty();
        }
        self.scroll_to_cursor(viewport);
    }

    fn handle_redo(&mut self, viewport: ViewportMetrics) {
        let entry = match self.redo_stack.pop() {
            Some(entry) => entry,
            None => {
                self.set_message("Nothing to redo.".to_string());
                return;
            }
        };

        for op in &entry.ops {
            match op {
                EditOp::Insert { pos, text } => self.buffer.raw_insert(*pos, text),
                EditOp::Delete { pos, text } => {
                    self.buffer.raw_delete(*pos, text.chars().count());
                }
            }
        }

        let (row, col) = entry.cursor_after;
        self.cursor_row = row.min(self.buffer.line_count().saturating_sub(1));
        self.cursor_col = col.min(self.buffer.line_len(self.cursor_row));
        self.desired_col = self.cursor_col;

        self.undo_stack.push(entry);

        self.last_insert_at = None;
        if Some(self.undo_stack.len()) == self.save_depth {
            self.buffer.clear_dirty();
        }
        self.scroll_to_cursor(viewport);
    }

    fn search_from_start(&mut self, viewport: ViewportMetrics) {
        let query = match &self.search {
            Some(search) => search.query.clone(),
            None => return,
        };

        let from = self
            .search
            .as_ref()
            .and_then(|search| search.current_match)
            .unwrap_or(0);
        if let Some(pos) = self.buffer.find_next(&query, from) {
            self.jump_to_match(pos, viewport);
        }
    }

    fn jump_to_match(&mut self, pos: usize, viewport: ViewportMetrics) {
        if let Some(search) = &mut self.search {
            search.current_match = Some(pos);
        }
        let (row, col) = self.buffer.offset_to_row_col(pos);
        self.cursor_row = row;
        self.cursor_col = col;
        self.desired_col = col;
        self.scroll_to_cursor(viewport);
    }

    fn replace_buffer(&mut self, buffer: Buffer, viewport: ViewportMetrics) {
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
        self.scroll_to_cursor(viewport);
    }

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
                    let visual_col = self.desired_col % wrap_width;
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
                    let visual_col = self.desired_col % wrap_width;
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

    fn move_left(&mut self, viewport: ViewportMetrics) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.buffer.line_len(self.cursor_row);
        }
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn move_right(&mut self, viewport: ViewportMetrics) {
        let line_len = self.buffer.line_len(self.cursor_row);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.buffer.line_count() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn move_line_start(&mut self, viewport: ViewportMetrics) {
        self.cursor_col = 0;
        self.desired_col = 0;
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn move_line_end(&mut self, viewport: ViewportMetrics) {
        self.cursor_col = self.buffer.line_len(self.cursor_row);
        self.desired_col = self.cursor_col;
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn page_up(&mut self, viewport: ViewportMetrics) {
        self.cursor_row = self.cursor_row.saturating_sub(viewport.rows);
        self.clamp_col_to_line();
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn page_down(&mut self, viewport: ViewportMetrics) {
        let last_line = self.buffer.line_count().saturating_sub(1);
        self.cursor_row = (self.cursor_row + viewport.rows).min(last_line);
        self.clamp_col_to_line();
        self.last_insert_at = None;
        self.scroll_to_cursor(viewport);
    }

    fn clamp_col_to_line(&mut self) {
        let line_len = self.buffer.line_len(self.cursor_row);
        self.cursor_col = self.desired_col.min(line_len);
    }

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

    fn handle_select_all(&mut self) {
        let last_row = self.buffer.line_count().saturating_sub(1);
        let last_col = self.buffer.line_len(last_row);
        self.selection_start = Some((0, 0));
        self.selection_end = Some((last_row, last_col));
    }

    /// Insert text at cursor position (for paste operations)
    pub fn insert_text_at_cursor(&mut self, text: &str, viewport: ViewportMetrics) {
        if text.is_empty() {
            return;
        }

        if self.has_selection() {
            self.replace_selection_with_text(text, viewport);
        } else {
            self.insert_text_no_selection(text, viewport);
        }
    }

    /// Delete the current selection and return the deleted text
    pub fn delete_selection(&mut self) -> Option<String> {
        let (start_offset, end_offset, text, start) = self.selected_text_and_bounds()?;
        self.buffer
            .raw_delete(start_offset, end_offset - start_offset);
        self.cursor_row = start.0;
        self.cursor_col = start.1;
        self.desired_col = start.1;
        self.clear_selection();
        Some(text)
    }

    fn insert_text_no_selection(&mut self, text: &str, viewport: ViewportMetrics) {
        let start_pos = self
            .buffer
            .char_offset_for(self.cursor_row, self.cursor_col);
        let start_row = self.cursor_row;
        let start_col = self.cursor_col;
        let mut row = self.cursor_row;
        let mut col = self.cursor_col;

        for ch in text.chars() {
            if ch == '\n' {
                self.buffer.insert_newline(row, col);
                row += 1;
                col = 0;
            } else {
                self.buffer.insert_char(row, col, ch);
                col += 1;
            }
        }

        self.cursor_row = row;
        self.cursor_col = col;
        self.desired_col = col;

        self.push_undo(
            vec![EditOp::Insert {
                pos: start_pos,
                text: text.to_string(),
            }],
            (start_row, start_col),
            (self.cursor_row, self.cursor_col),
        );
        self.finish_edit(viewport, true);
    }

    fn replace_selection_with_text(&mut self, text: &str, viewport: ViewportMetrics) {
        let (start_offset, end_offset, deleted_text, start) = match self.selected_text_and_bounds()
        {
            Some(data) => data,
            None => {
                self.insert_text_no_selection(text, viewport);
                return;
            }
        };

        self.buffer
            .raw_delete(start_offset, end_offset - start_offset);
        self.cursor_row = start.0;
        self.cursor_col = start.1;
        self.desired_col = start.1;

        let mut ops = vec![EditOp::Delete {
            pos: start_offset,
            text: deleted_text,
        }];

        if !text.is_empty() {
            self.buffer.raw_insert(start_offset, text);
            let inserted_len = text.chars().count();
            let (new_row, new_col) = self.buffer.offset_to_row_col(start_offset + inserted_len);
            self.cursor_row = new_row;
            self.cursor_col = new_col;
            self.desired_col = new_col;
            ops.push(EditOp::Insert {
                pos: start_offset,
                text: text.to_string(),
            });
        }

        self.clear_selection();
        self.push_undo(
            ops,
            start,
            if text.is_empty() {
                start
            } else {
                (self.cursor_row, self.cursor_col)
            },
        );
        self.finish_edit(viewport, !text.is_empty());
    }

    fn delete_selection_with_undo(&mut self, viewport: ViewportMetrics) {
        let (start_offset, _end_offset, deleted_text, start) = match self.selected_text_and_bounds()
        {
            Some(data) => data,
            None => return,
        };

        self.delete_selection();
        self.push_undo(
            vec![EditOp::Delete {
                pos: start_offset,
                text: deleted_text,
            }],
            start,
            start,
        );
        self.finish_edit(viewport, false);
    }

    pub fn cut_selection(&mut self, viewport: ViewportMetrics) -> Option<String> {
        let text = self.get_selected_text()?;
        self.delete_selection_with_undo(viewport);
        Some(text)
    }

    fn finish_edit(&mut self, viewport: ViewportMetrics, merge_next_insert: bool) {
        self.last_insert_at = if merge_next_insert {
            Some(Instant::now())
        } else {
            None
        };
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }

    fn handle_quit(&mut self) {
        if self.buffer.is_dirty() {
            let now = Instant::now();
            let within_window = self
                .last_quit_at
                .map(|instant| now.duration_since(instant) < QUIT_CONFIRM_DURATION)
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
    }

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
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
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
        if !self.redo_stack.is_empty() {
            self.save_depth = None;
        }
        self.redo_stack.clear();
        self.scroll_to_cursor(viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn viewport() -> ViewportMetrics {
        ViewportMetrics { rows: 4, cols: 12 }
    }

    fn config() -> Config {
        Config {
            tab_width: 4,
            line_numbers: true,
            theme: "dark".to_string(),
            word_wrap: true,
            wrap_column: 80,
        }
    }

    #[test]
    fn commands_edit_buffer_and_undo_redo() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());

        core.apply_command(Command::InsertChar('a'), viewport())
            .unwrap();
        core.apply_command(Command::InsertChar('b'), viewport())
            .unwrap();
        core.apply_command(Command::InsertNewline, viewport())
            .unwrap();
        core.apply_command(Command::InsertChar('c'), viewport())
            .unwrap();

        assert_eq!(core.buffer().rope.to_string(), "ab\nc");

        core.apply_command(Command::Undo, viewport()).unwrap();
        assert_eq!(core.buffer().rope.to_string(), "ab\n");

        core.apply_command(Command::Redo, viewport()).unwrap();
        assert_eq!(core.buffer().rope.to_string(), "ab\nc");
    }

    #[test]
    fn search_wraps_and_cancel_restores_position() {
        let mut core = EditorCore::new(
            Buffer::from_content("alpha beta alpha".to_string()),
            config(),
        );
        core.apply_command(Command::MoveRight, viewport()).unwrap();
        core.apply_command(Command::MoveRight, viewport()).unwrap();

        core.start_search();
        core.set_search_query("alpha".to_string(), viewport());
        assert_eq!(core.snapshot().search_match, Some((0, 5)));

        core.search_next(viewport());
        assert_eq!(core.snapshot().search_match, Some((11, 5)));

        core.cancel_search();
        let snapshot = core.snapshot();
        assert_eq!((snapshot.cursor_row, snapshot.cursor_col), (0, 2));
    }

    #[test]
    fn backspace_and_delete_commands_update_content() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());

        core.apply_command(Command::InsertChar('a'), viewport())
            .unwrap();
        core.apply_command(Command::InsertChar('b'), viewport())
            .unwrap();
        core.apply_command(Command::InsertNewline, viewport())
            .unwrap();
        core.apply_command(Command::InsertChar('c'), viewport())
            .unwrap();
        core.apply_command(Command::InsertChar('d'), viewport())
            .unwrap();
        assert_eq!(core.buffer().rope.to_string(), "ab\ncd");

        core.apply_command(Command::Backspace, viewport()).unwrap();
        assert_eq!(core.buffer().rope.to_string(), "ab\nc");

        core.apply_command(Command::MoveLineStart, viewport())
            .unwrap();
        core.apply_command(Command::Backspace, viewport()).unwrap();
        assert_eq!(core.buffer().rope.to_string(), "abc");

        core.apply_command(Command::DeleteChar, viewport()).unwrap();
        assert_eq!(core.buffer().rope.to_string(), "ab");
    }

    #[test]
    fn search_prev_moves_backward_and_wraps() {
        let mut core = EditorCore::new(
            Buffer::from_content("one two one two".to_string()),
            config(),
        );

        core.start_search();
        core.set_search_query("two".to_string(), viewport());
        assert_eq!(core.snapshot().search_match, Some((4, 3)));

        core.search_next(viewport());
        assert_eq!(core.snapshot().search_match, Some((12, 3)));

        core.search_prev(viewport());
        assert_eq!(core.snapshot().search_match, Some((4, 3)));

        core.search_prev(viewport());
        assert_eq!(core.snapshot().search_match, Some((12, 3)));
    }

    #[test]
    fn open_requires_confirmation_when_dirty() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());
        core.apply_command(Command::InsertChar('x'), viewport())
            .unwrap();

        let first = core.apply_command(Command::Open, viewport()).unwrap();
        let second = core.apply_command(Command::Open, viewport()).unwrap();

        assert!(first.is_none());
        assert!(matches!(
            second,
            Some(FrontendRequest::OpenFilePicker { .. })
        ));
    }

    #[test]
    fn horizontal_scroll_tracks_cursor() {
        // Soft wrap must be off so horizontal scrolling kicks in.
        let mut cfg = config();
        cfg.word_wrap = false;
        let mut core = EditorCore::new(Buffer::new_empty(), cfg);
        for ch in "abcdefghijk".chars() {
            core.apply_command(Command::InsertChar(ch), viewport())
                .unwrap();
        }

        let snapshot = core.snapshot();
        assert!(snapshot.col_offset > 0);
    }

    #[test]
    fn save_command_requests_path_for_unnamed_buffer() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());
        let request = core.apply_command(Command::Save, viewport()).unwrap();
        assert!(matches!(
            request,
            Some(FrontendRequest::SaveFilePicker {
                suggested_path: None
            })
        ));
    }

    #[test]
    fn save_to_path_clears_dirty_state() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());
        core.apply_command(Command::InsertChar('x'), viewport())
            .unwrap();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rcte-core-save-{unique}.txt"));
        core.save_to_path(path.clone());

        assert!(!core.buffer().is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "x");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn quit_requires_confirmation_when_dirty() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());
        core.apply_command(Command::InsertChar('x'), viewport())
            .unwrap();

        core.apply_command(Command::Quit, viewport()).unwrap();
        assert!(!core.should_quit());

        core.apply_command(Command::Quit, viewport()).unwrap();
        assert!(core.should_quit());
    }

    // Tests for new selection and clipboard functionality
    #[test]
    fn set_cursor_position_clamps_to_valid_range() {
        let mut core = EditorCore::new(Buffer::from_content("hello\nworld".to_string()), config());

        // Test setting cursor within valid range
        core.set_cursor_position(0, 3, viewport());
        assert_eq!(core.cursor_row, 0);
        assert_eq!(core.cursor_col, 3);

        // Test clamping row to max
        core.set_cursor_position(100, 0, viewport());
        assert_eq!(core.cursor_row, 1); // max row (2 lines - 1)

        // Test clamping col to line length
        core.set_cursor_position(0, 100, viewport());
        assert_eq!(core.cursor_row, 0);
        assert_eq!(core.cursor_col, 5); // "hello" length
    }

    #[test]
    fn handle_select_all_selects_entire_document() {
        let mut core = EditorCore::new(Buffer::from_content("hello\nworld".to_string()), config());

        core.handle_select_all();

        assert!(core.has_selection());
        assert_eq!(core.selection_start, Some((0, 0)));
        assert_eq!(core.selection_end, Some((1, 5))); // row 1, "world" length
    }

    #[test]
    fn selection_state_is_maintained_correctly() {
        let mut core = EditorCore::new(Buffer::from_content("hello\nworld".to_string()), config());

        // Initially no selection
        assert!(!core.has_selection());
        assert_eq!(core.get_selection_range(), None);

        // Set selection start
        core.set_selection_start(0, 2);
        assert!(!core.has_selection());
        assert_eq!(core.selection_start, Some((0, 2)));
        assert_eq!(core.selection_end, Some((0, 2)));

        // Update selection end
        core.set_selection_end(1, 3);
        assert!(core.has_selection());
        assert_eq!(core.selection_start, Some((0, 2)));
        assert_eq!(core.selection_end, Some((1, 3)));

        // Clear selection
        core.clear_selection();
        assert!(!core.has_selection());
        assert_eq!(core.selection_start, None);
        assert_eq!(core.selection_end, None);
    }

    #[test]
    fn get_selected_text_returns_correct_text() {
        let mut core = EditorCore::new(Buffer::from_content("hello\nworld".to_string()), config());

        // Select "ell" from first line
        core.set_selection_start(0, 1);
        core.set_selection_end(0, 4);

        let selected = core.get_selected_text();
        assert_eq!(selected, Some("ell".to_string()));

        // Select across lines
        core.set_selection_start(0, 3);
        core.set_selection_end(1, 2);

        let selected = core.get_selected_text();
        assert_eq!(selected, Some("lo\nwo".to_string()));
    }

    #[test]
    fn select_all_command_works() {
        let mut core = EditorCore::new(Buffer::from_content("abc\ndef".to_string()), config());

        core.apply_command(Command::SelectAll, viewport()).unwrap();

        let snapshot = core.snapshot();
        assert_eq!(snapshot.selection_start, Some((0, 0)));
        assert_eq!(snapshot.selection_end, Some((1, 3)));
    }

    #[test]
    fn insert_text_at_cursor_works() {
        let mut core = EditorCore::new(Buffer::from_content("hello world".to_string()), config());

        // Position cursor and insert text
        core.set_cursor_position(0, 6, viewport());
        core.insert_text_at_cursor("beautiful ", viewport());

        assert_eq!(core.buffer().line(0), "hello beautiful world");
        assert_eq!(core.cursor_col, 16); // 6 + "beautiful ".len()
    }

    #[test]
    fn insert_text_with_newlines() {
        let mut core = EditorCore::new(Buffer::new_empty(), config());

        core.insert_text_at_cursor("line1\nline2\nline3", viewport());

        assert_eq!(core.buffer().line_count(), 3);
        assert_eq!(core.buffer().line(0), "line1");
        assert_eq!(core.buffer().line(1), "line2");
        assert_eq!(core.buffer().line(2), "line3");
    }

    #[test]
    fn delete_selection_removes_text() {
        let mut core = EditorCore::new(Buffer::from_content("hello world".to_string()), config());

        // Select "world" and delete it
        core.set_selection_start(0, 6);
        core.set_selection_end(0, 11);
        let deleted = core.delete_selection();

        assert_eq!(deleted, Some("world".to_string()));
        assert_eq!(core.buffer().line(0), "hello ");
        assert_eq!(core.cursor_col, 6); // Cursor at start of selection
        assert!(!core.has_selection()); // Selection cleared
    }

    #[test]
    fn insert_char_replaces_selection_in_single_undo_step() {
        let mut core = EditorCore::new(Buffer::from_content("hello world".to_string()), config());
        core.set_selection_start(0, 6);
        core.set_selection_end(0, 11);

        core.apply_command(Command::InsertChar('R'), viewport())
            .unwrap();
        assert_eq!(core.buffer().line(0), "hello R");

        core.apply_command(Command::Undo, viewport()).unwrap();
        assert_eq!(core.buffer().line(0), "hello world");
    }

    #[test]
    fn cut_selection_is_undoable() {
        let mut core = EditorCore::new(Buffer::from_content("abc def".to_string()), config());
        core.set_selection_start(0, 4);
        core.set_selection_end(0, 7);

        let cut = core.cut_selection(viewport());
        assert_eq!(cut.as_deref(), Some("def"));
        assert_eq!(core.buffer().line(0), "abc ");

        core.apply_command(Command::Undo, viewport()).unwrap();
        assert_eq!(core.buffer().line(0), "abc def");
    }

    #[test]
    fn set_scroll_offset_works() {
        let mut core = EditorCore::new(
            Buffer::from_content("a\nb\nc\nd\ne\nf".to_string()),
            config(),
        );

        core.set_scroll_offset(3);
        assert_eq!(core.snapshot().scroll_offset, 3);

        // Should not panic with large values
        core.set_scroll_offset(100);
        assert_eq!(core.snapshot().scroll_offset, 100);
    }
}

#[cfg(test)]
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
        // cursor at 0; next_word_start(0) = 6 (skips "hello" and trailing space, leaves "world")
        core.apply_command(Command::DeleteWordRight, vp()).unwrap();
        assert_eq!(core.buffer().line(0), "world");
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

#[cfg(test)]
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

    #[test]
    fn move_down_crosses_buffer_row() {
        // Two lines; cursor on first, already at last visual row → moves to row 1
        let mut core = make_core_wrap("hello\nworld");
        // "hello" is 5 chars, wrap_width=13: only 1 visual row
        // so move_down goes to next buffer row
        core.apply_command(Command::MoveDown, vp_narrow()).unwrap();
        assert_eq!(core.cursor_row, 1);
    }

    #[test]
    fn move_up_crosses_buffer_row() {
        let mut core = make_core_wrap("hello\nworld");
        core.cursor_row = 1;
        core.cursor_col = 3;
        core.apply_command(Command::MoveUp, vp_narrow()).unwrap();
        assert_eq!(core.cursor_row, 0);
    }

    #[test]
    fn move_down_at_last_row_does_nothing() {
        let mut core = make_core_wrap("hello");
        core.apply_command(Command::MoveDown, vp_narrow()).unwrap();
        // Only one buffer row, no next row → stays put
        assert_eq!((core.cursor_row, core.cursor_col), (0, 0));
    }

    #[test]
    fn move_up_at_first_row_does_nothing() {
        let mut core = make_core_wrap("hello");
        core.apply_command(Command::MoveUp, vp_narrow()).unwrap();
        assert_eq!((core.cursor_row, core.cursor_col), (0, 0));
    }
}
