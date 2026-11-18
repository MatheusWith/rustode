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

    /// Length in bytes of the UTF-8 text.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// The char holding byte `byte` (the length at the end).
    #[must_use]
    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.rope.byte_to_char(byte.min(self.len_bytes()))
    }

    /// The first byte of char `offset` (the length in bytes at the end).
    #[must_use]
    pub fn char_to_byte(&self, offset: usize) -> usize {
        self.rope.char_to_byte(offset.min(self.len_chars()))
    }

    /// The text from byte `byte` to the end of the rope chunk holding it,
    /// empty at the end: a parser reads the text chunk by chunk without
    /// copying it. `byte` must be on a char boundary.
    #[must_use]
    pub fn chunk_at_byte(&self, byte: usize) -> &str {
        if byte >= self.len_bytes() {
            return "";
        }
        let (chunk, start, _, _) = self.rope.chunk_at_byte(byte);
        &chunk[byte - start..]
    }

    /// The chunks of the text between bytes `start` and `end`, which must be
    /// on char boundaries.
    pub fn byte_chunks(&self, start: usize, end: usize) -> impl Iterator<Item = &str> {
        let end = end.min(self.len_bytes());
        self.rope.byte_slice(start.min(end)..end).chunks()
    }

    /// Byte `byte` with its line and byte column.
    fn byte_position(&self, byte: usize) -> BytePosition {
        let line = self.rope.byte_to_line(byte);
        BytePosition {
            offset: byte,
            line,
            column: byte - self.rope.line_to_byte(line),
        }
    }

    /// Replaces `range` with `text`; returns the change in bytes.
    pub(crate) fn splice(&mut self, range: Range, text: &str) -> ByteEdit {
        let start = self.byte_position(self.rope.char_to_byte(range.start));
        let old_end = self.byte_position(self.rope.char_to_byte(range.end));
        self.rope.remove(range.start..range.end);
        self.rope.insert(range.start, text);
        ByteEdit {
            start,
            old_end,
            new_end: self.byte_position(start.offset + text.len()),
        }
    }
}

/// A byte offset in the text, with its line and its column in bytes. Lines
/// are counted as the buffer counts them, ending at CR, LF or CRLF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BytePosition {
    /// Bytes from the start of the text.
    pub offset: usize,
    /// The line, zero-based.
    pub line: usize,
    /// Bytes from the start of the line.
    pub column: usize,
}

/// One change to the text in bytes, as an incremental parser wants it: the
/// text from `start` to `old_end` was replaced by the text from `start` to
/// `new_end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteEdit {
    /// Where the change starts, in the old and the new text alike.
    pub start: BytePosition,
    /// Where the replaced text ended, in the old text.
    pub old_end: BytePosition,
    /// Where the inserted text ends, in the new text.
    pub new_end: BytePosition,
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

    #[test]
    fn splice_reports_the_change_in_bytes_lines_and_byte_columns() {
        let mut buffer = Buffer::from_text("é\nab\r\ncd");
        assert_eq!((buffer.len_bytes(), buffer.char_to_byte(2)), (9, 3));
        assert_eq!(buffer.byte_to_char(3), 2);
        let edit = buffer.splice(Range::new(3, 6), "x\ny");
        assert_eq!(buffer.text(), "é\nax\nycd");
        let at = |offset, line, column| BytePosition {
            offset,
            line,
            column,
        };
        assert_eq!(edit.start, at(4, 1, 1));
        assert_eq!(edit.old_end, at(7, 2, 0), "after the CRLF");
        assert_eq!(edit.new_end, at(7, 2, 1));
    }

    #[test]
    fn chunks_cover_the_text_without_copying_it() {
        let text = "fn é() {}\n".repeat(500);
        let buffer = Buffer::from_text(&text);
        let mut read = String::new();
        while read.len() < buffer.len_bytes() {
            read.push_str(buffer.chunk_at_byte(read.len()));
        }
        assert_eq!(read, text);
        assert_eq!(buffer.chunk_at_byte(buffer.len_bytes()), "");
        let middle: String = buffer.byte_chunks(3, 11).collect();
        assert_eq!(middle, &text[3..11]);
    }
}
