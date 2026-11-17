//! The single in-process API that every frontend uses.

mod commands;
mod config;
mod document;
mod explorer;
pub mod extensions;
mod external;
mod find;
mod fuzzy;
mod language;
mod quick_open;
mod tabs;

pub use config::{
    Chord, Config, ConfigChange, ConfigFile, ContextValue, Diagnostic, DiagnosticKind, Keybinding,
    Location, SettingChange, Severity, Value, When,
};

pub use commands::{COMMANDS, CommandInfo, CommandMatch, CommandScope};
pub use document::{Document, DocumentId};
pub use explorer::{Explorer, ExplorerRow};
pub use extensions::{
    ExtensionError, ExtensionEvent, ExtensionOperation, ExtensionSearch, ExtensionSummary,
    InstalledExtension, NOT_LOADED, OPEN_VSX, default_extensions_dir,
};
pub use external::Resolution;
pub use find::{FindOptions, FindQuery, MATCHES_LIMIT, PatternError};
pub use fuzzy::ItemScore;
pub use quick_open::{FileMatch, QuickQuery, parse_quick_query};
pub use rustode_core::{EditError, Movement, Position, Range, Selection};
pub use rustode_fs::{Encoding, Eol};
/// Shared tracing and metrics setup, re-exported so frontends depend on the
/// workspace alone: call `trace::init_from_env()` at startup and record
/// input-to-frame latency with `trace::LatencyRecorder`.
pub use rustode_trace as trace;
pub use tabs::EditorGroup;

use rustode_core::Editor;

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a workspace operation failed.
#[derive(Debug)]
pub enum Error {
    /// The file could not be read or written.
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The document's text cannot be written in the requested encoding.
    Encode(rustode_fs::EncodeError),
    /// No open document has this id.
    UnknownDocument(DocumentId),
    /// An edit was refused.
    Edit(EditError),
    /// The document has unsaved changes; closing it needs an explicit discard.
    Dirty(DocumentId),
    /// The file changed on disk under unsaved edits; resolve the conflict
    /// with [`Workspace::resolve_conflict`] before saving.
    Conflict(DocumentId),
    /// [`Workspace::resolve_conflict`] was called with no conflict pending.
    NoConflict(DocumentId),
}

/// Something that happened in the workspace, for frontends to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// The text of a document changed (edit, undo or redo).
    DocumentChanged {
        /// The document.
        id: DocumentId,
    },
    /// A clean document was reloaded because its file changed on disk; it
    /// now matches the file. The reload is one undo step.
    Reloaded {
        /// The document.
        id: DocumentId,
    },
    /// The file of a dirty document changed on disk. The text is untouched;
    /// the frontend asks the user to [`Resolution::ReloadTheirs`] or
    /// [`Resolution::KeepMine`].
    Conflict {
        /// The document.
        id: DocumentId,
    },
    /// The file of a document was deleted. The text is kept; saving
    /// writes the file again.
    Deleted {
        /// The document.
        id: DocumentId,
    },
    /// The file of a document was renamed away. The document stays on its
    /// old path, as after a delete; `to` lets the frontend offer to follow.
    Renamed {
        /// The document.
        id: DocumentId,
        /// Where the file went.
        to: PathBuf,
    },
    /// The file of a deleted or renamed document is back on disk.
    Restored {
        /// The document.
        id: DocumentId,
    },
    /// Extension search, install, uninstall or listing results arrived
    /// ([`Workspace::poll_extensions`]).
    Extension(ExtensionEvent),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Encode(err) => err.fmt(f),
            Self::UnknownDocument(id) => write!(f, "no open document {id:?}"),
            Self::Edit(err) => err.fmt(f),
            Self::Dirty(id) => write!(f, "{id:?} has unsaved changes"),
            Self::Conflict(id) => write!(
                f,
                "{id:?} changed on disk under unsaved edits; reload or keep the edits first"
            ),
            Self::NoConflict(id) => write!(f, "{id:?} has no conflict to resolve"),
        }
    }
}

impl std::error::Error for Error {}

/// All open documents and the operations frontends perform on them.
#[derive(Debug, Default)]
pub struct Workspace {
    documents: Vec<Option<Document>>,
    events: Vec<Event>,
    /// Watches the files of open documents; `None` until the first open, or
    /// if the platform watcher could not be created.
    watcher: Option<rustode_fs::Watcher>,
    /// Settings and keybindings (`load_config`, `reload_config`).
    config: Config,
    /// The editor group and its tabs (one group until splits arrive).
    group: EditorGroup,
    /// The opened folder, if any.
    explorer: Option<Explorer>,
    /// The opened folder's files for quick open, listed on first use.
    files: Option<Vec<PathBuf>>,
    /// Extension management: folder, registry, background work, results.
    extensions: extensions::Extensions,
}

impl Workspace {
    /// An empty workspace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a file, detecting its encoding and line endings.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn open(&mut self, path: &Path) -> Result<DocumentId, Error> {
        if let Some(id) = self.find_open(path) {
            self.group.open(id);
            return Ok(id);
        }
        let bytes = rustode_fs::read_file(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        let document = Document::from_owned_bytes(path.to_owned(), bytes);
        tracing::debug!(path = %path.display(), encoding = document.encoding().label(), "opened");
        self.documents.push(Some(document));
        self.watch(path);
        let id = DocumentId(self.documents.len() - 1);
        self.group.open(id);
        Ok(id)
    }

    /// The open document for `path`, compared as absolute paths.
    fn find_open(&self, path: &Path) -> Option<DocumentId> {
        let wanted = std::path::absolute(path).ok()?;
        (0..self.documents.len()).map(DocumentId).find(|&id| {
            self.document(id)
                .and_then(|d| std::path::absolute(d.path()).ok())
                .is_some_and(|p| p == wanted)
        })
    }

    /// The tabs of the editor group, in display order.
    #[must_use]
    pub fn tabs(&self) -> &[DocumentId] {
        self.group.tabs()
    }

    /// The editor group.
    #[must_use]
    pub fn group(&self) -> &EditorGroup {
        &self.group
    }

    /// The document of the active tab.
    #[must_use]
    pub fn active_document(&self) -> Option<DocumentId> {
        self.group.active()
    }

    /// Makes `id`'s tab the active one.
    ///
    /// # Errors
    /// When the document has no tab.
    pub fn activate(&mut self, id: DocumentId) -> Result<(), Error> {
        if self.group.activate(id) {
            Ok(())
        } else {
            Err(Error::UnknownDocument(id))
        }
    }

    /// Activates the next tab, wrapping around (Ctrl+PageDown).
    pub fn next_tab(&mut self) {
        self.group.cycle(true);
    }

    /// Activates the previous tab, wrapping around (Ctrl+PageUp).
    pub fn previous_tab(&mut self) {
        self.group.cycle(false);
    }

    /// Activates the tab used before the active one: what a quick Ctrl+Tab
    /// does in VS Code, whose editor picker opens on the second most
    /// recently used editor.
    pub fn recently_used_tab(&mut self) {
        self.group.activate_recent(false);
    }

    /// Activates the tab used longest ago: what a quick Ctrl+Shift+Tab does
    /// in VS Code.
    pub fn least_recently_used_tab(&mut self) {
        self.group.activate_recent(true);
    }

    /// Closes a document and its tab; the most recently used tab becomes
    /// active. Unsaved changes are only dropped when `discard` is true.
    ///
    /// # Errors
    /// When there is no such document, or it is dirty and `discard` is false.
    pub fn close(&mut self, id: DocumentId, discard: bool) -> Result<(), Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        if document.is_dirty() && !discard {
            return Err(Error::Dirty(id));
        }
        let path = document.path().to_owned();
        if let Some(watcher) = self.watcher.as_mut()
            && let Err(err) = watcher.unwatch_file(&path)
        {
            tracing::debug!(path = %path.display(), %err, "cannot unwatch");
        }
        self.documents[id.0] = None;
        self.group.close(id);
        Ok(())
    }

    /// Opens a folder in the explorer, replacing any opened before.
    ///
    /// # Errors
    /// When the folder cannot be read.
    pub fn open_folder(&mut self, path: &Path) -> Result<(), Error> {
        let explorer = Explorer::open(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        self.explorer = Some(explorer);
        self.files = None;
        Ok(())
    }

    /// The opened folder's tree, if a folder is open.
    #[must_use]
    pub fn explorer(&self) -> Option<&Explorer> {
        self.explorer.as_ref()
    }

    /// Expands or collapses a folder of the explorer; returns whether it is
    /// now expanded.
    ///
    /// # Errors
    /// When no folder is open or the folder cannot be read.
    pub fn toggle_folder(&mut self, path: &Path) -> Result<bool, Error> {
        let explorer = self.explorer.as_mut().ok_or_else(|| Error::Io {
            path: path.to_owned(),
            source: std::io::Error::other("no folder is open"),
        })?;
        explorer.toggle(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
    }

    /// Reads the explorer's root and expanded folders again.
    pub fn refresh_explorer(&mut self) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.refresh();
        }
        self.files = None;
    }

    /// Moves the cursor to a 1-based line and column (column 1 when `None`),
    /// clamped to the text, as quick open's `:line:column` does.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn go_to_line(
        &mut self,
        id: DocumentId,
        line: usize,
        column: Option<usize>,
    ) -> Result<(), Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let position = Position::new(
            line.saturating_sub(1),
            column.unwrap_or(1).saturating_sub(1),
        );
        let offset = document.offset_of(position);
        self.set_cursor(id, offset)
    }

    /// An open document.
    #[must_use]
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.get(id.0).and_then(Option::as_ref)
    }

    fn document_mut(&mut self, id: DocumentId) -> Result<&mut Document, Error> {
        self.documents
            .get_mut(id.0)
            .and_then(Option::as_mut)
            .ok_or(Error::UnknownDocument(id))
    }

    /// Writes the document to its file in its encoding, BOM and line endings.
    ///
    /// # Errors
    /// When the text cannot be encoded or the file cannot be written, or
    /// while the file's external change is an unresolved conflict.
    pub fn save(&mut self, id: DocumentId) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        if document.has_conflict() {
            return Err(Error::Conflict(id));
        }
        let bytes = document.encoded().map_err(Error::Encode)?;
        let path = document.path().to_owned();
        rustode_fs::write_file(&path, &bytes).map_err(|source| Error::Io { path, source })?;
        document.mark_saved(&bytes);
        Ok(())
    }

    /// Changes the encoding used on save. Refused if the text cannot be represented.
    ///
    /// # Errors
    /// When a character has no representation in `encoding`.
    pub fn set_encoding(
        &mut self,
        id: DocumentId,
        encoding: Encoding,
        bom: bool,
    ) -> Result<(), Error> {
        self.document_mut(id)?
            .set_encoding(encoding, bom)
            .map_err(Error::Encode)
    }

    /// Rewrites every line ending of the document as `eol`.
    ///
    /// # Errors
    /// When no document has this id.
    pub fn set_eol(&mut self, id: DocumentId, eol: Eol) -> Result<(), Error> {
        let changed = self.document_mut(id)?.set_eol(eol).map_err(Error::Edit)?;
        self.changed(id, changed);
        Ok(())
    }

    /// Moves the cursor of a document. Ends the current undo step.
    ///
    /// # Errors
    /// When there is no such document or `offset` is past its end.
    pub fn set_cursor(&mut self, id: DocumentId, offset: usize) -> Result<(), Error> {
        self.edit(id, |editor| editor.set_cursor(offset).map(|()| false))
    }

    /// Replaces all selections of a document; `primary` indexes `list`.
    ///
    /// # Errors
    /// When there is no such document, `list` is empty or out of range.
    pub fn set_selections(
        &mut self,
        id: DocumentId,
        list: Vec<Selection>,
        primary: usize,
    ) -> Result<(), Error> {
        self.edit(id, |editor| {
            editor.set_selections(list, primary).map(|()| false)
        })
    }

    /// Adds a selection at the next occurrence of the primary selection
    /// (`Ctrl+D`); with an empty selection, selects the word first. Returns
    /// whether the selections changed.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn add_next_occurrence(&mut self, id: DocumentId) -> Result<bool, Error> {
        self.select(id, Editor::add_next_occurrence)
    }

    /// Adds a cursor below each cursor. Returns whether one was added.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn add_cursor_below(&mut self, id: DocumentId) -> Result<bool, Error> {
        self.select(id, Editor::add_cursor_below)
    }

    /// Adds a cursor above each cursor. Returns whether one was added.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn add_cursor_above(&mut self, id: DocumentId) -> Result<bool, Error> {
        self.select(id, Editor::add_cursor_above)
    }

    /// Box selection from `from` to `to`.
    ///
    /// # Errors
    /// When there is no such document or a line does not exist.
    pub fn column_select(
        &mut self,
        id: DocumentId,
        from: Position,
        to: Position,
    ) -> Result<(), Error> {
        self.edit(id, |editor| editor.column_select(from, to).map(|()| false))
    }

    fn select(&mut self, id: DocumentId, f: fn(&mut Editor) -> bool) -> Result<bool, Error> {
        Ok(f(self.document_mut(id)?.editor_mut()))
    }

    /// Types `text` at the cursor, grouping undo steps the way VS Code does.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn type_text(&mut self, id: DocumentId, text: &str) -> Result<(), Error> {
        let text = self.with_document_eol(id, text)?;
        self.edit(id, |editor| {
            editor.type_text(&text);
            Ok(!text.is_empty())
        })
    }

    /// Moves every cursor of a document; with `select`, extends the
    /// selections instead (Shift). Ends the current undo step.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn move_cursor(
        &mut self,
        id: DocumentId,
        movement: Movement,
        select: bool,
    ) -> Result<(), Error> {
        self.edit(id, |editor| {
            editor.move_cursors(movement, select);
            Ok(false)
        })
    }

    /// Enter: a line break in the document's line ending, keeping the
    /// current line's indentation. Starts a new undo step that typing joins.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn enter(&mut self, id: DocumentId) -> Result<(), Error> {
        let eol = self.document_mut(id)?.eol().as_str();
        self.edit(id, |editor| {
            editor.insert_line_break(eol);
            Ok(true)
        })
    }

    /// Tab: spaces up to the next tab stop, or a tab char, following
    /// `editor.tabSize` and `editor.insertSpaces` (with the document
    /// language's overrides). Its own undo step.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn tab(&mut self, id: DocumentId) -> Result<(), Error> {
        let tab_size = self.tab_size(id)?;
        let language = self.document_mut(id)?.language_id();
        let insert_spaces = !matches!(
            self.config.language_value(language, "editor.insertSpaces"),
            Some(Value::Bool(false))
        );
        self.edit(id, |editor| {
            editor.insert_tab(tab_size, insert_spaces);
            Ok(true)
        })
    }

    /// The tab size for a document: `editor.tabSize` with its language's
    /// override, 4 by default. Frontends use it to draw tabs.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn tab_size(&self, id: DocumentId) -> Result<usize, Error> {
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        Ok(
            match self
                .config
                .language_value(document.language_id(), "editor.tabSize")
            {
                Some(Value::Number(n)) => setting_count(*n).unwrap_or(4),
                _ => 4,
            },
        )
    }

    /// The primary cursor as VS Code's status bar shows it: 1-based line, and
    /// a 1-based column where tabs advance to the next tab stop and every
    /// other char counts one (`CursorColumns.toStatusbarColumn`,
    /// `src/vs/editor/common/core/cursorColumns.ts:61-77`).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn status_line_column(&self, id: DocumentId) -> Result<(usize, usize), Error> {
        let tab_size = self.tab_size(id)?;
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let position = document.cursor_position();
        let prefix: String = document
            .line(position.line)
            .chars()
            .take(position.column)
            .collect();
        let column = prefix.chars().fold(0, |column, c| {
            if c == '\t' {
                (column / tab_size + 1) * tab_size
            } else {
                column + 1
            }
        });
        Ok((position.line + 1, column + 1))
    }

    /// Ctrl+Backspace: deletes back to the start of the previous word.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn delete_word_left(&mut self, id: DocumentId) -> Result<(), Error> {
        self.edit(id, |editor| Ok(editor.delete_word_left()))
    }

    /// Ctrl+Delete: deletes to the end of the next word.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn delete_word_right(&mut self, id: DocumentId) -> Result<(), Error> {
        self.edit(id, |editor| Ok(editor.delete_word_right()))
    }

    /// Escape: drops secondary cursors, or else collapses the selection.
    /// Returns whether the selections changed.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn cancel_selections(&mut self, id: DocumentId) -> Result<bool, Error> {
        self.select(id, Editor::cancel_selections)
    }

    /// Selects the whole text.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn select_all(&mut self, id: DocumentId) -> Result<(), Error> {
        self.edit(id, |editor| {
            editor.select_all();
            Ok(false)
        })
    }

    /// `text` with its line breaks in the document's line ending: VS Code's
    /// model keeps one line ending, so inserted text follows it.
    fn with_document_eol(&mut self, id: DocumentId, text: &str) -> Result<String, Error> {
        let eol = self.document_mut(id)?.eol();
        Ok(if text.contains(['\r', '\n']) {
            rustode_fs::normalize_eol(text, eol)
        } else {
            text.to_owned()
        })
    }

    /// Deletes the char before the cursor (Backspace).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn delete_left(&mut self, id: DocumentId) -> Result<(), Error> {
        self.edit(id, |editor| Ok(editor.delete_left()))
    }

    /// Deletes the char after the cursor (Delete).
    ///
    /// # Errors
    /// When there is no such document.
    pub fn delete_right(&mut self, id: DocumentId) -> Result<(), Error> {
        self.edit(id, |editor| Ok(editor.delete_right()))
    }

    /// Replaces `range` with `text` as its own undo step.
    ///
    /// # Errors
    /// When there is no such document or `range` is past its end.
    pub fn replace(&mut self, id: DocumentId, range: Range, text: &str) -> Result<(), Error> {
        let text = self.with_document_eol(id, text)?;
        self.edit(id, |editor| editor.replace(range, &text))
    }

    /// Undoes the last undo step. Returns whether there was one.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn undo(&mut self, id: DocumentId) -> Result<bool, Error> {
        let document = self.document_mut(id)?;
        let undone = document.editor_mut().undo();
        document.sync_eol();
        self.changed(id, undone);
        Ok(undone)
    }

    /// Redoes the last undone step. Returns whether there was one.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn redo(&mut self, id: DocumentId) -> Result<bool, Error> {
        let document = self.document_mut(id)?;
        let redone = document.editor_mut().redo();
        document.sync_eol();
        self.changed(id, redone);
        Ok(redone)
    }

    /// The events since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Runs `f` on a document's editor; `f` returns whether the text changed.
    fn edit(
        &mut self,
        id: DocumentId,
        f: impl FnOnce(&mut Editor) -> Result<bool, EditError>,
    ) -> Result<(), Error> {
        let changed = f(self.document_mut(id)?.editor_mut()).map_err(Error::Edit)?;
        self.changed(id, changed);
        Ok(())
    }

    fn changed(&mut self, id: DocumentId, changed: bool) {
        if changed {
            self.events.push(Event::DocumentChanged { id });
        }
    }
}

/// A count setting such as `editor.tabSize`: a whole number from 1 to 100,
/// VS Code's bounds (`src/vs/editor/common/config/editorConfigurationSchema.ts:25-31`).
fn setting_count(value: f64) -> Option<usize> {
    (1u32..=100)
        .find(|n| (f64::from(*n) - value).abs() < f64::EPSILON)
        .and_then(|n| usize::try_from(n).ok())
}
