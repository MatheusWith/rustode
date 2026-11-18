//! Every part reached and worked by keyboard alone, as in VS Code: F6 and
//! Shift+F6 cycle the parts, Tab and Shift+Tab move through them in order,
//! the active tab and the status bar take arrows, Ctrl+M lets Tab leave the
//! editor, and the empty editor group takes focus. Read from the AccessKit
//! tree's focus, which is what screen readers follow, and from snapshots
//! of the focus outlines.

use super::{open_paths, press, status, temp_dir, type_text};
use eframe::egui::{Key, Modifiers, Vec2, accesskit::Role};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::EditorApp;
use std::path::PathBuf;

const SIZE: Vec2 = Vec2::new(640.0, 300.0);
const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;
const CTRL: Modifiers = Modifiers::COMMAND;

/// A folder named `demo` with `a.rs` and `b.rs`, both open, `b.rs` active.
fn project(name: &str) -> Vec<PathBuf> {
    let root = temp_dir(name).join("demo");
    std::fs::create_dir_all(&root).expect("dir");
    let mut paths = vec![root.clone()];
    for file in ["a.rs", "b.rs"] {
        let path = root.join(file);
        std::fs::write(&path, "fn main() {}\n").expect("fixture");
        paths.push(path);
    }
    paths
}

/// What has keyboard focus, as screen readers see it: the focused node, or
/// the active descendant it points at.
fn focused(harness: &Harness<'_, EditorApp>) -> String {
    let node: Node<'_> = harness
        .query_all(egui_kittest::kittest::by())
        .find(Node::is_focused)
        .expect("something has focus");
    let data = node.accesskit_node();
    let name = data.label().or_else(|| data.value()).unwrap_or_default();
    format!("{:?} {name}", data.role())
}

fn text(harness: &Harness<'_, EditorApp>) -> String {
    harness
        .get_by_role(Role::MultilineTextInput)
        .accesskit_node()
        .document_range()
        .text()
}

#[test]
fn f6_and_shift_f6_cycle_editor_status_bar_and_sidebar_as_vs_code_does() {
    let mut harness = open_paths(&project("f6"), SIZE);
    assert_eq!(focused(&harness), "MultilineTextInput b.rs");
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "TreeItem b.rs", "the explorer's row");
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "MultilineTextInput b.rs");
    press(&mut harness, SHIFT, Key::F6);
    assert_eq!(focused(&harness), "TreeItem b.rs");
    press(&mut harness, SHIFT, Key::F6);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, SHIFT, Key::F6);
    assert_eq!(focused(&harness), "MultilineTextInput b.rs");
    press(&mut harness, CTRL, Key::B);
    press(&mut harness, NONE, Key::F6);
    press(&mut harness, NONE, Key::F6);
    assert_eq!(
        focused(&harness),
        "MultilineTextInput b.rs",
        "a hidden sidebar is skipped"
    );
}

#[test]
fn f6_from_the_tab_strip_counts_as_the_editor_part() {
    let mut harness = open_paths(&project("f6-tabs"), SIZE);
    press(&mut harness, SHIFT, Key::F6);
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(focused(&harness), "Tab b.rs");
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "Status ");
}

#[test]
fn tab_and_shift_tab_move_through_the_parts_in_order() {
    let mut harness = open_paths(&project("tab-order"), SIZE);
    press(&mut harness, CTRL | SHIFT, Key::E);
    assert_eq!(focused(&harness), "TreeItem b.rs");
    let forward = ["Tab b.rs", "Button Close", "MultilineTextInput b.rs"];
    for expected in forward {
        press(&mut harness, NONE, Key::Tab);
        assert_eq!(focused(&harness), expected);
    }
    assert_eq!(
        text(&harness),
        "fn main() {}\n",
        "Tab into the editor typed nothing"
    );
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(
        focused(&harness),
        "MultilineTextInput b.rs",
        "the editor keeps Tab"
    );
    assert_eq!(text(&harness), "    fn main() {}\n", "and indents with it");
    press(&mut harness, CTRL, Key::Z);
    press(&mut harness, CTRL, Key::M);
    assert!(status(&harness).contains(&"Tab Moves Focus".to_owned()));
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(focused(&harness), "TreeItem b.rs", "around to the start");
    for expected in [
        "Status ",
        "MultilineTextInput b.rs",
        "Button Close",
        "Tab b.rs",
        "TreeItem b.rs",
    ] {
        press(&mut harness, SHIFT, Key::Tab);
        assert_eq!(focused(&harness), expected);
    }
    assert_eq!(text(&harness), "fn main() {}\n");
    press(&mut harness, CTRL, Key::M);
    assert!(!status(&harness).contains(&"Tab Moves Focus".to_owned()));
}

#[test]
fn the_active_tab_takes_arrows_home_end_and_enter() {
    let root = temp_dir("tab-keys").join("demo");
    std::fs::create_dir_all(&root).expect("dir");
    let files: Vec<PathBuf> = ["a.rs", "b.rs", "c.rs"]
        .iter()
        .map(|file| {
            let path = root.join(file);
            std::fs::write(&path, format!("// {file}\n")).expect("fixture");
            path
        })
        .collect();
    let mut harness = open_paths(&files, SIZE);
    press(&mut harness, CTRL, Key::M);
    press(&mut harness, SHIFT, Key::Tab);
    press(&mut harness, SHIFT, Key::Tab);
    press(&mut harness, CTRL, Key::M);
    assert_eq!(focused(&harness), "Tab c.rs");
    harness.snapshot("focus_tab");
    press(&mut harness, NONE, Key::ArrowLeft);
    assert_eq!(focused(&harness), "Tab b.rs", "the focus follows");
    assert_eq!(text(&harness), "// b.rs\n", "b.rs is active");
    press(&mut harness, NONE, Key::Home);
    assert_eq!(focused(&harness), "Tab a.rs");
    press(&mut harness, NONE, Key::ArrowUp);
    assert_eq!(focused(&harness), "Tab a.rs", "no wrapping");
    press(&mut harness, NONE, Key::End);
    assert_eq!(focused(&harness), "Tab c.rs");
    press(&mut harness, NONE, Key::ArrowLeft);
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(
        focused(&harness),
        "MultilineTextInput b.rs",
        "Enter opens it"
    );
}

#[test]
fn the_status_bar_takes_arrows_home_end_and_escape() {
    let mut harness = open_paths(&project("status-keys"), SIZE);
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, NONE, Key::ArrowRight);
    assert_eq!(focused(&harness), "Label Ln 1, Col 1");
    press(&mut harness, NONE, Key::ArrowRight);
    assert_eq!(focused(&harness), "Label UTF-8");
    harness.snapshot("focus_status_item");
    press(&mut harness, NONE, Key::End);
    assert_eq!(focused(&harness), "Label Rust");
    press(&mut harness, NONE, Key::ArrowRight);
    assert_eq!(focused(&harness), "Label Ln 1, Col 1", "wraps around");
    press(&mut harness, NONE, Key::ArrowLeft);
    assert_eq!(focused(&harness), "Label Rust");
    press(&mut harness, NONE, Key::Home);
    assert_eq!(focused(&harness), "Label Ln 1, Col 1");
    press(&mut harness, NONE, Key::Escape);
    assert_eq!(focused(&harness), "Status ", "Escape leaves the item");
    press(&mut harness, NONE, Key::Escape);
    assert_eq!(focused(&harness), "MultilineTextInput b.rs", "then the bar");
}

#[test]
fn the_empty_editor_group_takes_focus() {
    let dir = temp_dir("empty-group");
    let file = dir.join("only.txt");
    std::fs::write(&file, "x\n").expect("fixture");
    let mut harness = open_paths(&[file], SIZE);
    press(&mut harness, CTRL, Key::W);
    assert_eq!(focused(&harness), "Group Editor Group 1 (empty)");
    harness.snapshot("focus_empty_group");
    press(&mut harness, NONE, Key::F6);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, SHIFT, Key::F6);
    assert_eq!(focused(&harness), "Group Editor Group 1 (empty)");
    type_text(&mut harness, "typed nowhere");
    assert_eq!(focused(&harness), "Group Editor Group 1 (empty)");
}

#[test]
fn a_window_with_nothing_open_focuses_the_empty_group() {
    let mut harness = open_paths(&[], SIZE);
    assert_eq!(focused(&harness), "Group Editor Group 1 (empty)");
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(focused(&harness), "Status ");
    press(&mut harness, NONE, Key::Tab);
    assert_eq!(focused(&harness), "Group Editor Group 1 (empty)");
}
