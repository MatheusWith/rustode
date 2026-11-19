//! Workspace seam: what a frontend needs to draw and drive an editor —
//! line access, status-bar data, cursor movement, Enter and Tab.

use rustode_workspace::{DocumentId, Eol, Movement, Position, Selection, Workspace};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-view-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open_named(name: &str, file: &str, text: &str) -> (Workspace, DocumentId) {
    let path = dir(name).join(file);
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id)
}

fn open(name: &str, text: &str) -> (Workspace, DocumentId) {
    open_named(name, "f.txt", text)
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("document").text()
}

fn cursor(ws: &Workspace, id: DocumentId) -> usize {
    ws.document(id).expect("document").cursor()
}

fn moves(ws: &mut Workspace, id: DocumentId, movement: Movement, select: bool) {
    ws.move_cursor(id, movement, select).expect("move");
}

#[test]
fn lines_are_read_one_at_a_time_without_line_breaks() {
    let (ws, id) = open("lines", "one\r\ntwo\nthree");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.line_count(), 3);
    assert_eq!(doc.line(0), "one");
    assert_eq!(doc.line(1), "two");
    assert_eq!(doc.line(2), "three");
}

#[test]
fn only_cr_lf_and_crlf_break_lines_as_in_vs_code() {
    // VS Code splits lines on \r\n, \r and \n only (pieceTreeBase.ts:37-62);
    // U+2028, U+2029 and U+0085 stay inside the line.
    let (ws, id) = open("separators", "a\u{2028}b\u{2029}c\u{85}d\re");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.line_count(), 2);
    assert_eq!(doc.line(0), "a\u{2028}b\u{2029}c\u{85}d");
}

#[test]
fn status_bar_data_comes_from_the_document() {
    let (mut ws, id) = open_named("status", "main.rs", "fn main() {\n    x\n}\n");
    ws.set_cursor(id, 16).expect("cursor");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.cursor_position(), Position::new(1, 4));
    assert_eq!(doc.language_name(), "Rust");
    assert_eq!(doc.language_id(), "rust");
    assert_eq!(doc.eol(), Eol::Lf);
    assert_eq!(
        doc.offset_of(Position::new(1, 99)),
        17,
        "columns clamp to the line"
    );
    assert_eq!(doc.position_of(17), Position::new(1, 5));
}

#[test]
fn languages_follow_the_apps_built_in_extensions() {
    for (file, name) in [
        ("a.h", "C++"),
        ("a.c", "C"),
        ("Dockerfile", "Docker"),
        ("Makefile", "Makefile"),
        // TOML is rustode's own addition: the App has no built-in TOML language.
        ("Cargo.toml", "TOML"),
        ("a.txt", "Plain Text"),
        ("notes", "Plain Text"),
        ("x.JSON", "JSON"),
        ("tsconfig.jsonc", "JSON with Comments"),
    ] {
        let (ws, id) = open_named(&format!("lang-{}", file.replace('.', "_")), file, "");
        assert_eq!(
            ws.document(id).expect("doc").language_name(),
            name,
            "{file}"
        );
    }
}

#[test]
fn left_and_right_step_over_crlf_and_collapse_selections() {
    let (mut ws, id) = open("left-right", "ab\r\ncd");
    ws.set_cursor(id, 2).expect("cursor");
    moves(&mut ws, id, Movement::Right, false);
    assert_eq!(cursor(&ws, id), 4, "a CRLF is one step");
    moves(&mut ws, id, Movement::Left, false);
    assert_eq!(cursor(&ws, id), 2);
    moves(&mut ws, id, Movement::Left, true);
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.selections(), [Selection::new(2, 1)]);
    moves(&mut ws, id, Movement::Right, false);
    assert_eq!(cursor(&ws, id), 2, "Right collapses a selection to its end");
}

#[test]
fn up_and_down_keep_the_column_across_short_lines() {
    // VS Code's leftoverVisibleColumns (cursorMoveOperations.ts:157-187).
    let (mut ws, id) = open("vertical", "abcdef\nab\nabcdef");
    ws.set_cursor(id, 5).expect("cursor");
    moves(&mut ws, id, Movement::Down, false);
    assert_eq!(cursor(&ws, id), 9, "clamped to the short line's end");
    moves(&mut ws, id, Movement::Down, false);
    assert_eq!(cursor(&ws, id), 15, "back to column 5");
    moves(&mut ws, id, Movement::Down, false);
    assert_eq!(cursor(&ws, id), 16, "past the last line: end of document");
    ws.set_cursor(id, 3).expect("cursor");
    moves(&mut ws, id, Movement::Up, false);
    assert_eq!(
        cursor(&ws, id),
        0,
        "above the first line: start of document"
    );
    ws.set_cursor(id, 15).expect("cursor");
    moves(&mut ws, id, Movement::PageUp(2), false);
    assert_eq!(cursor(&ws, id), 5);
}

#[test]
fn home_toggles_between_first_non_blank_and_column_zero() {
    let (mut ws, id) = open("home", "    let x;");
    ws.set_cursor(id, 8).expect("cursor");
    moves(&mut ws, id, Movement::LineStart, false);
    assert_eq!(cursor(&ws, id), 4);
    moves(&mut ws, id, Movement::LineStart, false);
    assert_eq!(cursor(&ws, id), 0);
    moves(&mut ws, id, Movement::LineEnd, false);
    assert_eq!(cursor(&ws, id), 10);
    moves(&mut ws, id, Movement::DocumentStart, true);
    assert_eq!(
        ws.document(id).expect("doc").selections(),
        [Selection::new(10, 0)]
    );
}

#[test]
fn enter_keeps_indentation_and_starts_an_undo_step_that_typing_joins() {
    let (mut ws, id) = open("enter", "    abc");
    ws.set_cursor(id, 7).expect("cursor");
    ws.type_text(id, "d").expect("type");
    ws.enter(id).expect("enter");
    for ch in ["e", "f"] {
        ws.type_text(id, ch).expect("type");
    }
    assert_eq!(text(&ws, id), "    abcd\n    ef");
    ws.undo(id).expect("undo");
    assert_eq!(
        text(&ws, id),
        "    abcd",
        "Enter and the typing after it undo together"
    );
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "    abc");
}

#[test]
fn enter_inserts_the_documents_line_ending() {
    let (mut ws, id) = open("enter-crlf", "a\r\nb");
    ws.set_cursor(id, 1).expect("cursor");
    ws.enter(id).expect("enter");
    assert_eq!(text(&ws, id), "a\r\n\r\nb");
    ws.type_text(id, "\n").expect("type");
    assert_eq!(
        text(&ws, id),
        "a\r\n\r\n\r\nb",
        "typed line breaks follow the document"
    );
}

#[test]
fn tab_jumps_to_the_next_tab_stop_using_the_settings() {
    let (mut ws, id) = open_named("tab", "main.rs", "ab");
    ws.set_cursor(id, 2).expect("cursor");
    ws.tab(id).expect("tab");
    assert_eq!(text(&ws, id), "ab  ", "default tabSize 4 with spaces");
    ws.undo(id).expect("undo");

    let config = dir("tab-config");
    let settings = config.join("settings.json");
    std::fs::write(
        &settings,
        r#"{ "editor.tabSize": 3, "[rust]": { "editor.insertSpaces": false } }"#,
    )
    .expect("settings");
    let _ = ws.load_config(&settings, Path::new("/nonexistent/keybindings.json"));
    ws.tab(id).expect("tab");
    assert_eq!(text(&ws, id), "ab\t", "the [rust] override wins");
}

#[test]
fn tab_is_its_own_undo_step() {
    let (mut ws, id) = open("tab-undo", "");
    for ch in ["a", "b"] {
        ws.type_text(id, ch).expect("type");
    }
    ws.tab(id).expect("tab");
    ws.type_text(id, "c").expect("type");
    assert_eq!(text(&ws, id), "ab  c");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "ab  ");
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "ab");
}

#[test]
fn undoing_a_line_ending_change_restores_the_line_ending() {
    let (mut ws, id) = open("eol-undo", "a\nb");
    ws.set_eol(id, Eol::Crlf).expect("eol");
    assert_eq!(ws.document(id).expect("doc").eol(), Eol::Crlf);
    ws.undo(id).expect("undo");
    assert_eq!(text(&ws, id), "a\nb");
    assert_eq!(ws.document(id).expect("doc").eol(), Eol::Lf);
    ws.redo(id).expect("redo");
    assert_eq!(ws.document(id).expect("doc").eol(), Eol::Crlf);
}

#[test]
fn a_reload_from_disk_takes_the_files_line_ending() {
    let path = dir("eol-reload").join("f.txt");
    std::fs::write(&path, "a\nb").expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    std::fs::write(&path, "a\r\nb\r\n").expect("external change");
    ws.check_disk(id).expect("check");
    assert_eq!(text(&ws, id), "a\r\nb\r\n");
    assert_eq!(ws.document(id).expect("doc").eol(), Eol::Crlf);
    ws.undo(id).expect("undo");
    assert_eq!(ws.document(id).expect("doc").eol(), Eol::Lf);
}

#[test]
fn the_status_bar_shows_encodings_as_vs_code_does() {
    let (ws, id) = open("enc-utf8", "a");
    assert_eq!(ws.document(id).expect("doc").encoding_name(), "UTF-8");
    let path = dir("enc-bom").join("f.txt");
    std::fs::write(&path, b"\xEF\xBB\xBFa").expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    assert_eq!(
        ws.document(id).expect("doc").encoding_name(),
        "UTF-8 with BOM"
    );
    assert_eq!(ws.tab_size(id).expect("tab size"), 4);
}

#[test]
fn escape_and_select_all_go_through_the_workspace() {
    let (mut ws, id) = open("escape", "abc");
    ws.select_all(id).expect("select all");
    assert_eq!(
        ws.document(id).expect("doc").selections(),
        [Selection::new(0, 3)]
    );
    assert!(ws.cancel_selections(id).expect("escape"));
    assert_eq!(
        ws.document(id).expect("doc").selections(),
        [Selection::cursor(3)]
    );
}

#[test]
fn the_status_column_expands_tabs_like_vs_code() {
    let (mut ws, id) = open("status-col", "\tab\n");
    ws.set_cursor(id, 2).expect("cursor");
    assert_eq!(ws.status_line_column(id).expect("status"), (1, 6));
    ws.set_cursor(id, 4).expect("cursor");
    assert_eq!(ws.status_line_column(id).expect("status"), (2, 1));
}
