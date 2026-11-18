//! The workbench around the editor: the explorer in the sidebar, the tab
//! strip and the status bar, driven by keys and clicks and read back from
//! the AccessKit tree (a tree of tree items, a tab list of tabs, a status
//! node of labels), the window title and image snapshots.

use super::{open_paths, press, status, temp_dir, type_text};
use eframe::egui::{
    Event, Key, Modifiers, MouseWheelUnit, TouchPhase, Vec2, ViewportId, accesskit::Role,
};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::{EditorApp, ROW_HEIGHT};
use std::path::PathBuf;
use std::time::Duration;

/// A window wide enough for the sidebar, the tabs and some text.
const SIZE: Vec2 = Vec2::new(640.0, 360.0);
const CTRL: Modifiers = Modifiers::COMMAND;
const NONE: Modifiers = Modifiers::NONE;

/// A folder named `demo` (a fixed name keeps its header stable in
/// snapshots) with `src/main.rs`, `src/lib.rs`, `README.md` and `.git`.
fn project(name: &str) -> PathBuf {
    let root = temp_dir(name).join("demo");
    for (file, text) in [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub mod x;\n"),
        ("README.md", "# demo\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("fixture");
    }
    std::fs::create_dir_all(root.join(".git")).expect("git dir");
    root
}

/// Files `a.rs`, `b.md` and `c.txt` in a folder of their own.
fn three_files(name: &str) -> Vec<PathBuf> {
    let dir = temp_dir(name);
    ["a.rs", "b.md", "c.txt"]
        .iter()
        .map(|file| {
            let path = dir.join(file);
            std::fs::write(&path, format!("{file}\n")).expect("fixture");
            path
        })
        .collect()
}

fn tree<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::Tree)
}

fn editor<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::MultilineTextInput)
}

/// The explorer rows in view: indented by level, `▸`/`▾` for a collapsed
/// or expanded folder, and ` <` after the selected row.
fn rows(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    tree(harness)
        .query_all_by_role(Role::TreeItem)
        .map(|row| {
            let node = row.accesskit_node();
            let level = node.level().expect("a tree item has a level");
            let marker = match node.data().is_expanded() {
                Some(true) => "▾ ",
                Some(false) => "▸ ",
                None => "  ",
            };
            let selected = if node.is_selected() == Some(true) {
                " <"
            } else {
                ""
            };
            format!(
                "{}{marker}{}{selected}",
                "  ".repeat(level - 1),
                node.label().unwrap_or_default()
            )
        })
        .collect()
}

/// The tabs, in order: their name, with `*` on the selected one and `●`
/// on one with unsaved changes.
fn tabs(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .get_by_role(Role::TabList)
        .query_all_by_role(Role::Tab)
        .map(|tab| {
            let node = tab.accesskit_node();
            let mut text = node.label().unwrap_or_default();
            if node.description().as_deref() == Some("Unsaved changes") {
                text.push_str(" ●");
            }
            if node.is_selected() == Some(true) {
                text.push_str(" *");
            }
            text
        })
        .collect()
}

/// Whether the explorer has keyboard focus: screen readers see it on the
/// tree's selected row, its active descendant.
fn explorer_focused(harness: &Harness<'_, EditorApp>) -> bool {
    harness.query_by_role(Role::Tree).is_some_and(|tree| {
        tree.is_focused()
            || tree
                .query_all_by_role(Role::TreeItem)
                .any(|row| row.is_focused())
    })
}

fn editor_label(harness: &Harness<'_, EditorApp>) -> String {
    editor(harness).accesskit_node().label().unwrap_or_default()
}

fn title(harness: &Harness<'_, EditorApp>) -> String {
    harness.state().window_title()
}

#[test]
fn a_folder_opens_in_the_explorer_with_the_focus_on_its_tree() {
    let root = project("folder");
    let mut harness = open_paths(&[root], SIZE);
    let node = tree(&harness);
    assert_eq!(
        node.accesskit_node().label().as_deref(),
        Some("Files Explorer")
    );
    assert!(
        explorer_focused(&harness),
        "the explorer takes keyboard focus"
    );
    assert_eq!(rows(&harness), ["▸ src <", "  README.md"], ".git is hidden");
    assert_eq!(tabs(&harness), [] as [String; 0]);
    assert!(
        harness.query_by_role(Role::MultilineTextInput).is_none(),
        "no editor without a tab"
    );
    assert!(harness.query_by_value("No file open").is_some());
    assert!(status(&harness).is_empty(), "{:?}", status(&harness));
    assert_eq!(title(&harness), "demo - rustode");
    harness.snapshot("folder");
}

#[test]
fn explorer_keys_expand_open_and_move_between_parts() {
    let root = project("keys");
    let mut harness = open_paths(&[root], SIZE);
    press(&mut harness, NONE, Key::ArrowRight);
    assert_eq!(
        rows(&harness),
        ["▾ src <", "    lib.rs", "    main.rs", "  README.md"]
    );
    press(&mut harness, NONE, Key::ArrowDown);
    press(&mut harness, NONE, Key::ArrowDown);
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(tabs(&harness), ["main.rs *"]);
    assert!(
        editor(&harness).is_focused(),
        "Enter moves focus to the editor"
    );
    assert_eq!(editor_label(&harness), "main.rs");
    assert_eq!(
        status(&harness),
        ["Ln 1, Col 1", "UTF-8", "LF", "Rust"],
        "Enter reached the explorer only"
    );
    press(&mut harness, Modifiers::SHIFT, Key::F6);
    assert!(
        explorer_focused(&harness),
        "Shift+F6 goes back to the explorer"
    );
    press(&mut harness, NONE, Key::ArrowDown);
    press(&mut harness, NONE, Key::Space);
    assert_eq!(tabs(&harness), ["main.rs", "README.md *"]);
    assert!(explorer_focused(&harness), "Space keeps the focus");
    harness.snapshot("explorer_and_tabs");
    press(&mut harness, NONE, Key::ArrowUp);
    press(&mut harness, NONE, Key::ArrowLeft);
    assert_eq!(rows(&harness)[0], "▾ src <", "Left goes to the parent");
    press(&mut harness, NONE, Key::ArrowLeft);
    assert_eq!(rows(&harness), ["▸ src <", "  README.md"], "then collapses");
    press(&mut harness, NONE, Key::End);
    press(&mut harness, NONE, Key::Escape);
    assert!(editor(&harness).is_focused(), "Escape goes to the editor");
    assert_eq!(editor_label(&harness), "README.md");
}

#[test]
fn ctrl_page_down_page_up_and_ctrl_tab_switch_tabs() {
    let mut harness = open_paths(&three_files("switch"), SIZE);
    assert_eq!(tabs(&harness), ["a.rs", "b.md", "c.txt *"]);
    press(&mut harness, CTRL, Key::PageDown);
    assert_eq!(tabs(&harness), ["a.rs *", "b.md", "c.txt"], "wraps around");
    assert_eq!(editor_label(&harness), "a.rs");
    assert_eq!(status(&harness).last().map(String::as_str), Some("Rust"));
    press(&mut harness, CTRL, Key::PageUp);
    assert_eq!(editor_label(&harness), "c.txt");
    press(&mut harness, CTRL, Key::Tab);
    assert_eq!(editor_label(&harness), "a.rs", "the tab used before");
    press(&mut harness, CTRL, Key::Tab);
    assert_eq!(editor_label(&harness), "c.txt");
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::Tab);
    assert_eq!(editor_label(&harness), "b.md", "the tab used longest ago");
    assert!(editor(&harness).is_focused());
}

#[test]
fn ctrl_w_asks_before_closing_unsaved_changes() {
    let paths = three_files("close");
    let mut harness = open_paths(&paths[..2], SIZE);
    type_text(&mut harness, "x");
    assert_eq!(tabs(&harness), ["a.rs", "b.md ● *"]);
    assert_eq!(title(&harness), "\u{25cf} b.md - rustode");
    harness.snapshot("unsaved_tab");
    let prompt = "Unsaved changes: Ctrl+W again closes without saving, Ctrl+S saves";
    press(&mut harness, CTRL, Key::W);
    assert_eq!(status(&harness)[0], prompt);
    assert_eq!(tabs(&harness).len(), 2);
    type_text(&mut harness, "y");
    press(&mut harness, CTRL, Key::W);
    assert_eq!(tabs(&harness).len(), 2, "typing in between asks again");
    press(&mut harness, CTRL, Key::W);
    assert_eq!(tabs(&harness), ["a.rs *"], "the second Ctrl+W discards");
    assert_eq!(
        std::fs::read_to_string(&paths[1]).expect("read"),
        "b.md\n",
        "nothing was saved"
    );
    assert_ne!(status(&harness)[0], prompt);
    press(&mut harness, CTRL, Key::W);
    assert_eq!(tabs(&harness), [] as [String; 0]);
    assert!(harness.query_by_value("No file open").is_some());
    assert_eq!(title(&harness), "rustode");
}

#[test]
fn clicking_a_tab_activates_it_and_its_close_button_closes_it() {
    let paths = three_files("click-tabs");
    let mut harness = open_paths(&paths, SIZE);
    harness.get_by_role_and_label(Role::Tab, "a.rs").click();
    harness.run();
    assert_eq!(tabs(&harness), ["a.rs *", "b.md", "c.txt"]);
    assert!(editor(&harness).is_focused());
    harness
        .get_by_role_and_label(Role::Tab, "b.md")
        .query_by_role(Role::Button)
        .expect("each tab has a close button")
        .click();
    harness.run();
    assert_eq!(tabs(&harness), ["a.rs *", "c.txt"], "b.md closed");
    let closes = harness.query_all_by_role_and_label(Role::Button, "Close");
    assert_eq!(closes.count(), 2, "one close button per tab");
}

#[test]
fn ctrl_b_toggles_the_sidebar_and_hands_its_focus_to_the_editor() {
    let root = project("sidebar");
    let mut harness = open_paths(&[root.clone(), root.join("README.md")], SIZE);
    assert!(editor(&harness).is_focused(), "a file takes the focus");
    assert!(editor(&harness).rect().left() > 100.0, "beside the sidebar");
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::E);
    assert!(explorer_focused(&harness));
    assert_eq!(
        rows(&harness),
        ["▸ src", "  README.md <"],
        "the active file is selected"
    );
    press(&mut harness, CTRL, Key::B);
    assert!(harness.query_by_role(Role::Tree).is_none(), "hidden");
    assert!(editor(&harness).is_focused(), "the editor takes the focus");
    assert!(
        editor(&harness).rect().left() < 1.0,
        "the editor takes the width"
    );
    press(&mut harness, CTRL, Key::B);
    assert!(harness.query_by_role(Role::Tree).is_some(), "shown again");
    assert!(editor(&harness).is_focused(), "the focus stays");
}

#[test]
fn ctrl_shift_e_without_a_folder_says_so() {
    let paths = three_files("no-folder");
    let mut harness = open_paths(&paths[..1], SIZE);
    assert!(harness.query_by_value("NO FOLDER OPENED").is_some());
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::E);
    assert_eq!(status(&harness)[0], "No folder opened");
    assert!(editor(&harness).is_focused());
    press(&mut harness, NONE, Key::End);
    assert_eq!(
        status(&harness)[0],
        "Ln 1, Col 5",
        "a key clears the message"
    );
}

#[test]
fn clicking_explorer_rows_toggles_folders_and_opens_files_keeping_focus() {
    let root = project("click-rows");
    let mut harness = open_paths(&[root], SIZE);
    harness.get_by_role_and_label(Role::TreeItem, "src").click();
    harness.run();
    assert_eq!(
        rows(&harness),
        ["▾ src <", "    lib.rs", "    main.rs", "  README.md"]
    );
    harness
        .get_by_role_and_label(Role::TreeItem, "main.rs")
        .click();
    harness.run();
    assert_eq!(tabs(&harness), ["main.rs *"]);
    assert!(explorer_focused(&harness), "a click keeps the focus here");
    assert_eq!(rows(&harness)[2], "    main.rs <");
}

#[test]
fn the_title_names_the_file_the_folder_and_the_app() {
    let root = project("title");
    let main = root.join("src/main.rs");
    let mut harness = open_paths(&[root, main], SIZE);
    assert_eq!(title(&harness), "main.rs - demo - rustode");
    type_text(&mut harness, "x");
    assert_eq!(title(&harness), "\u{25cf} main.rs - demo - rustode");
    press(&mut harness, CTRL, Key::S);
    assert_eq!(title(&harness), "main.rs - demo - rustode", "Ctrl+S saved");
}

/// A folder of `count` files named `f00`, `f01`, …
fn many_files(name: &str, count: usize) -> PathBuf {
    let root = temp_dir(name).join("many");
    std::fs::create_dir_all(&root).expect("dir");
    for n in 0..count {
        std::fs::write(root.join(format!("f{n:02}")), "").expect("fixture");
    }
    root
}

fn row_labels(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    tree(harness)
        .query_all_by_role(Role::TreeItem)
        .filter_map(|row| row.accesskit_node().label())
        .collect()
}

#[test]
fn only_the_explorer_rows_in_view_are_exposed_and_the_wheel_scrolls_them() {
    let mut harness = open_paths(&[many_files("long", 60)], SIZE);
    let shown = row_labels(&harness);
    assert!(shown.len() < 20, "{shown:?}");
    assert_eq!(shown[0], "f00");
    press(&mut harness, NONE, Key::End);
    let shown = row_labels(&harness);
    assert_eq!(
        shown.last().map(String::as_str),
        Some("f59"),
        "End reveals it"
    );
    assert_eq!(rows(&harness).last().map(String::as_str), Some("  f59 <"));
    let first = |harness: &Harness<'_, EditorApp>| -> usize {
        row_labels(harness)[0][1..]
            .parse()
            .expect("a numbered file")
    };
    let before = first(&harness);
    let rect = tree(&harness).rect();
    harness.hover_at(rect.center());
    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, 8.0 * ROW_HEIGHT),
        phase: TouchPhase::Move,
        modifiers: NONE,
    });
    harness.run();
    assert_eq!(first(&harness), before - 8, "the wheel scrolled 8 rows up");
    assert!(
        rows(&harness).iter().all(|row| !row.ends_with('<')),
        "the selection stays where it was, out of view"
    );
}

#[test]
fn the_workbench_settles_after_input_and_schedules_no_frame() {
    let root = project("idle");
    let mut harness = open_paths(&[root.clone(), root.join("README.md")], SIZE);
    let idle = |harness: &Harness<'_, EditorApp>| {
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay
    };
    assert_eq!(
        idle(&harness),
        Duration::MAX,
        "nothing scheduled after opening"
    );
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::E);
    press(&mut harness, NONE, Key::ArrowRight);
    press(&mut harness, CTRL, Key::B);
    press(&mut harness, CTRL, Key::Tab);
    assert_eq!(
        idle(&harness),
        Duration::MAX,
        "nothing scheduled after input"
    );
}

#[test]
fn the_tab_strip_scrolls_to_keep_the_active_tab_in_view() {
    let dir = temp_dir("many-tabs");
    let paths: Vec<PathBuf> = (0..8)
        .map(|n| {
            let path = dir.join(format!("file{n}.txt"));
            std::fs::write(&path, "").expect("fixture");
            path
        })
        .collect();
    let mut harness = open_paths(&paths, SIZE);
    let strip = harness.get_by_role(Role::TabList).rect();
    let tab = |harness: &Harness<'_, EditorApp>, name: &str| {
        harness.get_by_role_and_label(Role::Tab, name).rect()
    };
    let last = tab(&harness, "file7.txt");
    assert!(last.right() <= strip.right() + 0.5, "{last:?} in {strip:?}");
    assert!(tab(&harness, "file0.txt").left() < strip.left(), "scrolled");
    press(&mut harness, CTRL, Key::PageDown);
    let first = tab(&harness, "file0.txt");
    assert!(
        (first.left() - strip.left()).abs() < 0.5,
        "{first:?} in {strip:?}"
    );
}
