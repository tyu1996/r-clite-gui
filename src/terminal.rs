// Low-level terminal I/O abstraction built on crossterm.
//
// Handles entering/exiting raw mode, reading input events, and
// writing to the terminal. Guarantees terminal state restoration on
// drop (even on panic).

use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

/// Guards raw mode and the alternate screen.
///
/// Entering raw mode and the alternate screen happens in [`RawModeGuard::new`].
/// The [`Drop`] implementation disables raw mode and leaves the alternate
/// screen — this runs even when the process unwinds due to a panic, so the
/// user's shell is never left in a broken state.
pub struct RawModeGuard;

impl RawModeGuard {
    /// Enable raw mode and enter the alternate screen.
    pub fn new() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = io::stdout().flush();
    }
}

/// Return the current terminal size as `(width, height)` in columns/rows.
pub fn size() -> Result<(u16, u16)> {
    Ok(terminal::size()?)
}
