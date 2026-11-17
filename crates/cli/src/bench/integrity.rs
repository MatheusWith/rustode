//! The App integrity check of AGENTS.md §0.6, plus what `sha256sum -c`
//! cannot see: files the manifest does not list (the 2026-11-17 incident,
//! when App runs added Node compile-cache files to `01-app/`).

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// The result of checking one App folder against the manifest.
#[derive(Debug, Clone)]
pub(crate) struct Integrity {
    /// Entries in the manifest.
    pub listed: usize,
    /// What `sha256sum -c --quiet` printed (empty means every listed file matches).
    pub mismatches: Vec<String>,
    /// Files in the folder that the manifest does not list, as `./path`.
    pub unlisted: Vec<String>,
    /// Total size of the files in the folder.
    pub bytes: u64,
}

impl Integrity {
    /// Every listed file matches and nothing else is there.
    pub(crate) fn ok(&self) -> bool {
        self.mismatches.is_empty() && self.unlisted.is_empty()
    }

    /// One line: what matched and what did not.
    pub(crate) fn summary(&self) -> String {
        if self.ok() {
            format!(
                "ok: {} listed files match, 0 files outside the manifest",
                self.listed
            )
        } else {
            format!(
                "FAILED: {} sha256sum lines, {} files not in the manifest",
                self.mismatches.len(),
                self.unlisted.len()
            )
        }
    }

    /// The details of a failure, for an error message.
    pub(crate) fn problems(&self) -> String {
        let mut lines = Vec::new();
        if !self.mismatches.is_empty() {
            lines.push("sha256sum -c --quiet reported:".to_owned());
            lines.extend(self.mismatches.iter().take(20).map(|l| format!("  {l}")));
        }
        if !self.unlisted.is_empty() {
            lines.push(format!(
                "{} files are not in the manifest:",
                self.unlisted.len()
            ));
            lines.extend(self.unlisted.iter().take(20).map(|l| format!("  {l}")));
        }
        lines.join("\n")
    }

    /// The JSON form used in reports.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "ok": self.ok(),
            "listed": self.listed,
            "mismatches": self.mismatches,
            "unlisted": self.unlisted.len(),
            "unlisted_files": self.unlisted,
        })
    }
}

/// Checks `app_dir` against `manifest`, a `sha256sum` listing made from
/// inside the folder (`find . -type f -print0 | sort -z | xargs -0 sha256sum`).
pub(crate) fn check(app_dir: &Path, manifest: &Path) -> Result<Integrity, String> {
    let listed = read_manifest(manifest)?;
    let mut unlisted = Vec::new();
    let mut bytes = 0;
    walk(app_dir, ".", &listed, &mut unlisted, &mut bytes)
        .map_err(|err| format!("cannot list {}: {err}", app_dir.display()))?;
    unlisted.sort();
    let out = Command::new("sha256sum")
        .args(["-c", "--quiet"])
        .arg(manifest)
        .current_dir(app_dir)
        .output()
        .map_err(|err| format!("cannot run sha256sum: {err}"))?;
    let mut mismatches: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&out.stderr).lines())
        .map(str::to_owned)
        .collect();
    if !out.status.success() && mismatches.is_empty() {
        mismatches.push(format!("sha256sum exited with {}", out.status));
    }
    Ok(Integrity {
        listed: listed.len(),
        mismatches,
        unlisted,
        bytes,
    })
}

/// The files of `dir` that are not in `listed`, as `./path`.
pub(crate) fn unlisted(dir: &Path, manifest: &Path) -> Result<Vec<String>, String> {
    let listed = read_manifest(manifest)?;
    let mut unlisted = Vec::new();
    walk(dir, ".", &listed, &mut unlisted, &mut 0)
        .map_err(|err| format!("cannot list {}: {err}", dir.display()))?;
    unlisted.sort();
    Ok(unlisted)
}

fn read_manifest(manifest: &Path) -> Result<HashSet<String>, String> {
    let text = fs::read_to_string(manifest)
        .map_err(|err| format!("cannot read the manifest {}: {err}", manifest.display()))?;
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            listed_path(line).ok_or_else(|| {
                format!(
                    "{} line {} is not '<sha256>  <path>'",
                    manifest.display(),
                    i + 1
                )
            })
        })
        .collect()
}

/// The path of one `sha256sum` line; names with `\` or a newline start the
/// line with `\` and are escaped.
fn listed_path(line: &str) -> Option<String> {
    let (escaped, line) = match line.strip_prefix('\\') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let (hash, rest) = (line.get(..64)?, line.get(64..)?);
    let path = rest
        .strip_prefix("  ")
        .or_else(|| rest.strip_prefix(" *"))?;
    if !hash.bytes().all(|b| b.is_ascii_hexdigit()) || path.is_empty() {
        return None;
    }
    Some(if escaped {
        path.replace("\\n", "\n").replace("\\\\", "\\")
    } else {
        path.to_owned()
    })
}

/// Collects every non-folder entry under `dir` that is not in `listed`.
/// Symbolic links are not followed: a link is an entry the manifest must list.
fn walk(
    dir: &Path,
    rel: &str,
    listed: &HashSet<String>,
    unlisted: &mut Vec<String>,
    bytes: &mut u64,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = format!("{rel}/{}", entry.file_name().to_string_lossy());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&entry.path(), &name, listed, unlisted, bytes)?;
            continue;
        }
        *bytes += entry.metadata()?.len();
        if !listed.contains(&name) {
            unlisted.push(name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "39add6e5fa0e0f1e8a0ac67f3d22fa3b5cae90ed9c1ba33e6fe5f784c844f7f7";

    #[test]
    fn manifest_lines_give_their_paths() {
        assert_eq!(
            listed_path(&format!("{HASH}  ./bin/codium")).as_deref(),
            Some("./bin/codium")
        );
        assert_eq!(
            listed_path(&format!("{HASH} *./a b")).as_deref(),
            Some("./a b")
        );
        assert_eq!(
            listed_path(&format!("\\{HASH}  ./a\\nb")).as_deref(),
            Some("./a\nb")
        );
        assert_eq!(listed_path("not a manifest line"), None);
        assert_eq!(listed_path(&format!("{HASH}  ")), None);
    }
}
