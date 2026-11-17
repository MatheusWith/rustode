//! Problems found in a configuration file, with the place they were found.

use crate::jsonc::JsonError;
use std::fmt;

/// A place in a file: 1-based line and 1-based column, counted in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    /// The line, starting at 1.
    pub line: usize,
    /// The column, starting at 1, in Unicode scalar values.
    pub column: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// How serious a diagnostic is.
///
/// VS Code marks an unknown setting as a hint
/// (`src/vs/workbench/contrib/preferences/browser/preferencesRenderers.ts:742`), a value that does
/// not match the schema as a warning and a JSON syntax error as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The file cannot be read as intended.
    Error,
    /// A value is ignored.
    Warning,
    /// Informational; nothing is ignored.
    Hint,
}

/// What kind of problem a diagnostic describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The file is not valid JSON with comments.
    Syntax(JsonError),
    /// The file could not be read.
    Io,
    /// The setting is not in the schema; its value is kept as is.
    UnknownSetting,
    /// The value does not match the setting's schema and is ignored.
    InvalidValue,
    /// The same key appears twice in one object; the last one wins.
    DuplicateKey,
    /// A keybinding entry is malformed.
    InvalidKeybinding,
    /// A keybinding's `when` clause does not parse; the keybinding is ignored.
    InvalidWhen,
}

/// One problem in a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// What went wrong.
    pub kind: DiagnosticKind,
    /// How serious it is.
    pub severity: Severity,
    /// Where it starts.
    pub position: Location,
    /// A sentence for the user.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {:?}: {}",
            self.position, self.severity, self.message
        )
    }
}

/// Converts byte offsets into lines and columns.
pub(crate) struct LineIndex<'a> {
    text: &'a str,
    /// Byte offset where each line starts.
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Lines end at `\n`, `\r\n` or `\r`.
    pub(crate) fn new(text: &'a str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                    starts.push(i + 2);
                    i += 1;
                }
                b'\n' | b'\r' => starts.push(i + 1),
                _ => {}
            }
            i += 1;
        }
        Self { text, starts }
    }

    /// The position of a byte offset; offsets past the end map to the end.
    pub(crate) fn position(&self, offset: usize) -> Location {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.starts.partition_point(|&start| start <= offset);
        let start = self.starts[line - 1];
        Location {
            line,
            column: self.text[start..offset].chars().count() + 1,
        }
    }

    /// A diagnostic at a byte offset.
    pub(crate) fn diagnostic(
        &self,
        kind: DiagnosticKind,
        severity: Severity,
        offset: usize,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic {
            kind,
            severity,
            position: self.position(offset),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_lines_and_characters() {
        let index = LineIndex::new("ab\ncé\r\nx\ry");
        let at = |offset| {
            let p = index.position(offset);
            (p.line, p.column)
        };
        assert_eq!(at(0), (1, 1));
        assert_eq!(at(2), (1, 3));
        assert_eq!(at(3), (2, 1));
        assert_eq!(at(6), (2, 3), "é is one column");
        assert_eq!(at(8), (3, 1), "after \\r\\n");
        assert_eq!(at(10), (4, 1), "after a lone \\r");
        assert_eq!(at(99), (4, 2), "past the end");
    }
}
