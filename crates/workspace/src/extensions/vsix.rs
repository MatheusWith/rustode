//! Extracting a VSIX: only the entries under `extension/`, with that prefix
//! removed, as VS Code does (`extract(zipPath, tempLocation, { sourcePath:
//! 'extension' })`, `src/vs/platform/extensionManagement/node/extensionManagementService.ts:648`;
//! `src/vs/base/node/zip.ts:108-163`).
//!
//! An entry whose path would leave the target folder is refused and the
//! whole extraction fails, as VS Code's "Invalid file" check does
//! (`src/vs/base/node/zip.ts:76-81`); rustode also refuses `..`, empty and
//! backslash components outright. Files keep their permission bits
//! (`modeFromEntry`, `zip.ts:54-60`), with read and write for the owner
//! added so the folder can be managed later; symbolic links are written as
//! regular files, never followed. rustode also caps the entry count and the
//! extracted size (VS Code sets no cap).

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The folder of a VSIX that holds the extension.
const SOURCE: &str = "extension/";
/// Most entries extracted.
const MAX_ENTRIES: usize = 100_000;
/// Most bytes extracted in all.
const MAX_BYTES: u64 = 1 << 30;

/// The path of `name` inside the target folder, if it is safe: relative,
/// with no `.`, `..`, empty or backslash components.
pub(crate) fn safe_path(name: &str) -> Option<PathBuf> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty() || name.contains(['\\', '\0']) {
        return None;
    }
    let mut path = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

/// Extracts the `extension/` entries of `vsix` into `target`, which must
/// not exist yet. Returns the bytes written.
pub(crate) fn extract(vsix: &Path, target: &Path) -> Result<u64, String> {
    let file = File::open(vsix).map_err(|err| format!("{}: {err}", vsix.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| format!("not a VSIX: {err}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!(
            "the VSIX has {} entries; at most {MAX_ENTRIES} are extracted",
            archive.len()
        ));
    }
    fs::create_dir(target).map_err(|err| format!("{}: {err}", target.display()))?;
    let mut written = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("not a VSIX: {err}"))?;
        let name = entry
            .name()
            .map_err(|err| format!("not a VSIX: {err}"))?
            .into_owned();
        let Some(inner) = name.strip_prefix(SOURCE) else {
            continue;
        };
        if inner.is_empty() {
            continue;
        }
        let relative = safe_path(inner)
            .ok_or_else(|| format!("{name}: refused, the path leaves the extension folder"))?;
        let path = target.join(relative);
        if name.ends_with('/') {
            fs::create_dir_all(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        let mode = entry.unix_mode().map_or(0o644, |mode| mode & 0o777) | 0o600;
        let mut out = File::create(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let room = MAX_BYTES - written;
        let copied = io::copy(&mut (&mut entry).take(room + 1), &mut out)
            .map_err(|err| format!("{name}: {err}"))?;
        if copied > room {
            return Err(format!("the VSIX expands beyond {MAX_BYTES} bytes"));
        }
        written += copied;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|err| format!("{}: {err}", path.display()))?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_inside_the_folder_are_safe() {
        assert_eq!(safe_path("a/b.js"), Some(PathBuf::from("a/b.js")));
        assert_eq!(safe_path("dir/"), Some(PathBuf::from("dir")));
        for bad in [
            "",
            "/",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "a\\..\\b",
            "/etc/passwd",
            "a\0b",
        ] {
            assert_eq!(safe_path(bad), None, "{bad:?}");
        }
    }
}
