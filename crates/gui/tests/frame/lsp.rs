//! Input to frame with a language server: squiggles, the status bar's
//! counts (a button screen readers hear as "Errors: 1, Warnings: 1") and the
//! document's server, and the problems panel (Ctrl+Shift+M), whose rows go
//! to their problem. The server is `rustode-test-lsp`, which reports every
//! `ERROR` and `WARN`. Going to a problem opens or activates its file's tab.

use super::{press, status};
use eframe::egui::{Key, Modifiers, ViewportId, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use rustode_gui::EditorApp;
use rustode_workspace::Workspace;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const SAMPLE: &str = "fn main() {\n    let x = ERROR;\n    WARN\n}\n";

/// The test server, built next to this test binary: in one of the folders
/// above it, whatever layout cargo gives its build folder. When only this
/// crate's tests were built, it is built now.
fn server() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary");
    if let Some(path) = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("rustode-test-lsp"))
        .find(|path| path.is_file())
    {
        return path;
    }
    let profile = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "debug"))
        .expect("a debug profile folder");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "-p", "rustode-test-lsp"])
        .arg("--target-dir")
        .arg(profile.parent().expect("target dir"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building rustode-test-lsp failed");
    profile.join("rustode-test-lsp")
}

/// A window on `main.rs` in an opened folder, then settings that start the
/// test server.
fn open(name: &str) -> Harness<'static, EditorApp> {
    open_with(name, &[])
}

/// [`open`], with more files opened after `main.rs`: the last one is active.
fn open_with(name: &str, others: &[(&str, &str)]) -> Harness<'static, EditorApp> {
    let base = std::env::temp_dir().join(format!("rustode-gui-lsp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // The folder's name shows in the explorer, so it is the same every run;
    // the settings live next to it, out of the explorer.
    let dir = base.join("project");
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::create_dir_all(base.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, SAMPLE).expect("fixture");
    // A plain path's Debug form is a valid JSON string.
    let command = format!("{:?}", server().display().to_string());
    let settings =
        format!("{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}}}}}}}");
    std::fs::write(base.join("user/settings.json"), settings).expect("settings");
    let mut workspace = Workspace::new();
    workspace.open_folder(&dir).expect("folder");
    workspace.open(&file).expect("open");
    for (other, text) in others {
        let path = dir.join(other);
        std::fs::write(&path, text).expect("fixture");
        workspace.open(&path).expect("open");
    }
    let app = EditorApp::new(workspace);
    let mut harness = Harness::builder()
        .with_size([640.0, 360.0])
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    harness.run();
    // The window is idle now; the settings start the server.
    harness.state_mut().load_user_config(&base.join("user"));
    harness
}

/// Steps the window whenever it asks for a repaint, until `done` holds.
/// Returns how many repaints were asked for without any input.
#[track_caller]
fn wait(
    harness: &mut Harness<'_, EditorApp>,
    done: impl Fn(&Harness<'_, EditorApp>) -> bool,
) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut woken = 0;
    while !done(harness) {
        assert!(Instant::now() < deadline, "timed out");
        if harness.ctx.has_requested_repaint() {
            woken += 1;
            harness.step();
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    harness.run();
    woken
}

fn counts(harness: &Harness<'_, EditorApp>) -> Option<String> {
    harness
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .find(|label| label.starts_with("Errors") || label == "No Problems")
}

fn rows(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| label.contains(" at line "))
        .collect()
}

#[test]
fn diagnostics_arrive_while_idle_and_are_squiggled_and_counted() {
    let mut harness = open("squiggles");
    let woken = wait(&mut harness, |h| {
        counts(h).as_deref() == Some("Errors: 1, Warnings: 1")
    });
    assert!(woken > 0, "the server's messages woke the idle window");
    assert!(
        status(&harness)
            .iter()
            .any(|item| item == "rustode-test-lsp"),
        "the document's server: {:?}",
        status(&harness)
    );
    assert_eq!(
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay,
        Duration::MAX,
        "nothing is scheduled once the diagnostics are in"
    );
    harness.snapshot("lsp_squiggles");
}

#[test]
fn ctrl_shift_m_shows_the_problems_and_enter_goes_to_one() {
    let mut harness = open("panel");
    wait(&mut harness, |h| {
        counts(h).is_some_and(|c| c.starts_with("Errors"))
    });
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert_eq!(
        rows(&harness),
        [
            "Error: ERROR found at line 2 and character 13. generated by test-lsp",
            "Warning: WARN found at line 3 and character 5. generated by test-lsp",
        ]
    );
    harness.snapshot("lsp_problems");
    press(&mut harness, Modifiers::NONE, Key::Tab);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(
        status(&harness)
            .iter()
            .any(|item| item == "Ln 3, Col 9 (4 selected)"),
        "the warning is selected: {:?}",
        status(&harness)
    );
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert!(rows(&harness).is_empty(), "a second Ctrl+Shift+M hides it");
}

#[test]
fn a_click_on_the_counts_toggles_the_panel_and_a_row_click_goes_to_it() {
    let mut harness = open_with("click", &[("notes.txt", "notes\n")]);
    assert!(harness.state().window_title().starts_with("notes.txt"));
    wait(&mut harness, |h| {
        counts(h).is_some_and(|c| c.starts_with("Errors"))
    });
    harness
        .get_by(|node| node.label().is_some_and(|l| l.starts_with("Errors")))
        .click();
    harness.run();
    harness
        .get_by(|node| node.label().is_some_and(|l| l.starts_with("Error: ERROR")))
        .click();
    harness.run();
    assert!(
        status(&harness)
            .iter()
            .any(|item| item == "Ln 2, Col 18 (5 selected)"),
        "{:?}",
        status(&harness)
    );
    assert!(
        harness.state().window_title().starts_with("main.rs"),
        "the problem's tab is active: {}",
        harness.state().window_title()
    );
}
