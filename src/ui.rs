// Screen rendering: viewport, status bar, line numbers, and prompts.
//
// Translates the current editor state into terminal draw commands.
// Owns no state of its own beyond the current viewport dimensions.

use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
    QueueableCommand,
};

use crate::buffer::Buffer;
use crate::highlight;

/// All per-frame state needed to render the editor.
pub struct RenderState<'a> {
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub col_offset: usize,
    pub message: Option<&'a str>,
    /// (char_offset, match_len) of the current search match, if any.
    pub search_match: Option<(usize, usize)>,
    pub file_ext: Option<&'a str>,
}

/// Manages rendering the editor UI to the terminal.
pub struct Ui {
    /// Terminal width in columns.
    pub width: usize,
    /// Terminal height in rows.
    pub height: usize,
    /// Whether to show the line-number gutter.
    pub show_line_numbers: bool,
    /// Colour theme: `"dark"` or `"light"`.
    pub theme: String,
}

impl Ui {
    /// Create a new `Ui` sized to the given terminal dimensions.
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, show_line_numbers: true, theme: "dark".to_string() }
    }

    /// Number of rows available for text content.
    ///
    /// Two rows are reserved: one for the status bar and one for the message bar.
    pub fn viewport_rows(&self) -> usize {
        self.height.saturating_sub(2)
    }

    /// Width of the line-number gutter (digits + 1 space separator), or 0 if hidden.
    pub fn gutter_width(&self, line_count: usize) -> usize {
        if !self.show_line_numbers {
            return 0;
        }
        let digits = line_count.to_string().len();
        digits + 1 // right-align digits + space separator
    }

    /// Number of text columns visible after the gutter.
    pub fn text_area_width(&self, line_count: usize) -> usize {
        self.width.saturating_sub(self.gutter_width(line_count))
    }

    /// Render the complete editor screen.
    ///
    /// - Text lines from the buffer (or `~` beyond EOF) fill the viewport.
    /// - The status bar is drawn on the second-to-last row in inverse video.
    /// - The message bar is drawn on the last row.
    /// - The terminal cursor is repositioned to the editor cursor after drawing.
    pub fn render(&self, buffer: &Buffer, state: &RenderState<'_>) -> Result<()> {
        let RenderState {
            cursor_row,
            cursor_col,
            scroll_offset,
            col_offset,
            message,
            search_match,
            file_ext,
        } = *state;
        let mut stdout = io::stdout();

        stdout.queue(cursor::Hide)?;
        stdout.queue(cursor::MoveTo(0, 0))?;

        let rows = self.viewport_rows();
        let line_count = buffer.line_count();
        let gutter = self.gutter_width(line_count);
        let text_width = self.text_area_width(line_count);

        // Track block-comment state across lines for syntax highlighting.
        let mut in_block_comment = false;

        for screen_row in 0..rows {
            let file_row = screen_row + scroll_offset;

            stdout.queue(Clear(ClearType::CurrentLine))?;

            if file_row < line_count {
                // Render the line-number gutter.
                if gutter > 0 {
                    let num = format!("{:>width$} ", file_row + 1, width = gutter - 1);
                    stdout.queue(SetForegroundColor(Color::DarkGrey))?;
                    stdout.queue(Print(&num))?;
                    stdout.queue(ResetColor)?;
                }

                let line = buffer.line(file_row);
                let (spans, next_block) = highlight::highlight_line(&line, file_ext, in_block_comment, &self.theme);
                in_block_comment = next_block;

                // Compute per-char absolute offsets for search highlighting.
                // We need to know the char offset of each displayed char.
                let line_char_start = buffer.char_offset_for(file_row, 0);

                // Collect all chars with their absolute offset, then slice by col_offset.
                let mut char_pos = 0usize; // position within the line (0-indexed)
                let mut printed = 0usize;

                for span in &spans {
                    for ch in span.text.chars() {
                        if char_pos < col_offset {
                            char_pos += 1;
                            continue;
                        }
                        if printed >= text_width {
                            break;
                        }

                        let abs_offset = line_char_start + char_pos;
                        let is_match = search_match
                            .map(|(m_start, m_len)| abs_offset >= m_start && abs_offset < m_start + m_len)
                            .unwrap_or(false);

                        if is_match {
                            stdout.queue(SetAttribute(Attribute::Reverse))?;
                        } else if let Some(color) = span.color {
                            stdout.queue(SetForegroundColor(color))?;
                        }

                        write!(stdout, "{}", ch)?;

                        if is_match {
                            stdout.queue(SetAttribute(Attribute::Reset))?;
                        } else if span.color.is_some() {
                            stdout.queue(ResetColor)?;
                        }

                        char_pos += 1;
                        printed += 1;
                    }
                    if printed >= text_width {
                        break;
                    }
                }
            } else {
                // Lines beyond the file content show a tilde, vim-style.
                if gutter > 0 {
                    // Blank gutter for tilde lines.
                    write!(stdout, "{:width$}", "", width = gutter)?;
                }
                stdout.queue(SetForegroundColor(Color::DarkBlue))?;
                write!(stdout, "~")?;
                stdout.queue(ResetColor)?;
            }

            write!(stdout, "\r\n")?;
        }

        self.render_status_bar(&mut stdout, buffer, cursor_row, cursor_col)?;
        self.render_message_bar(&mut stdout, message)?;

        // Place the visible cursor at the editor cursor position.
        let screen_row = cursor_row.saturating_sub(scroll_offset);
        let screen_col = gutter + cursor_col.saturating_sub(col_offset);
        stdout.queue(cursor::MoveTo(screen_col as u16, screen_row as u16))?;
        stdout.queue(cursor::Show)?;

        stdout.flush()?;
        Ok(())
    }

    /// Draw the status bar on the current terminal row using inverse video.
    fn render_status_bar(
        &self,
        stdout: &mut impl Write,
        buffer: &Buffer,
        cursor_row: usize,
        cursor_col: usize,
    ) -> Result<()> {
        stdout.queue(SetAttribute(Attribute::Reverse))?;
        stdout.queue(Clear(ClearType::CurrentLine))?;

        let filename = buffer.display_name();
        let dirty_flag = if buffer.is_dirty() { " | [modified]" } else { "" };
        let status = format!(
            "{} | Ln {}, Col {}{}",
            filename,
            cursor_row + 1,
            cursor_col + 1,
            dirty_flag,
        );

        let padded = format!("{:<width$}", status, width = self.width);
        let display: String = padded.chars().take(self.width).collect();
        write!(stdout, "{}", display)?;

        stdout.queue(SetAttribute(Attribute::Reset))?;
        write!(stdout, "\r\n")?;
        Ok(())
    }

    /// Draw the message bar on the current terminal row.
    fn render_message_bar(&self, stdout: &mut impl Write, message: Option<&str>) -> Result<()> {
        stdout.queue(Clear(ClearType::CurrentLine))?;
        if let Some(msg) = message {
            let truncated: String = msg.chars().take(self.width).collect();
            write!(stdout, "{}", truncated)?;
        }
        Ok(())
    }
}
