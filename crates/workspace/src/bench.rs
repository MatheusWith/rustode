//! A non-interactive benchmark hook: a script the GUI runs on itself, with no
//! key injected, so `rustode bench` can time the editor against the App
//! (ADR 0024; AGENTS.md section 11.5.5 forbids keystrokes for a benchmark).
//!
//! `RUSTODE_BENCH_SCRIPT` holds one JSON object:
//!
//! - `{"kind":"edits","seed":1,"count":10000}`: after the first frame, `count`
//!   single-char inserts at seeded random offsets of the active document, each
//!   its own undo step, then every one of them undone;
//! - `{"kind":"search","pattern":"…","regex":true,"include":"","exclude":""}`:
//!   after the first frame, a search across the opened folder.
//!
//! A [`Driver`] runs the script one step per frame: the frontend calls
//! [`Driver::before_frame`] while it builds a frame and
//! [`Driver::after_present`] when the frame has been presented, and prints the
//! one report line the driver returns at the end. The driver holds no
//! frontend type; the workspace API does the work, so the same script can run
//! under any frontend and in the tests of this crate.
//!
//! The edit script is defined by [`edit_offsets`]: the 32-bit generator
//! mulberry32 (public domain), `offset = next % (len + 1)` and a letter
//! `a + next % 26`, with `len` growing by one after each insert. The App side
//! of the comparison (`bench/app_internals.cjs`) implements the same
//! generator, so both editors apply the same edits; the report carries a
//! hash of the text after the inserts and after the undo to check it.

use crate::{DocumentId, FindOptions, FindQuery, Range, SearchState, Workspace};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt;
use std::time::{Duration, Instant};

/// The environment variable that holds the script.
pub const SCRIPT_ENV: &str = "RUSTODE_BENCH_SCRIPT";
/// The most edits a script may ask for.
pub const MAX_EDITS: u32 = 1_000_000;
/// The longest search pattern a script may carry.
pub const MAX_PATTERN: usize = 4096;

/// What a script does.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Script {
    /// Seeded random inserts, then undo of all of them.
    Edits {
        /// The generator's seed.
        seed: u32,
        /// How many inserts.
        count: u32,
    },
    /// A search across the opened folder.
    Search(SearchScript),
}

/// A search the hook starts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchScript {
    /// The pattern, as typed in the search box.
    pub pattern: String,
    /// Use Regular Expression.
    #[serde(default)]
    pub regex: bool,
    /// "files to include".
    #[serde(default)]
    pub include: String,
    /// "files to exclude".
    #[serde(default)]
    pub exclude: String,
}

/// Why a script was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError(String);

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCRIPT_ENV}: {}", self.0)
    }
}

impl std::error::Error for ScriptError {}

impl Script {
    /// Parses the JSON of `RUSTODE_BENCH_SCRIPT`.
    ///
    /// # Errors
    /// When it is not valid JSON of the shapes above, asks for zero or more
    /// than [`MAX_EDITS`] edits, or carries an empty or over-long pattern.
    pub fn parse(json: &str) -> Result<Self, ScriptError> {
        let script: Self =
            serde_json::from_str(json).map_err(|err| ScriptError(err.to_string()))?;
        match &script {
            Self::Edits { count, .. } if !(1..=MAX_EDITS).contains(count) => Err(ScriptError(
                format!("count must be 1 to {MAX_EDITS}, not {count}"),
            )),
            Self::Search(search)
                if search.pattern.is_empty() || search.pattern.len() > MAX_PATTERN =>
            {
                Err(ScriptError(format!(
                    "pattern must be 1 to {MAX_PATTERN} bytes"
                )))
            }
            _ => Ok(script),
        }
    }

    /// The script in `value` (the variable's value, if set): `Ok(None)` when
    /// the variable is not set.
    ///
    /// # Errors
    /// When the value is not UTF-8 or [`Script::parse`] refuses it.
    pub fn from_env_value(value: Option<&OsStr>) -> Result<Option<Self>, ScriptError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let text = value
            .to_str()
            .ok_or_else(|| ScriptError("not valid UTF-8".to_owned()))?;
        Self::parse(text).map(Some)
    }
}

/// mulberry32, a 32-bit generator small enough to write the same way in
/// JavaScript (`Math.imul`), which the App side needs.
#[derive(Debug, Clone, Copy)]
struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(0x01 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(0x3D | t)) ^ t;
        t ^ (t >> 14)
    }
}

/// The inserts of an edit script: `(char offset, letter)` for each of `count`
/// edits, on a document of `start_chars` chars that grows by one char per
/// insert. Pure and deterministic.
pub fn edit_offsets(
    seed: u32,
    count: u32,
    start_chars: usize,
) -> impl Iterator<Item = (usize, char)> {
    let mut rng = Mulberry32(seed);
    (0..count).scan(start_chars, move |len, _| {
        let at = rng.next() as usize % (*len + 1);
        let letter = char::from(b'a' + u8::try_from(rng.next() % 26).unwrap_or(0));
        *len += 1;
        Some((at, letter))
    })
}

/// FNV-1a, 32 bits, over the UTF-8 bytes of `text`: the check that two
/// editors hold the same text (not a security hash).
#[must_use]
pub fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// What the frontend does after [`Driver::before_frame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing yet: ask for another frame.
    Wait,
    /// Start this search now, as the Search view would, and keep calling.
    StartSearch(SearchScript),
    /// The script is over: print this line (once) and stop.
    Report(String),
    /// Nothing is left to do.
    Finished,
}

/// What the insert phase measured.
#[derive(Debug, Clone, Copy)]
struct Inserted {
    /// Just before the first insert.
    started: Instant,
    /// The inserts alone.
    apply: Duration,
    start_chars: usize,
    chars: usize,
    hash: u32,
    /// Start of the inserts to the end of the frame that presented them.
    frame: Option<Duration>,
}

/// What the undo phase measured.
#[derive(Debug, Clone, Copy)]
struct Undone {
    started: Instant,
    undo: Duration,
    steps: u32,
    chars: usize,
    hash: u32,
    frame: Option<Duration>,
}

#[derive(Debug, Default)]
struct SearchTimes {
    started: Option<Instant>,
    first_seen: bool,
    first: Option<Duration>,
    done_seen: bool,
    complete: Option<Duration>,
    files: usize,
    matches: usize,
    limit_hit: bool,
    state: SearchState,
}

#[derive(Debug)]
enum Phase {
    /// Until a first frame was presented.
    Waiting,
    /// The inserts ran in this frame; its present ends their timing.
    Inserted(Inserted),
    /// The inserts are timed; the undo runs in the next frame.
    UndoDue(Inserted),
    /// The undo ran in this frame; its present ends its timing.
    Undone(Inserted, Undone),
    /// Both are timed; the next frame prints the report.
    Reporting(Inserted, Undone),
    /// The search started; waits for its results and its end.
    Searching(SearchTimes),
    /// The search ended and was timed; the next frame prints the report.
    SearchReporting(SearchTimes),
    Done,
}

/// Runs a [`Script`], one step per frame.
#[derive(Debug)]
pub struct Driver {
    script: Script,
    phase: Phase,
    presented: u32,
}

impl Driver {
    /// A driver for `script`.
    #[must_use]
    pub fn new(script: Script) -> Self {
        Self {
            script,
            phase: Phase::Waiting,
            presented: 0,
        }
    }

    /// Called while a frame is built, after the frontend took the search
    /// results that arrived. Runs the workspace work of the current step.
    pub fn before_frame(&mut self, workspace: &mut Workspace) -> Step {
        let phase = std::mem::replace(&mut self.phase, Phase::Done);
        let (phase, step) = match phase {
            Phase::Waiting if self.presented == 0 => (Phase::Waiting, Step::Wait),
            Phase::Waiting => self.start(workspace),
            Phase::UndoDue(inserted) => undo(workspace, inserted),
            Phase::Reporting(inserted, undone) => (
                Phase::Done,
                Step::Report(edits_line(&self.script, &inserted, &undone)),
            ),
            Phase::Searching(mut times) => {
                watch(workspace, &mut times);
                if times.complete.is_some() {
                    (Phase::SearchReporting(times), Step::Wait)
                } else {
                    (Phase::Searching(times), Step::Wait)
                }
            }
            Phase::SearchReporting(times) => (Phase::Done, Step::Report(search_line(&times))),
            Phase::Done => (Phase::Done, Step::Finished),
            waiting => (waiting, Step::Wait),
        };
        self.phase = phase;
        step
    }

    /// Called right after a frame was presented, at `at`.
    pub fn after_present(&mut self, at: Instant) {
        self.presented = self.presented.saturating_add(1);
        let phase = std::mem::replace(&mut self.phase, Phase::Done);
        self.phase = match phase {
            Phase::Inserted(inserted) => Phase::UndoDue(Inserted {
                frame: Some(at.saturating_duration_since(inserted.started)),
                ..inserted
            }),
            Phase::Undone(inserted, undone) => Phase::Reporting(
                inserted,
                Undone {
                    frame: Some(at.saturating_duration_since(undone.started)),
                    ..undone
                },
            ),
            Phase::Searching(mut times) => {
                if let Some(started) = times.started {
                    let since = at.saturating_duration_since(started);
                    if std::mem::take(&mut times.first_seen) {
                        times.first = Some(since);
                    }
                    if std::mem::take(&mut times.done_seen) {
                        times.complete = Some(since);
                    }
                }
                Phase::Searching(times)
            }
            other => other,
        };
    }

    fn start(&self, workspace: &mut Workspace) -> (Phase, Step) {
        match &self.script {
            Script::Edits { seed, count } => match insert(workspace, *seed, *count) {
                Ok(inserted) => (Phase::Inserted(inserted), Step::Wait),
                Err(why) => (Phase::Done, Step::Report(error_line(&why))),
            },
            Script::Search(search) => (
                Phase::Searching(SearchTimes {
                    started: Some(Instant::now()),
                    ..SearchTimes::default()
                }),
                Step::StartSearch(search.clone()),
            ),
        }
    }
}

fn error_line(why: &str) -> String {
    format!("rustode-gui: bench error: {why}")
}

/// The seeded inserts, one undo step each, on the active document.
fn insert(workspace: &mut Workspace, seed: u32, count: u32) -> Result<Inserted, String> {
    let id = active(workspace)?;
    let start_chars = chars_of(workspace, id)?.0;
    let started = Instant::now();
    for (at, letter) in edit_offsets(seed, count, start_chars) {
        workspace
            .replace(id, Range::empty(at), letter.encode_utf8(&mut [0; 4]))
            .map_err(|err| err.to_string())?;
    }
    let apply = started.elapsed();
    let (chars, hash) = chars_of(workspace, id)?;
    Ok(Inserted {
        started,
        apply,
        start_chars,
        chars,
        hash,
        frame: None,
    })
}

fn active(workspace: &Workspace) -> Result<DocumentId, String> {
    workspace
        .active_document()
        .ok_or_else(|| "no active document".to_owned())
}

/// The length in chars and the hash of a document's text.
fn chars_of(workspace: &Workspace, id: DocumentId) -> Result<(usize, u32), String> {
    let document = workspace
        .document(id)
        .ok_or_else(|| "the document was closed".to_owned())?;
    Ok((document.len_chars(), fnv1a(&document.text())))
}

/// Undoes every step of the active document.
fn undo(workspace: &mut Workspace, inserted: Inserted) -> (Phase, Step) {
    let result = (|| -> Result<Undone, String> {
        let id = active(workspace)?;
        let started = Instant::now();
        let mut steps = 0_u32;
        while workspace.undo(id).map_err(|err| err.to_string())? {
            steps = steps.saturating_add(1);
        }
        let undo = started.elapsed();
        let (chars, hash) = chars_of(workspace, id)?;
        Ok(Undone {
            started,
            undo,
            steps,
            chars,
            hash,
            frame: None,
        })
    })();
    match result {
        Ok(undone) => (Phase::Undone(inserted, undone), Step::Wait),
        Err(why) => (Phase::Done, Step::Report(error_line(&why))),
    }
}

/// Notes in `times` what the search state shows now; the next present
/// timestamps it.
fn watch(workspace: &Workspace, times: &mut SearchTimes) {
    let results = workspace.search_results();
    times.files = results.file_count();
    times.matches = results.match_count();
    times.limit_hit = results.limit_hit();
    if times.first.is_none() && !times.first_seen && times.files > 0 {
        times.first_seen = true;
    }
    if !results.is_running() && times.complete.is_none() && !times.done_seen {
        times.state = results.state();
        times.done_seen = true;
    }
}

fn micros(duration: Option<Duration>) -> String {
    duration.map_or_else(|| "none".to_owned(), |d| d.as_micros().to_string())
}

fn edits_line(script: &Script, inserted: &Inserted, undone: &Undone) -> String {
    let (seed, count) = match script {
        Script::Edits { seed, count } => (*seed, *count),
        Script::Search(_) => (0, 0),
    };
    format!(
        "rustode-gui: bench edits: seed={seed} count={count} start_chars={} \
         apply_us={} apply_frame_us={} applied_chars={} applied_hash={:08x} \
         undo_us={} undo_frame_us={} undo_steps={} undone_chars={} undone_hash={:08x}",
        inserted.start_chars,
        inserted.apply.as_micros(),
        micros(inserted.frame),
        inserted.chars,
        inserted.hash,
        undone.undo.as_micros(),
        micros(undone.frame),
        undone.steps,
        undone.chars,
        undone.hash,
    )
}

fn search_line(times: &SearchTimes) -> String {
    format!(
        "rustode-gui: bench search: files={} matches={} limit_hit={} state={:?} \
         first_result_us={} complete_us={}",
        times.files,
        times.matches,
        times.limit_hit,
        times.state,
        micros(times.first),
        micros(times.complete),
    )
}

/// The query a [`SearchScript`] stands for, for a frontend that starts the
/// search itself.
///
/// # Errors
/// When the pattern is not a valid regular expression.
pub fn search_query(script: &SearchScript) -> Result<crate::SearchQuery, crate::PatternError> {
    let find = FindQuery::new(
        &script.pattern,
        FindOptions {
            regex: script.regex,
            ..FindOptions::default()
        },
    )?;
    let mut query = crate::SearchQuery::new(find);
    query.include.clone_from(&script.include);
    query.exclude.clone_from(&script.exclude);
    Ok(query)
}
