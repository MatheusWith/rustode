//! Workspace seam, real servers: rust-analyzer and typescript-language-server
//! with no settings at all, as a user who just installed them gets them
//! ([`Workspace::use_default_language_servers`]).
//!
//! These tests are `#[ignore]`d: they need the programs on `PATH` (a test
//! skips itself, saying so, when its program is absent), they take seconds to
//! minutes, and rust-analyzer on a whole workspace can take 1.5 GB. Run them
//! one at a time:
//!
//! ```text
//! cargo nextest run -p rustode-workspace --test lsp_real --run-ignored only \
//!     -E 'test(real_)' --test-threads 1
//! ```
//!
//! Every test aborts, and so ends its server, when `MemAvailable` drops below
//! 700 MB (AGENTS.md §11.5), and ends its server when it finishes. The
//! `latency_*` tests print the startup and request times they measured, with
//! percentiles, on stderr (`--no-capture` shows them); `RUSTODE_LSP_REPS`
//! sets the number of repetitions (default 3 startups, 15 requests each).

use rustode_workspace::{DocumentId, MarkerSeverity, Position, ServerState, Workspace};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Below this, a test aborts (AGENTS.md §11.5).
const MIN_AVAILABLE_MB: u64 = 700;

fn search_path() -> OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

fn on_path(program: &str) -> bool {
    std::env::split_paths(&search_path()).any(|dir| dir.join(program).is_file())
}

fn available_mb() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("MemAvailable:"))
                .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        })
        .map_or(u64::MAX, |kb| kb / 1024)
}

/// A temporary folder, removed at the end.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-real-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }

    fn file(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(path, text).expect("fixture");
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A workspace with the built-in servers on, woken by the servers' readers.
struct Session {
    ws: Workspace,
    woken: Receiver<()>,
}

impl Session {
    fn new(folder: &Path) -> Self {
        let mut ws = Workspace::new();
        let (tx, woken) = mpsc::channel();
        ws.set_waker(move || {
            let _ = tx.send(());
        });
        ws.open_folder(folder).expect("folder");
        ws.use_default_language_servers(&search_path());
        Self { ws, woken }
    }

    /// Polls until `done`, waking when a server speaks; fails after
    /// `limit`, or at once when memory runs short.
    #[track_caller]
    fn wait(&mut self, what: &str, limit: Duration, done: impl Fn(&Workspace) -> bool) -> Duration {
        let started = Instant::now();
        loop {
            self.ws.poll_language_servers();
            if done(&self.ws) {
                return started.elapsed();
            }
            assert!(
                available_mb() >= MIN_AVAILABLE_MB,
                "memory ran short while waiting for {what}"
            );
            assert!(
                started.elapsed() < limit,
                "timed out waiting for {what}; servers: {:?}",
                self.ws.language_servers()
            );
            let _ = self.woken.recv_timeout(Duration::from_millis(50));
        }
    }

    fn offset_after(&self, id: DocumentId, needle: &str) -> usize {
        let text = self.ws.document(id).expect("document").text();
        let at = text.find(needle).unwrap_or_else(|| panic!("no {needle:?}"));
        text[..at + needle.len()].chars().count()
    }

    fn offset_of(&self, id: DocumentId, needle: &str) -> usize {
        self.offset_after(id, needle) - needle.chars().count()
    }

    fn slice(&self, id: DocumentId, start: usize, end: usize) -> String {
        let text = self.ws.document(id).expect("document").text();
        text.chars().skip(start).take(end - start).collect()
    }

    /// Hover at `offset`: its text and how long it took.
    fn hover(&mut self, id: DocumentId, offset: usize, limit: Duration) -> (String, Duration) {
        let started = Instant::now();
        self.ws.hide_hover();
        assert!(self.ws.request_hover(id, offset).expect("doc"), "no hover");
        self.wait("a hover", limit, |ws| !ws.hover_pending());
        let text = self.ws.hover().map(|h| h.text.clone()).unwrap_or_default();
        (text, started.elapsed())
    }

    /// The labels of the suggest list at `offset`, and how long it took.
    fn complete(
        &mut self,
        id: DocumentId,
        offset: usize,
        limit: Duration,
    ) -> (Vec<String>, Duration) {
        let started = Instant::now();
        self.ws.set_cursor(id, offset).expect("cursor");
        assert!(
            self.ws.request_completion(id).expect("doc"),
            "no completion"
        );
        self.wait("a completion", limit, |ws| !ws.completion_pending());
        let labels = self
            .ws
            .completion_list(500)
            .map(|list| list.rows.into_iter().map(|r| r.label).collect())
            .unwrap_or_default();
        (labels, started.elapsed())
    }

    /// Definition at `offset`: where it went (path, line), and the time.
    fn definition(
        &mut self,
        id: DocumentId,
        offset: usize,
        limit: Duration,
    ) -> (Option<(PathBuf, usize)>, Duration) {
        let started = Instant::now();
        assert!(
            self.ws.request_definition(id, offset).expect("doc"),
            "no definition"
        );
        self.wait("a definition", limit, |ws| !ws.navigation_pending());
        let elapsed = started.elapsed();
        let here = self.ws.active_document().and_then(|d| self.ws.document(d));
        let place = here.map(|doc| {
            let cursor = doc.position_of(doc.cursor());
            (doc.path().to_path_buf(), cursor.line)
        });
        (place, elapsed)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Stops every server: `shutdown`, `exit`, and a kill after a second.
        self.ws = Workspace::new();
    }
}

fn p50_p95(samples: &mut [Duration]) -> (Duration, Duration) {
    samples.sort();
    // Nearest rank: the smallest sample with at least `percent` % at or below.
    let at = |percent: usize| {
        let rank = (percent * samples.len()).div_ceil(100);
        samples[rank.clamp(1, samples.len()) - 1]
    };
    (at(50), at(95))
}

fn report(what: &str, samples: &mut [Duration]) {
    let (p50, p95) = p50_p95(samples);
    let max = samples.last().copied().unwrap_or_default();
    eprintln!(
        "MEASURED {what}: n={} p50={p50:?} p95={p95:?} max={max:?}",
        samples.len()
    );
}

fn reps(default: usize) -> usize {
    std::env::var("RUSTODE_LSP_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const RUST_MAIN: &str = r#"struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn norm(&self) -> i32 {
        self.x + self.y
    }
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let p = Point { x: 1, y: 2 };
    let text = "héllo 😀"; let broken: i32 = "x";
    let sum = add(p.x, p.y);
    let n = p.norm();
    println!("{text} {sum} {n} {broken}");
}
"#;

fn rust_fixture() -> Dir {
    let dir = Dir::new("rust");
    dir.file(
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    dir.file("src/main.rs", RUST_MAIN);
    dir
}

const TS_MAIN: &str = r#"export function add(a: number, b: number): number {
  return a + b;
}

const p = { x: 1, y: 2 };
const text = "héllo 😀"; const broken: number = "x";
const sum = add(p.x, p.y);
console.log(text, sum, broken);
"#;

/// A TypeScript package with `lib/tsserver.js`, which typescript-language-server
/// needs: `RUSTODE_TEST_TYPESCRIPT` (a package folder), else the global one.
/// TypeScript 7 ships no `tsserver.js`, so a machine with only that one
/// cannot run this test (and typescript-language-server cannot run there
/// either, outside a project that has its own TypeScript 6).
fn typescript_package() -> Option<PathBuf> {
    let explicit = std::env::var_os("RUSTODE_TEST_TYPESCRIPT").map(PathBuf::from);
    explicit
        .into_iter()
        .chain([PathBuf::from("/usr/lib/node_modules/typescript")])
        .find(|dir| dir.join("lib/tsserver.js").is_file())
}

fn ts_fixture(typescript: &Path) -> Dir {
    let dir = Dir::new("ts");
    // The workspace's own TypeScript, as a project that installed it has.
    std::fs::create_dir_all(dir.0.join("node_modules")).expect("folder");
    std::os::unix::fs::symlink(typescript, dir.0.join("node_modules/typescript")).expect("link");
    dir.file(
        "tsconfig.json",
        r#"{"compilerOptions": {"strict": true, "target": "es2020", "module": "commonjs"}, "include": ["src"]}"#,
    );
    dir.file("src/main.ts", TS_MAIN);
    dir
}

/// Edits after an emoji on the same line, then renames: the server must see
/// the change at the right column (so its diagnostic goes away), and its
/// rename must land on every use.
#[track_caller]
fn edit_then_rename(s: &mut Session, id: DocumentId, literal: &str, fix: &str, decl: &str) {
    let start = s.offset_of(id, literal);
    let range = rustode_workspace::Range::new(start, start + literal.chars().count());
    s.ws.replace(id, range, fix).expect("replace");
    // rust-analyzer's `cargo check` diagnostics refresh on save only.
    s.ws.save(id).expect("save");
    s.wait("the error to go away", START, |ws| {
        ws.markers(id)
            .iter()
            .all(|m| m.severity != MarkerSeverity::Error)
    });
    let at = s.offset_of(id, decl);
    s.ws.set_cursor(id, at + 1).expect("cursor");
    assert!(s.ws.rename_symbol(id, "plus").expect("doc"), "no rename");
    s.wait("the rename", REQUEST, |ws| {
        ws.document(id)
            .is_some_and(|d| d.text().contains("plus(p.x"))
    });
    let text = s.ws.document(id).expect("doc").text();
    assert!(!text.contains("add("), "{text}");
}

const START: Duration = Duration::from_secs(120);
const REQUEST: Duration = Duration::from_secs(30);

/// Opens `file`, waits for the server to run and for the diagnostic over
/// `"x"`; returns the document, the time to Running and the time to that
/// diagnostic.
#[track_caller]
fn start(s: &mut Session, file: &Path) -> (DocumentId, Duration, Duration) {
    let started = Instant::now();
    let id = s.ws.open(file).expect("open");
    s.wait("the server to run", START, |ws| {
        ws.language_server(id)
            .is_some_and(|st| st.state == ServerState::Running)
    });
    let running = started.elapsed();
    let first = s.wait("the diagnostic over \"x\"", START, |ws| {
        ws.markers(id)
            .iter()
            .any(|m| m.severity == MarkerSeverity::Error)
    });
    let _ = first;
    (id, running, started.elapsed())
}

#[test]
#[ignore = "real server: needs rust-analyzer; run with --run-ignored only"]
fn real_rust_analyzer_on_a_fixture_crate() {
    if !on_path("rust-analyzer") {
        eprintln!("SKIPPED: rust-analyzer is not on PATH");
        return;
    }
    let dir = rust_fixture();
    let mut s = Session::new(&dir.0);
    let (id, running, diagnostic) = start(&mut s, &dir.0.join("src/main.rs"));
    eprintln!("MEASURED fixture crate: running={running:?} first error={diagnostic:?}");

    // Diagnostics: the error is over `"x"`, after an emoji, so UTF-16 columns
    // were converted to chars.
    let broken =
        s.ws.markers(id)
            .into_iter()
            .find(|m| m.severity == MarkerSeverity::Error)
            .expect("an error");
    assert_eq!(s.slice(id, broken.range.start, broken.range.end), "\"x\"");

    // Hover over a call of `add`: waits for indexing, hence the retries.
    let at = s.offset_of(id, "add(p.x");
    let mut hover = String::new();
    for _ in 0..40 {
        hover = s.hover(id, at + 1, REQUEST).0;
        if hover.contains("fn add") {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(hover.contains("fn add(a: i32, b: i32) -> i32"), "{hover}");

    // Completion after `p.`.
    let after_dot = s.offset_after(id, "let n = p.");
    let (labels, _) = s.complete(id, after_dot, REQUEST);
    // (rust-analyzer 1 (9074e9b4c6) offers `norm` and the blanket `into`
    // and `try_into` here, not the fields: measured with a raw request.)
    assert!(labels.iter().any(|l| l.starts_with("norm")), "{labels:?}");

    // Definition of `add` from its call goes to the `fn add` line.
    let (place, _) = s.definition(id, at, REQUEST);
    let (path, line) = place.expect("went somewhere");
    assert!(path.ends_with("src/main.rs"), "{path:?}");
    assert_eq!(line, 11, "the line of `fn add`");

    // References: the declaration and the call.
    assert!(
        s.ws.request_references(id, s.offset_of(id, "add(a"))
            .expect("doc")
    );
    s.wait("references", REQUEST, |ws| !ws.navigation_pending());
    let found = s.ws.locations().map_or(0, |l| l.locations.len());
    assert!(found >= 2, "references: {found}");
    s.ws.dismiss_locations();

    edit_then_rename(&mut s, id, "\"x\"", "1", "add(a: i32");
    let text = s.ws.document(id).expect("doc").text();
    assert!(text.contains("fn plus(a: i32"), "{text}");

    // Format: rustfmt splits the two statements on line 17.
    s.ws.set_cursor(id, 0).expect("cursor");
    assert!(s.ws.format_document(id).expect("doc"));
    s.wait("the format", REQUEST, |ws| {
        ws.document(id)
            .is_some_and(|d| !d.text().contains("; let broken"))
    });
    assert_eq!(
        s.ws.document(id).map(|d| d.position_of(0)),
        Some(Position::new(0, 0))
    );
}

#[test]
#[ignore = "real server: needs typescript-language-server; run with --run-ignored only"]
fn real_typescript_language_server_on_a_fixture() {
    if !on_path("typescript-language-server") || !on_path("tsc") {
        eprintln!("SKIPPED: typescript-language-server or tsc is not on PATH");
        return;
    }
    let Some(typescript) = typescript_package() else {
        eprintln!("SKIPPED: no TypeScript with lib/tsserver.js (set RUSTODE_TEST_TYPESCRIPT)");
        return;
    };
    let dir = ts_fixture(&typescript);
    let mut s = Session::new(&dir.0);
    let (id, running, diagnostic) = start(&mut s, &dir.0.join("src/main.ts"));
    eprintln!("MEASURED fixture: running={running:?} first error={diagnostic:?}");
    let broken =
        s.ws.markers(id)
            .into_iter()
            .find(|m| m.severity == MarkerSeverity::Error)
            .expect("an error");
    assert_eq!(s.slice(id, broken.range.start, broken.range.end), "broken");

    let at = s.offset_of(id, "add(p.x");
    let (hover, _) = s.hover(id, at + 1, REQUEST);
    assert!(
        hover.contains("function add(a: number, b: number): number"),
        "{hover}"
    );

    let after_dot = s.offset_after(id, "add(p.");
    let (labels, _) = s.complete(id, after_dot, REQUEST);
    assert!(
        labels.iter().any(|l| l == "x") && labels.iter().any(|l| l == "y"),
        "{labels:?}"
    );

    let (place, _) = s.definition(id, at, REQUEST);
    let (path, line) = place.expect("went somewhere");
    assert!(path.ends_with("src/main.ts"), "{path:?}");
    assert_eq!(line, 0, "the line of `function add`");

    s.ws.set_cursor(id, 0).expect("cursor");
    edit_then_rename(&mut s, id, "\"x\"", "1", "add(a: number");
}

#[test]
#[ignore = "real server: needs rust-analyzer and about 1.5 GB; run with --run-ignored only"]
fn real_rust_analyzer_on_the_rustode_workspace() {
    if !on_path("rust-analyzer") {
        eprintln!("SKIPPED: rust-analyzer is not on PATH");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("workspace root");
    let mut s = Session::new(&root);
    let file = root.join("crates/core/src/editor.rs");
    let started = Instant::now();
    let id = s.ws.open(&file).expect("open");
    s.wait("the server to run", START, |ws| {
        ws.language_server(id)
            .is_some_and(|st| st.state == ServerState::Running)
    });
    let running = started.elapsed();
    // Indexing is over when hover over a type of this crate answers.
    let at = s.offset_of(id, "EditError::OutOfRange");
    let mut hover = String::new();
    let limit = Duration::from_secs(600);
    let begun = Instant::now();
    while begun.elapsed() < limit {
        hover = s.hover(id, at + 1, REQUEST).0;
        if hover.contains("OutOfRange") {
            break;
        }
        assert!(available_mb() >= MIN_AVAILABLE_MB, "memory ran short");
        std::thread::sleep(Duration::from_millis(500));
        s.ws.poll_language_servers();
    }
    let indexed = started.elapsed();
    assert!(hover.contains("OutOfRange"), "{hover}");
    eprintln!("MEASURED rustode workspace: running={running:?} first answering hover={indexed:?}");
    let (place, took) = s.definition(id, at + 1, REQUEST);
    eprintln!("MEASURED rustode workspace definition={took:?} -> {place:?}");
    assert!(place.is_some());
}

fn latency(fixture: &Dir, file: &str, probe: &Probe) {
    let startups = reps(3);
    let requests = reps(15);
    let mut running = Vec::new();
    let mut first_error = Vec::new();
    let (mut hovers, mut completions, mut definitions) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..startups {
        let mut s = Session::new(&fixture.0);
        let (id, up, error) = start(&mut s, &fixture.0.join(file));
        running.push(up);
        first_error.push(error);
        // Let indexing finish before timing requests.
        let at = s.offset_of(id, probe.call);
        for _ in 0..60 {
            if s.hover(id, at + 1, REQUEST).0.contains(probe.hover) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let dot = s.offset_after(id, probe.dot);
        for _ in 0..requests {
            let (text, took) = s.hover(id, at + 1, REQUEST);
            assert!(text.contains(probe.hover), "a real hover: {text:?}");
            hovers.push(took);
            let (labels, took) = s.complete(id, dot, REQUEST);
            assert!(!labels.is_empty(), "a real completion list");
            completions.push(took);
            let (place, took) = s.definition(id, at, REQUEST);
            assert!(place.is_some(), "a real definition");
            definitions.push(took);
            s.ws.set_cursor(id, 0).expect("cursor");
        }
    }
    report("time to Running", &mut running);
    report("time to the first error diagnostic", &mut first_error);
    report("hover", &mut hovers);
    report("completion", &mut completions);
    report("definition", &mut definitions);
}

struct Probe {
    call: &'static str,
    hover: &'static str,
    dot: &'static str,
}

#[test]
#[ignore = "real server: needs rust-analyzer; run with --run-ignored only"]
fn real_latency_rust_analyzer() {
    if !on_path("rust-analyzer") {
        eprintln!("SKIPPED: rust-analyzer is not on PATH");
        return;
    }
    latency(
        &rust_fixture(),
        "src/main.rs",
        &Probe {
            call: "add(p.x",
            hover: "fn add",
            dot: "let n = p.",
        },
    );
}

#[test]
#[ignore = "real server: needs typescript-language-server; run with --run-ignored only"]
fn real_latency_typescript_language_server() {
    if !on_path("typescript-language-server") || !on_path("tsc") {
        eprintln!("SKIPPED: typescript-language-server or tsc is not on PATH");
        return;
    }
    let Some(typescript) = typescript_package() else {
        eprintln!("SKIPPED: no TypeScript with lib/tsserver.js (set RUSTODE_TEST_TYPESCRIPT)");
        return;
    };
    latency(
        &ts_fixture(&typescript),
        "src/main.ts",
        &Probe {
            call: "add(p.x",
            hover: "function add",
            dot: "add(p.",
        },
    );
}
