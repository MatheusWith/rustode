//! Input to frame with a debug adapter: a click on the gutter sets a
//! breakpoint, F5 starts the launch configuration, the program stops on the
//! breakpoint's line (the status bar says so and the gutter marks the line),
//! F5 continues it to the end and Shift+F5 stops it. The adapter is
//! `rustode-test-dap`, whose program is a text file.

use super::{press, quiet, status, type_text};
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use rustode_gui::{EditorApp, LINE_HEIGHT};
use rustode_workspace::{DebugState, Workspace};
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

/// A window on the program in an opened folder with a `launch.json`, then
/// settings that name the test adapter.
fn open(name: &str) -> Harness<'static, EditorApp> {
    open_program(
        name,
        "print one\nprint two\nprint three\n",
        [640.0, 360.0],
        &[],
    )
}

/// As [`open`], on this program, in a window of this size.
fn open_program(
    name: &str,
    program: &str,
    size: [f32; 2],
    breakpoints: &[usize],
) -> Harness<'static, EditorApp> {
    open_with(name, program, size, breakpoints, &[])
}

/// As [`open_program`], with these options for the adapter.
fn open_with(
    name: &str,
    program: &str,
    size: [f32; 2],
    breakpoints: &[usize],
    args: &[&str],
) -> Harness<'static, EditorApp> {
    let base = std::env::temp_dir().join(format!("rustode-gui-dap-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("project");
    std::fs::create_dir_all(dir.join(".vscode")).expect("temp dir");
    std::fs::create_dir_all(base.join("user")).expect("temp dir");
    let file = dir.join("program.txt");
    std::fs::write(&file, program).expect("program");
    std::fs::write(
        dir.join(".vscode/launch.json"),
        r#"{ "configurations": [ { "name": "Run program", "type": "test", "request": "launch",
              "program": "${workspaceFolder}/program.txt" } ] }"#,
    )
    .expect("launch.json");
    let command = format!("{:?}", adapter().display().to_string());
    let args = format!(
        "[{}]",
        args.iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let settings = format!(
        "{{\"rustode.debugAdapters\": {{\"test\": {{\"command\": {command}, \"args\": {args}}}}}}}"
    );
    std::fs::write(base.join("user/settings.json"), settings).expect("settings");
    let mut workspace = Workspace::new();
    workspace.open_folder(&dir).expect("folder");
    let id = workspace.open(&file).expect("open");
    for line in breakpoints {
        workspace.toggle_breakpoint(id, *line).expect("breakpoint");
    }
    let app = EditorApp::new(workspace);
    let mut harness = Harness::builder().with_size(size).build_eframe(move |cc| {
        rustode_gui::fonts::install(&cc.egui_ctx);
        app
    });
    quiet(&mut harness);
    harness.state_mut().load_user_config(&base.join("user"));
    harness
}

/// Steps the window whenever it asks for a repaint, until `done` holds.
#[track_caller]
fn wait(harness: &mut Harness<'_, EditorApp>, done: impl Fn(&Harness<'_, EditorApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(harness) {
        assert!(Instant::now() < deadline, "timed out");
        if harness.ctx.has_requested_repaint() {
            harness.step();
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    quiet(harness);
}

fn debug_item(harness: &Harness<'_, EditorApp>) -> Option<String> {
    status(harness)
        .into_iter()
        .find(|item| item.starts_with("Debug: "))
}

fn click_at(harness: &mut Harness<'_, EditorApp>, pos: Pos2) {
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    quiet(harness);
}

#[test]
fn a_gutter_click_sets_a_breakpoint_and_f5_stops_on_it_and_continues() {
    let mut harness = open("run");
    let rect = harness.get_by_role(Role::MultilineTextInput).rect();
    // The gutter is the left of the editor; the second line is 1.5 lines down.
    click_at(
        &mut harness,
        Pos2::new(rect.left() + 6.0, rect.top() + 1.5 * LINE_HEIGHT),
    );
    let id = harness.state().workspace().tabs()[0];
    assert_eq!(harness.state().workspace().breakpoints(id), [1]);
    assert_eq!(
        status(&harness)
            .iter()
            .find(|i| i.starts_with("Ln "))
            .map(String::as_str),
        Some("Ln 1, Col 1"),
        "the click did not move the caret"
    );
    harness.snapshot("debug_breakpoint");

    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().debug_status().state == DebugState::Stopped
            && h.state().workspace().stopped_line(id).is_some()
    });
    assert_eq!(
        debug_item(&harness).as_deref(),
        Some("Debug: stopped (Run program)")
    );
    harness.snapshot("debug_stopped");

    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().debug_status().state == DebugState::Inactive
    });
    assert_eq!(
        debug_item(&harness).as_deref(),
        Some("Debug: the program exited with code 0")
    );
}

#[test]
fn shift_f5_stops_the_session_and_f9_toggles_at_the_caret() {
    let mut harness = open("stop");
    let id = harness.state().workspace().tabs()[0];
    press(&mut harness, Modifiers::NONE, Key::F9);
    assert_eq!(harness.state().workspace().breakpoints(id), [0]);
    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().debug_status().state == DebugState::Stopped
    });
    press(&mut harness, Modifiers::SHIFT, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().debug_status().state == DebugState::Inactive
    });
}

// ---- ticket 38: stepping, call stack, variables, watch and console ----

const NESTED: &str = "print starting\nglobal g 9\nvar a 1\nobj p k=v\nenter outer\nvar b 2\n\
obj o x=1 inner=@p\nprint in outer\nleave\nprint done\n";

fn state(harness: &Harness<'_, EditorApp>) -> DebugState {
    harness.state().workspace().debug_status().state
}

/// Stopped on the line of `print in outer` with the views loaded.
fn stopped_in_outer(name: &str) -> Harness<'static, EditorApp> {
    let mut harness = open_program(name, NESTED, [900.0, 520.0], &[7]);
    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        let ws = h.state().workspace();
        let rows = ws.debug_rows(rustode_workspace::DebugTree::Variables);
        state(h) == DebugState::Stopped
            && ws.debug_frames().len() == 2
            && rows.len() > 2
            && rows.iter().all(|r| !r.loading)
    });
    harness
}

#[test]
fn the_stopped_state_shows_call_stack_variables_watch_and_console() {
    let mut harness = stopped_in_outer("views");
    harness.snapshot("debug_views_stopped");
    // The panel is on screen: its rows are buttons a screen reader finds.
    harness.get_by_label("outer program.txt:8");
    harness.get_by_label("main program.txt:5");
    harness.get_by_label("b, value 2");
    harness.get_by_label("o, value Object");

    // Watch: the palette focuses the field; Enter adds the expression.
    press(&mut harness, Modifiers::NONE, Key::F1);
    type_text(&mut harness, "focus on watch");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    type_text(&mut harness, "b");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    wait(&mut harness, |h| {
        h.state()
            .workspace()
            .debug_rows(rustode_workspace::DebugTree::Watch)
            .first()
            .is_some_and(|r| r.value == "2")
    });
    // Console: evaluate an object, then open `o` in Variables with a click.
    press(&mut harness, Modifiers::NONE, Key::F1);
    type_text(&mut harness, "focus on debug console");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    type_text(&mut harness, "o");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    wait(&mut harness, |h| {
        h.state()
            .workspace()
            .debug_console()
            .iter()
            .any(|l| l.text == "Object")
    });
    harness.get_by_label("o, value Object").click();
    quiet(&mut harness);
    wait(&mut harness, |h| {
        h.state()
            .workspace()
            .debug_rows(rustode_workspace::DebugTree::Variables)
            .len()
            > 5
    });
    harness.get_by_label("x, value 1");
    harness.snapshot("debug_views_inspected");
}

#[test]
fn f10_f11_and_shift_f11_step_and_a_click_on_a_frame_moves_the_editor() {
    let mut harness = stopped_in_outer("steps");
    let id = harness.state().workspace().tabs()[0];
    let line = |h: &Harness<'_, EditorApp>| h.state().workspace().stopped_line(id);
    assert_eq!(line(&harness), Some(7));
    press(&mut harness, Modifiers::NONE, Key::F10);
    wait(&mut harness, |h| {
        state(h) == DebugState::Stopped && line(h) == Some(8)
    });
    wait(&mut harness, |h| {
        h.state().workspace().debug_frames().len() == 2
    });
    // Back to the caller by clicking its frame: the editor goes there.
    harness.get_by_label("main program.txt:5").click();
    quiet(&mut harness);
    assert_eq!(harness.state().workspace().debug_selected_frame(), 1);
    assert_eq!(line(&harness), Some(4));
    let cursor = harness
        .state()
        .workspace()
        .document(id)
        .expect("document")
        .cursor_position();
    assert_eq!(cursor.line, 4);

    press(&mut harness, Modifiers::SHIFT, Key::F11);
    wait(&mut harness, |h| {
        state(h) == DebugState::Stopped && line(h) == Some(9)
    });
    press(&mut harness, Modifiers::NONE, Key::F11);
    wait(&mut harness, |h| state(h) == DebugState::Inactive);
}

#[test]
fn f6_pauses_while_the_program_runs_and_moves_the_focus_otherwise() {
    let mut harness = open_program("f6", "print a\nWAIT\nprint b\n", [900.0, 520.0], &[]);
    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| state(h) == DebugState::Running);
    press(&mut harness, Modifiers::NONE, Key::F6);
    wait(&mut harness, |h| state(h) == DebugState::Stopped);
    assert_eq!(
        harness.state().workspace().debug_status().reason.as_deref(),
        Some("pause")
    );
    let focused = |h: &Harness<'_, EditorApp>| {
        h.query_all_by_role(Role::TextInput)
            .any(|node| node.accesskit_node().is_focused())
    };
    assert!(!focused(&harness), "the key paused, it did not move focus");
    // Stopped: F6 is "focus the next part", and the debug panel is one.
    press(&mut harness, Modifiers::NONE, Key::F6);
    assert_eq!(state(&harness), DebugState::Stopped);
    assert!(focused(&harness), "the panel's console took the focus");
}

// ---- ticket 39: the breakpoint editor and the marks of each kind ----

use rustode_workspace::{BreakpointKind, BreakpointSpec};

const LOOP: &str = "loop 3\ninc i\nprint tick\nend\n";

/// Runs a command from the palette.
fn palette(harness: &mut Harness<'_, EditorApp>, words: &str) {
    press(harness, Modifiers::NONE, Key::F1);
    type_text(harness, words);
    press(harness, Modifiers::NONE, Key::Enter);
}

#[test]
fn the_conditional_breakpoint_box_sets_a_condition_and_the_program_stops_when_it_holds() {
    let mut harness = open_program("conditional", LOOP, [640.0, 360.0], &[]);
    let id = harness.state().workspace().tabs()[0];
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    palette(&mut harness, "add conditional");
    // The box names the three texts and shows the expression's field.
    for name in ["Expression", "Hit Count", "Log Message"] {
        harness.get_by_label(name);
    }
    type_text(&mut harness, "i == 2");
    harness.snapshot("breakpoint_box");
    // Tab shows the hit count, which is empty; a click on a name goes back.
    press(&mut harness, Modifiers::NONE, Key::Tab);
    assert_eq!(
        harness.get_by_role(Role::TextInput).value().as_deref(),
        Some("")
    );
    harness.get_by_label("Expression").click();
    quiet(&mut harness);
    assert_eq!(
        harness.get_by_role(Role::TextInput).value().as_deref(),
        Some("i == 2"),
        "the text was kept"
    );
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(harness.query_by_role(Role::TextInput).is_none(), "closed");
    let info = harness
        .state()
        .workspace()
        .breakpoint_at(id, 2)
        .expect("set");
    assert_eq!(info.kind, BreakpointKind::Condition);
    assert_eq!(info.spec.condition.as_deref(), Some("i == 2"));
    harness.snapshot("breakpoint_conditional_mark");

    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().stopped_line(id) == Some(2)
            && h.state()
                .workspace()
                .debug_rows(rustode_workspace::DebugTree::Variables)
                .iter()
                .any(|r| r.name.trim() == "i" && r.value == "2")
    });
    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| state(h) == DebugState::Inactive);
}

#[test]
fn the_logpoint_box_edit_breakpoint_and_escape() {
    let mut harness = open_program("logpoint", LOOP, [640.0, 360.0], &[]);
    let id = harness.state().workspace().tabs()[0];
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    palette(&mut harness, "add logpoint");
    type_text(&mut harness, "tick {i}");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(
        harness
            .state()
            .workspace()
            .breakpoint_at(id, 2)
            .expect("set")
            .kind,
        BreakpointKind::LogPoint
    );
    // Edit Breakpoint opens on the log message the line has; Escape cancels.
    palette(&mut harness, "edit breakpoint");
    assert_eq!(
        harness.get_by_role(Role::TextInput).value().as_deref(),
        Some("tick {i}")
    );
    type_text(&mut harness, "zzz");
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.query_by_role(Role::TextInput).is_none(), "closed");
    assert_eq!(
        harness
            .state()
            .workspace()
            .breakpoint_at(id, 2)
            .and_then(|b| b.spec.log_message)
            .as_deref(),
        Some("tick {i}")
    );
    // A line with no breakpoint has nothing to edit.
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    palette(&mut harness, "edit breakpoint");
    assert!(harness.query_by_role(Role::TextInput).is_none());
    assert!(
        status(&harness)
            .iter()
            .any(|i| i.contains("there is no breakpoint on this line")),
        "{:?}",
        status(&harness)
    );
}

#[test]
fn each_kind_of_breakpoint_has_its_own_mark_and_an_unsupported_one_is_unverified() {
    let mut harness = open_with(
        "marks",
        "print a\nprint b\nprint c\n",
        [640.0, 360.0],
        &[],
        &["--no-conditions", "--no-hit-conditions", "--no-logpoints"],
    );
    let id = harness.state().workspace().tabs()[0];
    let spec = |c: Option<&str>, l: Option<&str>| BreakpointSpec {
        condition: c.map(str::to_owned),
        hit_condition: None,
        log_message: l.map(str::to_owned),
    };
    let ws = harness.state_mut().workspace_mut();
    ws.toggle_breakpoint(id, 0).expect("plain");
    ws.set_breakpoint(id, 1, spec(Some("a == 1"), None))
        .expect("condition");
    ws.set_breakpoint(id, 2, spec(None, Some("hi")))
        .expect("log");
    ws.toggle_breakpoint(id, 3).expect("past the end");
    quiet(&mut harness);
    harness.snapshot("breakpoint_marks");

    press(&mut harness, Modifiers::NONE, Key::F5);
    wait(&mut harness, |h| {
        h.state().workspace().stopped_line(id) == Some(0)
            && h.state()
                .workspace()
                .breakpoint_at(id, 3)
                .is_some_and(|b| !b.verified)
    });
    harness.snapshot("breakpoint_marks_unverified");
    let details = harness.state().workspace().breakpoint_details(id);
    assert!(details[1..].iter().all(|b| !b.verified), "{details:?}");
}
