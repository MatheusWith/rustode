//! Scanning a buffer for the matches of a query, as VS Code's
//! `TextModelSearch` does (`src/vs/editor/common/model/textModelSearch.ts`).

use super::FindQuery;
use regex::{CaptureLocations, Regex};
use rustode_core::{Buffer, Range};
use std::ops::ControlFlow::{self, Break, Continue};

/// The default `editor.wordSeparators`
/// (`src/vs/editor/common/core/wordHelper.ts:10`).
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// How many lines are copied out of the buffer at once. Batches hold whole
/// lines, so the text a regex runs over is never cut short.
const BATCH_LINES: usize = 1024;

/// One match, while the text it was found in is at hand.
pub(super) struct Hit<'h> {
    /// Where the match is in the document, in chars.
    pub range: Range,
    haystack: &'h str,
    bytes: (usize, usize),
    groups: Option<&'h CaptureLocations>,
}

impl Hit<'_> {
    /// Group `index` of the match (0 is the whole match); `None` when the
    /// group took no part in it or was not captured.
    pub fn group(&self, index: usize) -> Option<&str> {
        match self.groups {
            Some(groups) => groups.get(index).map(|(s, e)| &self.haystack[s..e]),
            None => (index == 0).then(|| &self.haystack[self.bytes.0..self.bytes.1]),
        }
    }

    /// The number of groups, the whole match included.
    pub fn group_count(&self) -> usize {
        self.groups.map_or(1, CaptureLocations::len)
    }
}

/// Calls `f` with every match of `query` in `buffer`, in order, until it
/// breaks. With `groups`, hits carry their capture groups.
pub(super) fn each_match<T>(
    buffer: &Buffer,
    query: &FindQuery,
    groups: bool,
    mut f: impl FnMut(&Hit) -> ControlFlow<T>,
) -> ControlFlow<T> {
    let Some(mut scanner) = Scanner::for_query(query, groups) else {
        return Continue(());
    };
    if query.multiline {
        return Joined::new(buffer).hits(&mut scanner, &mut f);
    }
    each_line(buffer, 0..buffer.len_lines(), |line| {
        line.hits(&mut scanner, &mut f)
    })
}

/// Calls `f` with the byte range of every match of `query` in `text`, a
/// whole file, in order, until it breaks: the matches [`each_match`] finds
/// in a buffer holding `text`. A query that cannot match a line break runs
/// on each line; `candidates`, when given, finds the lines that may hold a
/// match first (see [`line_candidates`]), so the other lines are skipped.
pub(super) fn each_match_in_text(
    text: &str,
    query: &FindQuery,
    candidates: Option<&Regex>,
    f: &mut dyn FnMut(std::ops::Range<usize>) -> ControlFlow<()>,
) {
    let Some(mut scanner) = Scanner::for_query(query, false) else {
        return;
    };
    // The scanner's offsets are byte offsets in `text` here.
    let mut emit = |hit: &Hit| f(hit.range.start..hit.range.end);
    if query.multiline {
        if !text.contains('\r') {
            let _ = scanner.hits(text, |byte| byte, &mut emit);
            return;
        }
        let (joined, crlf) = joined_text(text);
        let _ = scanner.hits(
            &joined,
            |byte| byte + crlf.partition_point(|&lf| lf < byte),
            &mut emit,
        );
        return;
    }
    let mut from = 0;
    loop {
        let start = match candidates {
            Some(regex) => match regex.find_at(text, from) {
                Some(found) => line_start_before(text, from, found.start()),
                None => return,
            },
            None => from,
        };
        let (line, eol) = split_line(&text[start..]);
        if scanner
            .hits(line, |byte| start + byte, &mut emit)
            .is_break()
            || eol == 0
        {
            return;
        }
        from = start + line.len() + eol;
    }
}

/// A regex that finds, in a whole text, a match on every line where the
/// query matches per line (and maybe on others): the query's regex with
/// `^` and `$` at line breaks. `None` when that does not hold, for a regex
/// with `\A`, `\z` or inline flags.
pub(super) fn line_candidates(query: &FindQuery) -> Option<Regex> {
    if query.multiline || query.regex.is_none() {
        return None;
    }
    let pattern = &query.pattern;
    if query.options.regex {
        let inline_flags = pattern
            .match_indices("(?")
            .any(|(at, _)| !matches!(pattern.as_bytes().get(at + 2), Some(b':' | b'P' | b'<')));
        if inline_flags || pattern.contains("\\A") || pattern.contains("\\z") {
            return None;
        }
    }
    let source = if query.options.regex {
        pattern.clone()
    } else {
        regex::escape(pattern)
    };
    regex::RegexBuilder::new(&source)
        .case_insensitive(!query.options.match_case)
        .multi_line(true)
        .crlf(true)
        .build()
        .ok()
}

/// The byte offset where the line holding `at` starts, not before `from`.
fn line_start_before(text: &str, from: usize, at: usize) -> usize {
    let bytes = text.as_bytes();
    // A match at the LF of a CRLF belongs to the line before the break.
    let end = if at > from && bytes.get(at) == Some(&b'\n') && bytes[at - 1] == b'\r' {
        at - 1
    } else {
        at
    };
    bytes[from..end]
        .iter()
        .rposition(|&b| b == b'\n' || b == b'\r')
        .map_or(from, |break_at| from + break_at + 1)
}

/// `text` with every line break as one LF, and the byte offsets in it of
/// the LFs that stand for a CRLF, as [`Joined`] makes for a buffer.
fn joined_text(text: &str) -> (String, Vec<usize>) {
    let mut joined = String::with_capacity(text.len());
    let mut crlf = Vec::new();
    let mut rest = text;
    loop {
        let (line, eol) = split_line(rest);
        joined.push_str(line);
        if eol == 2 {
            crlf.push(joined.len());
        }
        if eol == 0 {
            return (joined, crlf);
        }
        joined.push('\n');
        rest = &rest[line.len() + eol..];
    }
}

/// The first match starting at or after `from`, or else the first match:
/// VS Code's `matchAfterPosition` over the matches it highlights
/// (`src/vs/editor/contrib/find/browser/findDecorations.ts:251-271`).
/// `pick` takes what the caller needs from the match.
pub(super) fn first_from<T>(
    buffer: &Buffer,
    query: &FindQuery,
    from: usize,
    groups: bool,
    mut pick: impl FnMut(&Hit) -> T,
) -> Option<T> {
    let mut scanner = Scanner::for_query(query, groups)?;
    if query.multiline {
        let mut first = None;
        let found = Joined::new(buffer).hits(&mut scanner, &mut |hit| {
            if hit.range.start >= from {
                return Break(pick(hit));
            }
            if first.is_none() {
                first = Some(pick(hit));
            }
            Continue(())
        });
        return found.break_value().or(first);
    }
    let line = buffer.line_of(from);
    let after = each_line(buffer, line..buffer.len_lines(), |line| {
        line.hits(&mut scanner, &mut |hit| {
            if hit.range.start >= from {
                Break(pick(hit))
            } else {
                Continue(())
            }
        })
    });
    if let Break(found) = after {
        return Some(found);
    }
    each_line(buffer, 0..line + 1, |line| {
        line.hits(&mut scanner, &mut |hit| Break(pick(hit)))
    })
    .break_value()
}

/// The last match ending at or before `before`, or else the last match:
/// VS Code's `matchBeforePosition` (`findDecorations.ts:229-249`).
pub(super) fn last_before(buffer: &Buffer, query: &FindQuery, before: usize) -> Option<Range> {
    let mut scanner = Scanner::for_query(query, false)?;
    if query.multiline {
        let (mut found, mut last) = (None, None);
        let _ = Joined::new(buffer).hits(&mut scanner, &mut |hit| {
            if hit.range.end <= before {
                found = Some(hit.range);
            } else if found.is_some() {
                return Break(());
            }
            last = Some(hit.range);
            Continue(())
        });
        return found.or(last);
    }
    let line = buffer.line_of(before);
    let back = each_line_rev(buffer, 0..line + 1, |line| {
        line.last_hit(&mut scanner, before)
    });
    if let Break(found) = back {
        return Some(found);
    }
    each_line_rev(buffer, line..buffer.len_lines(), |line| {
        line.last_hit(&mut scanner, usize::MAX)
    })
    .break_value()
}

/// VS Code's Find Next from `after` (`src/vs/editor/contrib/find/browser/findModel.ts:384-414`):
/// the first match from `after`; when that is an empty match right at
/// `after`, the first one from one char further, or from the next line
/// when at a line's end or when the regex has `^` or `$` (`:361-382`).
pub(super) fn next_match(buffer: &Buffer, query: &FindQuery, after: usize) -> Option<Range> {
    let found = first_from(buffer, query, after, false, |hit| hit.range)?;
    if !(found.is_empty() && found.start == after) {
        return Some(found);
    }
    let line = buffer.line_of(after);
    let further = if query.has_line_stops() || after >= line_end(buffer, line) {
        if line + 1 < buffer.len_lines() {
            buffer.line_start(line + 1)
        } else {
            0
        }
    } else {
        after + 1
    };
    first_from(buffer, query, further, false, |hit| hit.range)
}

/// VS Code's Find Previous from `before` (`findModel.ts:289-355`), stepping
/// back over an empty match right at `before` as `_prevSearchPosition`
/// does (`:267-287`).
pub(super) fn previous_match(buffer: &Buffer, query: &FindQuery, before: usize) -> Option<Range> {
    let found = last_before(buffer, query, before)?;
    if !(found.is_empty() && found.start == before) {
        return Some(found);
    }
    let line = buffer.line_of(before);
    let further = if query.has_line_stops() || before <= buffer.line_start(line) {
        line_end(
            buffer,
            line.checked_sub(1).unwrap_or(buffer.len_lines() - 1),
        )
    } else {
        before - 1
    };
    last_before(buffer, query, further)
}

/// The char offset of the end of `line`, before its line break.
fn line_end(buffer: &Buffer, line: usize) -> usize {
    buffer.line_start(line) + buffer.line_len(line)
}

/// The whole text with every line break as one LF, as VS Code searches
/// with a multiline query, so `\n` matches a CRLF and `\r` never matches
/// (`textModelSearch.ts:175-224`).
struct Joined {
    text: String,
    /// Byte offsets in `text` of the LFs that stand for a CRLF: each one
    /// before a match shifts it one char further in the document.
    crlf: Vec<usize>,
}

impl Joined {
    fn new(buffer: &Buffer) -> Self {
        let mut text = String::with_capacity(buffer.len_chars());
        let mut crlf = Vec::new();
        let _ = each_line::<()>(buffer, 0..buffer.len_lines(), |line| {
            text.push_str(line.text);
            if line.eol == 2 {
                crlf.push(text.len());
            }
            if line.eol > 0 {
                text.push('\n');
            }
            Continue(())
        });
        Self { text, crlf }
    }

    fn hits<T>(
        &self,
        scanner: &mut Scanner,
        f: &mut impl FnMut(&Hit) -> ControlFlow<T>,
    ) -> ControlFlow<T> {
        let mut chars = CharOffsets::new(&self.text);
        let crlf = &self.crlf;
        scanner.hits(
            &self.text,
            |byte| chars.at(byte) + crlf.partition_point(|&lf| lf < byte),
            f,
        )
    }
}

/// One line of a batch.
struct Line<'t> {
    /// Char offset of the line's start in the document.
    start: usize,
    /// The line without its line break.
    text: &'t str,
    /// The length of the line break: 2 for CRLF, 0 after the last line.
    eol: usize,
}

impl Line<'_> {
    fn hits<T>(
        &self,
        scanner: &mut Scanner,
        f: &mut impl FnMut(&Hit) -> ControlFlow<T>,
    ) -> ControlFlow<T> {
        let mut chars = CharOffsets::new(self.text);
        scanner.hits(self.text, |byte| self.start + chars.at(byte), f)
    }

    /// Breaks with the line's last match that ends at or before `before`.
    fn last_hit(&self, scanner: &mut Scanner, before: usize) -> ControlFlow<Range> {
        let mut last = None;
        let _ = self.hits(scanner, &mut |hit| {
            if hit.range.end > before {
                return Break(());
            }
            last = Some(hit.range);
            Continue(())
        });
        last.map_or(Continue(()), Break)
    }
}

/// Calls `f` with each line in `lines`, in order, until it breaks.
fn each_line<T>(
    buffer: &Buffer,
    lines: std::ops::Range<usize>,
    mut f: impl FnMut(Line) -> ControlFlow<T>,
) -> ControlFlow<T> {
    let (mut first, end) = (lines.start, lines.end);
    while first < end {
        let last = (first + BATCH_LINES).min(end);
        let (start, text) = batch(buffer, first, last);
        for line in BatchLines::new(&text, start, last - first) {
            f(line)?;
        }
        first = last;
    }
    Continue(())
}

/// Calls `f` with each line in `lines`, last first, until it breaks.
fn each_line_rev<T>(
    buffer: &Buffer,
    lines: std::ops::Range<usize>,
    mut f: impl FnMut(Line) -> ControlFlow<T>,
) -> ControlFlow<T> {
    let (start_line, mut last) = (lines.start, lines.end);
    while last > start_line {
        let first = last.saturating_sub(BATCH_LINES).max(start_line);
        let (start, text) = batch(buffer, first, last);
        let lines: Vec<Line> = BatchLines::new(&text, start, last - first).collect();
        for line in lines.into_iter().rev() {
            f(line)?;
        }
        last = first;
    }
    Continue(())
}

/// Copies lines `first..last` out of the buffer, line breaks included,
/// with the char offset where they start.
fn batch(buffer: &Buffer, first: usize, last: usize) -> (usize, String) {
    let start = buffer.line_start(first);
    let end = if last < buffer.len_lines() {
        buffer.line_start(last)
    } else {
        buffer.len_chars()
    };
    (start, buffer.slice(Range::new(start, end)))
}

/// The lines of a batch, in order.
struct BatchLines<'t> {
    rest: &'t str,
    start: usize,
    left: usize,
}

impl<'t> BatchLines<'t> {
    fn new(text: &'t str, start: usize, lines: usize) -> Self {
        Self {
            rest: text,
            start,
            left: lines,
        }
    }
}

impl<'t> Iterator for BatchLines<'t> {
    type Item = Line<'t>;

    fn next(&mut self) -> Option<Line<'t>> {
        self.left = self.left.checked_sub(1)?;
        let (text, eol) = split_line(self.rest);
        let line = Line {
            start: self.start,
            text,
            eol,
        };
        self.start += text.chars().count() + eol;
        self.rest = &self.rest[text.len() + eol..];
        Some(line)
    }
}

/// The first line of `text` and the length of its line break (CR, LF or
/// CRLF, as in the buffer; 0 for the last line).
fn split_line(text: &str) -> (&str, usize) {
    let bytes = text.as_bytes();
    match bytes.iter().position(|&b| b == b'\n' || b == b'\r') {
        Some(at) if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') => (&text[..at], 2),
        Some(at) => (&text[..at], 1),
        None => (text, 0),
    }
}

/// VS Code's `Searcher` over one text at a time (`textModelSearch.ts:494-555`):
/// matches are found left to right from the end of the previous one; an
/// empty match found twice moves the search on by one char; a match that
/// ends at the end of the text ends the search; with whole word, matches
/// not at word boundaries are skipped.
struct Scanner<'q> {
    regex: &'q Regex,
    whole_word: bool,
    groups: Option<CaptureLocations>,
}

impl<'q> Scanner<'q> {
    /// A scanner for `query`; `None` when it finds nothing (an empty pattern).
    fn for_query(query: &'q FindQuery, groups: bool) -> Option<Self> {
        let regex = query.regex.as_ref()?;
        Some(Self {
            regex,
            whole_word: query.options.whole_word,
            groups: groups.then(|| regex.capture_locations()),
        })
    }

    /// Calls `f` with each match in `text`, one line or the joined text;
    /// `offset` maps the increasing byte offsets of matches in `text` to
    /// char offsets in the document.
    fn hits<T>(
        &mut self,
        text: &str,
        mut offset: impl FnMut(usize) -> usize,
        f: &mut impl FnMut(&Hit) -> ControlFlow<T>,
    ) -> ControlFlow<T> {
        let mut at = 0;
        let mut previous = None;
        loop {
            if previous.is_some_and(|(_, end)| end == text.len()) {
                return Continue(());
            }
            let Some(found) = self.find(text, at) else {
                return Continue(());
            };
            if previous == Some(found) {
                let next = text[found.1..].chars().next();
                match next {
                    Some(c) if found.0 == found.1 => at = found.1 + c.len_utf8(),
                    _ => return Continue(()),
                }
                continue;
            }
            previous = Some(found);
            at = found.1;
            if self.whole_word && !is_whole_word(text, found) {
                continue;
            }
            let range = Range::new(offset(found.0), offset(found.1));
            f(&Hit {
                range,
                haystack: text,
                bytes: found,
                groups: self.groups.as_ref(),
            })?;
        }
    }

    fn find(&mut self, text: &str, at: usize) -> Option<(usize, usize)> {
        let found = match &mut self.groups {
            Some(groups) => self.regex.captures_read_at(groups, text, at),
            None => self.regex.find_at(text, at),
        };
        found.map(|m| (m.start(), m.end()))
    }
}

/// Char offsets of increasing byte offsets in one text, counted
/// incrementally so a long line with many matches is read once.
struct CharOffsets<'t> {
    text: &'t str,
    byte: usize,
    char: usize,
}

impl<'t> CharOffsets<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            byte: 0,
            char: 0,
        }
    }

    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            (self.byte, self.char) = (0, 0);
        }
        self.char += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.char
    }
}

/// VS Code's `isValidMatch` (`textModelSearch.ts:431-492`): each side of the
/// match is the text's edge, a separator, whitespace or a line break, or
/// the match itself starts (ends) with a separator or whitespace.
fn is_whole_word(text: &str, (start, end): (usize, usize)) -> bool {
    let matched = &text[start..end];
    let left = start == 0
        || text[..start].chars().next_back().is_some_and(breaks_word)
        || matched.chars().next().is_some_and(separates);
    let right = end == text.len()
        || text[end..].chars().next().is_some_and(breaks_word)
        || matched.chars().next_back().is_some_and(separates);
    left && right
}

/// A word separator or whitespace, as VS Code's `WordCharacterClassifier`
/// classifies chars (`src/vs/editor/common/core/wordCharacterClassifier.ts:25-40`).
fn separates(c: char) -> bool {
    c == ' ' || c == '\t' || WORD_SEPARATORS.contains(c)
}

/// What ends a word next to a match: a separator, whitespace or a line break.
fn breaks_word(c: char) -> bool {
    separates(c) || c == '\r' || c == '\n'
}
