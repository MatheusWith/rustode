//! Quick open (Ctrl+P): files ranked by the fuzzy scorer, plus VS Code's
//! prefixes in the same box: `>` for commands and `:` for a line
//! (`src/vs/workbench/contrib/quickaccess/browser/commandsQuickAccess.ts`,
//! `src/vs/workbench/contrib/codeEditor/browser/quickaccess/gotoLineQuickAccess.ts`).
//! With an empty query it lists the open editors, most recently used first.

use crate::Workspace;
use crate::fuzzy::{ItemScore, score_item};
use std::path::{Path, PathBuf};

/// What the quick input text asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickQuery<'a> {
    /// Files whose names match.
    Files(&'a str),
    /// Commands whose labels match (`>` prefix).
    Commands(&'a str),
    /// A 1-based line and optional column (`:12`, `:12:3` or `:12,3`).
    Line {
        /// The line.
        line: usize,
        /// The column, if given.
        column: Option<usize>,
    },
    /// `:` followed by something that is not a line number.
    NoLine,
}

/// Reads the quick input text.
#[must_use]
pub fn parse_quick_query(input: &str) -> QuickQuery<'_> {
    if let Some(rest) = input.strip_prefix('>') {
        return QuickQuery::Commands(rest.trim_start());
    }
    let Some(rest) = input.strip_prefix(':') else {
        return QuickQuery::Files(input.trim());
    };
    let mut parts = rest.trim().splitn(2, [':', ',']);
    let line = parts.next().and_then(|l| l.trim().parse::<usize>().ok());
    let column = parts.next().and_then(|c| c.trim().parse::<usize>().ok());
    match line {
        Some(line) if line > 0 => QuickQuery::Line { line, column },
        _ => QuickQuery::NoLine,
    }
}

/// A file that matched a quick open query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    /// The file.
    pub path: PathBuf,
    /// Its name.
    pub label: String,
    /// Its folder, relative to the opened folder when inside it.
    pub description: String,
    /// The score and matched chars.
    pub score: ItemScore,
}

fn split(path: &Path, root: Option<&Path>) -> (String, String) {
    let shown = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
    let label = shown
        .file_name()
        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let description = shown
        .parent()
        .map_or_else(Default::default, |p| p.to_string_lossy().into_owned());
    (label, description)
}

impl Workspace {
    /// Files matching `query`, best first, at most `limit`. The opened
    /// folder's files are listed once and reused until
    /// [`refresh_explorer`](Self::refresh_explorer).
    pub fn quick_open(&mut self, query: &str, limit: usize) -> Vec<FileMatch> {
        let root = self.explorer().map(|e| e.root().to_owned());
        if query.trim().is_empty() {
            return self.recent_files(root.as_deref(), limit);
        }
        let mut candidates: Vec<PathBuf> = self.file_index().to_vec();
        for &id in self.group.recent() {
            if let Some(path) = self.document(id).map(|d| d.path().to_owned())
                && !candidates.contains(&path)
            {
                candidates.push(path);
            }
        }
        let mut matches: Vec<FileMatch> = candidates
            .into_iter()
            .filter_map(|path| {
                let (label, description) = split(&path, root.as_deref());
                let shown = if description.is_empty() {
                    label.clone()
                } else {
                    format!("{description}/{label}")
                };
                let score = score_item(&label, &description, &shown, query)?;
                Some(FileMatch {
                    path,
                    label,
                    description,
                    score,
                })
            })
            .collect();
        matches.sort_by(|a, b| {
            b.score
                .score
                .cmp(&a.score.score)
                .then(a.label.len().cmp(&b.label.len()))
                .then_with(|| a.path.cmp(&b.path))
        });
        matches.truncate(limit);
        matches
    }

    fn recent_files(&self, root: Option<&Path>, limit: usize) -> Vec<FileMatch> {
        self.group
            .recent()
            .iter()
            .filter_map(|&id| self.document(id))
            .take(limit)
            .map(|document| {
                let path = document.path().to_owned();
                let (label, description) = split(&path, root);
                FileMatch {
                    path,
                    label,
                    description,
                    score: ItemScore {
                        score: 0,
                        label: Vec::new(),
                        description: Vec::new(),
                    },
                }
            })
            .collect()
    }

    /// The opened folder's files as absolute paths, listed on first use.
    fn file_index(&mut self) -> &[PathBuf] {
        if self.files.is_none() {
            let files = self.explorer().map_or_else(Vec::new, |explorer| {
                let root = explorer.root().to_owned();
                match rustode_fs::list_files(&root) {
                    Ok(files) => files.into_iter().map(|f| root.join(f)).collect(),
                    Err(err) => {
                        tracing::warn!(root = %root.display(), %err, "cannot list files");
                        Vec::new()
                    }
                }
            });
            self.files = Some(files);
        }
        self.files.as_deref().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_pick_commands_and_lines() {
        assert_eq!(parse_quick_query("main"), QuickQuery::Files("main"));
        assert_eq!(parse_quick_query("> save"), QuickQuery::Commands("save"));
        assert_eq!(
            parse_quick_query(":12"),
            QuickQuery::Line {
                line: 12,
                column: None
            }
        );
        assert_eq!(
            parse_quick_query(":12:3"),
            QuickQuery::Line {
                line: 12,
                column: Some(3)
            }
        );
        assert_eq!(
            parse_quick_query(":7,2"),
            QuickQuery::Line {
                line: 7,
                column: Some(2)
            }
        );
        assert_eq!(parse_quick_query(":x"), QuickQuery::NoLine);
        assert_eq!(parse_quick_query(":0"), QuickQuery::NoLine);
    }
}
