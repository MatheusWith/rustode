//! Search across a generated monorepo at the workspace seam, with time and
//! peak-memory budgets: a literal search and a regex search to completion,
//! and the time to the first result.
//!
//! The tree lives in Cargo's target tmp dir, on disk, and is generated once
//! (a `.complete` marker keeps its counts): nested crates of Rust-like
//! files, plus folders the search must skip, `target/` folders ignored by
//! `.gitignore` and a `node_modules/` excluded by `search.exclude`'s
//! defaults. `search.maxResults` is lifted so both searches run to the end.
//!
//! The large case is `#[ignore]`d; run it alone, in release, with
//! `cargo nextest run -p rustode-workspace --release --run-ignored all -E 'test(stress_search)'`.
//! Each case skips itself when memory is short (AGENTS.md §3.4). Budgets
//! leave headroom over the values measured on the development machine;
//! each test's doc comment gives the measured value, its date, commit and
//! build, with the command above.

use rustode_workspace::{FindOptions, FindQuery, SearchQuery, SearchState, Workspace};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A `/proc` field in kB (Linux), e.g. `MemAvailable` or `VmHWM`.
fn proc_kb(file: &str, field: &str) -> Option<u64> {
    let text = std::fs::read_to_string(file).ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn enough_memory(needed_mb: u64) -> bool {
    let available_mb = proc_kb("/proc/meminfo", "MemAvailable:").unwrap_or(0) / 1024;
    if available_mb < needed_mb {
        eprintln!("skipped: needs {needed_mb} MB free, {available_mb} MB available");
        return false;
    }
    true
}

fn peak_rss_mb() -> u64 {
    proc_kb("/proc/self/status", "VmHWM:").unwrap_or(0) / 1024
}

/// The generated tree and how many matches each search must find in it.
struct Monorepo {
    root: PathBuf,
    files: usize,
    literal: usize,
    regex: usize,
}

/// One generated source file: 100 lines, some holding `needle`, some an
/// `fn item_N(` definition. Returns the text and both counts.
fn source(seed: usize) -> (String, usize, usize) {
    let (mut text, mut needles, mut items) = (String::new(), 0, 0);
    for line in 0..100 {
        if (seed + line).is_multiple_of(37) {
            let _ = writeln!(text, "    let needle = value_{line}; // Needle {seed}");
            needles += 2;
        } else if line % 10 == 0 {
            let _ = writeln!(text, "fn item_{line}(value: u32) -> u32 {{");
            items += 1;
        } else {
            let _ = writeln!(
                text,
                "    let x_{line} = compute(y, {seed}); // quick brown fox"
            );
        }
    }
    (text, needles, items)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, text).expect("write");
}

/// `crates` crates of `per_crate` files in five nested modules each, every
/// crate with an ignored `target/` of 10 files, and 200 excluded files in
/// `node_modules/`.
fn monorepo(name: &str, crates: usize, per_crate: usize) -> Monorepo {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("rustode-monorepo-{name}"));
    let marker = root.join(".complete");
    if let Ok(text) = std::fs::read_to_string(&marker) {
        let counts: Vec<usize> = text
            .split(' ')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        if let [files, literal, regex] = counts[..] {
            return Monorepo {
                root,
                files,
                literal,
                regex,
            };
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    let (mut files, mut literal, mut regex) = (0, 0, 0);
    write(&root.join(".gitignore"), "target/\n");
    for krate in 0..crates {
        let dir = root.join(format!("crates/crate-{krate:03}"));
        for file in 0..per_crate {
            let (text, needles, items) = source(krate * 31 + file);
            let path = dir.join(format!("src/m{}/sub/file_{file:03}.rs", file % 5));
            write(&path, &text);
            files += 1;
            literal += needles;
            regex += items;
        }
        for file in 0..10 {
            let (text, ..) = source(file);
            write(&dir.join(format!("target/debug/gen_{file}.rs")), &text);
        }
    }
    for package in 0..200 {
        let (text, ..) = source(package);
        write(
            &root.join(format!("node_modules/pkg-{package}/index.js")),
            &text,
        );
    }
    std::fs::write(&marker, format!("{files} {literal} {regex}")).expect("marker");
    Monorepo {
        root,
        files,
        literal,
        regex,
    }
}

/// The time to the first result and to the end of one search, with the
/// number of matches and files found.
fn timed(ws: &mut Workspace, query: SearchQuery) -> (Duration, Duration, usize, usize) {
    let first: Arc<Mutex<Option<Instant>>> = Arc::default();
    let seen = Arc::clone(&first);
    ws.set_search_waker(move || {
        let mut seen = seen.lock().expect("first");
        seen.get_or_insert_with(Instant::now);
    });
    let started = Instant::now();
    ws.start_search(query).expect("search");
    let deadline = started + Duration::from_secs(120);
    while ws.search_results().is_running() {
        ws.poll_search();
        assert!(Instant::now() < deadline, "the search did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    let total = started.elapsed();
    let first = first
        .lock()
        .expect("first")
        .map_or(total, |at| at - started);
    let results = ws.search_results();
    assert_eq!(results.state(), SearchState::Done);
    (first, total, results.match_count(), results.file_count())
}

/// Budgets for one tree: first result, literal search, regex search, peak RSS.
struct Budget {
    first: Duration,
    literal: Duration,
    regex: Duration,
    peak_mb: u64,
}

fn search_within(name: &str, crates: usize, per_crate: usize, budget: &Budget) {
    let tree = monorepo(name, crates, per_crate);
    let settings = tree.root.with_extension("settings.json");
    std::fs::write(&settings, r#"{ "search.maxResults": null }"#).expect("settings");
    let mut ws = Workspace::new();
    ws.load_config(&settings, &tree.root.with_extension("keybindings.json"));
    ws.open_folder(&tree.root).expect("open folder");
    let literal =
        SearchQuery::new(FindQuery::new("needle", FindOptions::default()).expect("literal"));
    let (first, literal_time, found, files) = timed(&mut ws, literal);
    assert_eq!(
        found, tree.literal,
        "every needle, none from ignored folders"
    );
    assert!(files <= tree.files);
    let regex = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    let regex = SearchQuery::new(FindQuery::new(r"fn item_\d+\(", regex).expect("regex"));
    let (_, regex_time, found, _) = timed(&mut ws, regex);
    assert_eq!(found, tree.regex);
    let peak = peak_rss_mb();
    eprintln!(
        "search {name} ({} files): first result {first:?}, literal {literal_time:?}, regex {regex_time:?}, peak RSS {peak} MB",
        tree.files
    );
    assert!(
        first <= budget.first,
        "first result after {first:?}, budget {:?}",
        budget.first
    );
    assert!(
        literal_time <= budget.literal,
        "literal search took {literal_time:?}, budget {:?}",
        budget.literal
    );
    assert!(
        regex_time <= budget.regex,
        "regex search took {regex_time:?}, budget {:?}",
        budget.regex
    );
    assert!(
        peak <= budget.peak_mb,
        "peaked at {peak} MB, budget {} MB",
        budget.peak_mb
    );
}

/// 500 files (20 crates of 25). Measured at `1d264c1` on 2026-11-18, debug
/// build (with `CARGO_PROFILE_DEV_DEBUG=0`), three runs of
/// `cargo nextest run -p rustode-workspace -E 'test(stress_search_small)'`
/// from the project root, on a machine busy with other builds (load
/// average about 9 on 12 threads): first result 1.9–3.5 ms, literal
/// search 30–37 ms, regex search 34–45 ms, peak RSS 11 MB. The budgets
/// hold for the debug build the full suite runs.
#[test]
fn stress_search_small_monorepo() {
    if enough_memory(300) {
        search_within(
            "small",
            20,
            25,
            &Budget {
                first: Duration::from_secs(1),
                literal: Duration::from_secs(2),
                regex: Duration::from_secs(2),
                peak_mb: 100,
            },
        );
    }
}

/// 10,000 files (200 crates of 50): 49.5 MB in 1,000,000 lines, holding
/// 54,040 literal and 97,297 regex matches, plus 2,200 skipped files
/// (measured with `wc -lc` over the searched files). Measured at `1d264c1` on
/// 2026-11-18, release build, six runs of the command in the module
/// comment from the project root, on a machine busy with other builds
/// (load average about 9 on 12 threads): first result 0.53–8.2 ms (median
/// 0.6 ms), literal search to completion 214–322 ms (median 219 ms), regex
/// search 218–250 ms (median 231 ms), peak RSS 21–22 MB.
#[test]
#[ignore = "large: run explicitly, in release"]
fn stress_search_monorepo() {
    if enough_memory(800) {
        search_within(
            "large",
            200,
            50,
            &Budget {
                first: Duration::from_millis(200),
                literal: Duration::from_millis(1500),
                regex: Duration::from_millis(1500),
                peak_mb: 150,
            },
        );
    }
}
