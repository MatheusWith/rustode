//! Input to frame: comparing the file on disk with the unsaved text after a
//! save hit a conflict. The diff is the workspace's
//! (`crates/workspace/tests/conflict_compare.rs`); these tests check what the
//! window draws and what its buttons and keys do.

use super::{fixture, open_paths, press, quiet, type_text};
use eframe::egui::{Key, Modifiers, Vec2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const COMPARE: &str = "workbench.files.action.resolveConflict";

/// A window on `f.txt` (`base`), with `mine` typed over everything and the
/// file changed to `disk` behind it, the conflict raised.
fn conflicted(
    name: &str,
    base: &str,
    mine: &str,
    disk: &str,
) -> (Harness<'static, EditorApp>, PathBuf) {
    let path = fixture(name, "f.txt", base);
    let mut harness = open_paths(std::slice::from_ref(&path), Vec2::new(900.0, 360.0));
    press(&mut harness, Modifiers::COMMAND, Key::A);
    type_text(&mut harness, mine);
    std::fs::write(&path, disk).expect("external write");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let workspace = harness.state_mut().workspace_mut();
        workspace.poll_external_changes();
        let id = workspace.active_document().expect("document");
        if workspace
            .document(id)
            .is_some_and(rustode_workspace::Document::has_conflict)
        {
            break;
        }
        assert!(Instant::now() < deadline, "no conflict raised");
        std::thread::sleep(Duration::from_millis(20));
    }
    quiet(&mut harness);
    (harness, path)
}

fn simple(name: &str) -> (Harness<'static, EditorApp>, PathBuf) {
    conflicted(name, "one\n", "mine one\n", "theirs\n")
}

fn open_compare(harness: &mut Harness<'static, EditorApp>) {
    press(harness, Modifiers::COMMAND, Key::S);
    harness
        .get_by_role_and_label(Role::Button, "Compare")
        .click();
    quiet(harness);
}

fn compare_is_open(harness: &Harness<'_, EditorApp>) -> bool {
    harness
        .query_by_label_contains("(in file) ↔ f.txt (in rustode)")
        .is_some()
}

#[test]
fn the_prompt_offers_compare_and_it_opens_the_two_texts_side_by_side() {
    let (mut harness, path) = simple("cmp-open");
    press(&mut harness, Modifiers::COMMAND, Key::S);
    harness.get_by_role_and_label(Role::Button, "Compare");
    harness.get_by_role_and_label(Role::Button, "Overwrite");
    harness.get_by_role_and_label(Role::Button, "Revert");
    harness
        .get_by_role_and_label(Role::Button, "Compare")
        .click();
    quiet(&mut harness);
    assert!(compare_is_open(&harness));
    harness.get_by_label_contains("f.txt (in file) ↔ f.txt (in rustode) - Resolve save conflict");
    harness.get_by_label_contains("-     1  theirs");
    harness.get_by_label_contains("+     1  mine one");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Compare")
            .is_none(),
        "the prompt is gone"
    );
    harness.snapshot("compare_side_by_side");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        b"theirs\n",
        "nothing written"
    );
}

#[test]
fn the_command_opens_it_from_the_palette_or_a_keybinding() {
    let (mut harness, _) = simple("cmp-cmd");
    assert!(
        harness
            .state_mut()
            .workspace_mut()
            .execute(COMPARE)
            .expect("compare")
    );
    quiet(&mut harness);
    assert!(compare_is_open(&harness));
}

#[test]
fn overwrite_in_the_comparison_writes_my_text_and_closes_it() {
    let (mut harness, path) = simple("cmp-overwrite");
    open_compare(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "Overwrite")
        .click();
    quiet(&mut harness);
    assert!(!compare_is_open(&harness));
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
}

#[test]
fn revert_in_the_comparison_loads_the_file_and_closes_it() {
    let (mut harness, path) = simple("cmp-revert");
    open_compare(&mut harness);
    harness
        .get_by_role_and_label(Role::Button, "Revert")
        .click();
    quiet(&mut harness);
    assert!(!compare_is_open(&harness));
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    assert_eq!(workspace.document(id).expect("doc").text(), "theirs\n");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}

#[test]
fn close_and_escape_keep_the_conflict() {
    for (name, by_key) in [("cmp-close", false), ("cmp-esc", true)] {
        let (mut harness, path) = simple(name);
        open_compare(&mut harness);
        if by_key {
            press(&mut harness, Modifiers::NONE, Key::Escape);
        } else {
            harness
                .get_by_role_and_label(Role::Button, "Close comparison")
                .click();
            quiet(&mut harness);
        }
        assert!(!compare_is_open(&harness), "{name}");
        let workspace = harness.state().workspace();
        let id = workspace.active_document().expect("document");
        assert!(
            workspace.document(id).expect("doc").has_conflict(),
            "{name}"
        );
        assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n", "{name}");
    }
}

#[test]
fn keys_meant_for_the_comparison_do_not_type_into_the_document() {
    let (mut harness, _) = simple("cmp-keys");
    open_compare(&mut harness);
    type_text(&mut harness, "zzz");
    press(&mut harness, Modifiers::NONE, Key::N);
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    assert_eq!(workspace.document(id).expect("doc").text(), "mine one\n");
}

#[test]
fn n_and_p_go_to_the_next_and_previous_change() {
    let base = (1..=60)
        .map(|n| format!("line {n}\n"))
        .collect::<Vec<_>>()
        .concat();
    let mine = base
        .replace("line 3\n", "mine 3\n")
        .replace("line 50\n", "mine 50\n");
    let (mut harness, _) = conflicted(
        "cmp-nav",
        &base,
        &mine,
        &base.replace("line 50\n", "disk 50\n"),
    );
    open_compare(&mut harness);
    harness.get_by_label_contains("change 1/2");
    harness.get_by_label_contains("mine 3");
    assert!(
        harness.query_by_label_contains("mine 50").is_none(),
        "far down, not drawn"
    );
    press(&mut harness, Modifiers::NONE, Key::N);
    harness.get_by_label_contains("change 2/2");
    harness.get_by_label_contains("mine 50");
    harness.get_by_label_contains("disk 50");
    press(&mut harness, Modifiers::NONE, Key::P);
    harness.get_by_label_contains("change 1/2");
}

#[test]
fn a_file_over_the_limit_shows_the_message_and_still_resolves() {
    let big = "l\n".repeat(rustode_workspace::COMPARE_MAX_LINES + 1);
    let (mut harness, path) = conflicted("cmp-big", "one\n", "mine\n", &big);
    open_compare(&mut harness);
    harness.get_by_label_contains("too large to compare");
    harness
        .get_by_role_and_label(Role::Button, "Overwrite")
        .click();
    quiet(&mut harness);
    assert_eq!(std::fs::read(&path).expect("read"), b"mine\n");
}
