//! Workspace seam: language servers configured in `settings.json`, started
//! per workspace root and language, their diagnostics on the documents and
//! in the problems list, and supervision (crash, restart, give up).
//!
//! The server is `rustode-test-lsp` (`crates/test-lsp`), a stdio test server
//! that publishes a diagnostic over every `ERROR`, `WARN`, `INFO` and `HINT`
//! and exits abruptly on `CRASH`.

use rustode_workspace::{
    DocumentId, Event, MarkerSeverity, Position, Problem, Range, ServerState, Workspace,
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
