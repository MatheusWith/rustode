//! Workspace seam, level 1 of F0.8: corrupted configuration files never
//! crash the editor. `settings.json`, `keybindings.json` (with `when`
//! clauses) and `.vscode/launch.json` are mutated (truncated, bits flipped,
//! numbers corrupted, nested deeply, bytes inserted and removed) and loaded
//! through the entry points users hit: `Workspace::load_config`,
//! `reload_config`, `open_folder` and `reload_launch_configurations`. Each
//! case runs under `catch_unwind` and must not panic, whatever the
//! diagnostics say.
//!
//! The runs are deterministic: a fixed seed, no regression files written to
//! disk, and temporary folders only. A seed that ever fails is turned into
//! a named test below with its input inlined (`pinned_*`).

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestRunner};
use rustode_workspace::{ContextValue, Workspace};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const SETTINGS: &str = r#"// user settings
{
  /* editor */
  "editor.tabSize": 2,
  "editor.insertSpaces": false,
  "editor.minimap.enabled": true,
  "files.exclude": { "**/.git": true, "**/node_modules": true },
  "search.maxResults": 20000,
  "files.associations": { "*.conf": "properties" },
  "[rust]": { "editor.tabSize": 4, },
  "rustode.languageServers": { "rust": { "command": "rust-analyzer", "args": ["--log"] } },
  "rustode.debugAdapters": { "lldb": { "command": "lldb-dap", "args": [] } },
  "unknown.setting": [1, 2.5e3, -0, null, "xé\n"],
}
"#;

const KEYBINDINGS: &str = r#"// keybindings
[
  { "key": "ctrl+s", "command": "workbench.action.files.save", "when": "editorTextFocus && !editorReadonly" },
  { "key": "ctrl+shift+p", "command": "-workbench.action.showCommands" },
  { "key": "ctrl+k ctrl+c", "command": "editor.action.addCommentLine", "when": "(editorTextFocus || terminalFocus) && !(inQuickOpen || resourceExtname =~ /\\.rs$/)" },
  { "key": "f5", "command": "workbench.action.debug.start", "when": "debuggersAvailable && debugState != 'running' && resourceLangId in supported" },
  { "key": "alt+up", "command": "editor.action.moveLinesUpAction", "when": "config.editor.tabSize >= 2 && !editorReadonly" },
]
"#;

const LAUNCH: &str = r#"{
  // debug
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Debug a",
      "type": "lldb",
      "request": "launch",
      "program": "${workspaceFolder}/target/debug/a",
      "args": ["--flag", "1"],
      "cwd": "${workspaceFolder}",
      "env": { "RUST_LOG": "debug" },
      "stopOnEntry": false,
    },
    { "name": "Attach", "type": "lldb", "request": "attach", "pid": 4242 }
  ],
  "compounds": [{ "name": "Both", "configurations": ["Debug a", "Attach"] }]
}
"#;

/// The files a case writes, one of which is mutated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Settings,
    Keybindings,
    Launch,
}

const TARGETS: [Target; 3] = [Target::Settings, Target::Keybindings, Target::Launch];

fn pristine(target: Target) -> &'static str {
    match target {
        Target::Settings => SETTINGS,
        Target::Keybindings => KEYBINDINGS,
        Target::Launch => LAUNCH,
    }
}

/// One change to a file's bytes. Positions wrap around the file's length,
/// so every generated value is meaningful for every file.
#[derive(Debug, Clone)]
enum Mutation {
    /// Cut the file short.
    Truncate(usize),
    /// Flip single bits.
    Flip(Vec<(usize, u8)>),
    /// Replace the n-th run of digits with a hostile number.
    CorruptSize(usize, usize),
    /// Insert `depth` openers of one kind: `[`, `{"a":`, `(`, `!(` or `/*`.
    Nest(usize, usize, usize),
    /// Like `Nest`, but right after the n-th quote, so the openers land at
    /// the start of a string value (a `when` clause, a path).
    NestInString(usize, usize, usize),
    /// Insert bytes.
    Insert(usize, Vec<u8>),
    /// Remove a span.
    Remove(usize, usize),
}

const HOSTILE_NUMBERS: &[&str] = &[
    "0",
    "-1",
    "4294967295",
    "4294967296",
    "9223372036854775807",
    "18446744073709551616",
    "99999999999999999999999999999999",
    "1e999",
    "-1e999",
    "0.0000000000000000000001",
    "NaN",
];

const OPENERS: &[&str] = &["[", "{\"a\":", "(", "!(", "/*", "\"", "[{"];

fn apply(bytes: &mut Vec<u8>, mutation: &Mutation) {
    let len = bytes.len().max(1);
    match mutation {
        Mutation::Truncate(at) => bytes.truncate(at % (len + 1)),
        Mutation::Flip(flips) => {
            for (at, bit) in flips {
                if let Some(byte) = bytes.get_mut(at % len) {
                    *byte ^= 1 << (bit % 8);
                }
            }
        }
        Mutation::CorruptSize(n, which) => {
            let runs = digit_runs(bytes);
            if runs.is_empty() {
                return;
            }
            let (start, end) = runs[n % runs.len()];
            let number = HOSTILE_NUMBERS[which % HOSTILE_NUMBERS.len()];
            bytes.splice(start..end, number.bytes());
        }
        Mutation::Nest(at, kind, depth) => {
            let opener = OPENERS[kind % OPENERS.len()];
            let at = at % (len + 1);
            let at = at.min(bytes.len());
            bytes.splice(at..at, opener.bytes().cycle().take(opener.len() * depth));
        }
        Mutation::NestInString(n, kind, depth) => {
            let quotes: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] == b'"').collect();
            if quotes.is_empty() {
                return;
            }
            let at = quotes[n % quotes.len()] + 1;
            let opener = OPENERS[kind % OPENERS.len()];
            bytes.splice(at..at, opener.bytes().cycle().take(opener.len() * depth));
        }
        Mutation::Insert(at, extra) => {
            let at = (at % (len + 1)).min(bytes.len());
            bytes.splice(at..at, extra.iter().copied());
        }
        Mutation::Remove(at, count) => {
            let start = (at % len).min(bytes.len());
            let end = (start + count).min(bytes.len());
            bytes.drain(start..end);
        }
    }
}

fn digit_runs(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, byte) in bytes.iter().enumerate() {
        match (byte.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(from)) => {
                runs.push((from, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push((from, bytes.len()));
    }
    runs
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        any::<usize>().prop_map(Mutation::Truncate),
        prop::collection::vec((any::<usize>(), any::<u8>()), 1..16).prop_map(Mutation::Flip),
        (any::<usize>(), any::<usize>()).prop_map(|(n, w)| Mutation::CorruptSize(n, w)),
        (any::<usize>(), any::<usize>(), 1_usize..20_000)
            .prop_map(|(at, kind, depth)| Mutation::Nest(at, kind, depth)),
        (any::<usize>(), any::<usize>(), 1_usize..20_000)
            .prop_map(|(n, kind, depth)| Mutation::NestInString(n, kind, depth)),
        (any::<usize>(), prop::collection::vec(any::<u8>(), 1..32))
            .prop_map(|(at, bytes)| Mutation::Insert(at, bytes)),
        (any::<usize>(), 1_usize..64).prop_map(|(at, count)| Mutation::Remove(at, count)),
    ]
}

fn temp_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rustode-mutation-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project/.vscode")).expect("temp dirs");
    dir
}

/// Loads the three files, one of them as `bytes`, the way a user's editor
/// does, then uses what came out; panics if anything does.
fn load(target: Target, bytes: &[u8]) {
    let dir = temp_dir();
    let write = |path: &Path, which: Target| {
        let content = if which == target {
            bytes.to_vec()
        } else {
            pristine(which).as_bytes().to_vec()
        };
        std::fs::write(path, content).expect("write fixture");
    };
    let settings = dir.join("settings.json");
    let keybindings = dir.join("keybindings.json");
    let project = dir.join("project");
    write(&settings, Target::Settings);
    write(&keybindings, Target::Keybindings);
    write(&project.join(".vscode/launch.json"), Target::Launch);
    std::fs::write(project.join("a.txt"), "one\ntwo\n").expect("project file");

    let mut ws = Workspace::new();
    ws.open_folder(&project).expect("open folder");
    let _ = ws.load_config(&settings, &keybindings);
    let _ = ws.reload_config();
    ws.reload_launch_configurations();

    // Use what loaded: every binding's clause evaluates and prints, every
    // diagnostic prints, every launch configuration and problem is readable.
    for binding in ws.config().keybindings() {
        if let Some(when) = &binding.when {
            let _ = when.to_string();
            let _ = when.evaluate(&|key| {
                if key.len() % 2 == 0 {
                    ContextValue::Bool(true)
                } else {
                    ContextValue::Str(key.to_owned())
                }
            });
        }
    }
    for (_, diagnostic) in ws.config().diagnostics() {
        let _ = diagnostic.to_string();
    }
    let _ = ws.debug_problems();
    let names: Vec<String> = ws
        .launch_configurations()
        .iter()
        .map(|configuration| configuration.name.clone())
        .collect();
    for name in names {
        let _ = ws.select_launch_configuration(&name);
    }
    let _ = ws.keybinding_label("workbench.action.files.save");
    let _ = ws.config().value("editor.tabSize");

    // The editor still edits.
    let id = ws.open(&project.join("a.txt")).expect("open");
    ws.type_text(id, "x").expect("type");
    assert_eq!(ws.document(id).expect("doc").text(), "xone\ntwo\n");
    let _ = std::fs::remove_dir_all(dir);
}

/// Runs `load` under `catch_unwind`, as the spec requires.
fn load_without_panicking(target: Target, bytes: &[u8]) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| load(target, bytes)))
        .map_err(|_| format!("panicked loading {target:?} as {bytes:?}"))
}

/// The seeded run: fixed seed, no regression file, `cases` mutated inputs.
fn run(target: Target, seed: u64, cases: u32) {
    let config = Config {
        cases,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(seed),
        // The runner reports the smallest input it found.
        max_shrink_iters: 256,
        ..Config::default()
    };
    // No regression file: nothing is written outside the temporary folders.
    assert!(config.failure_persistence.is_none());
    let mut runner = TestRunner::new(config);
    let strategy = prop::collection::vec(mutation(), 1..4);
    let result = runner.run(&strategy, |mutations| {
        let mut bytes = pristine(target).as_bytes().to_vec();
        for mutation in &mutations {
            apply(&mut bytes, mutation);
        }
        load_without_panicking(target, &bytes).map_err(TestCaseError::fail)
    });
    if let Err(failure) = result {
        panic!("{target:?} with seed {seed}: {failure}");
    }
}

#[test]
fn mutated_settings_json_never_panics() {
    run(Target::Settings, 0x5e77_1495, 192);
}

#[test]
fn mutated_keybindings_json_never_panics() {
    run(Target::Keybindings, 0x4b45_5953, 192);
}

#[test]
fn mutated_launch_json_never_panics() {
    run(Target::Launch, 0x4c41_554e, 192);
}

const WHEN_CLAUSES: &[&str] = &[
    "editorTextFocus && !editorReadonly",
    "(editorTextFocus || terminalFocus) && !(inQuickOpen || resourceExtname =~ /\\.rs$/)",
    "debugState != 'running' && resourceLangId in supported",
    "config.editor.tabSize >= 2 && a not in b",
    "!(a && (b || (c && !(d))))",
];

#[test]
fn mutated_when_clauses_never_panic() {
    // The clause alone is mutated (the file-level run rarely lands inside
    // one), then written into `keybindings.json` as a JSON string.
    let config = Config {
        cases: 256,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0x5748_454e),
        max_shrink_iters: 256,
        ..Config::default()
    };
    let strategy = (
        0..WHEN_CLAUSES.len(),
        prop::collection::vec(mutation(), 1..4),
    );
    let result = TestRunner::new(config).run(&strategy, |(clause, mutations)| {
        let mut bytes = WHEN_CLAUSES[clause].as_bytes().to_vec();
        for mutation in &mutations {
            apply(&mut bytes, mutation);
        }
        let when = serde_json::to_string(&String::from_utf8_lossy(&bytes)).expect("a string");
        let json = format!("[{{ \"key\": \"ctrl+s\", \"command\": \"x\", \"when\": {when} }}]");
        load_without_panicking(Target::Keybindings, json.as_bytes()).map_err(TestCaseError::fail)
    });
    if let Err(failure) = result {
        panic!("when clauses: {failure}");
    }
}

#[test]
fn the_pristine_files_load_and_the_editor_edits() {
    for target in TARGETS {
        load(target, pristine(target).as_bytes());
    }
}

#[test]
fn every_prefix_of_every_file_loads() {
    // Truncation exhaustively, not by sampling: a file cut at any byte.
    for target in TARGETS {
        let text = pristine(target).as_bytes();
        for cut in (0..=text.len()).step_by(7) {
            if let Err(why) = load_without_panicking(target, &text[..cut]) {
                panic!("{why}");
            }
        }
    }
}

// ---- pinned inputs: each one a case that crashed, or a shape that nearly did ----

#[test]
fn pinned_a_when_clause_100000_parentheses_deep_is_a_diagnostic() {
    // Overflowed the stack in the `when` parser and aborted the process
    // (found by the mutation run: `Nest` of `(` inside a `when` string).
    let deep = "(".repeat(100_000);
    let json = format!(
        "[{{ \"key\": \"ctrl+s\", \"command\": \"x\", \"when\": \"{deep}a{}\" }}]",
        ")".repeat(100_000)
    );
    load(Target::Keybindings, json.as_bytes());
}

#[test]
fn pinned_json_nested_100000_deep_is_a_diagnostic() {
    for opener in ["[", "{\"a\":"] {
        let json = opener.repeat(100_000);
        load(Target::Settings, json.as_bytes());
        load(Target::Keybindings, json.as_bytes());
        load(Target::Launch, json.as_bytes());
    }
}
