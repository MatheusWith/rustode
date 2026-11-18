//! Diagnostics as VS Code keeps them, as markers (Code - OSS, studied and
//! reimplemented; see `ATTRIBUTION.md`):
//!
//! - severities rank error, warning, info, hint
//!   (`src/vs/platform/markers/common/markers.ts:62-73`); a diagnostic
//!   without one is an error, a choice LSP leaves to the client;
//! - in an open document a marker moves with the text and never grows when
//!   typing at its edges, like VS Code's marker decorations
//!   (`src/vs/editor/common/services/markerDecorationsService.ts:324`,
//!   tracked as `src/vs/editor/common/model/intervalTree.ts:396-490` does);
//!   it keeps moving until the server publishes again;
//! - an empty range is drawn over the word at its position, or one char wide
//!   at a line's end, and a hint over its first two chars
//!   (`markerDecorationsService.ts:228-263`);
//! - the problems list leaves hints out
//!   (`src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts:500-502`)
//!   and orders files by their most severe problem, then by path, and each
//!   file's problems by severity, then position
//!   (`src/vs/workbench/contrib/markers/browser/markersModel.ts:24-35`,
//!   `:86-90`); VS Code compares paths with `localeCompare`, approximated
//!   here by comparing them case-insensitively first.

use crate::{Document, Position, Range};
use serde_json::Value;
use std::path::PathBuf;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkerSeverity {
    /// A hint: drawn, but not a problem.
    Hint,
    /// Information.
    Info,
    /// A warning.
    Warning,
    /// An error.
    Error,
}

impl MarkerSeverity {
    /// The severity of LSP's `DiagnosticSeverity` number.
    fn from_lsp(value: Option<u64>) -> Self {
        match value {
            Some(2) => Self::Warning,
            Some(3) => Self::Info,
            Some(4) => Self::Hint,
            _ => Self::Error,
        }
    }

    /// The name VS Code shows, e.g. `Error`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Hint => "Hint",
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
}

/// A diagnostic on an open document, where a frontend draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The chars to underline; empty at a line's end, drawn one char wide.
    pub range: Range,
    /// How serious it is.
    pub severity: MarkerSeverity,
    /// What the server says.
    pub message: String,
    /// Who says it, e.g. `rustc`.
    pub source: Option<String>,
    /// The server's code for it, e.g. `E0308`.
    pub code: Option<String>,
}

/// One row of the problems list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file.
    pub path: PathBuf,
    /// How serious it is: never a hint.
    pub severity: MarkerSeverity,
    /// What the server says.
    pub message: String,
    /// Who says it.
    pub source: Option<String>,
    /// The server's code for it.
    pub code: Option<String>,
    /// Where it starts, 0-based line and char column.
    pub start: Position,
    /// Where it ends.
    pub end: Position,
}

/// How many problems of each kind there are, as the status bar shows them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProblemCounts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
    /// Infos.
    pub infos: usize,
}

/// Where a stored marker is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// In an open document: char offsets that follow its edits.
    Offsets(Range),
    /// In a file that is not open: lines and columns. Columns count the
    /// server's code units as chars, which is exact for ASCII; they become
    /// offsets when the file opens.
    Lines(Position, Position),
}

/// A diagnostic as the workspace keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    pub severity: MarkerSeverity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
    pub anchor: Anchor,
}

impl Stored {
    /// An LSP `Diagnostic`; `anchor` places its start and end positions.
    pub(crate) fn from_lsp(diagnostic: &Value, anchor: impl Fn(&Value, &Value) -> Anchor) -> Self {
        let range = &diagnostic["range"];
        let code = match &diagnostic["code"] {
            Value::String(code) => Some(code.clone()),
            Value::Number(code) => Some(code.to_string()),
            _ => None,
        };
        Self {
            severity: MarkerSeverity::from_lsp(diagnostic["severity"].as_u64()),
            message: diagnostic["message"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            source: diagnostic["source"].as_str().map(str::to_owned),
            code,
            anchor: anchor(&range["start"], &range["end"]),
        }
    }

    /// The offsets in `doc`, however it is anchored.
    pub(crate) fn range_in(&self, doc: &Document) -> Range {
        match self.anchor {
            Anchor::Offsets(range) => {
                let len = doc.len_chars();
                Range::new(range.start.min(len), range.end.min(len))
            }
            Anchor::Lines(start, end) => {
                let start = doc.offset_of(start);
                Range::new(start, doc.offset_of(end).max(start))
            }
        }
    }

    /// The start and end positions, in `doc` when the file is open.
    pub(crate) fn positions(&self, doc: Option<&Document>) -> (Position, Position) {
        match (self.anchor, doc) {
            (Anchor::Lines(start, end), _) => (start, end),
            (Anchor::Offsets(_), Some(doc)) => {
                let range = self.range_in(doc);
                (doc.position_of(range.start), doc.position_of(range.end))
            }
            (Anchor::Offsets(range), None) => {
                (Position::new(0, range.start), Position::new(0, range.end))
            }
        }
    }

    pub(crate) fn marker(&self, doc: &Document) -> Marker {
        Marker {
            range: decoration(doc, self.range_in(doc), self.severity),
            severity: self.severity,
            message: self.message.clone(),
            source: self.source.clone(),
            code: self.code.clone(),
        }
    }

    pub(crate) fn problem(&self, path: &std::path::Path, doc: Option<&Document>) -> Problem {
        let (start, end) = self.positions(doc);
        Problem {
            path: path.to_owned(),
            severity: self.severity,
            message: self.message.clone(),
            source: self.source.clone(),
            code: self.code.clone(),
            start,
            end,
        }
    }
}

/// Where a marker is drawn: a hint over its first two chars, an empty range
/// over the word at its position unless it is past the line's content.
fn decoration(doc: &Document, range: Range, severity: MarkerSeverity) -> Range {
    let at = doc.position_of(range.start);
    let line: Vec<char> = doc.line(at.line).chars().collect();
    let line_start = range.start - at.column;
    let mut range = range;
    if severity == MarkerSeverity::Hint {
        range = Range::new(range.start, line_start + (at.column + 2).min(line.len()));
    }
    if !range.is_empty() {
        return range;
    }
    let content_end = line
        .iter()
        .rposition(|c| !c.is_whitespace())
        .map_or(0, |i| i + 1);
    if at.column >= content_end {
        return range;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = at.column;
    while start > 0 && is_word(line[start - 1]) {
        start -= 1;
    }
    let mut end = at.column;
    while end < line.len() && is_word(line[end]) {
        end += 1;
    }
    Range::new(line_start + start, line_start + end)
}

/// Sorts problems as VS Code's problems list does.
pub(crate) fn order(mut files: Vec<(PathBuf, Vec<Problem>)>) -> Vec<Problem> {
    let key = |p: &Problem| {
        (
            std::cmp::Reverse(p.severity),
            p.start.line,
            p.start.column,
            p.end.line,
            p.end.column,
        )
    };
    for (_, problems) in &mut files {
        problems.sort_by_key(key);
    }
    files.retain(|(_, problems)| !problems.is_empty());
    files.sort_by(|(a, pa), (b, pb)| {
        let worst = |problems: &[Problem]| problems.first().map(|p| std::cmp::Reverse(p.severity));
        worst(pa).cmp(&worst(pb)).then_with(|| {
            let (la, lb) = (a.to_string_lossy(), b.to_string_lossy());
            la.to_lowercase().cmp(&lb.to_lowercase()).then(la.cmp(&lb))
        })
    });
    files
        .into_iter()
        .flat_map(|(_, problems)| problems)
        .collect()
}
