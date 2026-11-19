//! Workspace seam, real debug adapters: lldb-dap on a tiny Rust program and
//! debugpy on a tiny Python program, each started the way a user sets them
//! up: an entry of `rustode.debugAdapters` and a configuration of
//! `launch.json`.
//!
//! These tests are `#[ignore]`d: they need the programs (a test skips itself,
//! saying so, when `lldb-dap` is not on `PATH` or `python3` cannot import
//! `debugpy`), they build or start a program, and they take seconds. Run them
//! one at a time:
//!
//! ```text
//! cargo nextest run -p rustode-workspace --test dap_real --run-ignored only \
//!     --test-threads 1
//! ```
//!
//! Each test covers: launch, a conditional breakpoint hit (the loop's fourth
//! pass, `i == 3`), stepping, the variables of the stop and an evaluation.
//! Each one ends its adapter and the program, and checks that no process
//! whose command line names its folder is left; a leftover is killed and
//! fails the test. A test aborts when `MemAvailable` drops below 700 MB
//! (AGENTS.md §11.5).
//!
//! The launch configurations follow the adapters' documentation:
//!
//! - lldb-dap (<https://github.com/llvm/llvm-project/blob/main/lldb/tools/lldb-dap/README.md>,
//!   "Launching a debugee" and "Configuration Settings Reference"): `type`
//!   `lldb-dap`, `request` `launch`, `program`, `args`, `cwd` and, from
//!   lldb-dap 21, `console: "internalConsole"`, which is the only one
//!   rustode supports (it answers `runInTerminal` with a refusal);
//! - debugpy (<https://github.com/microsoft/debugpy/wiki/Debug-configuration-settings>):
//!   `type` `debugpy`, `request` `launch`, `program`, `args`, `cwd`,
//!   `console` and `justMyCode`; the adapter is `python3 -m debugpy.adapter`,
//!   which speaks DAP on stdio.

use rustode_workspace::{
    BreakpointSpec, ConsoleKind, DebugState, DebugTree, DocumentId, StepKind, Workspace,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Below this, a test aborts (AGENTS.md §11.5).
const MIN_AVAILABLE_MB: u64 = 700;

fn available_mb() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("MemAvailable:"))
                .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        })
        .map_or(u64::MAX, |kb| kb / 1024)
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// The processes of the adapters under test, by command line.
fn adapter_pids() -> Vec<u32> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
        let first = cmdline.split_whitespace().next().unwrap_or_default();
        if first.ends_with("lldb-dap") || cmdline.contains("debugpy") {
            found.push(pid);
        }
    }
    found
}

/// A temporary folder; at the end it is removed and every process that
/// names it, and every adapter that was not running before the test, is
/// checked for and killed.
struct Dir(PathBuf, Vec<u32>);

impl Dir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rustode-dapreal-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir, adapter_pids())
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(&path, text).expect("fixture");
        path
    }

    /// The processes whose command line names this folder.
    fn leftovers(&self) -> Vec<(u32, String)> {
        let needle = self.0.display().to_string();
        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
            if cmdline.contains(&needle) {
                found.push((pid, cmdline));
            }
        }
        for pid in adapter_pids() {
            if !self.1.contains(&pid) && !found.iter().any(|(p, _)| *p == pid) {
                found.push((pid, "an adapter that was not running before".to_owned()));
            }
        }
        found
    }

    /// Kills what is left and returns it.
    fn reap(&self) -> Vec<(u32, String)> {
        let mut left = self.leftovers();
        // The adapter's exit may be a moment behind the end of the session.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !left.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            left = self.leftovers();
        }
        for (pid, _) in &left {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        }
        left
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Reached on a panic too: nothing outlives the test.
        let _ = self.reap();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[track_caller]
fn wait(ws: &mut Workspace, what: &str, limit: Duration, done: impl Fn(&Workspace) -> bool) {
    let deadline = Instant::now() + limit;
    loop {
        ws.poll_debug();
        if done(ws) {
            return;
        }
        assert!(
            available_mb() >= MIN_AVAILABLE_MB,
            "memory ran short waiting for {what}"
        );
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; status {:?}; console {:?}",
            ws.debug_status(),
            ws.debug_console()
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stopped_at(ws: &Workspace, id: DocumentId, line: usize) -> bool {
    ws.debug_status().state == DebugState::Stopped && ws.stopped_line(id) == Some(line)
}

fn local(ws: &Workspace, name: &str) -> Option<String> {
    ws.debug_rows(DebugTree::Variables)
        .into_iter()
        .find(|r| r.name.trim() == name)
        .map(|r| r.value)
}

/// What the program under test looks like to the checks.
struct Case {
    /// The folder of the project, which is the workspace's folder.
    dir: Dir,
    /// The source file, opened in the workspace.
    source: PathBuf,
    /// The zero-based line of the loop's body, where the breakpoint goes.
    body_line: usize,
}

/// A workspace on the case's folder with the adapter set and `launch.json`
/// written, and the source open.
fn workspace(case: &Case, adapter: &serde_json::Value, launch: &str) -> (Workspace, DocumentId) {
    let user = case.dir.0.join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    std::fs::write(
        user.join("settings.json"),
        serde_json::json!({"rustode.debugAdapters": adapter}).to_string(),
    )
    .expect("settings");
    case.dir.write(".vscode/launch.json", launch);
    let mut ws = Workspace::new();
    ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
    ws.open_folder(&case.dir.0).expect("folder");
    let id = ws.open(&case.source).expect("open");
    (ws, id)
}

/// Ends the workspace and checks that nothing it started is left running.
fn finish(case: &Case, ws: Workspace) {
    drop(ws);
    let left = case.dir.reap();
    assert!(left.is_empty(), "processes left running: {left:?}");
}

const LIMIT: Duration = Duration::from_secs(60);

/// Launch, a conditional breakpoint, an evaluation, a step and the end.
fn run_case(case: &Case, adapter: &serde_json::Value, launch: &str, condition: &str) {
    let (mut ws, id) = workspace(case, adapter, launch);
    let spec = BreakpointSpec {
        condition: Some(condition.to_owned()),
        ..BreakpointSpec::default()
    };
    ws.set_breakpoint(id, case.body_line, spec)
        .expect("breakpoint");
    assert!(ws.debug_problems().is_empty(), "{:?}", ws.debug_problems());

    let started = Instant::now();
    ws.start_debugging().expect("start");

    // The conditional breakpoint: the loop runs on until `i == 3`.
    wait(&mut ws, "the conditional stop", LIMIT, |ws| {
        stopped_at(ws, id, case.body_line)
            && local(ws, "i").is_some()
            && ws
                .debug_rows(DebugTree::Variables)
                .iter()
                .all(|r| !r.loading)
    });
    eprintln!(
        "launch to conditional stop: {} ms (measured, one run)",
        started.elapsed().as_millis()
    );
    assert_eq!(ws.debug_status().reason.as_deref(), Some("breakpoint"));
    assert_eq!(local(&ws, "i").as_deref(), Some("3"));
    assert_eq!(local(&ws, "total").as_deref(), Some("3"), "0 + 1 + 2");
    let info = ws.breakpoint_at(id, case.body_line).expect("kept");
    assert!(info.verified, "the adapter verified it: {info:?}");

    // An evaluation in the console, in the stopped frame.
    ws.evaluate_in_console("total + i").expect("evaluate");
    wait(&mut ws, "the evaluation", LIMIT, |ws| {
        ws.debug_console()
            .iter()
            .any(|l| l.kind == ConsoleKind::Result)
    });
    let result = ws
        .debug_console()
        .iter()
        .find(|l| l.kind == ConsoleKind::Result)
        .map(|l| l.text.clone())
        .expect("a result");
    assert!(result.contains('6'), "3 + 3, got {result:?}");

    // Stepping: over the body's line moves the stop, and the total is 6.
    ws.step_debugging(StepKind::Over).expect("step");
    wait(&mut ws, "the step", LIMIT, |ws| {
        ws.debug_status().state == DebugState::Stopped
            && ws.debug_status().reason.as_deref() == Some("step")
            && ws
                .stopped_line(id)
                .is_some_and(|line| line != case.body_line)
            && ws
                .debug_rows(DebugTree::Variables)
                .iter()
                .all(|r| !r.loading)
    });
    assert_eq!(ws.debug_frames().len().min(1), 1, "a call stack");
    wait(&mut ws, "the total after the step", LIMIT, |ws| {
        local(ws, "total").as_deref() == Some("6")
    });

    // On to the end: the condition does not hold again.
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", LIMIT, |ws| {
        ws.debug_status().state == DebugState::Inactive
    });
    eprintln!(
        "whole session: {} ms (measured, one run)",
        started.elapsed().as_millis()
    );
    for line in ws.debug_console().iter().take(12) {
        eprintln!("console {:?}: {}", line.kind, line.text);
    }
    finish(case, ws);
}

/// A session ended by Stop while stopped: the adapter and the program are
/// gone afterwards.
fn stop_case(case: &Case, adapter: &serde_json::Value, launch: &str) {
    let (mut ws, id) = workspace(case, adapter, launch);
    ws.toggle_breakpoint(id, case.body_line)
        .expect("breakpoint");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", LIMIT, |ws| {
        stopped_at(ws, id, case.body_line)
    });
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", Duration::from_secs(30), |ws| {
        ws.debug_status().state == DebugState::Inactive
    });
    finish(case, ws);
}

// ---- lldb-dap on a Rust program ----

const RUST_SOURCE: &str = "fn main() {\n    let mut total: i32 = 0;\n    for i in 0..6 {\n        total += i;\n    }\n    println!(\"total {total}\");\n}\n";
const CARGO_TOML: &str =
    "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";

/// Builds the Rust program with debug info; `None` when cargo cannot.
fn rust_case(name: &str) -> Option<Case> {
    let dir = Dir::new(name);
    let source = dir.write("src/main.rs", RUST_SOURCE);
    dir.write("Cargo.toml", CARGO_TOML);
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--offline", "--manifest-path"])
        .arg(dir.0.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(dir.0.join("target"))
        .env_remove("CARGO_BUILD_JOBS")
        .env("CARGO_BUILD_JOBS", "1")
        .stdout(Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(Case {
        dir,
        source,
        body_line: 3,
    })
}

fn lldb_launch(dir: &Path) -> String {
    let _ = dir;
    r#"{ "version": "0.2.0", "configurations": [ {
        "name": "Debug fixture", "type": "lldb-dap", "request": "launch",
        "program": "${workspaceFolder}/target/debug/fixture",
        "args": [], "cwd": "${workspaceFolder}", "console": "internalConsole"
    } ] }"#
        .to_owned()
}

fn lldb_adapter() -> serde_json::Value {
    serde_json::json!({"lldb-dap": {"command": "lldb-dap"}})
}

#[test]
#[ignore = "needs lldb-dap on PATH and cargo; run on its own"]
fn real_lldb_dap_conditional_breakpoint_step_variables_evaluate() {
    if !on_path("lldb-dap") {
        eprintln!("skipping: lldb-dap is not on PATH");
        return;
    }
    let Some(case) = rust_case("lldb") else {
        eprintln!("skipping: cargo could not build the fixture");
        return;
    };
    let launch = lldb_launch(&case.dir.0);
    run_case(&case, &lldb_adapter(), &launch, "i == 3");
}

#[test]
#[ignore = "needs lldb-dap on PATH and cargo; run on its own"]
fn real_lldb_dap_stop_ends_the_adapter_and_the_program() {
    if !on_path("lldb-dap") {
        eprintln!("skipping: lldb-dap is not on PATH");
        return;
    }
    let Some(case) = rust_case("lldb-stop") else {
        eprintln!("skipping: cargo could not build the fixture");
        return;
    };
    let launch = lldb_launch(&case.dir.0);
    stop_case(&case, &lldb_adapter(), &launch);
}

// ---- debugpy on a Python program ----

const PY_SOURCE: &str = "def work(n):\n    total = 0\n    for i in range(n):\n        total += i\n    return total\n\n\nprint(work(6))\n";

fn debugpy_available() -> bool {
    on_path("python3")
        && Command::new("python3")
            .args(["-I", "-c", "import debugpy"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

fn python_case(name: &str) -> Case {
    let dir = Dir::new(name);
    let source = dir.write("fixture.py", PY_SOURCE);
    Case {
        dir,
        source,
        body_line: 3,
    }
}

fn debugpy_adapter() -> serde_json::Value {
    serde_json::json!({"debugpy": {
        "command": "python3",
        "args": ["-m", "debugpy.adapter"],
        "env": {"PYDEVD_DISABLE_FILE_VALIDATION": "1"}
    }})
}

fn debugpy_launch(console: Option<&str>) -> String {
    let console = console.map_or_else(String::new, |c| format!(r#""console": "{c}","#));
    format!(
        r#"{{ "version": "0.2.0", "configurations": [ {{
            "name": "Debug fixture", "type": "debugpy", "request": "launch",
            "program": "${{workspaceFolder}}/fixture.py", "args": [],
            "cwd": "${{workspaceFolder}}", {console} "justMyCode": true
        }} ] }}"#
    )
}

#[test]
#[ignore = "needs python3 with debugpy; run on its own"]
fn real_debugpy_conditional_breakpoint_step_variables_evaluate() {
    if !debugpy_available() {
        eprintln!("skipping: python3 cannot import debugpy");
        return;
    }
    let case = python_case("debugpy");
    run_case(
        &case,
        &debugpy_adapter(),
        &debugpy_launch(Some("internalConsole")),
        "i == 3",
    );
}

/// debugpy's default `console` is `integratedTerminal`, which asks the client
/// for `runInTerminal`; rustode says it cannot, and the program must still run
/// under the adapter's own console.
#[test]
#[ignore = "needs python3 with debugpy; run on its own"]
fn real_debugpy_default_console_works_without_run_in_terminal() {
    if !debugpy_available() {
        eprintln!("skipping: python3 cannot import debugpy");
        return;
    }
    let case = python_case("debugpy-default");
    run_case(&case, &debugpy_adapter(), &debugpy_launch(None), "i == 3");
}

#[test]
#[ignore = "needs python3 with debugpy; run on its own"]
fn real_debugpy_stop_ends_the_adapter_and_the_program() {
    if !debugpy_available() {
        eprintln!("skipping: python3 cannot import debugpy");
        return;
    }
    let case = python_case("debugpy-stop");
    stop_case(
        &case,
        &debugpy_adapter(),
        &debugpy_launch(Some("internalConsole")),
    );
}
