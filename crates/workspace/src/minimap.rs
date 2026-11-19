//! What the GUI's minimap needs from the workspace: whether it is shown
//! (`editor.minimap.enabled`, toggled by `editor.action.toggleMinimap`) and
//! the colored runs of the lines it draws.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the setting is on by default (`src/vs/editor/common/config/editorOptions.ts:3582`, `:3599-3603`)
//! and the minimap draws no whitespace: it paints each non-blank char of a
//! line on its column, a tab advancing to the next tab stop, up to
//! `editor.minimap.maxColumn`, 120 by default
//! (`src/vs/editor/browser/viewParts/minimap/minimap.ts:2213-2275`, the line
//! renderer: tabs and spaces only move on, `:2251-2258`;
//! `editorOptions.ts:3589`). The color of each char comes from its token, as
//! here from its [`TokenKind`]. Not done: VS Code draws a full-width char as
//! two blocks (`:2260-2262`); here every char is one column.
//!
//! The toggle is rustode's own session override: VS Code's action writes the
//! user's setting, and rustode does not write `settings.json` yet. A later
//! change of `editor.minimap.enabled` in the file wins over the toggle.

use crate::{Document, DocumentId, Error, Workspace};
use rustode_config::Value;
use rustode_core::Position;
use rustode_syntax::TokenKind;
use std::ops::Range;

/// The setting that shows or hides the minimap.
pub const MINIMAP_SETTING: &str = "editor.minimap.enabled";

/// A run of non-blank chars of one line, with one color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinimapRun {
    /// The column it starts at, counting a tab as the columns up to the next
    /// tab stop.
    pub column: usize,
    /// How many columns it covers.
    pub len: usize,
    /// What the text is, or `None` for text that is not highlighted.
    pub kind: Option<TokenKind>,
}

impl Document {
    /// The runs of the lines in `lines` (zero-based, end excluded, clamped to
    /// the document), one list per line, in column order, with no run past
    /// `max_column` columns. A blank char ends a run, so the runs are
    /// exactly what the minimap paints. Only those lines are read, and no
    /// more than `max_column` chars of each.
    #[must_use]
    pub fn minimap_lines(
        &self,
        lines: Range<usize>,
        tab_size: usize,
        max_column: usize,
    ) -> Vec<Vec<MinimapRun>> {
        let end = lines.end.min(self.line_count());
        if lines.start >= end {
            return Vec::new();
        }
        let tab_size = tab_size.max(1);
        let highlights = self.highlights(lines.start..end);
        (lines.start..end)
            .map(|line| {
                let start = self.offset_of(Position::new(line, 0));
                let first = highlights.partition_point(|h| h.range.end <= start);
                line_runs(
                    &self.line(line),
                    start,
                    &highlights[first..],
                    tab_size,
                    max_column,
                )
            })
            .collect()
    }
}

/// The runs of one line, whose first char is the document's char `start`.
fn line_runs(
    text: &str,
    start: usize,
    highlights: &[crate::Highlight],
    tab_size: usize,
    max_column: usize,
) -> Vec<MinimapRun> {
    let mut runs: Vec<MinimapRun> = Vec::new();
    let mut next = 0;
    let mut column = 0;
    for (index, ch) in text.chars().enumerate() {
        if column >= max_column {
            break;
        }
        let width = if ch == '\t' {
            tab_size - column % tab_size
        } else {
            1
        };
        if !ch.is_whitespace() {
            let offset = start + index;
            while highlights.get(next).is_some_and(|h| h.range.end <= offset) {
                next += 1;
            }
            let kind = highlights
                .get(next)
                .filter(|h| h.range.start <= offset)
                .map(|h| h.kind);
            match runs.last_mut() {
                Some(last) if last.column + last.len == column && last.kind == kind => {
                    last.len += 1;
                }
                _ => runs.push(MinimapRun {
                    column,
                    len: 1,
                    kind,
                }),
            }
        }
        column += width;
    }
    runs
}

impl Workspace {
    /// Whether the minimap is shown: `editor.minimap.enabled` (true by
    /// default), unless [`Workspace::toggle_minimap`] flipped it since the
    /// setting last changed.
    #[must_use]
    pub fn minimap_enabled(&self) -> bool {
        self.minimap_toggled
            .unwrap_or_else(|| self.minimap_setting())
    }

    /// Flips [`Workspace::minimap_enabled`] for this session
    /// (`editor.action.toggleMinimap`); returns the new state.
    pub fn toggle_minimap(&mut self) -> bool {
        let shown = !self.minimap_enabled();
        self.minimap_toggled = Some(shown);
        shown
    }

    fn minimap_setting(&self) -> bool {
        !matches!(self.config.value(MINIMAP_SETTING), Some(Value::Bool(false)))
    }

    /// The runs of the lines `lines` of the document, for the minimap, with
    /// the document's tab size (see [`Document::minimap_lines`]).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn minimap_lines(
        &self,
        id: DocumentId,
        lines: Range<usize>,
        max_column: usize,
    ) -> Result<Vec<Vec<MinimapRun>>, Error> {
        let tab_size = self.tab_size(id)?;
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        Ok(document.minimap_lines(lines, tab_size, max_column))
    }
}
