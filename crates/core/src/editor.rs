//! Editing one buffer: selections (multi-cursor), edits, and undo/redo
//! grouped like VS Code.
//!
//! Multi-cursor behavior was studied in VS Code and reimplemented here (see
//! `ATTRIBUTION.md`): box selection follows `ColumnSelection.columnSelect`
//! (`src/vs/editor/common/cursor/cursorColumnSelection.ts:12-77`), adding a
//! cursor above or below follows `addCursorUp`/`addCursorDown`
//! (`src/vs/editor/common/cursor/cursorMoveCommands.ts:18-46`), and adding
//! the next occurrence follows `editor.action.addSelectionToNextFindMatch`
//! (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:355-410`, `:592-620`).

use crate::buffer::Buffer;
use crate::history::{Change, EditKind, History, Step};
use crate::selection::{Position, Selection, Selections};
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

/// Why an edit or a selection change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// An offset or range goes past the end of the buffer.
    OutOfRange {
        /// The offending offset.
        offset: usize,
        /// The buffer length in chars.
        len: usize,
    },
    /// A line does not exist.
    NoSuchLine {
        /// The offending line.
        line: usize,
        /// The number of lines.
        lines: usize,
    },
    /// An editor always has at least one selection.
    NoSelections,
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
            Self::NoSuchLine { line, lines } => {
                write!(f, "line {line} does not exist ({lines} lines)")
            }
            Self::NoSelections => f.write_str("at least one selection is required"),
        }
    }
}

impl std::error::Error for EditError {}

/// One replacement to make at one selection.
struct Edit {
    range: Range,
    text: String,
    primary: bool,
}

/// A buffer with one or more selections and an undo history.
///
/// Every state of the text has a version; undo and redo return to earlier
/// versions, so a frontend can tell whether the text matches what was saved.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    buffer: Buffer,
    history: History,
    selections: Selections,
    version: u64,
    last_version: u64,
}

impl Editor {
    /// An editor over `text`, with one cursor at the start.
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

    /// All selections, sorted by position and never overlapping.
    #[must_use]
    pub fn selections(&self) -> &[Selection] {
        self.selections.as_slice()
    }

    /// The primary selection: the last one added, which a frontend reveals.
    #[must_use]
    pub fn primary(&self) -> Selection {
        self.selections.primary()
    }

    /// The primary cursor, as a char offset.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.primary().head
    }

    /// The version of the current text.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The line and column of char `offset`.
    #[must_use]
    pub fn position_of(&self, offset: usize) -> Position {
        let line = self.buffer.line_of(offset);
        Position::new(line, offset - self.buffer.line_start(line))
    }

    /// Replaces all selections with one cursor. Ends the current undo step.
    ///
    /// # Errors
    /// When `offset` is past the end of the buffer.
    pub fn set_cursor(&mut self, offset: usize) -> Result<(), EditError> {
        self.set_selections(vec![Selection::cursor(offset)], 0)
    }

    /// Replaces all selections; `primary` indexes `list`. Overlapping
    /// selections are merged. Ends the current undo step.
    ///
    /// # Errors
    /// When `list` is empty or a selection is past the end of the buffer.
    pub fn set_selections(
        &mut self,
        list: Vec<Selection>,
        primary: usize,
    ) -> Result<(), EditError> {
        if list.is_empty() {
            return Err(EditError::NoSelections);
        }
        for selection in &list {
            self.check(selection.anchor.max(selection.head))?;
        }
        self.select(Selections::from_list(list, primary));
        Ok(())
    }

    /// Types `text` at every selection, replacing what is selected.
    pub fn type_text(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 {
            EditKind::of_typing(text, self.history.previous())
        } else {
            EditKind::Other
        };
        let edits = self.each(|_, selection| (selection.range(), text.to_owned()));
        self.apply(kind, edits);
    }

    /// Deletes the selected text, or the char before each cursor (a CRLF
    /// counts as one). Returns whether anything was deleted.
    pub fn delete_left(&mut self) -> bool {
        let edits = self.each(|buffer, selection| {
            if !selection.is_empty() || selection.head == 0 {
                return (selection.range(), String::new());
            }
            let mut start = selection.head - 1;
            if start > 0
                && buffer.char_at(start) == Some('\n')
                && buffer.char_at(start - 1) == Some('\r')
            {
                start -= 1;
            }
            (Range::new(start, selection.head), String::new())
        });
        self.apply(EditKind::DeletingLeft, edits)
    }

    /// Deletes the selected text, or the char after each cursor (a CRLF
    /// counts as one). Returns whether anything was deleted.
    pub fn delete_right(&mut self) -> bool {
        let edits = self.each(|buffer, selection| {
            let len = buffer.len_chars();
            if !selection.is_empty() || selection.head >= len {
                return (selection.range(), String::new());
            }
            let mut end = selection.head + 1;
            if end < len
                && buffer.char_at(selection.head) == Some('\r')
                && buffer.char_at(end) == Some('\n')
            {
                end += 1;
            }
            (Range::new(selection.head, end), String::new())
        });
        self.apply(EditKind::DeletingRight, edits)
    }

    /// Replaces `range` with `text` as a programmatic edit, its own undo step.
    /// Selections keep their place in the surrounding text. Returns whether
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
        let after = self.selections.map(|offset| {
            if offset >= range.end {
                offset - range.len() + inserted
            } else if offset > range.start {
                range.start + inserted
            } else {
                offset
            }
        });
        let changes = vec![self.change(range, text)];
        self.commit(EditKind::Other, changes, after);
        Ok(true)
    }

    /// Adds a selection at the next occurrence of the primary selection's
    /// text, wrapping around (`Ctrl+D`). With an empty primary selection,
    /// first selects the word under each empty cursor instead. Returns
    /// whether the selections changed.
    pub fn add_next_occurrence(&mut self) -> bool {
        let primary = self.primary();
        if primary.is_empty() {
            return self.select_words();
        }
        let chars: Vec<char> = self.buffer.text().chars().collect();
        let needle: Vec<char> = self.buffer.slice(primary.range()).chars().collect();
        let taken: Vec<Range> = self.selections().iter().map(|s| s.range()).collect();
        let from = primary.range().end;
        let candidates = (from..chars.len()).chain(0..from);
        for start in candidates {
            let end = start + needle.len();
            if end > chars.len() || chars[start..end] != needle[..] {
                continue;
            }
            let range = Range::new(start, end);
            if taken.contains(&range) {
                continue;
            }
            let mut list = self.selections().to_vec();
            list.push(Selection::new(start, end));
            let primary = list.len() - 1;
            self.select(Selections::from_list(list, primary));
            return true;
        }
        false
    }

    /// Adds a cursor on the line below each selection's cursor, at the same
    /// column or the line's end. Returns whether a cursor was added.
    pub fn add_cursor_below(&mut self) -> bool {
        self.add_cursor_vertically(true)
    }

    /// Adds a cursor on the line above each selection's cursor, at the same
    /// column or the line's end. Returns whether a cursor was added.
    pub fn add_cursor_above(&mut self) -> bool {
        self.add_cursor_vertically(false)
    }

    /// Box selection from `from` to `to`: one selection per line between
    /// them, from `from.column` to `to.column`. Lines too short to reach the
    /// box are skipped; if every line is, each gets a cursor at its end.
    ///
    /// # Errors
    /// When a line does not exist.
    pub fn column_select(&mut self, from: Position, to: Position) -> Result<(), EditError> {
        let lines = self.buffer.len_lines();
        for line in [from.line, to.line] {
            if line >= lines {
                return Err(EditError::NoSuchLine { line, lines });
            }
        }
        let line_numbers: Vec<usize> = if from.line <= to.line {
            (from.line..=to.line).collect()
        } else {
            (to.line..=from.line).rev().collect()
        };
        let mut list = Vec::new();
        for &line in &line_numbers {
            let len = self.buffer.line_len(line);
            let reaches = match from.column.cmp(&to.column) {
                std::cmp::Ordering::Less => len >= from.column,
                std::cmp::Ordering::Greater => len >= to.column,
                std::cmp::Ordering::Equal => true,
            };
            if reaches {
                list.push(Selection::new(
                    self.buffer.offset_of(line, from.column),
                    self.buffer.offset_of(line, to.column),
                ));
            }
        }
        if list.is_empty() {
            list = line_numbers
                .iter()
                .map(|&line| {
                    Selection::cursor(self.buffer.line_start(line) + self.buffer.line_len(line))
                })
                .collect();
        }
        let primary = list.len() - 1;
        self.select(Selections::from_list(list, primary));
        Ok(())
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
        self.restore(step, true);
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
        self.restore(step, false);
        true
    }

    fn restore(&mut self, step: Step, before: bool) {
        if before {
            self.selections = step.cursor_before;
            self.version = step.version_before;
        } else {
            self.selections = step.cursor_after;
            self.version = step.version_after;
        }
    }

    fn select(&mut self, selections: Selections) {
        self.selections = selections;
        self.history.close_step();
    }

    fn select_words(&mut self) -> bool {
        let mut changed = false;
        let list: Vec<Selection> = self
            .selections()
            .iter()
            .map(|&selection| {
                if !selection.is_empty() {
                    return selection;
                }
                let range = self.word_at(selection.head);
                if range.is_empty() {
                    return selection;
                }
                changed = true;
                Selection::new(range.start, range.end)
            })
            .collect();
        if changed {
            let primary = self.selections.primary_index();
            self.select(Selections::from_list(list, primary));
        }
        changed
    }

    fn word_at(&self, offset: usize) -> Range {
        let is_word = |at: usize| {
            self.buffer
                .char_at(at)
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        };
        let mut start = offset;
        while start > 0 && is_word(start - 1) {
            start -= 1;
        }
        let mut end = offset;
        while is_word(end) {
            end += 1;
        }
        Range::new(start, end)
    }

    fn add_cursor_vertically(&mut self, down: bool) -> bool {
        let lines = self.buffer.len_lines();
        let mut list = self.selections().to_vec();
        let mut primary = self.selections.primary_index();
        let before = list.len();
        for (index, selection) in self.selections().iter().enumerate() {
            let position = self.position_of(selection.head);
            let line = if down {
                Some(position.line + 1).filter(|&l| l < lines)
            } else {
                position.line.checked_sub(1)
            };
            if let Some(line) = line {
                list.push(Selection::cursor(
                    self.buffer.offset_of(line, position.column),
                ));
                if index == self.selections.primary_index() {
                    primary = list.len() - 1;
                }
            }
        }
        if list.len() == before {
            return false;
        }
        self.select(Selections::from_list(list, primary));
        true
    }

    /// One edit per selection, from `f(buffer, selection) -> (range, text)`.
    fn each(&self, f: impl Fn(&Buffer, Selection) -> (Range, String)) -> Vec<Edit> {
        let primary = self.selections.primary_index();
        self.selections()
            .iter()
            .enumerate()
            .map(|(index, &selection)| {
                let (range, text) = f(&self.buffer, selection);
                Edit {
                    range,
                    text,
                    primary: index == primary,
                }
            })
            .collect()
    }

    /// Applies one edit per selection as a single undo step, leaving a cursor
    /// after each. Returns whether the text changed.
    fn apply(&mut self, kind: EditKind, edits: Vec<Edit>) -> bool {
        let edits = merge(edits);
        if edits
            .iter()
            .all(|e| e.range.is_empty() && e.text.is_empty())
        {
            return false;
        }
        let (mut inserted, mut removed) = (0, 0);
        let mut after = Vec::with_capacity(edits.len());
        let mut primary = 0;
        for (index, edit) in edits.iter().enumerate() {
            let text_len = edit.text.chars().count();
            after.push(Selection::cursor(
                edit.range.start + inserted - removed + text_len,
            ));
            inserted += text_len;
            removed += edit.range.len();
            if edit.primary {
                primary = index;
            }
        }
        let changes = edits
            .iter()
            .rev()
            .filter(|e| !(e.range.is_empty() && e.text.is_empty()))
            .map(|e| self.change(e.range, &e.text))
            .collect();
        self.commit(kind, changes, Selections::from_list(after, primary));
        true
    }

    /// Builds the change replacing `range` with `text`, and applies it.
    fn change(&mut self, range: Range, text: &str) -> Change {
        let removed = self.buffer.slice(range);
        self.buffer.remove(range);
        self.buffer.insert(range.start, text);
        Change {
            offset: range.start,
            removed,
            inserted: text.to_owned(),
        }
    }

    fn commit(&mut self, kind: EditKind, changes: Vec<Change>, after: Selections) {
        self.last_version += 1;
        let versions = (self.version, self.last_version);
        let before = std::mem::replace(&mut self.selections, after.clone());
        self.history
            .record(kind, changes, (before, after), versions);
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

/// Sorts edits by position and merges overlapping ones (e.g. two cursors
/// deleting the same CRLF).
fn merge(mut edits: Vec<Edit>) -> Vec<Edit> {
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    let mut merged: Vec<Edit> = Vec::with_capacity(edits.len());
    for edit in edits {
        match merged.last_mut() {
            Some(last) if edit.range.start < last.range.end => {
                last.range = Range::new(last.range.start, last.range.end.max(edit.range.end));
                last.primary |= edit.primary;
            }
            _ => merged.push(edit),
        }
    }
    merged
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

    #[test]
    fn cursors_deleting_the_same_crlf_merge() {
        let mut editor = Editor::new("a\r\nb");
        editor
            .set_selections(vec![Selection::cursor(2), Selection::cursor(3)], 0)
            .expect("select");
        assert!(editor.delete_left());
        assert_eq!(editor.buffer().text(), "ab");
        assert_eq!(editor.selections(), [Selection::cursor(1)]);
    }

    #[test]
    fn selection_changes_are_validated() {
        let mut editor = Editor::new("ab");
        assert_eq!(
            editor.set_selections(Vec::new(), 0),
            Err(EditError::NoSelections)
        );
        assert!(
            editor
                .set_selections(vec![Selection::new(0, 3)], 0)
                .is_err()
        );
        assert!(
            editor
                .column_select(Position::new(0, 0), Position::new(1, 0))
                .is_err()
        );
        assert_eq!(editor.position_of(2), Position::new(0, 2));
    }
}
