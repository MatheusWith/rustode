//! Replace across files, with a preview of every change, as VS Code's
//! Search view does it: a replace field next to the search box, the text
//! each match would become, and Replace All, Replace All in a file,
//! Replace on one match and Dismiss on a match or a file.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - the replace text of a match is built from the match as `ReplacePattern`
//!   builds it, groups and `\u \l \U \L` included
//!   (`src/vs/workbench/contrib/search/browser/searchTreeModel/match.ts:83-120`,
//!   `src/vs/editor/contrib/find/browser/replacePattern.ts`);
//! - the edits of the chosen matches are applied as one bulk edit, and
//!   every edited file is **saved** afterwards
//!   (`src/vs/workbench/contrib/search/browser/replaceService.ts:115-138`,
//!   `:133`), so a replaced file is not left dirty; undo then brings the
//!   text back as an unsaved change;
//! - the commands: Replace on a match and Replace All in a file, both on
//!   `Ctrl+Shift+1`, and Dismiss on `Delete`
//!   (`searchActionsRemoveReplace.ts:59-68`, `:154-160`, `:190-196`);
//!   Replace All of the whole view is the widget's button
//!   (`searchView.ts:814-856`);
//! - a replaced match leaves the results, and a file left with no match
//!   leaves with it; Dismiss removes a match or a file the same way
//!   (`searchActionsRemoveReplace.ts:95-130`).
//!
//! What differs, recorded in `docs/gaps.md`: VS Code opens a diff editor per
//! file for the preview, here the preview is the match's line before and
//! after; it asks to confirm Replace All, here the frontend decides; and its
//! bulk edit keeps closed files' models out of the tabs, here an edited file
//! stays open as a tab, so Ctrl+Z works in it.
//!
//! A file is checked against the search before anything changes: each
//! chosen match must still be found where the search found it, with the
//! same text around it. Otherwise nothing is written, the search runs again
//! and the call returns [`ReplaceError::Stale`]. Matches that stay in the
//! results after a partial replace follow the text that moved.

use super::{FileResult, SearchEvent, SearchMatch, SearchQuery, SearchResults};
use crate::bulk_edit::follow;
use crate::{DocumentId, Event, Position, Range, Workspace};
use rustode_core::Buffer;
use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The largest file read to build previews or to replace in; a larger one
/// is refused rather than read whole.
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;

/// What a replace changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceScope {
    /// Every match left in the results (`search.action.replaceAll`).
    All,
    /// Every match of one file (`search.action.replaceAllInFile`); the
    /// file's index in the results.
    File(usize),
    /// One match (`search.action.replace`).
    Match {
        /// The file's index in the results.
        file: usize,
        /// The match's index in the file.
        index: usize,
    },
}

/// What a replace did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceOutcome {
    /// How many matches were replaced.
    pub matches: usize,
    /// In how many files.
    pub files: usize,
    /// Files that were edited but could not be saved, with the reason; they
    /// stay open with unsaved changes.
    pub unsaved: Vec<String>,
}

/// "N occurrence(s) across M file(s)", in VS Code's four forms
/// (`searchView.ts:858-888`, `:890-935`), with ` with 'text'` for a
/// non-empty replace text of at most ten lines, then `…` after the tenth.
fn across(matches: usize, files: usize, replacement: &str) -> String {
    let occurrences = if matches == 1 {
        "occurrence"
    } else {
        "occurrences"
    };
    let file_word = if files == 1 { "file" } else { "files" };
    let mut text = format!("{matches} {occurrences} across {files} {file_word}");
    if !replacement.is_empty() {
        let lines: Vec<&str> = replacement.split('\n').collect();
        let shown = if lines.len() > 10 {
            format!("{}\n...", lines[..10].join("\n"))
        } else {
            replacement.to_owned()
        };
        let _ = write!(text, " with '{shown}'");
    }
    text
}

impl ReplaceOutcome {
    /// The line that tells what was replaced, as VS Code's view does after
    /// Replace All (`searchView.ts:858-888`).
    #[must_use]
    pub fn message(&self, replacement: &str) -> String {
        format!(
            "Replaced {}.",
            across(self.matches, self.files, replacement)
        )
    }
}

/// Why nothing was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceError {
    /// There is no such match or file, or no search.
    NothingToReplace,
    /// These files changed since the search (or could not be read);
    /// nothing was written and the search ran again.
    Stale(Vec<PathBuf>),
    /// The edits could not be applied; nothing was written.
    Failed(String),
}

impl fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingToReplace => f.write_str("There is nothing to replace"),
            Self::Stale(files) => write!(
                f,
                "{} changed since the search: nothing was replaced and the search ran again",
                files.first().map_or_else(
                    || "a file".to_owned(),
                    |p| p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned()
                    )
                )
            ),
            Self::Failed(why) => write!(f, "The replace was not applied: {why}"),
        }
    }
}

impl std::error::Error for ReplaceError {}

/// One match of a file as it is now, with the text that replaces it.
#[derive(Debug)]
struct Scanned {
    range: Range,
    start: (usize, usize),
    end: (usize, usize),
    text: String,
}

/// The replace field, its previews, and what the last replace changed.
#[derive(Default)]
pub(super) struct ReplaceState {
    text: String,
    /// The replacements of each file, built when its preview is first
    /// shown and dropped when the replace text or the search changes.
    previews: Mutex<HashMap<PathBuf, Arc<Vec<Scanned>>>>,
    /// The documents the last replace edited and their versions after it.
    undo: Option<Vec<(DocumentId, u64)>>,
    /// The replace being done, and what the last one ended with.
    job: Option<Job>,
    outcome: Option<Result<ReplaceOutcome, ReplaceError>>,
}

impl ReplaceState {
    pub(super) fn forget_previews(&mut self) {
        self.previews
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.undo = None;
        self.job = None;
    }
}

/// The current text of `path`: the open document's, else the file's.
fn load(workspace: &Workspace, path: &Path) -> Result<Buffer, String> {
    if let Some(id) = workspace.find_open(path)
        && let Some(document) = workspace.document(id)
    {
        return Ok(document.editor().buffer().clone());
    }
    let bytes = rustode_fs::read_file(path).map_err(|err| err.to_string())?;
    if bytes.len() > MAX_SCAN_BYTES {
        return Err("the file is too large".to_owned());
    }
    Ok(Buffer::from_text(&rustode_fs::decode_owned(bytes).text))
}

fn position(buffer: &Buffer, offset: usize) -> (usize, usize) {
    let line = buffer.line_of(offset);
    (line, offset.saturating_sub(buffer.line_start(line)))
}

/// Every match in `buffer` with its replacement.
fn scan(buffer: &Buffer, query: &SearchQuery, replacement: &str) -> Vec<Scanned> {
    crate::find::replacements(buffer, &query.find, replacement)
        .into_iter()
        .map(|(range, text)| Scanned {
            range,
            start: position(buffer, range.start),
            end: position(buffer, range.end),
            text,
        })
        .collect()
}

/// Whether `found` is still where the search found it, with the same text
/// around it.
fn is_current(buffer: &Buffer, scanned: &[Scanned], found: &SearchMatch) -> Option<Range> {
    let key = (
        (found.start.line, found.start.column),
        (found.end.line, found.end.column),
    );
    let hit = scanned.iter().find(|s| (s.start, s.end) == key)?;
    if found.start.line >= buffer.len_lines() {
        return None;
    }
    let line = buffer.line_text(found.start.line);
    let same = line
        .chars()
        .skip(found.text_column)
        .take(found.text.chars().count())
        .eq(found.text.chars());
    same.then_some(hit.range)
}

/// The preview window of the match `start..end` in `buffer`, as the search
/// keeps it.
fn rebuild(buffer: &Buffer, start: usize, end: usize) -> SearchMatch {
    let (line, column) = position(buffer, start);
    let (end_line, end_column) = position(buffer, end);
    let text_column = column.saturating_sub(rustode_fs::PREVIEW_BEFORE);
    let text: String = buffer
        .line_text(line)
        .chars()
        .skip(text_column)
        .take(column - text_column + rustode_fs::PREVIEW_CHARS)
        .collect();
    SearchMatch {
        start: Position::new(line, column),
        end: Position::new(end_line, end_column),
        text,
        text_column,
    }
}

/// What to do with one file.
struct FilePlan {
    /// The file's result when the replace started.
    result: FileResult,
    /// Indexes of the matches to replace.
    chosen: Vec<usize>,
    /// The text the edits were computed for.
    buffer: Buffer,
    edits: Vec<(Range, String)>,
    /// The matches that stay: char offsets before the edit.
    kept: Vec<(usize, usize)>,
}

/// A replace being done a few files at a time, so a frontend keeps
/// drawing: first every chosen file is checked and its edits built, then
/// each is edited and saved.
struct Job {
    query: SearchQuery,
    replacement: String,
    /// The chosen files, as the results had them.
    chosen: Vec<(FileResult, Vec<usize>)>,
    planned: usize,
    plans: Vec<FilePlan>,
    stale: Vec<PathBuf>,
    applied: usize,
    active: Option<DocumentId>,
    undo: Vec<(DocumentId, u64)>,
    outcome: ReplaceOutcome,
}

/// How far a replace has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceProgress {
    /// Files checked or edited so far, over both passes.
    pub done: usize,
    /// Files in both passes.
    pub total: usize,
}

impl SearchResults {
    /// The matches `scope` covers, per file, as the results have them now.
    fn chosen(&self, scope: ReplaceScope) -> Vec<(FileResult, Vec<usize>)> {
        let all = |file: &FileResult| (file.clone(), (0..file.matches.len()).collect::<Vec<_>>());
        let mut found: Vec<(FileResult, Vec<usize>)> = match scope {
            ReplaceScope::All => self.files.iter().map(all).collect(),
            ReplaceScope::File(file) => self.files.get(file).map(all).into_iter().collect(),
            ReplaceScope::Match { file, index } => self
                .files
                .get(file)
                .filter(|f| index < f.matches.len())
                .map(|f| (f.clone(), vec![index]))
                .into_iter()
                .collect(),
        };
        found.retain(|(_, matches)| !matches.is_empty());
        found
    }
}

impl Workspace {
    /// Sets the replace field's text. Previews are built with it as VS
    /// Code builds them: in a regex search `$1`, `$&` and `\u \l \U \L`
    /// read the match.
    pub fn set_search_replacement(&mut self, text: &str) {
        if self.search.replace.text != text {
            text.clone_into(&mut self.search.replace.text);
            self.search.replace.forget_previews();
        }
    }

    /// The replace field's text.
    #[must_use]
    pub fn search_replacement(&self) -> &str {
        &self.search.replace.text
    }

    /// The text match `index` of result `file` would become: its
    /// replacement, with line breaks as the replace text has them. `None`
    /// when there is no such match, or the file changed or cannot be read.
    /// The file is read on the first call for it; later calls use what was
    /// built, until the replace text or the search changes.
    #[must_use]
    pub fn search_replacement_text(&self, file: usize, index: usize) -> Option<String> {
        let results = &self.search.results;
        let result = results.files.get(file)?;
        let found = result.matches.get(index)?;
        let query = results.query.as_ref()?;
        let cached = self
            .search
            .replace
            .previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&result.path)
            .cloned();
        let scanned = if let Some(scanned) = cached {
            scanned
        } else {
            let built = load(self, &result.path)
                .map(|buffer| scan(&buffer, query, &self.search.replace.text))
                .unwrap_or_default();
            let built = Arc::new(built);
            self.search
                .replace
                .previews
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(result.path.clone(), Arc::clone(&built));
            built
        };
        let key = (
            (found.start.line, found.start.column),
            (found.end.line, found.end.column),
        );
        scanned
            .iter()
            .find(|s| (s.start, s.end) == key)
            .map(|s| s.text.clone())
    }

    /// Dismiss on a match (`search.action.remove`): it leaves the results
    /// and is not replaced. Returns whether there was such a match; none is
    /// dismissed while a replace is running.
    pub fn dismiss_search_match(&mut self, file: usize, index: usize) -> bool {
        if self.search.replace.job.is_some() {
            return false;
        }
        let results = &mut self.search.results;
        let Some(result) = results.files.get_mut(file) else {
            return false;
        };
        if index >= result.matches.len() {
            return false;
        }
        result.matches.remove(index);
        results.matches = results.matches.saturating_sub(1);
        if result.matches.is_empty() {
            results.files.remove(file);
        }
        self.events.push(Event::Search(SearchEvent::Results));
        true
    }

    /// Dismiss on a file: all its matches leave the results. Returns
    /// whether there was such a file; none is dismissed while a replace is
    /// running.
    pub fn dismiss_search_file(&mut self, file: usize) -> bool {
        if self.search.replace.job.is_some() {
            return false;
        }
        let results = &mut self.search.results;
        if file >= results.files.len() {
            return false;
        }
        let removed = results.files.remove(file);
        results.matches = results.matches.saturating_sub(removed.matches.len());
        self.events.push(Event::Search(SearchEvent::Results));
        true
    }

    /// What VS Code asks before Replace All (`searchView.ts:833-837`,
    /// `:890-935`), e.g. "Replace 4 occurrences across 2 files with 'x'?".
    /// `None` when `scope` covers no match.
    #[must_use]
    pub fn replace_confirmation(&self, scope: ReplaceScope) -> Option<String> {
        let chosen = self.search.results.chosen(scope);
        let matches: usize = chosen.iter().map(|(_, m)| m.len()).sum();
        (matches > 0).then(|| {
            format!(
                "Replace {}?",
                across(matches, chosen.len(), &self.search.replace.text)
            )
        })
    }

    /// Replaces the matches `scope` covers and waits for it: starts the
    /// replace and runs it to the end. A frontend that has to keep
    /// drawing uses [`Workspace::start_replace`] and
    /// [`Workspace::poll_replace`] instead.
    ///
    /// # Errors
    /// As [`Workspace::start_replace`], and what the replace ends with.
    pub fn replace_in_search(
        &mut self,
        scope: ReplaceScope,
    ) -> Result<ReplaceOutcome, ReplaceError> {
        self.start_replace(scope)?;
        self.poll_replace(None);
        self.take_replace_outcome()
            .unwrap_or(Err(ReplaceError::NothingToReplace))
    }

    /// Starts replacing the matches `scope` covers with the replace text.
    /// Every chosen file is checked first and none changes unless all can
    /// (a file that changed since the search stops the replace with
    /// [`ReplaceError::Stale`] and the search runs again); then each file
    /// takes its edits as one undo step, is saved (as VS Code does after a
    /// replace) and stays open. The work happens in
    /// [`Workspace::poll_replace`], a few files at a time. The replaced
    /// matches leave the results; the others follow the text that moved.
    ///
    /// # Errors
    /// [`ReplaceError::NothingToReplace`] when `scope` covers no match, and
    /// [`ReplaceError::Failed`] while another replace is running.
    pub fn start_replace(&mut self, scope: ReplaceScope) -> Result<(), ReplaceError> {
        if self.search.replace.job.is_some() {
            return Err(ReplaceError::Failed(
                "a replace is already running".to_owned(),
            ));
        }
        let Some(query) = self.search.results.query.clone() else {
            return Err(ReplaceError::NothingToReplace);
        };
        let chosen = self.search.results.chosen(scope);
        if chosen.is_empty() {
            return Err(ReplaceError::NothingToReplace);
        }
        self.search.replace.outcome = None;
        self.search.replace.job = Some(Job {
            query,
            replacement: self.search.replace.text.clone(),
            chosen,
            planned: 0,
            plans: Vec::new(),
            stale: Vec::new(),
            applied: 0,
            active: self.active_document(),
            undo: Vec::new(),
            outcome: ReplaceOutcome {
                matches: 0,
                files: 0,
                unsaved: Vec::new(),
            },
        });
        Ok(())
    }

    /// Whether a replace is running.
    #[must_use]
    pub fn is_replacing(&self) -> bool {
        self.search.replace.job.is_some()
    }

    /// How far the running replace has got.
    #[must_use]
    pub fn replace_progress(&self) -> Option<ReplaceProgress> {
        let job = self.search.replace.job.as_ref()?;
        let files = job.chosen.len();
        Some(ReplaceProgress {
            done: job.planned + job.applied,
            total: files * 2,
        })
    }

    /// Does the running replace's work for about `budget` (`None`: until it
    /// ends), at least one file. Returns whether a replace was running.
    /// When it ends, [`Event::Search`] says [`SearchEvent::Replaced`] and
    /// [`Workspace::take_replace_outcome`] has the result.
    pub fn poll_replace(&mut self, budget: Option<Duration>) -> bool {
        if self.search.replace.job.is_none() {
            return false;
        }
        let started = Instant::now();
        loop {
            self.replace_step();
            if self.search.replace.job.is_none() {
                self.events.push(Event::Search(SearchEvent::Replaced));
                return true;
            }
            if budget.is_some_and(|budget| started.elapsed() >= budget) {
                self.events.push(Event::Search(SearchEvent::Results));
                return true;
            }
        }
    }

    /// What the last replace ended with, once.
    pub fn take_replace_outcome(&mut self) -> Option<Result<ReplaceOutcome, ReplaceError>> {
        self.search.replace.outcome.take()
    }

    /// One file of the running replace.
    fn replace_step(&mut self) {
        let Some(mut job) = self.search.replace.job.take() else {
            return;
        };
        let finished = if job.planned < job.chosen.len() {
            self.plan_file(&mut job);
            if job.planned == job.chosen.len() && !job.stale.is_empty() {
                let stale = std::mem::take(&mut job.stale);
                let _ = self.start_search(job.query.clone());
                self.search.replace.outcome = Some(Err(ReplaceError::Stale(stale)));
                true
            } else {
                false
            }
        } else if job.applied < job.plans.len() {
            match self.apply_file(&mut job) {
                Ok(()) => false,
                Err(why) => {
                    self.search.replace.outcome = Some(Err(ReplaceError::Failed(why)));
                    true
                }
            }
        } else {
            self.search.results.notice = Some(job.outcome.message(&job.replacement));
            self.search.replace.outcome = Some(Ok(std::mem::replace(
                &mut job.outcome,
                ReplaceOutcome {
                    matches: 0,
                    files: 0,
                    unsaved: Vec::new(),
                },
            )));
            true
        };
        if finished {
            if let Some(active) = job.active {
                let _ = self.activate(active);
            }
            self.search.replace.forget_previews();
            let undo = std::mem::take(&mut job.undo);
            self.search.replace.undo = (!undo.is_empty()).then_some(undo);
        } else {
            self.search.replace.job = Some(job);
        }
    }

    /// Checks one chosen file against the search and builds its edits.
    fn plan_file(&self, job: &mut Job) {
        let at = job.planned;
        job.planned += 1;
        let (result, indexes) = &job.chosen[at];
        let Ok(buffer) = load(self, &result.path) else {
            job.stale.push(result.path.clone());
            return;
        };
        let scanned = scan(&buffer, &job.query, &job.replacement);
        let mut edits = Vec::with_capacity(indexes.len());
        let mut kept = Vec::new();
        let last_line = buffer.len_lines().saturating_sub(1);
        for (index, found) in result.matches.iter().enumerate() {
            if indexes.contains(&index) {
                let Some(range) = is_current(&buffer, &scanned, found) else {
                    job.stale.push(result.path.clone());
                    return;
                };
                let text = scanned
                    .iter()
                    .find(|s| s.range == range)
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                edits.push((range, text));
            } else {
                kept.push((
                    buffer.offset_of(found.start.line.min(last_line), found.start.column),
                    buffer.offset_of(found.end.line.min(last_line), found.end.column),
                ));
            }
        }
        job.plans.push(FilePlan {
            result: result.clone(),
            chosen: indexes.clone(),
            buffer,
            edits,
            kept,
        });
    }

    /// Edits and saves one planned file, and brings the results up to date.
    fn apply_file(&mut self, job: &mut Job) -> Result<(), String> {
        let at = job.applied;
        job.applied += 1;
        let plan = &job.plans[at];
        let path = plan.result.path.clone();
        let was_open = self.find_open(&path).is_some();
        let id = self.open(&path).map_err(|err| err.to_string())?;
        let same = self
            .document(id)
            .is_some_and(|d| d.editor().buffer().text() == plan.buffer.text());
        if !same {
            // Edited or changed on disk since it was checked: left alone.
            if !was_open {
                let _ = self.close(id, true);
            }
            job.outcome
                .unsaved
                .push(format!("{}: changed while replacing", path.display()));
            return Ok(());
        }
        let applied = self.apply_planned(vec![(id, plan.edits.clone())])?;
        if let Err(err) = self.save(id) {
            job.outcome
                .unsaved
                .push(format!("{}: {err}", path.display()));
        }
        if let Some(document) = self.document(id) {
            job.undo.push((id, document.version()));
        }
        job.outcome.matches += plan.chosen.len();
        job.outcome.files += 1;
        let edits: &[(Range, usize)] = applied
            .iter()
            .find(|a| a.document == id)
            .map_or(&[], |a| a.edits.as_slice());
        let kept: Vec<SearchMatch> = match self.document(id) {
            Some(document) => {
                let buffer = document.editor().buffer();
                plan.kept
                    .iter()
                    .map(|&(start, end)| {
                        let (start, end) = (follow(start, edits), follow(end, edits));
                        rebuild(buffer, start, end.max(start))
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let results = &mut self.search.results;
        if let Some(index) = results.files.iter().position(|f| f.path == path) {
            let before = results.files[index].matches.len();
            results.matches = results
                .matches
                .saturating_sub(before.saturating_sub(kept.len()));
            results.files[index].matches = kept;
            if results.files[index].matches.is_empty() {
                results.files.remove(index);
            }
        }
        Ok(())
    }

    /// Undoes the last replace in every document it edited, as VS Code's
    /// undo does for the bulk edit. Returns how many documents changed.
    ///
    /// # Errors
    /// [`ReplaceError::NothingToReplace`] when there is nothing to undo, and
    /// [`ReplaceError::Failed`] when a document was edited since, in which
    /// case none is undone.
    pub fn undo_replace(&mut self) -> Result<usize, ReplaceError> {
        let Some(edited) = self.search.replace.undo.take() else {
            return Err(ReplaceError::NothingToReplace);
        };
        let current = edited
            .iter()
            .all(|(id, version)| self.document(*id).is_some_and(|d| d.version() == *version));
        if !current {
            return Err(ReplaceError::Failed(
                "a replaced file was edited since, so the replace is not undone".to_owned(),
            ));
        }
        let mut undone = 0;
        for (id, _) in edited {
            if self.undo(id).unwrap_or(false) {
                undone += 1;
            }
        }
        Ok(undone)
    }
}
