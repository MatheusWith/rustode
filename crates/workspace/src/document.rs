//! One open document: its text and how it is stored on disk.

use rustode_core::{EditError, Editor, Range, Selection};
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
    /// Decodes a file's bytes, without copying valid UTF-8 (large files).
    pub(crate) fn from_owned_bytes(path: PathBuf, bytes: Vec<u8>) -> Self {
        Self::from_decoded(path, rustode_fs::decode_owned(bytes))
    }

    fn from_decoded(path: PathBuf, decoded: rustode_fs::Decoded) -> Self {
        let rustode_fs::Decoded {
            text,
            encoding,
            bom,
        } = decoded;
        Self {
            path,
            editor: Editor::new(&text),
            encoding,
            bom,
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

    /// All selections, sorted and never overlapping.
    #[must_use]
    pub fn selections(&self) -> &[Selection] {
        self.editor.selections()
    }

    /// The primary selection, the one a frontend reveals.
    #[must_use]
    pub fn primary_selection(&self) -> Selection {
        self.editor.primary()
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
