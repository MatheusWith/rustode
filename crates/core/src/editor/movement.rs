//! Cursor movement, Enter and Tab, following VS Code (reimplemented; see
//! `ATTRIBUTION.md`):
//!
//! - Left/Right collapse a selection to its start/end
//!   (`src/vs/editor/common/cursor/cursorMoveOperations.ts:64-87`), and step
//!   over a CRLF at once.
//! - Up/Down start from a selection's start/end, keep the column a cursor
//!   could not reach on a short line (`leftoverVisibleColumns`), go to the
//!   start of the first line or the end of the last one at the edges
//!   (`cursorMoveOperations.ts:157-187`, `:203-264`).
//! - Home goes to the first non-blank char, or to column 0 if already there
//!   (`cursorMoveOperations.ts:323-336`).
//! - Enter starts a new undo step that the typing after it joins, and keeps
//!   the current line's indentation up to the cursor for text without
//!   language rules (`src/vs/editor/common/cursor/cursorTypeEditOperations.ts:573-596`).
//! - Tab inserts up to the next tab stop, as its own undo step
//!   (`cursorTypeEditOperations.ts:844-883`; `src/vs/editor/browser/coreCommands.ts:2040-2044`).
//!
//! Columns count chars; VS Code counts visible columns (a tab spans to the next
//! tab stop) for Up/Down.

use super::{Editor, Selections};
use crate::history::EditKind;
use crate::selection::Selection;

/// Where cursors move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    /// One char left; a CRLF counts as one. A selection collapses to its start.
    Left,
    /// One char right; a CRLF counts as one. A selection collapses to its end.
    Right,
    /// One line up, keeping the column.
    Up,
    /// One line down, keeping the column.
    Down,
    /// This many lines up (Page Up with the viewport height).
    PageUp(usize),
    /// This many lines down (Page Down with the viewport height).
    PageDown(usize),
    /// The first non-blank char of the line, or column 0 if already there.
    LineStart,
    /// The end of the line.
    LineEnd,
    /// The start of the document.
    DocumentStart,
    /// The end of the document.
    DocumentEnd,
}

impl Movement {
    fn vertical_lines(self) -> Option<(bool, usize)> {
        match self {
            Self::Up => Some((true, 1)),
            Self::Down => Some((false, 1)),
            Self::PageUp(n) => Some((true, n.max(1))),
            Self::PageDown(n) => Some((false, n.max(1))),
            _ => None,
        }
    }
}

impl Editor {
    /// Moves every cursor. With `select`, each selection's anchor stays and
    /// the selection grows or shrinks (Shift); without it, selections collapse.
    /// Ends the current undo step.
    pub fn move_cursors(&mut self, movement: Movement, select: bool) {
        let count = self.selections().len();
        let vertical = movement.vertical_lines();
        let goals = match (vertical, self.goal_columns.take()) {
            (Some(_), Some(kept)) if kept.len() == count => kept.into_iter().map(Some).collect(),
            _ => vec![None; count],
        };
        let mut list = Vec::with_capacity(count);
        let mut kept = Vec::with_capacity(count);
        for (selection, goal) in self.selections().iter().zip(goals) {
            let (head, goal) = self.moved(*selection, movement, select, goal);
            let anchor = if select { selection.anchor } else { head };
            list.push(Selection::new(anchor, head));
            kept.push(goal);
        }
        let selections = Selections::from_list(list, self.selections.primary_index());
        let merged = selections.as_slice().len() != count;
        self.select(selections);
        if vertical.is_some() && !merged {
            self.goal_columns = Some(kept);
        }
    }

    /// Escape: with several selections, keeps only the primary one (VS Code's
    /// `removeSecondaryCursors`); otherwise collapses the selection to its
    /// cursor (`cancelSelection`), as in `src/vs/editor/browser/coreCommands.ts:1773-1824`.
    /// Returns whether the selections changed.
    pub fn cancel_selections(&mut self) -> bool {
        let primary = self.primary();
        let next = if self.selections().len() > 1 {
            primary
        } else if primary.is_empty() {
            return false;
        } else {
            Selection::cursor(primary.head)
        };
        self.select(Selections::single(next));
        true
    }

    /// Selects the whole text, anchored at the start.
    pub fn select_all(&mut self) {
        let end = self.buffer.len_chars();
        self.select(Selections::single(Selection::new(0, end)));
    }

    /// Enter: replaces each selection with `eol` plus the indentation of its
    /// line up to the cursor. Starts a new undo step; typing after it joins.
    pub fn insert_line_break(&mut self, eol: &str) {
        let edits = self.each(|buffer, selection| {
            let start = selection.range().start;
            let line = buffer.line_of(start);
            let column = start - buffer.line_start(line);
            let indent = buffer.line_prefix(line, buffer.leading_whitespace(line).min(column));
            (selection.range(), format!("{eol}{indent}"))
        });
        self.history.close_step();
        self.apply(EditKind::TypingOther, edits);
    }

    /// Tab: replaces each selection with spaces up to the next multiple of
    /// `tab_size` (or a tab char when `insert_spaces` is false), as its own
    /// undo step.
    pub fn insert_tab(&mut self, tab_size: usize, insert_spaces: bool) {
        let tab_size = tab_size.max(1);
        let edits = self.each(|buffer, selection| {
            let start = selection.range().start;
            let text = if insert_spaces {
                let line = buffer.line_of(start);
                let prefix = buffer.line_prefix(line, start - buffer.line_start(line));
                let column = visible_column(&prefix, tab_size);
                " ".repeat(tab_size - column % tab_size)
            } else {
                "\t".to_owned()
            };
            (selection.range(), text)
        });
        self.history.close_step();
        self.apply(EditKind::Other, edits);
    }

    /// Where `selection`'s cursor goes, and the column it aims for on the
    /// next vertical move.
    fn moved(
        &self,
        selection: Selection,
        movement: Movement,
        select: bool,
        goal: Option<usize>,
    ) -> (usize, usize) {
        let collapse = !select && !selection.is_empty();
        let range = selection.range();
        if let Some((up, lines)) = movement.vertical_lines() {
            let from = match (collapse, up) {
                (true, true) => range.start,
                (true, false) => range.end,
                (false, _) => selection.head,
            };
            return self.vertical(from, goal, up, lines);
        }
        let head = match movement {
            Movement::Left if collapse => range.start,
            Movement::Right if collapse => range.end,
            Movement::Left => self.step_left(selection.head),
            Movement::Right => self.step_right(selection.head),
            Movement::LineStart => self.smart_home(selection.head),
            Movement::LineEnd => {
                let line = self.buffer.line_of(selection.head);
                self.buffer.line_start(line) + self.buffer.line_len(line)
            }
            Movement::DocumentStart => 0,
            _ => self.buffer.len_chars(),
        };
        (head, self.position_of(head).column)
    }

    /// A vertical move of `lines` from `from`. Past the first line the cursor
    /// goes to the document start, past the last one to its end; the goal
    /// column survives that unless the cursor was already there.
    fn vertical(&self, from: usize, goal: Option<usize>, up: bool, lines: usize) -> (usize, usize) {
        let position = self.position_of(from);
        let last = self.buffer.len_lines() - 1;
        let wanted = goal.unwrap_or(position.column);
        let target = if up {
            position.line.checked_sub(lines)
        } else {
            Some(position.line + lines).filter(|&l| l <= last)
        };
        let Some(line) = target else {
            let (offset, column) = if up {
                (0, 0)
            } else {
                (self.buffer.len_chars(), self.buffer.line_len(last))
            };
            let at_edge = from == offset;
            return (offset, if at_edge { column } else { wanted });
        };
        let column = wanted.min(self.buffer.line_len(line));
        (self.buffer.line_start(line) + column, wanted)
    }

    fn step_left(&self, offset: usize) -> usize {
        let crlf = offset >= 2
            && self.buffer.char_at(offset - 1) == Some('\n')
            && self.buffer.char_at(offset - 2) == Some('\r');
        offset.saturating_sub(if crlf { 2 } else { 1 })
    }

    fn step_right(&self, offset: usize) -> usize {
        let crlf = self.buffer.char_at(offset) == Some('\r')
            && self.buffer.char_at(offset + 1) == Some('\n');
        (offset + if crlf { 2 } else { 1 }).min(self.buffer.len_chars())
    }

    fn smart_home(&self, offset: usize) -> usize {
        let line = self.buffer.line_of(offset);
        let start = self.buffer.line_start(line);
        let blank = self.buffer.leading_whitespace(line);
        let first_non_blank = if blank == self.buffer.line_len(line) {
            start
        } else {
            start + blank
        };
        if offset == first_non_blank {
            start
        } else {
            first_non_blank
        }
    }
}

/// The visible column after `prefix`, where a tab advances to the next tab stop.
fn visible_column(prefix: &str, tab_size: usize) -> usize {
    prefix.chars().fold(0, |column, c| {
        if c == '\t' {
            (column / tab_size + 1) * tab_size
        } else {
            column + 1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_advance_the_visible_column_to_the_next_stop() {
        assert_eq!(visible_column("", 4), 0);
        assert_eq!(visible_column("ab", 4), 2);
        assert_eq!(visible_column("a\t", 4), 4);
        assert_eq!(visible_column("a\tb\t", 4), 8);
    }

    #[test]
    fn several_cursors_move_together_and_merge_when_they_meet() {
        let mut editor = Editor::new("ab\ncd");
        editor
            .set_selections(vec![Selection::cursor(1), Selection::cursor(4)], 0)
            .expect("select");
        editor.move_cursors(Movement::Down, false);
        assert_eq!(
            editor.selections(),
            [Selection::cursor(4), Selection::cursor(5)]
        );
        editor.move_cursors(Movement::DocumentEnd, false);
        assert_eq!(editor.selections(), [Selection::cursor(5)]);
    }

    #[test]
    fn the_goal_column_survives_paging_past_the_end() {
        // VS Code keeps a negative leftover here, so Up returns to column 1.
        let mut editor = Editor::new("abcdef\nab\nxy");
        editor.set_cursor(8).expect("cursor");
        editor.move_cursors(Movement::PageDown(5), false);
        assert_eq!(editor.cursor(), 12);
        editor.move_cursors(Movement::Up, false);
        assert_eq!(editor.cursor(), 8);
    }

    #[test]
    fn escape_drops_secondary_cursors_then_the_selection() {
        let mut editor = Editor::new("abc abc");
        editor
            .set_selections(vec![Selection::new(0, 3), Selection::new(4, 7)], 1)
            .expect("select");
        assert!(editor.cancel_selections());
        assert_eq!(editor.selections(), [Selection::new(4, 7)]);
        assert!(editor.cancel_selections());
        assert_eq!(editor.selections(), [Selection::cursor(7)]);
        assert!(!editor.cancel_selections());
        editor.select_all();
        assert_eq!(editor.selections(), [Selection::new(0, 7)]);
    }

    #[test]
    fn a_whitespace_only_line_homes_to_column_zero() {
        let mut editor = Editor::new("    ");
        editor.set_cursor(2).expect("cursor");
        editor.move_cursors(Movement::LineStart, false);
        assert_eq!(editor.cursor(), 0);
    }
}
