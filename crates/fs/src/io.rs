//! Reading and writing whole files.

use std::io;
use std::path::Path;

/// Reads a file's bytes.
///
/// # Errors
/// When the file cannot be read.
pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    let _span = tracing::debug_span!("file_op", op = "read", path = %path.display()).entered();
    std::fs::read(path)
}

/// Writes a file atomically and durably: a unique temporary sibling, flushed
/// and renamed over the target, with the folder flushed on Unix. A failed
/// write never leaves a half-written file or a temporary file behind, the
/// file's permissions are kept, and a symbolic link is written through to
/// the file it points to (see `crate::atomic`).
///
/// # Errors
/// When the link chain cannot be followed, or the temporary file cannot be
/// created, written, flushed or renamed over the target.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let _span = tracing::debug_span!("file_op", op = "write", path = %path.display()).entered();
    crate::atomic::write_atomic(path, bytes)
}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The entry's name; names that are not valid UTF-8 are converted lossily.
    pub name: String,
    /// The entry's full path.
    pub path: std::path::PathBuf,
    /// Whether it is a directory (following symbolic links).
    pub is_dir: bool,
}

/// The entries of a directory, in no particular order. Entries that vanish
/// or cannot be read while listing are skipped.
///
/// # Errors
/// When the directory cannot be read.
pub fn list_dir(path: &Path) -> io::Result<Vec<DirEntry>> {
    let _span = tracing::debug_span!("file_op", op = "list", path = %path.display()).entered();
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let is_dir = std::fs::metadata(&path).is_ok_and(|m| m.is_dir());
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            is_dir,
        });
    }
    Ok(entries)
}
