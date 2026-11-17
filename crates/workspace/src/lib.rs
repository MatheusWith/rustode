//! The single in-process API that every frontend uses.

mod config;
mod document;
mod external;

pub use config::{
    Chord, Config, ConfigChange, ConfigFile, Diagnostic, DiagnosticKind, Keybinding, Location,
    SettingChange, Severity, Value, When,
};

pub use document::{Document, DocumentId};
pub use external::Resolution;
pub use rustode_core::{EditError, Position, Range, Selection};
pub use rustode_fs::{Encoding, Eol};

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Encode(err) => err.fmt(f),
            Self::UnknownDocument(id) => write!(f, "no open document {id:?}"),
            Self::Edit(err) => err.fmt(f),
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
        let bytes = rustode_fs::read_file(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        let document = Document::from_owned_bytes(path.to_owned(), bytes);
        tracing::debug!(path = %path.display(), encoding = document.encoding().label(), "opened");
        self.documents.push(Some(document));
        self.watch(path);
        Ok(DocumentId(self.documents.len() - 1))
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
        self.edit(id, |editor| {
            editor.type_text(text);
            Ok(!text.is_empty())
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
        self.edit(id, |editor| editor.replace(range, text))
    }

    /// Undoes the last undo step. Returns whether there was one.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn undo(&mut self, id: DocumentId) -> Result<bool, Error> {
        let mut undone = false;
        self.edit(id, |editor| {
            undone = editor.undo();
            Ok(undone)
        })?;
        Ok(undone)
    }

    /// Redoes the last undone step. Returns whether there was one.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn redo(&mut self, id: DocumentId) -> Result<bool, Error> {
        let mut redone = false;
        self.edit(id, |editor| {
            redone = editor.redo();
            Ok(redone)
        })?;
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
