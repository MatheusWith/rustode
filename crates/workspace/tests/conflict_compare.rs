//! Workspace seam: comparing the file on disk with the unsaved text when a
//! save hit a conflict (VS Code's `workbench.files.action.resolveConflict`).
//!
//! The diff model is the workspace's; the frontends only draw it. The
//! watcher is real, so each test waits for its conflict with a bounded loop.

use rustode_workspace::{
    COMPARE_MAX_BYTES, COMPARE_MAX_CHANGES, COMPARE_MAX_LINES, ConflictCompare, DiffKind,
    DocumentId, Event, Workspace,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const COMPARE: &str = "workbench.files.action.resolveConflict";
const OVERWRITE: &str = "workbench.files.action.saveIgnoreModifiedSince";
const REVERT: &str = "workbench.files.action.revertLocalChanges";

/// A document holding `mine`, unsaved, whose file now holds `disk`, with the
/// conflict raised.
fn conflicted(name: &str, mine: &str, disk: &[u8]) -> (Workspace, DocumentId, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-cmp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, "base\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    assert!(ws.execute("editor.action.selectAll").expect("select all"));
    ws.type_text(id, mine).expect("type");
    ws.take_events();
    std::fs::write(&path, disk).expect("external write");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ws.poll_external_changes();
        if ws.take_events().contains(&Event::Conflict { id }) {
            return (ws, id, path);
        }
        assert!(Instant::now() < deadline, "no conflict raised");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `count` lines, `prefix` then the line's number.
fn numbered(prefix: &str, count: usize) -> String {
    (0..count).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "{prefix}{n}");
        text
    })
}

/// Each line as `kind disk mine text`, for readable assertions; a missing
/// number is `-`.
fn shape(compare: &ConflictCompare) -> Vec<String> {
    let number = |n: Option<usize>| n.map_or_else(|| "-".to_owned(), |n| n.to_string());
    compare
        .lines
        .iter()
        .map(|l| {
            let mark = match l.kind {
                DiffKind::Same => "=",
                DiffKind::Removed => "-",
                DiffKind::Added => "+",
            };
            format!("{mark} {} {} {}", number(l.disk), number(l.mine), l.text)
        })
        .collect()
}

#[test]
fn compare_marks_what_only_the_file_has_and_what_only_my_text_has() {
    let (ws, id, _) = conflicted("typical", "a\nB\nc\nd\n", b"a\nb\nc\n");
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(
        shape(&compare),
        ["= 1 1 a", "- 2 - b", "+ - 2 B", "= 3 3 c", "+ - 4 d"]
    );
    assert_eq!((compare.removed, compare.added), (1, 2));
    assert_eq!(compare.changes, [1, 4], "where each change block starts");
    assert_eq!(compare.notice, None);
    assert_eq!(
        compare.title,
        "f.txt (in file) ↔ f.txt (in rustode) - Resolve save conflict"
    );
}

#[test]
fn compare_pairs_removed_and_added_lines_side_by_side() {
    let (ws, id, _) = conflicted("sides", "a\nB\nc\nd\n", b"a\nb\nc\n");
    let sides = ws.conflict_compare(id).expect("compare").side_by_side();
    let cell = |c: &Option<rustode_workspace::SideCell>| {
        c.as_ref().map(|c| format!("{} {}", c.number, c.text))
    };
    let rows: Vec<_> = sides
        .rows
        .iter()
        .map(|r| (cell(&r.disk), cell(&r.mine), r.changed))
        .collect();
    let some = |s: &str| Some(s.to_owned());
    assert_eq!(
        rows,
        [
            (some("1 a"), some("1 a"), false),
            (some("2 b"), some("2 B"), true),
            (some("3 c"), some("3 c"), false),
            (None, some("4 d"), true),
        ]
    );
    assert_eq!(sides.changes, [1, 3]);
}

#[test]
fn compare_with_an_empty_file_shows_every_line_as_added() {
    let (ws, id, _) = conflicted("empty", "x\ny\n", b"");
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(shape(&compare), ["+ - 1 x", "+ - 2 y"]);
    assert_eq!(compare.changes, [0]);
}

#[test]
fn compare_with_identical_lines_says_the_difference_is_not_in_the_lines() {
    let (ws, id, _) = conflicted("crlf", "a\nb\n", b"a\r\nb\r\n");
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(shape(&compare), ["= 1 1 a", "= 2 2 b"]);
    assert_eq!(compare.changes.len(), 0);
    assert_eq!(
        compare.notice.as_deref(),
        Some(
            "The lines are the same; only line endings, the final newline or the encoding differ."
        )
    );
}

#[test]
fn compare_ignores_line_ending_style_when_other_lines_differ() {
    let (ws, id, _) = conflicted("crlf-diff", "a\nb\nc\n", b"a\r\nX\r\nc\r\n");
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(
        shape(&compare),
        ["= 1 1 a", "- 2 - X", "+ - 2 b", "= 3 3 c"]
    );
    assert!(compare.lines.iter().all(|l| !l.text.contains('\r')));
}

#[test]
fn compare_keeps_unicode_lines_whole() {
    let (ws, id, _) = conflicted(
        "unicode",
        "héllo\n日本\n🙂\n",
        "héllo\n日本語\n🙂\n".as_bytes(),
    );
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(
        shape(&compare),
        ["= 1 1 héllo", "- 2 - 日本語", "+ - 2 日本", "= 3 3 🙂"]
    );
}

#[test]
fn compare_shortens_a_huge_line_for_drawing_but_not_for_matching() {
    let long = "x".repeat(10_000);
    let (ws, id, _) = conflicted(
        "long",
        &format!("{long}\nmine\n"),
        format!("{long}\ntheirs\n").as_bytes(),
    );
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(
        compare.lines[0].kind,
        DiffKind::Same,
        "matched on the full text"
    );
    assert!(compare.lines[0].text.chars().count() <= 2_001);
    assert!(compare.lines[0].text.ends_with('…'));
}

#[test]
fn compare_refuses_a_file_with_too_many_lines_and_says_the_limit() {
    let many = "l\n".repeat(COMPARE_MAX_LINES + 1);
    let (ws, id, _) = conflicted("many-lines", "mine\n", many.as_bytes());
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(compare.lines.len(), 0);
    assert_eq!(compare.changes.len(), 0);
    let notice = compare.notice.expect("a message");
    assert!(
        notice.contains("too large to compare")
            && notice.contains(&format!("{}", COMPARE_MAX_LINES + 1))
            && notice.contains(&format!("{COMPARE_MAX_LINES}")),
        "{notice}"
    );
    assert!(
        notice.contains("Overwrite") && notice.contains("Revert"),
        "{notice}"
    );
}

#[test]
fn compare_refuses_a_file_with_too_many_bytes() {
    let wide = "w".repeat(COMPARE_MAX_BYTES + 1);
    let (ws, id, _) = conflicted("many-bytes", "mine\n", wide.as_bytes());
    let notice = ws
        .conflict_compare(id)
        .expect("compare")
        .notice
        .expect("a message");
    assert!(
        notice.contains("too large to compare") && notice.contains("bytes"),
        "{notice}"
    );
}

#[test]
fn compare_gives_up_on_texts_that_differ_in_too_many_lines() {
    let mine = numbered("m", COMPARE_MAX_CHANGES + 1);
    let disk = numbered("d", COMPARE_MAX_CHANGES + 1);
    let (ws, id, _) = conflicted("many-changes", &mine, disk.as_bytes());
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(compare.lines.len(), 0);
    let notice = compare.notice.expect("a message");
    assert!(
        notice.contains("differ in too many lines")
            && notice.contains(&COMPARE_MAX_CHANGES.to_string()),
        "{notice}"
    );
}

#[test]
fn a_big_file_with_few_changes_still_compares() {
    let base = numbered("line ", 20_000);
    let disk = base.replace("line 100\n", "changed\n");
    let (ws, id, _) = conflicted("big", &base, disk.as_bytes());
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(compare.notice, None);
    assert_eq!((compare.removed, compare.added), (1, 1));
}

#[test]
fn the_command_is_registered_with_vs_codes_id_and_a_file_category() {
    let command = rustode_workspace::COMMANDS
        .iter()
        .find(|c| c.id == COMPARE)
        .expect("registered");
    assert_eq!(command.label, "File: Compare");
}

#[test]
fn the_command_runs_only_during_a_conflict_and_names_the_file_otherwise() {
    let dir = std::env::temp_dir().join(format!("rustode-cmp-{}-none", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, "one\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    let err = ws.execute(COMPARE).expect_err("no conflict");
    assert_eq!(err.to_string(), "f.txt has no conflict to resolve");
    let err = ws
        .conflict_compare(id)
        .map(|_| ())
        .expect_err("no conflict");
    assert_eq!(err.to_string(), "f.txt has no conflict to resolve");
}

#[test]
fn the_command_asks_the_frontend_to_open_the_comparison_and_resolves_nothing() {
    let (mut ws, id, path) = conflicted("cmd", "mine\n", b"theirs\n");
    assert!(ws.execute(COMPARE).expect("compare"));
    assert_eq!(ws.take_events(), [Event::CompareConflict { id }]);
    assert!(
        ws.document(id).expect("doc").has_conflict(),
        "still in conflict"
    );
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}

#[test]
fn from_the_comparison_overwrite_saves_my_text() {
    let (mut ws, id, path) = conflicted("accept", "mine\n", b"theirs\n");
    ws.execute(COMPARE).expect("compare");
    assert!(ws.execute(OVERWRITE).expect("overwrite"));
    assert_eq!(std::fs::read(&path).expect("read"), b"mine\n");
    assert!(ws.conflict_compare(id).is_err(), "no conflict left");
}

#[test]
fn from_the_comparison_revert_loads_the_file_and_keeps_my_text_in_undo() {
    let (mut ws, id, path) = conflicted("revert", "mine\n", b"theirs\n");
    ws.execute(COMPARE).expect("compare");
    assert!(ws.execute(REVERT).expect("revert"));
    assert_eq!(ws.document(id).expect("doc").text(), "theirs\n");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(ws.document(id).expect("doc").text(), "mine\n");
}

#[test]
fn the_prompt_offers_compare_first_in_vs_codes_words() {
    let (ws, id, _) = conflicted("prompt", "mine\n", b"theirs\n");
    let prompt = ws.conflict_prompt(id).expect("prompt");
    assert_eq!(
        prompt.message,
        "Failed to save 'f.txt': The content of the file is newer. Please compare your version \
         with the file contents or overwrite the content of the file with your changes."
    );
    let choices: Vec<_> = prompt
        .choices
        .iter()
        .map(|c| (c.command, c.label))
        .collect();
    assert_eq!(
        choices,
        [
            (COMPARE, "Compare"),
            (OVERWRITE, "Overwrite"),
            (REVERT, "Revert")
        ]
    );
}

#[test]
fn compare_expands_tabs_and_blanks_control_chars_for_drawing() {
    let (ws, id, _) = conflicted("tabs", "a\tb\u{7}c\n", b"x\n");
    let compare = ws.conflict_compare(id).expect("compare");
    assert_eq!(shape(&compare), ["- 1 - x", "+ - 1 a    b c"]);
}
