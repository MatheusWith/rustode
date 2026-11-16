//! UI-agnostic text buffer, selections, edits and undo. No I/O.

use ropey::Rope;

/// The text of one document, stored as a rope so large files edit cheaply.
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

    /// Length in Unicode scalar values.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Number of lines; an empty buffer has one line.
    #[must_use]
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
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
        assert_eq!(Buffer::default().len_lines(), 1);
    }
}
