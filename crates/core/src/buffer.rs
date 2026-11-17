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

    /// The line holding char `offset` (zero-based). `offset` may equal the length.
    #[must_use]
    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.char_to_line(offset)
    }

    /// The char offset where `line` starts. `line` must exist.
    #[must_use]
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_char(line)
    }

    /// The number of chars in `line`, without its line break. `line` must exist.
    #[must_use]
    pub fn line_len(&self, line: usize) -> usize {
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
        }
        if len > 0 && slice.char(len - 1) == '\r' {
            len -= 1;
        }
        len
    }

    /// The char offset of `line`, `column`, clamping the column to the line.
    #[must_use]
    pub fn offset_of(&self, line: usize, column: usize) -> usize {
        self.line_start(line) + column.min(self.line_len(line))
    }

    /// The text of `line` without its line break. `line` must exist.
    #[must_use]
    pub fn line_text(&self, line: usize) -> String {
        self.rope
            .line(line)
            .slice(..self.line_len(line))
            .to_string()
    }

    /// The first `column` chars of `line`. Both must exist.
    #[must_use]
    pub fn line_prefix(&self, line: usize, column: usize) -> String {
        self.rope.line(line).slice(..column).to_string()
    }

    /// The number of spaces and tabs that start `line`. `line` must exist.
    #[must_use]
    pub fn leading_whitespace(&self, line: usize) -> usize {
        self.rope
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count()
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
        assert_eq!(buffer.line_len(0), 1);
        assert_eq!(buffer.line_start(1), 3);
        assert_eq!(buffer.line_of(3), 1);
        assert_eq!(buffer.offset_of(1, 9), 4);
        assert_eq!(buffer.line_text(0), "a");
        let indented = Buffer::from_text(" \tx\ny");
        assert_eq!(indented.leading_whitespace(0), 2);
        assert_eq!(indented.line_prefix(0, 2), " \t");
        assert_eq!(
            Buffer::from_text("a\u{2028}b").len_lines(),
            1,
            "only CR/LF/CRLF"
        );
    }
}
