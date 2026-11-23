//! Every crate root denies the panic family in production code, with
//! `#![cfg_attr(not(test), deny(...))]` (AGENTS.md §11.7.1 rule 2). The
//! attribute is the mechanism filmcraft uses: it leaves unit tests, and the
//! integration tests, examples and benches (separate crates), untouched.
//!
//! The roots checked are the `lib` and `bin` targets of every workspace
//! package, read from `cargo metadata`, so a new crate or binary without the
//! attribute fails here. The lint set required of each package is
//! [`required`]: later tickets add `string_slice` and `indexing_slicing`
//! there, for the packages whose sites have been fixed.

use std::process::Command;

/// The panic family, denied in production code of every crate.
const PANIC_FAMILY: [&str; 6] = [
    "clippy::unwrap_used",
    "clippy::expect_used",
    "clippy::panic",
    "clippy::unreachable",
    "clippy::unimplemented",
    "clippy::todo",
];

/// The lints a package's roots must deny. One place to extend per package.
fn required(_package: &str) -> Vec<&'static str> {
    PANIC_FAMILY.to_vec()
}

/// The lints of `required` that `source` does not deny with
/// `#![cfg_attr(not(test), deny(...))]`. Comments are ignored.
fn missing(source: &str, required: &[&str]) -> Vec<String> {
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect();
    let prefix = "#![cfg_attr(not(test),deny(";
    let mut denied: Vec<&str> = Vec::new();
    let mut rest = code.as_str();
    while let Some(start) = rest.find(prefix) {
        let after = &rest[start + prefix.len()..];
        let Some(end) = after.find("))]") else { break };
        denied.extend(after[..end].split(',').filter(|lint| !lint.is_empty()));
        rest = &after[end..];
    }
    required
        .iter()
        .filter(|lint| !denied.contains(lint))
        .map(|lint| (*lint).to_owned())
        .collect()
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

/// `(package, source path)` of every `lib` and `bin` target.
fn roots(metadata: &serde_json::Value) -> Vec<(String, String)> {
    let mut roots = Vec::new();
    for package in metadata["packages"].as_array().expect("packages") {
        let name = package["name"].as_str().expect("package name");
        for target in package["targets"].as_array().expect("targets") {
            let production = target["kind"]
                .as_array()
                .expect("kind")
                .iter()
                .filter_map(|kind| kind.as_str())
                .any(|kind| matches!(kind, "lib" | "bin"));
            if production {
                let path = target["src_path"].as_str().expect("src_path");
                roots.push((name.to_owned(), path.to_owned()));
            }
        }
    }
    roots
}

#[test]
fn every_crate_root_denies_the_panic_family_in_production_code() {
    let roots = roots(&metadata());
    assert!(roots.len() >= 14, "found only {} crate roots", roots.len());
    let mut problems = Vec::new();
    for (package, path) in &roots {
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let lints = missing(&source, &required(package));
        if !lints.is_empty() {
            problems.push(format!(
                "{path} ({package}) does not deny {}",
                lints.join(", ")
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "add `#![cfg_attr(not(test), deny(...))]` with these lints to each crate root \
         (AGENTS.md §11.7.1):\n{}",
        problems.join("\n")
    );
}

const FULL: &str = "//! Docs.\n\n#![cfg_attr(\n    not(test),\n    deny(\n        clippy::unwrap_used,\n        \
    clippy::expect_used,\n        clippy::panic,\n        clippy::unreachable,\n        \
    clippy::unimplemented,\n        clippy::todo\n    )\n)]\n\nmod a;\n";

#[test]
fn the_check_accepts_the_attribute_however_rustfmt_wraps_it() {
    assert_eq!(missing(FULL, &PANIC_FAMILY), Vec::<String>::new());
    let one_line = "#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, \
        clippy::panic, clippy::unreachable, clippy::unimplemented, clippy::todo))]";
    assert_eq!(missing(one_line, &PANIC_FAMILY), Vec::<String>::new());
}

#[test]
fn the_check_fails_without_the_attribute() {
    assert_eq!(missing("mod a;\n", &PANIC_FAMILY).len(), 6);
}

#[test]
fn the_check_names_each_lint_that_is_absent() {
    let source = FULL.replace("clippy::unreachable,", "");
    assert_eq!(missing(&source, &PANIC_FAMILY), ["clippy::unreachable"]);
}

#[test]
fn the_check_rejects_look_alikes() {
    // Applies to test builds only, warns instead of denying, is a comment.
    for source in [
        FULL.replace("not(test)", "test"),
        FULL.replace("deny(", "warn("),
        FULL.replace("#![cfg_attr", "// #![cfg_attr"),
    ] {
        assert_eq!(missing(&source, &PANIC_FAMILY).len(), 6, "{source}");
    }
}

#[test]
fn the_required_set_can_grow_per_package() {
    let source = FULL;
    let mut wanted = PANIC_FAMILY.to_vec();
    wanted.push("clippy::indexing_slicing");
    assert_eq!(missing(source, &wanted), ["clippy::indexing_slicing"]);
}
