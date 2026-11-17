//! One open document: its text and how it is stored on disk.

use rustode_core::{EditError, Editor, Range};
use rustode_fs::{EncodeError, Encoding, Eol};
use std::path::{Path, PathBuf};

/// Identifies an open document within a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub(crate) usize);

/// An open document.
///
/// Line endings are kept exactly as read, so saving an unchanged file
/// reproduces its bytes even when it mixes LF and CRLF.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    editor: Editor,
    encoding: Encoding,
    bom: bool,
    saved_version: u64,
}

impl Document {
    pub(crate) fn from_bytes(path: PathBuf, bytes: &[u8]) -> Self {
        let decoded = rustode_fs::decode(bytes);
        Self {
            path,
            editor: Editor::new(&decoded.text),
            encoding: decoded.encoding,
            bom: decoded.bom,
            saved_version: 0,
        }
    }

    /// The file this document belongs to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The whole text.
    #[must_use]
    pub fn text(&self) -> String {
        self.editor.buffer().text()
    }

    /// Length of the text in chars.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.editor.buffer().len_chars()
    }

    /// The cursor, as a char offset.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.editor.cursor()
    }

    /// Whether the text differs from the last saved (or opened) state.
    /// Undoing back to that state makes the document clean again.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.editor.version() != self.saved_version
    }

    pub(crate) fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved_version = self.editor.version();
    }

    /// The encoding used on save.
    #[must_use]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Whether a byte order mark is written on save.
    #[must_use]
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// The dominant line ending.
    #[must_use]
    pub fn eol(&self) -> Eol {
        rustode_fs::detect_eol(&self.text()).dominant
    }

    /// Whether the text mixes LF and CRLF.
    #[must_use]
    pub fn has_mixed_eol(&self) -> bool {
        rustode_fs::detect_eol(&self.text()).mixed
    }

    pub(crate) fn encoded(&self) -> Result<Vec<u8>, EncodeError> {
        rustode_fs::encode(&self.text(), self.encoding, self.bom)
    }

    pub(crate) fn set_encoding(
        &mut self,
        encoding: Encoding,
        bom: bool,
    ) -> Result<(), EncodeError> {
        rustode_fs::encode(&self.text(), encoding, bom)?;
        self.encoding = encoding;
        self.bom = bom && encoding != Encoding::Windows1252;
        Ok(())
    }

    /// Rewrites every line ending as `eol`, as one undoable edit. Returns
    /// whether the text changed.
    pub(crate) fn set_eol(&mut self, eol: Eol) -> Result<bool, EditError> {
        let text = self.text();
        let normalized = rustode_fs::normalize_eol(&text, eol);
        if normalized == text {
            return Ok(false);
        }
        self.editor
            .replace(Range::new(0, self.len_chars()), &normalized)
    }
}
