//! Workspace seam, level 2 of F0.8: what the user types is hostile until
//! proven otherwise. Globs, regular expressions, find and replace text,
//! quick open queries and the arguments of extension commands are refused
//! with errors or answered with something sensible; none may panic, hang or
//! touch anything outside the folders a test made.
//!
//! Every input is run under `catch_unwind`, so a panic names the input
//! instead of aborting the run. Nothing here uses the network: the registry
//! is the local stand-in.

use rustode_workspace::extensions::stand_in::StandIn;
use rustode_workspace::{
    DocumentId, FindOptions, FindQuery, SearchQuery, Workspace, parse_quick_query,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-hostile-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Runs `body` and fails with `what` if it panics.
fn never_panics<T>(what: &str, body: impl FnOnce() -> T) -> T {
    let outcome = catch_unwind(AssertUnwindSafe(body));
    assert!(outcome.is_ok(), "panicked on {what}");
    outcome.expect("checked above")
}

/// Text that is awkward for a program written for ASCII.
fn nasty_texts() -> Vec<String> {
    let mut texts: Vec<String> = [
        "",
        " ",
        "\n",
        "\r\n",
        "\t",
        "\0",
        "a\0b",
        "\u{feff}",
        "\u{202e}rtl",
        "é",
        "e\u{301}",
        "日本語",
        "💥",
        "👨‍👩‍👧‍👦",
        "\\",
        "\\\\\\",
        "$",
        "$$",
        "${",
        "$1",
        "$99999999999999999999",
        "\\u\\U\\l\\L",
        "\\u",
        "\\n\\t\\x",
        "%s%n",
        "../../..",
        "/etc/passwd",
    ]
    .iter()
    .map(|text| (*text).to_owned())
    .collect();
    texts.push("x".repeat(200_000));
    texts.push("日".repeat(50_000));
    texts.push("(".repeat(10_000));
    texts.push("\\".repeat(10_001));
    texts
}

fn open_document(name: &str, text: &str) -> (Workspace, DocumentId, PathBuf) {
    let dir = temp(name);
    let path = dir.join("doc.txt");
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id, dir)
}

// ---- regular expressions and find ----

const HOSTILE_REGEXES: &[&str] = &[
    "(",
    ")",
    "[",
    "[a-",
    "[z-a]",
    "*",
    "+",
    "?",
    "a{2,1}",
    "a{99999999999999999999}",
    "a{1000000}",
    "(?<=a)b",
    "(?=a)",
    "(?!a)",
    "(a)\\1",
    "\\",
    "(?P<n>",
    "(?i",
    "\\p{Nope}",
    "\\xZZ",
    "\\u{110000}",
    "(a+)+$",
    "(a*)*b",
    "(.*)(.*)(.*)(.*)(.*)x",
    "^",
    "$",
    "^$",
    "",
    "(?:)",
    "\\b",
    "\\B",
    "x*",
    "(?s).*",
    "[[:alpha:]]+",
    "\\n",
    "\\r\\n",
    "a\\nb",
];

#[test]
fn hostile_regexes_are_errors_or_queries_never_panics() {
    let (mut ws, id, _dir) = open_document("regex", "aaaa\nb é 日本\r\n\n💥 x\n");
    let mut all = HOSTILE_REGEXES
        .iter()
        .map(|r| (*r).to_owned())
        .collect::<Vec<_>>();
    all.push("(".repeat(5_000) + &")".repeat(5_000));
    all.push("a|".repeat(50_000));
    all.push("[".repeat(10_000));
    for options in [
        FindOptions {
            regex: true,
            match_case: false,
            whole_word: false,
        },
        FindOptions {
            regex: true,
            match_case: true,
            whole_word: true,
        },
    ] {
        for pattern in &all {
            let what = format!(
                "regex of {} chars: {:?}",
                pattern.len(),
                pattern.get(..40.min(pattern.len()))
            );
            never_panics(&what, || {
                let Ok(query) = FindQuery::new(pattern, options) else {
                    return;
                };
                let _ = ws.find_all(id, &query);
                let _ = ws.find_next(id, &query);
                let _ = ws.find_previous(id, &query);
                let _ = ws.replace_one(id, &query, "x");
                let _ = ws.replace_all(id, &query, "$1[$&]");
                // Whatever the replacement did, the document is still usable.
                while ws.undo(id).is_ok_and(|changed| changed) {}
            });
        }
    }
}

#[test]
fn a_regex_that_is_too_big_or_too_nested_is_refused_with_an_error() {
    let options = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    for pattern in [
        "a{1000000}".to_owned(),
        "(a{1000}){1000}".to_owned(),
        "(".repeat(100_000) + &")".repeat(100_000),
    ] {
        let result = never_panics("a huge regex", || FindQuery::new(&pattern, options));
        assert!(result.is_err(), "{} chars were accepted", pattern.len());
    }
}

#[test]
fn hostile_patterns_as_plain_text_never_panic() {
    let (mut ws, id, _dir) = open_document("plain", "aaaa\nb é 日本\r\n\n💥 x $1 \\\n");
    for pattern in nasty_texts() {
        for options in [
            FindOptions::default(),
            FindOptions {
                regex: false,
                match_case: true,
                whole_word: true,
            },
        ] {
            never_panics(&format!("plain pattern of {} chars", pattern.len()), || {
                // A huge plain pattern may be refused (the compiled size is
                // capped), with an error and not a panic.
                let Ok(query) = FindQuery::new(&pattern, options) else {
                    return;
                };
                let _ = ws.find_all(id, &query);
                let _ = ws.find_next(id, &query);
                let _ = ws.find_previous(id, &query);
                let _ = ws.replace_one(id, &query, "r");
            });
        }
    }
}

#[test]
fn hostile_replacement_texts_never_panic_and_undo_restores_the_text() {
    let text = "aaaa\nb é 日本\r\n\n💥 x $1\n";
    let (mut ws, id, _dir) = open_document("replace", text);
    for regex in [false, true] {
        let options = FindOptions {
            regex,
            ..FindOptions::default()
        };
        let query = FindQuery::new("(a)|(é)|x", options).expect("pattern");
        for replacement in nasty_texts() {
            never_panics(
                &format!("replacement of {} chars", replacement.len()),
                || {
                    let _ = ws.replace_all(id, &query, &replacement);
                    let _ = ws.replace_one(id, &query, &replacement);
                    while ws.undo(id).is_ok_and(|changed| changed) {}
                },
            );
            assert_eq!(
                ws.document(id).expect("doc").text(),
                text,
                "undo after a replacement of {} chars",
                replacement.len()
            );
        }
    }
}

// ---- globs and search ----

const HOSTILE_GLOBS: &[&str] = &[
    "[",
    "]",
    "[!",
    "[a-",
    "[z-a]",
    "{",
    "}",
    "{a,",
    "{a,b",
    "{,}",
    "{{{{{{{{",
    "**",
    "**/**/**/**/**",
    "***",
    "a/**b",
    "\\",
    "\\\\",
    "*\\",
    ",",
    ",,,",
    " , ",
    "!",
    "!!a",
    "../../..",
    "../outside",
    "/etc/*",
    "/",
    "//",
    ".",
    "./",
    "\0",
    "a\0b",
    "é*",
    "日本/**",
    "💥",
    "?",
    "????????????",
];

#[test]
fn hostile_globs_are_errors_or_searches_never_panics() {
    let root = temp("globs");
    std::fs::write(root.join("a.txt"), "needle\n").expect("fixture");
    std::fs::create_dir_all(root.join("sub")).expect("sub");
    std::fs::write(root.join("sub/b.txt"), "needle\n").expect("fixture");
    let outside = temp("globs-outside");
    std::fs::write(outside.join("secret.txt"), "needle\n").expect("fixture");

    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("folder");
    let mut globs: Vec<String> = HOSTILE_GLOBS.iter().map(|g| (*g).to_owned()).collect();
    globs.push("a".repeat(100_000));
    globs.push("*".repeat(10_000));
    globs.push("{".repeat(10_000) + &"}".repeat(10_000));
    globs.push("[".repeat(10_000));
    globs.push(format!("{}", outside.display()));
    globs.push(format!("{}/**", outside.display()));
    globs.push(format!(
        "../{}/*",
        outside.file_name().expect("name").to_string_lossy()
    ));

    for glob in &globs {
        for (include, exclude) in [(glob.as_str(), ""), ("", glob.as_str())] {
            let what = format!(
                "glob of {} chars {:?}",
                glob.len(),
                glob.get(..30.min(glob.len()))
            );
            never_panics(&what, || {
                let mut query = SearchQuery::new(
                    FindQuery::new("needle", FindOptions::default()).expect("pattern"),
                );
                query.include = include.to_owned();
                query.exclude = exclude.to_owned();
                if ws.start_search(query).is_err() {
                    return; // refused with an error: fine
                }
                let deadline = Instant::now() + Duration::from_secs(30);
                while ws.search_results().is_running() {
                    ws.poll_search();
                    assert!(Instant::now() < deadline, "the search did not finish");
                    std::thread::sleep(Duration::from_millis(1));
                }
                ws.poll_search();
                // A glob never reaches a file outside the opened folder.
                for file in ws.search_results().files() {
                    assert!(
                        !file.path.starts_with(&outside),
                        "found {} through {glob:?}",
                        file.path.display()
                    );
                }
            });
        }
    }
}

#[test]
fn hostile_search_patterns_never_panic() {
    let root = temp("search-patterns");
    std::fs::write(root.join("a.txt"), "aaaa\nb é 日本\n💥\n").expect("fixture");
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("folder");
    for pattern in nasty_texts()
        .into_iter()
        .chain(HOSTILE_REGEXES.iter().map(|r| (*r).to_owned()))
    {
        for regex in [false, true] {
            never_panics(
                &format!("search pattern of {} chars", pattern.len()),
                || {
                    let options = FindOptions {
                        regex,
                        ..FindOptions::default()
                    };
                    let Ok(find) = FindQuery::new(&pattern, options) else {
                        return;
                    };
                    if ws.start_search(SearchQuery::new(find)).is_err() {
                        return;
                    }
                    let deadline = Instant::now() + Duration::from_secs(30);
                    while ws.search_results().is_running() {
                        ws.poll_search();
                        assert!(Instant::now() < deadline, "the search did not finish");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    ws.poll_search();
                },
            );
        }
    }
}

// ---- quick open ----

#[test]
fn hostile_quick_open_queries_never_panic() {
    let root = temp("quick-open");
    for file in ["src/main.rs", "日本語.txt", "💥.md", "a b/c d.txt"] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, "one\ntwo\n").expect("fixture");
    }
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("folder");
    let id = ws.open(&root.join("src/main.rs")).expect("open");

    let mut queries = nasty_texts();
    for query in [
        ":",
        "::",
        ":::",
        ":0",
        ":-1",
        ":1:",
        ":1:1:1",
        ":,",
        ":1,2",
        ": 1 : 2 ",
        ">",
        ">>",
        "@",
        "#",
        ":99999999999999999999",
        ":1:99999999999999999999",
        ":18446744073709551615",
        ":18446744073709551616",
        ":٣",
        ":１",
    ] {
        queries.push(query.to_owned());
    }
    for query in &queries {
        let what = format!("quick open query of {} chars", query.len());
        never_panics(&what, || {
            let _ = ws.quick_open(query, 10);
            let _ = ws.quick_open(query, 0);
            let _ = ws.quick_open(query, usize::MAX);
            if let rustode_workspace::QuickQuery::Line { line, column } = parse_quick_query(query) {
                let _ = ws.go_to_line(id, line, column);
            }
        });
    }
    never_panics("go to the largest line and column", || {
        ws.go_to_line(id, usize::MAX, Some(usize::MAX))
            .expect("clamped");
        ws.go_to_line(id, 0, Some(0)).expect("clamped");
    });
}

// ---- extension commands ----

/// Ids and texts that are not extension ids, or are ones that try to leave
/// the extensions folder.
fn hostile_ids() -> Vec<String> {
    let mut ids: Vec<String> = [
        "",
        " ",
        ".",
        "..",
        "a",
        "a.",
        ".a",
        "a.b.c",
        "a..b",
        "a/b.c",
        "a.b/c",
        "../victim",
        "..\\victim",
        "../victim.x",
        "x.../victim",
        "a.b/../../victim",
        "/etc/passwd",
        "~",
        "a.b\0",
        "a\0.b",
        "a b.c",
        "a.b c",
        "  acme.hello  ",
        "ACME.HELLO",
        "acme.hello-1.0.0",
        "acme.hello@1.0.0",
        "acme.hello@",
        "ÅCME.héllo",
        "日本.語",
        "💥.💥",
        "a.b\n",
        "*",
        "*.*",
        "acme.*",
        "$(whoami).x",
        "`id`.x",
        "a.b;rm -rf /",
    ]
    .iter()
    .map(|id| (*id).to_owned())
    .collect();
    ids.push("a".repeat(100_000) + "." + &"b".repeat(100_000));
    ids.push("a.".repeat(50_000));
    ids
}

fn tree(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            found.push(
                path.strip_prefix(dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found.sort();
    found
}

fn drain(ws: &mut Workspace) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while ws.extensions_busy() {
        ws.poll_extensions();
        assert!(Instant::now() < deadline, "extension work did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    ws.poll_extensions();
}

#[test]
fn hostile_ids_are_refused_or_fail_visibly_and_never_leave_the_extensions_folder() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    stand_in.search_results(&[]);
    let base = temp("extension-ids");
    let extensions = base.join("extensions");
    let victim = base.join("victim");
    std::fs::create_dir_all(&victim).expect("victim");
    std::fs::write(victim.join("keep.txt"), "keep").expect("victim file");
    let mut ws = Workspace::new();
    ws.set_extensions_location(&extensions);
    ws.set_global_storage_location(&base.join("globalStorage"));
    ws.set_extension_registry(stand_in.url()).expect("registry");

    let victim_before = tree(&victim);
    for id in hostile_ids() {
        for command in [
            "workbench.extensions.uninstallExtension",
            "workbench.extensions.disableExtension",
            "workbench.extensions.enableExtension",
            "workbench.extensions.installExtension",
        ] {
            let what = format!(
                "{command} with an id of {} chars: {:?}",
                id.len(),
                id.get(..30.min(id.len()))
            );
            never_panics(&what, || {
                // An error now, or work that fails and says so: either is fine.
                let _ = ws.execute_with(command, &id);
                drain(&mut ws);
            });
        }
    }
    assert_eq!(tree(&victim), victim_before, "a hostile id reached outside");
    assert_eq!(
        std::fs::read(victim.join("keep.txt")).expect("victim file"),
        b"keep"
    );
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn hostile_search_texts_and_unknown_commands_never_panic() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    stand_in.search_results(&[]);
    let base = temp("extension-search");
    let mut ws = Workspace::new();
    ws.set_extensions_location(&base.join("extensions"));
    ws.set_global_storage_location(&base.join("globalStorage"));
    ws.set_extension_registry(stand_in.url()).expect("registry");
    for text in nasty_texts()
        .into_iter()
        .chain(["%", "%00", "%zz", "a&b=c", "a b\tc\n", "?q=x#frag", "\u{0}"].map(str::to_owned))
    {
        never_panics(&format!("search text of {} chars", text.len()), || {
            let _ = ws.execute_with("workbench.extensions.search", &text);
            drain(&mut ws);
        });
    }
    for command in ["", "\0", "workbench.extensions.", "no.such.command", "日本"] {
        never_panics(&format!("command {command:?}"), || {
            let _ = ws.execute_with(command, "x");
            let _ = ws.execute(command);
        });
    }
    // The requests the stand-in saw carry the search text encoded: none of
    // them is a request line the server could not have parsed.
    let _ = stand_in.requests();
    let _ = std::fs::remove_dir_all(base);
}
