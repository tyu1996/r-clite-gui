// Screen rendering: viewport, status bar, line numbers, and prompts.
//
// Translates the current editor state into terminal draw commands.
// Owns no state of its own beyond the current viewport dimensions.

use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor,
    style::{Attribute, SetAttribute},
    terminal::{Clear, ClearType},
    QueueableCommand,
};

use crate::buffer::Buffer;

/// Manages rendering the editor UI to the terminal.
///
/// The `Ui` struct is stateless beyond the terminal dimensions; all editor
/// state is passed in at render time.  Width and height are updated each frame
/// so that the editor adapts to terminal resize events.
pub struct Ui {
    /// Terminal width in columns.
    pub width: usize,
    /// Terminal height in rows.
    pub height: usize,
}

impl Ui {
    /// Create a new `Ui` sized to the given terminal dimensions.
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    /// Number of rows available for text content.
    ///
    /// Two rows are reserved: one for the status bar and one for the message
    /// bar.
    pub fn viewport_rows(&self) -> usize {
        self.height.saturating_sub(2)
    }

    /// Render the complete editor screen.
    ///
    /// - Text lines from the buffer (or `~` beyond EOF) fill the viewport.
    /// - The status bar is drawn on the second-to-last row in inverse video.
    /// - The message bar is drawn on the last row.
    /// - The terminal cursor is repositioned to the editor cursor after drawing.
    pub fn render(
        &self,
        buffer: &Buffer,
        cursor_row: usize,
        cursor_col: usize,
        scroll_offset: usize,
        message: Option<&str>,
    ) -> Result<()> {
        let mut stdout = io::stdout();

        // Hide the cursor while we redraw to avoid visible flickering.
        stdout.queue(cursor::Hide)?;
        stdout.queue(cursor::MoveTo(0, 0))?;

        let rows = self.viewport_rows();

        for screen_row in 0..rows {
            let file_row = screen_row + scroll_offset;

            // Clear any leftover content from the previous frame.
            stdout.queue(Clear(ClearType::CurrentLine))?;

            if file_row < buffer.line_count() {
                let line = buffer.line(file_row);
                // Truncate to viewport width (horizontal scroll comes in M3).
                let truncated: String = line.chars().take(self.width).collect();
                write!(stdout, "{}", truncated)?;
            } else {
                // Lines beyond the file content show a tilde, vim-style.
                write!(stdout, "~")?;
            }

            // \r\n moves to column 0 of the next row (required in raw mode).
            write!(stdout, "\r\n")?;
        }

        self.render_status_bar(&mut stdout, buffer, cursor_row, cursor_col)?;
        self.render_message_bar(&mut stdout, message)?;

        // Place the visible cursor at the editor cursor position.
        let screen_row = cursor_row.saturating_sub(scroll_offset);
        stdout.queue(cursor::MoveTo(cursor_col as u16, screen_row as u16))?;
        stdout.queue(cursor::Show)?;

        stdout.flush()?;
        Ok(())
    }

    /// Draw the status bar on the current terminal row using inverse video.
    ///
    /// Format: `<filename> | Ln <line>, Col <col> | [modified]`
    ///
    /// The dirty flag section is omitted when the buffer is clean.
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

        // Pad or truncate to exactly `width` columns so inverse video fills the row.
        let padded = format!("{:<width$}", status, width = self.width);
        let display: String = padded.chars().take(self.width).collect();
        write!(stdout, "{}", display)?;

        stdout.queue(SetAttribute(Attribute::Reset))?;
        write!(stdout, "\r\n")?;
        Ok(())
    }

    /// Draw the message bar on the current terminal row.
    ///
    /// Shows `message` if provided, otherwise leaves the row blank.
    fn render_message_bar(&self, stdout: &mut impl Write, message: Option<&str>) -> Result<()> {
        stdout.queue(Clear(ClearType::CurrentLine))?;
        if let Some(msg) = message {
            let truncated: String = msg.chars().take(self.width).collect();
            write!(stdout, "{}", truncated)?;
        }
        Ok(())
    }
}
