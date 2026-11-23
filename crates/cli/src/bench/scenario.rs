//! The versioned scenario list, `bench/scenarios.toml` (spec, story 106).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{Failure, display};

/// The scenario list file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct List {
    /// Bumped whenever an existing scenario changes meaning.
    pub version: u32,
    /// The scenarios, in file order.
    #[serde(rename = "scenario")]
    pub scenarios: Vec<Scenario>,
    /// Where the list was read from.
    #[serde(skip)]
    pub path: PathBuf,
}

/// One scenario, run on both sides.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scenario {
    /// Unique, lower-case words joined by `-`.
    pub id: String,
    /// One line for people.
    pub title: String,
    /// What the runner does on each side.
    pub kind: Kind,
    /// The generated input file.
    pub fixture: Fixture,
    /// In-process repetitions on the rustode side (`open-file` only; unused
    /// by `launch`, which has no in-process side).
    #[serde(default)]
    pub rustode: RustodeSide,
    /// How the App side is driven.
    pub app: AppSide,
    /// The rustode GUI side: launches of `rustode-gui` on the same virtual
    /// display. Required by `launch`; optional for `open-file`.
    #[serde(default)]
    pub gui: Option<GuiSide>,
    /// What a `launch` scenario measures. Required by `launch`.
    #[serde(default)]
    pub launch: Option<LaunchSpec>,
    /// The seeded edit script of an `edit-undo` scenario.
    #[serde(default)]
    pub edits: Option<EditsSpec>,
    /// The monorepo and the query of a `search` scenario.
    #[serde(default)]
    pub search: Option<SearchSpec>,
}

/// The scenario kinds the runner implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    /// Open one generated text file.
    OpenFile,
    /// Launch each editor with the generated file and measure the launch
    /// (startup to first frame, or idle memory), with no in-process side.
    Launch,
    /// Open the fixture, apply seeded random inserts to its text model, then
    /// undo all of them (ADR 0024), through each editor's own API, no keys.
    EditUndo,
    /// Search a generated monorepo with one regex through each editor's own
    /// search (ADR 0024), no keys.
    Search,
}

impl Kind {
    /// The name used in the list and in reports.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "open-file",
            Self::Launch => "launch",
            Self::EditUndo => "edit-undo",
            Self::Search => "search",
        }
    }
}

/// The seeded inserts of an `edit-undo` scenario: `count` single-letter
/// inserts at offsets from `rustode_workspace::bench::edit_offsets(seed, …)`,
/// each its own undo step, then every step undone.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditsSpec {
    /// The generator's seed.
    pub seed: u32,
    /// How many inserts.
    pub count: u32,
}

/// The monorepo of a `search` scenario and the search run over it. The tree
/// is generated (`fixture::write_tree`): `files` searched source files of
/// `[scenario.fixture].bytes` bytes and `line_bytes`-byte lines, plus files
/// that a default exclude, an ignore file, the include globs or the exclude
/// globs must keep out of the results.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchSpec {
    /// Source files under `src/`.
    pub files: u32,
    /// The generator's seed.
    pub seed: u32,
    /// The pattern, as typed in each editor's search box.
    pub pattern: String,
    /// Use Regular Expression.
    pub regex: bool,
    /// "files to include", comma-separated globs.
    pub include: String,
    /// "files to exclude", comma-separated globs.
    pub exclude: String,
}

/// A generated fixture: numbered ASCII lines with LF.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fixture {
    /// Exact file size.
    pub bytes: u64,
    /// Bytes per line, LF included.
    pub line_bytes: u32,
}

/// Repetitions on the rustode side.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RustodeSide {
    /// Unrecorded runs first.
    pub warmup: u32,
    /// Recorded runs.
    pub repetitions: u32,
}

/// How the App side is driven.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppSide {
    /// Unrecorded launches first.
    pub warmup: u32,
    /// Recorded launches.
    pub repetitions: u32,
    /// Longest wait for each launch to reach its end point.
    pub timeout_s: u32,
    /// How often the editor is checked over CDP.
    pub poll_ms: u32,
    /// The CDP driver script, relative to the list file.
    pub driver: PathBuf,
    /// The `settings.json` for each fresh profile, relative to the list file.
    pub settings: PathBuf,
    /// Record a Chromium trace of each launch (categories `viz` and
    /// `blink.user_timing`) to find the App's first presented frame after the
    /// first line is in the DOM (gaps.md G22). Off unless asked for.
    #[serde(default)]
    pub trace: bool,
}

/// Launches of the rustode GUI (`rustode-gui`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GuiSide {
    /// Unrecorded launches first.
    pub warmup: u32,
    /// Recorded launches.
    pub repetitions: u32,
    /// Longest wait for each launch to present its first frame.
    pub timeout_s: u32,
}

/// What a `launch` scenario measures and under which cache state.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchSpec {
    /// What is read from each launch.
    pub measure: Measure,
    /// The state of the operating system's file cache at each launch.
    pub cache: Cache,
    /// `idle`: seconds to wait after the editor is ready before memory is read.
    #[serde(default)]
    pub settle_s: u32,
}

/// What a launch scenario reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Measure {
    /// Time from the spawn to the first frame and to the fixture's first line.
    Startup,
    /// Memory of the editor's processes `settle_s` after it is ready.
    Idle,
}

/// The file-cache state of an editor's own files at each launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Cache {
    /// Before each launch the editor's files are dropped from the page cache
    /// (`posix_fadvise(DONTNEED)`, `bench/evict.py`).
    Cold,
    /// Nothing is dropped; the previous launch left its files cached.
    Warm,
}

impl Cache {
    /// The name used in the list and in reports.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

impl Measure {
    /// The name used in the list, the driver config and reports.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Idle => "idle",
        }
    }
}

impl List {
    /// Reads and validates the list at `path`.
    pub(crate) fn load(path: &Path) -> Result<Self, Failure> {
        let shown = display::path(path);
        let text = std::fs::read_to_string(path).map_err(|err| {
            Failure::Usage(format!(
                "cannot read the scenario list {shown}: {err} (run from the project root or pass --scenarios)"
            ))
        })?;
        let mut list: Self =
            toml::from_str(&text).map_err(|err| Failure::Usage(format!("{shown}: {err}")))?;
        path.clone_into(&mut list.path);
        let problems = list.problems();
        if problems.is_empty() {
            Ok(list)
        } else {
            Err(Failure::Usage(format!("{shown}: {}", problems.join("; "))))
        }
    }

    /// The scenario with `id`.
    pub(crate) fn get(&self, id: &str) -> Result<&Scenario, Failure> {
        self.scenarios.iter().find(|s| s.id == id).ok_or_else(|| {
            let known: Vec<&str> = self.scenarios.iter().map(|s| s.id.as_str()).collect();
            Failure::Usage(format!(
                "no scenario '{id}' in {}; known: {}",
                display::path(&self.path),
                known.join(", ")
            ))
        })
    }

    /// The `--list` output.
    pub(crate) fn describe(&self) -> String {
        let mut out = format!(
            "{} (list version {})\n",
            display::path(&self.path),
            self.version
        );
        for s in &self.scenarios {
            let _ = writeln!(
                out,
                "{}  {}  [fixture {} bytes; rustode {} opens, {} GUI launches; App {} launches]",
                s.id,
                s.title,
                s.fixture.bytes,
                s.rustode.repetitions,
                s.gui.map_or(0, |g| g.repetitions),
                s.app.repetitions
            );
        }
        out
    }

    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.scenarios.is_empty() {
            problems.push("the list has no [[scenario]]".to_owned());
        }
        for (i, s) in self.scenarios.iter().enumerate() {
            if self.scenarios[..i].iter().any(|other| other.id == s.id) {
                problems.push(format!("scenario id '{}' appears twice", s.id));
            }
            problems.extend(s.problems().into_iter().map(|p| format!("{}: {p}", s.id)));
        }
        problems
    }
}

impl Scenario {
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let id_ok = !self.id.is_empty()
            && self.id.split('-').all(|w| {
                !w.is_empty()
                    && w.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            });
        let checks = [
            (id_ok, "the id must be lower-case words joined by '-'"),
            (
                (16..=4096).contains(&self.fixture.line_bytes),
                "fixture.line_bytes must be 16 to 4096",
            ),
            (
                self.fixture.bytes >= u64::from(self.fixture.line_bytes),
                "fixture.bytes must hold at least one whole line",
            ),
            (
                self.kind != Kind::OpenFile || self.rustode.repetitions >= 1,
                "rustode.repetitions must be at least 1",
            ),
            (
                self.kind != Kind::Launch || (self.launch.is_some() && self.gui.is_some()),
                "a launch scenario needs [scenario.launch] and [scenario.gui]",
            ),
            (
                self.kind == Kind::Launch || self.launch.is_none(),
                "[scenario.launch] belongs to launch scenarios",
            ),
            (
                self.kind != Kind::EditUndo
                    || self
                        .edits
                        .is_some_and(|e| (1..=1_000_000).contains(&e.count)),
                "an edit-undo scenario needs [scenario.edits] with count 1 to 1000000",
            ),
            (
                self.kind == Kind::EditUndo || self.edits.is_none(),
                "[scenario.edits] belongs to edit-undo scenarios",
            ),
            (
                self.kind != Kind::Search
                    || self
                        .search
                        .as_ref()
                        .is_some_and(|s| (1..=100_000).contains(&s.files) && !s.pattern.is_empty()),
                "a search scenario needs [scenario.search] with files 1 to 100000 and a pattern",
            ),
            (
                self.kind == Kind::Search || self.search.is_none(),
                "[scenario.search] belongs to search scenarios",
            ),
            (
                !matches!(self.kind, Kind::EditUndo | Kind::Search) || self.gui.is_some(),
                "edit-undo and search scenarios need [scenario.gui]",
            ),
            (
                self.gui
                    .is_none_or(|g| g.repetitions >= 1 && g.timeout_s >= 1),
                "gui.repetitions and gui.timeout_s must be at least 1",
            ),
            (
                self.launch
                    .is_none_or(|l| l.measure != Measure::Idle || l.settle_s >= 1),
                "launch.settle_s must be at least 1 for an idle scenario",
            ),
            (
                self.app.repetitions >= 1,
                "app.repetitions must be at least 1",
            ),
            (self.app.timeout_s >= 1, "app.timeout_s must be at least 1"),
            (self.app.poll_ms >= 1, "app.poll_ms must be at least 1"),
        ];
        for (ok, problem) in checks {
            if !ok {
                problems.push(problem.to_owned());
            }
        }
        problems
    }
}
