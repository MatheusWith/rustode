//! `rustode bench`: runs one scenario of the versioned scenario list on
//! rustode and on the App, and writes a reproducible, labeled report (spec,
//! stories 60 and 104-112).
//!
//! A run takes the bench lock, checks the App (integrity against the
//! manifest, no file outside it, one App at a time, enough memory), creates
//! its run folder and fixture, measures rustode in-process, drives a
//! verified copy of the App through the QA harness on the virtual display,
//! and writes `report.txt` and `report.json`. Refusals stop before anything
//! is written. A run that fails after its folder exists is still reported,
//! and the folder is renamed `<run>-INVALIDA` or `<run>-INTERROMPIDA` so it
//! is never read as product evidence (AGENTS.md §0.5).

mod app_side;
mod args;
mod display;
mod fixture;
mod guard;
mod integrity;
mod plan;
mod report;
mod rustode_side;
mod scenario;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

use args::Mode;
use plan::Plan;
use report::{Environment, Report};
use scenario::List;

/// Why `rustode bench` stopped before, or instead of, a run.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Bad arguments or configuration (exit code 2).
    Usage(String),
    /// The App does not match its manifest (exit code 3).
    Integrity(String),
    /// Another App or bench is running, or memory is short (exit code 4).
    Refused(String),
    /// Anything else (exit code 1).
    Run(String),
}

impl Failure {
    fn code(&self) -> u8 {
        match self {
            Self::Run(_) => 1,
            Self::Usage(_) => 2,
            Self::Integrity(_) => 3,
            Self::Refused(_) => 4,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Integrity(message) | Self::Run(message) => {
                f.write_str(message)
            }
            Self::Refused(message) => write!(f, "refused: {message}"),
        }
    }
}

/// How a run that got its folder went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Every step finished.
    Ok,
    /// A step failed (the harness, the driver, the copy).
    Failed,
    /// The driver refused a later launch (memory) or the session.
    Refused,
    /// The memory watchdog killed the App.
    Aborted,
    /// The App changed during the run, or its copy did not match.
    Invalid,
}

impl Status {
    /// The name used in reports.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::Aborted => "aborted",
            Self::Invalid => "invalid",
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Failed => 1,
            Self::Invalid => 3,
            Self::Refused => 4,
            Self::Aborted => 5,
        }
    }

    /// The run-folder suffix that marks a run as not product evidence.
    fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Failed | Self::Invalid => Some("INVALIDA"),
            Self::Refused | Self::Aborted => Some("INTERROMPIDA"),
        }
    }
}

/// Runs `rustode bench` with the arguments that follow `bench`.
pub fn run(args: &[String]) -> ExitCode {
    match dispatch(args) {
        Ok(code) => ExitCode::from(code),
        Err(failure) => {
            eprintln!("error: {failure}");
            ExitCode::from(failure.code())
        }
    }
}

fn dispatch(args: &[String]) -> Result<u8, Failure> {
    let options = args::parse(args)?;
    if options.mode == Mode::Help {
        print!("{}", args::USAGE);
        return Ok(0);
    }
    let list = List::load(&options.scenarios)?;
    if options.mode == Mode::List {
        print!("{}", list.describe());
        return Ok(0);
    }
    let scenario = list.get(options.scenario.as_deref().unwrap_or_default())?;
    let plan = Plan::new(&options, &list, scenario)?;
    if options.mode == Mode::DryRun {
        print!("{}", plan.describe());
        return Ok(0);
    }
    execute(&plan)
}

fn execute(plan: &Plan) -> Result<u8, Failure> {
    let _lock = guard::lock(&plan.lock)?;
    let preflight = if plan.runs_app() {
        Some(app_side::preflight(plan)?)
    } else {
        None
    };
    let environment = Environment::read(plan);
    create_run_folder(plan)?;
    fixture::write(&plan.fixture(), plan.scenario.fixture)
        .map_err(|err| Failure::Run(format!("cannot write the fixture: {err}")))?;
    let fixture_sha256 = sha256(&plan.fixture());
    let mut status = Status::Ok;
    let mut reasons = Vec::new();
    let rustode = if plan.runs_rustode() {
        eprintln!(
            "bench: rustode: {} + {} opens of the fixture",
            plan.scenario.rustode.warmup, plan.scenario.rustode.repetitions
        );
        rustode_side::open_file(
            &plan.fixture(),
            plan.scenario.fixture.bytes,
            plan.scenario.rustode,
        )
        .map_err(|err| {
            status = Status::Failed;
            reasons.push(err);
        })
        .ok()
    } else {
        None
    };
    let app = preflight.map(|preflight| app_side::run(plan, preflight));
    if let Some(app) = &app {
        if app.status != Status::Ok && (status == Status::Ok || app.status == Status::Invalid) {
            status = app.status;
        }
        reasons.extend(app.reasons.iter().cloned());
        let writes = plan.app_out().join("copy-writes.txt");
        let _ = fs::create_dir_all(plan.app_out())
            .and_then(|()| fs::write(&writes, app.copy_writes.join("\n") + "\n"));
    }
    let (run_id, run_folder) = finish_folder(plan, status, &mut reasons);
    let report = Report {
        plan,
        run_id,
        run_folder,
        finished: SystemTime::now(),
        status,
        reasons,
        environment,
        fixture_sha256,
        rustode,
        app,
    };
    write_report(&report)?;
    Ok(status.code())
}

fn create_run_folder(plan: &Plan) -> Result<(), Failure> {
    let folder = &plan.run_folder;
    if let Some(parent) = folder.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Failure::Run(format!("cannot create {}: {err}", display::path(parent)))
        })?;
    }
    fs::create_dir(folder)
        .and_then(|()| fs::create_dir(folder.join("fixtures")))
        .map_err(|err| {
            Failure::Run(format!(
                "cannot create the run folder {}: {err}",
                display::path(folder)
            ))
        })
}

/// Renames the run folder of a failed run; returns the final id and folder.
fn finish_folder(plan: &Plan, status: Status, reasons: &mut Vec<String>) -> (String, PathBuf) {
    let Some(suffix) = status.suffix() else {
        return (plan.run_id.clone(), plan.run_folder.clone());
    };
    let run_id = format!("{}-{suffix}", plan.run_id);
    let renamed = plan.run_folder.with_file_name(&run_id);
    match fs::rename(&plan.run_folder, &renamed) {
        Ok(()) => (run_id, renamed),
        Err(err) => {
            reasons.push(format!("cannot rename the run folder to {run_id}: {err}"));
            (plan.run_id.clone(), plan.run_folder.clone())
        }
    }
}

fn write_report(report: &Report) -> Result<(), Failure> {
    let text = report.text();
    let json = serde_json::to_string_pretty(&report.json())
        .map_err(|err| Failure::Run(format!("cannot render the report: {err}")))?;
    let folder = &report.run_folder;
    fs::write(folder.join("report.txt"), &text)
        .and_then(|()| fs::write(folder.join("report.json"), json + "\n"))
        .map_err(|err| Failure::Run(format!("cannot write the report: {err}")))?;
    print!("{text}");
    eprintln!(
        "bench: report written to {}",
        display::path(&folder.join("report.txt"))
    );
    if report.status != Status::Ok {
        eprintln!(
            "error: the run is {} ({}):",
            report.status.as_str(),
            report.run_id
        );
        for reason in &report.reasons {
            eprintln!("  {}", reason.replace('\n', "\n  "));
        }
    }
    Ok(())
}

/// The SHA-256 of a file, from `sha256sum`.
fn sha256(path: &Path) -> Option<String> {
    let out = Command::new("sha256sum").arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    out.status
        .success()
        .then(|| text.split_whitespace().next().map(str::to_owned))
        .flatten()
}
