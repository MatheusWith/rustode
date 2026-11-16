//! One open document: its text and how it is stored on disk.

use rustode_core::Buffer;
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
    buffer: Buffer,
    encoding: Encoding,
    bom: bool,
}

impl Document {
    pub(crate) fn from_bytes(path: PathBuf, bytes: &[u8]) -> Self {
        let decoded = rustode_fs::decode(bytes);
        Self {
            path,
            buffer: Buffer::from_text(&decoded.text),
            encoding: decoded.encoding,
            bom: decoded.bom,
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
        self.buffer.text()
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
        rustode_fs::detect_eol(&self.buffer.text()).dominant
    }

    /// Whether the text mixes LF and CRLF.
    #[must_use]
    pub fn has_mixed_eol(&self) -> bool {
        rustode_fs::detect_eol(&self.buffer.text()).mixed
    }

    pub(crate) fn encoded(&self) -> Result<Vec<u8>, EncodeError> {
        rustode_fs::encode(&self.buffer.text(), self.encoding, self.bom)
    }

    pub(crate) fn set_encoding(
        &mut self,
        encoding: Encoding,
        bom: bool,
    ) -> Result<(), EncodeError> {
        rustode_fs::encode(&self.buffer.text(), encoding, bom)?;
        self.encoding = encoding;
        self.bom = bom && encoding != Encoding::Windows1252;
        Ok(())
    }

    pub(crate) fn set_eol(&mut self, eol: Eol) {
        self.buffer = Buffer::from_text(&rustode_fs::normalize_eol(&self.buffer.text(), eol));
    }
}
