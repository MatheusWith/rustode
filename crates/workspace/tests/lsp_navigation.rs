//! Workspace seam: go to definition, find references, rename and format
//! through a language server, with `rustode-test-lsp` (`crates/test-lsp`) as
//! the server: a declaration is a word after `fn `, a use is any whole-word
//! match, over the `.rs` files of the folder, open or not.

use rustode_workspace::{DocumentId, Event, LocationsKind, Position, Range, Selection, Workspace};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// The test server, built next to this test binary (see `tests/lsp.rs`).
fn server() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary");
    if let Some(path) = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("rustode-test-lsp"))
        .find(|path| path.is_file())
    {
        return path;
    }
    let profile = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "debug"))
        .expect("a debug profile folder");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "-p", "rustode-test-lsp"])
        .arg("--target-dir")
        .arg(profile.parent().expect("target dir"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building rustode-test-lsp failed");
    profile.join("rustode-test-lsp")
}

struct Fixture {
    dir: PathBuf,
    events: Vec<Event>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-nav-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("user")).expect("temp dir");
        Self {
            dir,
            events: Vec::new(),
        }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(&path, text).expect("fixture");
        path
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("user").join("server.log")
    }

    fn workspace(&self, args: &[&str]) -> Workspace {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.extend(["--log".to_owned(), self.log_path().display().to_string()]);
        let settings = serde_json::json!({
            "rustode.languageServers": {
                "rust": {"command": server().display().to_string(), "args": all}
            }
        });
        std::fs::write(self.dir.join("user/settings.json"), settings.to_string()).expect("conf");
        let mut ws = Workspace::new();
        let user = self.dir.join("user");
        ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
        ws.open_folder(&self.dir).expect("folder");
        ws
    }

    fn requests(&self, method: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|m| m["method"] == method)
            .collect()
    }

    /// Polls until `done` holds for the events gathered so far.
    #[track_caller]
    fn wait(
        &mut self,
        ws: &mut Workspace,
        what: &str,
        done: impl Fn(&Workspace, &[Event]) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            ws.poll_language_servers();
            self.events.extend(ws.take_events());
            if done(ws, &self.events) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for a notice and returns it.
    #[track_caller]
    fn notice(&mut self, ws: &mut Workspace) -> String {
        self.events.clear();
        self.wait(ws, "a notice", |_, e| {
            e.iter().any(|e| matches!(e, Event::Notice(_)))
        });
        self.events
            .iter()
            .find_map(|e| match e {
                Event::Notice(text) => Some(text.clone()),
                _ => None,
            })
            .expect("a notice")
    }

    /// Opens `name` and waits until its server runs.
    fn open(&mut self, ws: &mut Workspace, name: &str) -> DocumentId {
        let id = ws.open(&self.dir.join(name)).expect("open");
        self.wait(ws, "the server", |ws, _| {
            ws.language_servers()
                .iter()
                .any(|s| s.state == rustode_workspace::ServerState::Running)
        });
        id
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const A: &str = "fn helper() {}\nfn main() {\n    helper();   \n}\n";
const B: &str = "fn other() {\n    helper();\n}\n";
const C: &str = "fn helper() {}\n";

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("doc").text()
}

/// The offset of the `n`th (0-based) `needle` in `of`, plus `inside`.
fn at(of: &str, needle: &str, n: usize, inside: usize) -> usize {
    of.match_indices(needle).nth(n).expect("needle").0 + inside
}

// ---- definition ----

#[test]
fn one_definition_is_gone_to_straight_away_even_in_a_closed_file() {
    let mut fx = Fixture::new("def-one");
    fx.file("src/a.rs", A);
    fx.file("src/b.rs", B);
    let mut ws = fx.workspace(&[]);
    let b = fx.open(&mut ws, "src/b.rs");
    ws.set_cursor(b, at(B, "helper", 0, 2)).expect("cursor");
    assert!(ws.execute("editor.action.revealDefinition").expect("run"));
    assert!(ws.navigation_pending());
    fx.wait(&mut ws, "the jump", |ws, _| {
        ws.active_document() != Some(b) && !ws.navigation_pending()
    });
    let a = ws.active_document().expect("a.rs opened");
    assert_eq!(ws.document(a).expect("doc").path(), fx.dir.join("src/a.rs"));
    assert_eq!(
        ws.document(a).expect("doc").primary_selection(),
        Selection::new(3, 9),
        "the declared word is selected"
    );
    assert!(ws.locations().is_none());
}

#[test]
fn no_definition_says_so_with_the_word() {
    let mut fx = Fixture::new("def-none");
    fx.file("src/a.rs", "fn main() {\n    println(1);\n}\n");
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, 20).expect("cursor");
    assert!(ws.request_definition(a, 20).expect("request"));
    assert_eq!(fx.notice(&mut ws), "No definition found for 'println'");
}

#[test]
fn several_definitions_are_listed_in_order_and_picked() {
    let mut fx = Fixture::new("def-many");
    fx.file("src/a.rs", A);
    fx.file("src/c.rs", C);
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    assert!(
        ws.request_definition(a, at(A, "helper", 1, 1))
            .expect("request")
    );
    fx.wait(&mut ws, "the list", |_, e| {
        e.contains(&Event::LocationsChanged)
    });
    let list = ws.locations().expect("a list").clone();
    assert_eq!(list.kind, LocationsKind::Definition);
    assert_eq!(list.word, "helper");
    let places: Vec<_> = list
        .locations
        .iter()
        .map(|l| (l.path.file_name().expect("name").to_owned(), l.start, l.end))
        .collect();
    assert_eq!(places.len(), 2);
    assert_eq!(places[0].0, "a.rs");
    assert_eq!(places[1].0, "c.rs");
    assert_eq!(places[1].1, Position::new(0, 3));
    assert_eq!(list.locations[0].preview, "fn helper() {}");

    let picked = ws.pick_location(1).expect("pick").expect("a document");
    assert_eq!(
        ws.document(picked).expect("doc").path(),
        fx.dir.join("src/c.rs")
    );
    assert_eq!(ws.active_document(), Some(picked));
    assert!(ws.locations().is_none(), "the list closes");
    assert!(ws.pick_location(0).expect("nothing").is_none());
}

#[test]
fn location_links_are_read_too() {
    let mut fx = Fixture::new("def-links");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--definition-links"]);
    let a = fx.open(&mut ws, "src/a.rs");
    let _ = ws.request_definition(a, at(A, "helper", 1, 1));
    fx.wait(&mut ws, "the jump", |ws, _| !ws.navigation_pending());
    assert_eq!(
        ws.document(a).expect("doc").primary_selection(),
        Selection::new(3, 9)
    );
}

#[test]
fn a_new_request_cancels_the_one_in_flight_and_a_changed_document_drops_it() {
    let mut fx = Fixture::new("def-cancel");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--slow", "300"]);
    let a = fx.open(&mut ws, "src/a.rs");
    assert!(
        ws.request_definition(a, at(A, "helper", 1, 1))
            .expect("first")
    );
    assert!(
        ws.request_references(a, at(A, "helper", 1, 1))
            .expect("second")
    );
    let log = fx.log_path();
    fx.wait(&mut ws, "the cancel", |_, _| {
        std::fs::read_to_string(&log).is_ok_and(|text| text.contains("$/cancelRequest"))
    });
    ws.dismiss_locations();
    assert!(!ws.navigation_pending());
    // An answer for a document that changed meanwhile is dropped.
    assert!(
        ws.request_references(a, at(A, "helper", 1, 1))
            .expect("third")
    );
    ws.type_text(a, "x").expect("type");
    std::thread::sleep(Duration::from_millis(500));
    ws.poll_language_servers();
    assert!(ws.locations().is_none());
}

// ---- references ----

#[test]
fn references_list_every_use_with_previews_across_open_and_closed_files() {
    let mut fx = Fixture::new("refs");
    fx.file("src/a.rs", A);
    fx.file("src/b.rs", B);
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
    assert!(ws.execute("editor.action.goToReferences").expect("run"));
    fx.wait(&mut ws, "the list", |_, e| {
        e.contains(&Event::LocationsChanged)
    });
    let list = ws.locations().expect("list");
    assert_eq!(list.kind, LocationsKind::References);
    let found: Vec<_> = list
        .locations
        .iter()
        .map(|l| {
            (
                l.path
                    .file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned(),
                l.start.line,
                l.preview.clone(),
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("a.rs".to_owned(), 0, "fn helper() {}".to_owned()),
            ("a.rs".to_owned(), 2, "helper();".to_owned()),
            ("b.rs".to_owned(), 1, "helper();".to_owned()),
        ]
    );
    ws.dismiss_locations();
    assert!(ws.locations().is_none());
}

#[test]
fn references_to_an_unused_word_say_none_found() {
    let mut fx = Fixture::new("refs-none");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    let blank = at(A, "    helper", 0, 1);
    assert!(ws.request_references(a, blank).expect("request"));
    fx.wait(&mut ws, "an answer", |ws, _| !ws.navigation_pending());
    assert!(ws.locations().is_none(), "no word, no references");
}

#[test]
fn positions_in_other_encodings_land_on_the_word() {
    let mut fx = Fixture::new("refs-utf8");
    let text = "fn helper() {}\n// é😀 helper\n";
    fx.file("src/a.rs", text);
    let mut ws = fx.workspace(&["--encoding", "utf-8"]);
    let a = fx.open(&mut ws, "src/a.rs");
    assert!(ws.request_references(a, 4).expect("request"));
    fx.wait(&mut ws, "the list", |_, e| {
        e.contains(&Event::LocationsChanged)
    });
    let second = &ws.locations().expect("list").locations[1];
    assert_eq!(second.start, Position::new(1, 6), "chars, not bytes");
    assert_eq!(second.end, Position::new(1, 12));
}

// ---- rename ----

#[test]
fn rename_edits_open_and_closed_files_and_each_document_undoes_alone() {
    let mut fx = Fixture::new("rename");
    fx.file("src/a.rs", A);
    fx.file("src/b.rs", B);
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 1, 3)).expect("cursor");
    let target = ws.rename_target(a).expect("target").expect("a word");
    assert_eq!(target.text, "helper");
    assert_eq!(
        target.range,
        Range::new(at(A, "helper", 1, 0), at(A, "helper", 1, 6))
    );

    fx.events.clear();
    assert!(ws.rename_symbol(a, "assist").expect("request"));
    assert_eq!(
        fx.notice(&mut ws),
        "Renamed 'helper' to 'assist': 3 edit(s) in 2 file(s)"
    );
    assert_eq!(
        text(&ws, a),
        "fn assist() {}\nfn main() {\n    assist();   \n}\n"
    );
    let b = ws
        .tabs()
        .iter()
        .copied()
        .find(|&t| t != a)
        .expect("b opened");
    assert_eq!(text(&ws, b), "fn other() {\n    assist();\n}\n");
    assert!(
        ws.document(b).expect("doc").is_dirty(),
        "unsaved, as in VS Code"
    );
    assert_eq!(
        std::fs::read_to_string(fx.dir.join("src/b.rs")).expect("disk"),
        B,
        "nothing is written"
    );
    assert_eq!(ws.active_document(), Some(a), "the tab stays");
    assert_eq!(
        ws.document(a).expect("doc").cursor(),
        at(A, "helper", 1, 3),
        "the cursor stays in the word"
    );

    assert!(ws.undo(a).expect("undo a"));
    assert_eq!(text(&ws, a), A, "one step undoes the whole document");
    assert_eq!(text(&ws, b), "fn other() {\n    assist();\n}\n");
    assert!(ws.undo(b).expect("undo b"));
    assert_eq!(text(&ws, b), B);
    // The server saw the new texts: a later request works on them.
    assert!(ws.redo(a).expect("redo"));
}

#[test]
fn the_old_changes_shape_works_and_names_are_validated() {
    let mut fx = Fixture::new("rename-changes");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--rename", "changes"]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
    assert!(!ws.rename_symbol(a, "  ").expect("blank"));
    assert!(!ws.rename_symbol(a, "helper").expect("same"));
    assert!(!ws.rename_symbol(a, "a\nb").expect("newline"));
    assert!(ws.rename_symbol(a, "bad name!").expect("sent"));
    let why = fx.notice(&mut ws);
    assert!(
        why.starts_with("The rename failed:") && why.contains("not a valid name"),
        "{why}"
    );
    assert_eq!(text(&ws, a), A);
    assert!(ws.rename_symbol(a, "go").expect("request"));
    assert_eq!(
        fx.notice(&mut ws),
        "Renamed 'helper' to 'go': 2 edit(s) in 1 file(s)"
    );
    assert_eq!(text(&ws, a), "fn go() {}\nfn main() {\n    go();   \n}\n");
    assert_eq!(
        ws.document(a).expect("doc").cursor(),
        at(A, "helper", 0, 1),
        "the cursor keeps its place in the word"
    );
}

#[test]
fn an_edit_for_another_version_or_with_file_operations_or_overlaps_changes_nothing() {
    for (mode, why) in [
        ("stale", "changed since the edit was computed"),
        ("fileop", "not supported yet"),
        ("overlap", "overlapping edits"),
    ] {
        let mut fx = Fixture::new(&format!("rename-{mode}"));
        fx.file("src/a.rs", A);
        fx.file("src/b.rs", B);
        let mut ws = fx.workspace(&["--rename", mode]);
        let a = fx.open(&mut ws, "src/a.rs");
        ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
        assert!(ws.rename_symbol(a, "assist").expect("request"));
        let notice = fx.notice(&mut ws);
        assert!(
            notice.starts_with("The rename was not applied:") && notice.contains(why),
            "{mode}: {notice}"
        );
        assert_eq!(text(&ws, a), A, "{mode}");
        for tab in ws.tabs().to_vec() {
            assert!(!ws.document(tab).expect("doc").is_dirty(), "{mode}");
        }
    }
}

#[test]
fn a_position_past_the_end_is_clamped_not_a_crash() {
    let mut fx = Fixture::new("rename-hostile");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--rename", "hostile"]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
    assert!(ws.rename_symbol(a, "assist").expect("request"));
    let _ = fx.notice(&mut ws);
    assert_eq!(
        text(&ws, a),
        "fn assist() {}\nfn main() {\n    assist();   \n}\nX",
        "the stray insertion went to the end"
    );
}

#[test]
fn a_rename_for_a_document_that_changed_is_not_applied() {
    let mut fx = Fixture::new("rename-late");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--slow", "300"]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
    assert!(ws.rename_symbol(a, "assist").expect("request"));
    ws.type_text(a, "// ").expect("type");
    let notice = fx.notice(&mut ws);
    assert!(notice.contains("the rename was not applied"), "{notice}");
    assert!(text(&ws, a).contains("// "), "the typing stays");
    assert!(
        text(&ws, a).contains("helper();"),
        "and nothing else changed"
    );
}

#[test]
fn rename_without_a_server_or_a_word_does_nothing() {
    let mut fx = Fixture::new("rename-off");
    fx.file("src/a.rs", A);
    let mut ws = fx.workspace(&["--no-rename", "--no-formatting", "--no-navigation"]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "helper", 0, 1)).expect("cursor");
    assert_eq!(ws.rename_target(a).expect("target"), None);
    assert!(!ws.rename_symbol(a, "assist").expect("no server"));
    assert!(!ws.format_document(a).expect("no formatter"));
    assert!(!ws.execute("editor.action.formatDocument").expect("run"));
    assert!(!ws.execute("editor.action.revealDefinition").expect("run"));
    assert!(!ws.request_references(a, 4).expect("none"));
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    ws.set_cursor(a, at(A, "{}", 0, 0)).expect("cursor");
    assert!(ws.rename_target(a).expect("target").is_none(), "no word");
    assert!(!ws.rename_symbol(a, "x").expect("no word"));
    ws.close(a, true).expect("close");
    assert!(ws.rename_symbol(a, "x").is_err(), "a closed document");
    assert!(ws.format_document(a).is_err());
    assert!(ws.request_definition(a, 0).is_err());
}

// ---- format ----

#[test]
fn format_is_one_undo_step_and_keeps_the_cursor_in_its_text() {
    let mut fx = Fixture::new("format");
    let source = "fn main() {  \n    1;\t\n}\n";
    fx.file("src/a.rs", source);
    let mut ws = fx.workspace(&[]);
    let a = fx.open(&mut ws, "src/a.rs");
    let cursor = at(source, "}", 0, 0);
    ws.set_cursor(a, cursor).expect("cursor");
    assert!(ws.execute("editor.action.formatDocument").expect("run"));
    assert_eq!(fx.notice(&mut ws), "Formatted the document");
    assert_eq!(text(&ws, a), "fn main() {\n    1;\n}\n");
    assert_eq!(ws.document(a).expect("doc").cursor(), cursor - 3);
    let sent = &fx.requests("textDocument/formatting")[0]["params"]["options"];
    assert_eq!(sent["tabSize"], 4);
    assert_eq!(sent["insertSpaces"], true);
    assert!(ws.undo(a).expect("undo"));
    assert_eq!(text(&ws, a), source, "a single step");

    assert!(ws.redo(a).expect("redo"));
    assert!(ws.format_document(a).expect("again"));
    assert_eq!(fx.notice(&mut ws), "Nothing to format");
}

#[test]
fn format_options_follow_the_settings_and_hostile_edits_are_clamped() {
    let mut fx = Fixture::new("format-hostile");
    fx.file("src/a.rs", "a  \n");
    let mut ws = fx.workspace(&["--hostile-format"]);
    std::fs::write(
        fx.dir.join("user/settings.json"),
        serde_json::json!({
            "editor.insertSpaces": false,
            "editor.tabSize": 2,
            "rustode.languageServers": {"rust": {"command": server().display().to_string(),
                "args": ["--hostile-format", "--log", fx.log_path().display().to_string()]}},
        })
        .to_string(),
    )
    .expect("settings");
    let user = fx.dir.join("user");
    ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
    let a = fx.open(&mut ws, "src/a.rs");
    assert!(ws.format_document(a).expect("request"));
    let _ = fx.notice(&mut ws);
    assert_eq!(text(&ws, a), "a\n// end\n");
    let sent = &fx.requests("textDocument/formatting")[0]["params"]["options"];
    assert_eq!(
        (sent["tabSize"].clone(), sent["insertSpaces"].clone()),
        (2.into(), false.into())
    );
}

#[test]
fn a_format_for_a_document_that_changed_is_dropped() {
    let mut fx = Fixture::new("format-late");
    fx.file("src/a.rs", "a  \n");
    let mut ws = fx.workspace(&["--slow", "300"]);
    let a = fx.open(&mut ws, "src/a.rs");
    assert!(ws.format_document(a).expect("request"));
    ws.type_text(a, "b").expect("type");
    let notice = fx.notice(&mut ws);
    assert!(notice.contains("the format was not applied"), "{notice}");
    assert_eq!(text(&ws, a), "ba  \n");
}

// ---- commands ----

#[test]
fn the_commands_have_vs_code_ids_and_keys() {
    let ws = Workspace::new();
    for (query, id, key) in [
        ("go to definition", "editor.action.revealDefinition", "F12"),
        (
            "go to references",
            "editor.action.goToReferences",
            "Shift+F12",
        ),
        ("rename symbol", "editor.action.rename", "F2"),
        (
            "format document",
            "editor.action.formatDocument",
            "Ctrl+Shift+I",
        ),
    ] {
        let found = ws.commands(query);
        assert_eq!(found[0].command.id, id);
        assert_eq!(found[0].keybinding.as_deref(), Some(key));
    }
}
