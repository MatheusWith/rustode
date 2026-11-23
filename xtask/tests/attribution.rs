//! `ATTRIBUTION.md`'s "Editor dependencies" table is the single source for the
//! crates rustode depends on: every direct third-party dependency of a
//! workspace member has a row, with the right kind and the right users.
//!
//! The kind of a crate is the strongest use any member makes of it, in this
//! order: `prod` (a normal dependency of a crate that ships), `build` (a build
//! dependency), `dev` (a dev-dependency) and `tooling` (a dependency only of
//! `xtask` or of the never-shipped test crates). A row for a crate that is not
//! a direct dependency (one pulled in through another) is allowed only when
//! its "Used by" cell says "through".

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

/// Workspace members that never ship: a dependency only they use is `tooling`.
const TOOLING_CRATES: [&str; 3] = ["xtask", "rustode-test-lsp", "rustode-test-dap"];

const KINDS: [&str; 4] = ["prod", "build", "dev", "tooling"];

/// What `cargo metadata` says about one direct third-party dependency.
#[derive(Debug, PartialEq, Eq)]
struct Actual {
    kind: &'static str,
    users: BTreeSet<String>,
}

/// One parsed table row.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    kind: String,
    users: BTreeSet<String>,
    transitive: bool,
}

fn metadata() -> serde_json::Value {
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("metadata is JSON")
}

fn rank(kind: &str) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(KINDS.len())
}

/// The direct third-party dependencies of every workspace member.
fn actual(metadata: &serde_json::Value) -> BTreeMap<String, Actual> {
    let mut found: BTreeMap<String, Actual> = BTreeMap::new();
    let packages = metadata["packages"].as_array().expect("packages");
    for package in packages {
        let user = package["name"].as_str().unwrap_or_default();
        let dependencies = package["dependencies"].as_array().expect("dependencies");
        for dep in dependencies {
            if !dep["source"].is_string() {
                continue; // a path dependency: one of rustode's own crates
            }
            let kind = match dep["kind"].as_str() {
                _ if TOOLING_CRATES.contains(&user) => "tooling",
                Some("dev") => "dev",
                Some("build") => "build",
                _ => "prod",
            };
            let name = dep["name"].as_str().unwrap_or_default().to_owned();
            let entry = found.entry(name).or_insert_with(|| Actual {
                kind,
                users: BTreeSet::new(),
            });
            if rank(kind) < rank(entry.kind) {
                entry.kind = kind;
            }
            entry.users.insert(user.to_owned());
        }
    }
    found
}

/// The backticked words of a cell, up to a "(through …)" note.
fn words(cell: &str) -> Vec<String> {
    let head = cell.split("(through").next().unwrap_or_default();
    head.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, w)| w.to_owned())
        .collect()
}

/// The rows of the "Editor dependencies" table:
/// `| Crate | Kind | Version | License | Used by | What for |`.
fn rows(attribution: &str) -> BTreeMap<String, Row> {
    let mut rows = BTreeMap::new();
    let mut inside = false;
    for line in attribution.lines() {
        if line.starts_with("## ") {
            inside = line == "## Editor dependencies";
            continue;
        }
        if !inside || !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // cells[0] is empty (before the first bar).
        let (Some(name), Some(kind), Some(used_by)) = (cells.get(1), cells.get(2), cells.get(5))
        else {
            continue;
        };
        let name = name.trim_matches('`').to_owned();
        let users: BTreeSet<String> = words(used_by).into_iter().collect();
        rows.insert(
            name,
            Row {
                kind: (*kind).to_owned(),
                users,
                transitive: used_by.contains("through"),
            },
        );
    }
    rows
}

/// Every disagreement between the table and the manifests, one line each.
fn problems(metadata: &serde_json::Value, attribution: &str) -> Vec<String> {
    let actual = actual(metadata);
    let rows = rows(attribution);
    let mut found = Vec::new();
    for (name, want) in &actual {
        let Some(row) = rows.get(name) else {
            found.push(format!("`{name}` has no row"));
            continue;
        };
        if row.kind != want.kind {
            found.push(format!(
                "`{name}` is `{}` in the table but `{}` in the manifests",
                row.kind, want.kind
            ));
        }
        if row.users != want.users {
            found.push(format!(
                "`{name}` is used by {:?} in the table but by {:?} in the manifests",
                row.users, want.users
            ));
        }
    }
    for (name, row) in &rows {
        if !actual.contains_key(name) && !row.transitive {
            found.push(format!("`{name}` has a row but is not a direct dependency"));
        }
        if !KINDS.contains(&row.kind.as_str()) {
            found.push(format!("`{name}` has the unknown kind `{}`", row.kind));
        }
    }
    found
}

#[test]
fn attribution_lists_every_direct_dependency_with_its_kind_and_users() {
    let attribution =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../ATTRIBUTION.md"))
            .expect("ATTRIBUTION.md is readable");
    assert_eq!(problems(&metadata(), &attribution), Vec::<String>::new());
}

#[test]
fn the_check_catches_a_missing_row_a_wrong_kind_and_wrong_users() {
    let fake = serde_json::json!({ "packages": [
        { "name": "rustode-gui", "dependencies": [
            { "name": "tracing", "source": "registry+x", "kind": null },
            { "name": "egui_kittest", "source": "registry+x", "kind": "dev" },
            { "name": "rustode-workspace", "source": null, "kind": null },
            { "name": "regex", "source": "registry+x", "kind": null }
        ] },
        { "name": "rustode-fs", "dependencies": [
            { "name": "tracing", "source": "registry+x", "kind": null }
        ] },
        { "name": "xtask", "dependencies": [
            { "name": "serde_json", "source": "registry+x", "kind": null }
        ] }
    ] });
    let table = "\
## Editor dependencies

| Crate | Kind | Version | License | Used by | What for |
|---|---|---|---|---|---|
| `tracing` | prod | 0.1 | MIT | `rustode-fs` | spans |
| `egui_kittest` | prod | 0.36 | MIT | `rustode-gui` | tests |
| `serde_json` | tooling | 1 | MIT | `xtask` | metadata |
| `tracing-log` | prod | 0.2 | MIT | `rustode-trace` (through `tracing-subscriber`) | bridge |
| `left_over` | prod | 1 | MIT | `rustode-fs` | nothing |

## Another section

| `ignored` | prod | 1 | MIT | `nobody` | not read |
";
    let found = problems(&fake, table);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(found.iter().any(|p| p.contains("`regex` has no row")));
    assert!(found.iter().any(|p| p.contains("`tracing` is used by")));
    assert!(
        found
            .iter()
            .any(|p| p.contains("`egui_kittest` is `prod` in the table but `dev`"))
    );
    assert!(
        found
            .iter()
            .any(|p| p.contains("`left_over` has a row but is not a direct dependency"))
    );
}

#[test]
fn a_table_that_matches_the_manifests_has_no_problems() {
    let fake = serde_json::json!({ "packages": [
        { "name": "rustode-fs", "dependencies": [
            { "name": "tracing", "source": "registry+x", "kind": null }
        ] },
        { "name": "rustode-test-lsp", "dependencies": [
            { "name": "serde_json", "source": "registry+x", "kind": null }
        ] }
    ] });
    let table = "\
## Editor dependencies

| `tracing` | prod | 0.1 | MIT | `rustode-fs` | spans |
| `serde_json` | tooling | 1 | MIT | `rustode-test-lsp` | messages |
";
    assert_eq!(problems(&fake, table), Vec::<String>::new());
}
