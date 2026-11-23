//! The run's report, as text (also printed) and as JSON.
//!
//! Every result carries a label: `measured` means read from a clock or from
//! `/proc` during this run; `estimated` means derived from an assumption the
//! report names. Scenario inputs are listed apart, as inputs.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::app_side::{DriverResults, Launch, Outcome};
use super::fixture::TreeSummary;
use super::guard::{ABORT_BELOW_MB, ABORT_PSI, MIN_AVAILABLE_MB};
use super::gui_side;
use super::plan::{CDP_PORT, Plan};
use super::scenario::{Cache, Kind};
use super::{Status, display, guard};

/// Why the two sides cannot be compared yet (spec, "Performance contract").
const NOT_LIKE_FOR_LIKE: &str = "The two sides are not like-for-like until the GUI exists (M3): \
rustode's number is an in-process open with no process start, window or rendering; the App's \
numbers include starting Electron and the workbench and rendering on a software-rendered virtual \
display. Do not divide one by the other.";

const NOT_LIKE_FOR_LIKE_IN_PROCESS: &str = "rustode's in-process `Workspace::open` has no process start, window or rendering and is not comparable with any App number.";

/// The frame events differ between the two editors (ADR 0024; gaps.md G15).
const BRACKET: &str = "The two editors have no common 'first frame' event: rustode's is the return from its first wgpu present, the App's closest events are Chromium's first contentful paint and the first fixture line seen in the DOM (polled over CDP). The comparison is therefore approximate: read the rustode number against the interval between the App's two, and do not divide one by the other. When the App launches were traced (rows 'launch to first swap' and 'launch to second swap'), the first swap is the closest counterpart of rustode's first frame: the display compositor's first drawn-and-swapped frame after the first line entered the DOM (gaps.md G22); say which event a ratio uses, and that the swap may be one frame early (the second swap bounds it).";

/// What may be concluded from an `edit-undo` scenario.
const EDIT_NOTE: &str = "Both editors applied the same seeded edits (checked by the hash of the text after the inserts and after the undo, below), but through different APIs: rustode's `Workspace::replace` takes a char offset and returns a result, the App's `TextModel.pushEditOperations` takes a line and column (so `getPositionAt` is inside its timed loop) and fires its content-change events, which the visible editor, minimap, bracket matching and tokenization listen to on every edit; rustode's GUI redraws once after the whole script. The 'to next frame' times end at the frame presented after the step (rustode: the return from `RedrawRequested`; App: the second `requestAnimationFrame` callback after the step), which are not the same event and the App's is paced by the compositor's frame callbacks. Treat the ratio as 'what a programmatic bulk edit costs through each editor's own model API', not as typing speed.";

/// What may be concluded from a `search` scenario.
const SEARCH_NOTE: &str = "Both editors searched the same generated tree with the same pattern, include and exclude globs, default excludes and ignore files, and each through its own search (rustode's `Workspace::start_search`, streamed into the Search view; the App's `workbench.action.findInFiles` command, which runs ripgrep). The pattern is written to mean the same in the Rust `regex` crate and in ripgrep's engine (ASCII classes and word boundaries). 'First result' is the first result row drawn (rustode: the frame presented after the first file arrived; App: the first `.monaco-list-row` of the search view, seen by a MutationObserver in the page); 'complete' is when the search has ended and its results are shown (rustode: the frame presented after the search state became `Done`; App: the sidebar's progress bar went from `active` to `done`, and the result message had stopped changing). The counts are checked against each other and against the generator's oracle below.";

/// Summary of samples with nearest-rank percentiles, the definition that
/// `rustode_workspace::trace::LatencyRecorder` uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stats {
    n: usize,
    min: f64,
    p50: f64,
    p95: f64,
    max: f64,
}

impl Stats {
    fn of(samples: &[f64]) -> Option<Self> {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let (&min, &max) = (sorted.first()?, sorted.last()?);
        let rank = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100).max(1) - 1];
        Some(Self {
            n: sorted.len(),
            min,
            p50: rank(50),
            p95: rank(95),
            max,
        })
    }
}

/// One result: a metric on one side, measured in this run.
struct Row {
    side: &'static str,
    metric: &'static str,
    name: &'static str,
    unit: &'static str,
    decimals: usize,
    samples: Vec<f64>,
    meaning: String,
}

/// What was measured about the machine and the builds at the start.
#[derive(Debug)]
pub(crate) struct Environment {
    git: String,
    kernel: String,
    cpu: String,
    cpus: usize,
    mem_total_mb: Option<u64>,
    mem_available_mb: Option<u64>,
    app_version: Option<String>,
}

impl Environment {
    /// Reads the environment for `plan`.
    pub(crate) fn read(plan: &Plan) -> Self {
        let first_line = |path: &str| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|t| t.lines().next().map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned())
        };
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|t| {
                t.lines().find_map(|l| {
                    l.strip_prefix("model name")
                        .map(|r| r.trim_start_matches([' ', '\t', ':']).to_owned())
                })
            })
            .unwrap_or_else(|| "unknown CPU".to_owned());
        let app_version = std::fs::read_to_string(plan.version_file())
            .ok()
            .map(|text| {
                let parts: Vec<&str> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect();
                match parts.as_slice() {
                    [version, commit, arch, ..] => format!("{version}, commit {commit}, {arch}"),
                    other => other.join(", "),
                }
            });
        Self {
            git: git_state(&plan.cwd),
            kernel: first_line("/proc/sys/kernel/osrelease"),
            cpu,
            cpus: std::thread::available_parallelism().map_or(0, std::num::NonZero::get),
            mem_total_mb: guard::meminfo_mb(&plan.proc_dir, "MemTotal:").ok(),
            mem_available_mb: guard::mem_available_mb(&plan.proc_dir).ok(),
            app_version,
        }
    }
}

fn git_state(cwd: &Path) -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    match (
        git(&["rev-parse", "--short=12", "HEAD"]),
        git(&["status", "--porcelain", "--untracked-files=no"]),
    ) {
        (Some(head), Some(changes)) if changes.is_empty() => format!("git {head} (clean)"),
        (Some(head), Some(_)) => format!("git {head} (uncommitted changes)"),
        _ => "not a git checkout".to_owned(),
    }
}

/// Everything the report says about a finished run.
pub(crate) struct Report<'a> {
    /// The plan that ran.
    pub plan: &'a Plan<'a>,
    /// The final run id (with `-INVALIDA` or `-INTERROMPIDA` when it failed).
    pub run_id: String,
    /// The final run folder.
    pub run_folder: PathBuf,
    /// When the run ended.
    pub finished: SystemTime,
    /// How the run went.
    pub status: Status,
    /// Why, when not ok.
    pub reasons: Vec<String>,
    /// The environment at the start.
    pub environment: Environment,
    /// SHA-256 of the fixture.
    pub fixture_sha256: Option<String>,
    /// The generated monorepo of a `search` scenario.
    pub tree: Option<TreeSummary>,
    /// The recorded rustode opens, if that side ran and succeeded.
    pub rustode: Option<Vec<Duration>>,
    /// Why the in-process opens were skipped (too little free memory), if they were.
    pub rustode_skipped: Option<String>,
    /// The rustode GUI side, if it ran.
    pub gui: Option<gui_side::Outcome>,
    /// The App side, if it ran.
    pub app: Option<Outcome>,
}

impl Report<'_> {
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(durations) = &self.rustode {
            rows.push(Row {
                side: "rustode",
                metric: "workspace_open",
                name: "Workspace::open",
                unit: "ms",
                decimals: 3,
                samples: durations.iter().map(|d| round3(d.as_secs_f64() * 1e3)).collect(),
                meaning: "`Workspace::open` on a fresh `Workspace` in this process: read the file, detect encoding and line endings, build the buffer, hash the bytes for external-change detection and start watching the file; no window and no rendering (rustode has no GUI until M3); clock: `std::time::Instant`".to_owned(),
            });
        }
        if let Some(results) = self.gui.as_ref().and_then(|g| g.results.as_ref()) {
            let recorded: Vec<&Launch> = results.launches.iter().filter(|l| !l.warmup).collect();
            rows.extend(gui_rows(&recorded));
            rows.extend(script_rows("rustode", &recorded));
        }
        if let Some(results) = self.app.as_ref().and_then(|a| a.results.as_ref()) {
            let recorded: Vec<&Launch> = results.launches.iter().filter(|l| !l.warmup).collect();
            rows.extend(app_rows(self.plan, &recorded));
            rows.extend(script_rows("app", &recorded));
        }
        rows.retain(|row| !row.samples.is_empty());
        rows
    }

    /// The text report.
    pub(crate) fn text(&self) -> String {
        let mut out = String::from("rustode bench report\n\n");
        let plan = self.plan;
        for (key, value) in [
            ("run", self.run_id.clone()),
            ("status", self.status.as_str().to_owned()),
            ("command", display::command(&plan.argv)),
            ("folder", display::path(&plan.cwd)),
            ("run folder", display::path(&self.run_folder)),
            ("scenario", plan.scenario_line()),
            (
                "time",
                format!(
                    "{} to {} (UTC)",
                    display::utc(plan.started),
                    display::utc(self.finished)
                ),
            ),
        ] {
            let _ = writeln!(out, "{key:<12}{value}");
        }
        self.text_inputs(&mut out);
        self.text_environment(&mut out);
        self.text_gui(&mut out);
        self.text_app(&mut out);
        self.text_results(&mut out);
        self.text_notes(&mut out);
        out
    }

    /// The fixture's line of the inputs: a file with its hash, or the
    /// monorepo with its query and oracle.
    fn text_fixture(&self, out: &mut String) {
        let s = self.plan.scenario;
        if let (Some(tree), Some(search)) = (&self.tree, &s.search) {
            let _ = writeln!(
                out,
                "  fixture     fixtures/{}/: a generated monorepo (seed {}): {} files, {} bytes, {} source files of {} bytes in lines of {} bytes; digest fnv1a64 {:016x}",
                s.id,
                search.seed,
                tree.files,
                tree.bytes,
                search.files,
                s.fixture.bytes,
                s.fixture.line_bytes,
                tree.digest
            );
            let _ = writeln!(
                out,
                "  query       {} /{}/ include '{}' exclude '{}' (default excludes and ignore files on)",
                if search.regex { "regex" } else { "text" },
                search.pattern,
                search.include,
                search.exclude
            );
            if TreeSummary::expects(search) {
                let _ = writeln!(
                    out,
                    "  oracle      the generator planted the matches: {} searched files, {} with matches, {} matches (counted from the text it wrote, not by a regex engine)",
                    tree.searched_files, tree.expected_files, tree.expected_matches
                );
            }
        } else {
            let _ = writeln!(
                out,
                "  fixture     fixtures/{}.txt: {} bytes, numbered ASCII lines of {} bytes with LF; sha256 {}",
                s.id,
                s.fixture.bytes,
                s.fixture.line_bytes,
                self.fixture_sha256.as_deref().unwrap_or("unknown")
            );
        }
    }

    fn text_inputs(&self, out: &mut String) {
        let s = self.plan.scenario;
        let _ = writeln!(out, "\ninputs (from the scenario list; not results)");
        self.text_fixture(out);
        if let Some(edits) = s.edits {
            let _ = writeln!(
                out,
                "  edits       {} single-letter inserts at seeded random offsets (mulberry32 seed {}), each its own undo step, then every step undone",
                edits.count, edits.seed
            );
        }
        if self.plan.scenario.kind == Kind::OpenFile {
            let _ = writeln!(
                out,
                "  rustode     {} warm-up + {} recorded in-process opens",
                s.rustode.warmup, s.rustode.repetitions
            );
        }
        if let Some(gui) = s.gui {
            let _ = writeln!(
                out,
                "  rustode GUI {} warm-up + {} recorded launches; timeout {} s each",
                gui.warmup, gui.repetitions, gui.timeout_s
            );
        }
        if let Some(launch) = s.launch {
            let _ = writeln!(
                out,
                "  launch      measures {}; {} cache (editor files {}){}",
                launch.measure.as_str(),
                launch.cache.as_str(),
                if launch.cache == Cache::Cold {
                    "dropped from the page cache before each launch"
                } else {
                    "left in the page cache by the previous launch"
                },
                if launch.settle_s > 0 {
                    format!(
                        "; memory read {} s after the editor is ready",
                        launch.settle_s
                    )
                } else {
                    String::new()
                }
            );
        }
        let _ = writeln!(
            out,
            "  App         {} warm-up + {} recorded launches; timeout {} s each; first line checked every {} ms",
            s.app.warmup, s.app.repetitions, s.app.timeout_s, s.app.poll_ms
        );
        let _ = writeln!(
            out,
            "  guards      the App starts only with MemAvailable >= {MIN_AVAILABLE_MB} MB; the QA watchdog aborts it below {ABORT_BELOW_MB} MB, or with PSI some avg10 > {ABORT_PSI} for 5 samples"
        );
    }

    fn text_environment(&self, out: &mut String) {
        let e = &self.environment;
        let build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let mb = |v: Option<u64>| v.map_or_else(|| "unknown".to_owned(), |v| format!("{v} MB"));
        let _ = writeln!(out, "\nenvironment (measured at the start of the run)");
        let _ = writeln!(
            out,
            "  rustode     {}, {}, {build} build",
            env!("CARGO_PKG_VERSION"),
            e.git
        );
        if let Some(version) = &e.app_version {
            let _ = writeln!(
                out,
                "  App         {version} (02-package-analysis/version.txt)"
            );
        }
        let _ = writeln!(
            out,
            "  machine     Linux {}; {}; {} CPUs; MemTotal {}; MemAvailable {}",
            e.kernel,
            e.cpu,
            e.cpus,
            mb(e.mem_total_mb),
            mb(e.mem_available_mb)
        );
    }

    fn text_gui(&self, out: &mut String) {
        let Some(gui) = &self.gui else {
            return;
        };
        let bin = display::path(&self.plan.gui_bin);
        let profile = if bin.contains("/release/") {
            "release build"
        } else if bin.contains("/debug/") {
            "debug build"
        } else {
            "build profile unknown"
        };
        let _ = writeln!(out, "\nrustode GUI (measured)");
        let _ = writeln!(
            out,
            "  binary      {bin} ({profile}, from its path); {}; sha256 {}",
            gui.version.as_deref().unwrap_or("version unknown"),
            gui.binary_sha256.as_deref().unwrap_or("unknown")
        );
        let _ = writeln!(
            out,
            "  rendering   {}",
            self.plan
                .gui_env()
                .iter()
                .filter(|(k, _)| matches!(
                    k.as_str(),
                    "WGPU_BACKEND"
                        | "LIBGL_ALWAYS_SOFTWARE"
                        | "GALLIUM_DRIVER"
                        | "VK_ICD_FILENAMES"
                ))
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Some(results) = &gui.results {
            let _ = writeln!(
                out,
                "  driver      Node {}; status {}",
                results.node.as_deref().unwrap_or("unknown"),
                results.status
            );
        }
    }

    fn text_app(&self, out: &mut String) {
        let Some(app) = &self.app else {
            let _ = writeln!(out, "\nApp: skipped (--side rustode)");
            return;
        };
        let _ = writeln!(
            out,
            "\nApp integrity ({} against {}; measured)",
            display::path(&self.plan.app_dir()),
            display::path(&self.plan.manifest())
        );
        let _ = writeln!(out, "  before      {}", app.before.summary());
        if let Some(copy) = &app.copy {
            let dir = app
                .copy_dir
                .as_deref()
                .map_or_else(Default::default, display::path);
            let _ = writeln!(out, "  copy        {} ({dir})", copy.summary());
        }
        if let Some(after) = &app.after {
            let _ = writeln!(out, "  after       {}", after.summary());
        }
        if app
            .copy
            .as_ref()
            .is_some_and(super::integrity::Integrity::ok)
        {
            let _ = writeln!(
                out,
                "  The App ran from a verified copy outside 01-app/, deleted after the run{}.",
                if app.reaper { "" } else { " (no reaper unit)" }
            );
            let count = app.copy_writes.len();
            let _ = writeln!(
                out,
                "  The App added {count} file{} to its copy during the run (measured; app/copy-writes.txt){}",
                if count == 1 { "" } else { "s" },
                if count == 0 { "." } else { ":" }
            );
            for file in app.copy_writes.iter().take(5) {
                let _ = writeln!(out, "    {file}");
            }
        }
        if let Some(results) = &app.results {
            if let Some(gpu) = &results.gpu {
                let _ = writeln!(
                    out,
                    "  rendering   {}",
                    [
                        "gl_renderer",
                        "gl_implementation",
                        "gpu_compositing",
                        "opengl",
                        "vulkan"
                    ]
                    .iter()
                    .filter_map(|k| Some(format!("{k}={}", gpu.get(k)?.as_str()?)))
                    .collect::<Vec<_>>()
                    .join("; ")
                );
            }
            let _ = writeln!(
                out,
                "  driver      Node {}, playwright-core {}; status {}",
                results.node.as_deref().unwrap_or("unknown"),
                results.playwright_core.as_deref().unwrap_or("unknown"),
                results.status
            );
        }
    }

    fn text_results(&self, out: &mut String) {
        let rows = self.rows();
        let _ = writeln!(out, "\nresults (nearest-rank percentiles)");
        if rows.is_empty() {
            let _ = writeln!(out, "  none");
            return;
        }
        let _ = writeln!(
            out,
            "  {:<8} {:<26} {:<4} {:>3} {:>10} {:>10} {:>10} {:>10}  label",
            "side", "metric", "unit", "n", "min", "p50", "p95", "max"
        );
        for row in &rows {
            let Some(st) = Stats::of(&row.samples) else {
                continue;
            };
            let d = row.decimals;
            let _ = writeln!(
                out,
                "  {:<8} {:<26} {:<4} {:>3} {:>10.d$} {:>10.d$} {:>10.d$} {:>10.d$}  measured",
                side_name(row.side),
                row.name,
                row.unit,
                st.n,
                st.min,
                st.p50,
                st.p95,
                st.max
            );
        }
        let _ = writeln!(out, "\nwhat the results measure");
        for row in &rows {
            let _ = writeln!(
                out,
                "  - {} {}: {}",
                side_name(row.side),
                row.name,
                row.meaning
            );
        }
    }

    fn notes(&self) -> Vec<(Option<&'static str>, String)> {
        let mut notes = Vec::new();
        if let Some(app) = self.app.as_ref().filter(|a| a.results.is_some()) {
            notes.push((
                Some("estimated"),
                format!(
                    "Each App time is late by up to one polling interval plus one CDP round trip: about {} ms for the first line and 25 ms for the CDP endpoint and the workbench (estimated from the polling intervals, not measured).",
                    self.plan.scenario.app.poll_ms
                ),
            ));
            if let Some(note) = app.results.as_ref().and_then(sigkill_note) {
                notes.push((Some("measured"), note));
            }
            if app.leftovers_killed {
                notes.push((
                    None,
                    "App processes were still running after the driver finished and were killed."
                        .to_owned(),
                ));
            }
        }
        if cfg!(debug_assertions) && (self.rustode.is_some() || self.gui.is_some()) {
            notes.push((
                None,
                "rustode ran as a debug build; build with --release for numbers worth comparing."
                    .to_owned(),
            ));
        }
        if let Some(why) = &self.rustode_skipped {
            notes.push((
                Some("measured"),
                format!(
                    "rustode's in-process `Workspace::open` was not measured: {why}. The GUI and App results do not depend on it."
                ),
            ));
        }
        notes.extend(self.outcome_notes());
        notes.extend(self.equivalence_notes());
        notes.push((None, self.comparison_note()));
        for reason in &self.reasons {
            notes.push((Some("problem"), reason.clone()));
        }
        notes
    }

    /// Launches that never reached their end point, for each side.
    fn outcome_notes(&self) -> Vec<(Option<&'static str>, String)> {
        let mut notes = Vec::new();
        let sides = [
            (
                "rustode GUI",
                self.gui.as_ref().and_then(|g| g.results.as_ref()),
            ),
            ("App", self.app.as_ref().and_then(|a| a.results.as_ref())),
        ];
        for (name, results) in sides {
            let Some(results) = results else { continue };
            let recorded: Vec<&Launch> = results.launches.iter().filter(|l| !l.warmup).collect();
            let failed: Vec<String> = recorded
                .iter()
                .filter_map(|l| {
                    let outcome = l.outcome.as_deref()?;
                    let complaint = l.complaint.as_deref().filter(|c| !c.is_empty());
                    Some(format!(
                        "{} ({outcome}: {}{})",
                        l.label,
                        l.outcome_detail.as_deref().unwrap_or(""),
                        complaint
                            .map_or_else(String::new, |c| format!("; the workbench said: {c}"))
                    ))
                })
                .collect();
            let warm: Vec<String> = recorded
                .iter()
                .filter(|l| l.cache_reference_resident_pct.is_some_and(|pct| pct >= 1.0))
                .map(|l| {
                    format!(
                        "{} ({:.1}% after {} attempts)",
                        l.label,
                        l.cache_reference_resident_pct.unwrap_or_default(),
                        l.evict_attempts.unwrap_or_default()
                    )
                })
                .collect();
            if !warm.is_empty() {
                notes.push((
                    Some("measured"),
                    format!(
                        "{name}: {} of {} recorded launches were not fully cold: part of the editor's main file was still in the page cache after the eviction: {}.",
                        warm.len(),
                        recorded.len(),
                        warm.join("; ")
                    ),
                ));
            }
            if !failed.is_empty() {
                notes.push((
                    Some("measured"),
                    format!(
                        "{name}: {} of {} recorded launches never reached the end point and have no time in the results above: {}.",
                        failed.len(),
                        recorded.len(),
                        failed.join("; ")
                    ),
                ));
            }
        }
        notes
    }

    /// Whether both editors did the same work: the same text after the edits
    /// and after the undo, the same result counts as each other and as the
    /// generator's oracle.
    fn equivalence_notes(&self) -> Vec<(Option<&'static str>, String)> {
        let recorded = |results: Option<&DriverResults>| -> Vec<Launch> {
            results
                .map(|r| r.launches.iter().filter(|l| !l.warmup).cloned().collect())
                .unwrap_or_default()
        };
        let gui = recorded(self.gui.as_ref().and_then(|g| g.results.as_ref()));
        let app = recorded(self.app.as_ref().and_then(|a| a.results.as_ref()));
        let expected = self.tree.as_ref().filter(|_| {
            self.plan
                .scenario
                .search
                .as_ref()
                .is_some_and(TreeSummary::expects)
        });
        let checks: [Check; 6] = [
            (
                "text hash after the inserts",
                |l| l.edit_applied_hash.clone(),
                None,
            ),
            (
                "text hash after the undo",
                |l| l.edit_undone_hash.clone(),
                None,
            ),
            (
                "chars after the inserts",
                |l| l.edit_applied_chars.map(|v| v.to_string()),
                None,
            ),
            (
                "undo steps",
                |l| l.edit_undo_steps.map(|v| v.to_string()),
                self.plan.scenario.edits.map(|e| e.count.to_string()),
            ),
            (
                "files with matches",
                |l| l.search_files.map(|v| v.to_string()),
                expected.map(|t| t.expected_files.to_string()),
            ),
            (
                "matches",
                |l| l.search_matches.map(|v| v.to_string()),
                expected.map(|t| t.expected_matches.to_string()),
            ),
        ];
        checks
            .into_iter()
            .filter_map(|(what, f, oracle)| compare_note(what, &gui, &app, f, oracle))
            .collect()
    }

    /// What may and may not be concluded from the two sides' numbers.
    fn comparison_note(&self) -> String {
        match self.plan.scenario.kind {
            Kind::OpenFile if self.plan.scenario.gui.is_some() => format!(
                "{NOT_LIKE_FOR_LIKE_IN_PROCESS} Compare the GUI's \"launch to first frame\" with the App's \"launch to first paint\" and \"launch to first line shown\" (a bracket, not one number): the GUI's first frame already shows the file, Chromium's first paint is not tied to the file, and the first line is polled over CDP. {BRACKET}"
            ),
            Kind::OpenFile => NOT_LIKE_FOR_LIKE.to_owned(),
            Kind::Launch => format!(
                "{BRACKET} Both editors ran on the same virtual display with software rendering, with the same fixture on their command line."
            ),
            Kind::EditUndo => EDIT_NOTE.to_owned(),
            Kind::Search => SEARCH_NOTE.to_owned(),
        }
    }

    fn text_notes(&self, out: &mut String) {
        let _ = writeln!(out, "\nnotes");
        for (label, text) in self.notes() {
            let text = text.replace('\n', "\n    ");
            match label {
                Some(label) => {
                    let _ = writeln!(out, "  - ({label}) {text}");
                }
                None => {
                    let _ = writeln!(out, "  - {text}");
                }
            }
        }
    }

    /// The JSON report.
    pub(crate) fn json(&self) -> Value {
        let plan = self.plan;
        let s = plan.scenario;
        let results: Vec<Value> = self.rows().iter().filter_map(row_json).collect();
        let notes: Vec<Value> = self
            .notes()
            .into_iter()
            .map(|(label, text)| json!({ "label": label, "text": text }))
            .collect();
        json!({
            "schema": "rustode-bench-report/1",
            "run": self.run_id,
            "status": self.status.as_str(),
            "reasons": self.reasons,
            "command": display::command(&plan.argv),
            "folder": display::path(&plan.cwd),
            "run_folder": display::path(&self.run_folder),
            "started": display::utc(plan.started),
            "finished": display::utc(self.finished),
            "scenario": {
                "id": s.id,
                "title": s.title,
                "kind": s.kind.as_str(),
                "list": display::path(&plan.list.path),
                "list_version": plan.list.version,
            },
            "inputs": self.inputs_json(),
            "environment": self.environment_json(),
            "integrity": self.app.as_ref().map(|app| json!({
                "app_dir": display::path(&plan.app_dir()),
                "manifest": display::path(&plan.manifest()),
                "before": app.before.to_json(),
                "copy": app.copy.as_ref().map(super::integrity::Integrity::to_json),
                "after": app.after.as_ref().map(super::integrity::Integrity::to_json),
            })),
            "gui": self.gui_json(),
            "app": self.app_json(),
            "results": results,
            "notes": notes,
            "like_for_like": false,
        })
    }

    fn inputs_json(&self) -> Value {
        let plan = self.plan;
        let s = plan.scenario;
        json!({
            "fixture": {
                "path": if self.tree.is_some() { format!("fixtures/{}/", s.id) } else { format!("fixtures/{}.txt", s.id) },
                "tree": self.tree.as_ref().map(|t| json!({
                    "files": t.files, "bytes": t.bytes, "digest_fnv1a64": format!("{:016x}", t.digest),
                    "searched_files": t.searched_files, "expected_files": t.expected_files, "expected_matches": t.expected_matches,
                    "oracle_applies": s.search.as_ref().is_some_and(TreeSummary::expects),
                })),
                "bytes": s.fixture.bytes,
                "line_bytes": s.fixture.line_bytes,
                "sha256": self.fixture_sha256,
                "first_line": plan.first_line(),
            },
            "rustode": { "warmup": s.rustode.warmup, "repetitions": s.rustode.repetitions },
            "script": plan.script(),
            "gui": s.gui.map(|g| json!({ "warmup": g.warmup, "repetitions": g.repetitions, "timeout_s": g.timeout_s })),
            "launch": s.launch.map(|l| json!({ "measure": l.measure.as_str(), "cache": l.cache.as_str(), "settle_s": l.settle_s })),
            "app": {
                "warmup": s.app.warmup,
                "repetitions": s.app.repetitions,
                "timeout_s": s.app.timeout_s,
                "poll_ms": s.app.poll_ms,
                "cdp_port": CDP_PORT,
                "driver": display::path(&plan.driver()),
                "settings": display::path(&plan.settings()),
            },
            "guards": {
                "min_available_mb": MIN_AVAILABLE_MB,
                "abort_below_mb": ABORT_BELOW_MB,
                "abort_psi_some_avg10": ABORT_PSI,
            },
        })
    }

    fn environment_json(&self) -> Value {
        let e = &self.environment;
        json!({
            "label": "measured",
            "rustode": env!("CARGO_PKG_VERSION"),
            "git": e.git,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "kernel": e.kernel,
            "cpu": e.cpu,
            "cpus": e.cpus,
            "mem_total_mb": e.mem_total_mb,
            "mem_available_mb": e.mem_available_mb,
            "app_version": e.app_version,
        })
    }

    fn gui_json(&self) -> Value {
        let Some(gui) = &self.gui else {
            return json!({ "status": "skipped" });
        };
        let results = gui.results.as_ref();
        json!({
            "status": gui.status.as_str(),
            "reason": (!gui.reasons.is_empty()).then(|| gui.reasons.join("\n")),
            "binary": display::path(&self.plan.gui_bin),
            "binary_sha256": gui.binary_sha256,
            "version": gui.version,
            "leftovers_killed": gui.leftovers_killed,
            "env": self.plan.gui_env().into_iter().collect::<std::collections::BTreeMap<_, _>>(),
            "node": results.and_then(|r| r.node.clone()),
            "launches": results.map(|r| launches_json(&r.launches)),
        })
    }

    fn app_json(&self) -> Value {
        let Some(app) = &self.app else {
            return json!({ "status": "skipped", "reason": "--side rustode" });
        };
        let results = app.results.as_ref();
        json!({
            "status": app.status.as_str(),
            "reason": (!app.reasons.is_empty()).then(|| app.reasons.join("\n")),
            "copy": app.copy_dir.as_deref().map(display::path),
            "copy_writes": app.copy_writes,
            "reaper": app.reaper,
            "leftovers_killed": app.leftovers_killed,
            "node": results.and_then(|r| r.node.clone()),
            "playwright_core": results.and_then(|r| r.playwright_core.clone()),
            "gpu": results.and_then(|r| r.gpu.clone()),
            "launches": results.map(|r| launches_json(&r.launches)),
        })
    }
}

fn launches_json(launches: &[Launch]) -> Vec<Value> {
    launches
        .iter()
        .map(|l| {
            let mut value = json!({
                "label": l.label,
                "warmup": l.warmup,
                "spawn_to_cdp_ms": l.spawn_to_cdp_ms,
                "spawn_to_first_paint_ms": l.spawn_to_first_paint_ms,
                "spawn_to_workbench_ms": l.spawn_to_workbench_ms,
                "spawn_to_first_line_ms": l.spawn_to_first_line_ms,
                "spawn_to_first_frame_ms": l.spawn_to_first_frame_ms,
                "first_frame_after_main_ms": l.first_frame_after_main_ms,
                "shutdown_ms": l.shutdown_ms,
                "needed_sigkill": l.needed_sigkill,
                "peak_rss_mb": l.peak_rss_mb,
                "ready_rss_mb": l.ready_rss_mb,
                "ready_pss_mb": l.ready_pss_mb,
                "idle_rss_mb": l.idle_rss_mb,
                "idle_pss_mb": l.idle_pss_mb,
                "idle_procs": l.idle_procs,
                "outcome": l.outcome,
                "outcome_detail": l.outcome_detail,
                "complaint": l.complaint,
                "evict_attempts": l.evict_attempts,
                "cache_reference_resident_pct": l.cache_reference_resident_pct,
            });
            if let Some(map) = value.as_object_mut() {
                map.extend(match script_fields(l) {
                    Value::Object(extra) => extra,
                    _ => serde_json::Map::new(),
                });
            }
            value
        })
        .collect()
}

/// The fields only the `edit-undo` and `search` scenarios fill in.
fn script_fields(l: &Launch) -> Value {
    json!({
        "spawn_to_dom_mark_ms": l.spawn_to_dom_mark_ms,
        "spawn_to_first_swap_start_ms": l.spawn_to_first_swap_start_ms,
        "spawn_to_first_swap_ms": l.spawn_to_first_swap_ms,
        "first_swap_after_dom_ms": l.first_swap_after_dom_ms,
        "spawn_to_second_swap_ms": l.spawn_to_second_swap_ms,
        "trace_frames_drawn": l.trace_frames_drawn,
        "trace_error": l.trace_error,
                "spawn_to_folder_ms": l.spawn_to_folder_ms,
                "edit_start_chars": l.edit_start_chars,
                "edit_apply_ms": l.edit_apply_ms,
                "edit_apply_frame_ms": l.edit_apply_frame_ms,
                "edit_applied_chars": l.edit_applied_chars,
                "edit_applied_hash": l.edit_applied_hash,
                "edit_undo_ms": l.edit_undo_ms,
                "edit_undo_frame_ms": l.edit_undo_frame_ms,
                "edit_undo_steps": l.edit_undo_steps,
                "edit_undone_chars": l.edit_undone_chars,
                "edit_undone_hash": l.edit_undone_hash,
                "search_first_ms": l.search_first_ms,
                "search_complete_ms": l.search_complete_ms,
                "search_message_ms": l.search_message_ms,
                "search_files": l.search_files,
                "search_matches": l.search_matches,
                "search_limit_hit": l.search_limit_hit,
                "search_message": l.search_message,
                "search_state": l.search_state,
                "start_hwm_sum_mb": l.start_hwm_sum_mb,
                "start_hwm_max_mb": l.start_hwm_max_mb,
                "peak_hwm_sum_mb": l.peak_hwm_sum_mb,
                "peak_hwm_max_mb": l.peak_hwm_max_mb,
    })
}

/// Which launches did not exit within 10 s of SIGTERM, if any: their
/// shutdown times include that wait.
fn sigkill_note(results: &DriverResults) -> Option<String> {
    let killed: Vec<&str> = results
        .launches
        .iter()
        .filter(|l| l.needed_sigkill == Some(true))
        .map(|l| l.label.as_str())
        .collect();
    (!killed.is_empty()).then(|| {
        format!(
            "{} of {} launches did not exit within 10 s of SIGTERM and were killed with SIGKILL: {}; their shutdown times include the 10 s wait.",
            killed.len(),
            results.launches.len(),
            killed.join(", ")
        )
    })
}

fn row_json(row: &Row) -> Option<Value> {
    let st = Stats::of(&row.samples)?;
    Some(json!({
        "side": row.side,
        "metric": row.metric,
        "name": row.name,
        "unit": row.unit,
        "label": "measured",
        "n": st.n,
        "min": st.min,
        "p50": st.p50,
        "p95": st.p95,
        "max": st.max,
        "samples": row.samples,
        "meaning": row.meaning,
    }))
}

fn app_rows(plan: &Plan, launches: &[&Launch]) -> Vec<Row> {
    let pick = |f: fn(&Launch) -> Option<f64>| -> Vec<f64> {
        launches.iter().filter_map(|l| f(l)).map(round3).collect()
    };
    let launch = "from spawning the App (the QA launcher's `systemd-run --scope` memory cap, then the copy's codium with a fresh profile and the fixture on its command line) to";
    let poll = plan.scenario.app.poll_ms;
    let row = |metric, name, unit, samples, meaning: String| Row {
        side: "app",
        metric,
        name,
        unit,
        decimals: 0,
        samples,
        meaning,
    };
    vec![
        row("launch_to_cdp", "launch to CDP endpoint", "ms", pick(|l| l.spawn_to_cdp_ms),
            format!("{launch} its CDP endpoint answering `/json/version`, checked every 25 ms")),
        row("launch_to_first_paint", "launch to first paint", "ms", pick(|l| l.spawn_to_first_paint_ms),
            format!("{launch} Chromium's first contentful paint in the workbench page (`performance` paint timing, a wall-clock time read over CDP, minus the wall-clock time of the spawn): the first frame with content that Chromium painted, which can be before the editor is usable")),
        row("launch_to_workbench", "launch to workbench", "ms", pick(|l| l.spawn_to_workbench_ms),
            format!("{launch} the workbench's editor part existing (`.monaco-workbench .part.editor`), checked every 25 ms over CDP: the QA startup benchmark's \"workbench ready\"")),
        row("launch_to_first_line", "launch to first line shown", "ms", pick(|l| l.spawn_to_first_line_ms),
            format!("{launch} the fixture's first line rendered in an editor `.view-line`, checked every {poll} ms over CDP; includes Electron and workbench startup, reading the file and rendering it on the virtual display")),
        row("launch_to_first_swap", "launch to first swap", "ms", pick(|l| l.spawn_to_first_swap_ms),
            format!("{launch} the end of the first `Display::DrawAndSwap` that the display compositor began after the fixture's first line entered the DOM (a Chromium trace with categories viz and blink.user_timing, started right after the CDP connection; a MutationObserver in the page marks the moment the line is in the DOM; `process.hrtime` and the trace share CLOCK_MONOTONIC): the App's first presented frame that can show the file, the counterpart of rustode's first present (G22). It may be one frame early, if that frame was drawn from content committed before the line; the second swap bounds it from above")),
        row("launch_to_second_swap", "launch to second swap", "ms", pick(|l| l.spawn_to_second_swap_ms),
            "the end of the second `Display::DrawAndSwap` after the mark: the upper bound of 'the first frame that shows the first line'".to_owned()),
        row("swap_after_dom", "first swap after DOM mark", "ms", pick(|l| l.first_swap_after_dom_ms),
            "from the first line entering the DOM to the end of the first swap after it: what the page spends rendering and presenting the line".to_owned()),
        row("workbench_to_first_line", "workbench to first line", "ms",
            pick(|l| Some(l.spawn_to_first_line_ms? - l.spawn_to_workbench_ms?)),
            "the two previous times subtracted, per launch: what the App spends after its editor part exists until the first line shows".to_owned()),
        row("shutdown", "shutdown after SIGTERM", "ms", pick(|l| l.shutdown_ms),
            "SIGTERM to the App's main process until no process of its profile is left (SIGKILL after 10 s), as the QA startup benchmark does".to_owned()),
        row("peak_rss", "peak RSS, all processes", "MB", pick(|l| l.peak_rss_mb),
            "the largest total RSS of the App's processes in the QA watchdog's 1 s samples (collectors/sampler.py); the App is stopped about a second after its first line, so this is one to three samples: prefer the RSS at the first line".to_owned()),
        row("ready_rss", "RSS at first line", "MB", pick(|l| l.ready_rss_mb),
            "summed `Rss` of every process of the App's profile, read from /proc/<pid>/smaps_rollup right after the first line was seen (the App is still starting some processes then); shared pages are counted once per process".to_owned()),
        row("ready_pss", "PSS at first line", "MB", pick(|l| l.ready_pss_mb),
            "summed `Pss` of the same processes at the same moment".to_owned()),
        row("idle_rss", "idle RSS, all processes", "MB", pick(|l| l.idle_rss_mb),
            "summed `Rss` of every process of the App's profile, read from /proc/<pid>/smaps_rollup after the settle time: shared pages are counted once per process, so it over-counts".to_owned()),
        row("idle_pss", "idle PSS, all processes", "MB", pick(|l| l.idle_pss_mb),
            "summed `Pss` (proportional set size: shared pages split among their users) of the same processes, read at the same moment: the figure that adds up".to_owned()),
    ]
}

fn gui_rows(launches: &[&Launch]) -> Vec<Row> {
    let pick = |f: fn(&Launch) -> Option<f64>| -> Vec<f64> {
        launches.iter().filter_map(|l| f(l)).map(round3).collect()
    };
    let launch = "from spawning `rustode-gui` (release build, the fixture on its command line, a fresh profile folder, software rendering) to";
    let row = |metric, name, unit, samples, meaning: String| Row {
        side: "rustode",
        metric,
        name,
        unit,
        decimals: 0,
        samples,
        meaning,
    };
    vec![
        row("launch_to_first_frame", "launch to first frame", "ms", pick(|l| l.spawn_to_first_frame_ms),
            format!("{launch} the arrival of the line the GUI prints after returning from its first `RedrawRequested` (eframe has painted and called `wgpu::Queue::present`); the driver's clock, the same one the App's times use. The file is already open in that frame, because the GUI opens its arguments before it creates the window")),
        row("main_to_first_frame", "main to first frame", "ms", pick(|l| l.first_frame_after_main_ms),
            "the GUI's own `Instant`: from `rustode_trace::init_from_env` (the first thing `main` does) to the same first present; it leaves out exec, dynamic linking and the driver's pipe".to_owned()),
        row("shutdown", "shutdown after SIGTERM", "ms", pick(|l| l.shutdown_ms),
            "SIGTERM to the GUI process until no process of its tree is left (SIGKILL after 10 s)".to_owned()),
        row("peak_rss", "peak RSS, all processes", "MB", pick(|l| l.peak_rss_mb),
            "the largest total RSS of the GUI's process tree in the QA watchdog's 1 s samples (collectors/sampler.py); the GUI is stopped as soon as its first frame arrives, so this is usually one sample taken at the start: prefer the RSS at the first frame".to_owned()),
        row("ready_rss", "RSS at first frame", "MB", pick(|l| l.ready_rss_mb),
            "summed `Rss` of the GUI's process tree from /proc/<pid>/smaps_rollup right after the first-frame line arrived: the memory of the open file and the first frame".to_owned()),
        row("ready_pss", "PSS at first frame", "MB", pick(|l| l.ready_pss_mb),
            "summed `Pss` of the same processes at the same moment".to_owned()),
        row("idle_rss", "idle RSS, all processes", "MB", pick(|l| l.idle_rss_mb),
            "summed `Rss` of the GUI's process tree from /proc/<pid>/smaps_rollup after the settle time".to_owned()),
        row("idle_pss", "idle PSS, all processes", "MB", pick(|l| l.idle_pss_mb),
            "summed `Pss` of the same processes at the same moment".to_owned()),
    ]
}

/// What an equivalence check compares: its name, the value a launch reports
/// and the oracle's value, if there is one.
type Check<'a> = (&'a str, fn(&Launch) -> Option<String>, Option<String>);

/// One equivalence check: every recorded launch of each side reports the same
/// value as the other side and as the oracle (when there is one). `None` when
/// neither side reported the value.
fn compare_note(
    what: &str,
    gui: &[Launch],
    app: &[Launch],
    value: fn(&Launch) -> Option<String>,
    oracle: Option<String>,
) -> Option<(Option<&'static str>, String)> {
    let distinct = |launches: &[Launch]| -> Vec<String> {
        let mut values: Vec<String> = launches.iter().filter_map(value).collect();
        values.sort();
        values.dedup();
        values
    };
    let (g, a) = (distinct(gui), distinct(app));
    if g.is_empty() && a.is_empty() {
        return None;
    }
    let mut all = [g.clone(), a.clone()].concat();
    all.extend(oracle.clone());
    all.sort();
    all.dedup();
    let same = all.len() == 1 && !g.is_empty() && !a.is_empty();
    let shown = |v: &[String]| {
        if v.is_empty() {
            "not measured".to_owned()
        } else {
            v.join(" / ")
        }
    };
    Some((
        Some("measured"),
        format!(
            "{what}: rustode {}; App {}{}: {}",
            shown(&g),
            shown(&a),
            oracle.map_or_else(String::new, |o| format!("; oracle {o}")),
            if same {
                "equal"
            } else {
                "NOT EQUAL (or one side did not run): the two editors did not do the same work"
            }
        ),
    ))
}

/// The rows of the `edit-undo` and `search` scenarios, for either side.
fn script_rows(side: &'static str, launches: &[&Launch]) -> Vec<Row> {
    let app = side == "app";
    let pick = |f: fn(&Launch) -> Option<f64>| -> Vec<f64> {
        launches.iter().filter_map(|l| f(l)).map(round3).collect()
    };
    let count = |f: fn(&Launch) -> Option<u64>| -> Vec<f64> {
        launches
            .iter()
            .filter_map(|l| f(l))
            .map(|v| f64::from(u32::try_from(v).unwrap_or(u32::MAX)))
            .collect()
    };
    let row = |metric, name, unit, decimals, samples, meaning: String| Row {
        side,
        metric,
        name,
        unit,
        decimals,
        samples,
        meaning,
    };
    let (api, frame) = if app {
        (
            "the App's `TextModel` (`pushStackElement` and `pushEditOperations` for each insert, `getPositionAt` for its line and column, all in the timed loop; reached over CDP, no key sent)",
            "the second `requestAnimationFrame` callback after the step, on the page's `performance.now()` clock",
        )
    } else {
        (
            "`Workspace::replace` for each insert (a char offset, its own undo step), run by the GUI on itself after its first frame (`RUSTODE_BENCH_SCRIPT`, no key sent)",
            "the return from the `RedrawRequested` of the frame that follows the step, on `std::time::Instant`",
        )
    };
    let search = if app {
        "the command `workbench.action.findInFiles` with the query, executed on the App's command service over CDP; clock: the page's `performance.now()`, from just before the command"
    } else {
        "`Workspace::start_search`, started by the Search view's own code path, results streamed in the background and polled once per frame; clock: `std::time::Instant`, from the start of the search"
    };
    vec![
        row("edit_apply", "10k inserts, applying", "ms", 1, pick(|l| l.edit_apply_ms),
            format!("applying every seeded insert through {api}; the inserts alone")),
        row("edit_apply_frame", "10k inserts, to next frame", "ms", 1, pick(|l| l.edit_apply_frame_ms),
            format!("from before the first insert to the end of the next frame ({frame}); includes the inserts")),
        row("edit_undo", "undo of all, applying", "ms", 1, pick(|l| l.edit_undo_ms),
            "undoing every step until none is left (`Workspace::undo` / `TextModel.undo` in a loop); the undo alone".to_owned()),
        row("edit_undo_frame", "undo of all, to next frame", "ms", 1, pick(|l| l.edit_undo_frame_ms),
            format!("from before the first undo to the end of the next frame ({frame}); includes the undo")),
        row("edit_undo_steps", "undo steps", "n", 0, count(|l| l.edit_undo_steps),
            "how many undo steps there were (one per insert)".to_owned()),
        row("start_hwm", "peak RSS before the script", "MB", 1, pick(|l| l.start_hwm_sum_mb),
            "sum over the editor's processes of `VmHWM` (the kernel's peak resident size) read from /proc/<pid>/status right before the script: the startup and file-open peak; the sum over several processes is an upper bound of their simultaneous peak".to_owned()),
        row("peak_hwm", "peak RSS after the script", "MB", 1, pick(|l| l.peak_hwm_sum_mb),
            "the same sum of `VmHWM` read right after the script: the peak of the whole launch including the script; same caveat".to_owned()),
        row("peak_hwm_max", "peak RSS, largest process", "MB", 1, pick(|l| l.peak_hwm_max_mb),
            "the largest single process's `VmHWM` after the script (for the App, the renderer that holds the model)".to_owned()),
        row("search_first", "search, first result", "ms", 1, pick(|l| l.search_first_ms),
            format!("from the start of the search to the first result row drawn: {search}")),
        row("search_complete", "search, complete", "ms", 1, pick(|l| l.search_complete_ms),
            format!("from the start of the search to the end of the search with its final results shown (rustode: the presented frame after the state became Done; App: the sidebar progress bar active, then done, and the result message final): {search}")),
        row("search_files", "files with matches", "n", 0, count(|l| l.search_files),
            "files with at least one match in the results".to_owned()),
        row("search_matches", "matches", "n", 0, count(|l| l.search_matches),
            "matches in the results".to_owned()),
    ]
}

/// The side as people read it (the glossary's "App").
fn side_name(side: &str) -> &str {
    if side == "app" { "App" } else { side }
}

fn round3(value: f64) -> f64 {
    (value * 1e3).round() / 1e3
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustode_workspace::trace::LatencyRecorder;

    #[test]
    fn percentiles_match_the_trace_crate() {
        let millis: [u32; 10] = [16, 3, 9, 12, 1, 30, 5, 8, 4, 2];
        let samples = millis.map(f64::from);
        let st = Stats::of(&samples).expect("samples");
        let mut recorder = LatencyRecorder::new();
        for ms in millis {
            recorder.record(Duration::from_millis(u64::from(ms)));
        }
        let report = recorder.report().expect("report");
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        assert_eq!((st.n, st.min, st.max), (10, 1.0, 30.0));
        assert!((st.p50 - ms(report.p50)).abs() < 1e-9);
        assert!((st.p95 - ms(report.p95)).abs() < 1e-9);
        assert_eq!(Stats::of(&[]), None);
    }
}
