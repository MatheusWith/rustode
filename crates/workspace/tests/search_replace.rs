//! Workspace seam: replace across files with a preview — the replace text
//! and its per-match preview, replace all, in one file and one match,
//! dismissing matches, undo, files that changed since the search, and
//! results that stay consistent after a partial apply. Every test runs in
//! its own temporary folder.

use rustode_workspace::{
    FindOptions, FindQuery, ReplaceError, ReplaceScope, SearchQuery, Workspace,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-replace-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn folder(name: &str, files: &[(&str, &str)]) -> (Workspace, PathBuf) {
    let root = temp(name);
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("write");
    }
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("open folder");
    (ws, root)
}

fn settle(ws: &mut Workspace) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while ws.search_results().is_running() {
        ws.poll_search();
        assert!(Instant::now() < deadline, "the search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    ws.poll_search();
}

fn search(ws: &mut Workspace, pattern: &str, options: FindOptions) {
    let find = FindQuery::new(pattern, options).expect("pattern");
    ws.start_search(SearchQuery::new(find)).expect("search");
    settle(ws);
}

fn regex() -> FindOptions {
    FindOptions {
        regex: true,
        ..FindOptions::default()
    }
}

fn read(root: &Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).expect("read")
}

fn counts(ws: &Workspace) -> Vec<usize> {
    ws.search_results()
        .files()
        .iter()
        .map(|f| f.matches.len())
        .collect()
}

#[test]
fn the_preview_builds_each_replacement_from_the_match() {
    let (mut ws, _root) = folder(
        "preview",
        &[("a.txt", "foo-bar foo-baz\n"), ("b.txt", "foo-qux\n")],
    );
    search(&mut ws, r"foo-(\w+)", regex());
    ws.set_search_replacement(r"\U$1\E!");
    assert_eq!(ws.search_replacement(), r"\U$1\E!");
    // `\E` is not a VS Code case change: it stays as written.
    assert_eq!(ws.search_replacement_text(0, 0).as_deref(), Some(r"BAR\E!"));
    assert_eq!(ws.search_replacement_text(0, 1).as_deref(), Some(r"BAZ\E!"));
    assert_eq!(ws.search_replacement_text(1, 0).as_deref(), Some(r"QUX\E!"));
    ws.set_search_replacement("<$1>");
    assert_eq!(ws.search_replacement_text(0, 0).as_deref(), Some("<bar>"));
    assert_eq!(ws.search_replacement_text(5, 0), None);
    assert_eq!(ws.search_replacement_text(0, 9), None);
}

#[test]
fn replace_all_edits_every_file_and_saves_like_vs_code() {
    let (mut ws, root) = folder(
        "all",
        &[
            ("a.txt", "one two one\nthree one\n"),
            ("sub/b.txt", "one\n"),
            ("c.txt", "nothing\n"),
        ],
    );
    search(&mut ws, "one", FindOptions::default());
    ws.set_search_replacement("1");
    let outcome = ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!((outcome.matches, outcome.files), (4, 2));
    assert_eq!(read(&root, "a.txt"), "1 two 1\nthree 1\n");
    assert_eq!(read(&root, "sub/b.txt"), "1\n");
    assert_eq!(read(&root, "c.txt"), "nothing\n");
    assert!(ws.search_results().files().is_empty(), "all replaced");
}

#[test]
fn undo_restores_the_text_and_leaves_the_document_unsaved() {
    let (mut ws, root) = folder("undo", &[("a.txt", "one one\n"), ("b.txt", "one\n")]);
    search(&mut ws, "one", FindOptions::default());
    ws.set_search_replacement("two");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    let id = ws.open(&root.join("a.txt")).expect("open");
    assert_eq!(ws.document(id).expect("doc").text(), "two two\n");
    assert!(!ws.document(id).expect("doc").is_dirty(), "saved");
    assert!(ws.undo(id).expect("undo"), "one undo step per document");
    assert_eq!(ws.document(id).expect("doc").text(), "one one\n");
    assert!(ws.document(id).expect("doc").is_dirty());
    assert_eq!(read(&root, "a.txt"), "two two\n", "the file is untouched");
}

#[test]
fn replacing_in_a_file_leaves_the_others() {
    let (mut ws, root) = folder("file", &[("a.txt", "x x\n"), ("b.txt", "x\n")]);
    search(&mut ws, "x", FindOptions::default());
    ws.set_search_replacement("y");
    let outcome = ws
        .replace_in_search(ReplaceScope::File(0))
        .expect("replace");
    assert_eq!((outcome.matches, outcome.files), (2, 1));
    assert_eq!(read(&root, "a.txt"), "y y\n");
    assert_eq!(read(&root, "b.txt"), "x\n");
    assert_eq!(counts(&ws), [1], "the other file's match stays");
}

#[test]
fn replacing_one_match_keeps_the_rest_consistent() {
    let (mut ws, root) = folder("one", &[("a.txt", "cat cat\ncat\n")]);
    search(&mut ws, "cat", FindOptions::default());
    ws.set_search_replacement("elephant\nand");
    ws.replace_in_search(ReplaceScope::Match { file: 0, index: 0 })
        .expect("replace");
    assert_eq!(read(&root, "a.txt"), "elephant\nand cat\ncat\n");
    // The two matches left moved: one to the next line, one down a line.
    let matches = &ws.search_results().files()[0].matches;
    assert_eq!(matches.len(), 2);
    assert_eq!((matches[0].start.line, matches[0].start.column), (1, 4));
    assert_eq!((matches[1].start.line, matches[1].start.column), (2, 0));
    assert_eq!(matches[0].preview().inside, "cat");
    assert_eq!(matches[0].preview().before, "and ");
    // A second replace uses the updated positions.
    ws.replace_in_search(ReplaceScope::Match { file: 0, index: 1 })
        .expect("replace");
    assert_eq!(read(&root, "a.txt"), "elephant\nand cat\nelephant\nand\n");
    assert_eq!(counts(&ws), [1]);
}

#[test]
fn a_dismissed_match_or_file_is_not_replaced() {
    let (mut ws, root) = folder(
        "dismiss",
        &[("a.txt", "z z z\n"), ("b.txt", "z\n"), ("c.txt", "z\n")],
    );
    search(&mut ws, "z", FindOptions::default());
    assert!(ws.dismiss_search_match(0, 1));
    assert!(ws.dismiss_search_file(2));
    assert!(!ws.dismiss_search_match(9, 0));
    assert!(!ws.dismiss_search_file(9));
    assert_eq!(ws.search_results().match_count(), 3);
    ws.set_search_replacement("w");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(read(&root, "a.txt"), "w z w\n");
    assert_eq!(read(&root, "b.txt"), "w\n");
    assert_eq!(read(&root, "c.txt"), "z\n");
    // Dismissed matches left the results before, replaced ones after.
    assert_eq!(counts(&ws).len(), 0, "all replaced or dismissed");
}

#[test]
fn a_file_that_changed_on_disk_is_refused_and_searched_again() {
    let (mut ws, root) = folder("stale", &[("a.txt", "one\n"), ("b.txt", "one\n")]);
    search(&mut ws, "one", FindOptions::default());
    std::fs::write(root.join("b.txt"), "zero\none\n").expect("change");
    ws.set_search_replacement("two");
    let err = ws
        .replace_in_search(ReplaceScope::All)
        .expect_err("stale file");
    let ReplaceError::Stale(files) = err else {
        panic!("expected Stale, got {err}");
    };
    assert_eq!(files, [root.join("b.txt")]);
    assert_eq!(read(&root, "a.txt"), "one\n", "nothing was written");
    assert_eq!(read(&root, "b.txt"), "zero\none\n");
    settle(&mut ws);
    assert_eq!(counts(&ws), [1, 1], "the search ran again");
    let b = &ws.search_results().files()[1].matches[0];
    assert_eq!(b.start.line, 1);
    ws.replace_in_search(ReplaceScope::All).expect("now fine");
    assert_eq!(read(&root, "b.txt"), "zero\ntwo\n");
}

#[test]
fn an_open_document_is_edited_as_it_is_in_the_editor() {
    let (mut ws, root) = folder("open", &[("a.txt", "one\n")]);
    let id = ws.open(&root.join("a.txt")).expect("open");
    ws.type_text(id, "one ").expect("type");
    search(&mut ws, "one", FindOptions::default());
    assert_eq!(counts(&ws), [2]);
    ws.set_search_replacement("1");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(ws.document(id).expect("doc").text(), "1 1\n");
    assert_eq!(read(&root, "a.txt"), "1 1\n", "saved, as VS Code does");
}

#[test]
fn a_document_edited_since_the_search_is_refused() {
    let (mut ws, root) = folder("edited", &[("a.txt", "one\n")]);
    let id = ws.open(&root.join("a.txt")).expect("open");
    search(&mut ws, "one", FindOptions::default());
    ws.set_cursor(id, 0).expect("cursor");
    ws.type_text(id, "zero ").expect("type");
    ws.set_search_replacement("2");
    let err = ws.replace_in_search(ReplaceScope::All).expect_err("stale");
    assert!(matches!(err, ReplaceError::Stale(_)), "{err}");
    assert_eq!(ws.document(id).expect("doc").text(), "zero one\n");
}

#[test]
fn line_endings_of_the_file_are_kept() {
    let (mut ws, root) = folder("crlf", &[("a.txt", "a\r\nb\r\n")]);
    search(&mut ws, "a", FindOptions::default());
    ws.set_search_replacement("x\ny");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(read(&root, "a.txt"), "x\r\ny\r\nb\r\n");
}

#[test]
fn an_empty_replacement_deletes_the_matches() {
    let (mut ws, root) = folder("empty", &[("a.txt", "a-b-c\n")]);
    search(&mut ws, "-", FindOptions::default());
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(read(&root, "a.txt"), "abc\n");
}

#[test]
fn hostile_scopes_are_errors_not_panics() {
    let (mut ws, root) = folder("hostile", &[("a.txt", "a\n")]);
    // No search yet.
    assert!(matches!(
        ws.replace_in_search(ReplaceScope::All),
        Err(ReplaceError::NothingToReplace)
    ));
    search(&mut ws, "a", FindOptions::default());
    for scope in [
        ReplaceScope::File(7),
        ReplaceScope::Match { file: 0, index: 7 },
        ReplaceScope::Match {
            file: usize::MAX,
            index: usize::MAX,
        },
    ] {
        assert!(matches!(
            ws.replace_in_search(scope),
            Err(ReplaceError::NothingToReplace)
        ));
    }
    assert_eq!(read(&root, "a.txt"), "a\n");
}

#[test]
fn a_file_that_vanished_is_refused() {
    let (mut ws, root) = folder("gone", &[("a.txt", "a\n")]);
    search(&mut ws, "a", FindOptions::default());
    std::fs::remove_file(root.join("a.txt")).expect("remove");
    ws.set_search_replacement("b");
    assert!(matches!(
        ws.replace_in_search(ReplaceScope::All),
        Err(ReplaceError::Stale(_))
    ));
}

#[test]
fn a_new_search_forgets_the_preview_of_the_old_replace_text() {
    let (mut ws, _root) = folder("forget", &[("a.txt", "q\n")]);
    search(&mut ws, "q", FindOptions::default());
    ws.set_search_replacement("1");
    assert_eq!(ws.search_replacement_text(0, 0).as_deref(), Some("1"));
    ws.set_search_replacement("2");
    assert_eq!(ws.search_replacement_text(0, 0).as_deref(), Some("2"));
}

#[test]
fn undo_replace_undoes_every_edited_document_at_once() {
    let (mut ws, root) = folder("undo-all", &[("a.txt", "k\n"), ("b.txt", "k\n")]);
    assert!(matches!(
        ws.undo_replace(),
        Err(ReplaceError::NothingToReplace)
    ));
    search(&mut ws, "k", FindOptions::default());
    ws.set_search_replacement("m");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(ws.undo_replace().expect("undo"), 2);
    for name in ["a.txt", "b.txt"] {
        let id = ws.open(&root.join(name)).expect("open");
        assert_eq!(ws.document(id).expect("doc").text(), "k\n");
    }
    assert!(matches!(
        ws.undo_replace(),
        Err(ReplaceError::NothingToReplace)
    ));
}

#[test]
fn undo_replace_is_refused_after_a_later_edit() {
    let (mut ws, root) = folder("undo-late", &[("a.txt", "k\n")]);
    search(&mut ws, "k", FindOptions::default());
    ws.set_search_replacement("m");
    ws.replace_in_search(ReplaceScope::All).expect("replace");
    let id = ws.open(&root.join("a.txt")).expect("open");
    ws.type_text(id, "!").expect("type");
    assert!(matches!(ws.undo_replace(), Err(ReplaceError::Failed(_))));
    assert_eq!(ws.document(id).expect("doc").text(), "!m\n");
}

#[test]
fn the_replace_commands_are_registered_with_vs_code_ids() {
    for id in [
        "search.action.replaceAll",
        "search.action.replace",
        "search.action.replaceAllInFile",
        "search.action.remove",
    ] {
        assert!(
            rustode_workspace::COMMANDS.iter().any(|c| c.id == id),
            "{id}"
        );
    }
    let ws = Workspace::new();
    assert_eq!(
        ws.keybinding_label("search.action.replace").as_deref(),
        Some("Ctrl+Shift+1")
    );
    assert_eq!(
        ws.keybinding_label("search.action.remove").as_deref(),
        Some("Delete")
    );
}

#[test]
fn the_messages_follow_vs_codes_wording() {
    let (mut ws, _root) = folder("messages", &[("a.txt", "q q\n"), ("b.txt", "q\n")]);
    search(&mut ws, "q", FindOptions::default());
    ws.set_search_replacement("w");
    assert_eq!(
        ws.replace_confirmation(ReplaceScope::All).as_deref(),
        Some("Replace 3 occurrences across 2 files with 'w'?")
    );
    assert_eq!(
        ws.replace_confirmation(ReplaceScope::Match { file: 1, index: 0 })
            .as_deref(),
        Some("Replace 1 occurrence across 1 file with 'w'?")
    );
    assert_eq!(ws.replace_confirmation(ReplaceScope::File(9)), None);
    let outcome = ws.replace_in_search(ReplaceScope::All).expect("replace");
    assert_eq!(
        outcome.message("w"),
        "Replaced 3 occurrences across 2 files with 'w'."
    );
    assert_eq!(
        outcome.message(""),
        "Replaced 3 occurrences across 2 files."
    );
}

#[test]
fn a_replace_runs_in_slices_and_results_follow() {
    let files: Vec<(String, String)> = (0..40)
        .map(|n| (format!("f{n:02}.txt"), "x x x\n".to_owned()))
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (mut ws, root) = folder("slices", &refs);
    search(&mut ws, "x", FindOptions::default());
    ws.set_search_replacement("yy");
    ws.start_replace(ReplaceScope::All).expect("start");
    assert!(ws.is_replacing());
    assert!(matches!(
        ws.start_replace(ReplaceScope::All),
        Err(ReplaceError::Failed(_))
    ));
    assert!(!ws.dismiss_search_file(0), "no dismissing while it runs");
    let progress = ws.replace_progress().expect("progress");
    assert_eq!((progress.done, progress.total), (0, 80));
    // Zero budget: one file per poll.
    assert!(ws.poll_replace(Some(Duration::ZERO)));
    assert_eq!(ws.replace_progress().expect("progress").done, 1);
    let mut polls = 1;
    while ws.is_replacing() {
        ws.poll_replace(Some(Duration::ZERO));
        polls += 1;
        assert!(polls < 200);
    }
    assert_eq!(polls, 81, "one file per poll, each pass");
    let outcome = ws.take_replace_outcome().expect("done").expect("ok");
    assert_eq!((outcome.matches, outcome.files), (120, 40));
    assert!(ws.take_replace_outcome().is_none(), "taken once");
    assert_eq!(read(&root, "f39.txt"), "yy yy yy\n");
    assert!(!ws.poll_replace(None), "nothing is running");
}

#[test]
fn starting_a_new_search_stops_a_running_replace() {
    let (mut ws, root) = folder("stop", &[("a.txt", "x\n"), ("b.txt", "x\n")]);
    search(&mut ws, "x", FindOptions::default());
    ws.set_search_replacement("y");
    ws.start_replace(ReplaceScope::All).expect("start");
    ws.poll_replace(Some(Duration::ZERO));
    search(&mut ws, "nothing", FindOptions::default());
    assert!(!ws.is_replacing());
    assert_eq!(read(&root, "a.txt"), "x\n", "planned, never applied");
}
