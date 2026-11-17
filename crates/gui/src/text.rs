//! How one document line is drawn.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! a tab reaches the next tab stop (`nextRenderTabStop` in
//! `src/vs/editor/common/core/cursorColumns.ts:122-124`), the status bar's
//! column counts a tab that way too (`toStatusbarColumn`, `:61-77`), and
//! rendering stops after 10,000 chars (`editor.stopRenderingLineAfter`,
//! `src/vs/editor/common/config/editorOptions.ts:6880-6883`).

/// Chars of a line that are drawn; the rest is cut, as in VS Code.
pub(crate) const RENDER_LIMIT: usize = 10_000;

/// A line as drawn: tabs become spaces up to the next tab stop, and the
/// line is cut after [`RENDER_LIMIT`] chars.
#[derive(Debug)]
pub(crate) struct DisplayLine {
    /// The text to lay out.
    pub(crate) text: String,
    /// Where each drawn column starts in `text` (in chars), plus the end.
    starts: Vec<usize>,
    /// Whether chars past the limit were left out.
    pub(crate) truncated: bool,
}

impl DisplayLine {
    pub(crate) fn new(line: &str, tab_size: usize) -> Self {
        let tab_size = tab_size.max(1);
        let mut text = String::with_capacity(line.len());
        let mut starts = Vec::with_capacity(line.len().min(RENDER_LIMIT) + 1);
        let mut width = 0;
        let mut truncated = false;
        for (column, ch) in line.chars().enumerate() {
            if column == RENDER_LIMIT {
                truncated = true;
                break;
            }
            starts.push(width);
            if ch == '\t' {
                let stop = width + tab_size - width % tab_size;
                text.extend(std::iter::repeat_n(' ', stop - width));
                width = stop;
            } else {
                text.push(ch);
                width += 1;
            }
        }
        starts.push(width);
        Self {
            text,
            starts,
            truncated,
        }
    }

    /// Drawn columns (chars of the line that are drawn).
    pub(crate) fn columns(&self) -> usize {
        self.starts.len() - 1
    }

    /// Where `column` starts in [`Self::text`]; past the cut, the end.
    pub(crate) fn display_index(&self, column: usize) -> usize {
        self.starts[column.min(self.columns())]
    }

    /// The column nearest to `index` in [`Self::text`]: inside a tab, the
    /// nearer of its two sides.
    pub(crate) fn column_at(&self, index: usize) -> usize {
        let next = self.starts.partition_point(|&start| start <= index);
        let column = next.saturating_sub(1);
        match self.starts.get(column + 1) {
            Some(&end) if index - self.starts[column] > (end - self.starts[column]) / 2 => {
                column + 1
            }
            _ => column,
        }
    }
}

/// The 1-based column the status bar shows for `column` of `line`: a tab
/// reaches the next tab stop.
pub(crate) fn status_column(line: &str, column: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let width = line.chars().take(column).fold(0, |width, ch| {
        if ch == '\t' {
            width + tab_size - width % tab_size
        } else {
            width + 1
        }
    });
    width + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_reach_the_next_stop() {
        let line = DisplayLine::new("a\tbc\td", 4);
        assert_eq!(line.text, "a   bc  d");
        assert_eq!(line.display_index(2), 4, "b starts at the tab stop");
        assert_eq!(line.display_index(5), 8);
        assert_eq!(line.display_index(99), 9, "past the end: the end");
        assert_eq!(status_column("a\tbc\td", 2, 4), 5);
    }

    #[test]
    fn a_click_inside_a_tab_goes_to_its_nearer_side() {
        let line = DisplayLine::new("\tx", 4);
        assert_eq!(line.column_at(0), 0);
        assert_eq!(line.column_at(1), 0);
        assert_eq!(line.column_at(3), 1);
        assert_eq!(line.column_at(4), 1);
        assert_eq!(line.column_at(9), 2);
    }

    #[test]
    fn long_lines_are_cut() {
        let line = DisplayLine::new(&"x".repeat(RENDER_LIMIT + 5), 4);
        assert!(line.truncated);
        assert_eq!(line.columns(), RENDER_LIMIT);
        assert_eq!(line.display_index(RENDER_LIMIT + 3), RENDER_LIMIT);
    }
}
