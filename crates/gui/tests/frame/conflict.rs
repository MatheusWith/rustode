//! Input to frame: saving a document whose file changed under its unsaved
//! edits opens a prompt with the workspace's choices (VS Code's Overwrite and
//! Revert, registered commands); nothing is written before a choice. The
//! decision is the workspace's (`crates/workspace/tests/external_changes.rs`).

use super::{fixture, open, press, quiet};
use eframe::egui::{Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn conflicted(name: &str) -> (Harness<'static, EditorApp>, PathBuf) {
    let path = fixture(name, "f.txt", "one\n");
    let mut harness = open(&path);
    super::type_text(&mut harness, "mine ");
    std::fs::write(&path, "theirs\n").expect("external write");
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

fn prompt_is_open(harness: &Harness<'_, EditorApp>) -> bool {
    harness
        .query_by_role_and_label(Role::Button, "Overwrite")
        .is_some()
        && harness
            .query_by_role_and_label(Role::Button, "Revert")
            .is_some()
}

#[test]
fn saving_in_conflict_opens_the_prompt_and_writes_nothing() {
    let (mut harness, path) = conflicted("conf-prompt");
    assert!(!prompt_is_open(&harness));
    press(&mut harness, Modifiers::COMMAND, Key::S);
    assert!(prompt_is_open(&harness));
    harness.get_by_label_contains("Failed to save 'f.txt'");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    quiet(&mut harness);
    assert!(!prompt_is_open(&harness), "Cancel keeps editing");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}

#[test]
fn overwrite_writes_my_text() {
    let (mut harness, path) = conflicted("conf-overwrite");
    press(&mut harness, Modifiers::COMMAND, Key::S);
    harness
        .get_by_role_and_label(Role::Button, "Overwrite")
        .click();
    quiet(&mut harness);
    assert!(!prompt_is_open(&harness));
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
}

#[test]
fn revert_loads_the_files_text() {
    let (mut harness, path) = conflicted("conf-revert");
    press(&mut harness, Modifiers::COMMAND, Key::S);
    harness
        .get_by_role_and_label(Role::Button, "Revert")
        .click();
    quiet(&mut harness);
    assert!(!prompt_is_open(&harness));
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    assert_eq!(workspace.document(id).expect("doc").text(), "theirs\n");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}
