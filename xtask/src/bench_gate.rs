//! `cargo xtask bench-gate`: the micro-benchmark regression gate (ADR 0026).
//!
//! It checks out the base revision in a temporary git worktree and builds the
//! criterion benchmarks of the base and of the current tree. Then it measures
//! them on this machine in alternating rounds (base, current, base, current,
//! …), running the bench executables directly, so that no compile, cargo
//! lock or fingerprint check falls between two measurements. Every run writes
//! to the same criterion directory (`<target>/criterion`), saved as a named
//! baseline. For each benchmark the gate keeps the fastest mean estimate of
//! each tree across the rounds (other programs can only slow a run down) and
//! fails when the current tree's is more than the threshold slower.
//!
//! The base builds into its own target directory, `<target>/bench-gate/target`:
//! cargo names the artifacts of a path package after its path relative to
//! the workspace root, so the two trees would otherwise overwrite each
//! other's build.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// The criterion baselines the gate saves. It removes only these names.
const BASE: &str = "rustode-gate-base";
const CANDIDATE: &str = "rustode-gate-candidate";

/// Printed after a failure.
pub const GUIDE: &str = "\
A benchmark is slower than on the base revision by more than the limit.
See CONTRIBUTING.md, \"Micro-benchmark gate\": re-run the gate on a quiet
machine to rule out noise, then look at the change that slowed it down.";

/// Runs the gate; returns one line per benchmark slower than the limit.
pub fn run(args: &[String]) -> Result<Vec<String>, String> {
    let options = parse_args(args)?;
    let metadata = cargo_metadata(Path::new("."))?;
    let root = path_field(&metadata, "workspace_root")?;
    let target = path_field(&metadata, "target_directory")?;
    let criterion = target.join("criterion");
    remove_baselines(&criterion, &[BASE, CANDIDATE])?;
    let revision = format!("{}^{{commit}}", options.base);
    let sha = git(&root, &["rev-parse", "--verify", "--quiet", &revision])
        .map_err(|e| format!("cannot find the base '{}': {e}", options.base))?;
    println!(
        "bench-gate: base {} ({}), limit {}%, {} rounds",
        options.base,
        &sha[..sha.len().min(10)],
        options.threshold,
        options.rounds
    );
    let gate = target.join("bench-gate");
    let worktree = Worktree::add(&root, &gate.join("base"), &sha)?;
    let base = if bench_targets(&cargo_metadata(&worktree.path)?).is_empty() {
        println!("bench-gate: the base has no benchmarks; every result is new");
        Vec::new()
    } else {
        build(&worktree.path, &gate.join("target"))?
    };
    let candidate = build(&root, &target)?;
    if candidate.is_empty() {
        return Err("this tree has no benchmarks".to_owned());
    }
    let mut base_means = BTreeMap::new();
    let mut candidate_means = BTreeMap::new();
    for round in 1..=options.rounds {
        if !base.is_empty() {
            measure(&base, &criterion, BASE, round)?;
            keep_fastest(&mut base_means, read_baseline(&criterion, BASE)?);
        }
        measure(&candidate, &criterion, CANDIDATE, round)?;
        keep_fastest(&mut candidate_means, read_baseline(&criterion, CANDIDATE)?);
    }
    drop(worktree);
    let comparison = compare(&base_means, &candidate_means, options.threshold);
    println!();
    for line in &comparison.table {
        println!("{line}");
    }
    Ok(comparison.problems)
}

/// Builds the benchmarks of the workspace at `dir` into `target`; returns
/// their executables.
fn build(dir: &Path, target: &Path) -> Result<Vec<Executable>, String> {
    let out = Command::new("cargo")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args([
            "bench",
            "--locked",
            "--workspace",
            "--bench",
            "*",
            "--no-run",
        ])
        .args(["--message-format", "json-render-diagnostics"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run cargo bench: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo bench --no-run failed in {}", dir.display()));
    }
    bench_executables(&String::from_utf8_lossy(&out.stdout))
}

/// Runs every benchmark of one tree once, saving the results as `baseline`.
fn measure(
    benches: &[Executable],
    criterion: &Path,
    baseline: &str,
    round: u32,
) -> Result<(), String> {
    let start = Instant::now();
    for bench in benches {
        let status = Command::new(&bench.path)
            .current_dir(&bench.dir)
            .env("CRITERION_HOME", criterion)
            .args(["--bench", "--save-baseline", baseline, "--noplot"])
            .status()
            .map_err(|e| format!("cannot run {}: {e}", bench.path.display()))?;
        if !status.success() {
            return Err(format!("{} failed", bench.path.display()));
        }
    }
    println!(
        "bench-gate: round {round}: measured {baseline} in {:.0} s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

/// A detached checkout of the base revision, removed when dropped.
struct Worktree {
    root: PathBuf,
    path: PathBuf,
}

impl Worktree {
    fn add(root: &Path, path: &Path, sha: &str) -> Result<Self, String> {
        let worktree = Self {
            root: root.to_owned(),
            path: path.to_owned(),
        };
        worktree.remove(); // left over by an interrupted run
        let dir = path
            .to_str()
            .ok_or_else(|| format!("not a UTF-8 path: {}", path.display()))?;
        git(root, &["worktree", "add", "--detach", "--quiet", dir, sha])?;
        Ok(worktree)
    }

    fn remove(&self) {
        if let Some(dir) = self.path.to_str() {
            let _ = git(&self.root, &["worktree", "remove", "--force", dir]);
        }
        let _ = fs::remove_dir_all(&self.path);
        let _ = git(&self.root, &["worktree", "prune"]);
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        self.remove();
    }
}

fn cargo_metadata(dir: &Path) -> Result<serde_json::Value, String> {
    let out = Command::new("cargo")
        .current_dir(dir)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed in {}:\n{}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cannot parse cargo metadata: {e}"))
}

fn path_field(metadata: &serde_json::Value, field: &str) -> Result<PathBuf, String> {
    metadata[field]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| format!("cargo metadata has no {field}"))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// What to compare against, and how much slower is too slow.
#[derive(Debug, PartialEq)]
pub struct Options {
    /// The git revision whose benchmarks are the baseline.
    pub base: String,
    /// The largest accepted slowdown, in percent of the base's mean time.
    pub threshold: f64,
    /// How many times each tree is measured, alternating base and candidate.
    pub rounds: u32,
}

/// Reads `[--base <rev>] [--threshold <percent>] [--rounds <n>]`; the
/// defaults are `main`, 5% (the local limit of ADR 0026) and 2 rounds.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        base: "main".to_owned(),
        threshold: 5.0,
        rounds: 2,
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--base" => value()?.clone_into(&mut options.base),
            "--threshold" => {
                let text = value()?;
                options.threshold = text
                    .parse::<f64>()
                    .ok()
                    .filter(|t| t.is_finite() && *t >= 0.0)
                    .ok_or_else(|| {
                        format!("--threshold takes a percentage of 0 or more, not '{text}'")
                    })?;
            }
            "--rounds" => {
                let text = value()?;
                options.rounds = text
                    .parse::<u32>()
                    .ok()
                    .filter(|rounds| *rounds > 0)
                    .ok_or_else(|| {
                        format!("--rounds takes a whole number above 0, not '{text}'")
                    })?;
            }
            other => return Err(format!("unknown bench-gate option: {other}")),
        }
    }
    Ok(options)
}

/// A built benchmark, and the package directory cargo would run it from.
#[derive(Debug, PartialEq)]
pub struct Executable {
    pub path: PathBuf,
    pub dir: PathBuf,
}

/// The bench executables in the JSON messages of `cargo bench --no-run`.
pub fn bench_executables(messages: &str) -> Result<Vec<Executable>, String> {
    let mut found = Vec::new();
    for line in messages.lines().filter(|line| !line.trim().is_empty()) {
        let message: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("cannot parse cargo's message '{line}': {e}"))?;
        let is_bench = message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bench"));
        let (Some(path), Some(manifest)) = (
            message["executable"].as_str(),
            message["manifest_path"].as_str(),
        ) else {
            continue;
        };
        if message["reason"] == "compiler-artifact" && is_bench {
            found.push(Executable {
                path: PathBuf::from(path),
                dir: Path::new(manifest)
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_owned),
            });
        }
    }
    Ok(found)
}

/// The bench targets (`package/target`) in `cargo metadata` output.
pub fn bench_targets(metadata: &serde_json::Value) -> Vec<String> {
    let list = |value: &serde_json::Value| value.as_array().cloned().unwrap_or_default();
    let mut targets = Vec::new();
    for package in list(&metadata["packages"]) {
        for target in list(&package["targets"]) {
            if list(&target["kind"]).iter().any(|kind| kind == "bench") {
                targets.push(format!(
                    "{}/{}",
                    package["name"].as_str().unwrap_or("?"),
                    target["name"].as_str().unwrap_or("?")
                ));
            }
        }
    }
    targets
}

/// Adds one round's means to `fastest`, keeping the smaller mean of each
/// benchmark.
pub fn keep_fastest(fastest: &mut BTreeMap<String, f64>, round: BTreeMap<String, f64>) {
    for (id, mean) in round {
        fastest
            .entry(id)
            .and_modify(|best| *best = best.min(mean))
            .or_insert(mean);
    }
}

/// The mean time in nanoseconds of every benchmark saved under `baseline` in
/// criterion's output directory `dir`, keyed by benchmark id. A missing `dir`
/// holds no benchmarks.
pub fn read_baseline(dir: &Path, baseline: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut means = BTreeMap::new();
    if dir.is_dir() {
        collect(dir, dir, baseline, &mut means)?;
    }
    Ok(means)
}

/// Deletes every baseline named in `names` under criterion's output directory
/// `dir`, so that results of an earlier run cannot pass for this one's.
pub fn remove_baselines(dir: &Path, names: &[&str]) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let is_gate_baseline = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.contains(&name));
        if is_gate_baseline && path.join("estimates.json").is_file() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        } else {
            remove_baselines(&path, names)?;
        }
    }
    Ok(())
}

/// Criterion keeps each benchmark's results in `<id dirs>/<baseline>/`, next
/// to its other baselines (`new`, `base`, …) and to `report/` folders.
fn collect(
    root: &Path,
    dir: &Path,
    baseline: &str,
    means: &mut BTreeMap<String, f64>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let estimates = path.join("estimates.json");
        if path.file_name() == Some(baseline.as_ref()) && estimates.is_file() {
            let text = fs::read_to_string(&estimates)
                .map_err(|e| format!("cannot read {}: {e}", estimates.display()))?;
            let mean = mean_ns(&text).map_err(|e| format!("{}: {e}", estimates.display()))?;
            means.insert(benchmark_id(root, dir, &path), mean);
        } else {
            collect(root, &path, baseline, means)?;
        }
    }
    Ok(())
}

/// The id criterion records in `benchmark.json`, or else the benchmark's
/// directory relative to the output directory.
fn benchmark_id(root: &Path, benchmark: &Path, baseline: &Path) -> String {
    fs::read_to_string(baseline.join("benchmark.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["full_id"].as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            let relative = benchmark.strip_prefix(root).unwrap_or(benchmark);
            relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
}

/// The gate's verdict: a table for people, and one line per regression.
#[derive(Debug, Default, PartialEq)]
pub struct Comparison {
    pub table: Vec<String>,
    pub problems: Vec<String>,
}

/// Compares the candidate's mean times with the base's. A benchmark fails
/// when its mean is more than `threshold` percent above the base's.
pub fn compare(
    base: &BTreeMap<String, f64>,
    candidate: &BTreeMap<String, f64>,
    threshold: f64,
) -> Comparison {
    let ids: BTreeSet<&String> = base.keys().chain(candidate.keys()).collect();
    let width = ids.iter().map(|id| id.chars().count()).fold(9, usize::max) + 2;
    let row = |id: &str, base: &str, candidate: &str, change: &str, verdict: &str| {
        format!("{id:<width$}{base:<11}{candidate:<11}{change:<9}{verdict}")
    };
    let mut comparison = Comparison {
        table: vec![row("benchmark", "base", "candidate", "change", "verdict")],
        problems: Vec::new(),
    };
    for id in ids {
        let (old, new) = (base.get(id).copied(), candidate.get(id).copied());
        let line = match (old, new) {
            (Some(old), Some(new)) => {
                let change = (new - old) * 100.0 / old;
                let verdict = if change > threshold {
                    comparison.problems.push(format!(
                        "{id} is {change:.1}% slower than the base (the limit is {threshold}%)"
                    ));
                    "REGRESSION"
                } else {
                    "ok"
                };
                let change = format!("{change:+.1}%");
                row(id, &time(old), &time(new), &change, verdict)
            }
            (Some(old), None) => row(id, &time(old), "-", "-", "removed"),
            (None, Some(new)) => row(id, "-", &time(new), "-", "new"),
            (None, None) => continue,
        };
        comparison.table.push(line);
    }
    comparison
}

/// A duration in nanoseconds, in the largest unit that keeps it at least 1.
fn time(ns: f64) -> String {
    if ns < 1e3 {
        format!("{ns:.1} ns")
    } else if ns < 1e6 {
        format!("{:.2} µs", ns / 1e3)
    } else if ns < 1e9 {
        format!("{:.2} ms", ns / 1e6)
    } else {
        format!("{:.2} s", ns / 1e9)
    }
}

/// The mean time, in nanoseconds, from one criterion `estimates.json`.
pub fn mean_ns(estimates: &str) -> Result<f64, String> {
    let value: serde_json::Value =
        serde_json::from_str(estimates).map_err(|e| format!("not JSON: {e}"))?;
    value["mean"]["point_estimate"]
        .as_f64()
        .filter(|mean| mean.is_finite() && *mean > 0.0)
        .ok_or_else(|| "no positive mean.point_estimate".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape criterion 0.8 writes to `<benchmark>/<baseline>/estimates.json`.
    const ESTIMATES: &str = r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":811.9,"upper_bound":813.0},"point_estimate":812.4,"standard_error":0.28},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":811.0,"upper_bound":812.2},"point_estimate":811.6,"standard_error":0.3},"median_abs_dev":{"confidence_interval":{"confidence_level":0.95,"lower_bound":0.9,"upper_bound":2.1},"point_estimate":1.4,"standard_error":0.3},"slope":null,"std_dev":{"confidence_interval":{"confidence_level":0.95,"lower_bound":1.2,"upper_bound":2.7},"point_estimate":2.0,"standard_error":0.4}}"#;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn options_default_to_main_and_five_percent() {
        let options = |list: &[&str]| parse_args(&args(list));
        assert_eq!(
            options(&[]),
            Ok(Options {
                base: "main".to_owned(),
                threshold: 5.0,
                rounds: 2,
            })
        );
        assert_eq!(
            options(&["--threshold", "10", "--base", "abc123", "--rounds", "3"]),
            Ok(Options {
                base: "abc123".to_owned(),
                threshold: 10.0,
                rounds: 3,
            })
        );
        for bad in [
            &["--threshold", "ten"][..],
            &["--threshold", "-1"],
            &["--threshold", "NaN"],
            &["--threshold"],
            &["--base"],
            &["--rounds", "0"],
            &["--rounds", "two"],
            &["--fast"],
        ] {
            assert!(options(bad).is_err(), "{bad:?} is refused");
        }
    }

    #[test]
    fn bench_targets_are_listed_from_cargo_metadata() {
        let metadata = serde_json::json!({"packages": [
            {"name": "rustode-core", "targets": [
                {"name": "rustode_core", "kind": ["lib"]},
                {"name": "core", "kind": ["bench"]},
            ]},
            {"name": "xtask", "targets": [{"name": "xtask", "kind": ["bin"]}]},
        ]});
        assert_eq!(bench_targets(&metadata), ["rustode-core/core"]);
        assert_eq!(
            bench_targets(&serde_json::json!({"packages": []})),
            Vec::<String>::new()
        );
    }

    #[test]
    fn bench_executables_are_read_from_cargo_messages() {
        // trimmed from `cargo bench --no-run --message-format json-render-diagnostics`
        let messages = [
            r#"{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#ropey@1.6.1","manifest_path":"/r/ropey/Cargo.toml","target":{"kind":["lib"],"name":"ropey"},"executable":null,"fresh":true}"#,
            r#"{"reason":"build-script-executed","package_id":"x"}"#,
            r#"{"reason":"compiler-artifact","package_id":"path+file:///w/crates/core#rustode-core@0.1.0","manifest_path":"/w/crates/core/Cargo.toml","target":{"kind":["bench"],"crate_types":["bin"],"name":"core","src_path":"/w/crates/core/benches/core.rs","test":false},"executable":"/t/release/deps/core-87a42f9602048662","fresh":true}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(
            bench_executables(&messages),
            Ok(vec![Executable {
                path: PathBuf::from("/t/release/deps/core-87a42f9602048662"),
                dir: PathBuf::from("/w/crates/core"),
            }])
        );
        assert!(bench_executables("not json").is_err());
    }

    #[test]
    fn the_mean_is_the_point_estimate_of_the_mean() {
        assert_eq!(mean_ns(ESTIMATES), Ok(812.4));
    }

    /// A fresh, empty directory under the system temp dir.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustode-bench-gate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().expect("has a parent")).expect("dirs");
        fs::write(file, text).expect("write");
    }

    fn estimates(mean: f64) -> String {
        ESTIMATES.replace("812.4", &mean.to_string())
    }

    #[test]
    fn a_baseline_is_read_from_every_benchmark_that_has_it() {
        let dir = scratch("read");
        let full_id = |id: &str| format!(r#"{{"group_id":"x","full_id":"{id}"}}"#);
        write(&dir, "edit/insert/gate/estimates.json", &estimates(10.0));
        write(
            &dir,
            "edit/insert/gate/benchmark.json",
            &full_id("edit/insert"),
        );
        write(&dir, "edit/insert/new/estimates.json", &estimates(99.0));
        // benchmark ids with characters criterion cannot use in a directory name
        write(&dir, "undo/a_b/gate/estimates.json", &estimates(20.0));
        write(&dir, "undo/a_b/gate/benchmark.json", &full_id("undo/a b"));
        // without benchmark.json the directory path names it
        write(&dir, "solo/gate/estimates.json", &estimates(30.0));
        write(&dir, "report/index.html", "");
        let means = read_baseline(&dir, "gate").expect("readable");
        let expected: BTreeMap<String, f64> = [
            ("edit/insert".to_owned(), 10.0),
            ("solo".to_owned(), 30.0),
            ("undo/a b".to_owned(), 20.0),
        ]
        .into();
        assert_eq!(means, expected);
        assert_eq!(
            read_baseline(&dir.join("missing"), "gate"),
            Ok(BTreeMap::new())
        );

        write(&dir, "broken/gate/estimates.json", "{}");
        let error = read_baseline(&dir, "gate").expect_err("a broken file fails");
        assert!(error.contains("broken"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_gate_baselines_are_removed_and_others_kept() {
        let dir = scratch("clear");
        write(&dir, "edit/insert/gate/estimates.json", &estimates(1.0));
        write(&dir, "edit/insert/new/estimates.json", &estimates(2.0));
        write(&dir, "undo/all/other/estimates.json", &estimates(3.0));
        write(&dir, "undo/all/gate/estimates.json", &estimates(4.0));
        remove_baselines(&dir, &["gate", "other"]).expect("removable");
        assert_eq!(read_baseline(&dir, "gate"), Ok(BTreeMap::new()));
        assert_eq!(read_baseline(&dir, "other"), Ok(BTreeMap::new()));
        assert_eq!(
            read_baseline(&dir, "new"),
            Ok(means(&[("edit/insert", 2.0)]))
        );
        remove_baselines(&dir.join("missing"), &["gate"]).expect("nothing to remove");
        let _ = fs::remove_dir_all(&dir);
    }

    fn means(list: &[(&str, f64)]) -> BTreeMap<String, f64> {
        list.iter()
            .map(|(id, mean)| ((*id).to_owned(), *mean))
            .collect()
    }

    #[test]
    fn each_benchmark_keeps_its_fastest_round() {
        let mut fastest = BTreeMap::new();
        keep_fastest(&mut fastest, means(&[("a", 10.0), ("b", 20.0)]));
        keep_fastest(&mut fastest, means(&[("a", 12.0), ("b", 15.0), ("c", 1.0)]));
        assert_eq!(fastest, means(&[("a", 10.0), ("b", 15.0), ("c", 1.0)]));
    }

    #[test]
    fn only_a_slowdown_above_the_threshold_fails() {
        let base = means(&[
            ("buffer/read", 100.0),
            ("edit/slower", 100.0),
            ("edit/at_threshold", 2000.0),
            ("gone", 1.0),
            ("undo/faster", 3.0e6),
        ]);
        let candidate = means(&[
            ("buffer/read", 104.0),
            ("edit/slower", 110.5),
            ("edit/at_threshold", 2200.0),
            ("added", 2.5e9),
            ("undo/faster", 1.5e6),
        ]);
        let comparison = compare(&base, &candidate, 10.0);
        assert_eq!(
            comparison.problems,
            ["edit/slower is 10.5% slower than the base (the limit is 10%)"]
        );
        assert_eq!(
            comparison.table,
            [
                "benchmark          base       candidate  change   verdict",
                "added              -          2.50 s     -        new",
                "buffer/read        100.0 ns   104.0 ns   +4.0%    ok",
                "edit/at_threshold  2.00 µs    2.20 µs    +10.0%   ok",
                "edit/slower        100.0 ns   110.5 ns   +10.5%   REGRESSION",
                "gone               1.0 ns     -          -        removed",
                "undo/faster        3.00 ms    1.50 ms    -50.0%   ok",
            ]
        );
    }
}
