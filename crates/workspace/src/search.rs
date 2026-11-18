//! Search across the opened folder (Ctrl+Shift+F), as VS Code's Search
//! view does it. The search runs in the background on `rustode-fs`'s
//! engine; [`Workspace::poll_search`] takes the files with matches as they
//! arrive, and a new search or [`Workspace::cancel_search`] stops the
//! running one. The pattern and its options are a [`FindQuery`], so Match
//! Case, Match Whole Word and Use Regular Expression mean what they mean in
//! find in a document.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - the settings read for each search: `files.exclude` and
//!   `search.exclude`, merged over their defaults as VS Code merges object
//!   settings (`src/vs/platform/configuration/common/configurationModels.ts:219-229`),
//!   `search.exclude` winning (`src/vs/workbench/services/search/common/search.ts:488-506`);
//!   `search.useIgnoreFiles`, `search.useParentIgnoreFiles`,
//!   `search.useGlobalIgnoreFiles`, `search.followSymlinks` and
//!   `search.maxResults`, 20,000 by default and `null` for no limit
//!   (`src/vs/workbench/contrib/search/browser/search.common.contribution.ts:26-95`,
//!   `search.ts:32`); the exclude toggle turns off both the exclude
//!   settings and the ignore files (`searchView.ts:1673-1674`);
//! - results sorted by folder, then file name, a folder's files before its
//!   subfolders (`src/vs/workbench/contrib/search/browser/searchCompare.ts`,
//!   `src/vs/base/common/comparers.ts:265-304`), every file expanded
//!   (`search.collapseResults: alwaysExpand`, `search.contribution.ts:144-154`);
//! - the messages: "N results in M files" (`searchView.ts:2189-2199`), the
//!   no-results, cancelled and no-folder messages (`:127`, `:1833-1862`,
//!   `:2205`) without their links, and the limit warning (`:1899-1901`);
//! - a match's preview: the text before it trimmed and cut to about 26
//!   chars at a word boundary after `…`, then the match and the text after
//!   it, 250 chars in all (`searchTreeModel/match.ts:22`, `:65-81`;
//!   `src/vs/base/common/strings.ts:784-807`);
//! - opening a match selects it in its file, in a tab.

use crate::explorer::compare_names;
use crate::find::TextSearch;
use crate::{Config, DocumentId, Error, Event, FindQuery, Position, Selection, Value, Workspace};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// VS Code's default `search.maxResults` (`search.ts:32`).
pub const MAX_RESULTS: usize = 20_000;
/// The most chars a match preview shows (`match.ts:22`).
const PREVIEW_MAX: usize = 250;
/// About how many chars of text before a match a preview keeps (`match.ts:66`).
const PREVIEW_BEFORE: usize = 26;

/// What to search for, and where.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The pattern and the find options.
    pub find: FindQuery,
    /// "files to include": comma-separated globs.
    pub include: String,
    /// "files to exclude": comma-separated globs.
    pub exclude: String,
    /// "Use Exclude Settings and Ignore Files", on by default.
    pub use_excludes: bool,
}

impl SearchQuery {
    /// A search for `find` in every file, with the exclude settings and
    /// ignore files on.
    #[must_use]
    pub fn new(find: FindQuery) -> Self {
        Self {
            find,
            include: String::new(),
            exclude: String::new(),
            use_excludes: true,
        }
    }
}

/// Where the last search is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchState {
    /// No search yet, or an empty pattern.
    #[default]
    Idle,
    /// Results are still arriving.
    Running,
    /// It finished.
    Done,
    /// It was cancelled; the results found so far stay.
    Cancelled,
}

/// Why a search could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// An include or exclude glob is not valid, or is a path outside the
    /// opened folder.
    Glob(String),
    /// The search thread could not start.
    Start(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glob(message) => write!(f, "invalid glob pattern {message}"),
            Self::Start(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SearchError {}

/// Something that happened to the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchEvent {
    /// More files with matches arrived.
    Results,
    /// The search finished or was cancelled.
    Finished,
}

/// One match in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Where it starts (0-based line, column in chars).
    pub start: Position,
    /// Where it ends.
    pub end: Position,
    /// The start line around the match.
    text: String,
    /// The column in the start line where `text` starts.
    text_column: usize,
}

/// A match as the results list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPreview {
    /// The text before the match, cut to about 26 chars after `…`.
    pub before: String,
    /// The match (its first line).
    pub inside: String,
    /// The rest of the line, within the preview's 250 chars.
    pub after: String,
}

impl SearchMatch {
    /// The text to show for this match, VS Code's `preview()`.
    #[must_use]
    pub fn preview(&self) -> MatchPreview {
        let chars: Vec<char> = self.text.chars().collect();
        let start = (self.start.column - self.text_column).min(chars.len());
        let end = if self.end.line == self.start.line {
            (self.end.column - self.text_column).clamp(start, chars.len())
        } else {
            chars.len()
        };
        let full_before: String = chars[..start].iter().collect();
        let before = lcut(&full_before, PREVIEW_BEFORE, "…");
        let mut left = PREVIEW_MAX.saturating_sub(before.chars().count());
        let inside: String = chars[start..end].iter().take(left).collect();
        left -= inside.chars().count();
        let after: String = chars[end..].iter().take(left).collect();
        MatchPreview {
            before,
            inside,
            after,
        }
    }
}

/// VS Code's `lcut`: `text` without leading whitespace and, when it is at
/// least `n` chars long, cut from the left at a word boundary so that at
/// least `n` chars remain, after `prefix`. Word chars are ASCII letters,
/// digits and `_`, as JavaScript's `\b` counts them.
fn lcut(text: &str, n: usize, prefix: &str) -> String {
    let trimmed = text.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < n {
        return trimmed.to_owned();
    }
    let word = |at: usize| {
        chars
            .get(at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
    };
    let mut cut = 0;
    for at in 0..=chars.len() {
        let boundary = (at > 0 && word(at - 1)) != word(at);
        if !boundary {
            continue;
        }
        if chars.len() - at < n {
            break;
        }
        cut = at;
    }
    if cut == 0 {
        return trimmed.to_owned();
    }
    let rest: String = chars[cut..].iter().collect();
    format!("{prefix}{}", rest.trim_start())
}

/// A file with matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResult {
    /// The file.
    pub path: PathBuf,
    /// Its name.
    pub label: String,
    /// Its folder, relative to the opened folder when inside it.
    pub description: String,
    /// Its matches, in order.
    pub matches: Vec<SearchMatch>,
}

/// One row of the results list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRow {
    /// A file, with whether its matches are shown.
    File {
        /// Its index in [`SearchResults::files`].
        file: usize,
        /// Whether its matches follow.
        expanded: bool,
    },
    /// A match of a file.
    Match {
        /// The file's index.
        file: usize,
        /// The match's index in the file.
        index: usize,
    },
}

/// The last search and its results.
#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    query: Option<SearchQuery>,
    files: Vec<FileResult>,
    matches: usize,
    state: SearchState,
    limit_hit: bool,
    /// Whether a folder was open when it started.
    folder: bool,
    /// Files whose matches are hidden.
    collapsed: HashSet<PathBuf>,
}

impl SearchResults {
    /// The query of the last search, if any.
    #[must_use]
    pub fn query(&self) -> Option<&SearchQuery> {
        self.query.as_ref()
    }

    /// The files with matches, sorted as VS Code's results.
    #[must_use]
    pub fn files(&self) -> &[FileResult] {
        &self.files
    }

    /// How many matches there are in all.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches
    }

    /// How many files have matches.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Where the search is.
    #[must_use]
    pub fn state(&self) -> SearchState {
        self.state
    }

    /// Whether results are still arriving.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state == SearchState::Running
    }

    /// Whether matches beyond `search.maxResults` were left out.
    #[must_use]
    pub fn limit_hit(&self) -> bool {
        self.limit_hit
    }

    /// The line above the results: the count, or why there are none.
    #[must_use]
    pub fn message(&self) -> Option<String> {
        let query = self.query.as_ref()?;
        if self.matches > 0 {
            return Some(count_message(self.matches, self.files.len()));
        }
        Some(match self.state {
            SearchState::Idle | SearchState::Running => return None,
            SearchState::Cancelled => {
                "Search was canceled before any results could be found".into()
            }
            SearchState::Done if !self.folder => {
                "You have not opened or specified a folder. Only open files are currently searched"
                    .into()
            }
            SearchState::Done => no_results(query.include.trim(), query.exclude.trim()),
        })
    }

    /// The warning after a search that hit the limit.
    #[must_use]
    pub fn warning(&self) -> Option<&'static str> {
        self.limit_hit.then_some(
            "The result set only contains a subset of all matches. Be more specific in your search to narrow down the results.",
        )
    }

    /// Whether a file's matches are shown.
    #[must_use]
    pub fn is_expanded(&self, file: usize) -> bool {
        self.files
            .get(file)
            .is_some_and(|f| !self.collapsed.contains(&f.path))
    }

    /// The results list: each file, then its matches when it is expanded.
    #[must_use]
    pub fn rows(&self) -> Vec<SearchRow> {
        let mut rows = Vec::with_capacity(self.files.len() + self.matches);
        for (file, result) in self.files.iter().enumerate() {
            let expanded = !self.collapsed.contains(&result.path);
            rows.push(SearchRow::File { file, expanded });
            if expanded {
                rows.extend(
                    (0..result.matches.len()).map(|index| SearchRow::Match { file, index }),
                );
            }
        }
        rows
    }

    /// Adds a file's matches in its sorted place.
    fn insert(&mut self, root: Option<&Path>, found: rustode_fs::FileMatches) {
        let (label, description) = split(&found.path, root);
        let matches: Vec<SearchMatch> = found
            .matches
            .into_iter()
            .map(|m| SearchMatch {
                start: Position::new(m.start.0, m.start.1),
                end: Position::new(m.end.0, m.end.1),
                text: m.preview,
                text_column: m.preview_column,
            })
            .collect();
        self.matches += matches.len();
        let at = self
            .files
            .partition_point(|f| compare_paths(&f.path, &found.path) == Ordering::Less);
        self.files.insert(
            at,
            FileResult {
                path: found.path,
                label,
                description,
                matches,
            },
        );
    }
}

/// "N results in M files", in VS Code's four forms.
fn count_message(results: usize, files: usize) -> String {
    let results_word = if results == 1 { "result" } else { "results" };
    let files_word = if files == 1 { "file" } else { "files" };
    format!("{results} {results_word} in {files} {files_word}")
}

fn no_results(include: &str, exclude: &str) -> String {
    match (include.is_empty(), exclude.is_empty()) {
        (false, false) => format!("No results found in '{include}' excluding '{exclude}'"),
        (false, true) => format!("No results found in '{include}'"),
        (true, false) => format!("No results found excluding '{exclude}'"),
        (true, true) => {
            "No results found. Review your configured exclusions and check your gitignore files"
                .to_owned()
        }
    }
}

/// A file's name, and its folder relative to `root` when inside it.
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

/// VS Code's `comparePaths`: folder by folder, ignoring case; a path that
/// ends first (a file in a shallower folder) comes first; file names
/// compare naturally.
fn compare_paths(left: &Path, right: &Path) -> Ordering {
    let parts = |path: &Path| -> Vec<String> {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect()
    };
    let (left, right) = (parts(left), parts(right));
    for i in 0.. {
        let (end_left, end_right) = (i + 1 >= left.len(), i + 1 >= right.len());
        match (end_left, end_right) {
            (true, true) => {
                let (l, r) = (left.get(i), right.get(i));
                return compare_names(l.map_or("", |s| s), r.map_or("", |s| s));
            }
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        let order = left[i].to_lowercase().cmp(&right[i].to_lowercase());
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// The search settings, read when a search starts.
struct Settings {
    excludes: Vec<String>,
    ignore_files: rustode_fs::IgnoreFiles,
    follow_symlinks: bool,
    max_results: Option<usize>,
}

impl Settings {
    fn read(config: &Config) -> Self {
        let flag = |key: &str, default: bool| match config.value(key) {
            Some(Value::Bool(value)) => *value,
            _ => default,
        };
        let mut excludes = merged_object(config, "files.exclude");
        excludes.extend(merged_object(config, "search.exclude"));
        let excludes = excludes
            .into_iter()
            .filter(|(glob, value)| {
                *value == Value::Bool(true) && !glob.trim_start().starts_with('!')
            })
            .map(|(glob, _)| glob)
            .collect();
        let max_results = match config.value("search.maxResults") {
            Some(Value::Null) => None,
            Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => {
                // Saturating: a huge limit is no limit.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                // checked finite and non-negative above
                Some(n.floor() as usize)
            }
            _ => Some(MAX_RESULTS),
        };
        Self {
            excludes,
            ignore_files: rustode_fs::IgnoreFiles {
                local: flag("search.useIgnoreFiles", true),
                parent: flag("search.useParentIgnoreFiles", false),
                global: flag("search.useGlobalIgnoreFiles", false),
            },
            follow_symlinks: flag("search.followSymlinks", true),
            max_results,
        }
    }
}

/// An object setting: the user's entries over the default's.
fn merged_object(config: &Config, key: &str) -> BTreeMap<String, Value> {
    let mut merged = BTreeMap::new();
    if let Some(Value::Object(default)) = config.schema().get(key).map(|s| &s.default) {
        merged.extend(default.clone());
    }
    if let Some(Value::Object(user)) = config.settings().settings.values.get(key) {
        merged.extend(user.clone());
    }
    merged
}

/// The workspace's search state.
#[derive(Default)]
pub(crate) struct Searches {
    results: SearchResults,
    running: Option<rustode_fs::Search>,
    /// The opened folder when the running search started.
    root: Option<PathBuf>,
    waker: Option<rustode_fs::Waker>,
}

impl fmt::Debug for Searches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Searches")
            .field("results", &self.results.files.len())
            .field("state", &self.results.state)
            .finish_non_exhaustive()
    }
}

impl Workspace {
    /// Searches the opened folder, and the open documents as they are in
    /// the editor, in the background; results arrive through
    /// [`Workspace::poll_search`]. A running search is cancelled first. An
    /// empty pattern only clears the results.
    ///
    /// # Errors
    /// When an include or exclude glob is not valid; the results are then
    /// cleared.
    pub fn start_search(&mut self, query: SearchQuery) -> Result<(), SearchError> {
        self.cancel_search();
        let root = self.explorer().map(|e| absolute(e.root()));
        self.search.results = SearchResults {
            folder: root.is_some(),
            ..SearchResults::default()
        };
        if query.find.pattern().is_empty() {
            return Ok(());
        }
        let settings = Settings::read(&self.config);
        let (excludes, ignore_files) = if query.use_excludes {
            (settings.excludes, settings.ignore_files)
        } else {
            (Vec::new(), rustode_fs::IgnoreFiles::default())
        };
        let request = rustode_fs::SearchRequest {
            root: root.clone(),
            include: query.include.clone(),
            exclude: query.exclude.clone(),
            settings_excludes: excludes,
            ignore_files,
            follow_symlinks: settings.follow_symlinks,
            max_results: settings.max_results,
            open: self.open_texts(),
        };
        let matcher = Arc::new(TextSearch::new(query.find.clone()));
        let search = rustode_fs::Search::start(request, matcher, self.search.waker.clone())
            .map_err(|err| match err {
                rustode_fs::StartError::Glob(err) => SearchError::Glob(err.to_string()),
                rustode_fs::StartError::Thread(err) => SearchError::Start(err.to_string()),
            })?;
        self.search.results.query = Some(query);
        self.search.results.state = SearchState::Running;
        self.search.running = Some(search);
        self.search.root = root;
        Ok(())
    }

    /// The open documents, to search as they are in the editor.
    fn open_texts(&self) -> Vec<rustode_fs::OpenText> {
        self.documents
            .iter()
            .flatten()
            .map(|document| {
                let buffer = document.editor().buffer().clone();
                rustode_fs::OpenText {
                    path: absolute(document.path()),
                    text: Box::new(move || buffer.text()),
                }
            })
            .collect()
    }

    /// Stops the running search (Escape, or a new search); the results
    /// found so far stay. Returns whether a search was running.
    pub fn cancel_search(&mut self) -> bool {
        let Some(search) = self.search.running.take() else {
            return false;
        };
        search.cancel();
        self.search.results.state = SearchState::Cancelled;
        self.events.push(Event::Search(SearchEvent::Finished));
        true
    }

    /// Takes the results that arrived and reports them as events. Returns
    /// whether anything changed. Never blocks.
    pub fn poll_search(&mut self) -> bool {
        let Some(search) = self.search.running.as_ref() else {
            return false;
        };
        let updates = search.updates();
        if updates.is_empty() {
            return false;
        }
        let root = self.search.root.clone();
        let mut results = false;
        for update in updates {
            match update {
                rustode_fs::SearchUpdate::File(found) => {
                    self.search.results.insert(root.as_deref(), found);
                    results = true;
                }
                rustode_fs::SearchUpdate::Finished { limit_hit, .. } => {
                    self.search.results.limit_hit = limit_hit;
                    self.search.results.state = SearchState::Done;
                    self.search.running = None;
                }
            }
        }
        if results {
            self.events.push(Event::Search(SearchEvent::Results));
        }
        if self.search.running.is_none() {
            self.events.push(Event::Search(SearchEvent::Finished));
        }
        true
    }

    /// The last search and its results.
    #[must_use]
    pub fn search_results(&self) -> &SearchResults {
        &self.search.results
    }

    /// Calls `waker` (from another thread) whenever search results are
    /// waiting, so a frontend that sleeps until input knows to poll.
    pub fn set_search_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.search.waker = Some(Arc::new(waker));
    }

    /// Shows or hides a file's matches in the results list; returns
    /// whether they are now shown.
    pub fn toggle_search_file(&mut self, file: usize) -> bool {
        let results = &mut self.search.results;
        let Some(path) = results.files.get(file).map(|f| f.path.clone()) else {
            return false;
        };
        if results.collapsed.remove(&path) {
            true
        } else {
            results.collapsed.insert(path);
            false
        }
    }

    /// Opens the file of a match in a tab, with the match selected.
    ///
    /// # Errors
    /// When there is no such match or the file cannot be read.
    pub fn open_search_match(&mut self, file: usize, index: usize) -> Result<DocumentId, Error> {
        let found = self.search.results.files.get(file).and_then(|f| {
            f.matches
                .get(index)
                .map(|m| (f.path.clone(), m.start, m.end))
        });
        let Some((path, start, end)) = found else {
            return Err(Error::Io {
                path: PathBuf::new(),
                source: std::io::Error::other("no such search result"),
            });
        };
        let id = self.open(&path)?;
        let document = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let (from, to) = (document.offset_of(start), document.offset_of(end));
        self.set_selections(id, vec![Selection::new(from, to)], 0)?;
        Ok(id)
    }

    /// What Ctrl+Shift+F puts in the search box: the active document's
    /// selection when it lies on one line, escaped for a regex search as
    /// VS Code escapes it (`searchView.ts:1281-1283`,
    /// `src/vs/base/common/strings.ts:89-91`).
    #[must_use]
    pub fn search_seed(&self, regex: bool) -> Option<String> {
        let document = self.active_document().and_then(|id| self.document(id))?;
        let range = document.primary_selection().range();
        let (start, end) = (
            document.position_of(range.start),
            document.position_of(range.end),
        );
        if start.line != end.line || start.column >= end.column {
            return None;
        }
        let line = document.line(start.line);
        let selected = line
            .chars()
            .skip(start.column)
            .take(end.column - start.column);
        let mut seed = String::new();
        for c in selected {
            if regex && "\\{}*+?|^$.[]()".contains(c) {
                seed.push('\\');
            }
            seed.push(c);
        }
        Some(seed)
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcut_keeps_the_end_from_a_word_boundary() {
        assert_eq!(lcut("   short", 26, "…"), "short");
        assert_eq!(
            lcut("let value = compute_something(first, second) + ", 26, "…"),
            "…compute_something(first, second) + "
        );
        assert_eq!(lcut("x".repeat(40).as_str(), 26, "…"), "x".repeat(40));
    }

    #[test]
    fn paths_sort_folder_by_folder_with_files_first() {
        let mut paths = vec![
            "/r/src/z.rs",
            "/r/b.txt",
            "/r/src/sub/a.rs",
            "/r/A.txt",
            "/r/src/a10.rs",
            "/r/src/a2.rs",
        ];
        paths.sort_by(|a, b| compare_paths(Path::new(a), Path::new(b)));
        assert_eq!(
            paths,
            [
                "/r/A.txt",
                "/r/b.txt",
                "/r/src/a2.rs",
                "/r/src/a10.rs",
                "/r/src/z.rs",
                "/r/src/sub/a.rs"
            ]
        );
    }
}
