//! Completion: the suggest widget's model. A language server's items are
//! kept for as long as the cursor stays in the word they were asked for;
//! typing narrows them with [`fuzzy_score`], and accepting one is one undo
//! step.
//!
//! Studied in VS Code and reimplemented, see `ATTRIBUTION.md`:
//!
//! - **Triggers** (`src/vs/editor/contrib/suggest/browser/suggestModel.ts`):
//!   Ctrl+Space (`editor.action.triggerSuggest`, `suggestController.ts:809`);
//!   a trigger character the server named, when
//!   `editor.suggestOnTriggerCharacters` is on (`:226-300`); and an
//!   identifier char typed, when `editor.quickSuggestions` allows it for the
//!   place (`:365-455`; the default is on in code, off in comments and
//!   strings). VS Code waits `editor.quickSuggestionsDelay`, 10 ms
//!   (`editorOptions.ts` `quickSuggestionsDelay`), which is not done: the
//!   request goes at once.
//! - **Filter and order** (`completionModel.ts:119-251`): the text from the
//!   start of the item's edit range to the cursor is the word; without one
//!   every item stays with a score of -100; else the word is scored against
//!   the item's `filterText` (or label) and items that do not match go.
//!   Items sort by score, highest first, then by the order the provider's
//!   list has after `defaultComparator` (`suggest.ts:339-356`: `sortText`
//!   lower-cased when both have one, then label, then kind).
//! - **Accept** (`suggestController.ts:850-856`): Enter or Tab replaces the
//!   edit range (the item's `textEdit` range, else the word before the
//!   cursor) with its text; the range's end follows the chars typed since
//!   the request. Up and Down wrap, Page Up and Page Down stop at the ends
//!   (`:950-970`); Escape ends the session (`:918`).
//!
//! Not done: snippet tab stops (the text of a snippet goes in with its
//! placeholders replaced by their defaults), `additionalTextEdits`,
//! commit characters, item resolve, sorting by recently used, and the
//! detail pane.

use super::filter::fuzzy_score;
use super::hover::contents;
use super::requests::{Kind, Request, lsp_position};
use crate::{DocumentId, Error, Event, Range, TokenKind, Workspace};
use rustode_config::Value as Setting;
use serde_json::{Value, json};
use std::cell::Cell;
use std::cmp::Reverse;

/// What an item is, in VS Code's order (`CompletionItemKind`,
/// `src/vs/editor/common/languages.ts:353-383`), which breaks ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)] // the variants are LSP's kind names
pub enum CompletionKind {
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Struct,
    Interface,
    Module,
    Property,
    Event,
    Operator,
    Unit,
    Value,
    Constant,
    Enum,
    EnumMember,
    Keyword,
    Text,
    Color,
    File,
    Reference,
    Folder,
    TypeParameter,
    Snippet,
}

impl CompletionKind {
    fn from_lsp(kind: Option<u64>) -> Self {
        match kind {
            Some(1) => Self::Text,
            Some(2) => Self::Method,
            Some(3) => Self::Function,
            Some(4) => Self::Constructor,
            Some(5) => Self::Field,
            Some(6) => Self::Variable,
            Some(7) => Self::Class,
            Some(8) => Self::Interface,
            Some(9) => Self::Module,
            Some(11) => Self::Unit,
            Some(12) => Self::Value,
            Some(13) => Self::Enum,
            Some(14) => Self::Keyword,
            Some(15) => Self::Snippet,
            Some(16) => Self::Color,
            Some(17) => Self::File,
            Some(18) => Self::Reference,
            Some(19) => Self::Folder,
            Some(20) => Self::EnumMember,
            Some(21) => Self::Constant,
            Some(22) => Self::Struct,
            Some(23) => Self::Event,
            Some(24) => Self::Operator,
            Some(25) => Self::TypeParameter,
            _ => Self::Property,
        }
    }

    /// The kind's name in lower case, e.g. `function`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Function => "function",
            Self::Constructor => "constructor",
            Self::Field => "field",
            Self::Variable => "variable",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Property => "property",
            Self::Event => "event",
            Self::Operator => "operator",
            Self::Unit => "unit",
            Self::Value => "value",
            Self::Constant => "constant",
            Self::Enum => "enum",
            Self::EnumMember => "enum member",
            Self::Keyword => "keyword",
            Self::Text => "text",
            Self::Color => "color",
            Self::File => "file",
            Self::Reference => "reference",
            Self::Folder => "folder",
            Self::TypeParameter => "type parameter",
            Self::Snippet => "snippet",
        }
    }

    /// One letter for the kind, where there is no room for its name.
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Self::Method | Self::Function | Self::Constructor => 'f',
            Self::Field | Self::Property => 'p',
            Self::Variable => 'v',
            Self::Class | Self::Struct => 'c',
            Self::Interface => 'i',
            Self::Module | Self::File | Self::Folder => 'm',
            Self::Constant | Self::EnumMember | Self::Enum => 'e',
            Self::Keyword => 'k',
            Self::Snippet => 's',
            Self::TypeParameter => 't',
            _ => 'o',
        }
    }
}

/// One item the server offered.
#[derive(Debug, Clone)]
struct Item {
    label: String,
    kind: CompletionKind,
    detail: Option<String>,
    documentation: Option<String>,
    /// `sortText`, lower-cased.
    sort: Option<String>,
    filter: Option<String>,
    /// What goes into the document.
    insert: String,
    /// The `textEdit` range, in chars of the document as it was asked.
    edit: Option<Range>,
}

/// What a request was asked about.
#[derive(Debug, Clone, Copy)]
struct Asked {
    version: u64,
    cursor: usize,
    start: usize,
}

/// A completion session: from the request that opened it to the accept or
/// dismissal that ends it.
#[derive(Debug)]
pub(crate) struct Session {
    document: DocumentId,
    /// What the items were asked about; the items' ranges are in this text.
    version: u64,
    cursor: usize,
    /// Where the word before the cursor started.
    start: usize,
    items: Vec<Item>,
    incomplete: bool,
    pub request: Option<Request>,
    asked: Asked,
    selected: usize,
    /// The first row the list showed.
    top: Cell<usize>,
}

/// One row of the suggest list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRow {
    /// What is shown and filtered.
    pub label: String,
    /// What the item is.
    pub kind: CompletionKind,
    /// A short description, e.g. a signature.
    pub detail: Option<String>,
    /// The label's chars the typed word matched, ascending.
    pub matched: Vec<usize>,
}

/// The suggest list as it shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionList {
    /// The document.
    pub document: DocumentId,
    /// Where the word being completed starts, as a char offset: the list is
    /// drawn under it.
    pub start: usize,
    /// The rows in view, from row `first`.
    pub rows: Vec<CompletionRow>,
    /// The index in the whole list of `rows[0]`.
    pub first: usize,
    /// The selected row, as an index in the whole list.
    pub selected: usize,
    /// How many items match.
    pub total: usize,
    /// The selected item's documentation as plain text, if it has one.
    pub documentation: Option<String>,
}

/// A way to move the selection in the suggest list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionMove {
    /// Down, from the last row to the first.
    Next,
    /// Up, from the first row to the last.
    Previous,
    /// Down by this many rows, stopping at the last.
    PageDown(usize),
    /// Up by this many rows, stopping at the first.
    PageUp(usize),
    /// To this row of the whole list (a click), stopping at the last.
    To(usize),
}

/// An item that matches the word, and how well.
struct Visible {
    index: usize,
    score: i32,
    matched: Vec<usize>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Workspace {
    /// Asks the document's language server for completions at the cursor, as
    /// Ctrl+Space does. The answer arrives with
    /// [`poll_language_servers`](Self::poll_language_servers) as
    /// [`Event::CompletionChanged`]; read the list with
    /// [`completion_list`](Self::completion_list). Typing also asks, by
    /// itself, when the settings and the server's trigger characters say so.
    /// Returns whether a request went out: `false` when the document has no
    /// running server that completes, or a selection is made.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn request_completion(&mut self, id: DocumentId) -> Result<bool, Error> {
        self.send_completion(id, 1, None)
    }

    /// `triggerKind` 1 invoked, 2 trigger character, 3 incomplete list.
    fn send_completion(
        &mut self,
        id: DocumentId,
        kind: u8,
        ch: Option<char>,
    ) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        if doc.selections().len() != 1 || !doc.primary_selection().is_empty() {
            return Ok(false);
        }
        let asked = Asked {
            version: doc.version(),
            cursor: doc.cursor(),
            start: word_start(doc, doc.cursor()),
        };
        let Some((server, uri, encoding)) = self.lsp_target(id, |f| f.completion.is_some()) else {
            return Ok(false);
        };
        let mut context = json!({"triggerKind": kind});
        if let Some(ch) = ch {
            context["triggerCharacter"] = json!(ch.to_string());
        }
        let params = json!({
            "textDocument": {"uri": uri},
            "position": lsp_position(doc, asked.cursor, encoding),
            "context": context,
        });
        let keep = kind == 3
            && self
                .lsp
                .suggest
                .as_ref()
                .is_some_and(|s| s.document == id && !s.items.is_empty());
        let previous = self.lsp.suggest.as_mut().and_then(|s| s.request.take());
        if let Some(previous) = previous {
            self.lsp_cancel(previous);
        }
        let Some(request) =
            self.lsp_request(server, Kind::Completion, "textDocument/completion", &params)
        else {
            return Ok(false);
        };
        if keep {
            if let Some(session) = self.lsp.suggest.as_mut() {
                session.request = Some(request);
                session.asked = asked;
            }
        } else {
            self.lsp.suggest = Some(Session {
                document: id,
                version: asked.version,
                cursor: asked.cursor,
                start: asked.start,
                items: Vec::new(),
                incomplete: false,
                request: Some(request),
                asked,
                selected: 0,
                top: Cell::new(0),
            });
        }
        Ok(true)
    }

    /// The suggest list: up to `limit` rows, scrolled so that the selected
    /// one is in view; `None` while no list is open or nothing matches the
    /// word before the cursor.
    #[must_use]
    pub fn completion_list(&self, limit: usize) -> Option<CompletionList> {
        let (session, visible) = self.visible()?;
        let limit = limit.max(1);
        let selected = session.selected.min(visible.len() - 1);
        let mut top = session.top.get().min(visible.len().saturating_sub(limit));
        if selected < top {
            top = selected;
        } else if selected >= top + limit {
            top = selected + 1 - limit;
        }
        session.top.set(top);
        let rows = visible[top..visible.len().min(top + limit)]
            .iter()
            .map(|v| {
                let item = &session.items[v.index];
                CompletionRow {
                    label: item.label.clone(),
                    kind: item.kind,
                    detail: item.detail.clone(),
                    matched: v.matched.clone(),
                }
            })
            .collect();
        Some(CompletionList {
            document: session.document,
            start: session.start,
            rows,
            first: top,
            selected,
            total: visible.len(),
            documentation: session.items[visible[selected].index].documentation.clone(),
        })
    }

    /// Whether a suggest list is showing: the keys Enter, Tab, Escape, Up,
    /// Down, Page Up and Page Down belong to it then.
    #[must_use]
    pub fn completion_visible(&self) -> bool {
        self.visible().is_some()
    }

    /// Whether a completion request is in flight.
    #[must_use]
    pub fn completion_pending(&self) -> bool {
        self.lsp
            .suggest
            .as_ref()
            .is_some_and(|s| s.request.is_some())
    }

    /// Moves the selection of the suggest list. Returns whether a list was
    /// showing.
    pub fn completion_move(&mut self, movement: CompletionMove) -> bool {
        let Some(total) = self.visible().map(|(_, v)| v.len()) else {
            return false;
        };
        if let Some(session) = self.lsp.suggest.as_mut() {
            let now = session.selected.min(total - 1);
            session.selected = match movement {
                CompletionMove::Next => (now + 1) % total,
                CompletionMove::Previous => (now + total - 1) % total,
                CompletionMove::PageDown(rows) => (now + rows).min(total - 1),
                CompletionMove::PageUp(rows) => now.saturating_sub(rows),
                CompletionMove::To(row) => row.min(total - 1),
            };
        }
        self.events.push(Event::CompletionChanged);
        true
    }

    /// Accepts the selected item: its text replaces the word before the
    /// cursor (or the item's own range) as one undo step, and the list
    /// closes. Returns whether an item was accepted.
    ///
    /// # Errors
    /// When the edit is refused.
    pub fn completion_accept(&mut self) -> Result<bool, Error> {
        let Some((id, range, text)) = self.accepted() else {
            return Ok(false);
        };
        self.dismiss_completion();
        self.replace(id, range, &text)?;
        Ok(true)
    }

    /// The document, range and text the selected item would put in.
    fn accepted(&self) -> Option<(DocumentId, Range, String)> {
        let (session, visible) = self.visible()?;
        let item = &session.items[visible[session.selected.min(visible.len() - 1)].index];
        let doc = self.document(session.document)?;
        let start = item.edit.map_or(session.start, |r| r.start);
        let end = item.edit.map_or(session.cursor, |r| r.end);
        let moved = doc.cursor().cast_signed() - session.cursor.cast_signed();
        let end = (end.cast_signed() + moved)
            .max(start.cast_signed())
            .cast_unsigned()
            .min(doc.len_chars());
        Some((
            session.document,
            Range::new(start, end),
            item.insert.clone(),
        ))
    }

    /// Closes the suggest list, and cancels a request in flight.
    pub fn dismiss_completion(&mut self) {
        let Some(session) = self.lsp.suggest.take() else {
            return;
        };
        if let Some(request) = session.request {
            self.lsp_cancel(request);
        }
        self.events.push(Event::CompletionChanged);
    }

    pub(super) fn dismiss_completion_of(&mut self, id: DocumentId) {
        if self.lsp.suggest.as_ref().is_some_and(|s| s.document == id) {
            self.dismiss_completion();
        }
    }

    pub(super) fn completion_answered(&mut self, request: Request, result: &Value) {
        let Some(session) = self
            .lsp
            .suggest
            .as_ref()
            .filter(|s| s.request == Some(request))
        else {
            return;
        };
        let (id, asked) = (session.document, session.asked);
        let current = self
            .document(id)
            .is_some_and(|d| d.version() == asked.version);
        let encoding = self.lsp.server(request.0).map(|s| s.capabilities.encoding);
        let parsed = encoding
            .filter(|_| current)
            .and_then(|e| self.parse_completion(id, result, e));
        let Some((items, incomplete)) = parsed.filter(|(items, _)| !items.is_empty()) else {
            self.lsp.suggest = None;
            self.events.push(Event::CompletionChanged);
            return;
        };
        if let Some(session) = self.lsp.suggest.as_mut() {
            session.items = items;
            session.incomplete = incomplete;
            session.request = None;
            session.version = asked.version;
            session.cursor = asked.cursor;
            session.start = asked.start;
            session.selected = 0;
        }
        self.events.push(Event::CompletionChanged);
    }

    fn parse_completion(
        &self,
        id: DocumentId,
        result: &Value,
        encoding: super::encoding::PositionEncoding,
    ) -> Option<(Vec<Item>, bool)> {
        let doc = self.document(id)?;
        let (list, incomplete) = match result {
            Value::Array(list) => (list.as_slice(), false),
            Value::Object(object) => (
                object.get("items")?.as_array()?.as_slice(),
                object.get("isIncomplete") == Some(&Value::Bool(true)),
            ),
            _ => return None,
        };
        let range = |range: &Value| {
            let start = super::offset(doc, &range["start"], encoding);
            Range::new(
                start,
                super::offset(doc, &range["end"], encoding).max(start),
            )
        };
        let mut items: Vec<Item> = list
            .iter()
            .filter_map(|raw| {
                let label = raw["label"].as_str()?.to_owned();
                let edit = raw.get("textEdit").filter(|e| e.is_object());
                let text = edit
                    .and_then(|e| e["newText"].as_str())
                    .or_else(|| raw["insertText"].as_str())
                    .unwrap_or(&label);
                let insert = if raw["insertTextFormat"].as_u64() == Some(2) {
                    strip_snippet(text)
                } else {
                    text.to_owned()
                };
                let edit_range = edit
                    .and_then(|e| e.get("range").or_else(|| e.get("insert")))
                    .map(range);
                let documentation = contents(&raw["documentation"]);
                Some(Item {
                    kind: CompletionKind::from_lsp(raw["kind"].as_u64()),
                    detail: raw["detail"].as_str().map(str::to_owned),
                    documentation: Some(documentation).filter(|d| !d.is_empty()),
                    sort: raw["sortText"].as_str().map(str::to_lowercase),
                    filter: raw["filterText"].as_str().map(str::to_owned),
                    label,
                    insert,
                    edit: edit_range,
                })
            })
            .collect();
        items.sort_by(|a, b| {
            let by_sort = match (&a.sort, &b.sort) {
                (Some(x), Some(y)) => x.cmp(y),
                _ => std::cmp::Ordering::Equal,
            };
            by_sort
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        Some((items, incomplete))
    }

    /// The session and its items that match the word, best first; `None`
    /// while the session has no items, the cursor left the word or a
    /// selection is made.
    fn visible(&self) -> Option<(&Session, Vec<Visible>)> {
        let session = self.lsp.suggest.as_ref().filter(|s| !s.items.is_empty())?;
        let doc = self.document(session.document)?;
        let cursor = doc.cursor();
        if !doc.primary_selection().is_empty() || !in_word(doc, session.start, cursor) {
            return None;
        }
        let position = doc.position_of(cursor);
        let line_start = cursor - position.column;
        let line: Vec<char> = doc.line(position.line).chars().collect();
        let mut visible = Vec::new();
        for (index, item) in session.items.iter().enumerate() {
            let start = item.edit.map_or(session.start, |r| r.start);
            if start > cursor || start < line_start {
                continue;
            }
            let word: String = line[start - line_start..position.column].iter().collect();
            let Some((score, matched)) = score(item, &word) else {
                continue;
            };
            visible.push(Visible {
                index,
                score,
                matched,
            });
        }
        visible.sort_by_key(|v| (Reverse(v.score), v.index));
        (!visible.is_empty()).then_some((session, visible))
    }

    /// After a document's text changed: its list is filtered again, or ends
    /// when the cursor left the word; an incomplete list is asked again.
    pub(super) fn completion_edited(&mut self, id: DocumentId) {
        let Some(session) = self.lsp.suggest.as_ref().filter(|s| s.document == id) else {
            return;
        };
        let valid = self
            .document(id)
            .is_some_and(|doc| in_word(doc, session.start, doc.cursor()));
        if !valid {
            self.dismiss_completion();
            return;
        }
        let retrigger = session.incomplete && !session.items.is_empty();
        if let Some(session) = self.lsp.suggest.as_mut() {
            session.selected = 0;
        }
        self.events.push(Event::CompletionChanged);
        if retrigger {
            let _ = self.send_completion(id, 3, None);
        }
    }

    /// After text was typed: asks for completions when a trigger character
    /// or an identifier char says so.
    pub(crate) fn lsp_typed(&mut self, id: DocumentId, text: &str) {
        let mut chars = text.chars();
        let (Some(typed), None) = (chars.next(), chars.next()) else {
            return;
        };
        let Some((server, ..)) = self.lsp_target(id, |f| f.completion.is_some()) else {
            return;
        };
        let triggers = self
            .lsp
            .server(server)
            .and_then(|s| s.features.completion.clone())
            .unwrap_or_default();
        if triggers.contains(&typed) && self.flag(id, "editor.suggestOnTriggerCharacters") {
            let _ = self.send_completion(id, 2, Some(typed));
            return;
        }
        if !is_word(typed) || !self.quick_suggestions(id) {
            return;
        }
        let version = self.document(id).map(crate::Document::version);
        let covered = self.lsp.suggest.as_ref().is_some_and(|s| {
            s.document == id
                && match s.request {
                    None => !s.items.is_empty(),
                    Some(_) => Some(s.asked.version) == version,
                }
        });
        if !covered {
            let _ = self.send_completion(id, 1, None);
        }
    }

    fn flag(&self, id: DocumentId, key: &str) -> bool {
        let language = self.document(id).map_or("", crate::Document::language_id);
        !matches!(
            self.config.language_value(language, key),
            Some(Setting::Bool(false))
        )
    }

    /// Whether typing at the cursor of `id` may open the list by itself:
    /// `editor.quickSuggestions`, whose default is on in code and off in
    /// comments and strings
    /// (`src/vs/editor/common/config/editorOptions.ts:3905-3960`).
    fn quick_suggestions(&self, id: DocumentId) -> bool {
        let Some(doc) = self.document(id) else {
            return false;
        };
        let setting = self
            .config
            .language_value(doc.language_id(), "editor.quickSuggestions");
        let on = |value: Option<&Setting>, default: bool| match value {
            Some(Setting::Bool(on)) => *on,
            Some(Setting::String(mode)) => mode == "on" || mode == "inline",
            _ => default,
        };
        let category = |name: &str, default: bool| match setting {
            Some(Setting::Object(object)) => on(object.get(name), default),
            other => on(other, default),
        };
        let cursor = doc.cursor();
        let line = doc.position_of(cursor).line;
        let kind = doc
            .highlights(line..line + 1)
            .into_iter()
            .find(|h| h.range.start < cursor && cursor <= h.range.end)
            .map(|h| h.kind);
        match kind {
            Some(TokenKind::Comment) => category("comments", false),
            Some(TokenKind::String | TokenKind::Escape) => category("strings", false),
            _ => category("other", true),
        }
    }
}

/// Where the identifier before `cursor` starts.
fn word_start(doc: &crate::Document, cursor: usize) -> usize {
    let position = doc.position_of(cursor);
    let line: Vec<char> = doc.line(position.line).chars().collect();
    let mut column = position.column.min(line.len());
    while column > 0 && is_word(line[column - 1]) {
        column -= 1;
    }
    cursor - (position.column.min(line.len()) - column)
}

/// Whether the chars from `start` to `cursor` are on one line and all
/// identifier chars: the cursor is still in the word the list was asked for.
fn in_word(doc: &crate::Document, start: usize, cursor: usize) -> bool {
    if cursor < start {
        return false;
    }
    let (from, to) = (doc.position_of(start), doc.position_of(cursor));
    if from.line != to.line {
        return false;
    }
    doc.line(from.line)
        .chars()
        .skip(from.column)
        .take(to.column - from.column)
        .all(is_word)
}

/// How well `word` matches `item`, and which chars of its label: `None`
/// when it does not match. An empty word gives every item -100.
fn score(item: &Item, word: &str) -> Option<(i32, Vec<usize>)> {
    let blanks = word.chars().take_while(|c| matches!(c, ' ' | '\t')).count();
    if word.is_empty() || blanks >= word.chars().count() {
        return Some((-100, Vec::new()));
    }
    match &item.filter {
        None => fuzzy_score(word, &item.label, blanks).map(|m| (m.score, m.positions)),
        Some(filter) => {
            let matched = fuzzy_score(word, filter, blanks)?;
            let positions = if filter.eq_ignore_ascii_case(&item.label) {
                matched.positions
            } else {
                fuzzy_score(word, &item.label, blanks).map_or_else(Vec::new, |m| m.positions)
            };
            Some((matched.score, positions))
        }
    }
}

/// A snippet as plain text: tab stops go, placeholders and variables become
/// their defaults, choices their first option, escapes their char.
pub(super) fn strip_snippet(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    place(&chars, &mut at, &mut out, false);
    out
}

fn place(chars: &[char], at: &mut usize, out: &mut String, nested: bool) {
    while let Some(&c) = chars.get(*at) {
        *at += 1;
        match c {
            '}' if nested => return,
            '\\' if matches!(chars.get(*at), Some('$' | '}' | '\\')) => {
                out.push(chars[*at]);
                *at += 1;
            }
            '$' => dollar(chars, at, out),
            _ => out.push(c),
        }
    }
}

/// What follows a `$`: a tab stop, a variable, or `${…}`.
fn dollar(chars: &[char], at: &mut usize, out: &mut String) {
    let name = |at: &mut usize| {
        while chars
            .get(*at)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            *at += 1;
        }
    };
    match chars.get(*at) {
        Some('{') => {
            *at += 1;
            name(at);
            match chars.get(*at) {
                Some(':') => {
                    *at += 1;
                    place(chars, at, out, true);
                }
                Some('|') => {
                    *at += 1;
                    while let Some(&c) = chars.get(*at) {
                        if matches!(c, ',' | '|') {
                            break;
                        }
                        out.push(c);
                        *at += 1;
                    }
                    while chars.get(*at).is_some_and(|c| *c != '}') {
                        *at += 1;
                    }
                    *at += 1;
                }
                _ => {
                    while chars.get(*at).is_some_and(|c| *c != '}') {
                        *at += 1;
                    }
                    *at += 1;
                }
            }
        }
        Some(c) if c.is_alphanumeric() || *c == '_' => name(at),
        _ => out.push('$'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_become_plain_text() {
        assert_eq!(
            strip_snippet("fmt(${1:value}, ${2|a,b|}$0)"),
            "fmt(value, a)"
        );
        assert_eq!(
            strip_snippet("for $1 in ${2:iter} {\n\t$0\n}"),
            "for  in iter {\n\t\n}"
        );
        assert_eq!(strip_snippet("cost \\$5 ${name} $x"), "cost $5  ");
        assert_eq!(strip_snippet("${1:a ${2:b}} end"), "a b end");
        assert_eq!(strip_snippet("price$"), "price$");
    }

    #[test]
    fn kinds_follow_vs_codes_order() {
        assert!(CompletionKind::Method < CompletionKind::Snippet);
        assert_eq!(CompletionKind::from_lsp(Some(22)), CompletionKind::Struct);
        assert_eq!(CompletionKind::from_lsp(None), CompletionKind::Property);
        assert_eq!(CompletionKind::Function.label(), "function");
        assert_eq!(CompletionKind::Struct.letter(), 'c');
    }

    #[test]
    fn items_without_a_word_keep_the_provider_order() {
        let item = |label: &str| Item {
            label: label.to_owned(),
            kind: CompletionKind::Text,
            detail: None,
            documentation: None,
            sort: None,
            filter: None,
            insert: label.to_owned(),
            edit: None,
        };
        assert_eq!(score(&item("abc"), ""), Some((-100, vec![])));
        assert_eq!(score(&item("abc"), "  "), Some((-100, vec![])));
        assert_eq!(score(&item("abc"), "zz"), None);
        let by_filter = Item {
            filter: Some("tuple".to_owned()),
            ..item("pair")
        };
        assert!(score(&by_filter, "tu").is_some(), "the filter text matches");
        assert_eq!(score(&by_filter, "pa"), None, "the label does not");
    }
}
