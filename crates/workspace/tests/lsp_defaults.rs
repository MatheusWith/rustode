//! Workspace seam: the built-in language servers. A frontend that calls
//! [`Workspace::use_default_language_servers`] gets `rust-analyzer` for Rust
//! and `typescript-language-server` for TypeScript and JavaScript when they
//! are on the search path, unless the user's `rustode.languageServers` says
//! otherwise. The programs are links to `rustode-test-lsp` in a temporary
//! folder that stands in for `PATH`.

use rustode_workspace::{DocumentId, ServerState, Workspace};
use std::ffi::OsString;
use std::path::PathBuf;
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

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rustode-defaults-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("user")).expect("temp dir");
        std::fs::create_dir_all(dir.join("bin")).expect("bin dir");
        Self { dir }
    }

    /// Puts a program named `name` in the fake `PATH`.
    fn install(&self, name: &str) {
        std::os::unix::fs::symlink(server(), self.dir.join("bin").join(name)).expect("link");
    }

    fn search_path(&self) -> OsString {
        self.dir.join("bin").into_os_string()
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, "let x = 1;\n").expect("fixture");
        path
    }

    fn settings(&self, text: &str) {
        std::fs::write(self.dir.join("user/settings.json"), text).expect("settings");
    }

    /// A workspace with the defaults on and `settings` loaded first.
    fn workspace(&self, settings: Option<&str>) -> Workspace {
        let mut ws = Workspace::new();
        if let Some(text) = settings {
            self.settings(text);
            let user = self.dir.join("user");
            ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
        }
        ws.use_default_language_servers(&self.search_path());
        ws
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn names(ws: &Workspace) -> Vec<(String, String)> {
    ws.language_servers()
        .into_iter()
        .map(|s| (s.language, s.name))
        .collect()
}

#[track_caller]
fn running(ws: &mut Workspace, id: DocumentId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while ws
        .language_server(id)
        .is_none_or(|s| s.state != ServerState::Running)
    {
        ws.poll_language_servers();
        assert!(Instant::now() < deadline, "{:?}", ws.language_servers());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn rust_files_get_rust_analyzer_without_any_settings() {
    let f = Fixture::new("rust");
    f.install("rust-analyzer");
    let mut ws = f.workspace(None);
    let id = ws.open(&f.file("a.rs")).expect("open");
    running(&mut ws, id);
    assert_eq!(names(&ws), [("rust".into(), "rust-analyzer".into())]);
}

#[test]
fn typescript_and_javascript_get_typescript_language_server() {
    let f = Fixture::new("ts");
    f.install("typescript-language-server");
    let mut ws = f.workspace(None);
    let ts = ws.open(&f.file("a.ts")).expect("open");
    let js = ws.open(&f.file("b.js")).expect("open");
    let tsx = ws.open(&f.file("c.tsx")).expect("open");
    running(&mut ws, ts);
    running(&mut ws, js);
    running(&mut ws, tsx);
    let mut got = names(&ws);
    got.sort();
    let name = "typescript-language-server";
    assert_eq!(
        got,
        [
            ("javascript".to_owned(), name.to_owned()),
            ("typescript".to_owned(), name.to_owned()),
            ("typescriptreact".to_owned(), name.to_owned()),
        ]
    );
}

#[test]
fn a_program_that_is_not_installed_starts_nothing() {
    let f = Fixture::new("absent");
    f.install("typescript-language-server");
    let mut ws = f.workspace(None);
    let id = ws.open(&f.file("a.rs")).expect("open");
    ws.poll_language_servers();
    assert!(ws.language_server(id).is_none());
    assert_eq!(names(&ws).len(), 0);
    assert_eq!(ws.language_server_problems().len(), 0);
}

#[test]
fn a_plain_workspace_starts_no_server_even_when_the_program_exists() {
    let f = Fixture::new("plain");
    f.install("rust-analyzer");
    let mut ws = Workspace::new();
    let id = ws.open(&f.file("a.rs")).expect("open");
    ws.poll_language_servers();
    assert!(ws.language_server(id).is_none());
}

#[test]
fn the_users_entry_wins_over_the_default() {
    let f = Fixture::new("user");
    f.install("rust-analyzer");
    f.install("my-ra");
    let command = f.dir.join("bin/my-ra").display().to_string();
    let settings = serde_json::json!({
        "rustode.languageServers": {"rust": {"command": command}}
    });
    let mut ws = f.workspace(Some(&settings.to_string()));
    let id = ws.open(&f.file("a.rs")).expect("open");
    running(&mut ws, id);
    assert_eq!(names(&ws), [("rust".into(), "my-ra".into())]);
}

#[test]
fn null_switches_a_default_off_and_the_others_stay() {
    let f = Fixture::new("null");
    f.install("rust-analyzer");
    f.install("typescript-language-server");
    let settings = r#"{"rustode.languageServers": {"rust": null}}"#;
    let mut ws = f.workspace(Some(settings));
    let rs = ws.open(&f.file("a.rs")).expect("open");
    let ts = ws.open(&f.file("a.ts")).expect("open");
    running(&mut ws, ts);
    assert!(ws.language_server(rs).is_none());
    assert!(
        ws.language_server_problems().is_empty(),
        "null is not a problem"
    );
}

#[test]
fn settings_loaded_after_the_defaults_still_win_and_documents_attach() {
    let f = Fixture::new("late");
    f.install("rust-analyzer");
    f.install("my-ra");
    let mut ws = Workspace::new();
    let id = ws.open(&f.file("a.rs")).expect("open");
    ws.use_default_language_servers(&f.search_path());
    running(&mut ws, id);
    assert_eq!(names(&ws), [("rust".into(), "rust-analyzer".into())]);
    let command = f.dir.join("bin/my-ra").display().to_string();
    f.settings(
        &serde_json::json!({"rustode.languageServers": {"rust": {"command": command}}}).to_string(),
    );
    let user = f.dir.join("user");
    ws.load_config(&user.join("settings.json"), &user.join("keybindings.json"));
    running(&mut ws, id);
    assert_eq!(names(&ws), [("rust".into(), "my-ra".into())]);
}
