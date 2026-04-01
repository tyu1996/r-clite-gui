use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use arboard::Clipboard;
use eframe::egui::{
    self, Align2, Color32, Context, FontId, Frame, Key, RichText, TextFormat, TextStyle,
    TopBottomPanel, Vec2, ViewportCommand, text::CCursor,
};
use egui::text::LayoutJob;

use crate::{
    buffer::Buffer,
    config::Config,
    core::{EditorCore, FrontendRequest, ViewSnapshot, ViewportMetrics},
    highlight,
    keymap::Command,
};

const TOOLBAR_HEIGHT: f32 = 34.0;
const FOOTER_HEIGHT: f32 = 46.0;
const PANEL_PADDING: f32 = 12.0;

pub fn launch(file: Option<PathBuf>) -> Result<()> {
    let (config, warning) = Config::load();
    let buffer = match file {
        Some(path) => Buffer::open(path)?,
        None => Buffer::new_empty(),
    };

    let app = GuiApp::new(buffer, config, warning);
    let native_options = eframe::NativeOptions::default();

    eframe::run_native(
        "rcte-gui",
        native_options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))
}

pub struct GuiApp {
    core: EditorCore,
    cached_viewport: ViewportMetrics,
    cached_row_height: f32,
    clipboard: Option<Clipboard>,
    // Mouse drag state for text selection
    drag_start: Option<(usize, usize)>,
    is_dragging: bool,
    // Editor area rect for mouse interaction
    editor_rect: Option<egui::Rect>,
    // Replace mode state
    gui_search_mode: bool,
    replace_text: String,
}

impl GuiApp {
    pub fn new(buffer: Buffer, config: Config, startup_warning: Option<String>) -> Self {
        let mut core = EditorCore::new(buffer, config);
        if let Some(warning) = startup_warning {
            core.set_startup_message(warning);
        }

        let clipboard = Clipboard::new().ok();
        if clipboard.is_none() {
            eprintln!("Warning: Could not initialize clipboard");
        }

        Self {
            core,
            cached_viewport: ViewportMetrics { rows: 1, cols: 1 },
            cached_row_height: 18.0,
            clipboard,
            drag_start: None,
            is_dragging: false,
            editor_rect: None,
            gui_search_mode: false,
            replace_text: String::new(),
        }
    }

    fn handle_input(&mut self, ctx: &Context) {
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            if self.handle_event(ctx, event) {
                ctx.request_repaint();
            }
        }
    }

    fn handle_event(&mut self, ctx: &Context, event: egui::Event) -> bool {
        if self.core.is_search_active() {
            return self.handle_search_event(ctx, event);
        }

        // Reflow paragraph: Option+Q (macOS) / Alt+Q (Win/Linux)
        if let egui::Event::Key { key: egui::Key::Q, pressed: true, modifiers, .. } = &event {
            if modifiers.alt {
                return self.apply_command(ctx, Command::ReflowParagraph);
            }
        }

        match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if modifiers.shift {
                    if let Some(command) = selection_movement_command(key) {
                        return self.extend_selection_with_command(ctx, command);
                    }
                } else if is_cursor_movement_key(key) {
                    self.core.clear_selection();
                }

                if let Some(command) = map_shortcut(key, modifiers) {
                    return self.apply_command(ctx, command);
                }

                match key {
                    Key::Enter => self.apply_command(ctx, Command::InsertNewline),
                    Key::Tab => self.apply_command(ctx, Command::InsertTab),
                    Key::Backspace if modifiers.alt || modifiers.ctrl => {
                        self.apply_command(ctx, Command::DeleteWordLeft)
                    }
                    Key::Backspace => self.apply_command(ctx, Command::Backspace),
                    Key::Delete if modifiers.alt || modifiers.ctrl => {
                        self.apply_command(ctx, Command::DeleteWordRight)
                    }
                    Key::Delete => self.apply_command(ctx, Command::DeleteChar),
                    Key::ArrowUp => self.apply_command(ctx, Command::MoveUp),
                    Key::ArrowDown => self.apply_command(ctx, Command::MoveDown),
                    Key::ArrowLeft if modifiers.alt => {
                        self.apply_command(ctx, Command::MoveWordLeft)
                    }
                    Key::ArrowRight if modifiers.alt => {
                        self.apply_command(ctx, Command::MoveWordRight)
                    }
                    #[cfg(not(target_os = "macos"))]
                    Key::ArrowLeft if modifiers.ctrl => {
                        self.apply_command(ctx, Command::MoveWordLeft)
                    }
                    #[cfg(not(target_os = "macos"))]
                    Key::ArrowRight if modifiers.ctrl => {
                        self.apply_command(ctx, Command::MoveWordRight)
                    }
                    Key::ArrowLeft => self.apply_command(ctx, Command::MoveLeft),
                    Key::ArrowRight => self.apply_command(ctx, Command::MoveRight),
                    Key::Home => self.apply_command(ctx, Command::MoveLineStart),
                    Key::End => self.apply_command(ctx, Command::MoveLineEnd),
                    Key::PageUp => self.apply_command(ctx, Command::PageUp),
                    Key::PageDown => self.apply_command(ctx, Command::PageDown),
                    _ => false,
                }
            }
            egui::Event::Text(text) | egui::Event::Paste(text) => {
                let mut handled = false;
                for ch in text.chars() {
                    if ch == '\n' {
                        handled |= self.apply_command(ctx, Command::InsertNewline);
                    } else if ch != '\r' {
                        handled |= self.apply_command(ctx, Command::InsertChar(ch));
                    }
                }
                handled
            }
            // Mouse button press - start click or drag
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            } => {
                if let Some((row, col)) = self.mouse_pos_to_row_col(pos) {
                    if modifiers.shift {
                        // Shift+click extends selection
                        self.core.set_selection_end(row, col);
                    } else {
                        // Regular click - anchor selection start and position cursor
                        self.core.set_selection_start(row, col);
                        self.core
                            .set_cursor_position(row, col, self.cached_viewport);
                        self.drag_start = Some((row, col));
                    }
                    self.is_dragging = true;
                    true
                } else {
                    false
                }
            }
            // Mouse button release - end drag
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            } => {
                self.is_dragging = false;
                self.drag_start = None;
                true
            }
            // Mouse move - update selection during drag
            egui::Event::PointerMoved(pos) => {
                if self.is_dragging {
                    if let Some((row, col)) = self.mouse_pos_to_row_col(pos) {
                        if self.drag_start.is_some() {
                            // Update selection end during drag
                            self.core.set_selection_end(row, col);
                            // Also update cursor position
                            self.core
                                .set_cursor_position(row, col, self.cached_viewport);
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            // Mouse wheel scrolling
            egui::Event::MouseWheel { delta, .. } => {
                self.handle_scroll(delta);
                true
            }
            _ => false,
        }
    }

    /// Convert mouse position to (row, col) in the editor
    fn mouse_pos_to_row_col(&self, pos: egui::Pos2) -> Option<(usize, usize)> {
        let rect = self.editor_rect?;
        let snapshot = self.core.snapshot();
        let row_height = 18.0; // Matches font_metrics
        let char_width = 8.0; // Approximate char width
        let gutter_chars = if snapshot.show_line_numbers {
            self.core.buffer().line_count().to_string().len() + 1
        } else {
            0
        };
        let gutter_px = gutter_chars as f32 * char_width;
        let text_x = rect.left() + PANEL_PADDING + gutter_px;
        let text_y = rect.top() + PANEL_PADDING;

        // Check if click is within editor area
        if pos.x < text_x || pos.y < text_y {
            return None;
        }

        // Calculate row and col
        let rel_y = pos.y - text_y;
        let rel_x = pos.x - text_x;
        let row = (rel_y / row_height).floor() as usize + snapshot.scroll_offset;
        let col = (rel_x / char_width).floor() as usize + snapshot.col_offset;

        // Clamp to valid range
        let max_row = self.core.buffer().line_count().saturating_sub(1);
        let clamped_row = row.min(max_row);
        let line_len = self.core.buffer().line_len(clamped_row);
        let clamped_col = col.min(line_len);

        Some((clamped_row, clamped_col))
    }

    fn handle_scroll(&mut self, delta: egui::Vec2) {
        let lines = (delta.y / self.cached_row_height).round() as isize;
        if lines > 0 {
            self.scroll_down(lines as usize);
        } else if lines < 0 {
            self.scroll_up((-lines) as usize);
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        let snapshot = self.core.snapshot();
        let viewport = self.cached_viewport;
        let line_count = self.core.buffer().line_count();
        let max_scroll = if self.core.soft_wrap {
            let gutter_chars = if snapshot.show_line_numbers {
                line_count.to_string().len() + 1
            } else {
                0
            };
            let text_width = viewport.cols.saturating_sub(gutter_chars);
            self.core.total_visual_rows(text_width).saturating_sub(viewport.rows)
        } else {
            line_count.saturating_sub(viewport.rows)
        };
        let new_scroll = snapshot.scroll_offset.saturating_sub(lines).min(max_scroll);
        self.core.set_scroll_offset(new_scroll);
    }

    fn scroll_down(&mut self, lines: usize) {
        let snapshot = self.core.snapshot();
        let viewport = self.cached_viewport;
        let line_count = self.core.buffer().line_count();
        let max_scroll = if self.core.soft_wrap {
            let gutter_chars = if snapshot.show_line_numbers {
                line_count.to_string().len() + 1
            } else {
                0
            };
            let text_width = viewport.cols.saturating_sub(gutter_chars);
            self.core.total_visual_rows(text_width).saturating_sub(viewport.rows)
        } else {
            line_count.saturating_sub(viewport.rows)
        };
        let new_scroll = (snapshot.scroll_offset + lines).min(max_scroll);
        self.core.set_scroll_offset(new_scroll);
    }

    fn handle_search_event(&mut self, ctx: &Context, event: egui::Event) -> bool {
        match event {
            egui::Event::Key {
                key: Key::Escape,
                pressed: true,
                ..
            } => {
                self.core.cancel_search();
                self.gui_search_mode = false;
                true
            }
            egui::Event::Key {
                key: Key::Enter,
                pressed: true,
                modifiers,
                ..
            } => {
                if modifiers.command && modifiers.shift {
                    // Cmd+Shift+Enter → Replace All
                    if self.gui_search_mode && self.core.is_search_active() {
                        self.core.apply_replace_all(self.cached_viewport);
                        self.gui_search_mode = false;
                        return true;
                    }
                }
                if self.gui_search_mode && self.core.is_search_active() {
                    // Enter in Replace mode → Replace current and advance
                    self.core.apply_replace_one(self.cached_viewport);
                    true
                } else {
                    // Enter in Find mode → advance to next match
                    self.core.search_next(self.cached_viewport);
                    true
                }
            }
            egui::Event::Key {
                key: Key::Backspace,
                pressed: true,
                ..
            } => {
                self.core.pop_search_char(self.cached_viewport);
                true
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(command) = map_shortcut(key, modifiers) {
                    return self.apply_command(ctx, command);
                }
                false
            }
            egui::Event::Text(text) | egui::Event::Paste(text) => {
                for ch in text.chars() {
                    if ch != '\r' && ch != '\n' {
                        self.core.append_search_char(ch, self.cached_viewport);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn apply_command(&mut self, ctx: &Context, command: Command) -> bool {
        // Handle clipboard operations directly in GUI
        match command {
            Command::Copy => {
                if let Some(text) = self.core.get_selected_text() {
                    if let Some(ref mut clipboard) = self.clipboard {
                        if let Err(e) = clipboard.set_text(text) {
                            self.core.set_message(format!("Copy failed: {}", e));
                        } else {
                            self.core.set_message("Copied to clipboard".to_string());
                        }
                    } else {
                        self.core.set_message("Clipboard not available".to_string());
                    }
                }
                return true;
            }
            Command::Paste => {
                if let Some(ref mut clipboard) = self.clipboard {
                    match clipboard.get_text() {
                        Ok(text) => {
                            self.core.insert_text_at_cursor(&text, self.cached_viewport);
                            self.core.set_message("Pasted from clipboard".to_string());
                        }
                        Err(e) => {
                            self.core.set_message(format!("Paste failed: {}", e));
                        }
                    }
                } else {
                    self.core.set_message("Clipboard not available".to_string());
                }
                return true;
            }
            Command::Cut => {
                if self.core.has_selection() {
                    if let Some(text) = self.core.cut_selection(self.cached_viewport) {
                        if let Some(ref mut clipboard) = self.clipboard {
                            if let Err(e) = clipboard.set_text(text) {
                                self.core.set_message(format!("Cut failed: {}", e));
                            } else {
                                self.core.set_message("Cut to clipboard".to_string());
                            }
                        } else {
                            self.core.set_message("Clipboard not available".to_string());
                        }
                    }
                }
                return true;
            }
            Command::Replace => {
                if self.core.is_search_active() {
                    self.gui_search_mode = true;
                }
                return true;
            }
            Command::ToggleCaseSensitive => {
                if self.core.is_search_active() {
                    self.core.toggle_case_sensitive(self.cached_viewport);
                }
                return true;
            }
            Command::ReplaceAll => {
                if self.core.is_search_active() {
                    self.core.apply_replace_all(self.cached_viewport);
                    self.gui_search_mode = false;
                }
                return true;
            }
            _ => {}
        }

        let request = match self.core.apply_command(command, self.cached_viewport) {
            Ok(request) => request,
            Err(err) => {
                self.core.set_message(format!("Command error: {:#}", err));
                return true;
            }
        };

        if let Some(request) = request {
            self.handle_request(request, ctx);
        }

        if self.core.should_quit() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        true
    }

    fn handle_request(&mut self, request: FrontendRequest, ctx: &Context) {
        match request {
            FrontendRequest::OpenFilePicker { initial_dir } => {
                if let Some(path) = pick_open_file(initial_dir.as_deref()) {
                    self.core.open_path(path, self.cached_viewport);
                } else {
                    self.core.set_message("Open cancelled.".to_string());
                }
            }
            FrontendRequest::SaveFilePicker { suggested_path } => {
                if let Some(path) = pick_save_file(suggested_path.as_deref()) {
                    self.core.save_to_path(path);
                } else {
                    self.core.set_message("Save cancelled.".to_string());
                }
            }
        }

        if self.core.should_quit() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn extend_selection_with_command(&mut self, ctx: &Context, command: Command) -> bool {
        let before = self.core.snapshot();
        if !self.core.has_selection() {
            self.core
                .set_selection_start(before.cursor_row, before.cursor_col);
        }

        let handled = self.apply_command(ctx, command);
        let after = self.core.snapshot();
        self.core
            .set_selection_end(after.cursor_row, after.cursor_col);
        handled
    }

    fn render_chrome(&mut self, ctx: &Context) {
        TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_HEIGHT)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open").clicked() {
                        let _ = self.apply_command(ctx, Command::Open);
                    }
                    if ui.button("Save").clicked() {
                        let _ = self.apply_command(ctx, Command::Save);
                    }
                    if ui.button("Save As").clicked() {
                        let _ = self.apply_command(ctx, Command::SaveAs);
                    }
                    if ui.button("Find").clicked() {
                        let _ = self.apply_command(ctx, Command::Find);
                    }
                    if ui
                        .selectable_label(self.core.show_line_numbers(), "Line Numbers")
                        .clicked()
                    {
                        let _ = self.apply_command(ctx, Command::ToggleLineNumbers);
                    }
                    if self.core.is_search_active() {
                        if ui.button("Prev").clicked() {
                            self.core.search_prev(self.cached_viewport);
                        }
                        if ui.button("Next").clicked() {
                            self.core.search_next(self.cached_viewport);
                        }
                        if self.gui_search_mode {
                            // Replace field
                            ui.label("Replace:");
                            let response = egui::TextEdit::singleline(&mut self.replace_text)
                                .desired_width(120.0)
                                .show(ui);
                            if response.response.lost_focus() {
                                // Update replacement text in core when user finishes editing
                                if let Some(replacement) = self.core.search_replacement_mut() {
                                    *replacement = self.replace_text.clone();
                                }
                            }
                            // Match Case checkbox
                            let snapshot = self.core.snapshot();
                            let mut case_on = snapshot.case_sensitive;
                            if ui.checkbox(&mut case_on, "Match Case").clicked() {
                                self.core.toggle_case_sensitive(self.cached_viewport);
                            }
                            // Replace button
                            if ui.button("Replace").clicked() {
                                if let Some(replacement) = self.core.search_replacement_mut() {
                                    *replacement = self.replace_text.clone();
                                }
                                self.core.apply_replace_one(self.cached_viewport);
                            }
                            // Replace All button
                            if ui.button("Replace All").clicked() {
                                if let Some(replacement) = self.core.search_replacement_mut() {
                                    *replacement = self.replace_text.clone();
                                }
                                self.core.apply_replace_all(self.cached_viewport);
                                self.gui_search_mode = false;
                            }
                        }
                    }
                });
            });

        TopBottomPanel::bottom("footer")
            .exact_height(FOOTER_HEIGHT)
            .show(ctx, |ui| {
                let snapshot = self.core.snapshot();
                let status = status_text(&self.core, &snapshot);
                let message = snapshot.message.unwrap_or_default();

                ui.vertical_centered_justified(|ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(status).monospace().strong());
                        if snapshot.case_sensitive {
                            ui.label(RichText::new(" | Match case: ON").monospace().color(ui.visuals().hyperlink_color));
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let color = if self.core.is_search_active() {
                            ui.visuals().hyperlink_color
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.label(RichText::new(message).monospace().color(color));
                    });
                });
            });
    }

    fn render_editor(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(Frame::default().fill(background_color(self.core.theme())))
            .show(ctx, |ui| {
                let available = ui.available_size();
                let (row_height, char_width, line_height) = font_metrics(ui);
                self.cached_row_height = row_height;
                self.cached_viewport = viewport_from_size(
                    available,
                    char_width,
                    line_height,
                    self.core.show_line_numbers(),
                    self.core.buffer().line_count(),
                );

                let rect = ui.available_rect_before_wrap();
                self.editor_rect = Some(rect);
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, background_color(self.core.theme()));

                let snapshot = self.core.snapshot();
                let line_count = self.core.buffer().line_count();
                let gutter_chars = if snapshot.show_line_numbers {
                    line_count.to_string().len() + 1
                } else {
                    0
                };
                let gutter_px = gutter_chars as f32 * char_width;
                let text_x = rect.left() + PANEL_PADDING + gutter_px;
                let gutter_x = rect.left() + PANEL_PADDING;
                let text_cols = self
                    .cached_viewport
                    .cols
                    .saturating_sub(gutter_chars)
                    .max(1);
                let text_color = text_color(self.core.theme());
                let gutter_color = gutter_color(self.core.theme());
                let cursor_color = cursor_color(self.core.theme());
                let current_line_bg = current_line_color(self.core.theme());
                let search_bg = search_bg_color(self.core.theme());

                let first_line = snapshot.scroll_offset;
                let visible_rows = self.cached_viewport.rows.max(1);
                let last_line = (first_line + visible_rows).min(line_count);
                let mut in_block_comment = false;
                let selection_bg = selection_bg_color(self.core.theme());

                // Get normalized selection range if there is one
                let selection_range = if let (Some(start), Some(end)) =
                    (snapshot.selection_start, snapshot.selection_end)
                {
                    let norm_start = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
                        start
                    } else {
                        end
                    };
                    let norm_end = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
                        end
                    } else {
                        start
                    };
                    Some((norm_start, norm_end))
                } else {
                    None
                };

                let mut current_y = rect.top() + PANEL_PADDING;
                let mut showing_line_number = false;
                for (_row_index, file_row) in (first_line..last_line).enumerate() {
                    // Reset line number tracking for each new buffer row
                    showing_line_number = false;
                    let line = self.core.buffer().line(file_row);
                    let line_start = self.core.buffer().char_offset_for(file_row, 0);
                    let (render_col_offset, render_max_chars) = if snapshot.soft_wrap {
                        (0, usize::MAX)
                    } else {
                        (snapshot.col_offset, text_cols)
                    };
                    let (job, next_block_comment) = line_layout_job(
                        &line,
                        snapshot.file_ext.as_deref(),
                        self.core.theme(),
                        FontId::monospace(row_height - 2.0),
                        text_color,
                        search_bg,
                        snapshot.search_match,
                        line_start,
                        render_col_offset,
                        render_max_chars,
                        in_block_comment,
                        snapshot.soft_wrap,
                        rect.width() - PANEL_PADDING * 2.0 - gutter_px,
                    );
                    in_block_comment = next_block_comment;

                    // For soft wrap, use galley height for line background so highlight
                    // covers all wrapped visual rows of the current line
                    let galley = painter.layout_job(job);
                    let line_rect_height = if snapshot.soft_wrap {
                        galley.size().y
                    } else {
                        row_height
                    };
                    let line_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + PANEL_PADDING, current_y),
                        Vec2::new(rect.width() - PANEL_PADDING * 2.0, line_rect_height),
                    );
                    if snapshot.cursor_row == file_row {
                        painter.rect_filled(line_rect, 2.0, current_line_bg);
                    }

                    // With soft wrap, only show line number on first visual row of buffer row.
                    // For wrapped lines taller than row_height, we must clear the gutter
                    // background for the FULL galley height to prevent text overlap.
                    if snapshot.show_line_numbers {
                        let _full_gutter_height = if snapshot.soft_wrap && galley.size().y > row_height {
                            // Draw gutter background for full galley height so continuation
                            // rows don't show old text underneath
                            let gutter_rect = egui::Rect::from_min_size(
                                egui::pos2(gutter_x, current_y),
                                Vec2::new(gutter_px, galley.size().y),
                            );
                            painter.rect_filled(gutter_rect, 0.0, background_color(self.core.theme()));
                            galley.size().y
                        } else {
                            row_height
                        };
                        // Show line number only on first visual row of each buffer row
                        if !snapshot.soft_wrap || !showing_line_number {
                            let number = format!("{:>width$} ", file_row + 1, width = gutter_chars - 1);
                            painter.text(
                                egui::pos2(gutter_x, current_y),
                                Align2::LEFT_TOP,
                                number,
                                FontId::monospace(row_height - 2.0),
                                gutter_color,
                            );
                            showing_line_number = true;
                        } else {
                            // Blank gutter space for continuation rows
                            let blank: String = " ".repeat(gutter_chars);
                            painter.text(
                                egui::pos2(gutter_x, current_y),
                                Align2::LEFT_TOP,
                                blank,
                                FontId::monospace(row_height - 2.0),
                                gutter_color,
                            );
                        }
                    }

                    let text_origin = egui::pos2(text_x, current_y);
                    painter.galley(text_origin, galley.clone(), text_color);

                    // Draw selection highlight using galley cursor geometry instead of
                    // fixed char-width math so the highlight aligns exactly with text.
                    if let Some(((sel_start_row, sel_start_col), (sel_end_row, sel_end_col))) =
                        selection_range
                    {
                        if file_row >= sel_start_row && file_row <= sel_end_row {
                            let line_len = self.core.buffer().line_len(file_row);
                            let line_sel_start = if file_row == sel_start_row {
                                sel_start_col
                            } else {
                                0
                            };
                            let line_sel_end = if file_row == sel_end_row {
                                sel_end_col
                            } else {
                                line_len
                            };

                            let visible_start = line_sel_start.max(snapshot.col_offset);
                            let visible_end =
                                line_sel_end.min(snapshot.col_offset.saturating_add(text_cols));

                            if visible_start < visible_end {
                                let start_cursor =
                                    CCursor::new(visible_start.saturating_sub(snapshot.col_offset));
                                let end_cursor =
                                    CCursor::new(visible_end.saturating_sub(snapshot.col_offset));
                                let start_rect = galley.pos_from_cursor(start_cursor);
                                let end_rect = galley.pos_from_cursor(end_cursor);
                                let sel_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        text_origin.x + start_rect.min.x,
                                        text_origin.y + start_rect.min.y,
                                    ),
                                    Vec2::new(
                                        (end_rect.min.x - start_rect.min.x).max(2.0),
                                        start_rect.height().max(row_height),
                                    ),
                                );
                                painter.rect_filled(sel_rect, 1.0, selection_bg);
                            }
                        }
                    }

                    if snapshot.cursor_row == file_row {
                        draw_cursor(
                            &painter,
                            text_origin,
                            &galley,
                            snapshot.cursor_col,
                            snapshot.col_offset,
                            text_cols,
                            cursor_color,
                        );
                    }

                    // Advance Y position for next row - use galley height when soft wrapping
                    // so that wrapped lines don't overlap with the next buffer row
                    if snapshot.soft_wrap {
                        current_y += galley.size().y;
                    } else {
                        current_y += row_height;
                    }
                }

                if last_line < first_line + visible_rows {
                    for _ in 0..(first_line + visible_rows - last_line) {
                        if snapshot.show_line_numbers {
                            let blank: String = " ".repeat(gutter_chars);
                            painter.text(
                                egui::pos2(gutter_x, current_y),
                                Align2::LEFT_TOP,
                                blank,
                                FontId::monospace(row_height - 2.0),
                                gutter_color,
                            );
                        }
                        painter.text(
                            egui::pos2(text_x, current_y),
                            Align2::LEFT_TOP,
                            "~",
                            FontId::monospace(row_height - 2.0),
                            Color32::from_rgb(0, 255, 255), // DarkCyan equivalent
                        );
                        current_y += row_height;
                    }
                }
            });
    }

    pub fn core(&self) -> &EditorCore {
        &self.core
    }

    pub fn editor_rect(&self) -> Option<egui::Rect> {
        self.editor_rect
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.core.should_quit() {
            let _ = self.apply_command(ctx, Command::Quit);
            if !self.core.should_quit() {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            }
        }

        self.handle_input(ctx);
        self.render_chrome(ctx);
        self.render_editor(ctx);
        update_title(ctx, &self.core);
        if self.core.snapshot().message.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

fn map_shortcut(key: Key, modifiers: egui::Modifiers) -> Option<Command> {
    let shortcut = modifiers.command || modifiers.ctrl;
    if !shortcut {
        return None;
    }

    // macOS: Cmd+Option+F → Replace mode
    if let Key::F = key {
        if modifiers.command && modifiers.alt {
            return Some(Command::Replace);
        }
    }
    // Option+C → Toggle case (handled in apply_command when search is active)
    if let Key::C = key {
        if modifiers.alt {
            return Some(Command::ToggleCaseSensitive);
        }
    }

    match key {
        Key::Q => Some(Command::Quit),
        Key::S if modifiers.shift => Some(Command::SaveAs),
        Key::S => Some(Command::Save),
        Key::O => Some(Command::Open),
        Key::Z => Some(Command::Undo),
        Key::Y => Some(Command::Redo),
        Key::F if !modifiers.alt => Some(Command::Find),
        Key::L => Some(Command::ToggleLineNumbers),
        Key::W if modifiers.shift => Some(Command::ToggleSoftWrap),
        Key::C if !modifiers.alt => Some(Command::Copy),
        Key::V => Some(Command::Paste),
        Key::X => Some(Command::Cut),
        Key::A => Some(Command::SelectAll),
        _ => None,
    }
}

fn selection_movement_command(key: Key) -> Option<Command> {
    match key {
        Key::ArrowUp => Some(Command::MoveUp),
        Key::ArrowDown => Some(Command::MoveDown),
        Key::ArrowLeft => Some(Command::MoveLeft),
        Key::ArrowRight => Some(Command::MoveRight),
        Key::Home => Some(Command::MoveLineStart),
        Key::End => Some(Command::MoveLineEnd),
        Key::PageUp => Some(Command::PageUp),
        Key::PageDown => Some(Command::PageDown),
        _ => None,
    }
}

fn is_cursor_movement_key(key: Key) -> bool {
    selection_movement_command(key).is_some()
}

fn pick_open_file(initial_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Open file in rcte-gui");
    if let Some(dir) = initial_dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

fn pick_save_file(suggested_path: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Save file in rcte-gui");
    if let Some(path) = suggested_path {
        if let Some(parent) = path.parent() {
            dialog = dialog.set_directory(parent);
        }
        if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
            dialog = dialog.set_file_name(file_name);
        }
    }
    dialog.save_file()
}

fn update_title(ctx: &Context, core: &EditorCore) {
    let mut title = String::from("rcte-gui");
    title.push_str(" - ");
    title.push_str(&core.buffer().display_name());
    if core.buffer().is_dirty() {
        title.push_str(" [modified]");
    }
    ctx.send_viewport_cmd(ViewportCommand::Title(title));
}

fn status_text(core: &EditorCore, snapshot: &ViewSnapshot) -> String {
    let mut text = format!(
        "{} | Ln {}, Col {}",
        core.buffer().display_name(),
        snapshot.cursor_row + 1,
        snapshot.cursor_col + 1,
    );
    if core.buffer().is_dirty() {
        text.push_str(" | [modified]");
    }
    text.push_str(&format!(" | {}", core.theme()));
    text
}

fn viewport_from_size(
    size: Vec2,
    char_width: f32,
    row_height: f32,
    show_line_numbers: bool,
    line_count: usize,
) -> ViewportMetrics {
    let usable_width = (size.x - PANEL_PADDING * 2.0).max(char_width);
    let usable_height = (size.y - PANEL_PADDING * 2.0).max(row_height);
    let cols = (usable_width / char_width).floor().max(1.0) as usize;
    let rows = (usable_height / row_height).floor().max(1.0) as usize;
    let gutter_chars = if show_line_numbers {
        line_count.to_string().len() + 1
    } else {
        0
    };
    ViewportMetrics {
        rows,
        cols: cols.max(gutter_chars + 1),
    }
}

fn font_metrics(ui: &egui::Ui) -> (f32, f32, f32) {
    let font_id = FontId::monospace(15.0);
    let row_height = ui.text_style_height(&TextStyle::Monospace).max(18.0);
    let galley = ui
        .painter()
        .layout_no_wrap("W".to_string(), font_id.clone(), Color32::WHITE);
    let char_width = galley.size().x.max(8.0);
    (row_height, char_width, row_height)
}

fn background_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgb(245, 244, 240)
    } else {
        Color32::from_rgb(18, 22, 28)
    }
}

fn text_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgb(28, 30, 34)
    } else {
        Color32::from_rgb(230, 235, 240)
    }
}

fn gutter_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgb(110, 116, 128)
    } else {
        Color32::from_rgb(120, 132, 148)
    }
}

fn cursor_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgb(30, 95, 180)
    } else {
        Color32::from_rgb(88, 166, 255)
    }
}

fn current_line_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgba_unmultiplied(40, 86, 140, 24)
    } else {
        Color32::from_rgba_unmultiplied(88, 166, 255, 26)
    }
}

fn search_bg_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgba_unmultiplied(220, 170, 35, 72)
    } else {
        Color32::from_rgba_unmultiplied(255, 198, 88, 72)
    }
}

fn selection_bg_color(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgba_unmultiplied(66, 133, 244, 100)
    } else {
        Color32::from_rgba_unmultiplied(66, 133, 244, 120)
    }
}

fn color_for_span(color: Option<crossterm::style::Color>, theme: &str) -> Color32 {
    use crossterm::style::Color as T;
    match color {
        Some(T::Black) => Color32::from_rgb(0, 0, 0),
        Some(T::DarkGrey) => Color32::from_rgb(110, 120, 130),
        Some(T::Red) => Color32::from_rgb(220, 80, 80),
        Some(T::DarkRed) => Color32::from_rgb(160, 60, 60),
        Some(T::Green) => Color32::from_rgb(90, 180, 110),
        Some(T::DarkGreen) => Color32::from_rgb(60, 150, 95),
        Some(T::Yellow) => Color32::from_rgb(230, 190, 70),
        Some(T::DarkYellow) => Color32::from_rgb(205, 165, 55),
        Some(T::Blue) => Color32::from_rgb(100, 160, 240),
        Some(T::DarkBlue) => Color32::from_rgb(80, 125, 205),
        Some(T::Magenta) => Color32::from_rgb(200, 120, 230),
        Some(T::DarkMagenta) => Color32::from_rgb(155, 100, 195),
        Some(T::Cyan) => Color32::from_rgb(90, 210, 210),
        Some(T::DarkCyan) => Color32::from_rgb(80, 170, 180),
        Some(T::White) => text_color(theme),
        Some(T::Grey) => Color32::from_rgb(160, 170, 180),
        Some(T::AnsiValue(v)) => Color32::from_gray(v),
        Some(T::Rgb { r, g, b }) => Color32::from_rgb(r, g, b),
        None => text_color(theme),
        _ => text_color(theme),
    }
}

fn line_layout_job(
    line: &str,
    ext: Option<&str>,
    theme: &str,
    font_id: FontId,
    default_color: Color32,
    search_bg: Color32,
    search_match: Option<(usize, usize)>,
    line_start: usize,
    col_offset: usize,
    max_chars: usize,
    in_block_comment: bool,
    soft_wrap: bool,
    available_width: f32,
) -> (LayoutJob, bool) {
    let (spans, next_block_comment) = highlight::highlight_line(line, ext, in_block_comment, theme);
    let mut job = LayoutJob::default();
    if soft_wrap {
        job.wrap.max_width = available_width;
    } else {
        job.wrap.max_width = f32::INFINITY;
    }
    job.break_on_newline = false;

    let mut run = String::new();
    let mut run_color = default_color;
    let mut run_bg = Color32::TRANSPARENT;
    let mut line_index = 0usize;
    let mut visible_count = 0usize;

    let flush = |job: &mut LayoutJob, run: &mut String, color: Color32, bg: Color32| {
        if run.is_empty() {
            return;
        }
        job.append(
            run,
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color,
                background: bg,
                ..Default::default()
            },
        );
        run.clear();
    };

    for span in spans {
        let span_color = color_for_span(span.color, theme);
        for ch in span.text.chars() {
            if line_index < col_offset {
                line_index += 1;
                continue;
            }
            if visible_count >= max_chars {
                flush(&mut job, &mut run, run_color, run_bg);
                return (job, next_block_comment);
            }

            let in_match = search_match
                .map(|(start, len)| {
                    let abs = line_start + line_index;
                    abs >= start && abs < start + len
                })
                .unwrap_or(false);
            let bg = if in_match {
                search_bg
            } else {
                Color32::TRANSPARENT
            };

            if run.is_empty() {
                run_color = span_color;
                run_bg = bg;
                run.push(ch);
            } else if run_color == span_color && run_bg == bg {
                run.push(ch);
            } else {
                flush(&mut job, &mut run, run_color, run_bg);
                run_color = span_color;
                run_bg = bg;
                run.push(ch);
            }
            line_index += 1;
            visible_count += 1;
        }
    }

    flush(&mut job, &mut run, run_color, run_bg);
    (job, next_block_comment)
}

fn draw_cursor(
    painter: &egui::Painter,
    text_origin: egui::Pos2,
    galley: &egui::Galley,
    cursor_col: usize,
    col_offset: usize,
    max_visible_cols: usize,
    color: Color32,
) {
    if cursor_col < col_offset {
        return;
    }

    let visible_col = cursor_col - col_offset;
    if visible_col > max_visible_cols {
        return;
    }

    let cursor = CCursor::new(visible_col);
    let cursor_rect = galley.pos_from_cursor(cursor);
    let cursor_rect = egui::Rect::from_min_size(
        egui::pos2(
            text_origin.x + cursor_rect.min.x,
            text_origin.y + cursor_rect.min.y,
        ),
        Vec2::new(2.0, cursor_rect.height()),
    );
    painter.rect_filled(cursor_rect, 1.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_mapping_handles_save_variants() {
        let mut mods = egui::Modifiers::default();
        mods.ctrl = true;
        assert_eq!(map_shortcut(Key::S, mods), Some(Command::Save));

        mods.shift = true;
        assert_eq!(map_shortcut(Key::S, mods), Some(Command::SaveAs));

        assert_eq!(map_shortcut(Key::F, mods), Some(Command::Find));
    }

    #[test]
    fn selection_movement_maps_arrow_and_home_end() {
        assert_eq!(
            selection_movement_command(Key::ArrowLeft),
            Some(Command::MoveLeft)
        );
        assert_eq!(
            selection_movement_command(Key::End),
            Some(Command::MoveLineEnd)
        );
        assert_eq!(selection_movement_command(Key::A), None);
    }

    #[test]
    fn viewport_metrics_never_zero() {
        let viewport = viewport_from_size(Vec2::splat(0.0), 12.0, 18.0, true, 999);
        assert!(viewport.rows >= 1);
        assert!(viewport.cols >= 1);
    }

    #[test]
    fn line_layout_respects_horizontal_scroll() {
        let (job, _) = line_layout_job(
            "abcdef",
            None,
            "dark",
            FontId::monospace(14.0),
            Color32::WHITE,
            Color32::YELLOW,
            None,
            0,
            2,
            3,
            false,
            false,
            f32::INFINITY,
        );
        assert_eq!(job.text, "cde");
    }

    #[test]
    fn line_layout_carries_block_comment_state() {
        let (_, next_block_comment) = line_layout_job(
            "/* block",
            Some("rs"),
            "dark",
            FontId::monospace(14.0),
            Color32::WHITE,
            Color32::YELLOW,
            None,
            0,
            0,
            80,
            false,
            false,
            f32::INFINITY,
        );
        assert!(next_block_comment);
    }

    #[test]
    fn shortcut_mapping_handles_clipboard_commands() {
        let mut mods = egui::Modifiers::default();
        mods.ctrl = true;

        // Test Copy (Cmd/Ctrl+C)
        assert_eq!(map_shortcut(Key::C, mods), Some(Command::Copy));

        // Test Paste (Cmd/Ctrl+V)
        assert_eq!(map_shortcut(Key::V, mods), Some(Command::Paste));

        // Test Cut (Cmd/Ctrl+X)
        assert_eq!(map_shortcut(Key::X, mods), Some(Command::Cut));

        // Test Select All (Cmd/Ctrl+A)
        assert_eq!(map_shortcut(Key::A, mods), Some(Command::SelectAll));
    }

    #[test]
    fn shortcut_mapping_command_key_works_for_clipboard() {
        // Test that the command key (Cmd on macOS, Ctrl on Linux/Windows) works
        let mut mods = egui::Modifiers::default();
        mods.command = true;

        assert_eq!(map_shortcut(Key::C, mods), Some(Command::Copy));
        assert_eq!(map_shortcut(Key::V, mods), Some(Command::Paste));
        assert_eq!(map_shortcut(Key::X, mods), Some(Command::Cut));
        assert_eq!(map_shortcut(Key::A, mods), Some(Command::SelectAll));
    }
}
