//! `rustode bench` at the CLI seam (spec, "Testing Decisions", seam 3): run
//! the binary, assert on its output, exit codes and run folder.
//!
//! No test launches the App or a virtual display. Tests that reach the App
//! phase run in a sandbox with a fake App (a few small files and their
//! manifest), a fake QA harness whose `run_virtual.sh` records how it was
//! called and writes the results a driver would, a fake `/proc`, and fake
//! `systemd-run` and `systemctl` first on `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const RUSTODE: &str = env!("CARGO_BIN_EXE_rustode");

/// A small scenario list, so the tests run fast.
const TINY_LIST: &str = r#"
version = 7

[[scenario]]
id = "open-file-tiny"
title = "Open a tiny file"
kind = "open-file"

[scenario.fixture]
bytes = 4100
line_bytes = 64

[scenario.rustode]
warmup = 1
repetitions = 5

[scenario.app]
warmup = 1
repetitions = 3
timeout_s = 5
poll_ms = 10
driver = "launch.cjs"
settings = "app/settings.json"
"#;

/// Stands in for the QA harness's `run_virtual.sh`: records its arguments,
/// then writes the `results.json` the App driver would write.
const FAKE_RUN_VIRTUAL: &str = r#"#!/usr/bin/env python3
import json, os, sys
calls = os.environ["FAKE_CALLS"]
config = json.load(open(sys.argv[-1]))
target = config.get("target", "app")
codium = ([a for a in config["app_command"] if a.endswith("/codium")] or [config["app_command"][0]])[0]
name = "run_virtual.json" if target == "app" else "run_virtual-%s.json" % target
with open(os.path.join(calls, name), "w") as f:
    json.dump({"argv": sys.argv[1:], "codium_exists": os.path.isfile(codium), "config": config}, f)
if os.environ.get("FAKE_WRITE_INTO"):
    with open(os.environ["FAKE_WRITE_INTO"], "w") as f:
        f.write("written by the App")
def launch(label, warmup, i):
    return {"label": label, "warmup": warmup, "spawn_to_cdp_ms": 200.0 + i,
            "spawn_to_workbench_ms": 1800.0 + 10 * i, "spawn_to_first_line_ms": 2000.0 + 100 * i,
            "spawn_to_first_paint_ms": 900.0 + i, "ready_rss_mb": 1100.5 + i, "ready_pss_mb": 700.5 + i, "idle_rss_mb": 1200.5 + i, "idle_pss_mb": 800.5 + i,
            "shutdown_ms": 300.0, "peak_rss_mb": 900 + i,
            "needed_sigkill": label in os.environ.get("FAKE_SIGKILL", "").split(",")}
def gui_launch(label, warmup, i):
    return {"label": label, "warmup": warmup, "spawn_to_first_frame_ms": 300.0 + i,
            "first_frame_after_main_ms": 250.0 + i, "ready_rss_mb": 120.5 + i, "ready_pss_mb": 100.5 + i, "idle_rss_mb": 150.5 + i, "idle_pss_mb": 140.5 + i,
            "shutdown_ms": 20.0, "peak_rss_mb": 160 + i, "needed_sigkill": False,
            "evict_attempts": 5 if i == 2 else 1, "cache_reference_resident_pct": 28.4 if i == 2 else 0.0}
def script_fields(launch_fn):
    def wrapped(label, warmup, i):
        r = launch_fn(label, warmup, i)
        measure = config.get("measure")
        side_hash = os.environ.get("FAKE_%s_HASH" % target.upper(), "c0ffee00")
        if measure == "edits":
            r.update({"edit_start_chars": 4100, "edit_apply_ms": 50.0 + i, "edit_apply_frame_ms": 60.0 + i,
                      "edit_applied_chars": 4100 + config["script"]["count"], "edit_applied_hash": side_hash,
                      "edit_undo_ms": 40.0 + i, "edit_undo_frame_ms": 45.0 + i, "edit_undo_steps": config["script"]["count"],
                      "edit_undone_chars": 4100, "edit_undone_hash": "deadbeef",
                      "start_hwm_sum_mb": 100.0, "start_hwm_max_mb": 100.0, "peak_hwm_sum_mb": 120.0 + i, "peak_hwm_max_mb": 120.0 + i})
        if measure == "search":
            r.update({"search_first_ms": 20.0 + i, "search_complete_ms": 90.0 + i,
                      "search_files": int(os.environ.get("FAKE_FILES", "3")), "search_matches": int(os.environ.get("FAKE_MATCHES", "7")),
                      "start_hwm_sum_mb": 100.0, "start_hwm_max_mb": 100.0, "peak_hwm_sum_mb": 130.0 + i, "peak_hwm_max_mb": 130.0 + i})
        return r
    return wrapped
if target == "gui":
    launch = gui_launch
launch = script_fields(launch)
launches = [launch("warmup-%d" % i, True, 9) for i in range(1, config["warmup"] + 1)]
launches += [launch("rep-%d" % i, False, i) for i in range(1, config["repetitions"] + 1)]
status = os.environ.get("FAKE_STATUS", "ok")
results = {"schema": "rustode-bench-app-results/1", "status": status, "node": "v0-fake",
           "playwright_core": "0-fake", "launches": launches}
if status != "ok":
    results["reason"] = os.environ.get("FAKE_REASON", "")
with open(os.path.join(config["out_dir"], "results.json"), "w") as f:
    json.dump(results, f)
sys.exit(int(os.environ.get("FAKE_EXIT", "0")))
"#;

/// Stands in for a command on `PATH`: appends its arguments to a call log.
const FAKE_RECORDER: &str =
    "#!/bin/sh\necho \"$(basename \"$0\") $*\" >> \"$FAKE_CALLS/path-commands.txt\"\n";

/// A throwaway folder with a fake project, App, QA harness, Node and `/proc`.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "bench-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("sandbox");
        let sandbox = Self { root };
        sandbox.write("bench/scenarios.toml", TINY_LIST);
        sandbox.write("bench/launch.cjs", "// fake driver, never run\n");
        sandbox.write("bench/app/settings.json", "{}\n");
        sandbox.fake_app();
        sandbox.fake_harness();
        sandbox.fake_proc(4_000, &[]);
        fs::create_dir_all(sandbox.path("calls")).expect("calls");
        fs::create_dir_all(sandbox.path("copies")).expect("copies");
        sandbox
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(path, text).expect("write");
    }

    fn write_executable(&self, rel: &str, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        self.write(rel, text);
        fs::set_permissions(self.path(rel), fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    fn app_dir(&self) -> PathBuf {
        self.path("project/01-app/VSCode-linux-x64")
    }

    /// A few files and their manifest, built the way `assemble.sh` builds it.
    fn fake_app(&self) {
        self.write_executable(
            "project/01-app/VSCode-linux-x64/codium",
            "#!/bin/sh\nexit 1\n",
        );
        self.write(
            "project/01-app/VSCode-linux-x64/resources/app/product.json",
            "{}\n",
        );
        self.write("project/01-app/VSCode-linux-x64/bin/codium", "#!/bin/sh\n");
        self.write(
            "project/02-package-analysis/version.txt",
            "1.147.0\nabc123\nx64\n",
        );
        let out = Command::new("sh")
            .args([
                "-c",
                "find . -type f -print0 | sort -z | xargs -0 sha256sum",
            ])
            .current_dir(self.app_dir())
            .output()
            .expect("sha256sum");
        assert!(out.status.success());
        fs::write(
            self.path("project/02-package-analysis/manifest-sha256.txt"),
            out.stdout,
        )
        .expect("manifest");
    }

    fn fake_harness(&self) {
        self.write_executable("qa/suite/tools/run_virtual.sh", FAKE_RUN_VIRTUAL);
        self.write("qa/suite/collectors/sampler.py", "# fake, never run\n");
        self.write("qa/50_mesa.json", "{}\n");
        self.write("qa/suite/scenarios/lib.cjs", "// fake, never run\n");
        self.write("node_modules/playwright-core/package.json", "{}\n");
        self.write_executable("bin/node", "#!/bin/sh\nexit 1\n");
        self.write_executable("fakebin/systemd-run", FAKE_RECORDER);
        self.write_executable("fakebin/systemctl", FAKE_RECORDER);
    }

    /// A fake `/proc` with `MemAvailable` in MB and `(pid, argv0)` processes.
    fn fake_proc(&self, available_mb: u64, processes: &[(u32, &str)]) {
        let _ = fs::remove_dir_all(self.path("proc"));
        self.write(
            "proc/meminfo",
            &format!(
                "MemTotal:        8000000 kB\nMemAvailable:    {} kB\n",
                available_mb * 1024
            ),
        );
        for (pid, argv0) in processes {
            self.write(
                &format!("proc/{pid}/cmdline"),
                &format!("{argv0}\0--type=zygote\0"),
            );
        }
    }

    /// `rustode` with the sandbox's harness, lock, `/proc` and `PATH`.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(RUSTODE);
        let path = format!(
            "{}:{}",
            self.path("fakebin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .args(args)
            .current_dir(&self.root)
            .env("PATH", path)
            .env("RUSTODE_QA_ROOT", self.path("qa"))
            .env("RUSTODE_NODE", self.path("bin/node"))
            .env("RUSTODE_BENCH_LOCK", self.path("bench.lock"))
            .env("RUSTODE_BENCH_PROC", self.path("proc"))
            .env("RUSTODE_BENCH_MESA_EGL", self.path("qa/50_mesa.json"))
            .env("FAKE_CALLS", self.path("calls"))
            .env_remove("RUSTODE_LOG");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("rustode runs")
    }

    /// `rustode bench open-file-tiny` with the sandbox's project, out and copy folders.
    fn bench(&self, extra: &[&str]) -> Output {
        self.bench_with(extra, &[])
    }

    fn bench_with(&self, extra: &[&str], env: &[(&str, &str)]) -> Output {
        let project = self.path("project");
        let copies = self.path("copies");
        let mut args = vec![
            "bench",
            "open-file-tiny",
            "--project",
            project.to_str().expect("utf-8"),
            "--out",
            "out",
            "--app-copy-dir",
            copies.to_str().expect("utf-8"),
        ];
        args.extend_from_slice(extra);
        self.command(&args)
            .envs(env.iter().copied())
            .output()
            .expect("rustode runs")
    }

    /// The only run folder under `out/`.
    fn run_folder(&self) -> PathBuf {
        let entries: Vec<PathBuf> = fs::read_dir(self.path("out"))
            .expect("out folder")
            .map(|e| e.expect("entry").path())
            .collect();
        assert_eq!(entries.len(), 1, "one run folder: {entries:?}");
        entries.into_iter().next().expect("run folder")
    }

    fn report(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.run_folder().join("report.json")).expect("report.json");
        serde_json::from_str(&text).expect("report is JSON")
    }

    fn harness_call(&self) -> Option<serde_json::Value> {
        let text = fs::read_to_string(self.path("calls/run_virtual.json")).ok()?;
        Some(serde_json::from_str(&text).expect("call is JSON"))
    }

    fn copies_left(&self) -> usize {
        fs::read_dir(self.path("copies")).expect("copies").count()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A result row of the report, by side and metric.
fn result<'a>(report: &'a serde_json::Value, side: &str, metric: &str) -> &'a serde_json::Value {
    report["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|r| r["side"] == side && r["metric"] == metric)
        .unwrap_or_else(|| panic!("no {side} {metric} in {report:#}"))
}

#[test]
fn help_describes_options_environment_and_exit_codes() {
    let out = Command::new(RUSTODE)
        .args(["bench", "--help"])
        .output()
        .expect("runs");
    assert!(out.status.success());
    let text = stdout(&out);
    for needle in [
        "Usage: rustode bench",
        "--dry-run",
        "--list",
        "--side",
        "--out",
        "RUSTODE_QA_ROOT",
        "Exit codes",
        "INTERROMPIDA",
    ] {
        assert!(text.contains(needle), "help lacks {needle}: {text}");
    }
}

#[test]
fn the_versioned_scenario_list_has_the_open_file_scenario() {
    let out = Command::new(RUSTODE)
        .args(["bench", "--list"])
        .current_dir(repo_root())
        .output()
        .expect("runs");
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("bench/scenarios.toml"), "{text}");
    assert!(text.contains("version 3"), "{text}");
    assert!(text.contains("open-file-1mb"), "{text}");
    assert!(text.contains("1048576 bytes"), "{text}");
}

#[test]
fn dry_run_prints_the_plan_and_launches_nothing() {
    let sandbox = Sandbox::new("dry-run");
    let list = repo_root().join("bench/scenarios.toml");
    let out = sandbox.run(&[
        "bench",
        "--dry-run",
        "open-file-1mb",
        "--scenarios",
        list.to_str().expect("utf-8"),
        "--out",
        "out",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let plan = stdout(&out);
    for needle in [
        "dry run: nothing is launched or written",
        "run ",
        "command ",
        "folder ",
        "open-file-1mb",
        "list version 3",
        "1048576 bytes",
        "Workspace::open",
        "sha256sum -c --quiet",
        "not in the manifest",
        "one App at a time",
        "2000 MB",
        "700",
        "run_virtual.sh bwrap --dev-bind / / --unshare-net --die-with-parent",
        "--user-data-dir={profile}/ud",
        "--extensions-dir={profile}/ext",
        "--shared-data-dir={profile}/shared",
        "--remote-debugging-port=9333",
        "sampler.py",
        "report.txt",
    ] {
        assert!(plan.contains(needle), "plan lacks {needle:?}:\n{plan}");
    }
    assert!(!sandbox.path("out").exists(), "a dry run writes nothing");
    assert!(
        sandbox.harness_call().is_none(),
        "a dry run launches nothing"
    );
    assert!(
        !sandbox.path("calls/path-commands.txt").exists(),
        "a dry run starts no reaper"
    );
}

#[test]
fn unknown_scenario_is_a_usage_error() {
    let sandbox = Sandbox::new("unknown");
    let out = sandbox.run(&["bench", "no-such-scenario", "--dry-run"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("no-such-scenario"), "{err}");
    assert!(err.contains("open-file-tiny"), "lists the known ids: {err}");
}

#[test]
fn unknown_option_is_a_usage_error() {
    let out = Command::new(RUSTODE)
        .args(["bench", "--frobnicate"])
        .output()
        .expect("runs");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--frobnicate"));
}

#[test]
fn malformed_scenario_list_is_reported_with_its_path() {
    let sandbox = Sandbox::new("malformed");
    sandbox.write(
        "bench/scenarios.toml",
        "version = 1\n[[scenario]]\nid = 3\n",
    );
    let out = sandbox.run(&["bench", "--list"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("scenarios.toml"), "{}", stderr(&out));
}

/// The generated fixture: exact size, numbered lines, LF.
fn assert_fixture(folder: &Path) {
    let bytes = fs::read(folder.join("fixtures/open-file-tiny.txt")).expect("fixture");
    assert_eq!(bytes.len(), 4100);
    assert!(
        bytes.starts_with(b"00000001 the quick brown fox"),
        "numbered lines"
    );
    let text = std::str::from_utf8(&bytes).expect("ASCII");
    assert_eq!(text.matches('\n').count(), 4100 / 64);
}

/// The report says what ran, from where, as which run.
fn assert_header(report: &serde_json::Value, folder: &Path) {
    let run = folder
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    assert_eq!(report["run"], run.as_str());
    assert!(run.ends_with("-open-file-tiny"), "{run}");
    assert_eq!(report["status"], "ok");
    let command = report["command"].as_str().expect("command");
    assert!(command.contains("bench open-file-tiny"), "{command}");
    assert!(report["folder"].as_str().is_some_and(|f| !f.is_empty()));
    assert_eq!(report["scenario"]["list_version"], 7);
}

#[test]
fn the_rustode_side_runs_in_process_and_writes_a_labeled_report() {
    let sandbox = Sandbox::new("rustode-side");
    let out = sandbox.bench(&["--side", "rustode"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let folder = sandbox.run_folder();
    assert_fixture(&folder);
    let report = sandbox.report();
    assert_header(&report, &folder);
    let open = result(&report, "rustode", "workspace_open");
    assert_eq!(open["label"], "measured");
    assert_eq!(open["n"], 5);
    assert_eq!(open["samples"].as_array().expect("samples").len(), 5);
    assert_eq!(report["app"]["status"], "skipped");
    assert!(sandbox.harness_call().is_none(), "the App side did not run");

    let text = fs::read_to_string(folder.join("report.txt")).expect("report.txt");
    assert_eq!(
        stdout(&out).trim_end(),
        text.trim_end(),
        "stdout is the report"
    );
    let run = folder
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    assert!(text.contains(&format!("run         {run}")), "{text}");
    assert!(text.contains("measured"), "{text}");
    assert!(text.contains("not like-for-like"), "{text}");
}

fn strings(value: &serde_json::Value) -> Vec<&str> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|a| a.as_str().expect("string"))
        .collect()
}

/// The harness ran the driver without network, and the driver was told to
/// launch the copy with an isolated profile and CDP.
fn assert_launched_from_the_copy(sandbox: &Sandbox) {
    let call = sandbox.harness_call().expect("the harness was launched");
    let argv = strings(&call["argv"]);
    assert_eq!(
        argv[..6],
        [
            "bwrap",
            "--dev-bind",
            "/",
            "/",
            "--unshare-net",
            "--die-with-parent"
        ]
    );
    assert_eq!(argv[6], sandbox.path("bin/node").to_str().expect("utf-8"));
    assert!(argv[7].ends_with("bench/launch.cjs"), "{argv:?}");
    let app_command = strings(&call["config"]["app_command"]);
    for flag in [
        "--user-data-dir={profile}/ud",
        "--extensions-dir={profile}/ext",
        "--shared-data-dir={profile}/shared",
        "--remote-debugging-port=9333",
    ] {
        assert!(app_command.contains(&flag), "{flag} in {app_command:?}");
    }
    let codium = app_command
        .iter()
        .find(|a| a.ends_with("/codium"))
        .expect("codium");
    assert!(codium.starts_with(sandbox.path("copies").to_str().expect("utf-8")));
    assert_eq!(
        call["codium_exists"], true,
        "the copy existed during the run"
    );
    assert_eq!(sandbox.copies_left(), 0, "the copy is deleted afterwards");
    let reaper = fs::read_to_string(sandbox.path("calls/path-commands.txt")).expect("reaper");
    assert!(reaper.contains("systemd-run --user"), "{reaper}");
    assert!(reaper.contains("systemctl --user stop"), "{reaper}");
}

/// Integrity before, of the copy and after; App rows without the warm-up.
fn assert_app_results(report: &serde_json::Value) {
    assert_eq!(report["status"], "ok");
    for stage in ["before", "copy", "after"] {
        assert_eq!(
            report["integrity"][stage]["ok"], true,
            "{stage}: {report:#}"
        );
        assert_eq!(report["integrity"][stage]["listed"], 3);
        assert_eq!(report["integrity"][stage]["unlisted"], 0);
    }
    let first_line = result(report, "app", "launch_to_first_line");
    assert_eq!(first_line["n"], 3, "the warm-up is not recorded");
    assert_eq!(first_line["p50"], 2200.0);
    assert_eq!(first_line["max"], 2300.0);
    assert_eq!(
        result(report, "app", "workbench_to_first_line")["min"],
        290.0
    );
    for row in report["results"].as_array().expect("results") {
        assert!(
            row["label"] == "measured" || row["label"] == "estimated",
            "unlabeled: {row}"
        );
    }
}

#[test]
fn every_result_is_labeled_and_the_app_ran_from_a_verified_copy() {
    let sandbox = Sandbox::new("full");
    let out = sandbox.bench(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_launched_from_the_copy(&sandbox);
    assert_app_results(&sandbox.report());
    let text = fs::read_to_string(sandbox.run_folder().join("report.txt")).expect("report.txt");
    assert!(text.contains("verified copy"), "{text}");
    assert!(text.contains("not like-for-like"), "{text}");
}

#[test]
fn a_changed_app_file_refuses_the_run() {
    let sandbox = Sandbox::new("changed");
    fs::write(
        sandbox.app_dir().join("resources/app/product.json"),
        "{\"x\":1}\n",
    )
    .expect("edit");
    let out = sandbox.bench(&[]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("product.json: FAILED"), "{err}");
    assert!(sandbox.harness_call().is_none());
    assert!(!sandbox.path("out").exists());
}

#[test]
fn a_file_missing_from_the_manifest_refuses_the_run() {
    let sandbox = Sandbox::new("unlisted");
    sandbox.write(
        "project/01-app/VSCode-linux-x64/resources/app/node-compile-cache/extra",
        "x",
    );
    let out = sandbox.bench(&[]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("not in the manifest"), "{err}");
    assert!(
        err.contains("./resources/app/node-compile-cache/extra"),
        "{err}"
    );
    assert!(sandbox.harness_call().is_none());
    assert_eq!(sandbox.copies_left(), 0);
}

#[test]
fn a_running_app_refuses_the_run() {
    let sandbox = Sandbox::new("running");
    sandbox.fake_proc(4_000, &[(4242, "/opt/vscodium-bin/codium")]);
    let out = sandbox.bench(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("one App at a time"), "{err}");
    assert!(err.contains("4242"), "{err}");
    assert!(sandbox.harness_call().is_none());
}

#[test]
fn a_held_lock_refuses_the_run() {
    let sandbox = Sandbox::new("locked");
    let lock = fs::File::create(sandbox.path("bench.lock")).expect("lock file");
    lock.try_lock().expect("the test holds the lock");
    let out = sandbox.bench(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("another rustode bench"),
        "{}",
        stderr(&out)
    );
    assert!(sandbox.harness_call().is_none());
}

#[test]
fn too_little_memory_refuses_the_run() {
    let sandbox = Sandbox::new("memory");
    sandbox.fake_proc(1_500, &[]);
    let out = sandbox.bench(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("1500 MB"), "{err}");
    assert!(err.contains("2000 MB"), "{err}");
    assert!(sandbox.harness_call().is_none());
}

#[test]
fn a_watchdog_abort_marks_the_run_interrupted() {
    let sandbox = Sandbox::new("aborted");
    let out = sandbox.bench_with(
        &[],
        &[
            ("FAKE_STATUS", "aborted"),
            ("FAKE_REASON", "rep-2: MemAvailable 650 MB < 700 MB"),
            ("FAKE_EXIT", "5"),
        ],
    );
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let folder = sandbox.run_folder();
    assert!(
        folder.to_string_lossy().ends_with("-INTERROMPIDA"),
        "{}",
        folder.display()
    );
    let report = sandbox.report();
    assert_eq!(report["status"], "aborted");
    assert!(
        report["app"]["reason"]
            .as_str()
            .expect("reason")
            .contains("650 MB")
    );
    assert_eq!(sandbox.copies_left(), 0);
}

#[test]
fn an_app_that_writes_into_01_app_invalidates_the_run() {
    let sandbox = Sandbox::new("writes");
    let target = sandbox.app_dir().join("resources/app/written-during-run");
    let out = sandbox.bench_with(&[], &[("FAKE_WRITE_INTO", target.to_str().expect("utf-8"))]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("written-during-run"),
        "{}",
        stderr(&out)
    );
    let folder = sandbox.run_folder();
    assert!(
        folder.to_string_lossy().ends_with("-INVALIDA"),
        "{}",
        folder.display()
    );
    assert_eq!(sandbox.report()["status"], "invalid");
}

#[test]
fn the_run_folder_may_not_be_inside_the_evidence_folders() {
    let sandbox = Sandbox::new("evidence-out");
    let out_dir = sandbox.path("project/01-app/bench");
    let out = sandbox.bench(&["--out", out_dir.to_str().expect("utf-8")]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("01-app"), "{}", stderr(&out));
    assert!(!out_dir.exists());
}

/// The App driver at least parses: it only ever runs against the App, so
/// this is the one check it gets here. Skipped when Node is not installed.
#[test]
fn the_app_driver_is_valid_javascript() {
    let node = std::env::var_os("RUSTODE_NODE").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/opt/node-v24.22.1-linux-x64/bin/node")
        },
        PathBuf::from,
    );
    if !node.exists() {
        eprintln!("skipped: no Node at {}", node.display());
        return;
    }
    let out = Command::new(&node)
        .arg("--check")
        .arg(repo_root().join("bench/launch.cjs"))
        .output()
        .expect("node runs");
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn launches_killed_after_sigterm_are_flagged() {
    let sandbox = Sandbox::new("sigkill");
    let out = sandbox.bench_with(&[], &[("FAKE_SIGKILL", "warmup-1,rep-2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let notes = sandbox.report()["notes"].clone();
    let flagged = notes
        .as_array()
        .expect("notes")
        .iter()
        .find(|n| n["text"].as_str().is_some_and(|t| t.contains("SIGKILL")))
        .unwrap_or_else(|| panic!("no SIGKILL note in {notes:#}"));
    assert_eq!(flagged["label"], "measured");
    let text = flagged["text"].as_str().expect("text");
    assert!(text.contains("2 of 4 launches"), "{text}");
    assert!(text.contains("warmup-1, rep-2"), "{text}");
    let report = fs::read_to_string(sandbox.run_folder().join("report.txt")).expect("report.txt");
    assert!(report.contains("warmup-1, rep-2"), "{report}");
}

/// A list with one scenario of each kind that launches the GUI.
const LAUNCH_LIST: &str = r#"
version = 3

[[scenario]]
id = "startup-tiny"
title = "Startup, tiny"
kind = "launch"

[scenario.fixture]
bytes = 4100
line_bytes = 64

[scenario.launch]
measure = "idle"
cache = "cold"
settle_s = 3

[scenario.gui]
warmup = 1
repetitions = 4
timeout_s = 5

[scenario.app]
warmup = 1
repetitions = 3
timeout_s = 5
poll_ms = 10
driver = "launch.cjs"
settings = "app/settings.json"
"#;

impl Sandbox {
    /// A `rustode-gui` that is never run (the fake harness stands in for the driver).
    fn fake_gui(&self) -> PathBuf {
        self.write_executable(
            "bin/rustode-gui",
            "#!/bin/sh\necho 'rustode-gui 0.0.0-fake'\n",
        );
        self.path("bin/rustode-gui")
    }

    fn bench_launch(&self, extra: &[&str]) -> Output {
        self.write("bench/scenarios.toml", LAUNCH_LIST);
        self.write("bench/evict.py", "# fake, never run\n");
        let gui = self.fake_gui();
        let project = self.path("project");
        let copies = self.path("copies");
        let mut args = vec![
            "bench",
            "startup-tiny",
            "--project",
            project.to_str().expect("utf-8"),
            "--out",
            "out",
            "--app-copy-dir",
            copies.to_str().expect("utf-8"),
            "--gui-bin",
            gui.to_str().expect("utf-8"),
        ];
        args.extend_from_slice(extra);
        self.command(&args).output().expect("rustode runs")
    }
}

/// The GUI's driver call: its own target, the settings of the scenario, the
/// software-rendering environment, and no way to reach the machine's GPU.
fn assert_gui_call(sandbox: &Sandbox) {
    let gui: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(sandbox.path("calls/run_virtual-gui.json")).expect("gui call"),
    )
    .expect("json");
    let config = &gui["config"];
    assert_eq!(config["target"], "gui");
    assert_eq!(config["measure"], "idle");
    assert_eq!(config["cache"], "cold");
    assert_eq!(config["repetitions"], 4);
    assert_eq!(config["timeout_is_result"], false);
    assert!(strings(&config["app_command"])[0].ends_with("bin/rustode-gui"));
    assert_eq!(config["env"]["RUSTODE_REPORT_FIRST_FRAME"], "1");
    assert_eq!(config["env"]["LIBGL_ALWAYS_SOFTWARE"], "1");
    assert!(
        config["env"]["__EGL_VENDOR_LIBRARY_FILENAMES"]
            .as_str()
            .is_some_and(|f| f.ends_with("50_mesa.json")),
        "the GPU's EGL driver stays out: {config}"
    );
    assert!(strings(&config["sampler_command"]).contains(&"{pid}"));
    assert_eq!(
        gui["argv"][0], "bwrap",
        "run_virtual.sh runs bwrap, then the driver"
    );
    assert!(strings(&gui["argv"]).contains(&"--unshare-net"));
}

/// Every metric of both sides is there, once per recorded launch.
fn assert_launch_rows(report: &serde_json::Value) {
    let first = result(report, "rustode", "launch_to_first_frame");
    assert_eq!(first["n"], 4, "warm-ups are not recorded");
    assert_eq!(first["label"], "measured");
    for (side, metric, n) in [
        ("rustode", "idle_pss", 4),
        ("rustode", "ready_rss", 4),
        ("app", "ready_pss", 3),
        ("app", "launch_to_first_paint", 3),
        ("app", "idle_pss", 3),
    ] {
        assert_eq!(result(report, side, metric)["n"], n, "{side} {metric}");
    }
}

#[test]
fn a_launch_scenario_runs_the_gui_and_the_app_through_the_same_driver() {
    let sandbox = Sandbox::new("launch");
    let out = sandbox.bench_launch(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let report = sandbox.report();
    assert_eq!(report["scenario"]["kind"], "launch");
    assert_eq!(report["inputs"]["launch"]["cache"], "cold");
    assert_eq!(report["inputs"]["launch"]["settle_s"], 3);
    assert_eq!(report["gui"]["status"], "ok");
    assert_eq!(report["gui"]["version"], "rustode-gui 0.0.0-fake");
    assert_eq!(report["like_for_like"], false);

    // The GUI ran first, as its own target; then the App, as before.
    assert_gui_call(&sandbox);
    let app = sandbox.harness_call().expect("the App ran");
    assert_eq!(app["config"]["target"], "app");
    assert_eq!(app["config"]["cache"], "cold");
    assert_launch_rows(&report);

    let text = fs::read_to_string(sandbox.run_folder().join("report.txt")).expect("report.txt");
    for needle in [
        "rustode GUI",
        "cold cache",
        "no common 'first frame' event",
        "wgpu present",
        "1 of 4 recorded launches were not fully cold",
        "rep-2 (28.4% after 5 attempts)",
    ] {
        assert!(text.contains(needle), "report lacks {needle:?}:\n{text}");
    }
}

#[test]
fn a_launch_scenario_without_a_gui_or_launch_table_is_refused() {
    let sandbox = Sandbox::new("launch-bad");
    sandbox.write(
        "bench/scenarios.toml",
        &LAUNCH_LIST.replace(
            "[scenario.gui]\nwarmup = 1\nrepetitions = 4\ntimeout_s = 5\n",
            "",
        ),
    );
    let out = sandbox.run(&["bench", "--list"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("scenario.gui"), "{}", stderr(&out));
}

#[test]
fn an_idle_scenario_needs_a_settle_time() {
    let sandbox = Sandbox::new("launch-settle");
    sandbox.write(
        "bench/scenarios.toml",
        &LAUNCH_LIST.replace("settle_s = 3", "settle_s = 0"),
    );
    let out = sandbox.run(&["bench", "--list"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("settle_s"), "{}", stderr(&out));
}

#[test]
fn a_missing_gui_binary_is_a_usage_error_before_anything_is_written() {
    let sandbox = Sandbox::new("launch-nogui");
    sandbox.write("bench/scenarios.toml", LAUNCH_LIST);
    let project = sandbox.path("project");
    let out = sandbox.run(&[
        "bench",
        "startup-tiny",
        "--project",
        project.to_str().expect("utf-8"),
        "--out",
        "out",
        "--gui-bin",
        "/nonexistent/rustode-gui",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("rustode-gui"), "{}", stderr(&out));
    assert!(!sandbox.path("out").exists());
}

#[test]
fn the_dry_run_of_a_launch_scenario_plans_the_gui_without_launching_it() {
    let sandbox = Sandbox::new("launch-dry");
    let list = repo_root().join("bench/scenarios.toml");
    let out = sandbox.run(&[
        "bench",
        "--dry-run",
        "startup-cold",
        "--scenarios",
        list.to_str().expect("utf-8"),
        "--out",
        "out",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let plan = stdout(&out);
    for needle in [
        "rustode GUI",
        "RUSTODE_REPORT_FIRST_FRAME=1",
        "WGPU_BACKEND=gl",
        "file cache cold",
        "--unshare-net",
    ] {
        assert!(plan.contains(needle), "plan lacks {needle:?}:\n{plan}");
    }
    assert!(
        !plan.contains("Workspace::open"),
        "a launch scenario has no in-process side"
    );
    assert!(!sandbox.path("out").exists());
}

#[test]
fn the_versioned_list_has_the_ticket_26_scenarios() {
    let out = Command::new(RUSTODE)
        .args(["bench", "--list"])
        .current_dir(repo_root())
        .output()
        .expect("runs");
    let text = stdout(&out);
    for id in [
        "open-file-1mb",
        "open-file-100mb",
        "open-file-1gb",
        "startup-cold",
        "startup-warm",
        "idle-rss",
    ] {
        assert!(text.contains(id), "{id} missing: {text}");
    }
    assert!(
        !text.contains("typing"),
        "typing latency waits for ADR 0028: {text}"
    );
}

#[test]
fn the_eviction_helper_drops_files_and_says_how_many() {
    let sandbox = Sandbox::new("evict");
    sandbox.write("cache/a.bin", "aaaa");
    sandbox.write("cache/sub/b.bin", "bbbbbbbb");
    let out = Command::new("python3")
        .arg("-I")
        .arg(repo_root().join("bench/evict.py"))
        .arg(sandbox.path("cache"))
        .arg(sandbox.path("no-such-file"))
        .output();
    let Ok(out) = out else {
        eprintln!("skipped: no python3");
        return;
    };
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "evicted 2 files, 0 MiB");
    assert_eq!(
        fs::read_to_string(sandbox.path("cache/a.bin")).expect("still there"),
        "aaaa",
        "eviction never changes a file"
    );
}

/// A list with one scenario of each new kind, small enough to generate at once.
const SCRIPT_LIST: &str = r#"
version = 9

[[scenario]]
id = "edits-tiny"
title = "Edits, tiny"
kind = "edit-undo"

[scenario.fixture]
bytes = 4096
line_bytes = 64

[scenario.edits]
seed = 5
count = 40

[scenario.gui]
warmup = 1
repetitions = 2
timeout_s = 5

[scenario.app]
warmup = 1
repetitions = 2
timeout_s = 5
poll_ms = 10
driver = "launch.cjs"
settings = "app/settings.json"

[[scenario]]
id = "search-tiny"
title = "Search, tiny"
kind = "search"

[scenario.fixture]
bytes = 2048
line_bytes = 64

[scenario.search]
files = 40
seed = 3
pattern = '\bhandle_[a-z]+_[0-9]{3}\b'
regex = true
include = "**/*.ts,**/*.rs"
exclude = "**/legacy/**"

[scenario.gui]
warmup = 1
repetitions = 2
timeout_s = 5

[scenario.app]
warmup = 1
repetitions = 2
timeout_s = 5
poll_ms = 10
driver = "launch.cjs"
settings = "app/settings.json"
"#;

impl Sandbox {
    fn bench_script(&self, id: &str, env: &[(&str, &str)]) -> Output {
        self.write("bench/scenarios.toml", SCRIPT_LIST);
        self.write("bench/evict.py", "# fake, never run\n");
        let gui = self.fake_gui();
        let project = self.path("project");
        let copies = self.path("copies");
        self.command(&[
            "bench",
            id,
            "--project",
            project.to_str().expect("utf-8"),
            "--out",
            "out",
            "--app-copy-dir",
            copies.to_str().expect("utf-8"),
            "--gui-bin",
            gui.to_str().expect("utf-8"),
        ])
        .envs(env.iter().copied())
        .output()
        .expect("rustode runs")
    }

    fn note(&self, needle: &str) -> String {
        let report = self.report();
        report["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .filter_map(|n| n["text"].as_str())
            .find(|t| t.contains(needle))
            .unwrap_or_else(|| panic!("no note with {needle:?} in {:#}", report["notes"]))
            .to_owned()
    }
}

#[test]
fn the_versioned_list_has_the_ticket_42_scenarios_and_no_typing() {
    let out = Command::new(RUSTODE)
        .args(["bench", "--list"])
        .current_dir(repo_root())
        .output()
        .expect("runs");
    let text = stdout(&out);
    assert!(text.contains("list version 3"), "{text}");
    for id in [
        "edit-undo-10k",
        "search-monorepo",
        "startup-cold",
        "open-file-1gb",
    ] {
        assert!(text.contains(id), "{id} missing: {text}");
    }
    assert!(
        !text.contains("typing"),
        "typing waits for ADR 0028: {text}"
    );
}

#[test]
fn the_dry_run_of_the_edit_scenario_gives_the_gui_a_script_and_sends_no_key() {
    let sandbox = Sandbox::new("edits-dry");
    let list = repo_root().join("bench/scenarios.toml");
    let out = sandbox.run(&[
        "bench",
        "--dry-run",
        "edit-undo-10k",
        "--scenarios",
        list.to_str().expect("utf-8"),
        "--out",
        "out",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let plan = stdout(&out);
    for needle in [
        "RUSTODE_BENCH_SCRIPT={\"kind\":\"edits\",\"seed\":1,\"count\":10000}",
        "no key is sent",
        "--unshare-net",
    ] {
        assert!(plan.contains(needle), "plan lacks {needle:?}:\n{plan}");
    }
    assert!(!sandbox.path("out").exists());
}

#[test]
fn the_dry_run_of_the_search_scenario_plans_a_generated_monorepo() {
    let sandbox = Sandbox::new("search-dry");
    let list = repo_root().join("bench/scenarios.toml");
    let out = sandbox.run(&[
        "bench",
        "--dry-run",
        "search-monorepo",
        "--scenarios",
        list.to_str().expect("utf-8"),
        "--out",
        "out",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let plan = stdout(&out);
    assert!(plan.contains("generate the monorepo"), "{plan}");
    assert!(plan.contains("5000 source files"), "{plan}");
    assert!(plan.contains("\"kind\":\"search\""), "{plan}");
    assert!(
        plan.contains("search-monorepo"),
        "the folder is the fixture: {plan}"
    );
}

#[test]
fn a_new_kind_without_its_table_is_refused() {
    let sandbox = Sandbox::new("script-bad");
    for (bad, needle) in [
        (
            SCRIPT_LIST.replace("[scenario.edits]\nseed = 5\ncount = 40\n", ""),
            "scenario.edits",
        ),
        (SCRIPT_LIST.replace("count = 40", "count = 0"), "count"),
        (SCRIPT_LIST.replace("files = 40", "files = 0"), "files"),
        (
            SCRIPT_LIST.replace("kind = \"search\"", "kind = \"open-file\""),
            "belongs to search",
        ),
    ] {
        sandbox.write("bench/scenarios.toml", &bad);
        let out = sandbox.run(&["bench", "--list"]);
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert!(stderr(&out).contains(needle), "{needle}: {}", stderr(&out));
    }
}

#[test]
fn the_edit_scenario_checks_that_both_sides_hold_the_same_text() {
    let sandbox = Sandbox::new("edits-run");
    let out = sandbox.bench_script("edits-tiny", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let report = sandbox.report();
    assert_eq!(report["scenario"]["kind"], "edit-undo");
    assert_eq!(report["inputs"]["script"]["count"], 40);
    for (side, metric, n) in [
        ("rustode", "edit_apply", 2),
        ("rustode", "edit_undo_frame", 2),
        ("app", "edit_apply_frame", 2),
        ("app", "peak_hwm", 2),
        ("app", "edit_undo_steps", 2),
    ] {
        assert_eq!(result(&report, side, metric)["n"], n, "{side} {metric}");
    }
    assert_eq!(result(&report, "app", "edit_undo_steps")["p50"], 40.0);
    assert!(
        sandbox
            .note("text hash after the inserts")
            .ends_with(": equal")
    );
    assert!(sandbox.note("undo steps").contains("oracle 40"));
    // The GUI's call carries the script, the App's driver config too.
    let gui: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(sandbox.path("calls/run_virtual-gui.json")).expect("gui call"),
    )
    .expect("json");
    assert_eq!(gui["config"]["measure"], "edits");
    assert_eq!(gui["config"]["script"]["seed"], 5);
    assert_eq!(gui["config"]["script"]["start_chars"], 4096);
    let script = gui["config"]["env"]["RUSTODE_BENCH_SCRIPT"]
        .as_str()
        .expect("script env");
    assert!(script.contains("\"kind\":\"edits\""), "{script}");
    assert_eq!(
        sandbox.harness_call().expect("app")["config"]["measure"],
        "edits"
    );
}

#[test]
fn different_text_on_the_two_sides_is_called_out() {
    let sandbox = Sandbox::new("edits-diff");
    let out = sandbox.bench_script("edits-tiny", &[("FAKE_APP_HASH", "12345678")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let note = sandbox.note("text hash after the inserts");
    assert!(note.contains("NOT EQUAL"), "{note}");
    assert!(
        note.contains("12345678") && note.contains("c0ffee00"),
        "{note}"
    );
}

/// The matches of the generated tree, counted by `grep` (another engine than
/// the generator's own oracle and than either editor's).
fn grep_counts(tree: &Path) -> (u64, u64) {
    let out = Command::new("grep")
        .args(["-rEoc", r"\bhandle_[a-z]+_[0-9]{3}\b"])
        .args(["--include=*.ts", "--include=*.rs", "--exclude-dir=legacy"])
        .arg("src")
        .current_dir(tree)
        .output()
        .expect("grep runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let counts: Vec<u64> = text
        .lines()
        .filter_map(|l| l.rsplit(':').next()?.parse().ok())
        .collect();
    (
        counts.iter().filter(|c| **c > 0).count() as u64,
        counts.iter().sum(),
    )
}

#[test]
fn the_search_scenario_generates_a_tree_whose_counts_an_independent_grep_confirms() {
    let sandbox = Sandbox::new("search-run");
    let out = sandbox.bench_script("search-tiny", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let report = sandbox.report();
    let tree = &report["inputs"]["fixture"]["tree"];
    assert_eq!(tree["oracle_applies"], true);
    let folder = sandbox.run_folder().join("fixtures/search-tiny");
    let (files, matches) = grep_counts(&folder);
    assert_eq!(
        tree["expected_files"], files,
        "files with matches: oracle against grep"
    );
    assert_eq!(
        tree["expected_matches"], matches,
        "matches: oracle against grep"
    );
    assert!(matches > 0, "the tiny tree has something to find");
    // The fake sides reported 3 files and 7 matches: not the oracle's.
    assert!(sandbox.note("matches: ").contains("NOT EQUAL"));
    // Hidden folders exist in the tree, so the excludes have work to do.
    for hidden in ["node_modules", "generated", "logs", "src/legacy"] {
        assert!(folder.join(hidden).is_dir(), "{hidden}");
    }
    assert!(folder.join(".gitignore").is_file());
    let text = fs::read_to_string(sandbox.run_folder().join("report.txt")).expect("report.txt");
    assert!(
        text.contains("oracle      the generator planted the matches"),
        "{text}"
    );
    // A second run whose sides report what the oracle expects.
    let (e_files, e_matches) = (files.to_string(), matches.to_string());
    let sandbox = Sandbox::new("search-run-ok");
    let out = sandbox.bench_script(
        "search-tiny",
        &[("FAKE_FILES", &e_files), ("FAKE_MATCHES", &e_matches)],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(sandbox.note("matches: ").ends_with(": equal"));
    assert_eq!(result(&sandbox.report(), "app", "search_complete")["n"], 2);
}

#[test]
fn the_app_internals_module_is_valid_javascript_and_its_generator_matches_the_rust_one() {
    let node = std::env::var_os("RUSTODE_NODE").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/opt/node-v24.22.1-linux-x64/bin/node")
        },
        PathBuf::from,
    );
    if !node.exists() {
        eprintln!("skipped: no Node at {}", node.display());
        return;
    }
    let module = repo_root().join("bench/app_internals.cjs");
    let out = Command::new(&node)
        .arg("-e")
        .arg(format!(
            "const m = require({:?}); m.selfCheck(); console.log(JSON.stringify(m.editOffsets(1, 6, 1000)))",
            module.display().to_string()
        ))
        .output()
        .expect("node runs");
    assert!(out.status.success(), "{}", stderr(&out));
    // The six edits that crates/workspace/tests/bench.rs pins for the Rust generator.
    assert_eq!(
        stdout(&out).trim(),
        r#"[[496,"r"],[95,"h"],[270,"u"],[320,"e"],[559,"v"],[330,"i"]]"#
    );
    let check = Command::new(&node)
        .arg("--check")
        .arg(&module)
        .output()
        .expect("node runs");
    assert!(check.status.success(), "{}", stderr(&check));
}

#[test]
fn the_apps_trace_of_its_first_frame_is_planned_and_can_be_turned_off() {
    let sandbox = Sandbox::new("trace-dry");
    let list = repo_root().join("bench/scenarios.toml");
    let plan = |extra: &[&str]| {
        let mut args = vec![
            "bench",
            "--dry-run",
            "startup-warm",
            "--scenarios",
            list.to_str().expect("utf-8"),
            "--out",
            "out",
        ];
        args.extend_from_slice(extra);
        let out = sandbox.run(&args);
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out)
    };
    assert!(plan(&[]).contains("each launch is traced"), "{}", plan(&[]));
    assert!(!plan(&["--no-app-trace"]).contains("traced"));
    // The scenarios that script the editor are not traced.
    let edits = sandbox.run(&[
        "bench",
        "--dry-run",
        "edit-undo-10k",
        "--scenarios",
        list.to_str().expect("utf-8"),
        "--out",
        "out",
    ]);
    assert!(!stdout(&edits).contains("traced"));
}

#[test]
fn the_traced_scenarios_hand_the_trace_flag_to_the_app_driver_only() {
    let sandbox = Sandbox::new("trace-run");
    sandbox.write("bench/evict.py", "# fake, never run\n");
    let gui = sandbox.fake_gui();
    sandbox.write(
        "bench/scenarios.toml",
        &LAUNCH_LIST.replace("poll_ms = 10", "poll_ms = 10\ntrace = true"),
    );
    let project = sandbox.path("project");
    let copies = sandbox.path("copies");
    let out = sandbox
        .command(&[
            "bench",
            "startup-tiny",
            "--project",
            project.to_str().expect("utf-8"),
            "--out",
            "out",
            "--app-copy-dir",
            copies.to_str().expect("utf-8"),
            "--gui-bin",
            gui.to_str().expect("utf-8"),
        ])
        .output()
        .expect("runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        sandbox.harness_call().expect("app")["config"]["trace"],
        true
    );
    let gui: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(sandbox.path("calls/run_virtual-gui.json")).expect("gui call"),
    )
    .expect("json");
    assert_eq!(gui["config"]["trace"], false);
}

#[test]
fn a_machine_too_short_of_memory_for_the_in_process_open_skips_that_step_not_the_run() {
    let sandbox = Sandbox::new("in-process-skip");
    // The tiny fixture needs 700 MB (the watchdog's floor); give 600.
    sandbox.fake_proc(600, &[]);
    let out = sandbox.bench(&["--side", "rustode"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let report = sandbox.report();
    assert_eq!(report["status"], "ok", "a skipped step is not a failed run");
    assert!(
        report["results"]
            .as_array()
            .expect("results")
            .iter()
            .all(|r| r["metric"] != "workspace_open"),
        "nothing was measured: {report:#}"
    );
    let note = sandbox.note("was not measured");
    assert!(
        note.contains("MemAvailable 600 MB is below 700 MB"),
        "{note}"
    );
    assert!(note.contains("skipped"), "{note}");
    // With enough memory the same run measures it.
    let sandbox = Sandbox::new("in-process-ok");
    assert!(sandbox.bench(&["--side", "rustode"]).status.success());
    assert_eq!(
        result(&sandbox.report(), "rustode", "workspace_open")["n"],
        5
    );
}
