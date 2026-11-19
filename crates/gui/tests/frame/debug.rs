//! Input to frame with a debug adapter: a click on the gutter sets a
//! breakpoint, F5 starts the launch configuration, the program stops on the
//! breakpoint's line (the status bar says so and the gutter marks the line),
//! F5 continues it to the end and Shift+F5 stops it. The adapter is
//! `rustode-test-dap`, whose program is a text file.

use super::{press, quiet, status};
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
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
    let base = std::env::temp_dir().join(format!("rustode-gui-dap-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("project");
    std::fs::create_dir_all(dir.join(".vscode")).expect("temp dir");
    std::fs::create_dir_all(base.join("user")).expect("temp dir");
    let file = dir.join("program.txt");
    std::fs::write(&file, "print one\nprint two\nprint three\n").expect("program");
    std::fs::write(
        dir.join(".vscode/launch.json"),
        r#"{ "configurations": [ { "name": "Run program", "type": "test", "request": "launch",
              "program": "${workspaceFolder}/program.txt" } ] }"#,
    )
    .expect("launch.json");
    let command = format!("{:?}", adapter().display().to_string());
    let settings =
        format!("{{\"rustode.debugAdapters\": {{\"test\": {{\"command\": {command}}}}}}}");
    std::fs::write(base.join("user/settings.json"), settings).expect("settings");
    let mut workspace = Workspace::new();
    workspace.open_folder(&dir).expect("folder");
    workspace.open(&file).expect("open");
    let app = EditorApp::new(workspace);
    let mut harness = Harness::builder()
        .with_size([640.0, 360.0])
        .build_eframe(move |cc| {
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
