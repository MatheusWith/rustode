//! Workspace seam: a launch configuration from `launch.json` starts a
//! program under a debug adapter, which stops at the breakpoint set on a
//! line and continues; stopping, an adapter that crashes or sends garbage,
//! and the commands that drive it.
//!
//! The adapter is `rustode-test-dap` (`crates/test-dap`): its "program" is a
//! text file, one statement per line, and `CRASH` and `HOSTILE` lines make it
//! exit or misbehave.

use rustode_workspace::{DebugError, DebugState, DocumentId, Event, Workspace};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// The test adapter, built next to this test binary (see `lsp.rs`).
fn adapter() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary");
    if let Some(path) = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("rustode-test-dap"))
        .find(|path| path.is_file())
    {
        return path;
    }
    let profile = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "debug"))
        .expect("a debug profile folder");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "-p", "rustode-test-dap"])
        .arg("--target-dir")
        .arg(profile.parent().expect("target dir"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building rustode-test-dap failed");
    profile.join("rustode-test-dap")
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-dap-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("user")).expect("temp dir");
        std::fs::create_dir_all(dir.join("proj/.vscode")).expect("project");
        Self { dir }
    }

    fn project(&self) -> PathBuf {
        self.dir.join("proj")
    }

    fn program(&self, text: &str) -> PathBuf {
        let path = self.project().join("program.txt");
        std::fs::write(&path, text).expect("program");
        path
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("adapter.log")
    }

    fn launch_json(&self, text: &str) {
        std::fs::write(self.project().join(".vscode/launch.json"), text).expect("launch.json");
    }

    /// A `launch.json` with one configuration for the program.
    fn simple_launch(&self) {
        self.launch_json(
            r#"{
              // comments and trailing commas, as VS Code allows
              "version": "0.2.0",
              "configurations": [
                { "name": "Run program", "type": "test", "request": "launch",
                  "program": "${workspaceFolder}/program.txt", },
              ],
            }"#,
        );
    }

    /// A workspace with the folder open and the adapter configured with
    /// `args`.
    fn workspace(&self, args: &[&str]) -> Workspace {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.push("--log".to_owned());
        all.push(self.log_path().display().to_string());
        let settings = serde_json::json!({
            "rustode.debugAdapters": {
                "test": { "command": adapter().display().to_string(), "args": all }
            }
        });
        std::fs::write(self.dir.join("user/settings.json"), settings.to_string())
            .expect("settings");
        let mut ws = Workspace::new();
        let user = self.dir.join("user");
        ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
        ws.open_folder(&self.project()).expect("folder");
        ws
    }

    /// The requests the adapter received, in order.
    fn commands(&self) -> Vec<String> {
        self.log()
            .iter()
            .filter_map(|m| m["command"].as_str().map(str::to_owned))
            .collect()
    }

    fn log(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Polls the adapter until `done` holds, failing after ten seconds.
#[track_caller]
fn wait(ws: &mut Workspace, what: &str, done: impl Fn(&Workspace) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ws.poll_debug();
        if done(ws) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; status {:?}",
            ws.debug_status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// What the adapter logged for which `found` finds something, waiting for
/// the adapter to get to it.
#[track_caller]
fn eventually<T>(f: &Fixture, what: &str, found: impl Fn(&[serde_json::Value]) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = found(&f.log()) {
            return value;
        }
        assert!(Instant::now() < deadline, "the adapter never logged {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stopped(ws: &Workspace) -> bool {
    ws.debug_status().state == DebugState::Stopped
}

fn ended(ws: &Workspace) -> bool {
    ws.debug_status().state == DebugState::Inactive
}

/// The program, three lines long, opened with a breakpoint on its second.
fn with_breakpoint(f: &Fixture, ws: &mut Workspace) -> DocumentId {
    let path = f.program("print one\nprint two\nprint three\n");
    let id = ws.open(&path).expect("open");
    assert!(ws.toggle_breakpoint(id, 1).expect("toggle"));
    id
}

#[test]
fn launch_json_is_read_in_vs_codes_format_and_problems_are_reported() {
    let f = Fixture::new("launch-json");
    f.launch_json(
        r#"{ "configurations": [
            { "name": "A", "type": "test", "request": "launch" },
            { "name": "no type", "request": "launch" },
            5,
        ] "oops": }"#,
    );
    let ws = f.workspace(&[]);
    let names: Vec<&str> = ws
        .launch_configurations()
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["A"]);
    assert_eq!(ws.launch_configurations()[0].adapter_type, "test");
    let problems = ws.debug_problems();
    assert!(problems.len() >= 2, "{problems:?}");
    assert!(
        problems.iter().any(|p| p.contains("configurations[1]")),
        "{problems:?}"
    );
}

#[test]
fn a_missing_launch_json_is_no_configuration_and_no_problem() {
    let f = Fixture::new("no-launch-json");
    let mut ws = f.workspace(&[]);
    assert_eq!(ws.launch_configurations().len(), 0);
    assert_eq!(ws.debug_problems().len(), 0);
    assert_eq!(ws.start_debugging(), Err(DebugError::NoConfiguration));
}

#[test]
fn adapter_settings_with_problems_are_reported() {
    let f = Fixture::new("adapter-problems");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    std::fs::write(
        f.dir.join("user/settings.json"),
        r#"{"rustode.debugAdapters": {"test": {"args": []}, "other": 3}}"#,
    )
    .expect("settings");
    ws.reload_config();
    assert_eq!(ws.debug_problems().len(), 2);
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::NoAdapter("test".to_owned()))
    );
}

#[test]
fn start_without_a_folder_is_refused() {
    let mut ws = Workspace::new();
    assert_eq!(ws.start_debugging(), Err(DebugError::NoFolder));
    assert_eq!(ws.continue_debugging(), Err(DebugError::NotRunning));
    assert_eq!(ws.stop_debugging(), Err(DebugError::NotRunning));
}

#[test]
fn attach_requests_are_not_supported_yet() {
    let f = Fixture::new("attach");
    f.launch_json(r#"{"configurations": [{"name": "A", "type": "test", "request": "attach"}]}"#);
    let mut ws = f.workspace(&[]);
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::UnsupportedRequest("attach".to_owned()))
    );
}

#[test]
fn breakpoints_toggle_per_document_and_line() {
    let f = Fixture::new("toggle");
    let mut ws = f.workspace(&[]);
    let path = f.program("a\nb\nc\n");
    let id = ws.open(&path).expect("open");
    assert!(ws.toggle_breakpoint(id, 2).expect("set"));
    assert!(ws.toggle_breakpoint(id, 0).expect("set"));
    assert_eq!(ws.breakpoints(id), [0, 2]);
    assert!(!ws.toggle_breakpoint(id, 2).expect("clear"));
    assert_eq!(ws.breakpoints(id), [0]);
    assert!(ws.take_events().contains(&Event::DebugChanged));
}

#[test]
fn it_launches_stops_at_the_breakpoint_and_continues_to_the_end() {
    let f = Fixture::new("run");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let id = with_breakpoint(&f, &mut ws);

    ws.start_debugging().expect("start");
    assert_eq!(ws.debug_status().state, DebugState::Initializing);
    assert_eq!(ws.start_debugging(), Err(DebugError::AlreadyRunning));
    wait(&mut ws, "the stop", |ws| {
        stopped(ws) && ws.debug_status().location.is_some()
    });
    let status = ws.debug_status();
    assert_eq!(status.configuration.as_deref(), Some("Run program"));
    assert_eq!(status.reason.as_deref(), Some("breakpoint"));
    assert_eq!(ws.stopped_line(id), Some(1));
    assert!(ws.take_events().contains(&Event::DebugChanged));

    // The order VS Code uses, with the folder substituted into the program.
    let commands = f.commands();
    let position = |name: &str| commands.iter().position(|c| c == name).expect(name);
    assert!(position("initialize") < position("launch"));
    assert!(position("launch") < position("setBreakpoints"));
    assert!(position("setBreakpoints") < position("configurationDone"));
    let log = f.log();
    let launch = log
        .iter()
        .find(|m| m["command"] == "launch")
        .expect("launch");
    let program = f.project().join("program.txt").display().to_string();
    assert_eq!(launch["arguments"]["program"], program);
    let breakpoints = log
        .iter()
        .find(|m| m["command"] == "setBreakpoints")
        .expect("breakpoints");
    assert_eq!(breakpoints["arguments"]["breakpoints"][0]["line"], 2);

    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    let status = ws.debug_status();
    assert_eq!(
        status.message.as_deref(),
        Some("the program exited with code 0")
    );
    assert_eq!(ws.stopped_line(id), None);
    assert_eq!(ws.continue_debugging(), Err(DebugError::NotRunning));
}

#[test]
fn it_runs_again_after_the_end_and_continue_needs_a_stop() {
    let f = Fixture::new("again");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    f.program("print only\n");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", ended);
    ws.start_debugging().expect("start again");
    assert_eq!(ws.continue_debugging(), Err(DebugError::NotStopped));
    wait(&mut ws, "the end", |ws| {
        ended(ws) && ws.debug_status().message.is_some()
    });
}

#[test]
fn a_breakpoint_set_while_stopped_is_sent_at_once() {
    let f = Fixture::new("late-breakpoint");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let id = with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    assert!(ws.toggle_breakpoint(id, 2).expect("toggle"));
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the second stop", |ws| {
        stopped(ws) && ws.debug_status().location.is_some_and(|l| l.line == 2)
    });
    let sent = f
        .commands()
        .iter()
        .filter(|c| *c == "setBreakpoints")
        .count();
    assert_eq!(sent, 2);
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn stop_asks_the_adapter_to_terminate_or_disconnects() {
    for (args, expected) in [
        (&[][..], "terminate"),
        (&["--no-terminate"][..], "disconnect"),
    ] {
        let f = Fixture::new(&format!("stop-{expected}"));
        f.simple_launch();
        let mut ws = f.workspace(args);
        with_breakpoint(&f, &mut ws);
        ws.start_debugging().expect("start");
        wait(&mut ws, "the stop", stopped);
        ws.stop_debugging().expect("stop");
        wait(&mut ws, "the end", ended);
        assert_eq!(ws.debug_status().configuration, None);
        eventually(&f, expected, |log| {
            log.iter().find(|m| m["command"] == expected).map(|_| ())
        });
        assert_eq!(ws.stop_debugging(), Err(DebugError::NotRunning));
    }
}

#[test]
fn an_adapter_without_configuration_done_still_runs() {
    let f = Fixture::new("no-config-done");
    f.simple_launch();
    let mut ws = f.workspace(&["--no-config-done"]);
    f.program("print one\n");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", |ws| {
        ended(ws) && ws.debug_status().message.is_some()
    });
    assert!(!f.commands().iter().any(|c| c == "configurationDone"));
}

#[test]
fn a_launch_the_adapter_refuses_ends_the_session_with_its_reason() {
    let f = Fixture::new("fail-launch");
    f.simple_launch();
    let mut ws = f.workspace(&["--fail-launch"]);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", ended);
    let message = ws.debug_status().message.expect("a message");
    assert!(message.contains("cannot launch the program"), "{message}");
}

#[test]
fn a_crashing_adapter_is_reported_and_the_workspace_goes_on() {
    let f = Fixture::new("crash");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    f.program("print before\nCRASH\nprint after\n");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the crash", ended);
    let message = ws.debug_status().message.expect("a message");
    assert!(message.contains("exited with code 3"), "{message}");
    assert!(ws.take_events().contains(&Event::DebugChanged));
    // The workspace still works: a new session starts and ends.
    f.program("print fine\n");
    ws.start_debugging().expect("start again");
    wait(&mut ws, "the end", |ws| {
        ended(ws) && ws.debug_status().message.is_some()
    });
    assert_eq!(
        ws.debug_status().message.as_deref(),
        Some("the program exited with code 0")
    );
}

#[test]
fn an_adapter_that_sends_garbage_does_not_derail_the_session() {
    let f = Fixture::new("hostile");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    f.program("print before\nHOSTILE\nprint after\n");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", |ws| {
        ended(ws) && ws.debug_status().message.is_some()
    });
    assert_eq!(
        ws.debug_status().message.as_deref(),
        Some("the program exited with code 0")
    );
    // The adapter's own request was answered, politely and negatively.
    let refusal = eventually(&f, "the refusal", |log| {
        log.iter()
            .find(|m| m["type"] == "response" && m["command"] == "runInTerminal")
            .cloned()
    });
    assert_eq!(refusal["success"], false);
}

#[test]
fn the_debug_commands_run_by_id_and_f5_means_what_the_state_says() {
    let f = Fixture::new("commands");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let path = f.program("print one\nprint two\nprint three\n");
    let id = ws.open(&path).expect("open");
    ws.set_selections(id, vec![rustode_workspace::Selection::new(10, 10)], 0)
        .expect("cursor on the second line");
    assert!(
        ws.execute("editor.debug.action.toggleBreakpoint")
            .expect("runs")
    );
    assert_eq!(ws.breakpoints(id), [1]);

    assert_eq!(ws.debug_key_command(), "workbench.action.debug.start");
    assert!(ws.execute("workbench.action.debug.start").expect("runs"));
    wait(&mut ws, "the stop", stopped);
    assert_eq!(ws.debug_key_command(), "workbench.action.debug.continue");
    assert!(ws.execute("workbench.action.debug.continue").expect("runs"));
    wait(&mut ws, "the end", ended);

    assert!(ws.execute("workbench.action.debug.start").expect("runs"));
    assert!(ws.execute("workbench.action.debug.stop").expect("runs"));
    wait(&mut ws, "the end", ended);
    assert!(
        ws.execute("workbench.action.debug.continue").is_err(),
        "continue with no session is an error, not a crash"
    );
    for (id, key) in [
        ("workbench.action.debug.start", "F5"),
        ("workbench.action.debug.continue", "F5"),
        ("workbench.action.debug.stop", "Shift+F5"),
        ("editor.debug.action.toggleBreakpoint", "F9"),
    ] {
        assert_eq!(ws.keybinding_label(id).as_deref(), Some(key), "{id}");
    }
}

#[test]
fn dropping_the_workspace_ends_the_adapter() {
    let f = Fixture::new("drop");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    drop(ws);
    assert!(
        f.commands().iter().any(|c| c == "disconnect"),
        "the drop waits for the adapter"
    );
}
