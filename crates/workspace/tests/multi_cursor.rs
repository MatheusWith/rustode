//! Workspace seam: several cursors, add next occurrence, add cursor
//! above/below and box selection; edits at every cursor undo as one step.

use rustode_workspace::{DocumentId, Position, Selection, Workspace};

fn open(name: &str, text: &str) -> (Workspace, DocumentId) {
    let dir = std::env::temp_dir().join(format!("rustode-multi-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id)
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("document").text()
}

fn selections(ws: &Workspace, id: DocumentId) -> Vec<Selection> {
    ws.document(id).expect("document").selections().to_vec()
}

#[test]
fn typing_at_several_cursors_undoes_as_one_step() {
    let (mut ws, id) = open("type", "a\nb\nc");
    let cursors = vec![
        Selection::cursor(1),
        Selection::cursor(3),
        Selection::cursor(5),
    ];
    ws.set_selections(id, cursors, 0).expect("select");
    for ch in ["x", "y"] {
        ws.type_text(id, ch).expect("type");
    }
    assert_eq!(text(&ws, id), "axy\nbxy\ncxy");
    assert_eq!(
        selections(&ws, id),
        [
            Selection::cursor(3),
            Selection::cursor(7),
            Selection::cursor(11)
        ]
    );
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "a\nb\nc");
    assert_eq!(
        selections(&ws, id),
        [
            Selection::cursor(1),
            Selection::cursor(3),
            Selection::cursor(5)
        ]
    );
    ws.redo(id).expect("redo");
    assert_eq!(text(&ws, id), "axy\nbxy\ncxy");
}

#[test]
fn typing_replaces_every_selection() {
    let (mut ws, id) = open("replace-sel", "foo bar foo");
    let list = vec![Selection::new(0, 3), Selection::new(8, 11)];
    ws.set_selections(id, list, 1).expect("select");
    ws.type_text(id, "q").expect("type");
    assert_eq!(text(&ws, id), "q bar q");
    ws.delete_left(id).expect("delete");
    assert_eq!(text(&ws, id), " bar ");
}

#[test]
fn add_next_occurrence_selects_the_word_then_its_next_matches() {
    let (mut ws, id) = open("ctrl-d", "foo bar foo baz foo");
    ws.set_cursor(id, 9).expect("cursor");
    assert!(ws.add_next_occurrence(id).expect("ctrl+d"));
    assert_eq!(selections(&ws, id), [Selection::new(8, 11)]);
    assert!(ws.add_next_occurrence(id).expect("ctrl+d"));
    assert!(
        ws.add_next_occurrence(id).expect("ctrl+d"),
        "wraps to the start"
    );
    assert_eq!(
        selections(&ws, id),
        [
            Selection::new(0, 3),
            Selection::new(8, 11),
            Selection::new(16, 19)
        ]
    );
    assert!(
        !ws.add_next_occurrence(id).expect("ctrl+d"),
        "every match is selected"
    );
    ws.type_text(id, "x").expect("type");
    assert_eq!(text(&ws, id), "x bar x baz x");
}

#[test]
fn add_cursor_below_and_above_keep_the_column_or_clamp_to_the_line() {
    let (mut ws, id) = open("vertical", "abcd\nab\nabcdef");
    ws.set_cursor(id, 3).expect("cursor");
    assert!(ws.add_cursor_below(id).expect("below"));
    assert_eq!(
        selections(&ws, id),
        [Selection::cursor(3), Selection::cursor(7)]
    );
    assert!(ws.add_cursor_below(id).expect("below"));
    assert_eq!(
        selections(&ws, id),
        [
            Selection::cursor(3),
            Selection::cursor(7),
            Selection::cursor(10)
        ]
    );
    ws.set_cursor(id, 0).expect("cursor");
    assert!(
        !ws.add_cursor_above(id).expect("above"),
        "nothing above the first line"
    );
}

#[test]
fn box_selection_skips_short_lines_like_vs_code() {
    let (mut ws, id) = open("box", "abcdef\nab\nabcdef");
    ws.column_select(id, Position::new(0, 3), Position::new(2, 5))
        .expect("box");
    assert_eq!(
        selections(&ws, id),
        [Selection::new(3, 5), Selection::new(13, 15)]
    );
    ws.type_text(id, "_").expect("type");
    assert_eq!(text(&ws, id), "abc_f\nab\nabc_f");
}

#[test]
fn box_selection_past_every_line_puts_cursors_at_line_ends() {
    let (mut ws, id) = open("box-end", "ab\nabc");
    ws.column_select(id, Position::new(0, 5), Position::new(1, 8))
        .expect("box");
    assert_eq!(
        selections(&ws, id),
        [Selection::cursor(2), Selection::cursor(6)]
    );
}

#[test]
fn cursors_that_meet_are_merged() {
    let (mut ws, id) = open("merge", "abc");
    ws.set_selections(id, vec![Selection::cursor(1), Selection::cursor(2)], 0)
        .expect("select");
    ws.delete_left(id).expect("delete");
    ws.delete_left(id).expect("delete");
    assert_eq!(text(&ws, id), "c");
    assert_eq!(selections(&ws, id), [Selection::cursor(0)]);
}
