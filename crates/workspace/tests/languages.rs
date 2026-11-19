//! Workspace seam: a document's language is detected from its file name,
//! name pattern, extension and first line, follows `files.associations`,
//! can be picked from the language list, and every language with a grammar
//! is highlighted.

use rustode_workspace::{DocumentId, Event, TokenKind, Workspace};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-lang-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open_in(ws: &mut Workspace, dir: &std::path::Path, file: &str, text: &str) -> DocumentId {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("subdir");
    std::fs::write(&path, text).expect("fixture");
    ws.open(&path).expect("open")
}

/// The language id of a file with this name and text, with no settings.
fn detected(name: &str, file: &str, text: &str) -> String {
    let mut ws = Workspace::new();
    let id = open_in(&mut ws, &dir(name), file, text);
    ws.document(id).expect("doc").language_id().to_owned()
}

/// A workspace whose settings.json has these `files.associations`.
fn with_associations(name: &str, json: &str) -> (Workspace, PathBuf) {
    let dir = dir(name);
    let settings = dir.join("settings.json");
    std::fs::write(&settings, format!("{{ \"files.associations\": {json} }}")).expect("settings");
    let mut ws = Workspace::new();
    ws.load_config(&settings, &dir.join("keybindings.json"));
    (ws, dir)
}

#[test]
fn an_exact_file_name_beats_the_extension() {
    // `.babelrc.json` is a file name of JSON with Comments; its extension
    // `.json` says JSON.
    assert_eq!(detected("name", ".babelrc.json", "{}"), "jsonc");
    assert_eq!(detected("ext", "x.json", "{}"), "json");
    assert_eq!(detected("case", "MAKEFILE", ""), "makefile");
}

#[test]
fn a_name_pattern_beats_the_extension() {
    assert_eq!(detected("pat1", "tsconfig.base.json", "{}"), "jsonc");
    assert_eq!(detected("pat2", "Dockerfile.dev", "FROM x\n"), "dockerfile");
    assert_eq!(detected("pat3", "containerfile.ci", ""), "dockerfile");
    assert_eq!(detected("pat4", "other.base.json", "{}"), "json");
}

#[test]
fn the_longest_extension_wins() {
    assert_eq!(detected("long1", "app.js.map", "{}"), "json");
    assert_eq!(detected("long2", "app.tsx", ""), "typescriptreact");
    assert_eq!(detected("long3", "Cargo.toml", ""), "toml");
}

#[test]
fn the_first_line_decides_when_the_name_says_nothing() {
    for (text, language) in [
        ("#!/usr/bin/env python3\nprint(1)\n", "python"),
        ("#!/bin/bash\necho hi\n", "shellscript"),
        ("#!/usr/bin/env node\nconsole.log(1)\n", "javascript"),
        ("#!/usr/bin/env -S deno run\n", "typescript"),
        ("<?xml version=\"1.0\"?>\n<a/>\n", "xml"),
        ("#cloud-config\nusers: []\n", "yaml"),
        ("\u{feff}#!/bin/sh\n", "shellscript"),
        ("just text\n", "plaintext"),
        ("", "plaintext"),
    ] {
        assert_eq!(detected("first", "script", text), language, "{text:?}");
    }
}

#[test]
fn the_name_beats_the_first_line() {
    assert_eq!(detected("nb1", "notes.txt", "#!/bin/bash\n"), "plaintext");
    assert_eq!(detected("nb2", "x.rs", "#!/bin/bash\n"), "rust");
}

#[test]
fn files_associations_win_over_the_languages_own() {
    let (mut ws, dir) = with_associations(
        "assoc",
        r#"{ "*.conf": "ini", "*.rs": "plaintext", "**/docs/*.txt": "markdown", "Jenkinsfile": "python" }"#,
    );
    for (file, language) in [
        ("a.conf", "ini"),
        ("A.CONF", "ini"),
        ("main.rs", "plaintext"),
        ("docs/readme.txt", "markdown"),
        ("other/readme.txt", "plaintext"),
        ("Jenkinsfile", "python"),
        ("x.py", "python"),
    ] {
        let id = open_in(&mut ws, &dir, file, "x = 1\n");
        assert_eq!(
            ws.document(id).expect("doc").language_id(),
            language,
            "{file}"
        );
    }
}

#[test]
fn the_longest_association_pattern_wins() {
    let (mut ws, dir) = with_associations(
        "longest",
        r#"{ "*.log": "plaintext", "build-*.log": "ini", "*": "markdown" }"#,
    );
    let id = open_in(&mut ws, &dir, "build-1.log", "");
    assert_eq!(ws.document(id).expect("doc").language_id(), "ini");
    let id = open_in(&mut ws, &dir, "run.log", "");
    assert_eq!(ws.document(id).expect("doc").language_id(), "plaintext");
}

#[test]
fn associations_that_name_no_language_or_no_string_are_ignored() {
    let (mut ws, dir) = with_associations(
        "ignored",
        r#"{ "*.rs": "klingon", "*.py": 3, "*.go": null, "[": "ini" }"#,
    );
    let id = open_in(&mut ws, &dir, "a.rs", "fn main() {}\n");
    assert_eq!(ws.document(id).expect("doc").language_id(), "rust");
    let id = open_in(&mut ws, &dir, "a.py", "");
    assert_eq!(ws.document(id).expect("doc").language_id(), "python");
}

#[test]
fn changing_the_associations_re_detects_documents_not_picked() {
    let (mut ws, dir) = with_associations("reload", "{}");
    let follows = open_in(&mut ws, &dir, "a.conf", "key = 1\n");
    let picked = open_in(&mut ws, &dir, "b.conf", "key = 1\n");
    ws.set_language(picked, Some("yaml")).expect("pick");
    assert_eq!(
        ws.document(follows).expect("doc").language_id(),
        "plaintext"
    );
    let _ = ws.take_events();

    std::fs::write(
        dir.join("settings.json"),
        r#"{ "files.associations": { "*.conf": "toml" } }"#,
    )
    .expect("settings");
    ws.reload_config();
    assert_eq!(ws.document(follows).expect("doc").language_id(), "toml");
    assert!(
        !ws.document(follows)
            .expect("doc")
            .highlights(0..1)
            .is_empty(),
        "highlighted with the new grammar"
    );
    assert_eq!(
        ws.document(picked).expect("doc").language_id(),
        "yaml",
        "a picked language stays"
    );
    assert_eq!(ws.take_events(), [Event::LanguageChanged { id: follows }]);
}

#[test]
fn the_picker_lists_auto_detect_then_the_languages_by_name() {
    let mut ws = Workspace::new();
    let id = open_in(&mut ws, &dir("picks"), "a.py", "x = 1\n");
    let picks = ws.language_picks(id, "").expect("picks");
    assert_eq!(picks[0].label, "Auto Detect");
    assert_eq!(picks[0].id, None);
    let names: Vec<&str> = picks[1..].iter().map(|p| p.label).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(names, sorted);
    for name in [
        "Plain Text",
        "Rust",
        "Python",
        "TOML",
        "Markdown",
        "YAML",
        "HTML",
        "CSS",
        "Go",
        "C++",
        "JSON with Comments",
        "TypeScript JSX",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    let python = picks
        .iter()
        .find(|p| p.id == Some("python"))
        .expect("python");
    assert!(python.current);
    assert_eq!(python.description, "(python) - Configured Language");
    assert!(python.extensions.contains(".py"));
    let rust = picks.iter().find(|p| p.id == Some("rust")).expect("rust");
    assert!(!rust.current);
    assert_eq!(rust.description, "(rust)");
}

#[test]
fn the_picker_filters_on_the_label_and_the_id_and_hides_auto_detect_when_empty() {
    let mut ws = Workspace::new();
    let empty = open_in(&mut ws, &dir("filter"), "a.py", "");
    let picks = ws.language_picks(empty, "").expect("picks");
    assert_ne!(
        picks[0].label, "Auto Detect",
        "nothing to detect in an empty file"
    );
    let found = ws.language_picks(empty, "script").expect("picks");
    let names: Vec<&str> = found.iter().map(|p| p.label).collect();
    assert!(
        names.contains(&"JavaScript") && names.contains(&"Shell Script"),
        "{names:?}"
    );
    let by_id = ws.language_picks(empty, "(shellscript)").expect("picks");
    assert_eq!(by_id.len(), 1);
    let rust = ws.language_picks(empty, "rus").expect("picks");
    assert_eq!(rust[0].label, "Rust");
    assert_eq!(rust[0].positions, [0, 1, 2]);
    assert_eq!(ws.language_picks(empty, "zzz").expect("picks").len(), 0);
    let mut other = Workspace::new();
    let d = dir("filter-other");
    open_in(&mut other, &d, "a", "");
    let unknown = open_in(&mut other, &d, "b", "");
    assert!(ws.language_picks(unknown, "").is_err());
}

#[test]
fn picking_a_language_highlights_the_document_again() {
    let mut ws = Workspace::new();
    let id = open_in(
        &mut ws,
        &dir("pick"),
        "notes.txt",
        "def f(a):\n    return None\n",
    );
    assert_eq!(ws.document(id).expect("doc").highlights(0..2).len(), 0);
    let _ = ws.take_events();

    ws.set_language(id, Some("python")).expect("pick");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.language_id(), "python");
    assert_eq!(doc.language_name(), "Python");
    assert!(doc.language_picked());
    let kinds: Vec<TokenKind> = doc.highlights(0..2).iter().map(|h| h.kind).collect();
    assert!(kinds.contains(&TokenKind::Keyword), "{kinds:?}");
    assert!(kinds.contains(&TokenKind::ControlKeyword), "{kinds:?}");
    assert_eq!(ws.take_events(), [Event::LanguageChanged { id }]);

    // The tree follows the text after the switch.
    ws.set_cursor(id, 0).expect("cursor");
    ws.type_text(id, "# c\n").expect("type");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.highlights(0..1)[0].kind, TokenKind::Comment);

    // To a language with no grammar: plain again, and typing still works.
    ws.set_language(id, Some("log")).expect("pick");
    assert_eq!(ws.document(id).expect("doc").highlights(0..3).len(), 0);
    ws.type_text(id, "x").expect("type");
    assert_eq!(
        ws.take_events(),
        [
            Event::DocumentChanged { id },
            Event::LanguageChanged { id },
            Event::DocumentChanged { id },
        ]
    );
}

#[test]
fn auto_detect_goes_back_to_the_detected_language() {
    let mut ws = Workspace::new();
    let id = open_in(&mut ws, &dir("auto"), "main.rs", "fn main() {}\n");
    ws.set_language(id, Some("markdown")).expect("pick");
    assert_eq!(ws.document(id).expect("doc").language_id(), "markdown");
    ws.set_language(id, None).expect("auto");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.language_id(), "rust");
    assert!(!doc.language_picked());
    assert_ne!(doc.highlights(0..1).len(), 0);
    assert!(ws.set_language(id, Some("klingon")).is_err());
    assert_eq!(ws.document(id).expect("doc").language_id(), "rust");
}

#[test]
fn the_picker_is_a_palette_command() {
    let ws = Workspace::new();
    let found = ws.commands("language mode");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].command.id,
        "workbench.action.editor.changeLanguageMode"
    );
    assert_eq!(found[0].keybinding.as_deref(), Some("Ctrl+K M"));
}

/// One sample per language with a grammar: a file name, its text, and a
/// token the highlights must contain on its first lines.
const SAMPLES: &[(&str, &str, &str, TokenKind)] = &[
    ("a.rs", "fn main() {}\n", "fn", TokenKind::Keyword),
    ("a.json", "{\"a\": 1}\n", "1", TokenKind::Number),
    ("a.jsonc", "// c\n{}\n", "// c", TokenKind::Comment),
    ("a.toml", "[a]\nb = true\n", "true", TokenKind::Boolean),
    ("a.md", "# Title\n", "Title", TokenKind::Keyword),
    ("a.js", "const a = 1;\n", "const", TokenKind::Keyword),
    ("a.jsx", "let a = <b/>;\n", "let", TokenKind::Keyword),
    ("a.ts", "interface I {}\n", "interface", TokenKind::Keyword),
    ("a.tsx", "type T = number;\n", "number", TokenKind::Type),
    ("a.py", "def f(): pass\n", "def", TokenKind::Keyword),
    (
        "a.c",
        "int main() { return 0; }\n",
        "return",
        TokenKind::ControlKeyword,
    ),
    ("a.cpp", "class A {};\n", "class", TokenKind::Keyword),
    ("a.go", "package main\n", "package", TokenKind::Keyword),
    (
        "a.sh",
        "if true; then echo; fi\n",
        "then",
        TokenKind::ControlKeyword,
    ),
    ("a.yaml", "a: 1\n", "1", TokenKind::Number),
    (
        "a.html",
        "<a href=\"x\">t</a>\n",
        "href",
        TokenKind::Variable,
    ),
    ("a.css", "a { color: red; }\n", "color", TokenKind::Variable),
];

#[test]
fn every_language_with_a_grammar_is_highlighted_and_has_a_tree() {
    let mut ws = Workspace::new();
    let dir = dir("samples");
    for (file, text, token, kind) in SAMPLES {
        let id = open_in(&mut ws, &dir, file, text);
        let doc = ws.document(id).expect("doc");
        assert!(doc.syntax_tree().is_some(), "{file}: a tree");
        let chars: Vec<char> = doc.text().chars().collect();
        let found: Vec<(String, TokenKind)> = doc
            .highlights(0..doc.text().lines().count())
            .iter()
            .map(|h| (chars[h.range.start..h.range.end].iter().collect(), h.kind))
            .collect();
        assert!(
            found.contains(&((*token).to_owned(), *kind)),
            "{file}: {token:?} {kind:?} in {found:?}"
        );
    }
}
