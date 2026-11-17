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
    /// Repetitions on the rustode side.
    pub rustode: RustodeSide,
    /// How the App side is driven.
    pub app: AppSide,
}

/// The scenario kinds the runner implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    /// Open one generated text file.
    OpenFile,
}

impl Kind {
    /// The name used in the list and in reports.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "open-file",
        }
    }
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
#[derive(Debug, Clone, Copy, Deserialize)]
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
                "{}  {}  [fixture {} bytes; rustode {} opens; App {} launches]",
                s.id, s.title, s.fixture.bytes, s.rustode.repetitions, s.app.repetitions
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
                self.rustode.repetitions >= 1,
                "rustode.repetitions must be at least 1",
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
