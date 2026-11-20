//! The App side of a scenario: preflight, a verified copy, the launches on
//! the QA harness's virtual display without network, cleanup, and the
//! integrity check after the run.
//!
//! The App never runs from `01-app/`: running it lets Node write into its
//! install folder (the 2026-11-17 incident), so it runs from a copy that is
//! checked against the manifest first and deleted afterwards.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use super::guard::MIN_AVAILABLE_MB;
use super::integrity::{self, Integrity};
use super::plan::{APP_FOLDER, CDP_PORT, Plan};
use super::scenario::Kind;
use super::{Failure, Status, display, guard};

/// Kills what the reaper finds once the run should long be over, if this
/// process died without cleaning up: the App's processes, then the copy.
const REAPER_SCRIPT: &str = r#"sleep "$1"; pkill -TERM -f -- "[-]-user-data-dir=$2/"; sleep 3; pkill -KILL -f -- "[-]-user-data-dir=$2/"; rm -rf -- "$3""#;

/// What the preflight found.
#[derive(Debug)]
pub(crate) struct Preflight {
    /// The integrity check of `01-app/` before the run.
    pub before: Integrity,
}

/// What the driver reported (`results.json`).
#[derive(Debug, Deserialize)]
pub(crate) struct DriverResults {
    /// `ok`, `error`, `refused` or `aborted`.
    pub status: String,
    /// Why, when not `ok`.
    #[serde(default)]
    pub reason: Option<String>,
    /// The driver's Node version.
    #[serde(default)]
    pub node: Option<String>,
    /// The `playwright-core` version.
    #[serde(default)]
    pub playwright_core: Option<String>,
    /// Which GPU path the App's Chromium chose (App only).
    #[serde(default)]
    pub gpu: Option<serde_json::Value>,
    /// One entry per launch, warm-ups first.
    #[serde(default)]
    pub launches: Vec<Launch>,
}

/// One launch of the App, times in ms from its spawn.
#[derive(Debug, Deserialize)]
pub(crate) struct Launch {
    /// `warmup-N` or `rep-N`.
    pub label: String,
    /// Not recorded in the results.
    #[serde(default)]
    pub warmup: bool,
    /// CDP endpoint answering.
    pub spawn_to_cdp_ms: Option<f64>,
    /// The workbench's editor part in the DOM.
    pub spawn_to_workbench_ms: Option<f64>,
    /// The fixture's first line rendered.
    pub spawn_to_first_line_ms: Option<f64>,
    /// SIGTERM until no App process is left.
    pub shutdown_ms: Option<f64>,
    /// The App did not exit within 10 s of SIGTERM.
    pub needed_sigkill: Option<bool>,
    /// Peak total RSS in the watchdog's samples.
    pub peak_rss_mb: Option<f64>,
    /// App: Chromium's first contentful paint.
    #[serde(default)]
    pub spawn_to_first_paint_ms: Option<f64>,
    /// GUI: the driver's clock when the GUI's first-frame line arrived.
    #[serde(default)]
    pub spawn_to_first_frame_ms: Option<f64>,
    /// GUI: the GUI's own clock, `main` to its first presented frame.
    #[serde(default)]
    pub first_frame_after_main_ms: Option<f64>,
    /// Summed RSS of the editor's processes right after the end point.
    #[serde(default)]
    pub ready_rss_mb: Option<f64>,
    /// Summed PSS of the same processes at the same moment.
    #[serde(default)]
    pub ready_pss_mb: Option<f64>,
    /// Idle: summed RSS of the editor's processes after settling.
    #[serde(default)]
    pub idle_rss_mb: Option<f64>,
    /// Idle: summed PSS of the editor's processes after settling.
    #[serde(default)]
    pub idle_pss_mb: Option<f64>,
    /// Idle: how many processes were summed.
    #[serde(default)]
    pub idle_procs: Option<u32>,
    /// `timeout` or `exited-first`: the launch never reached its end point.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Why, in the driver's words.
    #[serde(default)]
    pub outcome_detail: Option<String>,
    /// App: what the workbench said when it could not show the file.
    #[serde(default)]
    pub complaint: Option<String>,
    /// Cold launches: how many eviction attempts it took.
    #[serde(default)]
    pub evict_attempts: Option<u32>,
    /// Cold launches: how much of the editor's main file the page cache still
    /// held after the eviction, in percent.
    #[serde(default)]
    pub cache_reference_resident_pct: Option<f64>,
}

/// How the App side went.
#[derive(Debug)]
pub(crate) struct Outcome {
    /// The worst thing that happened.
    pub status: Status,
    /// Why, when not ok.
    pub reasons: Vec<String>,
    /// Integrity of `01-app/` before, of the copy, and of `01-app/` after.
    pub before: Integrity,
    /// The copy's check.
    pub copy: Option<Integrity>,
    /// `01-app/` after the run.
    pub after: Option<Integrity>,
    /// Where the copy was.
    pub copy_dir: Option<PathBuf>,
    /// Files the App added to its copy during the run.
    pub copy_writes: Vec<String>,
    /// Whether a reaper unit guarded the run.
    pub reaper: bool,
    /// Whether App processes were still running after the driver finished.
    pub leftovers_killed: bool,
    /// The driver's results.
    pub results: Option<DriverResults>,
}

impl Outcome {
    pub(super) fn fail(&mut self, status: Status, reason: String) {
        if self.status == Status::Ok || status == Status::Invalid {
            self.status = status;
        }
        self.reasons.push(reason);
    }
}

/// Checks everything that must hold before the App may start.
pub(crate) fn preflight(plan: &Plan) -> Result<Preflight, Failure> {
    check_tools(plan)?;
    let app_dir = plan.app_dir();
    eprintln!(
        "bench: checking {} against the manifest",
        display::path(&app_dir)
    );
    let before = integrity::check(&app_dir, &plan.manifest()).map_err(Failure::Integrity)?;
    if !before.ok() {
        return Err(Failure::Integrity(format!(
            "App integrity check failed for {}:\n{}",
            display::path(&app_dir),
            before.problems()
        )));
    }
    guard::no_app_running(&plan.proc_dir)?;
    guard::enough_memory(&plan.proc_dir)?;
    guard::copy_fits(&plan.copy_parent, before.bytes)?;
    Ok(Preflight { before })
}

fn check_tools(plan: &Plan) -> Result<(), Failure> {
    let needed = [
        ("the App", plan.app_dir()),
        ("the manifest", plan.manifest()),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.run_virtual()),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.sampler()),
        ("the QA harness (RUSTODE_QA_ROOT)", plan.qa_lib()),
        ("playwright-core of the QA harness", plan.playwright_core()),
        ("Node (RUSTODE_NODE)", plan.node.clone()),
        ("the driver", plan.driver()),
        ("the settings", plan.settings()),
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
            "cannot run the App side, missing:\n  {}",
            missing.join("\n  ")
        )))
    }
}

/// Runs the App side; never returns early, so the report always gets an outcome.
pub(crate) fn run(plan: &Plan, preflight: Preflight) -> Outcome {
    let mut outcome = Outcome {
        status: Status::Ok,
        reasons: Vec::new(),
        before: preflight.before,
        copy: None,
        after: None,
        copy_dir: None,
        copy_writes: Vec::new(),
        reaper: false,
        leftovers_killed: false,
        results: None,
    };
    match Copy::create(plan) {
        Ok(copy) => {
            outcome.copy_dir = Some(copy.app_dir());
            outcome.reaper = copy.reaper.is_some();
            in_copy(plan, &copy, &mut outcome);
            outcome.leftovers_killed = stop_launches(&copy.profiles());
            match integrity::unlisted(&copy.app_dir(), &plan.manifest()) {
                Ok(writes) => outcome.copy_writes = writes,
                Err(err) => outcome.reasons.push(err),
            }
            if let Err(err) = copy.remove() {
                outcome.fail(Status::Failed, err);
            }
        }
        Err(err) => outcome.fail(Status::Failed, err),
    }
    eprintln!("bench: checking {} again", display::path(&plan.app_dir()));
    match integrity::check(&plan.app_dir(), &plan.manifest()) {
        Ok(after) => {
            if !after.ok() {
                outcome.fail(
                    Status::Invalid,
                    format!("01-app changed during the run:\n{}", after.problems()),
                );
            }
            outcome.after = Some(after);
        }
        Err(err) => outcome.fail(Status::Invalid, err),
    }
    outcome
}

fn in_copy(plan: &Plan, copy: &Copy, outcome: &mut Outcome) {
    eprintln!("bench: checking the copy");
    match integrity::check(&copy.app_dir(), &plan.manifest()) {
        Ok(check) => {
            let ok = check.ok();
            if !ok {
                outcome.fail(
                    Status::Invalid,
                    format!("the copy differs from the manifest:\n{}", check.problems()),
                );
            }
            outcome.copy = Some(check);
            if !ok {
                return;
            }
        }
        Err(err) => return outcome.fail(Status::Failed, err),
    }
    if let Err(err) = write_config(plan, copy) {
        return outcome.fail(Status::Failed, err);
    }
    match launch(plan, &copy.profiles()) {
        Ok((exit, results)) => {
            let taken = take_results(&plan.app_out(), exit, results);
            if let Some(reason) = taken.reason {
                outcome.fail(taken.status, reason);
            }
            outcome.results = taken.results;
        }
        Err(err) => outcome.fail(Status::Failed, err),
    }
}

fn write_config(plan: &Plan, copy: &Copy) -> Result<(), String> {
    let out = plan.app_out();
    fs::create_dir_all(&out).map_err(|err| format!("cannot create {}: {err}", out.display()))?;
    let app = &plan.scenario.app;
    let codium = copy.app_dir().join("codium");
    let mut config = driver_config(plan, "app", &out, &copy.profiles());
    config["settings"] = json!(plan.settings().to_string_lossy());
    config["warmup"] = json!(app.warmup);
    config["repetitions"] = json!(app.repetitions);
    config["timeout_ms"] = json!(u64::from(app.timeout_s) * 1_000);
    config["poll_ms"] = json!(app.poll_ms);
    config["app_command"] = json!(plan.app_command(&codium));
    config["sampler_command"] = json!(plan.sampler_command());
    config["env"] = json!(
        plan.software_rendering_env()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    config["evict_paths"] = json!([copy.app_dir().to_string_lossy()]);
    config["cache_reference"] = json!(codium.to_string_lossy());
    let text = serde_json::to_string_pretty(&config).map_err(|err| err.to_string())?;
    fs::write(plan.driver_config(), text + "\n")
        .and_then(|()| fs::copy(plan.settings(), out.join("settings.json")).map(drop))
        .map_err(|err| format!("cannot write the driver config: {err}"))
}

/// The driver configuration the two targets share.
pub(super) fn driver_config(
    plan: &Plan,
    target: &str,
    out: &Path,
    profiles: &Path,
) -> serde_json::Value {
    let s = |path: &Path| path.to_string_lossy().into_owned();
    let launch = plan.scenario.launch;
    let measure = launch.map_or("open", |l| l.measure.as_str());
    json!({
        "schema": "rustode-bench-app-driver/2",
        "target": target,
        "measure": measure,
        "cache": plan.cache().as_str(),
        "settle_s": launch.map_or(0, |l| l.settle_s),
        "timeout_is_result": plan.scenario.kind == Kind::OpenFile,
        "qa_root": s(&plan.qa_root),
        "playwright_core": s(&plan.playwright_core()),
        "out_dir": s(out),
        "profiles_root": s(profiles),
        "fixture": s(&plan.fixture()),
        "first_line": plan.first_line(),
        "cdp_port": CDP_PORT,
        "min_available_mb": MIN_AVAILABLE_MB,
        "evict_script": s(&plan.evict_script()),
    })
}

fn launch(plan: &Plan, profiles: &Path) -> Result<(ExitStatus, Option<DriverResults>), String> {
    run_driver(
        plan,
        profiles,
        &plan.app_out(),
        &plan.driver_config(),
        ("the App", plan.app_deadline()),
    )
}

/// Runs the driver with `config` on the virtual display; reads `out/results.json`.
pub(super) fn run_driver(
    plan: &Plan,
    profiles: &Path,
    out: &Path,
    config: &Path,
    (what, deadline): (&str, Duration),
) -> Result<(ExitStatus, Option<DriverResults>), String> {
    let log_path = out.join("driver.log");
    let log =
        File::create(&log_path).map_err(|err| format!("cannot create the driver log: {err}"))?;
    let err_log = log.try_clone().map_err(|err| err.to_string())?;
    let command = plan.launch_command(config);
    eprintln!(
        "bench: launching {what} on the virtual display (log: {})",
        display::path(&log_path)
    );
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err_log)
        .spawn()
        .map_err(|err| {
            format!(
                "cannot run {}: {err}",
                display::path(Path::new(&command[0]))
            )
        })?;
    let exit = wait(&mut child, (profiles, config), deadline)?;
    let results_path = out.join("results.json");
    let results = match fs::read_to_string(&results_path) {
        Ok(text) => Some(
            serde_json::from_str(&text)
                .map_err(|err| format!("{}: {err}", display::path(&results_path)))?,
        ),
        Err(_) => None,
    };
    Ok((exit, results))
}

/// Waits for the driver; past the deadline, stops the App and the driver.
fn wait(
    child: &mut Child,
    (profiles, config): (&Path, &Path),
    allowed: Duration,
) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + allowed;
    loop {
        if let Some(exit) = child.try_wait().map_err(|err| err.to_string())? {
            return Ok(exit);
        }
        if Instant::now() >= deadline {
            stop_launches(profiles);
            pkill("-TERM", &ere_escape(&config.to_string_lossy()));
            for _ in 0..50 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "the driver took longer than {} s and was stopped",
                allowed.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// What the driver's exit and `results.json` add up to.
pub(super) struct Taken {
    /// The status the results imply.
    pub status: Status,
    /// Why, when not ok.
    pub reason: Option<String>,
    /// The parsed results, if the driver wrote them.
    pub results: Option<DriverResults>,
}

/// Reads what a driver run left in `out`: its status, and why if it is not ok.
pub(super) fn take_results(out: &Path, exit: ExitStatus, results: Option<DriverResults>) -> Taken {
    let Some(results) = results else {
        let log = out.join("driver.log");
        let tail = fs::read_to_string(&log).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Taken {
            status: Status::Failed,
            reason: Some(format!(
                "the driver wrote no results ({exit}; run_virtual.sh exits 99 when the virtual display did not start); last lines of {}:\n  {}",
                display::path(&log),
                tail.join("\n  ")
            )),
            results: None,
        };
    };
    // "ok" with a failing exit status is a failure too.
    let status = match results.status.as_str() {
        "ok" if exit.success() => Status::Ok,
        "aborted" => Status::Aborted,
        "refused" => Status::Refused,
        _ => Status::Failed,
    };
    let reason = (status != Status::Ok).then(|| {
        results
            .reason
            .clone()
            .unwrap_or_else(|| format!("the driver exited with {exit}"))
    });
    Taken {
        status,
        reason,
        results: Some(results),
    }
}

/// Stops any App process of these profiles; returns whether one was running.
pub(super) fn stop_launches(profiles: &Path) -> bool {
    let pattern = format!(
        "[-]-user-data-dir={}/",
        ere_escape(&profiles.to_string_lossy())
    );
    let found = pkill("-TERM", &pattern);
    if found {
        std::thread::sleep(Duration::from_secs(3));
        pkill("-KILL", &pattern);
    }
    found
}

/// `pkill <signal> -f -- <pattern>`; whether a process matched.
pub(super) fn pkill(signal: &str, pattern: &str) -> bool {
    Command::new("pkill")
        .args([signal, "-f", "--", pattern])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `s` as a POSIX extended regular expression that matches only itself.
pub(super) fn ere_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".[]()*+?{}|^$\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A verified copy of the App and its profiles, deleted when dropped.
struct Copy {
    root: PathBuf,
    reaper: Option<String>,
}

impl Copy {
    fn create(plan: &Plan) -> Result<Self, String> {
        fs::create_dir_all(&plan.copy_parent)
            .map_err(|err| format!("cannot create {}: {err}", display::path(&plan.copy_parent)))?;
        let root = temp_dir(&plan.copy_parent, "rustode-bench-app.")
            .map_err(|err| format!("cannot create the copy folder: {err}"))?;
        let reaper = start_reaper(plan, &root);
        let copy = Self { root, reaper };
        eprintln!(
            "bench: copying the App to {}",
            display::path(&copy.app_dir())
        );
        let copied = Command::new("cp")
            .arg("-a")
            .arg(plan.app_dir())
            .arg(copy.app_dir())
            .status()
            .map_err(|err| format!("cannot run cp: {err}"))?;
        if !copied.success() {
            return Err(format!("cp -a of the App failed ({copied})"));
        }
        fs::create_dir(copy.profiles())
            .map_err(|err| format!("cannot create the profiles folder: {err}"))?;
        Ok(copy)
    }

    fn app_dir(&self) -> PathBuf {
        self.root.join(APP_FOLDER)
    }

    fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }

    fn remove(mut self) -> Result<(), String> {
        let removed = fs::remove_dir_all(&self.root);
        let root = std::mem::take(&mut self.root);
        if let Some(unit) = self.reaper.take() {
            if removed.is_ok() {
                stop_reaper(&unit);
            } else {
                eprintln!(
                    "bench: the reaper {unit} will delete {} later",
                    root.display()
                );
            }
        }
        removed.map_err(|err| format!("cannot delete the copy {}: {err}", display::path(&root)))
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        if self.root.as_os_str().is_empty() {
            return;
        }
        if fs::remove_dir_all(&self.root).is_ok()
            && let Some(unit) = &self.reaper
        {
            stop_reaper(unit);
        }
    }
}

/// A transient systemd unit that cleans up if this process dies mid-run, as
/// the QA harness's `qa-reaper-*` units do. Returns its name, or `None` if
/// it could not be started (the run goes on without it).
fn start_reaper(plan: &Plan, root: &Path) -> Option<String> {
    let suffix = root
        .file_name()?
        .to_string_lossy()
        .replace("rustode-bench-app.", "");
    let unit = format!("rustode-bench-reaper-{suffix}");
    let secs = (plan.app_deadline() + Duration::from_secs(300))
        .as_secs()
        .to_string();
    let profiles = ere_escape(&root.join("profiles").to_string_lossy());
    let started = Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect"])
        .arg(format!("--unit={unit}"))
        .args(["--", "bash", "-c", REAPER_SCRIPT, "rustode-bench-reaper"])
        .args([secs.as_str(), profiles.as_str()])
        .arg(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !started {
        eprintln!(
            "bench: warning: no reaper unit (systemd-run failed); a crash would leave the copy behind"
        );
    }
    started.then_some(unit)
}

fn stop_reaper(unit: &str) {
    let _ = Command::new("systemctl")
        .args(["--user", "stop", unit])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A new folder `<parent>/<prefix><random>`, like `mktemp -d`.
fn temp_dir(parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    use std::hash::{BuildHasher, RandomState};
    for attempt in 0..16_u32 {
        // Each RandomState has fresh random keys, so the hash is random.
        let random = RandomState::new().hash_one((std::process::id(), attempt));
        let path = parent.join(format!("{prefix}{random:016x}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("no free folder name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ere_escape_quotes_regex_characters() {
        assert_eq!(ere_escape("/var/tmp/a.b+c"), r"/var/tmp/a\.b\+c");
        assert_eq!(ere_escape("plain-path_1"), "plain-path_1");
    }
}
