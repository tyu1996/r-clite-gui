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
    /// Open a file into the current editor.
    Open,
    /// Undo the last edit operation.
    Undo,
    /// Redo the last undone operation.
    Redo,
    /// Open the find/search prompt.
    Find,
    /// Toggle line number display.
    ToggleLineNumbers,
    /// Copy selection to clipboard.
    Copy,
    /// Paste from clipboard at cursor.
    Paste,
    /// Cut selection to clipboard.
    Cut,
    /// Select all text in the document.
    SelectAll,
    /// Move cursor left by one word.
    MoveWordLeft,
    /// Move cursor right by one word.
    MoveWordRight,
    /// Delete from cursor back to the start of the previous word.
    DeleteWordLeft,
    /// Delete from cursor forward to the start of the next word.
    DeleteWordRight,
    /// Toggle soft word wrap on/off.
    ToggleSoftWrap,
    /// Reflow the current paragraph to the configured wrap column.
    ReflowParagraph,
    /// No-op — the key has no binding in the current context.
    None,
}

/// Translate a raw [`KeyEvent`] into an editor [`Command`].
///
/// Unknown or unbound keys map to [`Command::None`].
pub fn map(event: KeyEvent) -> Command {
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = event.modifiers.contains(KeyModifiers::ALT);

    match event.code {
        // Word navigation (Ctrl+arrow on Win/Linux; terminals report Option as ALT on macOS)
        // Must come before plain arrow keys so guards are checked first
        KeyCode::Left if ctrl || alt => Command::MoveWordLeft,
        KeyCode::Right if ctrl || alt => Command::MoveWordRight,

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

        // Word delete (must come before plain Backspace/Delete keys)
        KeyCode::Backspace if ctrl || alt => Command::DeleteWordLeft,
        KeyCode::Delete if ctrl || alt => Command::DeleteWordRight,

        // Quit
        KeyCode::Char('q') if ctrl => Command::Quit,

        // Reflow paragraph: Alt+Q (Option+Q on macOS — must come before Ctrl+Q)
        KeyCode::Char('q') if alt => Command::ReflowParagraph,

        // Save / Save As
        KeyCode::Char('s') if ctrl && event.modifiers.contains(KeyModifiers::SHIFT) => {
            Command::SaveAs
        }
        KeyCode::Char('S') if ctrl => Command::SaveAs,
        KeyCode::Char('s') if ctrl => Command::Save,
        KeyCode::Char('o') if ctrl => Command::Open,

        // Undo / Redo
        KeyCode::Char('z') if ctrl => Command::Undo,
        KeyCode::Char('y') if ctrl => Command::Redo,

        // Find
        KeyCode::Char('f') if ctrl => Command::Find,

        // Toggle line numbers
        KeyCode::Char('l') if ctrl => Command::ToggleLineNumbers,

        // Soft wrap toggle: Ctrl+Shift+W (or Option+W terminals may send ALT+W)
        KeyCode::Char('w') if ctrl && event.modifiers.contains(KeyModifiers::SHIFT) => {
            Command::ToggleSoftWrap
        }
        KeyCode::Char('W') if ctrl => Command::ToggleSoftWrap,

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
