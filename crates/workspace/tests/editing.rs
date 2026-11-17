//! Workspace seam: typing, deleting, programmatic edits, undo and redo.

use rustode_workspace::{DocumentId, Event, Range, Workspace};
use std::path::PathBuf;

fn open(name: &str, text: &str) -> (Workspace, DocumentId, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-edit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id, path)
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("document").text()
}

fn type_str(ws: &mut Workspace, id: DocumentId, s: &str) {
    for ch in s.chars() {
        ws.type_text(id, &ch.to_string()).expect("type");
    }
}

#[test]
fn typing_inserts_at_the_cursor_and_moves_it() {
    let (mut ws, id, _) = open("type", "ac");
    ws.set_cursor(id, 1).expect("cursor");
    ws.type_text(id, "b").expect("type");
    assert_eq!(text(&ws, id), "abc");
    assert_eq!(ws.document(id).expect("doc").cursor(), 2);
}

#[test]
fn a_space_starts_an_undo_step_that_the_next_word_joins_like_vs_code() {
    // Mirrors VS Code's `cursor.test.ts` "inserts undo stop when typing space" (:6113)
    // and "there is a single undo stop for consecutive whitespaces" (:6192).
    let (mut ws, id, _) = open("words", "");
    type_str(&mut ws, id, "abc d");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "abc");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "");

    type_str(&mut ws, id, "ab  cd");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "ab  ");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "ab");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "");
}

#[test]
fn switching_between_typing_and_deleting_starts_a_new_undo_step() {
    let (mut ws, id, _) = open("switch", "");
    type_str(&mut ws, id, "hello");
    ws.delete_left(id).expect("delete");
    ws.delete_left(id).expect("delete");
    type_str(&mut ws, id, "p!");
    assert_eq!(text(&ws, id), "help!");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "hel");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "hello");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "");
}

#[test]
fn moving_the_cursor_starts_a_new_undo_step() {
    let (mut ws, id, _) = open("move", "");
    type_str(&mut ws, id, "ab");
    ws.set_cursor(id, 0).expect("cursor");
    type_str(&mut ws, id, "x");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "ab");
    assert_eq!(ws.document(id).expect("doc").cursor(), 0);
}

#[test]
fn delete_right_and_programmatic_replace_are_their_own_steps() {
    let (mut ws, id, _) = open("replace", "hello world");
    ws.set_cursor(id, 0).expect("cursor");
    ws.delete_right(id).expect("delete");
    ws.delete_right(id).expect("delete");
    ws.replace(id, Range::new(4, 9), "WORLD").expect("replace");
    assert_eq!(text(&ws, id), "llo WORLD");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "llo world");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "hello world");
}

#[test]
fn redo_reapplies_and_a_new_edit_clears_it() {
    let (mut ws, id, _) = open("redo", "");
    type_str(&mut ws, id, "ab");
    ws.undo(id).expect("undo");
    ws.redo(id).expect("redo");
    assert_eq!(text(&ws, id), "ab");
    ws.undo(id).expect("undo");
    type_str(&mut ws, id, "z");
    assert!(
        !ws.redo(id).expect("redo"),
        "nothing to redo after a new edit"
    );
    assert_eq!(text(&ws, id), "z");
}

#[test]
fn undo_back_to_the_saved_state_is_clean_again() {
    let (mut ws, id, _) = open("dirty", "a");
    assert!(!ws.document(id).expect("doc").is_dirty());
    ws.set_cursor(id, 1).expect("cursor");
    type_str(&mut ws, id, "b");
    assert!(ws.document(id).expect("doc").is_dirty());
    ws.undo(id).expect("undo");
    assert!(!ws.document(id).expect("doc").is_dirty());
}

#[test]
fn edits_emit_events_and_out_of_range_edits_are_refused() {
    let (mut ws, id, _) = open("events", "abc");
    let _ = ws.take_events();
    ws.replace(id, Range::new(0, 1), "X").expect("replace");
    assert_eq!(ws.take_events(), [Event::DocumentChanged { id }]);
    assert!(ws.replace(id, Range::new(2, 99), "").is_err());
    assert!(ws.set_cursor(id, 4).is_err());
    assert_eq!(text(&ws, id), "Xbc");
}

#[test]
fn any_edit_sequence_followed_by_full_undo_restores_the_original() {
    let original = "fn main() {\r\n    println!(\"olá\");\n}\n";
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % (bound as u64)).expect("fits")
    };
    for round in 0..50 {
        let (mut ws, id, _) = open(&format!("prop{round}"), original);
        for _ in 0..40 {
            let len = ws.document(id).expect("doc").len_chars();
            match next(5) {
                0 => ws.set_cursor(id, next(len + 1)).expect("cursor"),
                1 => ws
                    .type_text(id, ["a", " ", "\n", "é", "日"][next(5)])
                    .expect("type"),
                2 => ws.delete_left(id).expect("delete"),
                3 => ws.delete_right(id).expect("delete"),
                _ => {
                    let start = next(len + 1);
                    let end = start + next(len - start + 1);
                    ws.replace(id, Range::new(start, end), "<r>")
                        .expect("replace");
                }
            }
        }
        while ws.undo(id).expect("undo") {}
        assert_eq!(text(&ws, id), original, "round {round}");
        assert!(!ws.document(id).expect("doc").is_dirty(), "round {round}");
    }
}
