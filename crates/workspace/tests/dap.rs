//! Workspace seam: a launch configuration from `launch.json` starts a
//! program under a debug adapter, which stops at the breakpoint set on a
//! line and continues; stopping, an adapter that crashes or sends garbage,
//! and the commands that drive it.
//!
//! The adapter is `rustode-test-dap` (`crates/test-dap`): its "program" is a
//! text file, one statement per line, and `CRASH` and `HOSTILE` lines make it
//! exit or misbehave.

use rustode_workspace::{
    ConsoleKind, DebugError, DebugState, DebugTree, DocumentId, Event, StepKind, VariableRow,
    Workspace,
};
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
fn requests_other_than_launch_and_attach_are_refused() {
    let f = Fixture::new("other-request");
    f.launch_json(r#"{"configurations": [{"name": "A", "type": "test", "request": "restart"}]}"#);
    let mut ws = f.workspace(&[]);
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::UnsupportedRequest("restart".to_owned()))
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

// ---- ticket 38: stepping, call stack, variables, watch and console ----

/// A program with two nested calls, objects that nest and a global.
const NESTED: &str = "global g 9\nvar a 1\nobj p k=v\nenter outer\nvar b 2\n\
obj o x=1 inner=@p\nenter inner\nprint deep\nleave\nleave\nprint done\n";

/// Starts `text` with a breakpoint on the zero-based `line` and waits for
/// the stop.
fn stopped_at(f: &Fixture, text: &str, line: usize, args: &[&str]) -> (Workspace, DocumentId) {
    f.simple_launch();
    let mut ws = f.workspace(args);
    let id = ws.open(&f.program(text)).expect("open");
    assert!(ws.toggle_breakpoint(id, line).expect("toggle"));
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop and its views", |ws| {
        let rows = ws.debug_rows(DebugTree::Variables);
        stopped(ws) && !rows.is_empty() && rows.iter().all(|r| !r.loading)
    });
    (ws, id)
}

fn names(rows: &[VariableRow]) -> Vec<String> {
    rows.iter()
        .map(|r| format!("{}{}", " ".repeat(r.depth), r.name))
        .collect()
}

fn values(ws: &Workspace, tree: DebugTree) -> Vec<(String, String)> {
    ws.debug_rows(tree)
        .into_iter()
        .map(|r| (r.name, r.value))
        .collect()
}

/// Waits for the program to stop on the zero-based `line`.
#[track_caller]
fn wait_line(ws: &mut Workspace, id: DocumentId, line: usize) {
    wait(ws, "the stop on that line", |ws| {
        stopped(ws) && ws.stopped_line(id) == Some(line)
    });
}

#[test]
fn a_stop_shows_the_threads_the_call_stack_and_the_first_scope_open() {
    let f = Fixture::new("stack");
    let (mut ws, id) = stopped_at(&f, NESTED, 3, &[]);
    wait(&mut ws, "the threads", |ws| !ws.debug_threads().is_empty());
    let threads: Vec<String> = ws.debug_threads().into_iter().map(|t| t.name).collect();
    assert_eq!(threads, ["main", "worker"]);
    let frames = ws.debug_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].name.as_str(), frames[0].line), ("main", 3));
    assert_eq!(ws.stopped_line(id), Some(3));
    // The first scope that is cheap to read opens by itself; Globals is
    // marked expensive and stays closed.
    let rows = ws.debug_rows(DebugTree::Variables);
    assert_eq!(names(&rows), ["Locals", " a", " p", "Globals"]);
    assert!(rows[0].expanded && !rows[3].expanded && rows[3].expandable);
    assert_eq!(rows[1].value, "1");
    assert_eq!(rows[1].kind.as_deref(), Some("int"));
    assert!(rows[2].expandable && rows[2].value == "Object");
}

#[test]
fn step_into_over_and_out_walk_the_program_and_the_stack() {
    let f = Fixture::new("steps");
    let (mut ws, id) = stopped_at(&f, NESTED, 3, &[]);

    ws.step_debugging(StepKind::Into).expect("into");
    assert_eq!(
        ws.step_debugging(StepKind::Over),
        Err(DebugError::NotStopped),
        "a step already on its way"
    );
    wait_line(&mut ws, id, 4);
    assert_eq!(ws.debug_status().reason.as_deref(), Some("step"));
    wait(&mut ws, "two frames", |ws| ws.debug_frames().len() == 2);
    let frames = ws.debug_frames();
    let stack: Vec<(&str, usize)> = frames.iter().map(|f| (f.name.as_str(), f.line)).collect();
    assert_eq!(stack, [("outer", 4), ("main", 3)]);

    ws.execute("workbench.action.debug.stepOver").expect("over");
    wait_line(&mut ws, id, 5);
    ws.execute("workbench.action.debug.stepOver").expect("over");
    wait_line(&mut ws, id, 6);
    // Over the `enter inner` .. `leave` pair: back in `outer`, past it.
    ws.execute("workbench.action.debug.stepOver").expect("over");
    wait_line(&mut ws, id, 9);
    wait(&mut ws, "one frame less", |ws| ws.debug_frames().len() == 2);
    ws.execute("workbench.action.debug.stepOut").expect("out");
    wait_line(&mut ws, id, 10);
    wait(&mut ws, "main only", |ws| ws.debug_frames().len() == 1);
    assert!(
        ws.execute("workbench.action.debug.stepOut").expect("runs"),
        "step out runs to the end when there is no caller"
    );
    wait(&mut ws, "the end", ended);
    let commands = f.commands();
    for wanted in ["next", "stepIn", "stepOut"] {
        assert!(commands.iter().any(|c| c == wanted), "{wanted}");
    }
    assert_eq!(
        ws.step_debugging(StepKind::Over),
        Err(DebugError::NotRunning)
    );
}

#[test]
fn variables_expand_lazily_through_their_references() {
    let f = Fixture::new("variables");
    let (mut ws, id) = stopped_at(&f, NESTED, 6, &[]);
    let rows = ws.debug_rows(DebugTree::Variables);
    assert_eq!(names(&rows), ["Locals", " b", " o", "Globals"]);
    let requests = |f: &Fixture| f.commands().iter().filter(|c| *c == "variables").count();
    assert_eq!(requests(&f), 1, "only the first scope was read");

    ws.toggle_debug_row(DebugTree::Variables, 2).expect("o");
    assert!(
        ws.debug_rows(DebugTree::Variables)[2].loading,
        "asked for, not here yet"
    );
    wait(&mut ws, "o's members", |ws| {
        names(&ws.debug_rows(DebugTree::Variables)).len() == 6
    });
    let rows = ws.debug_rows(DebugTree::Variables);
    assert_eq!(
        names(&rows),
        ["Locals", " b", " o", "  x", "  inner", "Globals"]
    );
    assert!(rows[4].expandable, "inner is another object");
    ws.toggle_debug_row(DebugTree::Variables, 4).expect("inner");
    wait(&mut ws, "inner's members", |ws| {
        ws.debug_rows(DebugTree::Variables).len() == 7
    });
    assert_eq!(
        values(&ws, DebugTree::Variables)[5],
        ("k".to_owned(), "v".to_owned())
    );

    // Collapsing and expanding again does not ask the adapter again.
    let before = requests(&f);
    ws.toggle_debug_row(DebugTree::Variables, 2).expect("close");
    assert_eq!(ws.debug_rows(DebugTree::Variables).len(), 4);
    ws.toggle_debug_row(DebugTree::Variables, 2).expect("open");
    assert_eq!(ws.debug_rows(DebugTree::Variables).len(), 7);
    assert_eq!(requests(&f), before);

    // The expensive scope is read when asked.
    ws.toggle_debug_row(DebugTree::Variables, 6)
        .expect("globals");
    wait(&mut ws, "the globals", |ws| {
        ws.debug_rows(DebugTree::Variables).len() == 8
    });
    assert_eq!(
        values(&ws, DebugTree::Variables)[7],
        ("g".to_owned(), "9".to_owned())
    );
    assert_eq!(
        ws.toggle_debug_row(DebugTree::Variables, 99),
        Err(DebugError::NoSuchRow)
    );
    let _ = id;
}

#[test]
fn selecting_a_frame_moves_the_editor_and_reads_its_scopes() {
    let f = Fixture::new("select-frame");
    let (mut ws, id) = stopped_at(&f, NESTED, 5, &[]);
    wait(&mut ws, "two frames", |ws| ws.debug_frames().len() == 2);
    let rows = ws.debug_rows(DebugTree::Variables);
    assert_eq!(names(&rows)[..3], ["Locals", " b", "Globals"]);

    // Move the cursor away and select the caller: the editor goes to its line.
    ws.go_to_line(id, 11, None).expect("cursor");
    ws.select_debug_frame(1).expect("main");
    assert_eq!(ws.debug_selected_frame(), 1);
    assert_eq!(ws.stopped_line(id), Some(3));
    let line = ws.document(id).expect("doc").cursor_position().line;
    assert_eq!(line, 3, "the editor follows the frame");
    wait(&mut ws, "main's variables", |ws| {
        names(&ws.debug_rows(DebugTree::Variables)).contains(&" a".to_owned())
    });
    let scopes: Vec<i64> = f
        .log()
        .iter()
        .filter(|m| m["command"] == "scopes")
        .filter_map(|m| m["arguments"]["frameId"].as_i64())
        .collect();
    assert_eq!(scopes, [2, 1], "the stop's frame, then the selected one");
    assert_eq!(ws.select_debug_frame(5), Err(DebugError::NoSuchRow));
}

#[test]
fn watch_expressions_are_evaluated_per_frame_and_keep_across_stops() {
    let f = Fixture::new("watch");
    let (mut ws, id) = stopped_at(&f, NESTED, 6, &[]);
    ws.add_watch("b").expect("b");
    ws.add_watch("nope").expect("nope");
    ws.add_watch("o").expect("o");
    assert_eq!(
        ws.add_watch("   "),
        Err(DebugError::BadWatch(
            "a watch expression cannot be empty".to_owned()
        ))
    );
    wait(&mut ws, "the results", |ws| {
        ws.debug_rows(DebugTree::Watch)
            .iter()
            .all(|r| r.value != "not available")
    });
    let rows = values(&ws, DebugTree::Watch);
    assert_eq!(rows[0], ("b".to_owned(), "2".to_owned()));
    assert_eq!(
        rows[1],
        ("nope".to_owned(), "name 'nope' is not defined".to_owned()),
        "a failed evaluation shows the adapter's message"
    );
    assert_eq!(rows[2], ("o".to_owned(), "Object".to_owned()));
    assert!(
        f.log()
            .iter()
            .any(|m| m["command"] == "evaluate" && m["arguments"]["context"] == "watch")
    );

    // A structured result expands like a variable.
    ws.toggle_debug_row(DebugTree::Watch, 2).expect("o");
    wait(&mut ws, "o's members", |ws| {
        ws.debug_rows(DebugTree::Watch).len() == 5
    });

    // Running: nothing is available. Stopping again evaluates them again.
    ws.step_debugging(StepKind::Over).expect("over");
    wait(&mut ws, "the next stop", |ws| {
        stopped(ws) && ws.stopped_line(id) == Some(9)
    });
    wait(&mut ws, "evaluated again", |ws| {
        values(ws, DebugTree::Watch).first().map(|r| r.1.as_str()) == Some("2")
    });
    assert_eq!(ws.debug_rows(DebugTree::Watch).len(), 3, "collapsed again");

    assert!(ws.remove_watch(1));
    assert!(!ws.remove_watch(9));
    let left: Vec<String> = ws
        .debug_rows(DebugTree::Watch)
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(left, ["b", "o"]);
    ws.execute("workbench.debug.viewlet.action.removeAllWatchExpressions")
        .expect("runs");
    assert_eq!(ws.debug_rows(DebugTree::Watch).len(), 0);
}

#[test]
fn a_watch_with_no_session_says_not_available_and_survives_the_session() {
    let f = Fixture::new("watch-idle");
    let (mut ws, _) = stopped_at(&f, NESTED, 6, &[]);
    ws.add_watch("b").expect("b");
    wait(&mut ws, "the value", |ws| {
        values(ws, DebugTree::Watch)[0].1 == "2"
    });
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
    assert_eq!(
        values(&ws, DebugTree::Watch),
        [("b".to_owned(), "not available".to_owned())]
    );
    assert_eq!(ws.debug_rows(DebugTree::Variables).len(), 0);
    assert_eq!(ws.debug_frames().len(), 0);
}

#[test]
fn the_console_shows_output_and_evaluates_expressions() {
    let f = Fixture::new("console");
    let text = "print out\nerror bad\nconsole note\nvar a 5\nvar z 1\nprint end\n";
    let (mut ws, _) = stopped_at(&f, text, 5, &[]);
    let lines: Vec<(ConsoleKind, String)> = ws
        .debug_console()
        .iter()
        .map(|l| (l.kind, l.text.clone()))
        .collect();
    assert_eq!(
        lines,
        [
            (ConsoleKind::Output, "out".to_owned()),
            (ConsoleKind::Error, "bad".to_owned()),
            (ConsoleKind::Output, "note".to_owned()),
        ]
    );
    ws.evaluate_in_console("a").expect("a");
    ws.evaluate_in_console("missing").expect("missing");
    ws.evaluate_in_console("!cancel").expect("cancel");
    ws.evaluate_in_console("   ").expect("blank is nothing");
    wait(&mut ws, "all answers", |ws| {
        ws.debug_console().len() >= 3 + 6
    });
    let tail: Vec<(ConsoleKind, String)> = ws
        .debug_console()
        .iter()
        .skip(3)
        .map(|l| (l.kind, l.text.clone()))
        .collect();
    assert_eq!(
        tail,
        [
            (ConsoleKind::Input, "a".to_owned()),
            (ConsoleKind::Input, "missing".to_owned()),
            (ConsoleKind::Input, "!cancel".to_owned()),
            (ConsoleKind::Result, "5".to_owned()),
            (
                ConsoleKind::Error,
                "name 'missing' is not defined".to_owned()
            ),
            (ConsoleKind::Error, "cancelled".to_owned()),
        ]
    );
    let evaluate = f
        .log()
        .into_iter()
        .find(|m| m["command"] == "evaluate")
        .expect("evaluate");
    assert_eq!(evaluate["arguments"]["context"], "repl");

    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    assert!(ws.debug_console().iter().any(|l| l.text == "end"));
    assert_eq!(ws.evaluate_in_console("a"), Err(DebugError::NotRunning));
}

#[test]
fn a_reply_that_never_comes_is_bounded_and_does_not_block() {
    let f = Fixture::new("hang");
    let (mut ws, _) = stopped_at(&f, NESTED, 3, &[]);
    let mut refused = false;
    for _ in 0..400 {
        if ws.evaluate_in_console("!hang").is_err() {
            refused = true;
            break;
        }
    }
    assert!(refused, "the table of waiting requests has a bound");
    assert_eq!(ws.evaluate_in_console("a"), Err(DebugError::Busy));
    // The session still answers what does not wait on the hung requests.
    assert!(ws.continue_debugging().is_ok());
    wait(&mut ws, "the end", ended);
}

#[test]
fn what_arrives_after_execution_moved_on_is_dropped() {
    let f = Fixture::new("stale");
    let (mut ws, id) = stopped_at(&f, NESTED, 6, &[]);
    // Ask for things, then continue before reading any answer: the answers
    // arrive after the program ran on and must not show up.
    ws.add_watch("b").expect("b");
    ws.toggle_debug_row(DebugTree::Variables, 2).expect("o");
    ws.step_debugging(StepKind::Over).expect("over");
    wait(&mut ws, "the next stop", |ws| {
        stopped(ws) && ws.stopped_line(id) == Some(9)
    });
    // The watch got its value from the new stop's evaluation, and the
    // variables tree shows the new stop's scopes, collapsed at `o`.
    wait(&mut ws, "the new views", |ws| {
        values(ws, DebugTree::Watch)[0].1 == "2" && ws.debug_rows(DebugTree::Variables).len() >= 4
    });
    let rows = ws.debug_rows(DebugTree::Variables);
    assert!(rows.iter().all(|r| !r.loading));
    assert_eq!(
        names(&rows).iter().filter(|n| *n == "  x").count(),
        0,
        "the children of the old stop's `o` are not in the new tree"
    );

    // While running, nothing of the old stop is left.
    ws.continue_debugging().expect("continue");
    ws.add_watch("never").expect("watch while running");
    wait(&mut ws, "the end", ended);
    assert_eq!(ws.debug_rows(DebugTree::Watch).len(), 2);
}

#[test]
fn pause_stops_a_running_program_and_the_keys_follow_the_state() {
    let f = Fixture::new("pause");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let id = ws
        .open(&f.program("print a\nWAIT\nprint b\n"))
        .expect("open");
    // No session: the step and pause keys mean nothing, F6 is not "pause".
    for command in ["pause", "stepOver", "stepInto", "stepOut"] {
        assert!(
            !ws.command_applies(&format!("workbench.action.debug.{command}")),
            "{command}"
        );
    }
    assert!(matches!(
        ws.execute("workbench.action.debug.pause"),
        Err(rustode_workspace::Error::Debug(DebugError::NotRunning))
    ));
    ws.start_debugging().expect("start");
    wait(&mut ws, "running", |ws| {
        ws.debug_status().state == DebugState::Running
    });
    assert!(
        ws.command_applies("workbench.action.debug.pause"),
        "running: F6 pauses"
    );
    assert!(ws.command_applies("workbench.action.debug.stepInto"));
    assert!(!ws.command_applies("workbench.action.debug.stepOver"));
    assert_eq!(
        ws.step_debugging(StepKind::Over),
        Err(DebugError::NotStopped)
    );

    ws.execute("workbench.action.debug.pause").expect("pause");
    wait(&mut ws, "the pause", stopped);
    assert_eq!(ws.debug_status().reason.as_deref(), Some("pause"));
    wait_line(&mut ws, id, 1);
    assert!(
        !ws.command_applies("workbench.action.debug.pause"),
        "stopped: F6 moves the focus"
    );
    assert!(ws.command_applies("workbench.action.debug.stepOver"));
    assert!(
        !ws.command_applies("workbench.action.debug.stepOut"),
        "one frame only"
    );
    assert_eq!(ws.pause_debugging(), Err(DebugError::NotRunningProgram));
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
}

#[test]
fn step_and_view_commands_have_vs_codes_ids_and_keys() {
    let ws = Workspace::new();
    for (id, key) in [
        ("workbench.action.debug.stepOver", "F10"),
        ("workbench.action.debug.stepInto", "F11"),
        ("workbench.action.debug.stepOut", "Shift+F11"),
        ("workbench.action.debug.pause", "F6"),
    ] {
        assert_eq!(ws.keybinding_label(id).as_deref(), Some(key), "{id}");
    }
    for id in [
        "workbench.debug.viewlet.action.addWatchExpression",
        "debug.removeWatchExpression",
        "workbench.debug.viewlet.action.removeAllWatchExpressions",
        "workbench.debug.action.focusCallStackView",
        "workbench.debug.action.focusVariablesView",
        "workbench.debug.action.focusWatchView",
        "workbench.debug.action.focusRepl",
    ] {
        assert!(
            rustode_workspace::COMMANDS.iter().any(|c| c.id == id),
            "{id}"
        );
    }
}

#[test]
fn hostile_replies_are_bounded_and_do_not_derail_the_session() {
    let f = Fixture::new("hostile-replies");
    f.simple_launch();
    let mut ws = f.workspace(&["--hostile-replies"]);
    let id = ws.open(&f.program(NESTED)).expect("open");
    ws.toggle_breakpoint(id, 3).expect("toggle");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    wait(&mut ws, "the garbage", |ws| {
        !ws.debug_frames().is_empty() && !ws.debug_rows(DebugTree::Variables).is_empty()
    });
    assert!(ws.debug_frames().len() <= 100);
    assert!(
        ws.debug_threads().is_empty(),
        "a threads list that is a string is nothing"
    );
    // Expand the scopes as far as the adapter's cycle goes: bounded depth
    // and bounded count, never a hang or a panic.
    for _ in 0..40 {
        let rows = ws.debug_rows(DebugTree::Variables);
        let Some(index) = rows
            .iter()
            .position(|r| r.expandable && !r.expanded && r.depth > 0)
        else {
            break;
        };
        if ws.toggle_debug_row(DebugTree::Variables, index).is_err() {
            break;
        }
        wait(&mut ws, "an answer", |ws| {
            ws.debug_rows(DebugTree::Variables)
                .iter()
                .all(|r| !r.loading)
        });
    }
    let rows = ws.debug_rows(DebugTree::Variables);
    assert!(rows.len() <= 2_000, "{} rows", rows.len());
    assert!(
        rows.iter()
            .all(|r| r.depth <= 9 && r.value.chars().count() <= 500)
    );
    ws.add_watch("x").expect("watch");
    ws.evaluate_in_console("x").expect("console");
    wait(&mut ws, "the console", |ws| !ws.debug_console().is_empty());
    assert!(ws.continue_debugging().is_ok());
    wait(&mut ws, "the end", ended);
}

#[test]
fn a_cycle_of_objects_expands_only_to_the_depth_limit() {
    let f = Fixture::new("cycle");
    let (mut ws, _) = stopped_at(&f, "obj c self=@c\nvar z 1\n", 1, &[]);
    for _ in 0..20 {
        let rows = ws.debug_rows(DebugTree::Variables);
        let Some(index) = rows.iter().rposition(|r| r.name == "self" || r.name == "c") else {
            break;
        };
        if !rows[index].expandable || rows[index].expanded {
            break;
        }
        ws.toggle_debug_row(DebugTree::Variables, index)
            .expect("open");
        wait(&mut ws, "the answer", |ws| {
            ws.debug_rows(DebugTree::Variables)
                .iter()
                .all(|r| !r.loading)
        });
    }
    let rows = ws.debug_rows(DebugTree::Variables);
    let deepest = rows.iter().map(|r| r.depth).max().unwrap_or(0);
    assert_eq!(deepest, 8, "the arrow stops at depth 8");
    assert!(rows.iter().filter(|r| r.depth == 8).all(|r| !r.expandable));
}

// ---- the owner's decision on `rustode.debugAdapters`: freely editable, live ----

impl Fixture {
    /// Rewrites `settings.json` and reads it again, as an edit and a save do.
    fn edit_settings(&self, ws: &mut Workspace, settings: &serde_json::Value) {
        std::fs::write(self.dir.join("user/settings.json"), settings.to_string())
            .expect("settings");
        ws.reload_config();
    }

    /// An entry for the test adapter that logs to `name`.
    fn entry(&self, name: &str) -> serde_json::Value {
        serde_json::json!({
            "command": adapter().display().to_string(),
            "args": ["--log", self.dir.join(name).display().to_string()],
        })
    }
}

fn adapter_ran(f: &Fixture, log: &str) -> bool {
    std::fs::read_to_string(f.dir.join(log)).is_ok_and(|text| text.contains("initialize"))
}

#[test]
fn a_changed_adapter_setting_applies_at_the_next_start_without_a_restart() {
    let f = Fixture::new("adapter-live-idle");
    f.simple_launch();
    f.program("print one\n");
    let mut ws = f.workspace(&[]);
    ws.reload_launch_configurations();

    // Another command line, applied live while idle.
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": f.entry("second.log")}}),
    );
    assert_eq!(ws.debug_problems().len(), 0);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", ended);
    assert!(adapter_ran(&f, "second.log"), "the new args were used");

    // Env too: only a changed environment makes the wrapper log here.
    let wrapper = serde_json::json!({
        "command": "sh",
        "args": ["-c", "exec \"$0\" --log \"$ADAPTER_LOG\"", adapter().display().to_string()],
        "env": {"ADAPTER_LOG": f.dir.join("env.log").display().to_string()},
    });
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": wrapper}}),
    );
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", ended);
    assert!(adapter_ran(&f, "env.log"), "the env was applied");

    // An entry that goes away leaves the type without an adapter; `null`
    // does the same; adding it back works again.
    f.edit_settings(&mut ws, &serde_json::json!({}));
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::NoAdapter("test".to_owned()))
    );
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": null}}),
    );
    assert_eq!(ws.debug_problems().len(), 0, "null is not a problem");
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::NoAdapter("test".to_owned()))
    );
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": f.entry("back.log")}}),
    );
    ws.start_debugging().expect("start");
    wait(&mut ws, "the end", ended);
    assert!(adapter_ran(&f, "back.log"));
}

#[test]
fn a_session_keeps_its_adapter_when_the_setting_changes() {
    let f = Fixture::new("adapter-live-running");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);

    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": {"command": "/no/such/adapter"}}}),
    );
    assert_eq!(
        ws.debug_status().state,
        DebugState::Stopped,
        "the session goes on"
    );
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    assert_eq!(
        ws.debug_status().message.as_deref(),
        Some("the program exited with code 0")
    );

    // The next start uses the new value: a command that cannot run is an
    // error to read, not a crash.
    assert!(matches!(ws.start_debugging(), Err(DebugError::Spawn(_))));

    // Removing the entry while a session runs does not end it either.
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": f.entry("again.log")}}),
    );
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {"test": null}}),
    );
    assert_eq!(ws.debug_status().state, DebugState::Stopped);
    ws.step_debugging(StepKind::Over).expect("step");
    wait(&mut ws, "the next stop", |ws| {
        stopped(ws) && ws.debug_status().reason.as_deref() == Some("step")
    });
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn invalid_adapter_entries_are_reported_and_the_valid_ones_still_work() {
    let f = Fixture::new("adapter-invalid");
    f.simple_launch();
    f.program("print one\n");
    let mut ws = f.workspace(&[]);
    let mut good = f.entry("good.log");
    good["env"] = serde_json::json!({"OK": "1", "NUM": 3});
    f.edit_settings(
        &mut ws,
        &serde_json::json!({"rustode.debugAdapters": {
            "test": good,
            "nocommand": {},
            "number": 7,
            "badenv": {"command": "x", "env": [1]},
            "badargs": {"command": "x", "args": "no"},
        }}),
    );
    let problems = ws.debug_problems();
    assert!(
        problems.iter().any(|p| p.contains("\"nocommand\"")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("\"number\"")),
        "{problems:?}"
    );
    assert!(problems.iter().any(|p| p.contains("env")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("NUM")), "{problems:?}");
    assert_eq!(problems.len(), 5, "{problems:?}");
    ws.start_debugging().expect("the valid entry works");
    wait(&mut ws, "the end", ended);
    assert!(adapter_ran(&f, "good.log"));

    // The whole setting of the wrong type is ignored and then reported
    // by the settings schema (an object is expected).
    f.edit_settings(&mut ws, &serde_json::json!({"rustode.debugAdapters": 5}));
    assert_eq!(
        ws.start_debugging(),
        Err(DebugError::NoAdapter("test".to_owned()))
    );
    assert!(ws.config().diagnostics().count() > 0);
}

// ---- ticket 39: conditional breakpoints, hit counts, logpoints, real adapters ----

use rustode_workspace::{BreakpointInput, BreakpointKind, BreakpointSpec};

/// A loop that runs its body five times; `i` is 1 on the first.
const LOOP: &str = "loop 5\ninc i\nprint tick\nend\nprint done\n";

fn spec(condition: &str, hit: &str, log: &str) -> BreakpointSpec {
    let some = |t: &str| (!t.is_empty()).then(|| t.to_owned());
    BreakpointSpec {
        condition: some(condition),
        hit_condition: some(hit),
        log_message: some(log),
    }
}

/// Starts the loop program with `spec` on its `print tick` line (index 2) and
/// waits for the first stop, if there is one.
fn loop_with(f: &Fixture, args: &[&str], spec: BreakpointSpec) -> (Workspace, DocumentId) {
    f.simple_launch();
    let mut ws = f.workspace(args);
    let id = ws.open(&f.program(LOOP)).expect("open");
    ws.set_breakpoint(id, 2, spec).expect("set");
    ws.start_debugging().expect("start");
    (ws, id)
}

/// The value of the local `name` in the stopped frame.
fn local(ws: &Workspace, name: &str) -> Option<String> {
    ws.debug_rows(DebugTree::Variables)
        .into_iter()
        .find(|r| r.name.trim() == name)
        .map(|r| r.value)
}

fn wait_local(ws: &mut Workspace, name: &str, value: &str) {
    wait(ws, "the local", |ws| {
        stopped(ws) && local(ws, name).as_deref() == Some(value)
    });
}

#[test]
fn a_conditional_breakpoint_stops_only_when_its_condition_holds() {
    let f = Fixture::new("condition");
    let (mut ws, id) = loop_with(&f, &[], spec("i == 3", "", ""));
    wait_local(&mut ws, "i", "3");
    assert_eq!(ws.stopped_line(id), Some(2));
    assert_eq!(ws.debug_status().reason.as_deref(), Some("breakpoint"));
    let sent = f
        .log()
        .into_iter()
        .find(|m| m["command"] == "setBreakpoints")
        .expect("breakpoints");
    assert_eq!(sent["arguments"]["breakpoints"][0]["condition"], "i == 3");
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    assert_eq!(
        ws.debug_status().message.as_deref(),
        Some("the program exited with code 0"),
        "it did not stop again"
    );
}

#[test]
fn a_hit_count_breakpoint_stops_on_the_nth_hit_and_after() {
    let f = Fixture::new("hit-count");
    let (mut ws, _id) = loop_with(&f, &[], spec("", "> 3", ""));
    wait_local(&mut ws, "i", "4");
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the next stop", |ws| {
        stopped(ws) && local(ws, "i").as_deref() == Some("5")
    });
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    let sent = f
        .log()
        .into_iter()
        .find(|m| m["command"] == "setBreakpoints")
        .expect("breakpoints");
    assert_eq!(sent["arguments"]["breakpoints"][0]["hitCondition"], "> 3");
}

#[test]
fn a_condition_and_a_hit_count_count_only_the_hits_where_the_condition_holds() {
    let f = Fixture::new("both");
    // Hits where i > 1 are i = 2, 3, 4, 5; the second of them is i = 3.
    let (mut ws, _id) = loop_with(&f, &[], spec("i > 1", "2", ""));
    wait_local(&mut ws, "i", "3");
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn a_logpoint_prints_its_message_and_never_stops() {
    let f = Fixture::new("logpoint");
    let (mut ws, id) = loop_with(&f, &[], spec("", "", "tick {i}"));
    wait(&mut ws, "the end", ended);
    let console: Vec<String> = ws
        .debug_console()
        .iter()
        .filter(|l| l.text.starts_with("tick "))
        .map(|l| l.text.clone())
        .collect();
    assert_eq!(console, ["tick 1", "tick 2", "tick 3", "tick 4", "tick 5"]);
    assert_eq!(ws.stopped_line(id), None);
    assert_eq!(
        ws.breakpoint_at(id, 2).expect("kept").kind,
        BreakpointKind::LogPoint
    );
}

#[test]
fn an_adapter_without_the_capability_gets_a_plain_breakpoint_and_it_shows_unverified() {
    let f = Fixture::new("unsupported");
    f.simple_launch();
    let mut ws = f.workspace(&["--no-conditions", "--no-hit-conditions", "--no-logpoints"]);
    let id = ws.open(&f.program(LOOP)).expect("open");
    ws.set_breakpoint(id, 2, spec("i == 3", "", ""))
        .expect("condition");
    ws.set_breakpoint(id, 1, spec("", "2", ""))
        .expect("hit count");
    ws.set_breakpoint(id, 0, spec("", "", "hello"))
        .expect("log");
    // Before a session every breakpoint looks fine.
    assert!(ws.breakpoint_details(id).iter().all(|b| b.verified));
    ws.start_debugging().expect("start");
    wait(&mut ws, "the first stop", stopped);
    let details = ws.breakpoint_details(id);
    assert!(details.iter().all(|b| !b.verified), "{details:?}");
    let reasons: Vec<&str> = details
        .iter()
        .map(|b| b.message.as_deref().expect("a reason"))
        .collect();
    assert!(reasons[0].contains("logpoints"), "{reasons:?}");
    assert!(reasons[1].contains("hit count"), "{reasons:?}");
    assert!(reasons[2].contains("conditional"), "{reasons:?}");
    // The kinds are kept, so the gutter can still tell them apart.
    let kinds: Vec<BreakpointKind> = details.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BreakpointKind::LogPoint,
            BreakpointKind::HitCount,
            BreakpointKind::Condition
        ]
    );
    // None of the texts was sent, so each one stopped as a plain breakpoint.
    for message in f.log().iter().filter(|m| m["command"] == "setBreakpoints") {
        for bp in message["arguments"]["breakpoints"]
            .as_array()
            .expect("list")
        {
            assert_eq!(bp.as_object().expect("object").len(), 1, "{bp}");
        }
    }
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
    assert!(
        ws.breakpoint_details(id).iter().all(|b| b.verified),
        "the state belongs to the session"
    );
}

#[test]
fn verification_comes_from_the_response_and_from_breakpoint_events() {
    let f = Fixture::new("verify");
    f.simple_launch();
    let mut ws = f.workspace(&["--late-verify"]);
    let id = ws.open(&f.program("WAIT\nprint done\n")).expect("open");
    assert!(ws.toggle_breakpoint(id, 1).expect("code"));
    assert!(ws.toggle_breakpoint(id, 40).expect("past the end"));
    ws.start_debugging().expect("start");
    wait(&mut ws, "the answer", |ws| {
        ws.breakpoint_at(id, 1).is_some_and(|b| !b.verified)
    });
    assert_eq!(
        ws.breakpoint_at(id, 1).and_then(|b| b.message).as_deref(),
        Some("waiting for the program")
    );
    // The adapter verifies the ones with code when asked (`!verify`).
    ws.evaluate_in_console("!verify").expect("evaluate");
    wait(&mut ws, "the event", |ws| {
        ws.breakpoint_at(id, 1).is_some_and(|b| b.verified)
    });
    assert!(!ws.breakpoint_at(id, 40).expect("kept").verified);
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);

    // Without the option the response itself says which one has no code.
    let f = Fixture::new("verify-response");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let id = ws.open(&f.program(LOOP)).expect("open");
    ws.toggle_breakpoint(id, 2).expect("code");
    ws.toggle_breakpoint(id, 40).expect("past the end");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the answer", |ws| {
        ws.breakpoint_at(id, 40).is_some_and(|b| !b.verified)
    });
    assert_eq!(
        ws.breakpoint_at(id, 40).and_then(|b| b.message).as_deref(),
        Some("no code on this line")
    );
    assert!(ws.breakpoint_at(id, 2).expect("code").verified);
}

#[test]
fn editing_a_breakpoint_while_running_sends_the_file_again_and_keeps_the_other_texts() {
    let f = Fixture::new("edit");
    let (mut ws, id) = loop_with(&f, &[], spec("i == 5", "", ""));
    wait_local(&mut ws, "i", "5");

    let mut draft = ws.begin_breakpoint_edit(id, 2, None).expect("draft");
    assert_eq!(draft.input, BreakpointInput::Expression);
    assert_eq!(draft.text(), "i == 5");
    draft.next_input();
    assert_eq!(draft.input, BreakpointInput::HitCount);
    assert_eq!(draft.text(), "");
    draft.set_text(" 2 ");
    draft.next_input();
    draft.set_text("hit {i}");
    draft.next_input();
    assert_eq!(
        draft.input,
        BreakpointInput::Expression,
        "three choices in a ring"
    );
    ws.finish_breakpoint_edit(&draft).expect("apply");
    let info = ws.breakpoint_at(id, 2).expect("still there");
    assert_eq!(info.spec.condition.as_deref(), Some("i == 5"));
    assert_eq!(info.spec.hit_condition.as_deref(), Some("2"), "trimmed");
    assert_eq!(info.spec.log_message.as_deref(), Some("hit {i}"));
    assert_eq!(info.kind, BreakpointKind::LogPoint);
    let sends = eventually(&f, "the second send", |log| {
        let sends = log
            .iter()
            .filter(|m| m["command"] == "setBreakpoints")
            .count();
        (sends >= 2).then_some(sends)
    });
    assert_eq!(sends, 2, "once at the start, once for the edit");

    // The editor on the edited line shows the breakpoint's own kind.
    assert_eq!(
        ws.begin_breakpoint_edit(id, 2, None).expect("draft").input,
        BreakpointInput::LogMessage
    );
    ws.set_breakpoint(id, 2, spec("", "", "")).expect("plain");
    assert_eq!(
        ws.breakpoint_at(id, 2).expect("plain").kind,
        BreakpointKind::Plain
    );
    assert!(
        !ws.toggle_breakpoint(id, 2).expect("clear"),
        "toggle removes any kind"
    );
    assert!(ws.breakpoint_at(id, 2).is_none());
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn breakpoint_texts_are_checked_and_hostile_ones_are_refused_or_cut() {
    let f = Fixture::new("hostile-text");
    let mut ws = f.workspace(&[]);
    let id = ws.open(&f.program(LOOP)).expect("open");
    for bad in ["a\nb", "a\rb", "a\0b"] {
        assert!(
            ws.set_breakpoint(id, 0, spec(bad, "", "")).is_err(),
            "{bad:?}"
        );
    }
    assert!(ws.breakpoint_at(id, 0).is_none(), "refused means unchanged");
    ws.set_breakpoint(id, 0, spec(&"x".repeat(100_000), "", ""))
        .expect("long ones are cut");
    let kept = ws
        .breakpoint_at(id, 0)
        .expect("set")
        .spec
        .condition
        .expect("text");
    assert_eq!(kept.chars().count(), rustode_workspace::MAX_BREAKPOINT_TEXT);
    ws.set_breakpoint(id, 0, spec("   ", "", ""))
        .expect("blank is none");
    assert_eq!(
        ws.breakpoint_at(id, 0).expect("set").kind,
        BreakpointKind::Plain
    );
    assert!(
        ws.begin_breakpoint_edit(id, 3, None).is_err(),
        "nothing to edit"
    );
    assert!(
        ws.begin_breakpoint_edit(id, 3, Some(BreakpointInput::Expression))
            .is_ok()
    );
    // A nonsense condition is the adapter's to judge: it stops with a message.
    f.simple_launch();
    ws.set_breakpoint(id, 2, spec("nope ==== 1", "", ""))
        .expect("set");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn the_breakpoint_editor_commands_are_registered_with_vs_codes_ids() {
    for (id, label) in [
        (
            "editor.debug.action.conditionalBreakpoint",
            "Debug: Add Conditional Breakpoint...",
        ),
        ("editor.debug.action.addLogPoint", "Debug: Add Logpoint..."),
        (
            "editor.debug.action.editBreakpoint",
            "Debug: Edit Breakpoint",
        ),
    ] {
        let info = rustode_workspace::COMMANDS
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(info.label, label);
    }
}

// ---- compatibility with real adapters ----

/// A `launch.json` with an `attach` configuration.
fn attach_launch(f: &Fixture) {
    f.launch_json(
        r#"{"configurations": [{"name": "Attach", "type": "test", "request": "attach",
            "program": "${workspaceFolder}/program.txt"}]}"#,
    );
}

#[test]
fn an_attach_configuration_sends_attach_and_stopping_leaves_the_program_alone() {
    let f = Fixture::new("attach-request");
    attach_launch(&f);
    let mut ws = f.workspace(&[]);
    let id = ws.open(&f.program("print one\nprint two\n")).expect("open");
    ws.toggle_breakpoint(id, 1).expect("toggle");
    ws.start_debugging().expect("attach starts");
    wait(&mut ws, "the stop", stopped);
    let commands = f.commands();
    assert!(commands.iter().any(|c| c == "attach"), "{commands:?}");
    assert!(!commands.iter().any(|c| c == "launch"), "{commands:?}");
    ws.stop_debugging().expect("stop");
    let disconnect = eventually(&f, "the disconnect", |log| {
        log.iter().find(|m| m["command"] == "disconnect").cloned()
    });
    assert_eq!(disconnect["arguments"]["terminateDebuggee"], false);
    assert!(
        !f.commands().iter().any(|c| c == "terminate"),
        "an attached program is not ours to terminate"
    );
    // A launch does terminate and disconnects with terminateDebuggee true.
    let f = Fixture::new("launch-terminates");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
    assert!(f.commands().iter().any(|c| c == "terminate"));
}

#[test]
fn initialized_before_the_initialize_response_or_right_after_it_is_survived() {
    for (mode, name) in [
        ("--initialized-first", "first"),
        ("--initialized-early", "early"),
    ] {
        let f = Fixture::new(&format!("initialized-{name}"));
        f.simple_launch();
        let mut ws = f.workspace(&[mode]);
        let id = with_breakpoint(&f, &mut ws);
        ws.start_debugging().expect("start");
        wait(&mut ws, "the stop", |ws| {
            stopped(ws) && ws.stopped_line(id) == Some(1)
        });
        let commands = f.commands();
        let position = |n: &str| commands.iter().position(|c| c == n).expect(n);
        assert!(
            position("launch") < position("setBreakpoints"),
            "{mode}: {commands:?}"
        );
        assert!(
            position("setBreakpoints") < position("configurationDone"),
            "{mode}"
        );
        ws.continue_debugging().expect("continue");
        wait(&mut ws, "the end", ended);
    }
}

#[test]
fn the_adapters_own_requests_are_refused_with_a_reason_and_the_session_goes_on() {
    let f = Fixture::new("reverse");
    f.simple_launch();
    let mut ws = f.workspace(&["--reverse-requests"]);
    with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    let init = f
        .log()
        .into_iter()
        .find(|m| m["command"] == "initialize")
        .expect("initialize");
    assert_eq!(init["arguments"]["supportsRunInTerminalRequest"], false);
    assert_eq!(init["arguments"]["supportsStartDebuggingRequest"], false);
    let replies = eventually(&f, "both refusals", |log| {
        let replies: Vec<serde_json::Value> = log
            .iter()
            .filter(|m| m["type"] == "response" && m["success"] == false)
            .cloned()
            .collect();
        (replies.len() >= 2).then_some(replies)
    });
    let messages: Vec<&str> = replies
        .iter()
        .filter_map(|r| r["message"].as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("terminal")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("subprocess")),
        "{messages:?}"
    );
    let console: Vec<&str> = ws.debug_console().iter().map(|l| l.text.as_str()).collect();
    assert!(
        console.iter().any(|l| l.starts_with("runInTerminal:")),
        "{console:?}"
    );
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
}

#[test]
fn an_adapter_that_exits_after_exited_ends_the_session_normally() {
    let f = Fixture::new("exit-without-terminated");
    f.simple_launch();
    let mut ws = f.workspace(&["--exit-without-terminated"]);
    with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", stopped);
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
    assert_eq!(
        ws.debug_status().message.as_deref(),
        Some("the program exited with code 0"),
        "not 'the debug adapter exited'"
    );
}

#[test]
fn a_stop_without_a_thread_id_uses_the_first_thread() {
    let f = Fixture::new("no-thread");
    f.simple_launch();
    let mut ws = f.workspace(&["--stop-without-thread"]);
    let id = with_breakpoint(&f, &mut ws);
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stack", |ws| {
        stopped(ws) && !ws.debug_frames().is_empty()
    });
    assert_eq!(ws.stopped_line(id), Some(1));
    assert_eq!(ws.debug_status().thread, Some(1));
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the end", ended);
}

#[test]
fn the_stop_is_found_in_a_file_the_adapter_names_through_a_symlink() {
    let f = Fixture::new("symlink");
    f.simple_launch();
    let mut ws = f.workspace(&[]);
    let id = ws
        .open(&f.program("print one\nprint two\nprint three\n"))
        .expect("open");
    ws.toggle_breakpoint(id, 1).expect("toggle");
    // The adapter is told the program through a link to the folder, and says
    // so in its frames: the document is open by the real path.
    let link = f.dir.join("link");
    std::os::unix::fs::symlink(f.project(), &link).expect("symlink");
    f.launch_json(&format!(
        r#"{{"configurations": [{{"name": "L", "type": "test", "request": "launch",
            "program": "{}/program.txt"}}]}}"#,
        link.display()
    ));
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop", |ws| {
        stopped(ws) && ws.stopped_line(id) == Some(1)
    });
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}

#[test]
fn hostile_breakpoint_answers_and_events_change_nothing() {
    let f = Fixture::new("hostile-breakpoints");
    f.simple_launch();
    let mut ws = f.workspace(&["--hostile-replies"]);
    let id = ws
        .open(&f.program("print a\nHOSTILE\nprint c\n"))
        .expect("open");
    for line in [0, 2] {
        assert!(ws.toggle_breakpoint(id, line).expect("toggle"));
    }
    ws.set_breakpoint(id, 1, spec("a", "", "")).expect("set");
    ws.start_debugging().expect("start");
    // The adapter stops at the first line; its answers to setBreakpoints
    // were garbage, and the garbage `breakpoint` events come when the
    // program goes on past the HOSTILE line.
    wait(&mut ws, "the stop", stopped);
    let kinds: Vec<BreakpointKind> = ws.breakpoint_details(id).iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BreakpointKind::Plain,
            BreakpointKind::Condition,
            BreakpointKind::Plain
        ]
    );
    ws.continue_debugging().expect("continue");
    wait(&mut ws, "the garbage events", |ws| {
        ws.debug_console()
            .iter()
            .any(|l| l.kind == ConsoleKind::Error)
            || ended(ws)
            || stopped(ws)
    });
    let details = ws.breakpoint_details(id);
    assert_eq!(details.len(), 3, "no breakpoint was added or lost");
    assert!(
        details
            .iter()
            .all(|b| b.message.as_ref().is_none_or(|m| m.chars().count() <= 500)),
        "{details:?}"
    );
    ws.stop_debugging().ok();
}

#[test]
fn a_relative_path_in_a_frame_is_resolved_against_the_configurations_cwd() {
    let f = Fixture::new("cwd");
    std::fs::create_dir_all(f.project().join("sub")).expect("sub");
    let file = f.project().join("sub/program.txt");
    std::fs::write(&file, "print one\nprint two\n").expect("program");
    f.launch_json(
        r#"{"configurations": [{"name": "Sub", "type": "test", "request": "launch",
            "program": "${workspaceFolder}/sub/program.txt", "cwd": "${workspaceFolder}/sub"}]}"#,
    );
    let mut ws = f.workspace(&["--relative-paths"]);
    let id = ws.open(&file).expect("open");
    ws.toggle_breakpoint(id, 1).expect("toggle");
    ws.start_debugging().expect("start");
    wait(&mut ws, "the stop in the right file", |ws| {
        stopped(ws) && ws.stopped_line(id) == Some(1)
    });
    ws.stop_debugging().expect("stop");
    wait(&mut ws, "the end", ended);
}
