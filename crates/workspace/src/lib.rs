//! The single in-process API that every frontend uses.

mod document;

pub use document::{Document, DocumentId};
pub use rustode_fs::{Encoding, Eol};

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Encode(err) => err.fmt(f),
            Self::UnknownDocument(id) => write!(f, "no open document {id:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// All open documents and the operations frontends perform on them.
#[derive(Debug, Default)]
pub struct Workspace {
    documents: Vec<Option<Document>>,
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
        let document = Document::from_bytes(path.to_owned(), &bytes);
        tracing::debug!(path = %path.display(), encoding = document.encoding().label(), "opened");
        self.documents.push(Some(document));
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
    /// When the text cannot be encoded or the file cannot be written.
    pub fn save(&mut self, id: DocumentId) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        let bytes = document.encoded().map_err(Error::Encode)?;
        let path = document.path().to_owned();
        rustode_fs::write_file(&path, &bytes).map_err(|source| Error::Io { path, source })
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
        self.document_mut(id)?.set_eol(eol);
        Ok(())
    }
}
