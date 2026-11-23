//! Production source files stay under 800 lines.
//!
//! Every `.rs` file under `crates/*/src/` and `xtask/src/` is counted whole
//! (`str::lines`), inline `#[cfg(test)]` modules included: the limit is about
//! how much a reader must hold, and a test module in the file counts. Files in
//! `tests/`, `benches/` and `examples/` are not production source and are not
//! counted.
//!
//! [`TEMPORARY_ALLOW`] lists the files that were already over the limit when
//! the check was added, each with the ticket that removes it. A file that is
//! not listed and goes over the limit fails; a listed file that falls to the
//! limit or under fails until it is removed from the list, so the list only
//! shrinks.

use std::path::{Path, PathBuf};

/// Longest allowed production source file, in lines.
const LIMIT: usize = 800;

/// Files over [`LIMIT`] at commit 8c3c9ab. Ticket 34 of the design-rules
/// plan ("Layout and view-models: splitting the large files") removes each one.
const TEMPORARY_ALLOW: &[&str] = &[
    "crates/cli/src/bench/report.rs", // 1206 lines at 8c3c9ab; ticket 34 removes it
    "crates/config/src/jsonc.rs",     // 847 lines at 8c3c9ab; ticket 34 removes it
    "crates/config/src/when.rs",      // 857 lines at 8c3c9ab; ticket 34 removes it
    "crates/core/src/editor.rs",      // 954 lines at 8c3c9ab; ticket 34 removes it
    "crates/gui/src/app.rs",          // 1125 lines at 8c3c9ab; ticket 34 removes it
    "crates/gui/src/editor.rs",       // 1185 lines at 8c3c9ab; ticket 34 removes it
    "crates/gui/src/find.rs",         // 892 lines at 8c3c9ab; ticket 34 removes it
    "crates/tui/src/app.rs",          // 1275 lines at 8c3c9ab; ticket 34 removes it
    "crates/tui/src/view.rs",         // 1198 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/commands.rs", // 862 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/extensions.rs", // 960 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/groups.rs", // 862 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/lib.rs",    // 807 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/lsp/mod.rs", // 842 lines at 8c3c9ab; ticket 34 removes it
    "crates/workspace/src/lsp/suggest.rs", // 868 lines at 8c3c9ab; ticket 34 removes it
];

/// Every production source file under `root`, as a path relative to it with
/// `/` separators.
fn production_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut crate_dirs = vec![root.join("xtask")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        crate_dirs.extend(entries.flatten().map(|e| e.path()));
    }
    for dir in crate_dirs {
        collect(&dir.join("src"), &mut found);
    }
    found.sort();
    found
        .into_iter()
        .filter_map(|p| p.strip_prefix(root).ok().map(Path::to_path_buf))
        .collect()
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// What is wrong, given each file's name and line count and the allow-list.
fn problems(files: &[(String, usize)], allow: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (name, lines) in files {
        let listed = allow.contains(&name.as_str());
        if *lines > LIMIT && !listed {
            found.push(format!("{name} has {lines} lines, over {LIMIT}"));
        } else if *lines <= LIMIT && listed {
            found.push(format!(
                "{name} has {lines} lines, within {LIMIT}: remove it from TEMPORARY_ALLOW"
            ));
        }
    }
    for name in allow {
        if !files.iter().any(|(n, _)| n == name) {
            found.push(format!("{name} is in TEMPORARY_ALLOW and does not exist"));
        }
    }
    found
}

#[test]
fn production_files_stay_within_the_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let files: Vec<(String, usize)> = production_files(&root)
        .into_iter()
        .map(|rel| {
            let text = std::fs::read_to_string(root.join(&rel)).expect("source is UTF-8");
            let name = rel.to_string_lossy().replace('\\', "/");
            (name, text.lines().count())
        })
        .collect();
    assert!(files.len() > 50, "found only {} source files", files.len());
    assert_eq!(problems(&files, TEMPORARY_ALLOW), Vec::<String>::new());
}

#[test]
fn the_allow_list_is_sorted_and_free_of_duplicates() {
    let mut sorted = TEMPORARY_ALLOW.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, TEMPORARY_ALLOW);
}

fn file(name: &str, lines: usize) -> (String, usize) {
    (name.to_owned(), lines)
}

#[test]
fn a_file_at_the_limit_passes() {
    assert_eq!(problems(&[file("a.rs", 800)], &[]), Vec::<String>::new());
}

#[test]
fn an_unlisted_file_over_the_limit_fails() {
    assert_eq!(
        problems(&[file("a.rs", 801)], &[]),
        ["a.rs has 801 lines, over 800"]
    );
}

#[test]
fn a_listed_file_over_the_limit_passes() {
    assert_eq!(
        problems(&[file("a.rs", 1200)], &["a.rs"]),
        Vec::<String>::new()
    );
}

#[test]
fn a_listed_file_that_dropped_under_the_limit_fails_until_removed() {
    assert_eq!(
        problems(&[file("a.rs", 800)], &["a.rs"]),
        ["a.rs has 800 lines, within 800: remove it from TEMPORARY_ALLOW"]
    );
}

#[test]
fn a_listed_file_that_is_gone_fails() {
    assert_eq!(
        problems(&[], &["a.rs"]),
        ["a.rs is in TEMPORARY_ALLOW and does not exist"]
    );
}

#[test]
fn only_production_sources_are_found() {
    let dir = std::env::temp_dir().join(format!("rustode-size-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for path in [
        "crates/a/src/lib.rs",
        "crates/a/src/deep/mod.rs",
        "crates/a/tests/t.rs",
        "crates/a/benches/b.rs",
        "xtask/src/main.rs",
        "xtask/tests/t.rs",
        "crates/a/src/notes.md",
    ] {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, "fn main() {}\n").expect("write");
    }
    let found: Vec<String> = production_files(&dir)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        found,
        [
            "crates/a/src/deep/mod.rs",
            "crates/a/src/lib.rs",
            "xtask/src/main.rs"
        ]
    );
}
