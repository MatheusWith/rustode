//! Byte ranges of matches to lines, char columns and previews. Lines end
//! at CR, LF or CRLF, as in the editor's buffer, and columns count chars,
//! so a match opens at the same place in the document.

use super::{PREVIEW_BEFORE, PREVIEW_CHARS, TextMatch};
use std::ops::Range;

/// Locates `ranges`, sorted and apart, in `text`.
pub(super) fn locate(text: &str, ranges: &[Range<usize>]) -> Vec<TextMatch> {
    let mut cursor = Cursor::new(text);
    ranges
        .iter()
        .map(|range| {
            let start = cursor.advance(range.start);
            let line_start = cursor.line_start;
            let end = cursor.advance(range.end);
            let (preview, preview_column) = preview(text, line_start, range.start, start.1);
            TextMatch {
                start,
                end,
                preview,
                preview_column,
            }
        })
        .collect()
}

/// Walks forward through a text, keeping the line and column.
struct Cursor<'t> {
    bytes: &'t [u8],
    /// Byte offset reached.
    at: usize,
    line: usize,
    /// Byte offset where the current line starts.
    line_start: usize,
    /// Chars from the line start to `at`.
    column: usize,
}

impl<'t> Cursor<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            bytes: text.as_bytes(),
            at: 0,
            line: 0,
            line_start: 0,
            column: 0,
        }
    }

    /// Moves to byte offset `to` (at or after the current one) and returns
    /// its line and column.
    fn advance(&mut self, to: usize) -> (usize, usize) {
        while self.at < to {
            let byte = self.bytes[self.at];
            self.at += 1;
            match byte {
                b'\r' if self.bytes.get(self.at) == Some(&b'\n') => {}
                b'\r' | b'\n' => {
                    self.line += 1;
                    self.line_start = self.at;
                    self.column = 0;
                }
                // UTF-8 continuation bytes do not start a char.
                0x80..=0xBF => {}
                _ => self.column += 1,
            }
        }
        (self.line, self.column)
    }
}

/// The part of the line around a match: up to [`PREVIEW_BEFORE`] chars
/// before `start` and [`PREVIEW_CHARS`] from it, within the line. Returns
/// it with its char column.
fn preview(text: &str, line_start: usize, start: usize, column: usize) -> (String, usize) {
    let before = text[line_start..start]
        .char_indices()
        .rev()
        .take(PREVIEW_BEFORE)
        .last()
        .map_or(start, |(at, _)| line_start + at);
    let skipped = column - text[before..start].chars().count();
    let after: String = text[start..]
        .chars()
        .take_while(|&c| c != '\n' && c != '\r')
        .take(PREVIEW_CHARS)
        .collect();
    (format!("{}{after}", &text[before..start]), skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_at_cr_lf_and_crlf_and_columns_count_chars() {
        let text = "a\r\nbé x\rc\nd x";
        let found = locate(text, &[7..8, 13..14]);
        assert_eq!(found[0].start, (1, 3));
        assert_eq!(found[0].end, (1, 4));
        assert_eq!(found[0].preview, "bé x");
        assert_eq!(found[1].start, (3, 2));
        assert_eq!(found[1].preview, "d x");
    }

    #[test]
    fn a_match_across_lines_ends_on_its_last_line() {
        let found = locate("ab\r\ncd", &[Range { start: 1, end: 5 }]);
        assert_eq!((found[0].start, found[0].end), ((0, 1), (1, 1)));
        assert_eq!(found[0].preview, "ab");
    }

    #[test]
    fn long_lines_keep_a_window_around_the_match() {
        let text = format!("{}needle{}", "x".repeat(500), "y".repeat(2000));
        let found = locate(
            &text,
            &[Range {
                start: 500,
                end: 506,
            }],
        );
        assert_eq!(found[0].preview_column, 500 - PREVIEW_BEFORE);
        assert_eq!(
            found[0].preview.chars().count(),
            PREVIEW_BEFORE + PREVIEW_CHARS
        );
        assert!(found[0].preview[PREVIEW_BEFORE..].starts_with("needle"));
    }
}
