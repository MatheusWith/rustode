//! Workspace seam: the built-in typescript-language-server runs on a
//! TypeScript 6 `tsserver.js` and never on TypeScript 7. The program is
//! `rustode-test-lsp` under the server's name; it logs what it receives to
//! `.rustode-test-lsp.log` in the workspace root, which shows the
//! `initializationOptions.tsserver.path` it was given. The TypeScript
//! packages are fake folders with a `package.json` and an empty
//! `lib/tsserver.js`.

use rustode_workspace::{ServerState, ServerStatus, Workspace};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
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

const NAME: &str = "typescript-language-server";

struct Fixture {
    dir: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-ts6-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("project");
        std::fs::create_dir_all(dir.join("user")).expect("user dir");
        std::fs::create_dir_all(dir.join("bin")).expect("bin dir");
        std::fs::create_dir_all(root.join("src")).expect("project dir");
        std::os::unix::fs::symlink(server(), dir.join("bin").join(NAME)).expect("link");
        std::fs::write(root.join("src/a.ts"), "let x = 1;\n").expect("source");
        Self { dir, root }
    }

    /// A fake TypeScript package at `package`; `version` `None` writes no
    /// readable one. Returns its `lib` folder.
    fn typescript(package: &Path, version: Option<&str>) -> PathBuf {
        let lib = package.join("lib");
        std::fs::create_dir_all(&lib).expect("lib");
        std::fs::write(lib.join("tsserver.js"), "").expect("tsserver.js");
        let text = version.map_or_else(
            || "{ not json".to_owned(),
            |v| format!("{{\"name\": \"typescript\", \"version\": \"{v}\"}}"),
        );
        std::fs::write(package.join("package.json"), text).expect("package.json");
        lib
    }

    fn local(&self, version: Option<&str>) -> PathBuf {
        Self::typescript(&self.root.join("node_modules/typescript"), version)
    }

    fn search_path(&self) -> OsString {
        self.dir.join("bin").into_os_string()
    }

    fn settings(&self, tsdk: Option<&Path>) {
        let settings = tsdk.map_or_else(
            || "{}".to_owned(),
            |t| serde_json::json!({"rustode.typescript.tsdk": t}).to_string(),
        );
        std::fs::write(self.dir.join("user/settings.json"), settings).expect("settings");
    }

    fn user(&self) -> (PathBuf, PathBuf) {
        let user = self.dir.join("user");
        (user.join("settings.json"), user.join("keybindings.json"))
    }

    /// A workspace on the project folder with `tsdk` set and the defaults on.
    fn workspace(&self, tsdk: Option<&Path>) -> Workspace {
        let mut ws = Workspace::new();
        self.settings(tsdk);
        let (settings, keys) = self.user();
        ws.load_config(&settings, &keys);
        ws.open_folder(&self.root).expect("folder");
        ws.use_default_language_servers(&self.search_path());
        ws
    }

    /// The `tsserver.path` of the last `initialize` the server received.
    fn tsserver_path(&self) -> Option<String> {
        let log = std::fs::read_to_string(self.root.join(".rustode-test-lsp.log")).ok()?;
        log.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .rfind(|m| m["method"] == "initialize")?["params"]["initializationOptions"]["tsserver"]
            ["path"]
            .as_str()
            .map(str::to_owned)
    }

    fn started(&self) -> bool {
        self.root.join(".rustode-test-lsp.log").exists()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn status(ws: &Workspace) -> ServerStatus {
    let mut servers = ws.language_servers();
    assert_eq!(servers.len(), 1, "{servers:?}");
    servers.remove(0)
}

#[track_caller]
fn wait_for(ws: &mut Workspace, what: &str, done: impl Fn(&Workspace) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(ws) {
        ws.poll_language_servers();
        assert!(
            Instant::now() < deadline,
            "{what}: {:?}",
            ws.language_servers()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[track_caller]
fn running(ws: &mut Workspace) {
    wait_for(ws, "running", |ws| {
        ws.language_servers()
            .first()
            .is_some_and(|s| s.state == ServerState::Running)
    });
}

fn source(f: &Fixture) -> PathBuf {
    f.root.join("src/a.ts")
}

#[test]
fn a_local_typescript_6_wins_over_the_setting() {
    let f = Fixture::new("local");
    let local = f.local(Some("6.0.3"));
    let other = Fixture::typescript(&f.dir.join("other"), Some("6.0.2"));
    let mut ws = f.workspace(Some(&other));
    ws.open(&source(&f)).expect("open");
    running(&mut ws);
    wait_for(&mut ws, "the log", |_| f.tsserver_path().is_some());
    assert_eq!(
        f.tsserver_path(),
        Some(local.join("tsserver.js").display().to_string())
    );
}

#[test]
fn the_setting_is_used_and_a_changed_setting_applies_live() {
    let f = Fixture::new("setting");
    let first = Fixture::typescript(&f.dir.join("one"), Some("6.0.3"));
    let second = Fixture::typescript(&f.dir.join("two"), Some("6.1.0"));
    let mut ws = f.workspace(Some(&first));
    ws.open(&source(&f)).expect("open");
    running(&mut ws);
    wait_for(&mut ws, "the log", |_| f.tsserver_path().is_some());
    assert_eq!(
        f.tsserver_path(),
        Some(first.join("tsserver.js").display().to_string())
    );

    f.settings(Some(&second));
    ws.reload_config();
    wait_for(&mut ws, "the restart on the new setting", |_| {
        f.tsserver_path() == Some(second.join("tsserver.js").display().to_string())
    });
    running(&mut ws);
    assert_eq!(status(&ws).message, None);
}

#[test]
fn a_relative_setting_is_relative_to_the_workspace_folder() {
    let f = Fixture::new("relative");
    let lib = Fixture::typescript(&f.root.join("tools/ts"), Some("6.0.3"));
    let mut ws = f.workspace(Some(Path::new("tools/ts/lib")));
    ws.open(&source(&f)).expect("open");
    running(&mut ws);
    wait_for(&mut ws, "the log", |_| f.tsserver_path().is_some());
    assert_eq!(
        f.tsserver_path(),
        Some(lib.join("tsserver.js").display().to_string())
    );
}

#[test]
fn only_typescript_7_is_refused_with_a_message_and_the_setting_fixes_it_live() {
    let f = Fixture::new("seven");
    // As the real TypeScript 7 package is: a version 7 and no tsserver.js.
    let lib = f.local(Some("7.0.2"));
    std::fs::remove_file(lib.join("tsserver.js")).expect("remove");
    let mut ws = f.workspace(None);
    ws.open(&source(&f)).expect("open");
    let got = status(&ws);
    assert_eq!(got.state, ServerState::Crashed);
    let message = got.message.expect("a message");
    for part in [
        "needs TypeScript 6",
        "is TypeScript 7",
        "rustode.typescript.tsdk",
    ] {
        assert!(message.contains(part), "{part} in {message}");
    }
    assert!(!f.started(), "the server must not start");
    ws.poll_language_servers();
    assert_eq!(
        status(&ws).state,
        ServerState::Crashed,
        "and is not restarted"
    );

    let six = Fixture::typescript(&f.dir.join("six"), Some("6.0.3"));
    f.settings(Some(&six));
    ws.reload_config();
    running(&mut ws);
    wait_for(&mut ws, "the log", |_| f.tsserver_path().is_some());
    assert_eq!(
        f.tsserver_path(),
        Some(six.join("tsserver.js").display().to_string())
    );
}

#[test]
fn nothing_found_is_refused_with_the_setting_named() {
    let f = Fixture::new("nothing");
    let mut ws = f.workspace(None);
    ws.open(&source(&f)).expect("open");
    let got = status(&ws);
    assert_eq!(got.state, ServerState::Crashed);
    let message = got.message.expect("a message");
    assert!(
        message.contains("needs TypeScript 6") && message.contains("rustode.typescript.tsdk"),
        "{message}"
    );
    assert!(!f.started());
}

#[test]
fn a_version_that_cannot_be_read_is_unknown_and_refused() {
    let f = Fixture::new("unknown");
    f.local(None);
    let mut ws = f.workspace(None);
    ws.open(&source(&f)).expect("open");
    let got = status(&ws);
    assert_eq!(got.state, ServerState::Crashed);
    let message = got.message.expect("a message");
    assert!(message.contains("could not be read"), "{message}");
    assert!(!f.started());

    // An unknown local does not block a good setting.
    let six = Fixture::typescript(&f.dir.join("six"), Some("6.0.3"));
    f.settings(Some(&six));
    ws.reload_config();
    running(&mut ws);
}

#[test]
fn a_global_typescript_6_next_to_the_language_server_is_found() {
    let f = Fixture::new("global");
    // <dir>/lib/node_modules/{typescript-language-server,typescript}: npm's
    // global root. The program is a copy inside its package, so that the
    // link on the search path resolves there.
    let modules = f.dir.join("lib/node_modules");
    let package = modules.join(NAME);
    std::fs::create_dir_all(&package).expect("package");
    std::fs::copy(server(), package.join("cli")).expect("copy");
    let bin = f.dir.join("bin").join(NAME);
    std::fs::remove_file(&bin).expect("remove link");
    std::os::unix::fs::symlink(package.join("cli"), &bin).expect("relink");
    let lib = Fixture::typescript(&modules.join("typescript"), Some("6.0.3"));
    let mut ws = f.workspace(None);
    ws.open(&source(&f)).expect("open");
    running(&mut ws);
    wait_for(&mut ws, "the log", |_| f.tsserver_path().is_some());
    assert_eq!(
        f.tsserver_path(),
        Some(lib.join("tsserver.js").display().to_string())
    );
}

#[test]
fn a_users_own_typescript_entry_is_left_alone() {
    let f = Fixture::new("own");
    f.local(Some("7.0.2"));
    let command = f.dir.join("bin").join(NAME).display().to_string();
    std::fs::write(
        f.dir.join("user/settings.json"),
        serde_json::json!({"rustode.languageServers": {"typescript": {"command": command}}})
            .to_string(),
    )
    .expect("settings");
    let mut ws = Workspace::new();
    let (settings, keys) = f.user();
    ws.load_config(&settings, &keys);
    ws.open_folder(&f.root).expect("folder");
    ws.use_default_language_servers(&f.search_path());
    ws.open(&source(&f)).expect("open");
    running(&mut ws);
    assert_eq!(f.tsserver_path(), None, "no tsserver.path is added");
}
