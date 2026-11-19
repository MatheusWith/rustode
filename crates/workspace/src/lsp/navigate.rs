//! Go to definition and find references: where a language server says the
//! symbol under the cursor is declared or used.
//!
//! Studied in VS Code and reimplemented, see `ATTRIBUTION.md`: a symbol
//! navigation action asks its providers and then acts on how many locations
//! came back. None shows "No definition found for '{word}'"; one goes
//! straight there, selecting its range; several show a list to pick from
//! (VS Code's peek widget, a list of locations here)
//! (`src/vs/editor/contrib/gotoSymbol/browser/goToCommands.ts:105-150`, `:182-199`,
//! `:258-262`). Go to References lists even one location, as VS Code's
//! peek does. The locations are ordered by file and position, as VS Code's
//! `ReferencesModel` does.
//!
//! One request at a time: asking again cancels the one in flight. An answer
//! for a document that changed since the request is dropped. A location in a
//! file that is not open is converted to chars by reading the file (and its
//! line is the preview); a file over [`MAX_FILE`] bytes keeps the server's
//! numbers and no preview. A hostile answer is bounded by [`MAX_LOCATIONS`],
//! and a position outside its file is clamped to the file's end.

use super::encoding::PositionEncoding;
use super::requests::{Kind, Op, lsp_position};
use super::uri;
use crate::{Document, DocumentId, Error, Event, Position, Range, Selection, Workspace};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The most locations kept from one answer.
pub const MAX_LOCATIONS: usize = 2_000;
/// The biggest file read to convert and preview locations.
const MAX_FILE: u64 = 8 * 1024 * 1024;

/// What the list of locations answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationsKind {
    /// Several places claim to define the symbol.
    Definition,
    /// Where the symbol is used.
    References,
}

impl LocationsKind {
    /// The list's title.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Definition => "Definitions",
            Self::References => "References",
        }
    }
}

/// A place in a file, in chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLocation {
    /// The file.
    pub path: PathBuf,
    /// Where the range starts.
    pub start: Position,
    /// Where it ends.
    pub end: Position,
    /// The text of the first line of the range, trimmed; empty when the file
    /// could not be read.
    pub preview: String,
}

/// The locations a request found, to pick from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    /// What was asked.
    pub kind: LocationsKind,
    /// The word the request was about.
    pub word: String,
    /// The places, ordered by file and position.
    pub locations: Vec<SymbolLocation>,
}

impl Locations {
    /// The places whose file name or text contain `query` (case does not
    /// matter; all of them for an empty query), each with its index in
    /// [`locations`](Self::locations), which
    /// [`Workspace::pick_location`] takes.
    #[must_use]
    pub fn filtered(&self, query: &str) -> Vec<(usize, &SymbolLocation)> {
        let query = query.trim().to_lowercase();
        self.locations
            .iter()
            .enumerate()
            .filter(|(_, location)| {
                query.is_empty()
                    || location.preview.to_lowercase().contains(&query)
                    || location
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }
}

/// The symbol a rename would change: the word under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    /// Where the word is.
    pub range: Range,
    /// The word, the new name's starting text.
    pub text: String,
}

/// Whether `c` is part of a word.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word at char `offset` of `doc`: the one the offset is in or right
/// after, as VS Code's `getWordAtPosition` finds it.
pub(super) fn word_at(doc: &Document, offset: usize) -> Option<(Range, String)> {
    let position = doc.position_of(offset);
    let chars: Vec<char> = doc.line(position.line).chars().collect();
    let column = position.column.min(chars.len());
    let on = |i: usize| chars.get(i).copied().is_some_and(is_word);
    let anchor = if on(column) {
        column
    } else if column > 0 && on(column - 1) {
        column - 1
    } else {
        return None;
    };
    let mut start = anchor;
    while start > 0 && on(start - 1) {
        start -= 1;
    }
    let mut end = anchor;
    while on(end) {
        end += 1;
    }
    let text: String = chars[start..end].iter().collect();
    let line = position.line;
    Some((
        Range::new(
            doc.offset_of(Position::new(line, start)),
            doc.offset_of(Position::new(line, end)),
        ),
        text,
    ))
}

/// The lines of a file, to convert and preview its locations.
enum Lines<'a> {
    Open(&'a Document),
    Disk(Vec<String>),
    Unreadable,
}

impl Lines<'_> {
    fn line(&self, line: usize) -> Option<String> {
        match self {
            Self::Open(doc) => (line < doc.line_count()).then(|| doc.line(line)),
            Self::Disk(lines) => lines.get(line).cloned(),
            Self::Unreadable => None,
        }
    }

    fn count(&self) -> Option<usize> {
        match self {
            Self::Open(doc) => Some(doc.line_count()),
            Self::Disk(lines) => Some(lines.len()),
            Self::Unreadable => None,
        }
    }
}

/// An LSP position as the server sent it.
#[derive(Debug, Clone, Copy)]
struct Raw {
    line: u64,
    character: u64,
}

fn raw(value: &Value) -> Raw {
    Raw {
        line: value["line"].as_u64().unwrap_or(0),
        character: value["character"].as_u64().unwrap_or(0),
    }
}

/// A location as a server sent it: a file and a range.
struct Found {
    path: PathBuf,
    start: Raw,
    end: Raw,
}

/// The locations in a `SymbolLocation`, `SymbolLocation[]` or `LocationLink[]` result;
/// anything else is no location.
fn found(result: &Value) -> Vec<Found> {
    let items: Vec<&Value> = match result {
        Value::Array(list) => list.iter().collect(),
        Value::Object(_) => vec![result],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let (target, range) = if item.get("targetUri").is_some() {
                let range = item
                    .get("targetSelectionRange")
                    .filter(|r| r.is_object())
                    .unwrap_or(&item["targetRange"]);
                (&item["targetUri"], range)
            } else {
                (&item["uri"], &item["range"])
            };
            let path = uri::to_path(target.as_str()?)?;
            range.is_object().then(|| Found {
                path,
                start: raw(&range["start"]),
                end: raw(&range["end"]),
            })
        })
        .take(MAX_LOCATIONS)
        .collect()
}

impl Workspace {
    /// Asks the document's language server where the symbol at char `offset`
    /// is defined. The answer arrives with
    /// [`poll_language_servers`](Self::poll_language_servers): one place is
    /// gone to at once (the file opens, its range is selected), several come
    /// as [`Event::LocationsChanged`] to read with
    /// [`locations`](Self::locations), none as an [`Event::Notice`]. Returns
    /// whether a request went out: `false` when the document has no running
    /// server that offers definitions.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn request_definition(&mut self, id: DocumentId, offset: usize) -> Result<bool, Error> {
        self.request_locations(id, offset, LocationsKind::Definition)
    }

    /// Asks where the symbol at char `offset` is used, declaration included.
    /// The answer comes as [`Event::LocationsChanged`], or an
    /// [`Event::Notice`] when there is none. Returns whether a request went
    /// out.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn request_references(&mut self, id: DocumentId, offset: usize) -> Result<bool, Error> {
        self.request_locations(id, offset, LocationsKind::References)
    }

    fn request_locations(
        &mut self,
        id: DocumentId,
        offset: usize,
        kind: LocationsKind,
    ) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let (version, offset) = (doc.version(), offset.min(doc.len_chars()));
        let word = word_at(doc, offset)
            .map(|(_, word)| word)
            .unwrap_or_default();
        let Some((server, uri, encoding)) = self.lsp_target(id, |f| match kind {
            LocationsKind::Definition => f.definition,
            LocationsKind::References => f.references,
        }) else {
            return Ok(false);
        };
        self.cancel_navigation();
        let Some(doc) = self.document(id) else {
            return Ok(false);
        };
        let mut params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": lsp_position(doc, offset, encoding),
        });
        let (method, request_kind) = match kind {
            LocationsKind::Definition => ("textDocument/definition", Kind::Definition),
            LocationsKind::References => {
                params["context"] = serde_json::json!({"includeDeclaration": true});
                ("textDocument/references", Kind::References)
            }
        };
        let Some(request) = self.lsp_request(server, request_kind, method, &params) else {
            return Ok(false);
        };
        self.lsp.ops.insert(
            request,
            Op {
                document: id,
                version,
                word,
                name: String::new(),
            },
        );
        self.lsp.navigation = Some(request);
        Ok(true)
    }

    /// Cancels the navigation request in flight, if any.
    fn cancel_navigation(&mut self) {
        if let Some(request) = self.lsp.navigation.take() {
            self.lsp_cancel(request);
        }
    }

    /// The locations to pick from, after a request found several.
    #[must_use]
    pub fn locations(&self) -> Option<&Locations> {
        self.lsp.locations.as_ref()
    }

    /// Whether a definition or references request is in flight.
    #[must_use]
    pub fn navigation_pending(&self) -> bool {
        self.lsp.navigation.is_some()
    }

    /// Closes the list of locations, and cancels a request in flight.
    pub fn dismiss_locations(&mut self) {
        self.cancel_navigation();
        if self.lsp.locations.take().is_some() {
            self.events.push(Event::LocationsChanged);
        }
    }

    /// Goes to the `index`th location of the list, which closes: its file
    /// opens (or its tab is activated) and its range is selected. Returns
    /// the document, or `None` when there is no such location.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn pick_location(&mut self, index: usize) -> Result<Option<DocumentId>, Error> {
        let Some(location) = self
            .lsp
            .locations
            .as_ref()
            .and_then(|list| list.locations.get(index))
            .cloned()
        else {
            return Ok(None);
        };
        self.dismiss_locations();
        self.go_to_location(&location).map(Some)
    }

    /// Opens a location's file, or activates its tab, and selects its range.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn go_to_location(&mut self, location: &SymbolLocation) -> Result<DocumentId, Error> {
        let id = self.open(&location.path)?;
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let start = doc.offset_of(location.start);
        let end = doc.offset_of(location.end).max(start);
        self.set_selections(id, vec![Selection::new(start, end)], 0)?;
        self.events.push(Event::Navigated { id });
        Ok(id)
    }

    /// A definition or references answer arrived.
    pub(super) fn locations_answered(
        &mut self,
        request: super::requests::Request,
        kind: Kind,
        result: &Value,
        failure: Option<String>,
    ) {
        let Some(op) = self.lsp.ops.remove(&request) else {
            return;
        };
        if self.lsp.navigation == Some(request) {
            self.lsp.navigation = None;
        }
        let current = self
            .document(op.document)
            .is_some_and(|doc| doc.version() == op.version);
        if !current {
            return;
        }
        if let Some(why) = failure {
            self.notice(why);
            return;
        }
        let Some(encoding) = self.lsp.server(request.0).map(|s| s.capabilities.encoding) else {
            return;
        };
        let (kind, noun) = if kind == Kind::Definition {
            (LocationsKind::Definition, "definition")
        } else {
            (LocationsKind::References, "references")
        };
        let mut locations = self.locations_of(&found(result), encoding);
        locations.sort_by(|a, b| (&a.path, a.start, a.end).cmp(&(&b.path, b.start, b.end)));
        locations.dedup_by(|a, b| (&a.path, a.start, a.end) == (&b.path, b.start, b.end));
        match locations.len() {
            0 if op.word.is_empty() => self.notice(format!("No {noun} found")),
            0 => self.notice(format!("No {noun} found for '{}'", op.word)),
            1 if kind == LocationsKind::Definition => {
                if let Err(err) = self.go_to_location(&locations[0]) {
                    self.notice(err.to_string());
                }
            }
            _ => {
                self.lsp.locations = Some(Locations {
                    kind,
                    word: op.word,
                    locations,
                });
                self.events.push(Event::LocationsChanged);
            }
        }
    }

    /// `found` as locations in chars, with their previews.
    fn locations_of(&self, found: &[Found], encoding: PositionEncoding) -> Vec<SymbolLocation> {
        let mut files: HashMap<&Path, Lines<'_>> = HashMap::new();
        let mut locations = Vec::with_capacity(found.len());
        for item in found {
            let lines = files.entry(item.path.as_path()).or_insert_with(|| {
                if let Some(doc) = self.find_open(&item.path).and_then(|id| self.document(id)) {
                    Lines::Open(doc)
                } else {
                    read_lines(&item.path)
                }
            });
            let (start, preview) = position(lines, item.start, encoding);
            let (end, _) = position(lines, item.end, encoding);
            locations.push(SymbolLocation {
                path: item.path.clone(),
                start,
                end: end.max(start),
                preview,
            });
        }
        locations
    }

    /// Tells the frontend something in words: the end of a request that
    /// found or did nothing.
    pub(super) fn notice(&mut self, text: String) {
        self.events.push(Event::Notice(text));
    }
}

/// The lines of the file at `path`, when it is small enough and text.
fn read_lines(path: &Path) -> Lines<'static> {
    let small = std::fs::metadata(path).is_ok_and(|meta| meta.len() <= MAX_FILE);
    if !small {
        return Lines::Unreadable;
    }
    match rustode_fs::read_file(path) {
        Ok(bytes) => Lines::Disk(
            String::from_utf8_lossy(&bytes)
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                .collect(),
        ),
        Err(_) => Lines::Unreadable,
    }
}

/// The char position of `at` in `lines` and its line's trimmed text. A line
/// past the end is the end of the last one; without lines the server's
/// numbers stand.
fn position(lines: &Lines<'_>, at: Raw, encoding: PositionEncoding) -> (Position, String) {
    let Some(count) = lines.count() else {
        let number = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
        return (
            Position::new(number(at.line), number(at.character)),
            String::new(),
        );
    };
    let last = count.saturating_sub(1);
    let line = usize::try_from(at.line).map_or(last, |n| n.min(last));
    let text = lines.line(line).unwrap_or_default();
    let column = if usize::try_from(at.line).is_ok_and(|n| n > last) {
        text.chars().count()
    } else {
        encoding.column(&text, at.character)
    };
    (Position::new(line, column), text.trim().to_owned())
}
