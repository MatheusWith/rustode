//! Find and replace in one document, with the options of VS Code's find
//! widget. The behavior was studied in VS Code and reimplemented here (see
//! `ATTRIBUTION.md`); the sources are cited where each part is implemented.

mod replace;
mod search;

use crate::{DocumentId, Eol, Error, Workspace};
use regex::{Regex, RegexBuilder};
use replace::ReplacePattern;
use rustode_core::{Buffer, Range, Selection};
use std::fmt;
use std::ops::ControlFlow::{Break, Continue};

/// VS Code highlights and counts at most this many matches; its find
/// widget then shows "19999+" (`src/vs/editor/contrib/find/browser/findModel.ts:81`,
/// `findWidget.ts:459-486`).
pub const MATCHES_LIMIT: usize = 19_999;

/// The find widget's toggles, with VS Code's keys
/// (`src/vs/editor/contrib/find/browser/findModel.ts:39-50`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Use Regular Expression (`Alt+R`).
    pub regex: bool,
    /// Match Case (`Alt+C`).
    pub match_case: bool,
    /// Match Whole Word (`Alt+W`).
    pub whole_word: bool,
}

/// What to find: a pattern and the find options, checked once so that
/// every search with it can run.
#[derive(Debug, Clone)]
pub struct FindQuery {
    pattern: String,
    options: FindOptions,
    /// `None` for an empty pattern, which finds nothing
    /// (`src/vs/editor/common/model/textModelSearch.ts:30-32`).
    regex: Option<Regex>,
    /// Whether the regex runs over the whole text joined with LF instead of
    /// over each line (`textModelSearch.ts:34-40`, `:169-172`).
    multiline: bool,
}

impl FindQuery {
    /// A query for `pattern`. Plain text matches literally; a regular
    /// expression follows the `regex` crate's syntax, where VS Code uses
    /// JavaScript's: look-around and backreferences are refused, and `\w`,
    /// `\d`, `\b` are Unicode-aware. Without Match Case, letters match in
    /// any case.
    ///
    /// # Errors
    /// When `options.regex` is on and `pattern` is not a valid regular
    /// expression. VS Code's find widget reports the same case as an input
    /// error (`src/vs/editor/contrib/find/browser/findWidget.ts:1005-1016`).
    pub fn new(pattern: &str, options: FindOptions) -> Result<Self, PatternError> {
        let regex = if pattern.is_empty() {
            None
        } else {
            Some(compile(pattern, options)?)
        };
        Ok(Self {
            pattern: pattern.to_owned(),
            options,
            regex,
            multiline: is_multiline(pattern, options.regex),
        })
    }

    /// The pattern as typed.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The options.
    #[must_use]
    pub fn options(&self) -> FindOptions {
        self.options
    }

    /// Whether moving on from an empty match goes to the next line rather
    /// than the next char: VS Code checks a regex for `^` or `$` anywhere
    /// (`src/vs/editor/contrib/find/browser/findModel.ts:268-271`).
    fn has_line_stops(&self) -> bool {
        self.options.regex && self.pattern.contains(['^', '$'])
    }
}

/// VS Code builds one regex for both kinds of query, escaping plain text,
/// adding `i` without Match Case and `m` to multiline queries
/// (`src/vs/base/common/strings.ts:210-240`).
fn compile(pattern: &str, options: FindOptions) -> Result<Regex, PatternError> {
    let source = if options.regex {
        pattern.to_owned()
    } else {
        regex::escape(pattern)
    };
    RegexBuilder::new(&source)
        .case_insensitive(!options.match_case)
        .multi_line(is_multiline(pattern, options.regex))
        .build()
        .map_err(|err| PatternError::new(&err))
}

/// VS Code's test for a query that can match a line break: plain text
/// holding a LF, or a regex holding a LF or the escapes `\n`, `\r` or `\W`
/// (`isMultilineRegexSource`, `src/vs/editor/common/model/textModelSearch.ts:69-99`).
fn is_multiline(pattern: &str, regex: bool) -> bool {
    if !regex {
        return pattern.contains('\n');
    }
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\n' => return true,
            '\\' if matches!(chars.next(), Some('n' | 'r' | 'W')) => return true,
            _ => {}
        }
    }
    false
}

/// A pattern that is not a valid regular expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    message: String,
}

impl PatternError {
    /// Keeps the one-line reason of a `regex` error, without the copy of
    /// the pattern it quotes.
    fn new(err: &regex::Error) -> Self {
        let text = err.to_string();
        let message = text
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("error: "))
            .unwrap_or(&text)
            .to_owned();
        Self { message }
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid regular expression: {}", self.message)
    }
}

impl std::error::Error for PatternError {}

/// A [`FindQuery`] as the matcher of a search across files: it finds what
/// find in a document would find in the file's text.
pub(crate) struct TextSearch {
    query: FindQuery,
    candidates: Option<Regex>,
}

impl TextSearch {
    pub(crate) fn new(query: FindQuery) -> Self {
        let candidates = search::line_candidates(&query);
        Self { query, candidates }
    }
}

impl rustode_fs::TextMatcher for TextSearch {
    fn find_matches(
        &self,
        text: &str,
        found: &mut dyn FnMut(std::ops::Range<usize>) -> std::ops::ControlFlow<()>,
    ) {
        search::each_match_in_text(text, &self.query, self.candidates.as_ref(), found);
    }
}

/// Every match of `query` in `buffer`, in order, with the text that replaces
/// it (the groups of a regex match read as `ReplacePattern` reads them).
/// The text keeps the line breaks as written in `replacement`.
pub(crate) fn replacements(
    buffer: &Buffer,
    query: &FindQuery,
    replacement: &str,
) -> Vec<(Range, String)> {
    let pattern = ReplacePattern::new(replacement, query.options.regex);
    let mut found = Vec::new();
    let _ = search::each_match::<()>(buffer, query, pattern.has_groups(), |hit| {
        found.push((hit.range, pattern.build(hit)));
        Continue(())
    });
    found
}

/// The text of a document, for searching.
fn editor_buffer(workspace: &Workspace, id: DocumentId) -> Result<&Buffer, Error> {
    let document = workspace.document(id).ok_or(Error::UnknownDocument(id))?;
    Ok(document.editor().buffer())
}

impl Workspace {
    /// Every match of `query` in a document, in order, at most
    /// [`MATCHES_LIMIT`], as VS Code highlights them
    /// (`src/vs/editor/contrib/find/browser/findModel.ts:216`).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn find_all(&self, id: DocumentId, query: &FindQuery) -> Result<Vec<Range>, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let mut found = Vec::new();
        let _ = search::each_match(document.editor().buffer(), query, false, |hit| {
            found.push(hit.range);
            if found.len() < MATCHES_LIMIT {
                Continue(())
            } else {
                Break(())
            }
        });
        Ok(found)
    }

    /// Find Next (`F3`, or `Enter` in the find input; `findController.ts:783-788`):
    /// selects the first match after the primary selection, wrapping around
    /// to the first match, and returns it. Other selections are dropped.
    /// Without a match nothing changes.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn find_next(&mut self, id: DocumentId, query: &FindQuery) -> Result<Option<Range>, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let after = document.primary_selection().range().end;
        let found = search::next_match(document.editor().buffer(), query, after);
        self.select_match(id, found)
    }

    /// Find Previous (`Shift+F3`, or `Shift+Enter` in the find input;
    /// `findController.ts:804-809`): selects the last match before the
    /// primary selection, wrapping around to the last match, and returns it.
    /// Other selections are dropped.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn find_previous(
        &mut self,
        id: DocumentId,
        query: &FindQuery,
    ) -> Result<Option<Range>, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let before = document.primary_selection().range().start;
        let found = search::previous_match(document.editor().buffer(), query, before);
        self.select_match(id, found)
    }

    /// Replace (`Ctrl+Shift+1`, or `Enter` in the replace input;
    /// `findController.ts:1154-1171`): when the primary selection is the next
    /// match, replaces it as one undo step and selects the match after it;
    /// otherwise only selects the next match. Returns whether a match was
    /// replaced. VS Code's `replace` (`src/vs/editor/contrib/find/browser/findModel.ts:481-505`),
    /// with the cursor left after the replacement when no match remains
    /// (`src/vs/editor/common/commands/replaceCommand.ts:28-32`).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn replace_one(
        &mut self,
        id: DocumentId,
        query: &FindQuery,
        replacement: &str,
    ) -> Result<bool, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let selection = document.primary_selection().range();
        let pattern = ReplacePattern::new(replacement, query.options.regex);
        let found = search::first_from(
            document.editor().buffer(),
            query,
            selection.start,
            pattern.has_groups(),
            |hit| (hit.range, pattern.build(hit)),
        );
        let Some((range, text)) = found else {
            return Ok(false);
        };
        if range != selection {
            self.select_match(id, Some(range))?;
            return Ok(false);
        }
        let text = with_eol(text, document.eol());
        let end = range.start + text.chars().count();
        let editor = self.document_mut(id)?.editor_mut();
        let changed = editor
            .replace_ranges(vec![(range, text)], Selection::cursor(end))
            .map_err(Error::Edit)?;
        self.changed(id, changed);
        let next = search::next_match(editor_buffer(self, id)?, query, end);
        self.select_match(id, next)?;
        Ok(true)
    }

    /// Replace All (`Ctrl+Alt+Enter`, or `Ctrl+Enter` in the replace input;
    /// `findController.ts:1176-1195`): replaces every match, however many,
    /// as one undo step, and returns how many there were
    /// (`src/vs/editor/contrib/find/browser/findModel.ts:515-530`, `:572-584`).
    /// Undo restores the text and every selection.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn replace_all(
        &mut self,
        id: DocumentId,
        query: &FindQuery,
        replacement: &str,
    ) -> Result<usize, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let pattern = ReplacePattern::new(replacement, query.options.regex);
        let eol = document.eol();
        let mut edits = Vec::new();
        let _ = search::each_match::<()>(
            document.editor().buffer(),
            query,
            pattern.has_groups(),
            |hit| {
                edits.push((hit.range, with_eol(pattern.build(hit), eol)));
                Continue(())
            },
        );
        let count = edits.len();
        if count == 0 {
            return Ok(0);
        }
        merge_touching(&mut edits);
        let after = track(
            document.editor().buffer(),
            document.primary_selection(),
            &edits,
        );
        let editor = self.document_mut(id)?.editor_mut();
        let changed = editor.replace_ranges(edits, after).map_err(Error::Edit)?;
        self.changed(id, changed);
        Ok(count)
    }

    /// Selects `found` as VS Code does, from its start to its end
    /// (`findModel.ts:255-265`).
    fn select_match(
        &mut self,
        id: DocumentId,
        found: Option<Range>,
    ) -> Result<Option<Range>, Error> {
        if let Some(range) = found {
            self.set_selections(id, vec![Selection::new(range.start, range.end)], 0)?;
        }
        Ok(found)
    }
}

/// `text` with its line breaks in the document's line ending, as VS Code's
/// buffer inserts text (`src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeTextBuffer.ts:276-287`).
fn with_eol(text: String, eol: Eol) -> String {
    if text.contains(['\r', '\n']) {
        rustode_fs::normalize_eol(&text, eol)
    } else {
        text
    }
}

/// Joins edits that touch into one, as VS Code's `ReplaceAllCommand` does
/// (`src/vs/editor/contrib/find/browser/replaceAllCommand.ts:41-59`).
fn merge_touching(edits: &mut Vec<(Range, String)>) {
    edits.dedup_by(|next, kept| {
        let touching = kept.0.end == next.0.start;
        if touching {
            kept.0.end = next.0.end;
            kept.1.push_str(&next.1);
        }
        touching
    });
}

/// Where VS Code's Replace All leaves the primary selection, the only one
/// it keeps (`src/vs/editor/contrib/find/browser/replaceAllCommand.ts:66-71`):
/// a selection never grows at its edges, and a cursor sticks to the char
/// before it at the end of a line and to the char after it elsewhere
/// (`src/vs/editor/common/cursor/cursor.ts:940-961`). `edits` are sorted and
/// apart; like VS Code, they are applied from the last one.
fn track(buffer: &Buffer, selection: Selection, edits: &[(Range, String)]) -> Selection {
    let range = selection.range();
    let sticks = if range.is_empty() {
        let line = buffer.line_of(range.start);
        let at_line_end = range.start == buffer.line_start(line) + buffer.line_len(line);
        (at_line_end, at_line_end)
    } else {
        (false, true)
    };
    let (mut start, mut end) = (range.start, range.end);
    for (edit, text) in edits.iter().rev() {
        let inserted = text.chars().count();
        start = follow(start, sticks.0, *edit, inserted);
        end = follow(end, sticks.1, *edit, inserted).max(start);
    }
    if selection.head < selection.anchor {
        Selection::new(end, start)
    } else {
        Selection::new(start, end)
    }
}

/// Where one end of a tracked range goes when `edit` is replaced by
/// `inserted` chars, as VS Code's `nodeAcceptEdit` moves it
/// (`src/vs/editor/common/model/intervalTree.ts:396-490`): ends before the
/// edit stay; an end inside the part of the edit that is replaced char for
/// char stays; one at the edge of that part stays if it sticks to the char
/// before it (or always, when text was removed there); the rest of the
/// edit's ends go to the end of the new text; ends after it shift.
pub(crate) fn follow(offset: usize, sticks_left: bool, edit: Range, inserted: usize) -> usize {
    let removed = edit.len();
    if offset < edit.start || (offset == edit.start && (removed > 0 || sticks_left)) {
        return offset;
    }
    let common = edit.start + removed.min(inserted);
    if offset > edit.start
        && (offset < common || (offset == common && (removed > inserted || sticks_left)))
    {
        return offset;
    }
    if offset <= edit.end {
        return edit.start + inserted;
    }
    offset + inserted - removed
}
