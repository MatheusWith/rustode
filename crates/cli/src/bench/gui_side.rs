//! The rustode GUI side of a scenario: launches of `rustode-gui` with the
//! fixture on the QA harness's virtual display, without network, driven by
//! the same driver as the App (`bench/launch.cjs`, target `gui`).
//!
//! The GUI prints one line on stderr after its first presented frame
//! (`rustode_trace::FirstFrame`); the driver stamps its arrival with the
//! clock it also uses for the App's end points.

use std::fs;
use std::path::Path;
use std::process::Stdio;

use serde_json::json;

use super::app_side::{self, DriverResults};
use super::plan::Plan;
use super::{Failure, Status, display};

/// How the GUI side went.
#[derive(Debug)]
pub(crate) struct Outcome {
    /// The worst thing that happened.
    pub status: Status,
    /// Why, when not ok.
    pub reasons: Vec<String>,
    /// SHA-256 of the GUI binary that ran.
    pub binary_sha256: Option<String>,
    /// `rustode-gui --version`.
    pub version: Option<String>,
    /// Whether GUI processes were still running after the driver finished.
    pub leftovers_killed: bool,
    /// The driver's results.
    pub results: Option<DriverResults>,
}

/// Checks that the GUI side can run; returns before anything starts.
pub(crate) fn preflight(plan: &Plan) -> Result<(), Failure> {
    let needed = [
        (
            "the rustode GUI (--gui-bin; cargo build --release -p rustode-gui)",
            plan.gui_bin.clone(),
        ),
        (
            "Mesa's EGL vendor file for software rendering (RUSTODE_BENCH_MESA_EGL)",
            plan.mesa_egl.clone(),
        ),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.run_virtual()),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.sampler()),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.qa_lib()),
        ("Node (RUSTODE_NODE)", plan.node.clone()),
        ("the driver", plan.driver()),
        ("the cache eviction helper", plan.evict_script()),
    ];
    let missing: Vec<String> = needed
        .iter()
        .filter(|(_, path)| !path.exists())
        .map(|(what, path)| format!("{what}: {}", display::path(path)))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Failure::Usage(format!(
            "cannot run the GUI side, missing:\n  {}",
            missing.join("\n  ")
        )))
    }
}

/// Runs the GUI side; never returns early, so the report always gets an outcome.
pub(crate) fn run(plan: &Plan) -> Outcome {
    let mut outcome = Outcome {
        status: Status::Ok,
        reasons: Vec::new(),
        binary_sha256: super::sha256(&plan.gui_bin),
        version: gui_version(&plan.gui_bin),
        leftovers_killed: false,
        results: None,
    };
    if plan.scenario.gui.is_none() {
        return outcome;
    }
    let out = plan.gui_out();
    let profiles = out.join("profiles");
    if let Err(err) =
        fs::create_dir_all(&profiles).and_then(|()| write_config(plan, &out, &profiles))
    {
        outcome.status = Status::Failed;
        outcome
            .reasons
            .push(format!("cannot write the GUI driver config: {err}"));
        return outcome;
    }
    let deadline = plan.gui_deadline();
    match app_side::run_driver(
        plan,
        &profiles,
        &out,
        &plan.gui_driver_config(),
        ("rustode-gui", deadline),
    ) {
        Ok((exit, results)) => {
            let taken = app_side::take_results(&out, exit, results);
            outcome.status = taken.status;
            outcome.reasons.extend(taken.reason);
            outcome.results = taken.results;
        }
        Err(err) => {
            outcome.status = Status::Failed;
            outcome.reasons.push(err);
        }
    }
    // Anchored at the start of the command line, so it never matches this
    // process (which has the path in `--gui-bin`) or the driver.
    let pattern = format!(
        "^{}( |$)",
        app_side::ere_escape(&plan.gui_bin.to_string_lossy())
    );
    outcome.leftovers_killed = app_side::pkill("-TERM", &pattern);
    if outcome.leftovers_killed {
        std::thread::sleep(std::time::Duration::from_secs(3));
        app_side::pkill("-KILL", &pattern);
    }
    outcome
}

fn write_config(plan: &Plan, out: &Path, profiles: &Path) -> std::io::Result<()> {
    let gui = plan.scenario.gui;
    let mut config = app_side::driver_config(plan, "gui", out, profiles);
    config["warmup"] = json!(gui.map_or(0, |g| g.warmup));
    config["repetitions"] = json!(gui.map_or(0, |g| g.repetitions));
    config["timeout_ms"] = json!(gui.map_or(0, |g| u64::from(g.timeout_s)) * 1_000);
    config["poll_ms"] = json!(5);
    config["app_command"] = json!(plan.gui_command());
    config["sampler_command"] = json!(plan.gui_sampler_command());
    config["env"] = json!(
        plan.gui_env()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    config["evict_paths"] = json!([plan.gui_bin.to_string_lossy()]);
    config["cache_reference"] = json!(plan.gui_bin.to_string_lossy());
    let text = serde_json::to_string_pretty(&config).map_err(std::io::Error::other)?;
    fs::write(plan.gui_driver_config(), text + "\n")
}

fn gui_version(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
