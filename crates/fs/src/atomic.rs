//! Atomic, durable file writes.
//!
//! [`write_atomic`] writes a temporary sibling of the target, created
//! exclusively under a unique name, gives it the target's permissions,
//! flushes it to disk, renames it over the target and, on Unix, flushes the
//! folder so the rename itself survives a power loss. Whatever step fails,
//! the temporary file is removed and the target is left as it was.
//!
//! A symbolic link is written through: the file it points to (after every
//! hop) is replaced, and the link stays a link. This follows what VS Code
//! does for a symbolic link, studied in its source tree and reimplemented
//! here: a document save writes without the `atomic` option
//! (`src/vs/workbench/services/textfile/common/textFileEditorModel.ts:1033-1038`),
//! atomic writes are refused for a link because the rename must stay on one
//! disk (`src/vs/platform/files/node/diskFileSystemProvider.ts:266-291`),
//! and the file is opened with `'w'`, which follows the link and truncates
//! its target (`diskFileSystemProvider.ts:458-464`). rustode keeps the
//! observable result (the target is written, the link is kept) and stays
//! atomic by putting the temporary file next to the resolved target, where
//! the rename cannot cross a disk.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How many links a path may go through before it is taken for a loop
/// (Linux's own limit, `MAXSYMLINKS`, is 40).
const MAX_LINK_HOPS: usize = 40;

/// How many unique names are tried before giving up on creating the
/// temporary file.
const MAX_NAME_ATTEMPTS: u64 = 64;

/// How many characters of the target's name the temporary name keeps, so a
/// long name cannot push the temporary one past the file system's limit.
const NAME_PREFIX_CHARS: usize = 32;

/// Distinguishes the temporary files of one process.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` atomically and durably; see the module comment.
///
/// # Errors
/// When the link chain cannot be followed (a loop included), the temporary
/// file cannot be created, written, given the target's permissions or
/// flushed, or the rename fails. The error does not carry the path; the
/// caller adds it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = resolve_links(path)?;
    let (temp, file) = create_temp(&target)?;
    let result = fill_and_rename(file, &temp, &target, bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result?;
    sync_folder(&target);
    Ok(())
}

/// Follows symbolic links from `path` to the file they finally point to,
/// which may not exist yet.
fn resolve_links(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_owned();
    for _ in 0..MAX_LINK_HOPS {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let next = std::fs::read_link(&current)?;
                current = match current.parent() {
                    Some(parent) if next.is_relative() => parent.join(next),
                    _ => next,
                };
            }
            // A regular file, a folder (the rename reports it) or nothing yet.
            Ok(_) => return Ok(current),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other(format!(
        "more than {MAX_LINK_HOPS} symbolic links to follow (a loop?)"
    )))
}

/// Creates a new, empty temporary file next to `target`, never reusing or
/// following anything already at its name.
fn create_temp(target: &Path) -> io::Result<(PathBuf, File)> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::other("the path has no file name"))?;
    let prefix: String = name
        .to_string_lossy()
        .chars()
        .take(NAME_PREFIX_CHARS)
        .collect();
    let pid = std::process::id();
    let mut last = None;
    for _ in 0..MAX_NAME_ATTEMPTS {
        let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = target.with_file_name(format!(".{prefix}.{pid}-{n}.rustode-save"));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => last = Some(err),
            Err(err) => return Err(err),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no free temporary name")))
}

/// Gives the temporary file the target's permissions, writes and flushes
/// it, and renames it over the target.
fn fill_and_rename(mut file: File, temp: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(meta) = std::fs::metadata(target) {
        file.set_permissions(meta.permissions())?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temp, target)
}

/// Flushes the folder holding `target`, so the rename is on disk. The file
/// is already in place, so a folder that cannot be flushed (some file
/// systems refuse) is logged, not reported as a failed save.
#[cfg(unix)]
fn sync_folder(target: &Path) {
    let folder = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if let Err(err) = File::open(folder).and_then(|dir| dir.sync_all()) {
        tracing::warn!(folder = %folder.display(), %err, "could not flush the folder after a save");
    }
}

#[cfg(not(unix))]
fn sync_folder(_target: &Path) {}
