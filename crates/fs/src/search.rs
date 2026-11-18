//! Workspace search: every file under a folder that the ignore files and
//! the exclude globs let through is read, decoded like an opened file, and
//! scanned by a [`TextMatcher`]; the files with matches stream back one at
//! a time while the search runs on background threads, until it ends, is
//! cancelled, or reaches its result limit.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`.
//! VS Code runs ripgrep with these arguments
//! (`src/vs/workbench/services/search/node/ripgrepTextSearchEngine.ts:506-540`):
//! hidden files are searched and `.gitignore` files apply outside git
//! repositories too (`--hidden --no-require-git`); `.gitignore` and
//! `.ignore` apply when `search.useIgnoreFiles` is on, those of parent
//! folders only with `search.useParentIgnoreFiles`, and the global git
//! ignore file only with `search.useGlobalIgnoreFiles`
//! (`src/vs/workbench/contrib/search/browser/search.common.contribution.ts:57-73`);
//! symbolic links are followed with `search.followSymlinks` (`:80-84`).
//! The `ignore` crate also reads `.git/info/exclude` with the git ignore
//! files. Files holding a NUL byte are skipped as binary unless a UTF-16
//! byte order mark says otherwise: VS Code leaves binary files to ripgrep,
//! whose rules are not in the source tree, so this rule is rustode's
//! approximation of them. The limit counts matches, and
//! is only "hit" when there are more matches than it allows, as VS Code
//! asks for one result more than it shows
//! (`src/vs/workbench/services/search/common/searchService.ts:562-568`).
//! Texts that are open in the editor are searched instead of their files,
//! unsaved edits included, as VS Code searches its open editors
//! (`searchService.ts:511-580`); include and exclude globs apply to them,
//! ignore files and exclude settings do not
//! (`src/vs/workbench/services/search/common/search.ts:508-536`).

mod globs;
mod locate;

pub use globs::GlobError;

use std::fmt;

use globs::Filter;
use std::collections::HashSet;
use std::ops::{ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

/// What a search looks for in each file's text.
pub trait TextMatcher: Send + Sync {
    /// Calls `found` with the byte range of every match in `text`, a whole
    /// file, in order and without overlaps, until `found` breaks.
    fn find_matches(&self, text: &str, found: &mut dyn FnMut(Range<usize>) -> ControlFlow<()>);
}

/// One match, located in its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    /// Zero-based line and char column of the match's first char.
    pub start: (usize, usize),
    /// Zero-based line and char column just past its last char.
    pub end: (usize, usize),
    /// The start line around the match: up to [`PREVIEW_BEFORE`] chars
    /// before it and [`PREVIEW_CHARS`] from its start, as VS Code keeps a
    /// window of `charsPerLine` chars, a fifth of them before the match
    /// (`src/vs/workbench/services/search/common/search.ts:341-368`;
    /// `src/vs/workbench/contrib/search/browser/searchView.ts:1666-1667`).
    pub preview: String,
    /// The char column in the start line where `preview` begins.
    pub preview_column: usize,
}

/// Chars of the start line kept in a preview from the match's start.
pub const PREVIEW_CHARS: usize = 1000;
/// Chars of the start line kept in a preview before the match.
pub const PREVIEW_BEFORE: usize = PREVIEW_CHARS / 5;

/// The matches of one file, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    /// The file.
    pub path: PathBuf,
    /// Its matches, at least one.
    pub matches: Vec<TextMatch>,
}

/// Which ignore files a search honors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IgnoreFiles {
    /// `.gitignore`, `.ignore` and `.git/info/exclude` inside the folder.
    pub local: bool,
    /// Those of the folder's parents too (only with `local`).
    pub parent: bool,
    /// The global git ignore file too (only with `local`).
    pub global: bool,
}

/// A text open in the editor, searched instead of its file.
pub struct OpenText {
    /// Its file.
    pub path: PathBuf,
    /// Produces the text, on the search's thread.
    pub text: Box<dyn FnOnce() -> String + Send>,
}

impl std::fmt::Debug for OpenText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenText")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Where to search and what to skip.
#[derive(Debug, Default)]
pub struct SearchRequest {
    /// The folder to search; `None` searches the open texts only.
    pub root: Option<PathBuf>,
    /// "files to include" as typed: comma-separated globs.
    pub include: String,
    /// "files to exclude" as typed: comma-separated globs.
    pub exclude: String,
    /// Globs excluded by the settings (`files.exclude`, `search.exclude`),
    /// relative to the folder.
    pub settings_excludes: Vec<String>,
    /// Which ignore files apply.
    pub ignore_files: IgnoreFiles,
    /// Whether symbolic links are followed.
    pub follow_symlinks: bool,
    /// The most matches reported; `None` for no limit.
    pub max_results: Option<usize>,
    /// Texts open in the editor, searched instead of their files.
    pub open: Vec<OpenText>,
}

/// What a running search reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchUpdate {
    /// A file with matches.
    File(FileMatches),
    /// The search is over; nothing follows.
    Finished {
        /// Whether matches beyond the limit were left out.
        limit_hit: bool,
        /// Whether it was cancelled before the end.
        cancelled: bool,
    },
}

/// Why a search could not start.
#[derive(Debug)]
pub enum StartError {
    /// An include or exclude glob is not valid.
    Glob(GlobError),
    /// The search thread could not start.
    Thread(std::io::Error),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glob(err) => err.fmt(f),
            Self::Thread(err) => write!(f, "cannot start the search: {err}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Called from a search thread when updates are waiting.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Flags shared by the search threads and the handle.
#[derive(Default)]
struct Shared {
    /// Stop as soon as possible: cancelled, or the limit was hit.
    stop: AtomicBool,
    /// Cancelled through the handle.
    cancelled: AtomicBool,
    /// More matches were found than the limit allows.
    limit_hit: AtomicBool,
    /// Matches still allowed.
    remaining: AtomicUsize,
    /// Whether the waker was called since the last [`Search::updates`].
    woken: AtomicBool,
}

/// A search running on background threads. Dropping it cancels it.
pub struct Search {
    updates: Receiver<SearchUpdate>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search")
            .field("cancelled", &self.shared.cancelled.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Search {
    /// Starts searching with `matcher`; `waker` is called (from a search
    /// thread) when updates are waiting for [`Search::updates`].
    ///
    /// # Errors
    /// When an include or exclude glob is not valid, or the search thread
    /// cannot start.
    pub fn start(
        request: SearchRequest,
        matcher: Arc<dyn TextMatcher>,
        waker: Option<Waker>,
    ) -> Result<Self, StartError> {
        let root = request
            .root
            .as_deref()
            .map(|root| std::path::absolute(root).unwrap_or_else(|_| root.to_owned()));
        let filter = Filter::new(
            root.as_deref(),
            &request.include,
            &request.exclude,
            &request.settings_excludes,
        )
        .map_err(StartError::Glob)?;
        let shared = Arc::new(Shared {
            remaining: AtomicUsize::new(request.max_results.unwrap_or(usize::MAX)),
            ..Shared::default()
        });
        let (sender, updates) = mpsc::channel();
        let job = Job {
            root,
            filter: Arc::new(filter),
            matcher,
            sender,
            shared: Arc::clone(&shared),
            waker,
            ignore_files: request.ignore_files,
            follow_symlinks: request.follow_symlinks,
        };
        let open = request.open;
        std::thread::Builder::new()
            .name("rustode-search".to_owned())
            .spawn(move || job.run(open))
            .map_err(StartError::Thread)?;
        Ok(Self { updates, shared })
    }

    /// The updates that arrived since the last call, oldest first. Never
    /// blocks.
    #[must_use]
    pub fn updates(&self) -> Vec<SearchUpdate> {
        self.shared.woken.store(false, Ordering::Release);
        self.updates.try_iter().collect()
    }

    /// Stops the search; it reports [`SearchUpdate::Finished`] with
    /// `cancelled` once its threads are done.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.shared.stop.store(true, Ordering::Release);
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Everything the search threads share.
struct Job {
    root: Option<PathBuf>,
    filter: Arc<Filter>,
    matcher: Arc<dyn TextMatcher>,
    sender: Sender<SearchUpdate>,
    shared: Arc<Shared>,
    waker: Option<Waker>,
    ignore_files: IgnoreFiles,
    follow_symlinks: bool,
}

impl Job {
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Acquire)
    }

    /// The open texts first, then the folder, then the end.
    fn run(self, open: Vec<OpenText>) {
        let _span = tracing::debug_span!("search").entered();
        let mut skip = HashSet::new();
        for text in open {
            let path = std::path::absolute(&text.path).unwrap_or(text.path);
            if !self.stopped() && self.filter.includes_open(self.root.as_deref(), &path) {
                let contents = (text.text)();
                self.search_text(&path, &contents);
            }
            skip.insert(path);
        }
        if let Some(root) = self.root.as_deref()
            && !self.stopped()
        {
            self.walk(root, &skip);
        }
        let finished = SearchUpdate::Finished {
            limit_hit: self.shared.limit_hit.load(Ordering::Acquire),
            cancelled: self.shared.cancelled.load(Ordering::Acquire),
        };
        self.send(finished);
    }

    /// Searches every file under `root`, in parallel, except `skip`.
    fn walk(&self, root: &Path, skip: &HashSet<PathBuf>) {
        let ignore = self.ignore_files;
        let filter = Arc::clone(&self.filter);
        let prune_root = root.to_owned();
        ignore::WalkBuilder::new(root)
            .hidden(false)
            .require_git(false)
            .git_ignore(ignore.local)
            .git_exclude(ignore.local)
            .ignore(ignore.local)
            .parents(ignore.local && ignore.parent)
            .git_global(ignore.local && ignore.global)
            .follow_links(self.follow_symlinks)
            .threads(0)
            .filter_entry(move |entry| {
                entry.depth() == 0 || !filter.excludes(&prune_root, entry.path())
            })
            .build_parallel()
            .run(|| {
                Box::new(|entry| {
                    if self.stopped() {
                        return ignore::WalkState::Quit;
                    }
                    if let Ok(entry) = entry
                        && entry.file_type().is_some_and(|t| t.is_file())
                        && !skip.contains(entry.path())
                        && self.filter.includes(root, entry.path())
                    {
                        self.search_file(entry.path());
                    }
                    ignore::WalkState::Continue
                })
            });
    }

    /// Reads, decodes and searches one file; binary files are skipped.
    fn search_file(&self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
        if !utf16 && bytes.contains(&0) {
            return;
        }
        let text = crate::decode_owned(bytes).text;
        self.search_text(path, &text);
    }

    /// Finds the matches of one text and reports them, within the limit.
    fn search_text(&self, path: &Path, text: &str) {
        let allowed = self
            .shared
            .remaining
            .load(Ordering::Acquire)
            .saturating_add(1);
        let mut ranges = Vec::new();
        self.matcher.find_matches(text, &mut |range| {
            ranges.push(range);
            if ranges.len() >= allowed || self.stopped() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        if ranges.is_empty() || self.stopped() {
            return;
        }
        let found = ranges.len();
        let left = self
            .shared
            .remaining
            .update(Ordering::AcqRel, Ordering::Acquire, |left| {
                left.saturating_sub(found)
            });
        if found > left {
            self.shared.limit_hit.store(true, Ordering::Release);
            self.shared.stop.store(true, Ordering::Release);
            ranges.truncate(left);
        }
        if ranges.is_empty() {
            return;
        }
        let matches = locate::locate(text, &ranges);
        self.send(SearchUpdate::File(FileMatches {
            path: path.to_owned(),
            matches,
        }));
    }

    /// Sends an update and wakes the receiver if it is not awake yet.
    fn send(&self, update: SearchUpdate) {
        if self.sender.send(update).is_ok()
            && !self.shared.woken.swap(true, Ordering::AcqRel)
            && let Some(waker) = &self.waker
        {
            waker();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Finds every occurrence of a literal, counting the texts it scans.
    struct Literal {
        needle: &'static str,
        scanned: AtomicUsize,
        delay: Duration,
    }

    impl Literal {
        fn new(needle: &'static str) -> Self {
            Self {
                needle,
                scanned: AtomicUsize::new(0),
                delay: Duration::ZERO,
            }
        }
    }

    impl TextMatcher for Literal {
        fn find_matches(&self, text: &str, found: &mut dyn FnMut(Range<usize>) -> ControlFlow<()>) {
            self.scanned.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(self.delay);
            for (at, _) in text.match_indices(self.needle) {
                if found(at..at + self.needle.len()).is_break() {
                    return;
                }
            }
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustode-fs-search-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn finish(search: &Search) -> (Vec<FileMatches>, bool, bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut files = Vec::new();
        loop {
            for update in search.updates() {
                match update {
                    SearchUpdate::File(file) => files.push(file),
                    SearchUpdate::Finished {
                        limit_hit,
                        cancelled,
                    } => return (files, limit_hit, cancelled),
                }
            }
            assert!(Instant::now() < deadline, "the search did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn binary_files_are_skipped_but_utf16_is_read() {
        let root = temp("binary");
        std::fs::write(root.join("bin.dat"), b"key\0key").expect("binary");
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("a key".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(root.join("wide.txt"), utf16).expect("utf16");
        let request = SearchRequest {
            root: Some(root.clone()),
            ..SearchRequest::default()
        };
        let search = Search::start(request, Arc::new(Literal::new("key")), None).expect("start");
        let (files, limit_hit, cancelled) = finish(&search);
        assert!(!limit_hit && !cancelled);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, root.join("wide.txt"));
        assert_eq!(files[0].matches[0].start, (0, 2));
    }

    #[test]
    fn a_cancelled_search_stops_reading_files() {
        let root = temp("cancel");
        for n in 0..200 {
            std::fs::write(root.join(format!("{n}.txt")), "key").expect("file");
        }
        let matcher = Arc::new(Literal {
            delay: Duration::from_millis(5),
            ..Literal::new("key")
        });
        let request = SearchRequest {
            root: Some(root),
            ..SearchRequest::default()
        };
        let search = Search::start(request, Arc::clone(&matcher) as _, None).expect("start");
        std::thread::sleep(Duration::from_millis(20));
        search.cancel();
        let (_, _, cancelled) = finish(&search);
        assert!(cancelled);
        let scanned = matcher.scanned.load(Ordering::Relaxed);
        assert!(
            scanned < 200,
            "{scanned} files were scanned after the cancel"
        );
    }
}
