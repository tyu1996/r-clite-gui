// Text buffer backed by a rope data structure (ropey::Rope).
//
// Provides insert, delete, undo/redo, and file I/O operations while
// correctly handling multi-byte UTF-8 text.

use std::{fs, path::PathBuf};

use anyhow::Result;
use ropey::Rope;

/// A text buffer backed by a [`ropey::Rope`].
///
/// Tracks whether the current content differs from what is stored on disk via
/// the *dirty flag*.  All text operations go through this type so that the
/// flag is maintained correctly.
pub struct Buffer {
    pub rope: Rope,
    /// File path associated with this buffer, if any.
    pub path: Option<PathBuf>,
    dirty: bool,
}

impl Buffer {
    /// Create an empty, unnamed buffer.
    pub fn new_empty() -> Self {
        Self {
            rope: Rope::new(),
            path: None,
            dirty: false,
        }
    }

    /// Open a file from disk into a new buffer.
    ///
    /// If `path` does not exist on disk, returns an empty buffer with that
    /// path pre-associated (the file is created on disk only when the user
    /// saves).
    pub fn open(path: PathBuf) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            Ok(Self {
                rope: Rope::from_str(&content),
                path: Some(path),
                dirty: false,
            })
        } else {
            // New file: buffer is empty and not dirty yet — the file does not
            // exist until the user saves for the first time.
            Ok(Self {
                rope: Rope::new(),
                path: Some(path),
                dirty: false,
            })
        }
    }

    /// Number of lines in the buffer.
    ///
    /// An empty buffer always reports 1 (one empty line where the cursor lives).
    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    /// Return the text of line `idx` (0-indexed), **without** a trailing newline.
    ///
    /// Returns an empty string if `idx` is out of range.
    pub fn line(&self, idx: usize) -> String {
        let total = self.rope.len_lines();
        if idx >= total {
            return String::new();
        }
        let slice = self.rope.line(idx);
        let s: String = slice.chars().collect();
        // Strip the trailing line ending (LF or CRLF).
        s.trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string()
    }

    /// Length of line `idx` in Unicode scalar values (chars), excluding the
    /// trailing newline.
    pub fn line_len(&self, idx: usize) -> usize {
        self.line(idx).chars().count()
    }

    /// Returns `true` if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// A human-readable name for the buffer.
    ///
    /// Shows the file name component of the path, or `[No Name]` for unnamed
    /// buffers.
    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => "[No Name]".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_has_one_line() {
        let buf = Buffer::new_empty();
        assert_eq!(buf.line_count(), 1);
    }

    #[test]
    fn empty_buffer_line_is_empty_string() {
        let buf = Buffer::new_empty();
        assert_eq!(buf.line(0), "");
        assert_eq!(buf.line_len(0), 0);
    }

    #[test]
    fn empty_buffer_is_not_dirty() {
        let buf = Buffer::new_empty();
        assert!(!buf.is_dirty());
    }

    #[test]
    fn no_name_buffer_display() {
        let buf = Buffer::new_empty();
        assert_eq!(buf.display_name(), "[No Name]");
    }

    #[test]
    fn named_buffer_display() {
        let buf = Buffer {
            rope: Rope::new(),
            path: Some(PathBuf::from("/tmp/hello.txt")),
            dirty: false,
        };
        assert_eq!(buf.display_name(), "hello.txt");
    }

    #[test]
    fn line_count_multiline() {
        let buf = Buffer {
            rope: Rope::from_str("line1\nline2\nline3"),
            path: None,
            dirty: false,
        };
        assert_eq!(buf.line_count(), 3);
    }

    #[test]
    fn line_content_no_trailing_newline() {
        let buf = Buffer {
            rope: Rope::from_str("hello\nworld\n"),
            path: None,
            dirty: false,
        };
        assert_eq!(buf.line(0), "hello");
        assert_eq!(buf.line(1), "world");
    }

    #[test]
    fn line_len_multibyte() {
        let buf = Buffer {
            rope: Rope::from_str("héllo\nwörld"),
            path: None,
            dirty: false,
        };
        // 'é' is one char even though it is two UTF-8 bytes.
        assert_eq!(buf.line_len(0), 5);
        assert_eq!(buf.line_len(1), 5);
    }

    #[test]
    fn line_out_of_range_returns_empty() {
        let buf = Buffer::new_empty();
        assert_eq!(buf.line(99), "");
        assert_eq!(buf.line_len(99), 0);
    }
}
