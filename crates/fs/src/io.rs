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

/// Writes a file through a temporary sibling and a rename, so a failed write
/// never leaves a half-written file behind.
///
/// # Errors
/// When the temporary file cannot be written or renamed over `path`.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let _span = tracing::debug_span!("file_op", op = "write", path = %path.display()).entered();
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("the path has no file name"))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(".rustode-save");
    let temp = path.with_file_name(temp_name);
    std::fs::write(&temp, bytes)?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&temp, meta.permissions())?;
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
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
