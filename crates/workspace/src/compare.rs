//! "Compare" for a disk conflict: the file on disk against the unsaved text,
//! line by line.
//!
//! The behavior follows VS Code (Code - OSS, studied and reimplemented; see
//! `ATTRIBUTION.md` and ADR 0034):
//!
//! - after a failed save, "Compare" is the first choice, and it opens a diff
//!   of the file's content against the model's, titled
//!   "{name} (in file) ↔ {name} (in {product}) - Resolve save conflict"
//!   (`ResolveSaveConflictAction`, command id
//!   `workbench.files.action.resolveConflict`, label "Compare":
//!   `src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:129-131`,
//!   `:247-256`);
//! - from that editor the user accepts the local changes ("Use your changes
//!   and overwrite file contents") or reverts them ("Discard your changes and
//!   revert to file contents"), which closes the comparison
//!   (`…/fileActions.contribution.ts:199-200`; handlers
//!   `…/textFileSaveErrorHandler.ts:410-448`). rustode's ids for those are
//!   [`OVERWRITE`](crate::external) and [`REVERT`](crate::external), the ones
//!   the prompt already runs.
//!
//! What is rustode's own: VS Code opens a diff editor on a virtual
//! document; rustode has no diff editor, so the workspace computes the diff
//! ([`ConflictCompare`]) and each frontend draws it read-only (the GUI side
//! by side, the terminal as one column of added and removed lines). The
//! command only checks that there is a conflict and tells the frontend
//! ([`Event::CompareConflict`]); it changes nothing.
//!
//! The diff is by lines, Myers' shortest edit script ([`myers`]). Line
//! endings are not compared (a trailing `\r` is dropped), so two texts that
//! differ only in them, in the final newline or in the encoding show no
//! changed line, and the model says so in [`ConflictCompare::notice`].
//!
//! **Limits** (hostile numbers, AGENTS.md §11.7.1): a side over
//! [`COMPARE_MAX_BYTES`] bytes or [`COMPARE_MAX_LINES`] lines, or texts that
//! differ in more than [`COMPARE_MAX_CHANGES`] lines, are not diffed. The
//! model then has no lines and a notice that says why and that Overwrite
//! and Revert still work. A line longer than [`COMPARE_LINE_CHARS`] chars is
//! matched whole but kept shortened, with `…`, for drawing; tabs and other
//! control chars are replaced by spaces there too.

mod myers;

use crate::{DocumentId, Error, Event, Workspace};
use myers::Op;
use std::collections::HashMap;

/// Largest side compared, in bytes of text.
pub const COMPARE_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Most lines on either side of a comparison.
pub const COMPARE_MAX_LINES: usize = 50_000;
/// Most lines added plus removed that a comparison will compute.
pub const COMPARE_MAX_CHANGES: usize = 2_000;
/// Longest line kept for drawing, in chars.
pub const COMPARE_LINE_CHARS: usize = 2_000;

/// What the title calls the editor.
const PRODUCT: &str = "rustode";

/// How a line of the comparison relates to the two texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// In both texts.
    Same,
    /// Only in the file on disk (what saving over it would lose).
    Removed,
    /// Only in the unsaved text (what saving would write).
    Added,
}

/// One line of the unified comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Whether the line is in both texts or one.
    pub kind: DiffKind,
    /// The line's number in the file on disk, counting from 1.
    pub disk: Option<usize>,
    /// The line's number in the unsaved text, counting from 1.
    pub mine: Option<usize>,
    /// The text, without its line ending, ready to draw: tabs expanded,
    /// control chars blanked, shortened to [`COMPARE_LINE_CHARS`] chars.
    pub text: String,
}

/// The file on disk against the unsaved text, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictCompare {
    /// The document in conflict.
    pub document: DocumentId,
    /// VS Code's title for the comparison.
    pub title: String,
    /// Every line of both texts once, in order; empty when the texts are
    /// too big to compare.
    pub lines: Vec<DiffLine>,
    /// Where each block of changed lines starts, as an index into `lines`.
    pub changes: Vec<usize>,
    /// Lines only in the unsaved text.
    pub added: usize,
    /// Lines only in the file on disk.
    pub removed: usize,
    /// A sentence to show instead of or besides the lines: why there is no
    /// diff, or that there is nothing to see.
    pub notice: Option<String>,
}

/// One side of a [`SideRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideCell {
    /// The line's number in its text, counting from 1.
    pub number: usize,
    /// The line, as in [`DiffLine::text`].
    pub text: String,
}

/// One row of the side-by-side comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideRow {
    /// The file on disk's line, if the row has one.
    pub disk: Option<SideCell>,
    /// The unsaved text's line, if the row has one.
    pub mine: Option<SideCell>,
    /// Whether the row is part of a change.
    pub changed: bool,
}

/// The comparison as rows with the file on the left and the unsaved text on
/// the right; a removed line and an added line next to each other share a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySide {
    /// The rows, in order.
    pub rows: Vec<SideRow>,
    /// Where each block of changed rows starts, as an index into `rows`.
    pub changes: Vec<usize>,
}

impl ConflictCompare {
    /// The lines as rows, the file on disk on the left.
    #[must_use]
    pub fn side_by_side(&self) -> SideBySide {
        let cell = |line: &DiffLine, number: Option<usize>| {
            number.map(|number| SideCell {
                number,
                text: line.text.clone(),
            })
        };
        let mut rows = Vec::new();
        let mut changes = Vec::new();
        let mut index = 0;
        while index < self.lines.len() {
            let line = &self.lines[index];
            if line.kind == DiffKind::Same {
                rows.push(SideRow {
                    disk: cell(line, line.disk),
                    mine: cell(line, line.mine),
                    changed: false,
                });
                index += 1;
                continue;
            }
            let end = self.lines[index..]
                .iter()
                .position(|l| l.kind == DiffKind::Same)
                .map_or(self.lines.len(), |n| index + n);
            let block = &self.lines[index..end];
            let removed: Vec<_> = block
                .iter()
                .filter(|l| l.kind == DiffKind::Removed)
                .collect();
            let added: Vec<_> = block.iter().filter(|l| l.kind == DiffKind::Added).collect();
            changes.push(rows.len());
            for row in 0..removed.len().max(added.len()) {
                rows.push(SideRow {
                    disk: removed.get(row).and_then(|l| cell(l, l.disk)),
                    mine: added.get(row).and_then(|l| cell(l, l.mine)),
                    changed: true,
                });
            }
            index = end;
        }
        SideBySide { rows, changes }
    }
}

impl Workspace {
    /// The file on disk against the unsaved text of a document whose save
    /// hit a conflict.
    ///
    /// Texts over the limits ([`COMPARE_MAX_BYTES`], [`COMPARE_MAX_LINES`],
    /// [`COMPARE_MAX_CHANGES`]) are not an error: the result has no lines
    /// and a [`notice`](ConflictCompare::notice).
    ///
    /// # Errors
    /// When there is no such document, or it has no conflict.
    pub fn conflict_compare(&self, id: DocumentId) -> Result<ConflictCompare, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let Some(bytes) = document.conflict.as_ref() else {
            return Err(Error::NoConflict {
                id,
                name: document.name(),
            });
        };
        let name = document.name();
        let title = format!("{name} (in file) ↔ {name} (in {PRODUCT}) - Resolve save conflict");
        let mut compare = ConflictCompare {
            document: id,
            title,
            lines: Vec::new(),
            changes: Vec::new(),
            added: 0,
            removed: 0,
            notice: None,
        };
        // The raw size is checked before decoding, so a huge file costs one
        // length comparison.
        let notice = if bytes.len() > COMPARE_MAX_BYTES {
            Some(too_large(
                "the file on disk",
                &format!("{} bytes", bytes.len()),
                COMPARE_MAX_BYTES,
                "bytes",
            ))
        } else {
            let disk = rustode_fs::decode(bytes).text;
            let mine = document.text();
            compare.fill(&disk, &mine)
        };
        compare.notice = notice.or_else(|| {
            compare.changes.is_empty().then(|| {
                "The lines are the same; only line endings, the final newline or the encoding differ."
                    .to_owned()
            })
        });
        Ok(compare)
    }

    /// Runs `workbench.files.action.resolveConflict`: checks there is a
    /// conflict and asks the frontend to show the comparison.
    pub(crate) fn compare_conflict(&mut self, id: DocumentId) -> Result<(), Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        if !document.has_conflict() {
            return Err(Error::NoConflict {
                id,
                name: document.name(),
            });
        }
        self.events.push(Event::CompareConflict { id });
        Ok(())
    }
}

impl ConflictCompare {
    /// Diffs the two texts into `lines`; returns the notice when they are
    /// over a limit.
    fn fill(&mut self, disk: &str, mine: &str) -> Option<String> {
        for (side, text) in [("the file on disk", disk), ("your text", mine)] {
            if text.len() > COMPARE_MAX_BYTES {
                return Some(too_large(
                    side,
                    &format!("{} bytes", text.len()),
                    COMPARE_MAX_BYTES,
                    "bytes",
                ));
            }
            let count = count_lines(text);
            if count > COMPARE_MAX_LINES {
                return Some(too_large(
                    side,
                    &format!("{count} lines"),
                    COMPARE_MAX_LINES,
                    "lines",
                ));
            }
        }
        let (disk_lines, mine_lines) = (split_lines(disk), split_lines(mine));
        let mut ids = HashMap::new();
        let (old, new) = (intern(&mut ids, &disk_lines), intern(&mut ids, &mine_lines));
        let Some(ops) = myers::diff(&old, &new, COMPARE_MAX_CHANGES) else {
            return Some(format!(
                "The file and your text differ in too many lines to compare (the limit is \
                 {COMPARE_MAX_CHANGES} lines added or removed). Use Overwrite or Revert to \
                 resolve the conflict without comparing."
            ));
        };
        let (mut i, mut j) = (0, 0);
        let mut in_change = false;
        for op in ops {
            let (kind, disk_no, mine_no, text) = match op {
                Op::Keep => {
                    i += 1;
                    j += 1;
                    (DiffKind::Same, Some(i), Some(j), disk_lines[i - 1])
                }
                Op::Delete => {
                    i += 1;
                    self.removed += 1;
                    (DiffKind::Removed, Some(i), None, disk_lines[i - 1])
                }
                Op::Insert => {
                    j += 1;
                    self.added += 1;
                    (DiffKind::Added, None, Some(j), mine_lines[j - 1])
                }
            };
            let changed = kind != DiffKind::Same;
            if changed && !in_change {
                self.changes.push(self.lines.len());
            }
            in_change = changed;
            self.lines.push(DiffLine {
                kind,
                disk: disk_no,
                mine: mine_no,
                text: shorten(text),
            });
        }
        None
    }
}

/// Gives each distinct line an id, so the diff compares numbers.
fn intern<'a>(ids: &mut HashMap<&'a str, u32>, lines: &[&'a str]) -> Vec<u32> {
    lines
        .iter()
        .map(|line| {
            let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            *ids.entry(*line).or_insert(next)
        })
        .collect()
}

/// The notice for a side over a limit.
fn too_large(whose: &str, amount: &str, limit: usize, unit: &str) -> String {
    format!(
        "The file is too large to compare: {whose} has {amount} and the limit is {limit} {unit}. \
         Use Overwrite or Revert to resolve the conflict without comparing."
    )
}

/// How many lines [`split_lines`] will give, without building them.
fn count_lines(text: &str) -> usize {
    let breaks = text.bytes().filter(|b| *b == b'\n').count();
    if text.is_empty() || text.ends_with('\n') {
        breaks
    } else {
        breaks + 1
    }
}

/// The lines of `text` without their endings (`\n` or `\r\n`); a final
/// newline does not start another line.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// The line as kept for drawing: tabs become four spaces, other control
/// chars a space (they would break a terminal's rows), and a line over
/// [`COMPARE_LINE_CHARS`] chars is cut and ends with `…`.
fn shorten(line: &str) -> String {
    let mut shown = String::new();
    for (count, c) in line.chars().enumerate() {
        if count == COMPARE_LINE_CHARS {
            shown.push('…');
            break;
        }
        match c {
            '\t' => shown.push_str("    "),
            c if c.is_control() => shown.push(' '),
            c => shown.push(c),
        }
    }
    shown
}
