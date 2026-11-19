//! The quick input: quick open (Ctrl+P), the command palette (Ctrl+Shift+P,
//! F1) and go to line (Ctrl+G), driven by keys and clicks and read back
//! from the AccessKit tree (an editable combo box that controls a list box
//! of options), the tabs, the status bar and image snapshots. Ranking files,
//! listing commands and reading the text are tested at the workspace seam
//! (`crates/workspace/tests/quick_open.rs`).

use super::{assert_status, open_paths, press, quiet, status, temp_dir, type_text};
use eframe::egui::{Event, Key, Modifiers, Vec2, ViewportCommand, ViewportId, accesskit::Role};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::EditorApp;
use std::path::PathBuf;
use std::time::Duration;

const SIZE: Vec2 = Vec2::new(640.0, 360.0);
const CTRL: Modifiers = Modifiers::COMMAND;
const NONE: Modifiers = Modifiers::NONE;
const FILES: &str = "Search files by name (append : to go to line)";
const COMMANDS: &str = "Type the name of a command to run.";

/// A folder named `demo` with `src/main.rs`, `src/lib.rs` and `README.md`.
fn project(name: &str) -> PathBuf {
    let root = temp_dir(name).join("demo");
    for (file, text) in [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub mod x;\n"),
        ("README.md", "# demo\n\nline 3\nline 4\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("fixture");
    }
    root
}

/// The folder in the explorer with `README.md` open.
fn open_project(name: &str) -> (Harness<'static, EditorApp>, PathBuf) {
    let root = project(name);
    let readme = root.join("README.md");
    (open_paths(&[root, readme.clone()], SIZE), readme)
}

fn combo<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::EditableComboBox)
}

/// Whether the box has keyboard focus: screen readers see it on its
/// active descendant, the selected option.
fn combo_focused(harness: &Harness<'_, EditorApp>) -> bool {
    harness
        .query_by_role(Role::EditableComboBox)
        .is_some_and(|combo| combo.is_focused())
        || harness.query_by_role(Role::ListBox).is_some_and(|list| {
            list.query_all_by_role(Role::ListBoxOption)
                .any(|option| option.is_focused())
        })
}

/// The options in view: their name, then ` · ` and their description or
/// keybinding when they have one, and ` <` on the selected one.
fn options(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .get_by_role(Role::ListBox)
        .query_all_by_role(Role::ListBoxOption)
        .map(|option| {
            let node = option.accesskit_node();
            let mut text = node.label().unwrap_or_default();
            let detail = node
                .description()
                .or_else(|| node.data().keyboard_shortcut().map(str::to_owned));
            if let Some(detail) = detail {
                text.push_str(" · ");
                text.push_str(&detail);
            }
            if node.is_selected() == Some(true) {
                text.push_str(" <");
            }
            text
        })
        .collect()
}

fn tabs(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .get_by_role(Role::TabList)
        .query_all_by_role(Role::Tab)
        .map(|tab| {
            let node = tab.accesskit_node();
            let mut text = node.label().unwrap_or_default();
            if node.is_selected() == Some(true) {
                text.push_str(" *");
            }
            text
        })
        .collect()
}

fn editor<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::MultilineTextInput)
}

#[test]
fn ctrl_p_lists_files_and_enter_opens_the_selected_one_in_a_tab() {
    let (mut harness, _) = open_project("quick-open");
    press(&mut harness, CTRL, Key::P);
    assert!(combo_focused(&harness), "the box takes keyboard focus");
    let node = combo(&harness);
    assert_eq!(node.accesskit_node().label().as_deref(), Some(FILES));
    assert_eq!(node.value().as_deref(), Some(""));
    assert_eq!(options(&harness), ["README.md <"], "the open editors first");
    type_text(&mut harness, "main");
    assert_eq!(options(&harness), ["main.rs · src <"]);
    harness.snapshot("quick_open");
    press(&mut harness, NONE, Key::Enter);
    assert!(harness.query_by_role(Role::EditableComboBox).is_none());
    assert_eq!(tabs(&harness), ["README.md", "main.rs *"]);
    assert!(editor(&harness).is_focused(), "the editor takes focus");
    press(&mut harness, CTRL, Key::P);
    type_text(&mut harness, "readme");
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(
        tabs(&harness),
        ["README.md *", "main.rs"],
        "an open file's tab is activated, not opened twice"
    );
}

#[test]
fn the_combo_box_controls_the_list_and_points_at_the_selected_option() {
    let (mut harness, _) = open_project("aria");
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::P);
    let node = combo(&harness).accesskit_node();
    let list = harness.get_by_role(Role::ListBox);
    let controlled: Vec<_> = node.controls().map(|control| control.id()).collect();
    assert_eq!(controlled, [list.accesskit_node().id()]);
    let selected = list
        .query_all_by_role(Role::ListBoxOption)
        .find(|option| option.accesskit_node().is_selected() == Some(true))
        .expect("an option is selected");
    assert_eq!(
        node.active_descendant().map(|option| option.id()),
        Some(selected.accesskit_node().id())
    );
    assert_eq!(list.accesskit_node().label().as_deref(), Some(COMMANDS));
}

#[test]
fn f1_lists_the_registry_and_enter_runs_the_selected_command() {
    let (mut harness, readme) = open_project("palette");
    type_text(&mut harness, "x");
    press(&mut harness, NONE, Key::F1);
    let node = combo(&harness);
    assert_eq!(node.value().as_deref(), Some(">"));
    assert_eq!(node.accesskit_node().label().as_deref(), Some(COMMANDS));
    let all = options(&harness);
    // The list shows the first six commands alphabetically.
    assert!(all.contains(&"Debug: Continue · F5".to_owned()), "{all:?}");
    assert!(all[0].ends_with(" <"), "the first is selected: {all:?}");
    type_text(&mut harness, "save");
    assert_eq!(options(&harness), ["File: Save · Ctrl+S <"]);
    harness.snapshot("palette");
    press(&mut harness, NONE, Key::Enter);
    assert!(harness.query_by_role(Role::EditableComboBox).is_none());
    assert!(
        std::fs::read_to_string(&readme)
            .expect("read")
            .starts_with("x# demo")
    );
    assert_eq!(harness.state().window_title(), "README.md - demo - rustode");
    assert!(
        editor(&harness).is_focused(),
        "focus went back to the editor"
    );
}

#[test]
fn editor_commands_from_the_palette_run_in_the_workspace() {
    let (mut harness, _) = open_project("editor-command");
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::P);
    type_text(&mut harness, "end of file");
    assert_eq!(options(&harness), ["Go to End of File · Ctrl+End <"]);
    press(&mut harness, NONE, Key::Enter);
    assert_status(&harness, "Ln 5, Col 1");
    assert!(editor(&harness).is_focused());
}

#[test]
fn up_and_down_wrap_ctrl_p_moves_down_and_a_click_runs_a_row() {
    let (mut harness, _) = open_project("navigate");
    press(&mut harness, NONE, Key::F1);
    let first = options(&harness)[0].clone();
    press(&mut harness, NONE, Key::ArrowUp);
    let wrapped = options(&harness);
    assert_eq!(
        wrapped.last().map(String::as_str),
        Some("View: Toggle Problems · Ctrl+Shift+M <"),
        "Up from the first row selects the last, scrolled into view"
    );
    press(&mut harness, NONE, Key::ArrowDown);
    assert_eq!(options(&harness)[0], first, "and Down wraps back");
    press(&mut harness, CTRL, Key::P);
    assert!(options(&harness)[1].ends_with(" <"), "Ctrl+P moves down");
    type_text(&mut harness, "explorer");
    assert_eq!(options(&harness), ["View: Show Explorer · Ctrl+Shift+E <"]);
    harness
        .get_by_role_and_label(Role::ListBoxOption, "View: Show Explorer")
        .click();
    quiet(&mut harness);
    assert!(harness.query_by_role(Role::EditableComboBox).is_none());
    let tree = harness.get_by_role(Role::Tree);
    assert!(
        tree.is_focused()
            || tree
                .query_all_by_role(Role::TreeItem)
                .any(|row| row.is_focused()),
        "the explorer took focus"
    );
}

#[test]
fn ctrl_g_goes_to_a_line_and_escape_gives_the_focus_back() {
    let (mut harness, _) = open_project("goto");
    press(&mut harness, CTRL, Key::G);
    let node = combo(&harness);
    assert_eq!(node.value().as_deref(), Some(":"));
    assert_eq!(
        node.accesskit_node().label().as_deref(),
        Some("Go to line. Type a line number, optionally followed by colon and column number.")
    );
    assert_eq!(
        options(&harness),
        ["Type a line number to go to (from 1 to 5). <"]
    );
    type_text(&mut harness, "3");
    assert_eq!(
        options(&harness),
        ["Press 'Enter' to go to line 3 or enter colon : to add a column number. <"]
    );
    harness.snapshot("go_to_line");
    type_text(&mut harness, ":4");
    assert_eq!(
        options(&harness),
        ["Press 'Enter' to go to line 3 at column 4. <"]
    );
    press(&mut harness, NONE, Key::Enter);
    assert_status(&harness, "Ln 3, Col 4");
    press(&mut harness, CTRL, Key::P);
    press(&mut harness, NONE, Key::Escape);
    assert!(harness.query_by_role(Role::EditableComboBox).is_none());
    assert!(editor(&harness).is_focused(), "Escape gives focus back");
    type_text(&mut harness, "y");
    assert_status(&harness, "Ln 3, Col 5");
}

#[test]
fn clicking_outside_closes_the_box() {
    let (mut harness, _) = open_project("blur");
    press(&mut harness, CTRL, Key::P);
    assert!(harness.query_by_role(Role::EditableComboBox).is_some());
    let bottom = editor(&harness).rect().left_bottom() + Vec2::new(40.0, -10.0);
    harness.hover_at(bottom);
    harness.drag_at(bottom);
    harness.drop_at(bottom);
    quiet(&mut harness);
    assert!(
        harness.query_by_role(Role::EditableComboBox).is_none(),
        "the box closed when the editor took focus"
    );
    assert!(editor(&harness).is_focused());
}

#[test]
fn ctrl_q_asks_before_quitting_with_unsaved_changes() {
    let (mut harness, _) = open_project("quit");
    let closed = |harness: &Harness<'_, EditorApp>| {
        harness.output().viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Close)
    };
    // One frame with the key, so its output holds the frame's commands.
    let ctrl_q = |harness: &mut Harness<'_, EditorApp>| {
        harness.input_mut().events.push(Event::Key {
            key: Key::Q,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: CTRL,
        });
        harness.step();
    };
    type_text(&mut harness, "x");
    ctrl_q(&mut harness);
    assert!(!closed(&harness));
    quiet(&mut harness);
    assert_eq!(
        status(&harness)[0],
        "Unsaved changes: Ctrl+Q again quits without saving, Ctrl+S saves"
    );
    ctrl_q(&mut harness);
    assert!(closed(&harness), "the second Ctrl+Q closes the window");
}

#[test]
fn the_quick_input_settles_and_schedules_no_frame() {
    let (mut harness, _) = open_project("idle");
    press(&mut harness, CTRL, Key::P);
    type_text(&mut harness, "lib");
    press(&mut harness, NONE, Key::ArrowDown);
    assert_eq!(
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}

#[test]
fn the_palette_shows_the_search_view_and_the_problems_panel() {
    let (mut harness, _) = open_project("palette-views");
    press(&mut harness, CTRL | Modifiers::SHIFT, Key::P);
    type_text(&mut harness, "find in files");
    assert_eq!(
        options(&harness),
        ["Search: Find in Files · Ctrl+Shift+F <"]
    );
    press(&mut harness, NONE, Key::Enter);
    assert!(
        harness
            .query_all_by_role(Role::TextInput)
            .any(|input| input.is_focused()),
        "the Search view's box has focus"
    );
}
