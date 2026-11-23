//! The layer table of AGENTS.md §11.7.5, checked against `cargo metadata`.
//!
//! The crates form layers and a dependency only points to a lower layer. The
//! check fails on:
//!
//! - an edge to a higher layer;
//! - an edge between two crates of the same layer that is not in
//!   [`SAME_LAYER`];
//! - a workspace crate that has no layer in [`LAYERS`];
//! - an I/O crate ([`IO_CRATES`]) among the normal or build dependencies of an
//!   L0 crate;
//! - a UI crate ([`UI_CRATES`]) among the dependencies of a crate below L3;
//! - a normal or build dependency on a tools crate.
//!
//! Which dependency kinds count:
//!
//! - **Edges between workspace crates** (upward, same-layer): every kind, so a
//!   dev-dependency cannot reach upward or sideways either. A crate's
//!   dev-dependency on itself (a feature switch for its own tests) is not an
//!   edge.
//! - **I/O crates in L0:** normal and build dependencies only; a test may use
//!   a file-system helper without making the crate do I/O.
//! - **UI crates below L3:** every kind. A test of a lower layer has no
//!   business drawing.
//! - **Tools crates** (`xtask`, the test servers, the benchmark runner) may
//!   depend on anything. Nothing may depend on one except through a
//!   dev-dependency. Today the test servers have no dependents: tests build
//!   and spawn them as processes (`crates/workspace/tests/lsp_typescript.rs`).
//!
//! The table lists crates that are planned and not in the tree yet; an entry
//! with no crate is ignored. Modeled on filmcraft's layer task.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Command;

/// Layer of each crate: 0 is the lowest; [`TOOLS`] is outside the stack.
const LAYERS: &[(&str, u8)] = &[
    ("rustode-core", 0),
    ("rustode-syntax", 1),
    ("rustode-fs", 1),
    ("rustode-config", 1),
    ("rustode-trace", 1),
    ("rustode-protocol", 1),
    // Planned: the supervised-worker crate and the network crate.
    ("rustode-guard", 1),
    ("rustode-net", 1),
    ("rustode-workspace", 2),
    ("rustode-cli", 3),
    ("rustode-tui", 3),
    ("rustode-gui", 3),
    ("xtask", TOOLS),
    ("rustode-test-lsp", TOOLS),
    ("rustode-test-dap", TOOLS),
    // Planned: the test registry and the benchmark runner.
    ("rustode-test-registry", TOOLS),
    ("test-registry", TOOLS),
    ("rustode-bench", TOOLS),
];

/// The layer of a crate outside the stack.
const TOOLS: u8 = 99;

/// Edges between two crates of the same layer that are allowed.
const SAME_LAYER: &[(&str, &str)] = &[
    // The guard logs through the shared tracing setup (planned crate).
    ("rustode-guard", "rustode-trace"),
];

/// Crates that do I/O: not allowed in L0.
const IO_CRATES: &[&str] = &["notify", "ignore", "ureq", "zip"];

/// Crates that draw: allowed only in the frontends.
const UI_CRATES: &[&str] = &["ratatui", "eframe", "egui", "winit", "egui_kittest"];

fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn metadata() -> Value {
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

/// Every layering violation in `metadata`, one sentence each.
fn violations(metadata: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let members: BTreeMap<&str, Option<u8>> = packages
        .iter()
        .filter_map(|p| p["name"].as_str())
        .map(|name| (name, layer_of(name)))
        .collect();
    for package in &packages {
        let name = package["name"].as_str().unwrap_or_default();
        let Some(layer) = members.get(name).copied().flatten() else {
            found.push(format!("{name} has no layer in the table"));
            continue;
        };
        let deps = package["dependencies"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for dep in &deps {
            let dep_name = dep["name"].as_str().unwrap_or_default();
            let kind = dep["kind"].as_str().unwrap_or("normal");
            check_edge(&mut found, (name, layer), (dep_name, kind), &members);
        }
    }
    found
}

fn check_edge(
    found: &mut Vec<String>,
    (name, layer): (&str, u8),
    (dep, kind): (&str, &str),
    members: &BTreeMap<&str, Option<u8>>,
) {
    if layer != TOOLS && layer < 3 && UI_CRATES.contains(&dep) {
        found.push(format!("{name} (L{layer}) depends on the UI crate {dep}"));
    }
    if layer == 0 && kind != "dev" && IO_CRATES.contains(&dep) {
        found.push(format!("{name} (L0) depends on the I/O crate {dep}"));
    }
    let Some(dep_layer) = members.get(dep).copied() else {
        return; // a third-party crate
    };
    if dep == name {
        return; // a feature switch for the crate's own tests
    }
    let Some(dep_layer) = dep_layer else {
        return; // reported as a crate with no layer
    };
    if dep_layer == TOOLS {
        if layer != TOOLS && kind != "dev" {
            found.push(format!("{name} depends on the tools crate {dep} ({kind})"));
        }
    } else if layer == TOOLS {
        // tools may depend on anything
    } else if dep_layer > layer {
        found.push(format!(
            "{name} (L{layer}) depends upward on {dep} (L{dep_layer})"
        ));
    } else if dep_layer == layer && !SAME_LAYER.contains(&(name, dep)) {
        found.push(format!(
            "{name} and {dep} are both L{layer} and the edge is not in SAME_LAYER"
        ));
    }
}

#[test]
fn the_workspace_follows_the_layer_table() {
    assert_eq!(violations(&metadata()), Vec::<String>::new());
}

#[test]
fn every_workspace_crate_has_a_layer() {
    let metadata = metadata();
    for package in metadata["packages"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("name");
        assert!(layer_of(name).is_some(), "{name} has no layer");
    }
}

/// A fake workspace: `(name, [(dependency, kind)])`, kind `""` for normal.
fn fake(crates: &[(&str, &[(&str, &str)])]) -> Value {
    let packages: Vec<Value> = crates
        .iter()
        .map(|(name, deps)| {
            let deps: Vec<Value> = deps
                .iter()
                .map(|(dep, kind)| {
                    json!({ "name": dep, "kind": if kind.is_empty() { Value::Null } else { json!(kind) } })
                })
                .collect();
            json!({ "name": name, "dependencies": deps })
        })
        .collect();
    json!({ "packages": packages })
}

#[test]
fn an_upward_edge_is_caught() {
    let fake = fake(&[
        ("rustode-core", &[("rustode-workspace", "")]),
        ("rustode-workspace", &[]),
    ]);
    assert_eq!(
        violations(&fake),
        ["rustode-core (L0) depends upward on rustode-workspace (L2)"]
    );
}

#[test]
fn an_upward_dev_dependency_is_caught() {
    let fake = fake(&[
        ("rustode-fs", &[("rustode-workspace", "dev")]),
        ("rustode-workspace", &[]),
    ]);
    assert_eq!(violations(&fake).len(), 1);
}

#[test]
fn a_crate_with_no_layer_is_caught() {
    let fake = fake(&[
        ("rustode-mystery", &[("rustode-core", "")]),
        ("rustode-core", &[]),
    ]);
    assert_eq!(
        violations(&fake),
        ["rustode-mystery has no layer in the table"]
    );
}

#[test]
fn an_io_crate_in_l0_is_caught_but_not_as_a_dev_dependency() {
    let bad = fake(&[("rustode-core", &[("notify", ""), ("zip", "build")])]);
    assert_eq!(violations(&bad).len(), 2);
    let fine = fake(&[("rustode-core", &[("ignore", "dev")])]);
    assert_eq!(violations(&fine), Vec::<String>::new());
    let above = fake(&[("rustode-workspace", &[("ureq", ""), ("zip", "")])]);
    assert_eq!(violations(&above), Vec::<String>::new());
}

#[test]
fn a_ui_crate_below_l3_is_caught() {
    let fake = fake(&[
        ("rustode-workspace", &[("ratatui", "")]),
        ("rustode-syntax", &[("egui_kittest", "dev")]),
        ("rustode-tui", &[("ratatui", "")]),
        (
            "rustode-gui",
            &[("eframe", ""), ("winit", ""), ("egui", "")],
        ),
    ]);
    assert_eq!(violations(&fake).len(), 2);
}

#[test]
fn a_same_layer_edge_needs_the_allow_list() {
    let bad = fake(&[
        ("rustode-fs", &[("rustode-config", "")]),
        ("rustode-config", &[]),
    ]);
    assert_eq!(
        violations(&bad),
        ["rustode-fs and rustode-config are both L1 and the edge is not in SAME_LAYER"]
    );
    let allowed = fake(&[
        ("rustode-guard", &[("rustode-trace", "")]),
        ("rustode-trace", &[]),
    ]);
    assert_eq!(violations(&allowed), Vec::<String>::new());
    let reversed = fake(&[
        ("rustode-trace", &[("rustode-guard", "")]),
        ("rustode-guard", &[]),
    ]);
    assert_eq!(violations(&reversed).len(), 1);
}

#[test]
fn a_tools_crate_is_never_a_normal_dependency() {
    let bad = fake(&[
        ("rustode-workspace", &[("rustode-test-lsp", "")]),
        ("rustode-test-lsp", &[]),
    ]);
    assert_eq!(violations(&bad).len(), 1);
    let fine = fake(&[
        ("rustode-workspace", &[("rustode-test-lsp", "dev")]),
        ("rustode-test-lsp", &[]),
        ("xtask", &[("rustode-workspace", ""), ("serde_json", "")]),
    ]);
    assert_eq!(violations(&fine), Vec::<String>::new());
}

#[test]
fn a_crates_dev_dependency_on_itself_is_not_an_edge() {
    let fake = fake(&[("rustode-workspace", &[("rustode-workspace", "dev")])]);
    assert_eq!(violations(&fake), Vec::<String>::new());
}
