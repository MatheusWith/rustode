//! Input to frame for go to definition (F12), find references (Shift+F12),
//! rename (F2) and format (Ctrl+Shift+I): the location list and the rename
//! box are in the AccessKit tree as a text input and buttons named after the
//! places, the answer shows in the status bar, and a PNG snapshot has each
//! box. The server is `rustode-test-lsp`; what is asked of it and applied is
//! tested at the workspace seam (`crates/workspace/tests/lsp_navigation.rs`).

use super::{lsp, press, quiet, type_text};
use eframe::egui::{Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use rustode_gui::EditorApp;
use rustode_workspace::{ServerState, Workspace};
use std::time::{Duration, Instant};

const A: &str = "fn helper() {}\nfn main() {\n    helper();   \n}\n";
const B: &str = "fn other() {\n    helper();\n}\n";

/// A window on `main.rs`, next to `other.rs`, with the test server running
/// and the cursor inside the call of `helper`.
fn open(name: &str) -> Harness<'static, EditorApp> {
    let base = std::env::temp_dir().join(format!("rustode-gui-nav-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("project");
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::create_dir_all(base.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, A).expect("fixture");
    std::fs::write(dir.join("other.rs"), B).expect("fixture");
    // A plain path's Debug form is a valid JSON string.
    let command = format!("{:?}", lsp::server().display().to_string());
    let settings =
        format!("{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}}}}}}}");
    std::fs::write(base.join("user/settings.json"), settings).expect("settings");
    let mut workspace = Workspace::new();
    let id = workspace.open(&file).expect("open");
    let call = A.match_indices("helper").nth(1).expect("call").0 + 2;
    workspace.set_cursor(id, call).expect("cursor");
    let app = EditorApp::new(workspace);
    let mut harness = Harness::builder()
        .with_size([640.0, 360.0])
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    quiet(&mut harness);
    harness.state_mut().load_user_config(&base.join("user"));
    wait(&mut harness, |h| {
        h.state()
            .workspace()
            .language_servers()
            .iter()
            .any(|s| s.state == ServerState::Running)
    });
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

fn text(harness: &Harness<'_, EditorApp>) -> String {
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    workspace.document(id).expect("doc").text()
}

fn buttons(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| label.contains(".rs:"))
        .collect()
}

fn status_has(harness: &Harness<'_, EditorApp>, needle: &str) -> bool {
    harness
        .get_by_role(Role::Status)
        .query_all_by_role(Role::Label)
        .filter_map(|item| item.value())
        .any(|shown| shown.contains(needle))
}

#[test]
fn shift_f12_lists_the_references_as_buttons_and_a_click_goes_there() {
    let mut harness = open("refs");
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    wait(&mut harness, |h| !buttons(h).is_empty());
    assert_eq!(
        buttons(&harness),
        [
            "fn helper() {} main.rs:1",
            "helper(); main.rs:3",
            "helper(); other.rs:2"
        ]
    );
    assert!(status_has(&harness, "References of 'helper': 3"));
    harness.snapshot("references_list");
    harness
        .get_by(|node| node.label().is_some_and(|l| l == "helper(); other.rs:2"))
        .click();
    quiet(&mut harness);
    assert!(buttons(&harness).is_empty(), "the list closes");
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    let doc = workspace.document(id).expect("doc");
    assert_eq!(doc.path().file_name().expect("name"), "other.rs");
    assert_eq!(doc.cursor_position().line, 1);
}

#[test]
fn typing_narrows_the_list_and_escape_closes_it() {
    let mut harness = open("narrow");
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    wait(&mut harness, |h| !buttons(h).is_empty());
    type_text(&mut harness, "other");
    assert_eq!(buttons(&harness), ["helper(); other.rs:2"]);
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(buttons(&harness).is_empty());
    assert!(harness.state().workspace().locations().is_none());
}

#[test]
fn f12_goes_to_the_definition() {
    let mut harness = open("def");
    press(&mut harness, Modifiers::NONE, Key::F12);
    wait(&mut harness, |h| {
        let ws = h.state().workspace();
        ws.active_document()
            .and_then(|id| ws.document(id))
            .is_some_and(|doc| doc.primary_selection().head == 9)
    });
    assert!(buttons(&harness).is_empty());
}

#[test]
fn f2_asks_for_the_name_and_enter_renames_in_every_file() {
    let mut harness = open("rename");
    press(&mut harness, Modifiers::NONE, Key::F2);
    let input = harness.get_by_role(Role::TextInput);
    assert_eq!(input.value().as_deref(), Some("helper"));
    harness.snapshot("rename_box");
    for _ in 0..6 {
        press(&mut harness, Modifiers::NONE, Key::Backspace);
    }
    type_text(&mut harness, "assist");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    wait(&mut harness, |h| {
        status_has(h, "Renamed to 'assist': 3 edit(s) in 2 file(s)")
    });
    assert!(harness.query_by_role(Role::TextInput).is_none(), "closed");
    assert_eq!(
        text(&harness),
        "fn assist() {}\nfn main() {\n    assist();   \n}\n"
    );
    press(&mut harness, Modifiers::COMMAND, Key::Z);
    assert_eq!(text(&harness), A, "one undo step");
}

#[test]
fn ctrl_shift_i_formats_the_document() {
    let mut harness = open("format");
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    wait(&mut harness, |h| status_has(h, "Formatted the document"));
    assert_eq!(
        text(&harness),
        "fn helper() {}\nfn main() {\n    helper();\n}\n"
    );
}
