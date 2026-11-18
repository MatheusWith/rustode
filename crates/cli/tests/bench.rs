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
driver = "app/open_file.cjs"
settings = "app/settings.json"
"#;

/// Stands in for the QA harness's `run_virtual.sh`: records its arguments,
/// then writes the `results.json` the App driver would write.
const FAKE_RUN_VIRTUAL: &str = r#"#!/usr/bin/env python3
import json, os, sys
calls = os.environ["FAKE_CALLS"]
config = json.load(open(sys.argv[-1]))
codium = [a for a in config["app_command"] if a.endswith("/codium")][0]
with open(os.path.join(calls, "run_virtual.json"), "w") as f:
    json.dump({"argv": sys.argv[1:], "codium_exists": os.path.isfile(codium), "config": config}, f)
if os.environ.get("FAKE_WRITE_INTO"):
    with open(os.environ["FAKE_WRITE_INTO"], "w") as f:
        f.write("written by the App")
def launch(label, warmup, i):
    return {"label": label, "warmup": warmup, "spawn_to_cdp_ms": 200.0 + i,
            "spawn_to_workbench_ms": 1800.0 + 10 * i, "spawn_to_first_line_ms": 2000.0 + 100 * i,
            "shutdown_ms": 300.0, "peak_rss_mb": 900 + i,
            "needed_sigkill": label in os.environ.get("FAKE_SIGKILL", "").split(",")}
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
        sandbox.write("bench/app/open_file.cjs", "// fake driver, never run\n");
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
    assert!(text.contains("version 1"), "{text}");
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
        "list version 1",
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
    assert!(argv[7].ends_with("bench/app/open_file.cjs"), "{argv:?}");
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
        .arg(repo_root().join("bench/app/open_file.cjs"))
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
