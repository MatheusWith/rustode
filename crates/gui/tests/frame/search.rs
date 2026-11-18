//! The Search view in the sidebar: Ctrl+Shift+F, a search whose results
//! stream into the list, the toggles, a click on a file or on a match, and
//! Escape, read from the AccessKit tree and an image snapshot. The search
//! itself is tested at the workspace seam (`crates/workspace/tests/search.rs`).

use super::{press, temp_dir, type_text};
use eframe::egui::{Key, Modifiers, accesskit::Role, accesskit::Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use rustode_gui::EditorApp;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A folder named `demo` with a few files, opened in the explorer, with
/// `notes.txt` in a tab; 1200 points wide, so the sidebar is 300.
fn open(name: &str) -> (Harness<'static, EditorApp>, PathBuf) {
    let root = temp_dir(name).join("demo");
    for (file, text) in [
        ("notes.txt", "hello\n"),
        (
            "src/lib.rs",
            "pub fn needle() {}\n// a needle and a Needle\n",
        ),
        ("src/main.rs", "fn main() { needle(); }\n"),
        ("README.md", "No match here.\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("fixture");
    }
    let app = EditorApp::open_paths(&[root.clone(), root.join("notes.txt")]).expect("open");
    let mut harness = Harness::builder()
        .with_size([1200.0, 400.0])
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    harness.run();
    (harness, root)
}

/// Steps frames until the search is over.
fn settle(harness: &mut Harness<'_, EditorApp>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.step();
        if !harness.state().workspace().search_results().is_running() {
            break;
        }
        assert!(Instant::now() < deadline, "the search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    harness.run();
}

fn ctrl_shift_f(harness: &mut Harness<'_, EditorApp>) {
    press(harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
}

/// Shows the view, types `text` in the search box and presses Enter.
fn search(harness: &mut Harness<'_, EditorApp>, text: &str) {
    ctrl_shift_f(harness);
    type_text(harness, text);
    press(harness, Modifiers::NONE, Key::Enter);
    settle(harness);
}

/// The text of every label, in tree order.
fn labels(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Label)
        .filter_map(|label| label.value())
        .collect()
}

#[track_caller]
fn assert_label(harness: &Harness<'_, EditorApp>, text: &str) {
    let labels = labels(harness);
    assert!(
        labels.iter().any(|label| label == text),
        "no label {text:?} in {labels:?}"
    );
}

/// The placeholder of the focused text input, if one has focus.
fn focused_input(harness: &Harness<'_, EditorApp>) -> Option<String> {
    harness
        .query_all_by_role(Role::TextInput)
        .find(egui_kittest::Node::is_focused)
        .and_then(|input| input.accesskit_node().placeholder().map(str::to_owned))
}

#[test]
fn ctrl_shift_f_shows_the_view_and_enter_lists_the_results() {
    let (mut harness, _) = open("search-results");
    ctrl_shift_f(&mut harness);
    assert_label(&harness, "SEARCH");
    assert_eq!(focused_input(&harness).as_deref(), Some("Search"));
    let placeholders: Vec<String> = harness
        .query_all_by_role(Role::TextInput)
        .filter_map(|input| input.accesskit_node().placeholder().map(str::to_owned))
        .collect();
    assert_eq!(
        placeholders,
        ["Search", "files to include", "files to exclude"]
    );
    type_text(&mut harness, "needle");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    settle(&mut harness);
    assert_label(&harness, "4 results in 2 files");
    assert!(harness.query_by_label("lib.rs").is_some());
    assert!(harness.query_by_label("main.rs").is_some());
    assert_eq!(
        harness
            .query_all_by_label("// a needle and a Needle")
            .count(),
        2
    );
    assert!(harness.query_by_label("fn main() { needle(); }").is_some());
    harness.snapshot("search_results");
}

#[test]
fn the_toggles_are_toggle_buttons_that_search_again() {
    let (mut harness, _) = open("search-toggles");
    search(&mut harness, "Needle");
    assert_label(&harness, "4 results in 2 files");
    let case = harness.get_by_label("Aa");
    assert_eq!(case.accesskit_node().toggled(), Some(Toggled::False));
    case.click();
    harness.step();
    settle(&mut harness);
    assert_eq!(
        harness.get_by_label("Aa").accesskit_node().toggled(),
        Some(Toggled::True)
    );
    assert_label(&harness, "1 result in 1 file");
    let focused = harness
        .query_all_by_role(Role::TextInput)
        .find(egui_kittest::Node::is_focused)
        .and_then(|input| input.value());
    assert_eq!(
        focused.as_deref(),
        Some("Needle"),
        "a click on a toggle gives focus back to the search box"
    );
    press(&mut harness, Modifiers::ALT, Key::C);
    settle(&mut harness);
    assert_label(&harness, "4 results in 2 files");
    assert_eq!(
        harness.get_by_label("Aa").accesskit_node().toggled(),
        Some(Toggled::False),
        "Alt+C toggles too"
    );
}

#[test]
fn a_click_on_a_file_collapses_it_and_on_a_match_opens_it() {
    let (mut harness, root) = open("search-click");
    search(&mut harness, "needle");
    harness.get_by_label("lib.rs").click();
    harness.run();
    assert!(harness.query_by_label("lib.rs").is_some());
    assert!(
        harness.query_by_label("pub fn needle() {}").is_none(),
        "the file's matches are hidden"
    );
    harness.get_by_label("fn main() { needle(); }").click();
    harness.run();
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("a tab");
    let document = workspace.document(id).expect("document");
    assert_eq!(document.path(), root.join("src/main.rs"));
    let selection = document.primary_selection().range();
    assert_eq!(
        (selection.start, selection.end),
        (12, 18),
        "the match is selected"
    );
    assert!(
        harness.get_by_role(Role::MultilineTextInput).is_focused(),
        "the editor has focus"
    );
}

#[test]
fn escape_gives_focus_back_to_the_editor() {
    let (mut harness, _) = open("search-escape");
    ctrl_shift_f(&mut harness);
    assert!(focused_input(&harness).is_some());
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.get_by_role(Role::MultilineTextInput).is_focused());
    assert_label(&harness, "SEARCH");
}

#[test]
fn f6_leaves_the_search_view_for_the_editor_and_comes_back_to_it() {
    let (mut harness, _root) = open("f6");
    ctrl_shift_f(&mut harness);
    let search_box = focused_input(&harness).expect("the search box has focus");
    press(&mut harness, Modifiers::NONE, Key::F6);
    assert!(focused_input(&harness).is_none(), "the editor");
    press(&mut harness, Modifiers::NONE, Key::F6);
    assert!(focused_input(&harness).is_none(), "the status bar");
    press(&mut harness, Modifiers::NONE, Key::F6);
    assert_eq!(
        focused_input(&harness),
        Some(search_box),
        "F6 from the status bar goes to the sidebar's search box"
    );
    press(&mut harness, Modifiers::SHIFT, Key::F6);
    assert!(
        focused_input(&harness).is_none(),
        "Shift+F6: the status bar"
    );
}
