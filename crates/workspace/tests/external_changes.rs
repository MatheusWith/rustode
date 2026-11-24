//! Workspace seam: files changed, deleted or renamed by other programs.
//!
//! The watcher is real, so each test waits for its events with a bounded
//! loop instead of a fixed sleep.

use rustode_workspace::{DocumentId, Event, Resolution, Workspace};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

fn open(name: &str, text: &str) -> (Workspace, DocumentId, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-ext-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    ws.take_events();
    (ws, id, path)
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("document").text()
}

/// Polls the watcher until an event matching `want` arrives; returns every
/// event seen up to and including it.
fn wait_for(ws: &mut Workspace, want: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        ws.poll_external_changes();
        seen.extend(ws.take_events());
        if seen.iter().any(&want) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no matching event within {WAIT:?}; saw {seen:?}");
}

/// Polls for a while and returns whatever events arrived.
fn settle(ws: &mut Workspace) -> Vec<Event> {
    let mut seen = Vec::new();
    for _ in 0..25 {
        ws.poll_external_changes();
        seen.extend(ws.take_events());
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

fn write_externally(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("external write");
}

#[test]
fn a_clean_document_reloads_when_its_file_changes() {
    let (mut ws, id, path) = open("reload", "one\n");
    write_externally(&path, b"two\n");
    let events = wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    assert!(
        events.contains(&Event::DocumentChanged { id }),
        "{events:?}"
    );
    assert_eq!(text(&ws, id), "two\n");
    assert!(!ws.document(id).expect("doc").is_dirty());
}

#[test]
fn a_reload_is_undoable_and_undo_makes_the_document_dirty() {
    let (mut ws, id, path) = open("undo-reload", "one\n");
    write_externally(&path, b"two\n");
    wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(text(&ws, id), "one\n");
    assert!(
        ws.document(id).expect("doc").is_dirty(),
        "the old text no longer matches the file"
    );
    assert!(ws.redo(id).expect("redo"));
    assert!(!ws.document(id).expect("doc").is_dirty());
}

#[test]
fn a_reload_keeps_the_cursor_before_the_changed_text() {
    let (mut ws, id, path) = open("cursor", "abc\ndef\n");
    ws.set_cursor(id, 2).expect("cursor");
    write_externally(&path, b"abc\nXYZ\n");
    wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    assert_eq!(ws.document(id).expect("doc").cursor(), 2);
}

#[test]
fn a_reload_picks_up_a_new_encoding() {
    let (mut ws, id, path) = open("encoding", "a\n");
    write_externally(&path, &[0xEF, 0xBB, 0xBF, b'b', b'\n']);
    wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.text(), "b\n");
    assert!(doc.has_bom());
}

#[test]
fn a_dirty_document_raises_a_conflict_and_keeps_its_text() {
    let (mut ws, id, path) = open("conflict", "one\n");
    ws.type_text(id, "mine ").expect("type");
    write_externally(&path, b"theirs\n");
    let events = wait_for(&mut ws, |e| *e == Event::Conflict { id });
    assert!(!events.contains(&Event::Reloaded { id }), "{events:?}");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.text(), "mine one\n");
    assert!(doc.is_dirty());
    assert!(doc.has_conflict());
}

#[test]
fn saving_during_a_conflict_is_refused_until_it_is_resolved() {
    let (mut ws, id, path) = open("refuse", "one\n");
    ws.type_text(id, "mine ").expect("type");
    write_externally(&path, b"theirs\n");
    wait_for(&mut ws, |e| *e == Event::Conflict { id });
    let err = ws.save(id).expect_err("conflict");
    assert!(err.to_string().contains("changed on disk"), "{err}");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}

#[test]
fn keeping_mine_resolves_the_conflict_and_saving_overwrites_theirs() {
    let (mut ws, id, path) = open("keep-mine", "one\n");
    ws.type_text(id, "mine ").expect("type");
    write_externally(&path, b"theirs\n");
    wait_for(&mut ws, |e| *e == Event::Conflict { id });
    ws.resolve_conflict(id, Resolution::KeepMine)
        .expect("keep mine");
    let doc = ws.document(id).expect("doc");
    assert!(!doc.has_conflict());
    assert!(doc.is_dirty(), "my text is still unsaved");
    assert_eq!(doc.text(), "mine one\n");
    ws.save(id).expect("save");
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
}

#[test]
fn reloading_theirs_replaces_my_text_as_an_undoable_step() {
    let (mut ws, id, path) = open("theirs", "one\n");
    ws.type_text(id, "mine ").expect("type");
    write_externally(&path, b"theirs\n");
    wait_for(&mut ws, |e| *e == Event::Conflict { id });
    ws.resolve_conflict(id, Resolution::ReloadTheirs)
        .expect("reload theirs");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.text(), "theirs\n");
    assert!(!doc.is_dirty());
    assert!(!doc.has_conflict());
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(text(&ws, id), "mine one\n", "my work is not lost");
}

#[test]
fn resolving_without_a_conflict_is_an_error() {
    let (mut ws, id, _path) = open("no-conflict", "one\n");
    assert!(ws.resolve_conflict(id, Resolution::KeepMine).is_err());
}

#[test]
fn a_touch_or_our_own_save_is_not_an_external_change() {
    let (mut ws, id, path) = open("touch", "one\n");
    write_externally(&path, b"one\n");
    ws.type_text(id, "x").expect("type");
    ws.save(id).expect("save");
    ws.type_text(id, "y").expect("type");
    let events = settle(&mut ws);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Conflict { .. } | Event::Reloaded { .. })),
        "{events:?}"
    );
    assert!(!ws.document(id).expect("doc").has_conflict());
}

#[test]
fn a_change_back_to_my_text_clears_the_dirty_state_instead_of_conflicting() {
    let (mut ws, id, path) = open("same", "one\n");
    ws.type_text(id, "x").expect("type");
    write_externally(&path, b"xone\n");
    let deadline = Instant::now() + WAIT;
    while ws.document(id).expect("doc").is_dirty() {
        assert!(Instant::now() < deadline, "never became clean");
        ws.poll_external_changes();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!ws.take_events().contains(&Event::Conflict { id }));
}

#[test]
fn a_deleted_file_orphans_the_document_without_losing_its_text() {
    let (mut ws, id, path) = open("delete", "one\n");
    ws.type_text(id, "mine ").expect("type");
    std::fs::remove_file(&path).expect("delete");
    wait_for(&mut ws, |e| *e == Event::Deleted { id });
    let doc = ws.document(id).expect("doc");
    assert!(doc.is_orphaned());
    assert_eq!(doc.text(), "mine one\n");
    ws.save(id).expect("save recreates the file");
    assert!(!ws.document(id).expect("doc").is_orphaned());
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
}

#[test]
fn a_deleted_file_that_comes_back_reloads_a_clean_document() {
    let (mut ws, id, path) = open("restore", "one\n");
    std::fs::remove_file(&path).expect("delete");
    wait_for(&mut ws, |e| *e == Event::Deleted { id });
    assert_eq!(text(&ws, id), "one\n", "a clean orphan keeps its text");
    write_externally(&path, b"back\n");
    let events = wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    assert!(events.contains(&Event::Restored { id }), "{events:?}");
    assert!(!ws.document(id).expect("doc").is_orphaned());
    assert_eq!(text(&ws, id), "back\n");
}

#[test]
fn a_renamed_file_reports_where_it_went() {
    let (mut ws, id, path) = open("rename", "one\n");
    let to = path.with_file_name("g.txt");
    std::fs::rename(&path, &to).expect("rename");
    wait_for(&mut ws, |e| *e == Event::Renamed { id, to: to.clone() });
    let doc = ws.document(id).expect("doc");
    assert!(doc.is_orphaned(), "the document stays on its old path");
    assert_eq!(doc.path(), path);
    assert_eq!(doc.text(), "one\n");
}

#[test]
fn an_editor_style_atomic_replace_is_a_change_not_a_delete() {
    let (mut ws, id, path) = open("atomic", "one\n");
    let temp = path.with_file_name("f.txt.tmp");
    std::fs::write(&temp, b"two\n").expect("temp");
    std::fs::rename(&temp, &path).expect("replace");
    let events = wait_for(&mut ws, |e| *e == Event::Reloaded { id });
    assert!(!events.contains(&Event::Deleted { id }), "{events:?}");
    assert!(!ws.document(id).expect("doc").is_orphaned());
    assert_eq!(text(&ws, id), "two\n");
}

#[test]
fn checking_one_document_works_without_waiting_for_the_watcher() {
    let (mut ws, id, path) = open("check", "one\n");
    write_externally(&path, b"two\n");
    ws.check_disk(id).expect("check");
    assert_eq!(text(&ws, id), "two\n");
    assert!(ws.take_events().contains(&Event::Reloaded { id }));
}

fn conflicted(name: &str) -> (Workspace, DocumentId, PathBuf) {
    let (mut ws, id, path) = open(name, "one\n");
    ws.type_text(id, "mine ").expect("type");
    write_externally(&path, b"theirs\n");
    wait_for(&mut ws, |e| *e == Event::Conflict { id });
    (ws, id, path)
}

#[test]
fn the_conflict_commands_are_registered_with_vs_codes_ids_and_a_category() {
    let find = |id: &str| {
        rustode_workspace::COMMANDS
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("{id} is not registered"))
    };
    assert_eq!(
        find("workbench.files.action.saveIgnoreModifiedSince").label,
        "File: Overwrite"
    );
    assert_eq!(
        find("workbench.files.action.revertLocalChanges").label,
        "File: Discard Your Changes and Revert to File Contents"
    );
}

#[test]
fn the_overwrite_command_resolves_the_conflict_in_favor_of_my_text_and_saves() {
    let (mut ws, id, path) = conflicted("cmd-overwrite");
    assert!(ws.save(id).is_err(), "a plain save is refused");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    assert!(
        ws.execute("workbench.files.action.saveIgnoreModifiedSince")
            .expect("overwrite")
    );
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
    let doc = ws.document(id).expect("doc");
    assert!(!doc.has_conflict() && !doc.is_dirty());
}

#[test]
fn the_revert_command_loads_the_file_and_keeps_my_text_in_undo() {
    let (mut ws, id, path) = conflicted("cmd-revert");
    assert!(
        ws.execute("workbench.files.action.revertLocalChanges")
            .expect("revert")
    );
    assert_eq!(text(&ws, id), "theirs\n");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(text(&ws, id), "mine one\n");
}

#[test]
fn a_conflict_command_without_a_conflict_says_which_file_has_none() {
    let (mut ws, _id, _path) = open("cmd-none", "one\n");
    for command in [
        "workbench.files.action.saveIgnoreModifiedSince",
        "workbench.files.action.revertLocalChanges",
    ] {
        let err = ws.execute(command).expect_err("no conflict");
        assert_eq!(err.to_string(), "f.txt has no conflict to resolve");
    }
}

#[test]
fn the_prompt_for_a_conflict_offers_the_commands_and_nothing_overwrites_before_a_choice() {
    let (mut ws, id, path) = conflicted("prompt");
    assert!(ws.conflict_prompt(id).is_some(), "a conflict has a prompt");
    let err = ws.save(id).expect_err("refused");
    assert!(matches!(err, rustode_workspace::Error::Conflict { .. }));
    let prompt = ws.conflict_prompt(id).expect("prompt");
    assert_eq!(
        prompt.message,
        "Failed to save 'f.txt': The content of the file is newer. Please compare your version \
         with the file contents or overwrite the content of the file with your changes."
    );
    let ids: Vec<_> = prompt.choices.iter().map(|c| c.command).collect();
    assert_eq!(
        ids,
        [
            "workbench.files.action.resolveConflict",
            "workbench.files.action.saveIgnoreModifiedSince",
            "workbench.files.action.revertLocalChanges"
        ]
    );
    let names: Vec<_> = prompt.choices.iter().map(|c| c.label).collect();
    assert_eq!(names, ["Compare", "Overwrite", "Revert"]);
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    ws.resolve_conflict(id, Resolution::KeepMine).expect("keep");
    assert!(ws.conflict_prompt(id).is_none(), "resolved: no prompt");
}
