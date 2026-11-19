//! Workspace seam: editor groups, splits, shared documents and per-view
//! cursors.

use rustode_workspace::{
    Axis, Direction, DocumentId, Error, GroupId, Layout, Range, Selection, SplitDirection,
    Workspace,
};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-groups-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn file(name: &str, f: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("rustode-groups-{}-{name}", std::process::id()))
        .join(f)
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).expect("write");
}

/// A workspace with `files` open in order, each holding its own name.
fn open(name: &str, files: &[&str]) -> (Workspace, Vec<DocumentId>) {
    let root = dir(name);
    let mut ws = Workspace::new();
    let ids = files
        .iter()
        .map(|f| {
            write(&root.join(f), &format!("{f} text"));
            ws.open(&root.join(f)).expect("open")
        })
        .collect();
    (ws, ids)
}

fn run(ws: &mut Workspace, id: &str) {
    assert!(matches!(ws.execute(id), Ok(true)), "{id}");
}

fn group_tabs(ws: &Workspace, group: GroupId) -> Vec<DocumentId> {
    ws.group_by_id(group).expect("group").tabs().to_vec()
}

#[test]
fn a_new_workspace_has_one_empty_group() {
    let ws = Workspace::new();
    assert_eq!(ws.group_ids().len(), 1);
    assert_eq!(ws.layout(), &Layout::Group(ws.active_group()));
    assert_eq!(ws.active_document(), None);
}

#[test]
fn splitting_right_copies_the_active_editor_into_a_new_focused_group() {
    let (mut ws, ids) = open("split", &["a.txt", "b.txt"]);
    let first = ws.active_group();
    let second = ws.split_group(SplitDirection::Right);
    assert_ne!(first, second);
    assert_eq!(ws.active_group(), second, "the new group takes the focus");
    assert_eq!(group_tabs(&ws, second), [ids[1]]);
    assert_eq!(
        group_tabs(&ws, first),
        ids,
        "the first group keeps its tabs"
    );
    assert_eq!(
        ws.layout(),
        &Layout::Split {
            axis: Axis::Horizontal,
            children: vec![Layout::Group(first), Layout::Group(second)]
        }
    );
    assert_eq!(ws.group_ids(), [first, second]);
    assert_eq!(ws.active_document(), Some(ids[1]));
    assert_eq!(ws.tabs(), [ids[1]], "tabs() is the active group's");
}

#[test]
fn splits_nest_and_same_axis_splits_join_their_parent() {
    let (mut ws, _) = open("nest", &["a.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    let c = ws.split_group(SplitDirection::Right);
    assert_eq!(
        ws.layout(),
        &Layout::Split {
            axis: Axis::Horizontal,
            children: vec![Layout::Group(a), Layout::Group(b), Layout::Group(c)]
        },
        "three columns, not a tree of two-way splits"
    );
    let d = ws.split_group(SplitDirection::Down);
    assert_eq!(
        ws.layout(),
        &Layout::Split {
            axis: Axis::Horizontal,
            children: vec![
                Layout::Group(a),
                Layout::Group(b),
                Layout::Split {
                    axis: Axis::Vertical,
                    children: vec![Layout::Group(c), Layout::Group(d)]
                }
            ]
        }
    );
    assert_eq!(ws.group_ids(), [a, b, c, d]);
}

#[test]
fn one_document_is_shown_by_two_groups_and_an_edit_shows_in_both() {
    let (mut ws, ids) = open("shared", &["a.txt"]);
    let left = ws.active_group();
    let _right = ws.split_group(SplitDirection::Right);
    ws.type_text(ids[0], "X").expect("type");
    ws.focus_group(left).expect("focus");
    assert_eq!(ws.document(ids[0]).expect("doc").text(), "Xa.txt text");
    assert_eq!(ws.tabs(), [ids[0]]);
    assert_eq!(
        ws.document(ids[0])
            .map(rustode_workspace::Document::is_dirty),
        Some(true)
    );
}

#[test]
fn each_view_has_its_own_cursors_and_selections() {
    let (mut ws, ids) = open("own", &["a.txt"]);
    let doc = ids[0];
    let left = ws.active_group();
    ws.set_cursor(doc, 1).expect("cursor");
    let right = ws.split_group(SplitDirection::Right);
    ws.set_selections(doc, vec![Selection::new(2, 4), Selection::cursor(6)], 1)
        .expect("selections");
    ws.focus_group(left).expect("focus left");
    assert_eq!(
        ws.document(doc).expect("doc").cursor(),
        1,
        "left kept its cursor"
    );
    assert_eq!(ws.document(doc).expect("doc").selections().len(), 1);
    // The inactive view is queried by group.
    let seen = ws.view_selections(right, doc).expect("right view");
    assert_eq!(
        seen.as_slice(),
        [Selection::new(2, 4), Selection::cursor(6)]
    );
    assert_eq!(seen.primary_index(), 1);
    assert_eq!(ws.view_cursor(left, doc), Some(1));
    assert_eq!(ws.view_cursor(right, doc), Some(6));
    ws.focus_group(right).expect("focus right");
    assert_eq!(ws.document(doc).expect("doc").selections().len(), 2);
    assert_eq!(ws.view_cursor(left, doc), Some(1));
}

#[test]
fn typing_above_the_other_view_keeps_its_cursor_on_the_same_text() {
    let (mut ws, ids) = open("above", &["a.txt"]);
    let doc = ids[0];
    ws.set_cursor(doc, 4).expect("cursor");
    let left = ws.active_group();
    let right = ws.split_group(SplitDirection::Right);
    // Right view: cursor at 4 ("a.tx|t text"). Left view moves to the start and types.
    ws.focus_group(left).expect("left");
    ws.set_cursor(doc, 0).expect("home");
    ws.type_text(doc, "abc").expect("type");
    assert_eq!(
        ws.view_cursor(right, doc),
        Some(7),
        "moved by the 3 typed chars"
    );
    ws.focus_group(right).expect("right");
    let document = ws.document(doc).expect("doc");
    assert_eq!(document.cursor(), 7);
    assert_eq!(&document.text()[7..8], "t", "still on the same text");
}

#[test]
fn deleting_across_the_other_views_cursor_moves_it_to_the_deletion() {
    let (mut ws, ids) = open("delete", &["a.txt"]);
    let doc = ids[0];
    ws.set_selections(doc, vec![Selection::new(3, 5)], 0)
        .expect("select");
    let left = ws.active_group();
    let right = ws.split_group(SplitDirection::Right);
    ws.focus_group(left).expect("left");
    ws.replace(doc, Range::new(1, 7), "").expect("delete");
    assert_eq!(ws.document(doc).expect("doc").text(), "aext");
    assert_eq!(
        ws.view_selections(right, doc).expect("view").as_slice(),
        [Selection::cursor(1)],
        "the selection collapsed where the text went"
    );
    // After the end: a deletion before the cursor shifts it left.
    let (mut ws, ids) = open("delete2", &["a.txt"]);
    let doc = ids[0];
    ws.set_cursor(doc, 8).expect("cursor");
    let left = ws.active_group();
    let right = ws.split_group(SplitDirection::Right);
    ws.focus_group(left).expect("left");
    ws.replace(doc, Range::new(0, 2), "").expect("delete");
    assert_eq!(ws.view_cursor(right, doc), Some(6));
}

#[test]
fn typing_at_the_other_views_cursor_pushes_it_after_the_text() {
    let (mut ws, ids) = open("same", &["a.txt"]);
    let doc = ids[0];
    ws.set_cursor(doc, 2).expect("cursor");
    let left = ws.active_group();
    let right = ws.split_group(SplitDirection::Right);
    ws.focus_group(left).expect("left");
    ws.type_text(doc, "ZZ").expect("type");
    assert_eq!(ws.view_cursor(right, doc), Some(4));
}

#[test]
fn undo_is_per_document_and_leaves_the_other_view_valid() {
    let (mut ws, ids) = open("undo", &["a.txt"]);
    let doc = ids[0];
    let left = ws.active_group();
    ws.set_cursor(doc, 0).expect("home");
    let right = ws.split_group(SplitDirection::Right);
    ws.set_cursor(doc, 8).expect("end");
    ws.focus_group(left).expect("left");
    ws.type_text(doc, "hello ").expect("type");
    assert_eq!(ws.view_cursor(right, doc), Some(14));
    // Undoing in the typing view takes its cursor back and the other view
    // stays on its text.
    assert!(ws.undo(doc).expect("undo"));
    assert_eq!(ws.document(doc).expect("doc").text(), "a.txt text");
    assert_eq!(ws.view_cursor(left, doc), Some(0));
    assert_eq!(
        ws.view_cursor(right, doc),
        Some(8),
        "the right cursor is back on its text"
    );
    // Redo from the other view: one history for the document.
    ws.focus_group(right).expect("right");
    assert!(ws.redo(doc).expect("redo"));
    assert_eq!(ws.document(doc).expect("doc").text(), "hello a.txt text");
    let len = ws.document(doc).expect("doc").len_chars();
    for group in [left, right] {
        assert!(ws.view_cursor(group, doc).expect("view") <= len);
    }
    assert_eq!(
        ws.view_cursor(left, doc),
        Some(6),
        "an empty cursor where text comes back ends up after it"
    );
}

#[test]
fn closing_one_view_keeps_the_document_open_for_the_other() {
    let (mut ws, ids) = open("close", &["a.txt"]);
    let doc = ids[0];
    let left = ws.active_group();
    let right = ws.split_group(SplitDirection::Right);
    ws.type_text(doc, "x").expect("dirty");
    ws.close_editor(right, doc, false)
        .expect("closes the view only");
    assert!(ws.document(doc).is_some(), "still open in the left group");
    assert_eq!(ws.group_ids(), [left], "the emptied group closed");
    assert_eq!(ws.active_group(), left);
    // The last view of a dirty document refuses without discard.
    assert!(matches!(ws.close_editor(left, doc, false), Err(Error::Dirty(d)) if d == doc));
    ws.close_editor(left, doc, true).expect("discard");
    assert!(ws.document(doc).is_none());
    assert_eq!(ws.group_ids().len(), 1, "the last group stays, empty");
}

#[test]
fn closing_the_last_editor_of_a_group_closes_the_group_and_refocuses_the_previous() {
    let (mut ws, ids) = open("last", &["a.txt", "b.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    let c = ws.split_group(SplitDirection::Right);
    ws.focus_group(b).expect("b");
    ws.focus_group(c).expect("c");
    ws.close_editor(c, ids[1], false).expect("close in c");
    assert_eq!(ws.group_ids(), [a, b]);
    assert_eq!(ws.active_group(), b, "the most recently active group");
}

#[test]
fn closing_a_document_removes_it_from_every_group() {
    let (mut ws, ids) = open("everywhere", &["a.txt", "b.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    ws.close(ids[1], true).expect("close b.txt");
    assert!(ws.document(ids[1]).is_none());
    assert_eq!(group_tabs(&ws, a), [ids[0]]);
    assert_eq!(
        ws.group_ids(),
        [a],
        "the second group only had b.txt, so it closed"
    );
    assert_eq!(b, b);
}

#[test]
fn closing_a_group_moves_its_editors_to_the_most_recently_active_one() {
    let (mut ws, ids) = open("closegroup", &["a.txt", "b.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    ws.open(&file("closegroup", "a.txt")).expect("a.txt in b");
    assert_eq!(group_tabs(&ws, b), [ids[1], ids[0]]);
    assert!(ws.close_group(b).expect("close"));
    assert_eq!(ws.group_ids(), [a]);
    assert_eq!(group_tabs(&ws, a), ids);
    assert!(ws.document(ids[0]).is_some() && ws.document(ids[1]).is_some());
    assert!(
        !ws.close_group(a).expect("close"),
        "the last group never closes"
    );
}

#[test]
fn closing_all_editors_in_a_group_closes_the_group() {
    let (mut ws, ids) = open("closeall", &["a.txt", "b.txt", "c.txt"]);
    let _ = ids;
    let (mut ws2, ids) = open("closeall2", &["a.txt", "b.txt"]);
    let a = ws2.active_group();
    let b = ws2.split_group(SplitDirection::Right);
    let c = ws2.open(&file("closeall2", "a.txt")).expect("reopen");
    assert_eq!(c, ids[0]);
    write(&file("closeall2", "c.txt"), "c");
    let only_here = ws2.open(&file("closeall2", "c.txt")).expect("c.txt");
    ws2.type_text(only_here, "!").expect("dirty");
    assert_eq!(group_tabs(&ws2, b), [ids[1], ids[0], only_here]);
    run_err(&mut ws2, "workbench.action.closeEditorsInGroup", only_here);
    assert_eq!(group_tabs(&ws2, b).len(), 3, "nothing closed");
    ws2.close_editors_in_group(b, true).expect("discard");
    assert_eq!(ws2.group_ids(), [a]);
    assert!(
        ws2.document(only_here).is_none(),
        "c.txt was shown only there"
    );
    assert_eq!(
        group_tabs(&ws2, a),
        ids,
        "the other documents stay in the first group"
    );
    ws.focus_group_index(0);
}

fn run_err(ws: &mut Workspace, id: &str, expected: DocumentId) {
    assert!(
        matches!(ws.execute(id), Err(Error::Dirty(d)) if d == expected),
        "{id}"
    );
}

#[test]
fn directional_focus_follows_the_grid_and_wraps() {
    let (mut ws, _) = open("direction", &["a.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    let c = ws.split_group(SplitDirection::Down); // b over c, a on the left
    ws.focus_group(a).expect("a");
    assert_eq!(
        ws.group_toward(a, Direction::Right),
        Some(b),
        "first child entered"
    );
    assert_eq!(ws.group_toward(a, Direction::Left), None);
    assert_eq!(ws.group_toward(b, Direction::Down), Some(c));
    assert_eq!(ws.group_toward(c, Direction::Left), Some(a));
    run(&mut ws, "workbench.action.focusRightGroup");
    assert_eq!(ws.active_group(), b);
    run(&mut ws, "workbench.action.focusBelowGroup");
    assert_eq!(ws.active_group(), c);
    run(&mut ws, "workbench.action.focusBelowGroup");
    assert_eq!(ws.active_group(), b, "wraps to the top");
    run(&mut ws, "workbench.action.focusLeftGroup");
    assert_eq!(ws.active_group(), a);
    run(&mut ws, "workbench.action.focusLeftGroup");
    assert_eq!(ws.active_group(), b, "wraps to the right edge");
    run(&mut ws, "workbench.action.focusAboveGroup");
    assert_eq!(ws.active_group(), c);
}

#[test]
fn focusing_by_number_creates_the_group_one_past_the_last() {
    let (mut ws, _) = open("number", &["a.txt"]);
    let a = ws.active_group();
    run(&mut ws, "workbench.action.focusSecondEditorGroup");
    assert_eq!(ws.group_ids().len(), 2, "Ctrl+2 made a group");
    let b = ws.active_group();
    assert_ne!(a, b);
    assert!(ws.tabs().is_empty(), "an empty group, nothing copied");
    run(&mut ws, "workbench.action.focusFirstEditorGroup");
    assert_eq!(ws.active_group(), a);
    run(&mut ws, "workbench.action.focusThirdEditorGroup");
    assert_eq!(ws.group_ids().len(), 3);
    ws.focus_group_index(7);
    assert_eq!(
        ws.group_ids().len(),
        3,
        "further than one past does nothing"
    );
    run(&mut ws, "workbench.action.focusPreviousGroup");
    assert_eq!(ws.active_group(), b);
    run(&mut ws, "workbench.action.focusNextGroup");
    run(&mut ws, "workbench.action.focusNextGroup");
    assert_eq!(ws.active_group(), a, "wraps");
}

#[test]
fn moving_an_editor_to_the_next_and_previous_group() {
    let (mut ws, ids) = open("move", &["a.txt", "b.txt"]);
    let a = ws.active_group();
    let b = ws.split_group(SplitDirection::Right);
    ws.focus_group(a).expect("a");
    run(&mut ws, "workbench.action.moveEditorToNextGroup");
    assert_eq!(ws.active_group(), b, "the focus follows the editor");
    assert_eq!(
        group_tabs(&ws, b),
        [ids[1]],
        "b.txt was in b already: one tab"
    );
    assert_eq!(group_tabs(&ws, a), [ids[0]]);
    run(&mut ws, "workbench.action.moveEditorToPreviousGroup");
    assert_eq!(ws.active_group(), a);
    assert!(
        !ws.move_editor_to_group(false),
        "nothing before the first group"
    );
    assert_eq!(ws.group_ids(), [a], "b was left empty and closed");
    assert_eq!(group_tabs(&ws, a), [ids[0], ids[1]]);
}

#[test]
fn joining_all_groups_gathers_every_editor_in_the_active_one() {
    let (mut ws, ids) = open("join", &["a.txt", "b.txt"]);
    let a = ws.active_group();
    let _b = ws.split_group(SplitDirection::Right);
    let _c = ws.split_group(SplitDirection::Down);
    ws.focus_group(a).expect("a");
    run(&mut ws, "workbench.action.joinAllGroups");
    assert_eq!(ws.group_ids(), [a]);
    assert_eq!(ws.layout(), &Layout::Group(a));
    assert_eq!(group_tabs(&ws, a), ids);
    assert!(ws.document(ids[0]).is_some() && ws.document(ids[1]).is_some());
}

#[test]
fn split_commands_use_vs_codes_ids_and_the_side_by_side_setting() {
    let (mut ws, _) = open("commands", &["a.txt"]);
    run(&mut ws, "workbench.action.splitEditor");
    assert_eq!(ws.group_ids().len(), 2);
    assert!(matches!(
        ws.layout(),
        Layout::Split {
            axis: Axis::Horizontal,
            ..
        }
    ));
    run(&mut ws, "workbench.action.splitEditorDown");
    assert!(
        matches!(ws.layout(), Layout::Split { axis: Axis::Horizontal, children } if children.len() == 2)
    );
    run(&mut ws, "workbench.action.splitEditorRight");
    assert_eq!(ws.group_ids().len(), 4);
    let root = dir("commands-settings");
    write(
        &root.join("settings.json"),
        r#"{"workbench.editor.openSideBySideDirection":"down"}"#,
    );
    write(&root.join("keybindings.json"), "[]");
    let (mut down, _) = open("commands-down", &["a.txt"]);
    let _ = down.load_config(&root.join("settings.json"), &root.join("keybindings.json"));
    run(&mut down, "workbench.action.splitEditor");
    assert!(matches!(
        down.layout(),
        Layout::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
}

#[test]
fn ctrl_w_closes_an_empty_group_and_an_editor_otherwise() {
    let (mut ws, _) = open("ctrlw", &["a.txt"]);
    assert!(
        !ws.command_applies("workbench.action.closeGroup"),
        "one group"
    );
    assert!(ws.command_applies("workbench.action.closeActiveEditor"));
    run(&mut ws, "workbench.action.focusSecondEditorGroup");
    assert!(
        ws.command_applies("workbench.action.closeGroup"),
        "empty, and several groups"
    );
    assert!(!ws.command_applies("workbench.action.closeActiveEditor"));
    run(&mut ws, "workbench.action.closeGroup");
    assert_eq!(ws.group_ids().len(), 1);
}

fn refused<T>(result: &Result<T, Error>) -> bool {
    result.is_err()
}

#[test]
fn stale_and_unknown_ids_are_refused_not_fatal() {
    let (mut ws, ids) = open("hostile", &["a.txt"]);
    let a = ws.active_group();
    write(&file("hostile", "gone.txt"), "x");
    let gone = ws.open(&file("hostile", "gone.txt")).expect("open");
    ws.close(gone, true).expect("close");
    let b = ws.split_group(SplitDirection::Right);
    ws.close_group(b).expect("close b");
    // `b` is gone: every call with it is an error or a no-op.
    assert!(refused(&ws.focus_group(b)));
    assert!(refused(&ws.close_group(b)));
    assert!(refused(&ws.close_editor(b, ids[0], true)));
    assert!(refused(&ws.close_editors_in_group(b, true)));
    assert!(ws.group_by_id(b).is_none());
    assert_eq!(ws.view_selections(b, ids[0]), None);
    assert_eq!(ws.view_cursor(b, ids[0]), None);
    assert_eq!(ws.group_toward(b, Direction::Left), None);
    // A document the group does not show.
    assert!(refused(&ws.close_editor(a, gone, true)));
    // Huge indexes do nothing.
    assert!(!ws.focus_group_index(usize::MAX));
    assert!(!ws.focus_group_index(1_000_000));
}

#[test]
fn every_group_command_is_safe_with_nothing_open() {
    let mut empty = Workspace::new();
    for command in rustode_workspace::COMMANDS.iter().filter(|c| {
        c.id.contains("Group")
            || c.id.contains("splitEditor")
            || c.id.contains("closeEditorsInGroup")
    }) {
        let _ = empty.execute(command.id);
        let _ = empty.execute(command.id);
    }
    assert!(!empty.group_ids().is_empty(), "there is always a group");
    assert!(empty.group_by_id(empty.active_group()).is_some());
}

#[test]
fn the_status_bar_and_find_follow_the_active_view() {
    let (mut ws, ids) = open("active-view", &["a.txt"]);
    let doc = ids[0];
    let left = ws.active_group();
    ws.set_cursor(doc, 2).expect("left cursor");
    let right = ws.split_group(SplitDirection::Right);
    ws.set_cursor(doc, 7).expect("right cursor");
    assert_eq!(ws.status_line_column(doc).expect("status"), (1, 8));
    ws.focus_group(left).expect("left");
    assert_eq!(
        ws.status_line_column(doc).expect("status"),
        (1, 3),
        "the active view's cursor"
    );
    assert_eq!(ws.active_document(), Some(doc));
    // Find from the left view starts at its cursor, not the right one's.
    let query = rustode_workspace::FindQuery::new("t", rustode_workspace::FindOptions::default())
        .expect("query");
    let found = ws.find_next(doc, &query).expect("find");
    assert!(found.is_some());
    assert_eq!(
        ws.document(doc).expect("doc").primary_selection().range(),
        Range::new(2, 3)
    );
    assert_eq!(
        ws.view_cursor(right, doc),
        Some(7),
        "the other view did not move"
    );
}
