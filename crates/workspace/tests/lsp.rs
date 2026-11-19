//! Workspace seam: language servers configured in `settings.json`, started
//! per workspace root and language, their diagnostics on the documents and
//! in the problems list, and supervision (crash, restart, give up).
//!
//! The server is `rustode-test-lsp` (`crates/test-lsp`), a stdio test server
//! that publishes a diagnostic over every `ERROR`, `WARN`, `INFO` and `HINT`
//! and exits abruptly on `CRASH`.

use rustode_workspace::{
    CompletionKind, CompletionMove, DocumentId, Event, MarkerSeverity, Position, Problem, Range,
    ServerState, Workspace,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The test server, built next to this test binary: in one of the folders
/// above it, whatever layout cargo gives its build folder. When only this
/// crate's tests were built, it is built now.
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
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-lsp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("user")).expect("temp dir");
        Self { dir }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("folder");
        }
        std::fs::write(&path, text).expect("fixture");
        path
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("user").join("server.log")
    }

    /// `settings.json` text with the test server for Rust, given `args`.
    fn settings_text(&self, args: &[&str]) -> String {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.push("--log".to_owned());
        all.push(self.log_path().display().to_string());
        serde_json::json!({
            "rustode.languageServers": {
                "rust": { "command": server().display().to_string(), "args": all }
            }
        })
        .to_string()
    }

    fn write_settings(&self, text: &str) {
        std::fs::write(self.dir.join("user/settings.json"), text).expect("settings");
    }

    fn load(&self, ws: &mut Workspace) {
        let user = self.dir.join("user");
        ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
    }

    /// A workspace with the test server configured, `args` passed to it.
    fn workspace(&self, args: &[&str]) -> Workspace {
        self.write_settings(&self.settings_text(args));
        let mut ws = Workspace::new();
        self.load(&mut ws);
        ws
    }

    /// The messages the server received, and the texts it synced.
    fn log(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn methods(&self) -> Vec<String> {
        self.log()
            .iter()
            .filter_map(|m| m["method"].as_str().map(str::to_owned))
            .collect()
    }

    /// The last text the server synced for any document.
    fn synced_text(&self) -> Option<String> {
        self.log()
            .iter()
            .rev()
            .find_map(|m| m["text"].as_str().map(str::to_owned))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Polls the servers until `done` holds, failing after ten seconds.
#[track_caller]
fn wait(ws: &mut Workspace, what: &str, done: impl Fn(&Workspace) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ws.poll_language_servers();
        if done(ws) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            ws.language_servers()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The text each marker of a document covers, with its severity.
fn covered(ws: &Workspace, id: DocumentId) -> Vec<(MarkerSeverity, String)> {
    let text: Vec<char> = ws.document(id).expect("doc").text().chars().collect();
    ws.markers(id)
        .iter()
        .map(|m| {
            let range = m.range;
            (m.severity, text[range.start..range.end].iter().collect())
        })
        .collect()
}

fn running(ws: &Workspace) -> bool {
    ws.language_servers()
        .iter()
        .any(|s| s.state == ServerState::Running)
}

#[test]
fn a_configured_server_starts_for_the_folder_and_its_diagnostics_show() {
    let fx = Fixture::new("start");
    let path = fx.file("src/main.rs", "fn main() {\n    ERROR\n}\n");
    let mut ws = fx.workspace(&[]);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());

    let markers = ws.markers(id);
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].range, Range::new(16, 21));
    assert_eq!(markers[0].severity, MarkerSeverity::Error);
    assert_eq!(markers[0].message, "ERROR found");
    assert_eq!(markers[0].source.as_deref(), Some("test-lsp"));
    let problems = ws.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].path, path);
    assert_eq!(problems[0].start, Position::new(1, 4));
    let counts = ws.problem_counts();
    assert_eq!((counts.errors, counts.warnings, counts.infos), (1, 0, 0));

    let status = ws.language_server(id).expect("a server for the document");
    assert_eq!(status.state, ServerState::Running);
    assert_eq!(status.language, "rust");
    assert_eq!(status.root, fx.dir);
    assert_eq!(status.name, "rustode-test-lsp");
    assert_initialized_then_opened(&fx);
}

/// The server got `initialize` for the folder, then the document.
fn assert_initialized_then_opened(fx: &Fixture) {
    let log = fx.log();
    let initialize = log
        .iter()
        .find(|m| m["method"] == "initialize")
        .expect("initialize");
    let root_uri = format!("file://{}", fx.dir.display());
    assert_eq!(initialize["params"]["rootUri"], root_uri.as_str());
    let open = log
        .iter()
        .find(|m| m["method"] == "textDocument/didOpen")
        .expect("didOpen");
    let document = &open["params"]["textDocument"];
    assert_eq!(document["languageId"], "rust");
    assert_eq!(document["text"], "fn main() {\n    ERROR\n}\n");
    assert_eq!(fx.methods()[..2], ["initialize", "initialized"]);
}

#[test]
fn settings_loaded_after_the_file_start_its_server() {
    let fx = Fixture::new("late");
    let path = fx.file("main.rs", "WARN\n");
    fx.write_settings(&fx.settings_text(&[]));
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    assert!(ws.language_servers().is_empty(), "no settings yet");
    fx.load(&mut ws);
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());
    assert_eq!(covered(&ws, id), [(MarkerSeverity::Warning, "WARN".into())]);
    let status = ws.language_server(id).expect("server");
    assert_eq!(status.root, fx.dir, "without a folder, the file's folder");
}

#[test]
fn positions_are_converted_from_each_position_encoding() {
    for encoding in [None, Some("utf-8"), Some("utf-16"), Some("utf-32")] {
        let fx = Fixture::new(&format!("encoding-{}", encoding.unwrap_or("default")));
        let path = fx.file("main.rs", "é日😀 ERROR\nWARN 😀\n");
        let args = encoding.map_or_else(Vec::new, |e| vec!["--encoding", e]);
        let mut ws = fx.workspace(&args);
        let id = ws.open(&path).expect("open");
        wait(&mut ws, "diagnostics", |ws| ws.markers(id).len() == 2);
        assert_eq!(
            covered(&ws, id),
            [
                (MarkerSeverity::Error, "ERROR".into()),
                (MarkerSeverity::Warning, "WARN".into())
            ],
            "{encoding:?}"
        );
        assert_eq!(ws.problems()[0].start, Position::new(0, 4), "{encoding:?}");
    }
}

#[test]
fn incremental_changes_keep_the_server_in_sync() {
    for sync in ["incremental", "full"] {
        for encoding in ["utf-8", "utf-16", "utf-32"] {
            let fx = Fixture::new(&format!("sync-{sync}-{encoding}"));
            let path = fx.file("main.rs", "aé😀b\r\nline two\n");
            let mut ws = fx.workspace(&["--sync", sync, "--encoding", encoding]);
            let id = ws.open(&path).expect("open");
            wait(&mut ws, "the server", running);
            ws.set_cursor(id, 3).expect("cursor");
            ws.type_text(id, "日本").expect("type");
            ws.set_cursor(id, 8).expect("after the CRLF");
            ws.type_text(id, "WARN ").expect("type");
            ws.delete_left(id).expect("delete");
            ws.enter(id).expect("enter");
            ws.undo(id).expect("undo");
            ws.replace(id, Range::new(0, 2), "x\ny").expect("replace");
            let text = ws.document(id).expect("doc").text();
            wait(&mut ws, "the synced text", |_| {
                fx.synced_text().as_deref() == Some(text.as_str())
            });
            wait(&mut ws, "the warning", |ws| {
                covered(ws, id) == [(MarkerSeverity::Warning, "WARN".into())]
            });
        }
    }
}

#[test]
fn markers_follow_edits_until_the_server_publishes_again() {
    let fx = Fixture::new("follow");
    let path = fx.file("main.rs", "say ERROR\n");
    let mut ws = fx.workspace(&["--publish", "save"]);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());
    ws.type_text(id, "ab ").expect("type before the marker");
    ws.poll_language_servers();
    assert_eq!(
        ws.markers(id)[0].range,
        Range::new(7, 12),
        "moved with the text"
    );
    assert_eq!(ws.problems()[0].start, Position::new(0, 7));
    ws.set_cursor(id, 9).expect("cursor");
    ws.type_text(id, "x").expect("type inside the marker");
    assert_eq!(ws.markers(id)[0].range, Range::new(7, 13), "grows inside");
    ws.save(id).expect("save");
    wait(&mut ws, "a new publish", |ws| ws.markers(id).is_empty());
    assert!(fx.methods().contains(&"textDocument/didSave".to_owned()));
}

#[test]
fn empty_ranges_are_widened_to_the_word_as_vs_code_draws_them() {
    let fx = Fixture::new("empty");
    let path = fx.file("main.rs", "call EMPTY(x)\n");
    let mut ws = fx.workspace(&[]);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());
    assert_eq!(
        covered(&ws, id),
        [(MarkerSeverity::Warning, "EMPTY".into())]
    );
    let problem = &ws.problems()[0];
    assert_eq!(
        (problem.start, problem.end),
        (Position::new(0, 5), Position::new(0, 5))
    );
}

#[test]
fn a_crashed_server_is_reported_and_restarted() {
    let fx = Fixture::new("crash");
    let path = fx.file("main.rs", "ERROR\n");
    let mut ws = fx.workspace(&[]);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());
    let _ = ws.take_events();
    ws.type_text(id, "CRASH ").expect("type");
    wait(&mut ws, "a restart", |ws| {
        ws.language_server(id).is_some_and(|s| s.restarts == 1)
    });
    let events = ws.take_events();
    assert!(
        events.contains(&Event::LanguageServerChanged {
            language: "rust".into(),
            state: ServerState::Crashed
        }),
        "{events:?}"
    );
    wait(&mut ws, "diagnostics again", |ws| {
        running(ws) && !ws.markers(id).is_empty()
    });
    assert_eq!(covered(&ws, id), [(MarkerSeverity::Error, "ERROR".into())]);
    let opens = fx
        .methods()
        .iter()
        .filter(|m| *m == "textDocument/didOpen")
        .count();
    assert_eq!(opens, 2, "the document is opened again on the new server");
    ws.type_text(id, "ok ").expect("editing goes on");
    let text = ws.document(id).expect("doc").text();
    wait(&mut ws, "the synced text", |_| {
        fx.synced_text().as_deref() == Some(text.as_str())
    });
}

#[test]
fn a_server_that_keeps_crashing_is_given_up_until_restarted_by_hand() {
    let fx = Fixture::new("give-up");
    let path = fx.file("main.rs", "CRASH ERROR\n");
    let mut ws = fx.workspace(&["--crash-on-open"]);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "giving up", |ws| {
        ws.language_server(id)
            .is_some_and(|s| s.state == ServerState::Crashed && s.restarts == 4)
    });
    std::thread::sleep(Duration::from_millis(100));
    ws.poll_language_servers();
    let status = ws.language_server(id).expect("server");
    assert_eq!(
        (status.state, status.restarts),
        (ServerState::Crashed, 4),
        "no fifth restart within three minutes"
    );
    assert!(
        status
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("5 times"),
        "{status:?}"
    );
    ws.replace(id, Range::new(0, 6), "")
        .expect("remove the trigger");
    assert!(
        ws.execute("rustode.restartLanguageServer")
            .expect("command")
    );
    wait(&mut ws, "diagnostics", |ws| !ws.markers(id).is_empty());
    assert_eq!(ws.language_server(id).expect("server").restarts, 0);
}

#[test]
fn a_server_that_cannot_start_is_reported_and_editing_goes_on() {
    let fx = Fixture::new("missing");
    let path = fx.file("main.rs", "ERROR\n");
    fx.write_settings(
        "{\"rustode.languageServers\": {\"rust\": {\"command\": \"/nonexistent/rustode-lsp\"}}}",
    );
    let mut ws = Workspace::new();
    fx.load(&mut ws);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the failure", |ws| {
        ws.language_server(id)
            .is_some_and(|s| s.state == ServerState::Crashed)
    });
    let status = ws.language_server(id).expect("server");
    assert!(status.message.is_some(), "{status:?}");
    ws.type_text(id, "x").expect("editing goes on");
    assert_eq!(ws.problems(), Vec::<Problem>::new());
}

#[test]
fn invalid_language_server_settings_are_reported_not_fatal() {
    let fx = Fixture::new("invalid");
    let path = fx.file("main.rs", "ERROR\n");
    let settings = serde_json::json!({
        "rustode.languageServers": {
            "python": { "command": 3 },
            "go": { "command": "gopls", "args": "-v" },
            "c": { "args": ["x"], "comand": "clangd" },
            "rust": { "command": server().display().to_string() },
        }
    });
    fx.write_settings(&settings.to_string());
    let mut ws = Workspace::new();
    fx.load(&mut ws);
    let problems = ws.language_server_problems();
    assert_eq!(problems.len(), 4, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| p.contains("\"python\"") && p.contains("command"))
    );
    assert!(
        problems
            .iter()
            .any(|p| p.contains("\"go\"") && p.contains("args"))
    );
    assert!(problems.iter().any(|p| p.contains("\"comand\"")));
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the valid server's diagnostics", |ws| {
        !ws.markers(id).is_empty()
    });

    fx.write_settings("{\"rustode.languageServers\": 1}");
    ws.reload_config();
    assert!(
        ws.config().diagnostics().count() > 0,
        "the wrong type is a settings diagnostic"
    );
    let status = ws.language_server(id).expect("still listed");
    assert_eq!(status.state, ServerState::Stopped, "the server stops");
    assert!(ws.problems().is_empty(), "its diagnostics go with it");

    fx.write_settings(&settings.to_string());
    ws.reload_config();
    wait(&mut ws, "the server back", |ws| !ws.markers(id).is_empty());
    assert_eq!(
        ws.language_servers().len(),
        1,
        "the same server, started again"
    );
}

#[test]
fn closing_a_document_and_dropping_the_workspace_reach_the_server() {
    let fx = Fixture::new("close");
    let path = fx.file("main.rs", "ERROR\n");
    let mut ws = fx.workspace(&[]);
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "diagnostics", |ws| !ws.problems().is_empty());
    ws.close(id, false).expect("close");
    wait(&mut ws, "the server to clear them", |ws| {
        ws.problems().is_empty()
    });
    assert!(fx.methods().contains(&"textDocument/didClose".to_owned()));
    drop(ws);
    let methods = fx.methods();
    let tail: Vec<&str> = methods.iter().rev().take(2).map(String::as_str).collect();
    assert_eq!(tail, ["exit", "shutdown"], "{methods:?}");
}

#[test]
fn problems_cover_every_file_in_vs_code_order() {
    let fx = Fixture::new("order");
    let a = fx.file("a.rs", "WARN\nINFO HINT\n");
    let b = fx.file("b.rs", "x WARN ERROR\nERROR\n");
    let c = fx.file("c.rs", "ERROR\n");
    let report = c.display().to_string();
    let mut ws = fx.workspace(&["--report", &report]);
    ws.open_folder(&fx.dir).expect("folder");
    let a_id = ws.open(&a).expect("open a");
    ws.open(&b).expect("open b");
    wait(&mut ws, "every file", |ws| ws.problems().len() == 6);

    let listed: Vec<(PathBuf, MarkerSeverity, Position)> = ws
        .problems()
        .into_iter()
        .map(|p: Problem| (p.path, p.severity, p.start))
        .collect();
    assert_eq!(
        listed,
        [
            (b.clone(), MarkerSeverity::Error, Position::new(0, 7)),
            (b.clone(), MarkerSeverity::Error, Position::new(1, 0)),
            (b.clone(), MarkerSeverity::Warning, Position::new(0, 2)),
            (c.clone(), MarkerSeverity::Error, Position::new(0, 0)),
            (a.clone(), MarkerSeverity::Warning, Position::new(0, 0)),
            (a.clone(), MarkerSeverity::Info, Position::new(1, 0)),
        ],
        "files with errors first, by path; in a file by severity, then position"
    );
    let counts = ws.problem_counts();
    assert_eq!((counts.errors, counts.warnings, counts.infos), (3, 2, 1));
    assert_eq!(
        covered(&ws, a_id)[2],
        (MarkerSeverity::Hint, "HI".into()),
        "a hint is drawn over its first two chars and is not a problem"
    );

    let problem = ws.problems()[3].clone();
    let c_id = ws.go_to_problem(&problem).expect("go to c.rs");
    assert_eq!(ws.active_document(), Some(c_id));
    let selection = ws.document(c_id).expect("doc").primary_selection();
    assert_eq!((selection.anchor, selection.head), (0, 5));
    assert_eq!(
        covered(&ws, c_id),
        [(MarkerSeverity::Error, "ERROR".into())]
    );
}

#[test]
fn each_root_gets_its_own_server() {
    let fx = Fixture::new("roots");
    let one = fx.file("one/main.rs", "ERROR\n");
    let two = fx.file("two/main.rs", "WARN\n");
    let mut ws = fx.workspace(&[]);
    let first = ws.open(&one).expect("open");
    let second = ws.open(&two).expect("open");
    wait(&mut ws, "both", |ws| {
        !ws.markers(first).is_empty() && !ws.markers(second).is_empty()
    });
    let mut roots: Vec<PathBuf> = ws.language_servers().into_iter().map(|s| s.root).collect();
    roots.sort();
    assert_eq!(roots, [fx.dir.join("one"), fx.dir.join("two")]);
    assert_eq!(
        covered(&ws, second),
        [(MarkerSeverity::Warning, "WARN".into())]
    );
}

#[test]
fn the_waker_is_called_when_a_message_arrives() {
    let fx = Fixture::new("waker");
    let path = fx.file("main.rs", "ERROR\n");
    let mut ws = fx.workspace(&[]);
    let (tx, rx) = mpsc::channel();
    ws.set_waker(move || {
        let _ = tx.send(());
    });
    let id = ws.open(&path).expect("open");
    let deadline = Instant::now() + Duration::from_secs(10);
    while ws.markers(id).is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        rx.recv_timeout(left).expect("woken before the deadline");
        ws.poll_language_servers();
    }
}

#[test]
fn the_commands_are_registered_with_vs_code_keys() {
    let ws = Workspace::new();
    let problems = ws.commands("toggle problems");
    assert_eq!(problems[0].command.id, "workbench.actions.view.problems");
    assert_eq!(problems[0].keybinding.as_deref(), Some("Ctrl+Shift+M"));
    let restart = ws.commands("restart language");
    assert_eq!(restart[0].command.id, "rustode.restartLanguageServer");
}

// ---- hover and completion ----

const CODE: &str = "fn main() {\n    let point = 1;\n    pri\n}\n";

/// A workspace with a running server on `src/main.rs`, and the document.
fn session(fx: &Fixture, args: &[&str]) -> (Workspace, DocumentId) {
    let path = fx.file("src/main.rs", CODE);
    let mut ws = fx.workspace(args);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the server", running);
    (ws, id)
}

/// The offset after the `pri` on the last line of code.
fn end_of_pri() -> usize {
    CODE.find("pri").expect("pri") + 3
}

fn requests(fx: &Fixture, method: &str) -> Vec<serde_json::Value> {
    fx.log()
        .into_iter()
        .filter(|m| m["method"] == method)
        .collect()
}

fn labels(ws: &Workspace) -> Vec<String> {
    ws.completion_list(20).map_or_else(Vec::new, |list| {
        list.rows.into_iter().map(|row| row.label).collect()
    })
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("doc").text()
}

#[test]
fn hover_shows_what_the_server_says_about_the_word() {
    let fx = Fixture::new("hover");
    let (mut ws, id) = session(&fx, &[]);
    let at = CODE.find("point").expect("word") + 2;
    assert!(ws.request_hover(id, at).expect("request"));
    assert!(ws.hover_pending());
    assert!(ws.hover().is_none(), "nothing before the answer");
    wait(&mut ws, "a hover", |ws| ws.hover().is_some());
    let hover = ws.hover().expect("hover").clone();
    assert_eq!(hover.text, "point\n\nlet point", "markdown as plain text");
    assert_eq!(hover.range, Some(Range::new(at - 2, at + 3)));
    assert_eq!(hover.anchor(), at - 2);
    assert_eq!(hover.offset, at);
    assert!(!ws.hover_pending());
    assert!(
        ws.take_events().contains(&Event::HoverChanged),
        "the frontend is told"
    );
    // The same place again asks nothing more.
    assert!(ws.request_hover(id, at).expect("again"));
    assert_eq!(requests(&fx, "textDocument/hover").len(), 1);
    ws.hide_hover();
    assert!(ws.hover().is_none());
}

#[test]
fn hover_over_nothing_shows_nothing() {
    let fx = Fixture::new("hover-none");
    let (mut ws, id) = session(&fx, &[]);
    let blank = CODE.find("    let").expect("indent") + 1;
    assert!(ws.request_hover(id, blank).expect("request"));
    wait(&mut ws, "the answer", |ws| !ws.hover_pending());
    assert!(ws.hover().is_none(), "a null answer is no hover");
}

#[test]
fn a_server_without_hover_is_not_asked() {
    let fx = Fixture::new("hover-off");
    let (mut ws, id) = session(&fx, &["--no-hover", "--no-completion"]);
    assert!(!ws.request_hover(id, 5).expect("request"));
    assert!(!ws.request_completion(id).expect("request"));
    assert_eq!(requests(&fx, "textDocument/hover").len(), 0);
    assert_eq!(requests(&fx, "textDocument/completion").len(), 0);
}

#[test]
fn asking_for_another_place_cancels_the_slow_request() {
    let fx = Fixture::new("hover-cancel");
    let (mut ws, id) = session(&fx, &["--slow", "300"]);
    let first = CODE.find("main").expect("main");
    let second = CODE.find("point").expect("point");
    assert!(ws.request_hover(id, first).expect("first"));
    assert!(ws.request_hover(id, second).expect("second"));
    wait(&mut ws, "the second hover", |ws| ws.hover().is_some());
    assert_eq!(ws.hover().expect("hover").text, "point\n\nlet point");
    let hovers = requests(&fx, "textDocument/hover");
    let cancels = requests(&fx, "$/cancelRequest");
    assert_eq!(hovers.len(), 2);
    assert_eq!(cancels.len(), 1, "the first one was cancelled");
    assert_eq!(cancels[0]["params"]["id"], hovers[0]["id"]);
}

#[test]
fn a_hover_whose_document_changed_is_dropped() {
    let fx = Fixture::new("hover-stale");
    let (mut ws, id) = session(&fx, &["--slow", "200"]);
    assert!(
        ws.request_hover(id, CODE.find("point").expect("point"))
            .expect("request")
    );
    ws.set_cursor(id, 0).expect("cursor");
    ws.type_text(id, "x").expect("type");
    assert!(!ws.hover_pending(), "the edit cancelled it");
    // Let the late answer, or the cancellation error, arrive.
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        ws.poll_language_servers();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ws.hover().is_none());
    assert_eq!(requests(&fx, "$/cancelRequest").len(), 1);
}

#[test]
fn ctrl_space_lists_the_matching_items_best_first() {
    let fx = Fixture::new("complete");
    let (mut ws, id) = session(&fx, &["--sync", "full"]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    assert!(ws.request_completion(id).expect("request"));
    assert!(ws.completion_pending());
    wait(&mut ws, "the list", Workspace::completion_visible);
    let list = ws.completion_list(10).expect("list");
    let names: Vec<&str> = list.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        names,
        ["println", "print", "PrimaryKey"],
        "equal scores keep the sortText order: {list:?}"
    );
    assert_eq!(list.start, end_of_pri() - 3, "under the word");
    assert_eq!((list.selected, list.total, list.first), (0, 3, 0));
    assert_eq!(list.rows[0].matched, vec![0, 1, 2]);
    assert_eq!(list.rows[1].detail.as_deref(), Some("fn print()"));
    assert_eq!(list.rows[0].kind, CompletionKind::Function);
    assert_eq!(list.rows[2].matched, vec![0, 1, 2]);
    let request = &requests(&fx, "textDocument/completion")[0];
    assert_eq!(request["params"]["context"]["triggerKind"], 1);
    assert_eq!(
        request["params"]["position"],
        serde_json::json!({"line": 2, "character": 7})
    );
}

#[test]
fn typing_narrows_the_list_without_asking_again() {
    let fx = Fixture::new("complete-filter");
    let (mut ws, id) = session(&fx, &[]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert_eq!(labels(&ws), ["println", "print"]);
    ws.type_text(id, "l").expect("type");
    assert_eq!(labels(&ws), ["println"], "narrowed by the filter alone");
    ws.delete_left(id).expect("backspace");
    assert_eq!(labels(&ws), ["println", "print"], "and widened again");
    assert_eq!(
        requests(&fx, "textDocument/completion").len(),
        1,
        "one request for the whole word"
    );
    ws.type_text(id, "z").expect("type");
    assert!(!ws.completion_visible(), "nothing matches");
}

#[test]
fn accepting_replaces_the_word_as_one_undo_step() {
    let fx = Fixture::new("complete-accept");
    let (mut ws, id) = session(&fx, &[]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert_eq!(ws.completion_list(10).expect("list").selected, 0);
    assert!(ws.completion_accept().expect("accept"));
    assert!(
        text(&ws, id).contains("    println!()\n"),
        "{}",
        text(&ws, id)
    );
    assert!(!ws.completion_visible(), "the list closes");
    assert_eq!(
        ws.document(id).expect("doc").cursor(),
        end_of_pri() - 3 + "println!()".len()
    );
    assert!(ws.undo(id).expect("undo"));
    assert!(
        text(&ws, id).contains("    prin\n"),
        "one step: {}",
        text(&ws, id)
    );
    assert!(ws.redo(id).expect("redo"));
    assert!(text(&ws, id).contains("    println!()\n"));
}

#[test]
fn an_item_with_only_insert_text_replaces_the_word_before_the_cursor() {
    let fx = Fixture::new("complete-insert");
    let (mut ws, id) = session(&fx, &[]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert!(ws.completion_move(CompletionMove::Next));
    assert_eq!(
        ws.completion_list(10).expect("list").rows[1].label,
        "print",
        "the second, with no edit of its own"
    );
    assert!(ws.completion_accept().expect("accept"));
    assert!(text(&ws, id).contains("    print\n"), "{}", text(&ws, id));
}

#[test]
fn the_trigger_character_asks_and_filters_by_what_follows() {
    let fx = Fixture::new("complete-dot");
    let path = fx.file("src/main.rs", "fn main() {\n    v\n}\n");
    let mut ws = fx.workspace(&[]);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the server", running);
    ws.set_cursor(id, 17).expect("cursor");
    ws.type_text(id, ".").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert_eq!(
        labels(&ws),
        ["len", "push", "pop"],
        "provider order, no word"
    );
    let request = &requests(&fx, "textDocument/completion")[0];
    assert_eq!(request["params"]["context"]["triggerKind"], 2);
    assert_eq!(request["params"]["context"]["triggerCharacter"], ".");
    ws.type_text(id, "p").expect("type");
    assert_eq!(labels(&ws), ["push", "pop"]);
    assert!(ws.completion_accept().expect("accept"));
    assert!(text(&ws, id).contains("    v.push\n"), "{}", text(&ws, id));
}

#[test]
fn quick_suggestions_follow_the_setting() {
    let fx = Fixture::new("complete-quick");
    let (mut ws, id) = session(&fx, &[]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    wait(&mut ws, "typing opens the list", |ws| {
        ws.completion_visible()
    });
    ws.type_text(id, " ").expect("type");
    assert!(!ws.completion_visible(), "a space ends the word");

    let fx = Fixture::new("complete-quick-off");
    let path = fx.file("src/main.rs", CODE);
    fx.write_settings(&fx.settings_text(&[]).replacen(
        '{',
        "{\"editor.quickSuggestions\": false,",
        1,
    ));
    let mut ws = Workspace::new();
    fx.load(&mut ws);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the server", running);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    std::thread::sleep(Duration::from_millis(200));
    ws.poll_language_servers();
    assert_eq!(requests(&fx, "textDocument/completion").len(), 0);
    assert!(
        ws.request_completion(id).expect("by hand"),
        "Ctrl+Space still works"
    );
}

#[test]
fn a_slow_answer_for_an_earlier_character_is_dropped() {
    let fx = Fixture::new("complete-stale");
    let (mut ws, id) = session(&fx, &["--slow", "250"]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    ws.type_text(id, "t").expect("type again before the answer");
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert_eq!(
        labels(&ws),
        ["print", "println"],
        "a full match first: {:?}",
        labels(&ws)
    );
    let asked = requests(&fx, "textDocument/completion");
    assert_eq!(asked.len(), 2, "each character asked");
    let cancels = requests(&fx, "$/cancelRequest");
    assert_eq!(cancels.len(), 1, "the first was cancelled");
    assert_eq!(cancels[0]["params"]["id"], asked[0]["id"]);
    assert_eq!(
        asked[1]["params"]["position"],
        serde_json::json!({"line": 2, "character": 9})
    );
}

#[test]
fn moving_out_of_the_word_or_escape_ends_the_list() {
    let fx = Fixture::new("complete-end");
    let (mut ws, id) = session(&fx, &[]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    assert!(ws.request_completion(id).expect("request"));
    wait(&mut ws, "the list", Workspace::completion_visible);
    ws.set_cursor(id, end_of_pri() - 1).expect("left");
    assert!(
        ws.completion_visible(),
        "still in the word: filtered by 'pr'"
    );
    ws.set_cursor(id, end_of_pri() - 4)
        .expect("out of the word");
    assert!(!ws.completion_visible());
    ws.set_cursor(id, end_of_pri()).expect("back");
    assert!(!ws.completion_visible() || ws.completion_list(10).is_some());
    ws.dismiss_completion();
    assert!(!ws.completion_visible());
    assert!(!ws.completion_accept().expect("nothing to accept"));
    assert!(!ws.completion_move(CompletionMove::Next));
}

#[test]
fn the_selection_wraps_and_pages_stop_at_the_ends() {
    let fx = Fixture::new("complete-move");
    let path = fx.file("src/main.rs", "fn main() {\n    v\n}\n");
    let mut ws = fx.workspace(&[]);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the server", running);
    ws.set_cursor(id, 17).expect("cursor");
    ws.type_text(id, ".").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    let selected = |ws: &Workspace| ws.completion_list(2).expect("list").selected;
    assert!(ws.completion_move(CompletionMove::Previous));
    assert_eq!(selected(&ws), 2, "up from the first goes to the last");
    ws.completion_move(CompletionMove::Next);
    assert_eq!(selected(&ws), 0, "down from the last goes to the first");
    ws.completion_move(CompletionMove::PageDown(10));
    assert_eq!(selected(&ws), 2, "a page stops at the end");
    let list = ws.completion_list(2).expect("list");
    assert_eq!(
        (list.first, list.rows.len()),
        (1, 2),
        "scrolled to keep it in view"
    );
    ws.completion_move(CompletionMove::PageUp(10));
    assert_eq!(selected(&ws), 0);
    assert_eq!(ws.completion_list(2).expect("list").first, 0);
}

#[test]
fn an_incomplete_list_is_asked_again_while_typing() {
    let fx = Fixture::new("complete-incomplete");
    let (mut ws, id) = session(&fx, &["--incomplete"]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    ws.type_text(id, "n").expect("type");
    wait(&mut ws, "the list", Workspace::completion_visible);
    ws.type_text(id, "t").expect("type");
    assert!(
        ws.completion_visible(),
        "the old list stays until the new one comes"
    );
    wait(&mut ws, "the second request", |_| {
        requests(&fx, "textDocument/completion").len() == 2
    });
    wait(&mut ws, "its answer", |ws| !ws.completion_pending());
    let asked = requests(&fx, "textDocument/completion");
    assert_eq!(asked[1]["params"]["context"]["triggerKind"], 3);
    assert_eq!(labels(&ws), ["print", "println"]);
}

#[test]
fn a_snippet_goes_in_as_text_with_its_defaults() {
    let fx = Fixture::new("complete-snippet");
    let path = fx.file("src/main.rs", "fn main() {\n    fm\n}\n");
    let mut ws = fx.workspace(&[]);
    ws.open_folder(&fx.dir).expect("folder");
    let id = ws.open(&path).expect("open");
    wait(&mut ws, "the server", running);
    ws.set_cursor(id, 18).expect("cursor");
    assert!(ws.request_completion(id).expect("request"));
    wait(&mut ws, "the list", Workspace::completion_visible);
    assert_eq!(labels(&ws), ["fmt"]);
    assert!(ws.completion_accept().expect("accept"));
    assert!(
        text(&ws, id).contains("    fmt(value, a)\n"),
        "{}",
        text(&ws, id)
    );
}

#[test]
fn a_crash_ends_what_was_asked() {
    let fx = Fixture::new("complete-crash");
    let (mut ws, id) = session(&fx, &["--slow", "5000"]);
    ws.set_cursor(id, end_of_pri()).expect("cursor");
    assert!(ws.request_completion(id).expect("request"));
    assert!(ws.request_hover(id, 3).expect("hover"));
    ws.type_text(id, " CRASH").expect("type");
    wait(&mut ws, "the crash", |ws| {
        ws.language_servers().iter().any(|s| s.restarts > 0)
    });
    assert!(
        !ws.completion_pending() && !ws.hover_pending(),
        "nobody waits for a dead server"
    );
}
