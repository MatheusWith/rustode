//! Workspace seam: search across the opened folder (Ctrl+Shift+F) — the
//! find options, include and exclude globs, ignore files and exclude
//! settings, open documents searched as they are in the editor, results
//! streamed in VS Code's order, cancellation and the result limit. Every
//! test runs in its own temporary folder.

use rustode_workspace::{
    Event, FindOptions, FindQuery, Position, SearchError, SearchEvent, SearchQuery, SearchRow,
    SearchState, Workspace,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-search-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A folder holding `files` (path, text), opened in a new workspace.
fn folder(name: &str, files: &[(&str, &str)]) -> (Workspace, PathBuf) {
    let root = temp(name);
    write(&root, files);
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("open folder");
    (ws, root)
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("write");
    }
}

/// Settings for the workspace, as a user `settings.json`.
fn settings(ws: &mut Workspace, root: &Path, json: &str) {
    let dir = root.with_extension("config");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("settings.json"), json).expect("settings");
    ws.load_config(&dir.join("settings.json"), &dir.join("keybindings.json"));
}

fn query(pattern: &str, options: FindOptions) -> SearchQuery {
    SearchQuery::new(FindQuery::new(pattern, options).expect("pattern"))
}

fn literal(pattern: &str) -> SearchQuery {
    query(pattern, FindOptions::default())
}

/// Polls until the search is over.
fn settle(ws: &mut Workspace) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while ws.search_results().is_running() {
        ws.poll_search();
        assert!(Instant::now() < deadline, "the search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    ws.poll_search();
}

fn search(ws: &mut Workspace, query: SearchQuery) {
    ws.start_search(query).expect("search");
    settle(ws);
}

/// Each file with results, relative to `root`, with its match count.
fn files(ws: &Workspace, root: &Path) -> Vec<(String, usize)> {
    ws.search_results()
        .files()
        .iter()
        .map(|file| {
            let path = file.path.strip_prefix(root).unwrap_or(&file.path);
            (path.to_string_lossy().into_owned(), file.matches.len())
        })
        .collect()
}

fn names(ws: &Workspace, root: &Path) -> Vec<String> {
    files(ws, root).into_iter().map(|(name, _)| name).collect()
}

#[test]
fn a_literal_search_finds_every_match_grouped_by_file() {
    let (mut ws, root) = folder(
        "literal",
        &[
            ("a.txt", "one needle\nno\nneedle and needle\n"),
            ("src/b.rs", "fn needle() {}\n"),
            ("src/c.rs", "nothing here\n"),
        ],
    );
    search(&mut ws, literal("needle"));
    let results = ws.search_results();
    assert_eq!(results.state(), SearchState::Done);
    assert_eq!(
        files(&ws, &root),
        [("a.txt".into(), 3), ("src/b.rs".into(), 1)]
    );
    assert_eq!(results.match_count(), 4);
    assert_eq!(results.file_count(), 2);
    assert_eq!(
        results.message().as_deref(),
        Some("4 results in 2 files"),
        "VS Code's count message"
    );
    let a = &results.files()[0];
    let ranges: Vec<(Position, Position)> = a.matches.iter().map(|m| (m.start, m.end)).collect();
    assert_eq!(
        ranges,
        [
            (Position::new(0, 4), Position::new(0, 10)),
            (Position::new(2, 0), Position::new(2, 6)),
            (Position::new(2, 11), Position::new(2, 17)),
        ]
    );
    assert_eq!(a.label, "a.txt");
    assert_eq!(results.files()[1].label, "b.rs");
    assert_eq!(results.files()[1].description, "src");
    let preview = a.matches[2].preview();
    assert_eq!(
        (
            preview.before.as_str(),
            preview.inside.as_str(),
            preview.after.as_str()
        ),
        ("needle and ", "needle", "")
    );
}

#[test]
fn match_case_whole_word_and_regex_mean_what_they_mean_in_find() {
    let (mut ws, root) = folder(
        "options",
        &[
            ("a.txt", "Needle needle needles\n"),
            ("b.txt", "need1e needle_x\n"),
        ],
    );
    search(&mut ws, literal("needle"));
    assert_eq!(
        files(&ws, &root),
        [("a.txt".into(), 3), ("b.txt".into(), 1)]
    );
    let case = FindOptions {
        match_case: true,
        ..FindOptions::default()
    };
    search(&mut ws, query("Needle", case));
    assert_eq!(files(&ws, &root), [("a.txt".into(), 1)]);
    let word = FindOptions {
        whole_word: true,
        ..FindOptions::default()
    };
    search(&mut ws, query("needle", word));
    assert_eq!(
        files(&ws, &root),
        [("a.txt".into(), 2)],
        "`_` is no word separator, so needle_x is one word"
    );
    let regex = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    search(&mut ws, query("need[l1]e", regex));
    assert_eq!(
        files(&ws, &root),
        [("a.txt".into(), 3), ("b.txt".into(), 2)]
    );
    search(&mut ws, query("^need", regex));
    assert_eq!(
        files(&ws, &root),
        [("a.txt".into(), 1), ("b.txt".into(), 1)]
    );
}

#[test]
fn include_and_exclude_globs_follow_vs_code() {
    let (mut ws, root) = folder(
        "globs",
        &[
            ("src/a.rs", "key"),
            ("src/b.ts", "key"),
            ("tests/c.rs", "key"),
            ("docs/d.md", "key"),
            ("lib/src/e.rs", "key"),
        ],
    );
    let mut with = |include: &str, exclude: &str| {
        let mut q = literal("key");
        q.include = include.into();
        q.exclude = exclude.into();
        search(&mut ws, q);
        names(&ws, &root)
    };
    assert_eq!(with("src", ""), ["lib/src/e.rs", "src/a.rs", "src/b.ts"]);
    assert_eq!(with("./src", ""), ["src/a.rs", "src/b.ts"]);
    assert_eq!(with(".rs", ""), ["lib/src/e.rs", "src/a.rs", "tests/c.rs"]);
    assert_eq!(
        with("*.{rs,md}", ""),
        ["docs/d.md", "lib/src/e.rs", "src/a.rs", "tests/c.rs"]
    );
    assert_eq!(with("docs, tests", ""), ["docs/d.md", "tests/c.rs"]);
    assert_eq!(
        with("", "tests, *.md"),
        ["lib/src/e.rs", "src/a.rs", "src/b.ts"]
    );
    assert_eq!(with("src", "*.ts"), ["lib/src/e.rs", "src/a.rs"]);
    assert_eq!(with("nothing", ""), Vec::<String>::new());
    assert_eq!(
        ws.search_results().message().as_deref(),
        Some("No results found in 'nothing'")
    );
}

#[test]
fn an_invalid_glob_is_refused_and_clears_the_results() {
    let (mut ws, _root) = folder("bad-glob", &[("a.txt", "key")]);
    search(&mut ws, literal("key"));
    assert_eq!(ws.search_results().file_count(), 1);
    let mut q = literal("key");
    q.include = "src, a[b".into();
    let err = ws.start_search(q).expect_err("invalid glob");
    assert!(
        matches!(&err, SearchError::Glob(message) if message.starts_with("a[b")),
        "{err}"
    );
    assert_eq!(ws.search_results().file_count(), 0);
    assert_eq!(ws.search_results().state(), SearchState::Idle);
}

#[test]
fn ignore_files_and_hidden_files() {
    let (mut ws, root) = folder(
        "ignore",
        &[
            (".gitignore", "target/\n*.log\n"),
            ("target/out.txt", "key"),
            ("app.log", "key"),
            ("src/.ignore", "gen.rs\n"),
            ("src/gen.rs", "key"),
            ("src/main.rs", "key"),
            (".env", "key"),
        ],
    );
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), [".env", "src/main.rs"]);
    settings(&mut ws, &root, r#"{ "search.useIgnoreFiles": false }"#);
    search(&mut ws, literal("key"));
    assert_eq!(
        names(&ws, &root),
        [
            ".env",
            "app.log",
            "src/gen.rs",
            "src/main.rs",
            "target/out.txt"
        ]
    );
}

#[test]
fn parent_ignore_files_apply_only_when_asked() {
    let outer = temp("parents");
    write(
        &outer,
        &[
            (".gitignore", "skipped.txt\n"),
            ("project/skipped.txt", "key"),
            ("project/kept.txt", "key"),
        ],
    );
    let project = outer.join("project");
    let mut ws = Workspace::new();
    ws.open_folder(&project).expect("open folder");
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &project), ["kept.txt", "skipped.txt"]);
    settings(
        &mut ws,
        &outer,
        r#"{ "search.useParentIgnoreFiles": true }"#,
    );
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &project), ["kept.txt"]);
}

#[test]
fn exclude_settings_merge_over_their_defaults() {
    let (mut ws, root) = folder(
        "excludes",
        &[
            ("node_modules/x/i.js", "key"),
            ("bower_components/b.js", "key"),
            ("saved.code-search", "key"),
            (".git/HEAD", "key"),
            ("gen/out.rs", "key"),
            ("src/a.rs", "key"),
        ],
    );
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["gen/out.rs", "src/a.rs"]);
    settings(
        &mut ws,
        &root,
        r#"{ "search.exclude": { "**/gen": true } }"#,
    );
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["src/a.rs"], "the defaults still apply");
    settings(
        &mut ws,
        &root,
        r#"{ "search.exclude": { "**/node_modules": false } }"#,
    );
    search(&mut ws, literal("key"));
    assert_eq!(
        names(&ws, &root),
        ["gen/out.rs", "node_modules/x/i.js", "src/a.rs"]
    );
    settings(&mut ws, &root, r#"{ "files.exclude": { "gen": true } }"#);
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["src/a.rs"]);
    let mut q = literal("key");
    q.use_excludes = false;
    search(&mut ws, q);
    assert_eq!(
        names(&ws, &root),
        [
            "saved.code-search",
            ".git/HEAD",
            "bower_components/b.js",
            "gen/out.rs",
            "node_modules/x/i.js",
            "src/a.rs"
        ],
        "the toggle turns the exclude settings off"
    );
}

#[test]
fn the_exclude_toggle_turns_ignore_files_off_too() {
    let (mut ws, root) = folder(
        "toggle",
        &[
            (".gitignore", "out/\n"),
            ("out/a.txt", "key"),
            ("b.txt", "key"),
        ],
    );
    let mut q = literal("key");
    search(&mut ws, q.clone());
    assert_eq!(names(&ws, &root), ["b.txt"]);
    q.use_excludes = false;
    search(&mut ws, q);
    assert_eq!(names(&ws, &root), ["b.txt", "out/a.txt"]);
}

#[test]
fn binary_files_are_skipped() {
    let (mut ws, root) = folder("binary", &[("a.txt", "key")]);
    std::fs::write(root.join("b.bin"), b"key\0\x01").expect("binary");
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["a.txt"]);
}

#[test]
fn open_documents_are_searched_as_they_are_in_the_editor() {
    let (mut ws, root) = folder("open", &[("a.txt", "old text\n"), ("b.txt", "key\n")]);
    let outside = temp("open-outside").join("notes.txt");
    std::fs::write(&outside, "a key outside\n").expect("outside");
    let a = ws.open(&root.join("a.txt")).expect("open a");
    ws.type_text(a, "key ").expect("type");
    ws.open(&outside).expect("open outside");
    search(&mut ws, literal("key"));
    let results = ws.search_results();
    let found: Vec<(PathBuf, Position)> = results
        .files()
        .iter()
        .map(|f| (f.path.clone(), f.matches[0].start))
        .collect();
    assert!(
        found.contains(&(root.join("a.txt"), Position::new(0, 0))),
        "the unsaved edit is searched: {found:?}"
    );
    assert!(found.contains(&(root.join("b.txt"), Position::new(0, 0))));
    assert!(
        found.contains(&(outside.clone(), Position::new(0, 2))),
        "an open file outside the folder is searched too: {found:?}"
    );
    assert_eq!(results.match_count(), 3);
}

#[test]
fn without_a_folder_only_open_documents_are_searched() {
    let dir = temp("no-folder");
    write(&dir, &[("open.txt", "nothing\n"), ("closed.txt", "key\n")]);
    let mut ws = Workspace::new();
    ws.open(&dir.join("open.txt")).expect("open");
    search(&mut ws, literal("key"));
    assert_eq!(ws.search_results().file_count(), 0);
    assert_eq!(
        ws.search_results().message().as_deref(),
        Some("You have not opened or specified a folder. Only open files are currently searched")
    );
    let id = ws.open(&dir.join("closed.txt")).expect("open");
    assert_eq!(ws.active_document(), Some(id));
    search(&mut ws, literal("key"));
    assert_eq!(
        ws.search_results().message().as_deref(),
        Some("1 result in 1 file")
    );
}

#[test]
fn results_are_sorted_by_folder_then_name() {
    let (mut ws, root) = folder(
        "sorted",
        &[
            ("b.txt", "key"),
            ("A.txt", "key"),
            ("src/z.rs", "key"),
            ("src/sub/a.rs", "key"),
            ("src/a10.rs", "key"),
            ("src/a2.rs", "key"),
        ],
    );
    search(&mut ws, literal("key"));
    assert_eq!(
        names(&ws, &root),
        [
            "A.txt",
            "b.txt",
            "src/a2.rs",
            "src/a10.rs",
            "src/z.rs",
            "src/sub/a.rs"
        ]
    );
}

/// A waker whose first call blocks until the test releases it, so that the
/// search cannot finish in between.
fn gated_waker(ws: &mut Workspace) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (reached, reached_rx) = mpsc::channel::<()>();
    let (release, release_rx) = mpsc::channel::<()>();
    let gate = Mutex::new(Some((reached, release_rx)));
    ws.set_search_waker(move || {
        let first = gate.lock().expect("gate").take();
        if let Some((reached, release)) = first {
            let _ = reached.send(());
            let _ = release.recv_timeout(Duration::from_secs(20));
        }
    });
    (reached_rx, release)
}

#[test]
fn results_stream_in_before_the_search_ends() {
    let names_list: Vec<String> = (0..60).map(|n| format!("f{n:02}.txt")).collect();
    let files_list: Vec<(&str, &str)> = names_list.iter().map(|n| (n.as_str(), "key")).collect();
    let (mut ws, root) = folder("stream", &files_list);
    let (reached, release) = gated_waker(&mut ws);
    ws.start_search(literal("key")).expect("search");
    reached
        .recv_timeout(Duration::from_secs(20))
        .expect("the first results arrive");
    assert!(ws.poll_search());
    let partial = names(&ws, &root);
    assert_ne!(partial.len(), 0, "some results arrived");
    assert!(
        ws.search_results().is_running(),
        "the waker holds a search thread"
    );
    let mut sorted = partial.clone();
    sorted.sort();
    assert_eq!(partial, sorted, "partial results are in order too");
    assert!(
        ws.take_events()
            .contains(&Event::Search(SearchEvent::Results))
    );
    release.send(()).expect("release");
    settle(&mut ws);
    assert_eq!(ws.search_results().state(), SearchState::Done);
    assert_eq!(names(&ws, &root), names_list);
    assert!(
        ws.take_events()
            .contains(&Event::Search(SearchEvent::Finished))
    );
}

#[test]
fn cancelling_stops_further_results() {
    let names_list: Vec<String> = (0..60).map(|n| format!("f{n:02}.txt")).collect();
    let files_list: Vec<(&str, &str)> = names_list.iter().map(|n| (n.as_str(), "key")).collect();
    let (mut ws, root) = folder("cancel", &files_list);
    let (reached, release) = gated_waker(&mut ws);
    ws.start_search(literal("key")).expect("search");
    reached
        .recv_timeout(Duration::from_secs(20))
        .expect("the first results arrive");
    ws.poll_search();
    let before = ws.search_results().match_count();
    assert!(ws.cancel_search());
    release.send(()).expect("release");
    std::thread::sleep(Duration::from_millis(50));
    assert!(!ws.poll_search(), "nothing arrives after the cancel");
    let results = ws.search_results();
    assert_eq!(results.state(), SearchState::Cancelled);
    assert_eq!(results.match_count(), before);
    assert_eq!(names(&ws, &root).len(), before);
    assert!(!ws.cancel_search(), "nothing is running any more");
}

#[test]
fn a_search_cancelled_before_any_result_says_so() {
    let (mut ws, _root) = folder("cancel-early", &[("a.txt", "key")]);
    ws.start_search(literal("key")).expect("search");
    assert!(ws.cancel_search());
    assert_eq!(
        ws.search_results().message().as_deref(),
        Some("Search was canceled before any results could be found")
    );
    assert!(
        ws.take_events()
            .contains(&Event::Search(SearchEvent::Finished))
    );
}

#[test]
fn a_new_search_replaces_the_running_one() {
    let (mut ws, root) = folder("replace", &[("a.txt", "old"), ("b.txt", "new")]);
    ws.start_search(literal("old")).expect("first");
    search(&mut ws, literal("new"));
    assert_eq!(names(&ws, &root), ["b.txt"]);
    assert_eq!(
        ws.search_results().query().map(|q| q.find.pattern()),
        Some("new")
    );
}

#[test]
fn the_result_limit_counts_matches() {
    let (mut ws, root) = folder(
        "limit",
        &[("a.txt", "key\nkey\nkey\n"), ("b.txt", "key\nkey\nkey\n")],
    );
    settings(&mut ws, &root, r#"{ "search.maxResults": 5 }"#);
    search(&mut ws, literal("key"));
    let results = ws.search_results();
    assert_eq!(results.match_count(), 5);
    assert!(results.limit_hit());
    assert_eq!(
        results.warning(),
        Some(
            "The result set only contains a subset of all matches. Be more specific in your search to narrow down the results."
        )
    );
    settings(&mut ws, &root, r#"{ "search.maxResults": 6 }"#);
    search(&mut ws, literal("key"));
    assert_eq!(ws.search_results().match_count(), 6);
    assert!(
        !ws.search_results().limit_hit(),
        "exactly the limit is not over it"
    );
}

#[test]
fn the_default_limit_is_20000_matches_and_null_lifts_it() {
    let (mut ws, root) = folder("limit-default", &[("many.txt", &"key\n".repeat(20_005))]);
    search(&mut ws, literal("key"));
    assert_eq!(ws.search_results().match_count(), 20_000);
    assert!(ws.search_results().limit_hit());
    settings(&mut ws, &root, r#"{ "search.maxResults": null }"#);
    search(&mut ws, literal("key"));
    assert_eq!(ws.search_results().match_count(), 20_005);
    assert!(!ws.search_results().limit_hit());
}

#[test]
fn opening_a_match_selects_it_in_a_tab() {
    let (mut ws, _root) = folder("open-match", &[("src/lib.rs", "fn a() {}\nlet key = 1;\n")]);
    search(&mut ws, literal("key"));
    let id = ws.open_search_match(0, 0).expect("open");
    assert_eq!(ws.active_document(), Some(id));
    let document = ws.document(id).expect("document");
    let selection = document.primary_selection().range();
    assert_eq!(document.position_of(selection.start), Position::new(1, 4));
    assert_eq!(document.position_of(selection.end), Position::new(1, 7));
    assert!(ws.open_search_match(0, 9).is_err());
}

#[test]
fn multiline_regexes_and_crlf_files() {
    let (mut ws, _root) = folder(
        "multiline",
        &[("crlf.txt", "one\r\ntwo\r\nthree"), ("lf.txt", "ab\ncd\n")],
    );
    let regex = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    search(&mut ws, query("one\\ntwo", regex));
    let results = ws.search_results();
    assert_eq!(results.file_count(), 1);
    let m = &results.files()[0].matches[0];
    assert_eq!((m.start, m.end), (Position::new(0, 0), Position::new(1, 3)));
    search(&mut ws, query("b\\nc", regex));
    let m = &ws.search_results().files()[0].matches[0];
    assert_eq!((m.start, m.end), (Position::new(0, 1), Position::new(1, 1)));
    search(&mut ws, query("^two$", regex));
    let m = &ws.search_results().files()[0].matches[0];
    assert_eq!((m.start, m.end), (Position::new(1, 0), Position::new(1, 3)));
}

#[test]
fn columns_count_chars_in_any_encoding() {
    let (mut ws, root) = folder("columns", &[("e.txt", "ééé key\n")]);
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("a key".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(root.join("w.txt"), utf16).expect("utf16");
    search(&mut ws, literal("key"));
    let starts: Vec<Position> = ws
        .search_results()
        .files()
        .iter()
        .map(|f| f.matches[0].start)
        .collect();
    assert_eq!(starts, [Position::new(0, 4), Position::new(0, 2)]);
}

#[test]
fn files_collapse_and_expand_in_the_rows() {
    let (mut ws, _root) = folder("rows", &[("a.txt", "key key"), ("b.txt", "key")]);
    search(&mut ws, literal("key"));
    let file = |file, expanded| SearchRow::File { file, expanded };
    let hit = |file, index| SearchRow::Match { file, index };
    assert_eq!(
        ws.search_results().rows(),
        [
            file(0, true),
            hit(0, 0),
            hit(0, 1),
            file(1, true),
            hit(1, 0)
        ]
    );
    assert!(!ws.toggle_search_file(0));
    assert_eq!(
        ws.search_results().rows(),
        [file(0, false), file(1, true), hit(1, 0)]
    );
    assert!(ws.toggle_search_file(0));
    assert!(ws.search_results().is_expanded(0));
}

#[test]
fn symbolic_links_are_followed_unless_turned_off() {
    let (mut ws, root) = folder("links", &[("a.txt", "key")]);
    let elsewhere = temp("links-target");
    write(&elsewhere, &[("b.txt", "key")]);
    std::os::unix::fs::symlink(&elsewhere, root.join("linked")).expect("symlink");
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["a.txt", "linked/b.txt"]);
    settings(&mut ws, &root, r#"{ "search.followSymlinks": false }"#);
    search(&mut ws, literal("key"));
    assert_eq!(names(&ws, &root), ["a.txt"]);
}

#[test]
fn an_empty_pattern_clears_the_results() {
    let (mut ws, _root) = folder("empty", &[("a.txt", "key")]);
    search(&mut ws, literal("key"));
    ws.start_search(literal("")).expect("empty");
    assert_eq!(ws.search_results().state(), SearchState::Idle);
    assert_eq!(ws.search_results().file_count(), 0);
    assert_eq!(ws.search_results().message(), None);
}

#[test]
fn the_seed_is_a_selection_on_one_line() {
    let (mut ws, root) = folder(
        "seed",
        &[("a.txt", "one two\nthree\n"), ("b.txt", "f(x.y)\n")],
    );
    let id = ws.open(&root.join("a.txt")).expect("open");
    assert_eq!(ws.search_seed(false), None);
    ws.set_selections(id, vec![rustode_workspace::Selection::new(4, 7)], 0)
        .expect("select");
    assert_eq!(ws.search_seed(false).as_deref(), Some("two"));
    ws.set_selections(id, vec![rustode_workspace::Selection::new(2, 7)], 0)
        .expect("select");
    assert_eq!(ws.search_seed(true).as_deref(), Some("e two"));
    ws.set_selections(id, vec![rustode_workspace::Selection::new(4, 10)], 0)
        .expect("select");
    assert_eq!(ws.search_seed(false), None);
    let b = ws.open(&root.join("b.txt")).expect("open");
    ws.set_selections(b, vec![rustode_workspace::Selection::new(0, 6)], 0)
        .expect("select");
    assert_eq!(ws.search_seed(true).as_deref(), Some("f\\(x\\.y\\)"));
}

#[test]
fn previews_cut_long_text_before_the_match() {
    let line = format!("{}let value = compute_something(first, key) + 1", "    ");
    let (mut ws, _root) = folder("preview", &[("a.txt", &line)]);
    search(&mut ws, literal("key"));
    let preview = ws.search_results().files()[0].matches[0].preview();
    // `lcut` keeps the shortest tail of at least 26 chars that starts at a
    // word boundary: the boundary before " = " leaves 28.
    assert_eq!(preview.before, "…= compute_something(first, ");
    assert_eq!(preview.inside, "key");
    assert_eq!(preview.after, ") + 1");
}
