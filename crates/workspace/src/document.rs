//! One open document: its text and how it is stored on disk.

use rustode_core::{EditError, Editor, Range, Selection};
use rustode_fs::{EncodeError, Encoding, Eol};
use std::path::{Path, PathBuf};

/// Length and 64-bit hash of a file's bytes. Two different contents share a
/// stamp with negligible probability (about 2^-64 per comparison).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiskStamp {
    len: usize,
    hash: u64,
}

impl DiskStamp {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(bytes);
        Self {
            len: bytes.len(),
            hash,
        }
    }
}

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
    /// A fingerprint of the bytes last read from or written to the file:
    /// what "unchanged on disk" is measured against. A fingerprint rather
    /// than the bytes, so a large file is not held twice.
    pub(crate) disk: DiskStamp,
    /// Whether the file is gone from disk (deleted or renamed away).
    pub(crate) orphaned: bool,
    /// The file's bytes when it changed under unsaved edits, until resolved.
    pub(crate) conflict: Option<Vec<u8>>,
}

impl Document {
    /// Decodes a file's bytes, without copying valid UTF-8 (large files).
    pub(crate) fn from_owned_bytes(path: PathBuf, bytes: Vec<u8>) -> Self {
        let disk = DiskStamp::of(&bytes);
        Self::from_decoded(path, rustode_fs::decode_owned(bytes), disk)
    }

    fn from_decoded(path: PathBuf, decoded: rustode_fs::Decoded, disk: DiskStamp) -> Self {
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
            disk,
            orphaned: false,
            conflict: None,
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

    /// Records that the text is now what `bytes`, the file's content, holds.
    pub(crate) fn mark_saved(&mut self, bytes: &[u8]) {
        self.saved_version = self.editor.version();
        self.disk = DiskStamp::of(bytes);
        self.orphaned = false;
        self.conflict = None;
    }

    /// Makes no version of the text count as saved, so the document stays
    /// dirty until it is saved again, even if undone to its old state.
    pub(crate) fn forget_saved_version(&mut self) {
        self.saved_version = u64::MAX;
    }

    pub(crate) fn set_stored_encoding(&mut self, encoding: Encoding, bom: bool) {
        self.encoding = encoding;
        self.bom = bom;
    }

    /// Whether the file was deleted or renamed away since it was opened or
    /// saved. The text is kept; saving writes the file again.
    #[must_use]
    pub fn is_orphaned(&self) -> bool {
        self.orphaned
    }

    /// Whether the file changed on disk under unsaved edits and the user has
    /// not chosen yet which version to keep.
    #[must_use]
    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
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
