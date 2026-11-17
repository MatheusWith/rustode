//! Selections: one or more cursors, each with an anchor and a head.
//!
//! As in VS Code, selections stay sorted and are merged when they overlap,
//! or when they touch and one of them is a plain cursor
//! (`CursorCollection`, `src/vs/editor/common/cursor/cursorCollection.ts:188-201`).

use crate::editor::Range;

/// A cursor and the text it selects: from `anchor` (where the selection
/// started) to `head` (where the cursor is). Empty when both are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Selection {
    /// Where the selection started, as a char offset.
    pub anchor: usize,
    /// Where the cursor is, as a char offset.
    pub head: usize,
}

impl Selection {
    /// A selection from `anchor` to `head`.
    #[must_use]
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// An empty selection (a plain cursor) at `offset`.
    #[must_use]
    pub fn cursor(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    /// The selected range, whichever way the selection points.
    #[must_use]
    pub fn range(self) -> Range {
        Range::new(self.anchor, self.head)
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }

    fn map(self, f: impl Fn(usize) -> usize) -> Self {
        Self {
            anchor: f(self.anchor),
            head: f(self.head),
        }
    }
}

/// A position as a zero-based line and a zero-based column in chars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    /// Zero-based line.
    pub line: usize,
    /// Zero-based column, in chars (a tab counts as one).
    pub column: usize,
}

impl Position {
    /// The position at `line`, `column`.
    #[must_use]
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// All selections of an editor, sorted by position and never overlapping,
/// plus which one is primary (the one a frontend scrolls to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selections {
    list: Vec<Selection>,
    primary: usize,
}

impl Default for Selections {
    fn default() -> Self {
        Self::single(Selection::cursor(0))
    }
}

impl Selections {
    pub fn single(selection: Selection) -> Self {
        Self {
            list: vec![selection],
            primary: 0,
        }
    }

    /// Normalizes `list`: sorted, overlapping or coinciding selections merged.
    /// `primary` indexes `list` before sorting. `list` must not be empty.
    pub fn from_list(list: Vec<Selection>, primary: usize) -> Self {
        let mut indexed: Vec<(Selection, bool)> = list
            .into_iter()
            .enumerate()
            .map(|(i, s)| (s, i == primary))
            .collect();
        indexed.sort_by_key(|(s, _)| (s.range().start, s.range().end));
        let mut merged: Vec<(Selection, bool)> = Vec::with_capacity(indexed.len());
        for (selection, is_primary) in indexed {
            match merged.last_mut() {
                Some((prev, prev_is_primary)) if overlaps(*prev, selection) => {
                    let start = prev.range().start.min(selection.range().start);
                    let end = prev.range().end.max(selection.range().end);
                    let forward = prev.head >= prev.anchor;
                    *prev = if forward {
                        Selection::new(start, end)
                    } else {
                        Selection::new(end, start)
                    };
                    *prev_is_primary |= is_primary;
                }
                _ => merged.push((selection, is_primary)),
            }
        }
        let primary = merged.iter().position(|(_, p)| *p).unwrap_or(0);
        Self {
            list: merged.into_iter().map(|(s, _)| s).collect(),
            primary,
        }
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.list
    }

    pub fn primary(&self) -> Selection {
        self.list[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Maps every offset through `f`, e.g. to follow an edit.
    pub fn map(&self, f: impl Fn(usize) -> usize) -> Self {
        Self::from_list(self.list.iter().map(|s| s.map(&f)).collect(), self.primary)
    }
}

fn overlaps(a: Selection, b: Selection) -> bool {
    // `a` starts first: merge overlaps, and touches when one is a cursor.
    let (a, b) = (a.range(), b.range());
    if a.is_empty() || b.is_empty() {
        b.start <= a.end
    } else {
        b.start < a.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_are_sorted_and_merged_keeping_the_primary() {
        let list = vec![
            Selection::new(5, 8),
            Selection::cursor(0),
            Selection::new(7, 10),
            Selection::cursor(0),
        ];
        let selections = Selections::from_list(list, 2);
        assert_eq!(
            selections.as_slice(),
            [Selection::cursor(0), Selection::new(5, 10)]
        );
        assert_eq!(selections.primary(), Selection::new(5, 10));
    }

    #[test]
    fn a_cursor_touching_a_selection_merges_into_it() {
        let selections = Selections::from_list(vec![Selection::new(2, 5), Selection::cursor(5)], 1);
        assert_eq!(selections.as_slice(), [Selection::new(2, 5)]);
    }

    #[test]
    fn touching_selections_stay_apart() {
        let selections = Selections::from_list(vec![Selection::new(0, 2), Selection::new(2, 4)], 0);
        assert_eq!(selections.as_slice().len(), 2);
    }
}
