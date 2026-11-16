//! `cargo xtask coverage`: line-coverage gates per crate group.
//!
//! Core crates need at least 85% of lines, frontends at least 60%. Thresholds are
//! never lowered; exceptions are justified in a pull request comment instead.

use std::collections::BTreeMap;
use std::process::Command;

/// A group of crates sharing one minimum line coverage.
pub struct Gate {
    pub name: &'static str,
    pub crates: &'static [&'static str],
    pub min_percent: f64,
}

pub const GATES: &[Gate] = &[
    Gate {
        name: "core",
        crates: &[
            "core",
            "syntax",
            "fs",
            "config",
            "workspace",
            "protocol",
            "trace",
        ],
        min_percent: 85.0,
    },
    Gate {
        name: "frontends",
        crates: &["cli", "tui", "gui"],
        min_percent: 60.0,
    },
];

/// Lines (covered, total) per crate directory name, from an llvm-cov JSON export.
pub fn lines_per_crate(export: &serde_json::Value) -> BTreeMap<String, (u64, u64)> {
    let mut totals = BTreeMap::new();
    let files = export["data"][0]["files"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    for file in files {
        let Some(name) = file["filename"].as_str().and_then(crate_of) else {
            continue;
        };
        let lines = &file["summary"]["lines"];
        let entry = totals.entry(name).or_insert((0, 0));
        entry.0 += lines["covered"].as_u64().unwrap_or(0);
        entry.1 += lines["count"].as_u64().unwrap_or(0);
    }
    totals
}

fn crate_of(path: &str) -> Option<String> {
    let rest = path.split("/crates/").nth(1)?;
    rest.split('/').next().map(str::to_owned)
}

/// Checks every gate; returns one line per failed gate. Groups without code yet pass.
pub fn check(totals: &BTreeMap<String, (u64, u64)>) -> (Vec<String>, Vec<String>) {
    let (mut report, mut problems) = (Vec::new(), Vec::new());
    for gate in GATES {
        let (covered, count) = gate
            .crates
            .iter()
            .filter_map(|c| totals.get(*c))
            .fold((0, 0), |acc, (cov, cnt)| (acc.0 + cov, acc.1 + cnt));
        if count == 0 {
            report.push(format!("{}: no instrumented lines yet", gate.name));
            continue;
        }
        #[allow(clippy::cast_precision_loss)] // line counts are far below 2^52
        let percent = covered as f64 * 100.0 / count as f64;
        report.push(format!(
            "{}: {percent:.1}% of {count} lines (minimum {}%)",
            gate.name, gate.min_percent
        ));
        if percent < gate.min_percent {
            problems.push(format!(
                "{} coverage {percent:.1}% is below {}%",
                gate.name, gate.min_percent
            ));
        }
    }
    (report, problems)
}

/// Runs the instrumented test suite and checks the gates.
pub fn run() -> Result<Vec<String>, String> {
    let out = Command::new("cargo")
        .args([
            "llvm-cov",
            "nextest",
            "--workspace",
            "--exclude",
            "xtask",
            "--json",
            "--summary-only",
        ])
        .output()
        .map_err(|e| format!("cannot run cargo llvm-cov: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo llvm-cov failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let export: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("cannot parse llvm-cov JSON: {e}"))?;
    let (report, problems) = check(&lines_per_crate(&export));
    for line in report {
        println!("{line}");
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(files: &[(&str, u64, u64)]) -> serde_json::Value {
        let files: Vec<_> = files
            .iter()
            .map(|(f, cov, cnt)| serde_json::json!({"filename": f, "summary": {"lines": {"covered": cov, "count": cnt}}}))
            .collect();
        serde_json::json!({"data": [{"files": files}]})
    }

    #[test]
    fn gates_pass_fail_and_skip_empty_groups() {
        let totals = lines_per_crate(&export(&[
            ("/r/crates/core/src/lib.rs", 90, 100),
            ("/r/crates/cli/src/main.rs", 50, 100),
            ("/r/xtask/src/main.rs", 0, 100),
        ]));
        let (report, problems) = check(&totals);
        assert_eq!(problems, ["frontends coverage 50.0% is below 60%"]);
        assert!(report[0].starts_with("core: 90.0%"), "{report:?}");

        let (report, problems) = check(&BTreeMap::new());
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(
            report,
            [
                "core: no instrumented lines yet",
                "frontends: no instrumented lines yet"
            ]
        );
    }
}
