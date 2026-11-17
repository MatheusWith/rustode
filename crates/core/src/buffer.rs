//! The rope-backed text of one document.

use crate::editor::Range;
use ropey::Rope;

/// The text of one document, stored as a rope so large files edit cheaply.
/// Offsets are in Unicode scalar values (chars).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buffer {
    rope: Rope,
}

impl Buffer {
    /// A buffer holding `text` exactly, line endings included.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
        }
    }

    /// The whole text. Allocates; use sparingly on large buffers.
    #[must_use]
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Length in chars.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Number of lines; an empty buffer has one line.
    #[must_use]
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// The char at `offset`, if any.
    #[must_use]
    pub fn char_at(&self, offset: usize) -> Option<char> {
        (offset < self.len_chars()).then(|| self.rope.char(offset))
    }

    /// The text in `range`. The range must be within the buffer.
    #[must_use]
    pub fn slice(&self, range: Range) -> String {
        self.rope.slice(range.start..range.end).to_string()
    }

    pub(crate) fn insert(&mut self, offset: usize, text: &str) {
        self.rope.insert(offset, text);
    }

    pub(crate) fn remove(&mut self, range: Range) {
        self.rope.remove(range.start..range.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_text_and_counts_lines() {
        let buffer = Buffer::from_text("a\r\nb\nc");
        assert_eq!(buffer.text(), "a\r\nb\nc");
        assert_eq!(buffer.len_lines(), 3);
        assert_eq!(buffer.len_chars(), 6);
        assert_eq!(buffer.char_at(1), Some('\r'));
        assert_eq!(buffer.char_at(6), None);
        assert_eq!(Buffer::default().len_lines(), 1);
    }
}
