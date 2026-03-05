// Key binding definitions and input dispatch.
//
// Maps raw crossterm::event::KeyEvent values to editor commands
// and routes them to the appropriate handler.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// High-level editor commands produced by the keymap layer.
///
/// Each variant represents a discrete action the editor can perform.  The
/// keymap layer translates raw [`KeyEvent`]s into these commands so that the
/// editor core never has to inspect key codes directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Move cursor up one line.
    MoveUp,
    /// Move cursor down one line.
    MoveDown,
    /// Move cursor left one character.
    MoveLeft,
    /// Move cursor right one character.
    MoveRight,
    /// Move cursor to the beginning of the current line.
    MoveLineStart,
    /// Move cursor to the end of the current line.
    MoveLineEnd,
    /// Move cursor up one viewport height.
    PageUp,
    /// Move cursor down one viewport height.
    PageDown,
    /// Quit the editor (with unsaved-changes guard).
    Quit,
    /// Insert a printable character at the cursor.
    InsertChar(char),
    /// Delete the character to the left of the cursor (Backspace).
    Backspace,
    /// Delete the character at the cursor (Delete / Del key).
    DeleteChar,
    /// Insert a newline and split the current line.
    InsertNewline,
    /// Insert a tab (rendered as spaces).
    InsertTab,
    /// Save the buffer to disk.
    Save,
    /// Save the buffer to a new path (Save As).
    SaveAs,
    /// Undo the last edit operation.
    Undo,
    /// Redo the last undone operation.
    Redo,
    /// Open the find/search prompt.
    Find,
    /// Toggle line number display.
    ToggleLineNumbers,
    /// No-op — the key has no binding in the current context.
    None,
}

/// Translate a raw [`KeyEvent`] into an editor [`Command`].
///
/// Unknown or unbound keys map to [`Command::None`].
pub fn map(event: KeyEvent) -> Command {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        // Arrow keys
        KeyCode::Up => Command::MoveUp,
        KeyCode::Down => Command::MoveDown,
        KeyCode::Left => Command::MoveLeft,
        KeyCode::Right => Command::MoveRight,

        // Emacs-style navigation aliases
        KeyCode::Char('p') if ctrl => Command::MoveUp,
        KeyCode::Char('n') if ctrl => Command::MoveDown,
        KeyCode::Char('b') if ctrl => Command::MoveLeft,
        KeyCode::Char('f') if ctrl => Command::MoveRight,

        // Line navigation
        KeyCode::Home => Command::MoveLineStart,
        KeyCode::End => Command::MoveLineEnd,

        // Page navigation
        KeyCode::PageUp => Command::PageUp,
        KeyCode::PageDown => Command::PageDown,

        // Quit
        KeyCode::Char('q') if ctrl => Command::Quit,

        // Save / Save As
        // Some terminals report Ctrl+Shift+S as Char('s') with CONTROL|SHIFT;
        // others report it as Char('S') with CONTROL only (shift absorbed into
        // the uppercase letter).  Handle both so Save As works everywhere.
        KeyCode::Char('s') if ctrl && event.modifiers.contains(KeyModifiers::SHIFT) => {
            Command::SaveAs
        }
        KeyCode::Char('S') if ctrl => Command::SaveAs,
        KeyCode::Char('s') if ctrl => Command::Save,

        // Undo / Redo
        KeyCode::Char('z') if ctrl => Command::Undo,
        KeyCode::Char('y') if ctrl => Command::Redo,

        // Find
        KeyCode::Char('f') if ctrl => Command::Find,

        // Toggle line numbers
        KeyCode::Char('l') if ctrl => Command::ToggleLineNumbers,

        // Editing
        KeyCode::Enter => Command::InsertNewline,
        KeyCode::Tab => Command::InsertTab,
        KeyCode::Backspace => Command::Backspace,
        KeyCode::Delete => Command::DeleteChar,

        // Printable characters (no modifier, or shift only for uppercase/symbols)
        KeyCode::Char(ch) if !ctrl => Command::InsertChar(ch),

        _ => Command::None,
    }
}
