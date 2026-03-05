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
    /// Create an unnamed buffer pre-populated with `content`.
    pub fn from_content(content: String) -> Self {
        Self {
            rope: Rope::from_str(&content),
            path: None,
            dirty: false,
        }
    }

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

    /// Clear the dirty flag without writing to disk.
    ///
    /// Used by the undo system to mark the buffer as clean when the content
    /// has been restored to the last-saved state.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
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

    /// Convert a (row, col) cursor position to a rope char offset.
    pub fn char_offset_for(&self, row: usize, col: usize) -> usize {
        self.char_offset(row, col)
    }

    /// Return the char at the given rope char offset, or `None` if out of bounds.
    pub fn char_at_offset(&self, pos: usize) -> Option<char> {
        if pos < self.rope.len_chars() {
            Some(self.rope.char(pos))
        } else {
            None
        }
    }

    /// Insert `text` at the given rope char offset. Marks dirty.
    ///
    /// Used by the undo/redo system to replay or reverse operations by offset.
    pub fn raw_insert(&mut self, pos: usize, text: &str) {
        self.rope.insert(pos, text);
        self.dirty = true;
    }

    /// Delete `len` chars starting at the given rope char offset. Marks dirty.
    ///
    /// Used by the undo/redo system to replay or reverse operations by offset.
    pub fn raw_delete(&mut self, pos: usize, len: usize) {
        if len > 0 && pos + len <= self.rope.len_chars() {
            self.rope.remove(pos..pos + len);
            self.dirty = true;
        }
    }

    /// Convert a rope char offset back to `(row, col)`.
    pub fn offset_to_row_col(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.rope.len_chars());
        let row = self.rope.char_to_line(pos);
        let line_start = self.rope.line_to_char(row);
        (row, pos - line_start)
    }

    /// Search for `query` (case-insensitive) starting at char offset `from`.
    ///
    /// Returns the char offset of the start of the next match, wrapping around
    /// the end of the document.  Returns `None` if there are no matches.
    pub fn find_next(&self, query: &str, from: usize) -> Option<usize> {
        if query.is_empty() {
            return None;
        }
        let text: String = self.rope.chars().collect();
        let low_text = text.to_lowercase();
        let low_query = query.to_lowercase();
        let query_len = low_query.chars().count();
        let doc_len = low_text.chars().count();
        if query_len > doc_len {
            return None;
        }
        // Build a char-indexed view to search efficiently.
        let chars: Vec<char> = low_text.chars().collect();
        let qchars: Vec<char> = low_query.chars().collect();
        // Search forward from `from` (wrapping).
        for offset in 0..doc_len {
            let pos = (from + offset) % doc_len;
            if pos + query_len > doc_len {
                continue;
            }
            if chars[pos..pos + query_len] == qchars[..] {
                return Some(pos);
            }
        }
        None
    }

    /// Search backward for `query` starting just before char offset `from`.
    pub fn find_prev(&self, query: &str, from: usize) -> Option<usize> {
        if query.is_empty() {
            return None;
        }
        let text: String = self.rope.chars().collect();
        let low_text = text.to_lowercase();
        let low_query = query.to_lowercase();
        let query_len = low_query.chars().count();
        let doc_len = low_text.chars().count();
        if query_len > doc_len {
            return None;
        }
        let chars: Vec<char> = low_text.chars().collect();
        let qchars: Vec<char> = low_query.chars().collect();
        // Search backward from `from - 1` (wrapping).
        for offset in 1..=doc_len {
            let pos = (from + doc_len - offset) % doc_len;
            if pos + query_len > doc_len {
                continue;
            }
            if chars[pos..pos + query_len] == qchars[..] {
                return Some(pos);
            }
        }
        None
    }

    /// Convert a (row, col) cursor position to a rope char offset.
    fn char_offset(&self, row: usize, col: usize) -> usize {
        if self.rope.len_chars() == 0 {
            return 0;
        }
        let line_start = self.rope.line_to_char(row.min(self.rope.len_lines().saturating_sub(1)));
        line_start + col
    }

    /// Insert a single character at (row, col). Marks the buffer dirty.
    pub fn insert_char(&mut self, row: usize, col: usize, ch: char) {
        let offset = self.char_offset(row, col);
        self.rope.insert_char(offset, ch);
        self.dirty = true;
    }

    /// Insert a string at (row, col). Marks the buffer dirty.
    pub fn insert_str(&mut self, row: usize, col: usize, s: &str) {
        let offset = self.char_offset(row, col);
        self.rope.insert(offset, s);
        self.dirty = true;
    }

    /// Insert a newline at (row, col), splitting the line. Marks dirty.
    pub fn insert_newline(&mut self, row: usize, col: usize) {
        let offset = self.char_offset(row, col);
        self.rope.insert_char(offset, '\n');
        self.dirty = true;
    }

    /// Delete the character at (row, col).
    ///
    /// If col == line_len (cursor at end of line) and a next line exists,
    /// joins the next line onto the current line by removing the newline.
    /// Returns `true` if anything was deleted.
    pub fn delete_char_at(&mut self, row: usize, col: usize) -> bool {
        let line_len = self.line_len(row);
        if col < line_len {
            let offset = self.char_offset(row, col);
            self.rope.remove(offset..offset + 1);
            self.dirty = true;
            true
        } else if row + 1 < self.line_count() {
            // Join next line: remove the newline at end of current line.
            let offset = self.char_offset(row, col);
            if offset < self.rope.len_chars() {
                self.rope.remove(offset..offset + 1);
                self.dirty = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Delete the character before (row, col) — Backspace behaviour.
    ///
    /// At the beginning of a line, joins the current line with the previous
    /// line (removes the preceding newline).  At (0, 0) this is a no-op.
    /// Returns the new (row, col) after the deletion.
    pub fn delete_char_before(&mut self, row: usize, col: usize) -> (usize, usize) {
        if col > 0 {
            let offset = self.char_offset(row, col - 1);
            self.rope.remove(offset..offset + 1);
            self.dirty = true;
            (row, col - 1)
        } else if row > 0 {
            // Remove the newline at the end of the previous line to join.
            let prev_len = self.line_len(row - 1);
            let offset = self.char_offset(row - 1, prev_len);
            if offset < self.rope.len_chars() {
                self.rope.remove(offset..offset + 1);
                self.dirty = true;
            }
            (row - 1, prev_len)
        } else {
            (0, 0)
        }
    }

    /// Save the buffer to its associated file path.
    ///
    /// Returns the number of bytes written. Clears the dirty flag on success.
    pub fn save(&mut self) -> Result<usize> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No file path associated with this buffer"))?;
        self.save_to(path)
    }

    /// Write the buffer content to `path`, update the associated path, and
    /// clear the dirty flag.  Returns the number of bytes written.
    pub fn save_to(&mut self, path: PathBuf) -> Result<usize> {
        let content: String = self.rope.to_string();
        let bytes = content.len();
        fs::write(&path, &content)?;
        self.path = Some(path);
        self.dirty = false;
        Ok(bytes)
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

    // ── Mutation tests ────────────────────────────────────────────────────────

    #[test]
    fn insert_char_marks_dirty() {
        let mut buf = Buffer::new_empty();
        assert!(!buf.is_dirty());
        buf.insert_char(0, 0, 'a');
        assert!(buf.is_dirty());
    }

    #[test]
    fn insert_char_content() {
        let mut buf = Buffer::new_empty();
        buf.insert_char(0, 0, 'h');
        buf.insert_char(0, 1, 'i');
        assert_eq!(buf.line(0), "hi");
        assert_eq!(buf.line_count(), 1);
    }

    #[test]
    fn insert_char_mid_line() {
        let mut buf = Buffer {
            rope: Rope::from_str("ac"),
            path: None,
            dirty: false,
        };
        buf.insert_char(0, 1, 'b');
        assert_eq!(buf.line(0), "abc");
    }

    #[test]
    fn insert_char_multibyte() {
        let mut buf = Buffer::new_empty();
        buf.insert_char(0, 0, '€');
        buf.insert_char(0, 1, '£');
        assert_eq!(buf.line(0), "€£");
        assert_eq!(buf.line_len(0), 2);
    }

    #[test]
    fn insert_newline_splits_line() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello world"),
            path: None,
            dirty: false,
        };
        buf.insert_newline(0, 5);
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line(0), "hello");
        assert_eq!(buf.line(1), " world");
    }

    #[test]
    fn delete_char_at_mid_line() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello"),
            path: None,
            dirty: false,
        };
        buf.delete_char_at(0, 2); // remove 'l'
        assert_eq!(buf.line(0), "helo");
        assert!(buf.is_dirty());
    }

    #[test]
    fn delete_char_at_joins_lines() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello\nworld"),
            path: None,
            dirty: false,
        };
        // cursor at end of line 0 (col == line_len == 5)
        buf.delete_char_at(0, 5);
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), "helloworld");
    }

    #[test]
    fn delete_char_at_end_of_last_line_is_noop() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello"),
            path: None,
            dirty: false,
        };
        let changed = buf.delete_char_at(0, 5);
        assert!(!changed);
        assert!(!buf.is_dirty());
        assert_eq!(buf.line(0), "hello");
    }

    #[test]
    fn delete_char_before_mid_line() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello"),
            path: None,
            dirty: false,
        };
        let (r, c) = buf.delete_char_before(0, 3); // backspace 'l'
        assert_eq!((r, c), (0, 2));
        assert_eq!(buf.line(0), "helo");
    }

    #[test]
    fn delete_char_before_joins_lines() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello\nworld"),
            path: None,
            dirty: false,
        };
        let (r, c) = buf.delete_char_before(1, 0); // backspace at start of line 1
        assert_eq!((r, c), (0, 5));
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), "helloworld");
    }

    #[test]
    fn delete_char_before_at_origin_is_noop() {
        let mut buf = Buffer {
            rope: Rope::from_str("hello"),
            path: None,
            dirty: false,
        };
        let (r, c) = buf.delete_char_before(0, 0);
        assert_eq!((r, c), (0, 0));
        assert!(!buf.is_dirty());
    }

    #[test]
    fn delete_char_multibyte() {
        let mut buf = Buffer {
            rope: Rope::from_str("a€b"),
            path: None,
            dirty: false,
        };
        buf.delete_char_at(0, 1); // remove '€'
        assert_eq!(buf.line(0), "ab");
        assert_eq!(buf.line_len(0), 2);
    }

    #[test]
    fn save_to_writes_file_and_clears_dirty() {
        use std::io::Read;
        let mut buf = Buffer {
            rope: Rope::from_str("hello\n"),
            path: None,
            dirty: true,
        };
        let tmp = std::env::temp_dir().join("rcte_test_save.txt");
        let bytes = buf.save_to(tmp.clone()).unwrap();
        assert_eq!(bytes, 6);
        assert!(!buf.is_dirty());
        let mut content = String::new();
        std::fs::File::open(&tmp).unwrap().read_to_string(&mut content).unwrap();
        assert_eq!(content, "hello\n");
        std::fs::remove_file(tmp).unwrap();
    }

    #[test]
    fn dirty_flag_transitions() {
        let mut buf = Buffer::new_empty();
        assert!(!buf.is_dirty());
        buf.insert_char(0, 0, 'x');
        assert!(buf.is_dirty());
        let tmp = std::env::temp_dir().join("rcte_test_dirty.txt");
        buf.save_to(tmp.clone()).unwrap();
        assert!(!buf.is_dirty());
        std::fs::remove_file(tmp).unwrap();
    }
}
