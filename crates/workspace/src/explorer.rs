//! The explorer: an opened folder's tree, read lazily, filtered and sorted
//! as VS Code's explorer does by default.
//!
//! - Folders come before files, then names are compared with numbers taken
//!   as numbers (`file2` before `file10`), ignoring case first
//!   (`explorer.sortOrder: default` with `compareFileNamesDefault`,
//!   `src/vs/workbench/contrib/files/browser/views/explorerViewer.ts:1440-1545`,
//!   `src/vs/base/common/comparers.ts:54-59`, `:201-215`). VS Code uses the
//!   locale's ICU collation; this comparison approximates it.
//! - `files.exclude`'s default entries are hidden: `.git`, `.svn`, `.hg`,
//!   `.jj`, `.DS_Store`, `Thumbs.db`
//!   (`src/vs/workbench/contrib/files/browser/files.contribution.ts:154-160`).
//!   User patterns are not read yet.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const EXCLUDED: [&str; 6] = [".git", ".svn", ".hg", ".jj", ".DS_Store", "Thumbs.db"];

/// One visible row of the explorer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerRow {
    /// The entry's path.
    pub path: PathBuf,
    /// The entry's name.
    pub name: String,
    /// Nesting below the root: its children are at depth 0.
    pub depth: usize,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Whether the folder is expanded.
    pub expanded: bool,
}

/// An opened folder and the state of its tree.
#[derive(Debug, Clone)]
pub struct Explorer {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<rustode_fs::DirEntry>>,
}

impl Explorer {
    /// Opens `root`, reading its entries.
    pub(crate) fn open(root: &Path) -> std::io::Result<Self> {
        let mut explorer = Self {
            root: root.to_owned(),
            expanded: HashSet::new(),
            children: HashMap::new(),
        };
        explorer.load(root)?;
        Ok(explorer)
    }

    /// The opened folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The visible rows: the root's entries and those of expanded folders.
    #[must_use]
    pub fn rows(&self) -> Vec<ExplorerRow> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        rows
    }

    fn push_rows(&self, dir: &Path, depth: usize, rows: &mut Vec<ExplorerRow>) {
        for entry in self.children.get(dir).into_iter().flatten() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(ExplorerRow {
                path: entry.path.clone(),
                name: entry.name.clone(),
                depth,
                is_dir: entry.is_dir,
                expanded,
            });
            if expanded {
                self.push_rows(&entry.path, depth + 1, rows);
            }
        }
    }

    /// Expands or collapses the folder `dir`; returns whether it is expanded.
    pub(crate) fn toggle(&mut self, dir: &Path) -> std::io::Result<bool> {
        if self.expanded.remove(dir) {
            return Ok(false);
        }
        self.load(dir)?;
        self.expanded.insert(dir.to_owned());
        Ok(true)
    }

    /// Reads the root and every expanded folder again.
    pub(crate) fn refresh(&mut self) {
        let dirs: Vec<PathBuf> = std::iter::once(self.root.clone())
            .chain(self.expanded.iter().cloned())
            .collect();
        for dir in dirs {
            if let Err(err) = self.load(&dir) {
                tracing::debug!(dir = %dir.display(), %err, "cannot refresh");
                self.expanded.remove(&dir);
                self.children.remove(&dir);
            }
        }
    }

    fn load(&mut self, dir: &Path) -> std::io::Result<()> {
        let mut entries: Vec<_> = rustode_fs::list_dir(dir)?
            .into_iter()
            .filter(|e| !EXCLUDED.contains(&e.name.as_str()))
            .collect();
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| compare_names(&a.name, &b.name))
        });
        self.children.insert(dir.to_owned(), entries);
        Ok(())
    }
}

/// File names compared with digit runs as numbers and case ignored first;
/// ties go to lowercase, then to the shorter name (`file1` before `file01`).
fn compare_names(left: &str, right: &str) -> Ordering {
    let mut lhs = left.chars().peekable();
    let mut rhs = right.chars().peekable();
    loop {
        match (lhs.peek().copied(), rhs.peek().copied()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let order = number_order(&digits(&mut lhs), &digits(&mut rhs));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(l), Some(r)) => {
                let order = l.to_lowercase().cmp(r.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                lhs.next();
                rhs.next();
            }
        }
    }
    // Same letters and numbers: lowercase first, then shorter, then bytes.
    let case = left.chars().zip(right.chars()).find_map(|pair| {
        match (pair.0.is_lowercase(), pair.1.is_lowercase()) {
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            _ => None,
        }
    });
    case.unwrap_or(Ordering::Equal)
        .then(left.len().cmp(&right.len()))
        .then(left.cmp(right))
}

fn digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
        run.push(c);
        chars.next();
    }
    run
}

fn number_order(left: &str, right: &str) -> Ordering {
    let (left, right) = (left.trim_start_matches('0'), right.trim_start_matches('0'));
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_naturally() {
        let mut names = vec![
            "file10", "File2", "file1", "file01", "a", "B", ".env", "file2",
        ];
        names.sort_by(|a, b| compare_names(a, b));
        assert_eq!(
            names,
            [
                ".env", "a", "B", "file1", "file01", "file2", "File2", "file10"
            ]
        );
    }
}
