//! Large-file and long-edit stress at the workspace seam, with time and
//! peak-memory budgets.
//!
//! The 100 MB and 1 GB cases are `#[ignore]`d (run them explicitly, one at a
//! time, with `cargo nextest run -p rustode-workspace --release --run-ignored
//! all -E 'test(stress)'`). Each case skips itself, with a message, when the
//! machine has less free memory than it needs (AGENTS.md §3.4). Fixtures live
//! in Cargo's target tmp dir, on disk, not in a tmpfs `/tmp`.
//!
//! Budgets leave headroom over the values measured on the development
//! machine; each test's doc comment gives the measured value, its date and
//! the build, using the command above.

use rustode_workspace::{DocumentId, Range, Workspace};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A generated text file of about `bytes` bytes: 79-char lines plus LF.
fn fixture(name: &str, bytes: usize) -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("rustode-stress-{name}.txt"));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes as u64) {
        return path;
    }
    let file = std::fs::File::create(&path).expect("fixture");
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    let line = format!(
        "{}\n",
        "the quick brown fox jumps over the lazy dog 0123456789 ".repeat(2)[..79].to_owned()
    );
    let mut written = 0;
    while written + line.len() <= bytes {
        out.write_all(line.as_bytes()).expect("write");
        written += line.len();
    }
    out.write_all(&line.as_bytes()[..bytes - written])
        .expect("write");
    out.flush().expect("flush");
    path
}

/// A `/proc` field in kB (Linux), e.g. `MemAvailable` or `VmHWM`.
fn proc_kb(file: &str, field: &str) -> Option<u64> {
    let text = std::fs::read_to_string(file).ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Whether there is enough free memory; prints why not otherwise.
fn enough_memory(needed_mb: u64) -> bool {
    let available_mb = proc_kb("/proc/meminfo", "MemAvailable:").unwrap_or(0) / 1024;
    if available_mb < needed_mb {
        eprintln!("skipped: needs {needed_mb} MB free, {available_mb} MB available");
        return false;
    }
    true
}

/// Peak resident memory of this test process, in MB.
fn peak_rss_mb() -> u64 {
    proc_kb("/proc/self/status", "VmHWM:").unwrap_or(0) / 1024
}

fn open_within(name: &str, bytes: usize, time: Duration, peak_mb: u64) {
    let path = fixture(name, bytes);
    let mut ws = Workspace::new();
    let started = Instant::now();
    let id = ws.open(&path).expect("open");
    let elapsed = started.elapsed();
    let peak = peak_rss_mb();
    let doc = ws.document(id).expect("document");
    assert_eq!(doc.len_chars(), bytes);
    eprintln!("open {name}: {elapsed:?}, peak RSS {peak} MB");
    assert!(
        elapsed <= time,
        "open {name} took {elapsed:?}, budget {time:?}"
    );
    assert!(
        peak <= peak_mb,
        "open {name} peaked at {peak} MB, budget {peak_mb} MB"
    );
}

/// 1 MB: measured 2 ms, peak RSS 5 MB (release, 2026-11-17); budget 200 ms, 64 MB.
#[test]
fn stress_open_1_mb() {
    if enough_memory(200) {
        open_within("1mb", 1 << 20, Duration::from_millis(200), 64);
    }
}

/// 100 MB: measured 151 ms, peak RSS 218 MB (release, 2026-11-17); budget 1 s, 450 MB.
#[test]
#[ignore = "large: run explicitly"]
fn stress_open_100_mb() {
    if enough_memory(1_000) {
        open_within("100mb", 100 << 20, Duration::from_secs(1), 450);
    }
}

/// 1 GB: not measured yet; estimated from the 100 MB run, scaled linearly, at
/// ~1.5 s and ~2.2 GB peak. Budget 15 s, 4.5 GB.
#[test]
#[ignore = "large: run explicitly, with the App closed"]
fn stress_open_1_gb() {
    if enough_memory(3_500) {
        open_within("1gb", 1 << 30, Duration::from_secs(15), 4_500);
    }
}

fn random_edits(ws: &mut Workspace, id: DocumentId, count: usize) {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % (bound as u64)).expect("fits")
    };
    for _ in 0..count {
        let len = ws.document(id).expect("doc").len_chars();
        match next(4) {
            0 => ws.set_cursor(id, next(len + 1)).expect("cursor"),
            1 => ws.type_text(id, ["x", " ", "\n"][next(3)]).expect("type"),
            2 => ws.delete_left(id).expect("delete"),
            _ => {
                let start = next(len + 1);
                let end = (start + next(16)).min(len);
                ws.replace(id, Range::new(start, end), "<edit>")
                    .expect("replace");
            }
        }
    }
}

/// 10k random edits then full undo on a 1 MB file: measured 9 ms, peak RSS
/// 9 MB (release, 2026-11-17); budget 5 s (also run in debug builds).
#[test]
fn stress_10k_edits_then_full_undo() {
    if !enough_memory(300) {
        return;
    }
    let path = fixture("edits", 1 << 20);
    let original = std::fs::read_to_string(&path).expect("read");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    let started = Instant::now();
    random_edits(&mut ws, id, 10_000);
    while ws.undo(id).expect("undo") {}
    let elapsed = started.elapsed();
    eprintln!(
        "10k edits + undo: {elapsed:?}, peak RSS {} MB",
        peak_rss_mb()
    );
    assert!(
        ws.document(id).expect("doc").text() == original,
        "full undo restores the file"
    );
    assert!(elapsed <= Duration::from_secs(5), "took {elapsed:?}");
}
