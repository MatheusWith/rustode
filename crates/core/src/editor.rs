//! Editing one buffer: a cursor, edits, and undo/redo grouped like VS Code.

use crate::buffer::Buffer;
use crate::history::{Change, EditKind, History, Step};
use std::fmt;

/// A half-open range of char offsets, `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    /// First char in the range.
    pub start: usize,
    /// One past the last char in the range.
    pub end: usize,
}

impl Range {
    /// The range `start..end`; the bounds are swapped if given in reverse.
    #[must_use]
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    /// An empty range at `offset`.
    #[must_use]
    pub fn empty(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Number of chars in the range.
    #[must_use]
    pub fn len(self) -> usize {
        self.end - self.start
    }

    /// Whether the range holds no chars.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// An offset or range goes past the end of the buffer.
    OutOfRange {
        /// The offending offset.
        offset: usize,
        /// The buffer length in chars.
        len: usize,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { offset, len } => {
                write!(
                    f,
                    "offset {offset} is past the end of the buffer ({len} chars)"
                )
            }
        }
    }
}

impl std::error::Error for EditError {}

/// A buffer with one cursor and an undo history.
///
/// Every state of the text has a version; undo and redo return to earlier
/// versions, so a frontend can tell whether the text matches what was saved.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    buffer: Buffer,
    history: History,
    cursor: usize,
    version: u64,
    last_version: u64,
}

impl Editor {
    /// An editor over `text`, with the cursor at the start.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::from_text(text),
            ..Self::default()
        }
    }

    /// The text.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The cursor, as a char offset.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The version of the current text.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Moves the cursor. Ends the current undo step, as in VS Code.
    ///
    /// # Errors
    /// When `offset` is past the end of the buffer.
    pub fn set_cursor(&mut self, offset: usize) -> Result<(), EditError> {
        self.check(offset)?;
        self.cursor = offset;
        self.history.close_step();
        Ok(())
    }

    /// Types `text` at the cursor and moves the cursor after it.
    pub fn type_text(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 {
            EditKind::of_typing(text, self.history.previous())
        } else {
            EditKind::Other
        };
        self.apply(
            kind,
            Range::empty(self.cursor),
            text,
            self.cursor + text.chars().count(),
        );
    }

    /// Deletes the char before the cursor (a CRLF counts as one). Returns
    /// whether anything was deleted.
    pub fn delete_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut start = self.cursor - 1;
        if start > 0
            && self.buffer.char_at(start) == Some('\n')
            && self.buffer.char_at(start - 1) == Some('\r')
        {
            start -= 1;
        }
        self.apply(
            EditKind::DeletingLeft,
            Range::new(start, self.cursor),
            "",
            start,
        );
        true
    }

    /// Deletes the char after the cursor (a CRLF counts as one). Returns
    /// whether anything was deleted.
    pub fn delete_right(&mut self) -> bool {
        let len = self.buffer.len_chars();
        if self.cursor >= len {
            return false;
        }
        let mut end = self.cursor + 1;
        if end < len
            && self.buffer.char_at(self.cursor) == Some('\r')
            && self.buffer.char_at(end) == Some('\n')
        {
            end += 1;
        }
        self.apply(
            EditKind::DeletingRight,
            Range::new(self.cursor, end),
            "",
            self.cursor,
        );
        true
    }

    /// Replaces `range` with `text` as a programmatic edit, its own undo step.
    /// The cursor keeps its place in the surrounding text. Returns whether
    /// the text changed.
    ///
    /// # Errors
    /// When `range` goes past the end of the buffer.
    pub fn replace(&mut self, range: Range, text: &str) -> Result<bool, EditError> {
        self.check(range.end)?;
        if range.is_empty() && text.is_empty() {
            return Ok(false);
        }
        let inserted = text.chars().count();
        let cursor = if self.cursor >= range.end {
            self.cursor - range.len() + inserted
        } else if self.cursor > range.start {
            range.start + inserted
        } else {
            self.cursor
        };
        self.apply(EditKind::Other, range, text, cursor);
        Ok(true)
    }

    /// Undoes the last undo step. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.history.undo() else {
            return false;
        };
        for change in step.changes.iter().rev() {
            self.buffer.remove(Range::new(
                change.offset,
                change.offset + change.inserted.chars().count(),
            ));
            self.buffer.insert(change.offset, &change.removed);
        }
        self.restore(&step, true);
        true
    }

    /// Redoes the last undone step. Returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.history.redo() else {
            return false;
        };
        for change in &step.changes {
            self.buffer.remove(Range::new(
                change.offset,
                change.offset + change.removed.chars().count(),
            ));
            self.buffer.insert(change.offset, &change.inserted);
        }
        self.restore(&step, false);
        true
    }

    fn restore(&mut self, step: &Step, before: bool) {
        if before {
            self.cursor = step.cursor_before;
            self.version = step.version_before;
        } else {
            self.cursor = step.cursor_after;
            self.version = step.version_after;
        }
    }

    fn apply(&mut self, kind: EditKind, range: Range, text: &str, cursor_after: usize) {
        let removed = self.buffer.slice(range);
        self.buffer.remove(range);
        self.buffer.insert(range.start, text);
        self.last_version += 1;
        let versions = (self.version, self.last_version);
        let cursors = (self.cursor, cursor_after);
        let change = Change {
            offset: range.start,
            removed,
            inserted: text.to_owned(),
        };
        self.history.record(kind, change, cursors, versions);
        self.cursor = cursor_after;
        self.version = self.last_version;
    }

    fn check(&self, offset: usize) -> Result<(), EditError> {
        let len = self.buffer.len_chars();
        if offset > len {
            return Err(EditError::OutOfRange { offset, len });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_is_deleted_as_one_line_break() {
        let mut editor = Editor::new("a\r\nb");
        editor.set_cursor(3).expect("cursor");
        assert!(editor.delete_left());
        assert_eq!(editor.buffer().text(), "ab");
        editor.set_cursor(1).expect("cursor");
        editor.type_text("\r\n");
        editor.set_cursor(1).expect("cursor");
        assert!(editor.delete_right());
        assert_eq!(editor.buffer().text(), "ab");
    }

    #[test]
    fn replace_keeps_the_cursor_in_the_surrounding_text() {
        let mut editor = Editor::new("hello world");
        editor.set_cursor(11).expect("cursor");
        assert_eq!(editor.replace(Range::new(0, 5), "hi"), Ok(true));
        assert_eq!(editor.cursor(), 8);
        assert_eq!(editor.replace(Range::empty(0), ""), Ok(false));
        assert!(editor.replace(Range::new(0, 99), "").is_err());
    }

    #[test]
    fn undo_and_redo_restore_versions() {
        let mut editor = Editor::new("");
        editor.type_text("a");
        let typed = editor.version();
        assert!(editor.undo());
        assert_eq!(editor.version(), 0);
        assert!(editor.redo());
        assert_eq!(editor.version(), typed);
        assert!(editor.undo());
        editor.type_text("b");
        assert_ne!(
            editor.version(),
            typed,
            "a new edit never reuses an undone version"
        );
        assert!(!editor.redo());
    }
}
