use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(feature = "collab")]
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use crate::buffer::Buffer;
use crate::config::Config;
use crate::core::{EditorCore, FrontendRequest, SearchMode, ViewportMetrics};
use crate::keymap::{self, Command};
use crate::terminal::RawModeGuard;
use crate::ui::{RenderState, Ui};

#[cfg(feature = "collab")]
use crate::collab::{CollabEvent, CollabHandle, CollabRole, OpKind};

#[cfg(feature = "collab")]
const COLLAB_QUIT_CONFIRM_DURATION: Duration = Duration::from_secs(3);

pub struct Editor {
    core: EditorCore,
    _guard: RawModeGuard,
    ui: Ui,
    prompt: Option<PromptState>,
    forced_quit: bool,
    #[cfg(feature = "collab")]
    collab_quit_count: u8,
    #[cfg(feature = "collab")]
    collab_last_quit_at: Option<Instant>,
    #[cfg(feature = "collab")]
    collab: Option<CollabHandle>,
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
    #[cfg(not(feature = "collab"))]
    pub fn new(buffer: Buffer, config: Config) -> Result<Self> {
        Self::new_inner(buffer, config)
    }

    #[cfg(feature = "collab")]
    pub fn new(buffer: Buffer, config: Config, collab: Option<CollabHandle>) -> Result<Self> {
        let mut editor = Self::new_inner(buffer, config)?;
        editor.collab = collab;
        Ok(editor)
    }

    fn new_inner(buffer: Buffer, config: Config) -> Result<Self> {
        let guard = RawModeGuard::new()?;
        let (width, height) = crate::terminal::size()?;
        let core = EditorCore::new(buffer, config);
        let mut ui = Ui::new(width as usize, height as usize);
        ui.show_line_numbers = core.show_line_numbers();
        ui.theme = core.theme().to_string();

        Ok(Self {
            core,
            _guard: guard,
            ui,
            prompt: None,
            forced_quit: false,
            #[cfg(feature = "collab")]
            collab_quit_count: 0,
            #[cfg(feature = "collab")]
            collab_last_quit_at: None,
            #[cfg(feature = "collab")]
            collab: None,
            #[cfg(feature = "collab")]
            last_sent_cursor: None,
        })
    }

    pub fn set_startup_message(&mut self, msg: String) {
        self.core.set_startup_message(msg);
    }

    pub fn run(&mut self) -> Result<()> {
        self.core.set_persistent_message(
            "Ctrl+Q quit  Ctrl+O open  Ctrl+S save  Ctrl+F find".to_string(),
        );

        loop {
            if let Ok((width, height)) = crate::terminal::size() {
                self.ui.width = width as usize;
                self.ui.height = height as usize;
            }
            self.ui.show_line_numbers = self.core.show_line_numbers();
            self.ui.theme = self.core.theme().to_string();

            #[cfg(feature = "collab")]
            self.apply_collab_events();

            #[cfg(feature = "collab")]
            self.send_cursor_if_needed();

            let snapshot = self.core.snapshot();

            #[cfg(feature = "collab")]
            let collab_status = self.collab.as_ref().map(|handle| handle.status_str());
            #[cfg(feature = "collab")]
            let peer_cursors = self
                .collab
                .as_ref()
                .map(|handle| handle.peer_cursors())
                .unwrap_or_default();

            self.ui.render(
                self.core.buffer(),
                &RenderState {
                    cursor_row: snapshot.cursor_row,
                    cursor_col: snapshot.cursor_col,
                    scroll_offset: snapshot.scroll_offset,
                    col_offset: snapshot.col_offset,
                    soft_wrap: snapshot.soft_wrap,
                    message: snapshot.message.as_deref(),
                    search_match: snapshot.search_match,
                    search_mode: self.core.search_mode(),
                    case_sensitive: self.core.snapshot().search.as_ref().map(|s| s.case_sensitive).unwrap_or(false),
                    search_replacement: self.core.search_replacement().as_deref(),
                    file_ext: snapshot.file_ext.as_deref(),
                    #[cfg(feature = "collab")]
                    collab_status: collab_status.as_deref(),
                    #[cfg(feature = "collab")]
                    peer_cursors: &peer_cursors,
                },
            )?;

            if self.forced_quit || self.core.should_quit() {
                break;
            }

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key)?;
                }
            }
        }

        Ok(())
    }

    fn viewport_metrics(&self) -> ViewportMetrics {
        ViewportMetrics {
            rows: self.ui.viewport_rows(),
            cols: self.ui.width,
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if self.prompt.is_some() {
            return self.handle_prompt_key(key);
        }
        if self.core.is_search_active() {
            return self.handle_search_key(key);
        }

        let command = keymap::map(key);
        self.handle_command(command)
    }

    fn handle_command(&mut self, command: Command) -> Result<()> {
        #[cfg(feature = "collab")]
        if self.handle_collab_only_commands(&command) {
            return Ok(());
        }

        #[cfg(feature = "collab")]
        let pending_remote = self.pending_collab_op(&command);

        let request = self.core.apply_command(command, self.viewport_metrics())?;

        #[cfg(feature = "collab")]
        self.dispatch_collab_op(pending_remote);

        if let Some(request) = request {
            self.handle_frontend_request(request)?;
        }

        Ok(())
    }

    fn handle_frontend_request(&mut self, request: FrontendRequest) -> Result<()> {
        match request {
            FrontendRequest::OpenFilePicker { initial_dir } => {
                self.pick_and_open_file(initial_dir.as_deref())?
            }
            FrontendRequest::SaveFilePicker { suggested_path } => {
                self.start_save_prompt(suggested_path);
            }
        }
        Ok(())
    }

    fn start_save_prompt(&mut self, suggested_path: Option<PathBuf>) {
        self.prompt = Some(PromptState {
            kind: PromptKind::SaveAs,
            input: suggested_path
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
        self.set_prompt_message();
    }

    fn handle_prompt_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let (kind, input) = match self.prompt.as_ref() {
                    Some(prompt) => (prompt.kind, prompt.input.clone()),
                    None => return Ok(()),
                };

                self.prompt = None;
                match kind {
                    PromptKind::SaveAs => {
                        if input.is_empty() {
                            self.core.set_message("Save cancelled.".to_string());
                        } else {
                            self.core.save_to_path(PathBuf::from(input));
                        }
                    }
                }
            }
            KeyCode::Esc => {
                self.prompt = None;
                self.core.set_message("Save cancelled.".to_string());
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
        self.core.set_message(message);
    }

    fn handle_search_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        let viewport = self.viewport_metrics();
        let mode = self.core.search_mode();
        match (mode, key.code) {
            // Searching mode
            (Some(SearchMode::Searching), KeyCode::Esc) => self.core.cancel_search(),
            (Some(SearchMode::Searching), KeyCode::Enter) | (Some(SearchMode::Searching), KeyCode::Char('n')) => {
                self.core.search_next(viewport)
            }
            (Some(SearchMode::Searching), KeyCode::Char('N')) => self.core.search_prev(viewport),
            (Some(SearchMode::Searching), KeyCode::Backspace) => self.core.pop_search_char(viewport),
            (Some(SearchMode::Searching), KeyCode::Char('c')) if key.modifiers.contains(KeyModifiers::ALT) => {
                self.core.toggle_case_sensitive(viewport)
            }
            (Some(SearchMode::Searching), KeyCode::Char('r')) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.core.enter_replace_mode()
            }
            (Some(SearchMode::Searching), KeyCode::Char(ch)) => self.core.append_search_char(ch, viewport),

            // ReplacePrompt mode — typing builds replacement string
            (Some(SearchMode::ReplacePrompt), KeyCode::Esc) => self.core.cancel_search(),
            (Some(SearchMode::ReplacePrompt), KeyCode::Enter) => {
                self.core.transition_to_replacing();
            }
            (Some(SearchMode::ReplacePrompt), KeyCode::Backspace) => {
                if let Some(s) = self.core.search_replacement_mut() {
                    s.pop();
                }
            }
            (Some(SearchMode::ReplacePrompt), KeyCode::Char(ch)) => self.core.append_replacement_char(ch),

            // Replacing mode
            (Some(SearchMode::Replacing), KeyCode::Esc) => self.core.cancel_search(),
            (Some(SearchMode::Replacing), KeyCode::Enter) => self.core.apply_replace_one(viewport),
            (Some(SearchMode::Replacing), KeyCode::Char('a')) => self.core.apply_replace_all(viewport),

            (None, _) => {} // No active search, ignore
            _ => {}
        }
        Ok(())
    }

    fn pick_and_open_file(&mut self, initial_dir: Option<&Path>) -> Result<()> {
        let selected =
            crate::terminal::suspend(|| crate::file_picker::pick_open_file(initial_dir))?;

        match selected {
            Some(path) if path.is_file() => self.core.open_path(path, self.viewport_metrics()),
            Some(_) => self
                .core
                .set_message("Open error: Selected path is not a file.".to_string()),
            None => self.core.set_message("Open cancelled.".to_string()),
        }

        Ok(())
    }

    #[cfg(feature = "collab")]
    fn handle_collab_only_commands(&mut self, command: &Command) -> bool {
        let Some(handle) = &self.collab else {
            return false;
        };

        match command {
            Command::Save | Command::SaveAs => {
                if matches!(handle.role, CollabRole::Guest { .. }) {
                    self.core.set_message("Only the host can save.".to_string());
                    return true;
                }
            }
            Command::Open => {
                self.core
                    .set_message("Open is unavailable during collaboration.".to_string());
                return true;
            }
            Command::Quit => match &handle.role {
                CollabRole::Guest { .. } => {
                    self.forced_quit = true;
                    return true;
                }
                CollabRole::Host { .. } if handle.peer_count() > 0 => {
                    let now = Instant::now();
                    let within_window = self
                        .collab_last_quit_at
                        .map(|instant| now.duration_since(instant) < COLLAB_QUIT_CONFIRM_DURATION)
                        .unwrap_or(false);
                    if within_window && self.collab_quit_count >= 1 {
                        self.forced_quit = true;
                    } else {
                        self.collab_quit_count = 1;
                        self.collab_last_quit_at = Some(now);
                        self.core.set_message(
                            "Disconnect all peers and quit? Press Ctrl+Q again to confirm."
                                .to_string(),
                        );
                    }
                    return true;
                }
                CollabRole::Host { .. } => {}
            },
            _ => {}
        }

        false
    }

    #[cfg(feature = "collab")]
    fn pending_collab_op(&self, command: &Command) -> Option<(OpKind, usize, String)> {
        let Some(_) = &self.collab else {
            return None;
        };

        let cursor_offset = self.core.buffer().char_offset_for(
            self.core.snapshot().cursor_row,
            self.core.snapshot().cursor_col,
        );

        match command {
            Command::InsertChar(ch) => Some((OpKind::Insert, cursor_offset, ch.to_string())),
            Command::InsertNewline => Some((OpKind::Insert, cursor_offset, "\n".to_string())),
            Command::InsertTab => Some((
                OpKind::Insert,
                cursor_offset,
                " ".repeat(self.core.tab_width()),
            )),
            Command::Backspace => {
                if cursor_offset == 0 {
                    None
                } else {
                    self.core
                        .buffer()
                        .char_at_offset(cursor_offset - 1)
                        .map(|ch| (OpKind::Delete, cursor_offset - 1, ch.to_string()))
                }
            }
            Command::DeleteChar => self
                .core
                .buffer()
                .char_at_offset(cursor_offset)
                .map(|ch| (OpKind::Delete, cursor_offset, ch.to_string())),
            _ => None,
        }
    }

    #[cfg(feature = "collab")]
    fn dispatch_collab_op(&self, pending: Option<(OpKind, usize, String)>) {
        let Some((kind, pos, text)) = pending else {
            return;
        };
        let Some(handle) = &self.collab else {
            return;
        };

        match kind {
            OpKind::Insert => handle.send_insert(pos, text),
            OpKind::Delete => handle.send_delete(pos, text),
        }
    }

    #[cfg(feature = "collab")]
    fn apply_collab_events(&mut self) {
        loop {
            let event = match self.collab.as_mut() {
                Some(handle) => handle.try_recv(),
                None => break,
            };

            match event {
                Some(CollabEvent::Edit {
                    kind,
                    pos,
                    text,
                    peer: _,
                    rev: _,
                }) => match kind {
                    OpKind::Insert => {
                        self.core
                            .apply_remote_insert(pos, &text, self.viewport_metrics());
                    }
                    OpKind::Delete => {
                        self.core
                            .apply_remote_delete(pos, &text, self.viewport_metrics());
                    }
                },
                Some(CollabEvent::FullSync { content, rev: _ }) => {
                    self.core
                        .replace_with_content(content, self.viewport_metrics());
                }
                Some(CollabEvent::LocalConfirm { .. })
                | Some(CollabEvent::PeersChanged { .. })
                | Some(CollabEvent::PeerCursor { .. })
                | Some(CollabEvent::ConnectionStatus { .. }) => {}
                None => break,
            }
        }
    }

    #[cfg(feature = "collab")]
    fn send_cursor_if_needed(&mut self) {
        let offset = self.core.buffer().char_offset_for(
            self.core.snapshot().cursor_row,
            self.core.snapshot().cursor_col,
        );
        if Some(offset) != self.last_sent_cursor {
            if let Some(handle) = &self.collab {
                handle.send_cursor(offset);
            }
            self.last_sent_cursor = Some(offset);
        }
    }
}
