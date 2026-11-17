//! The rustode side of a scenario, measured in this process.

use std::path::Path;
use std::time::{Duration, Instant};

use rustode_workspace::{Encoding, Eol, Workspace};

use super::scenario::RustodeSide;

/// Opens `path` with `Workspace::open` on a fresh `Workspace` each time and
/// returns the recorded durations (warm-ups dropped). Only the `open` call is
/// timed; checking the document and dropping the workspace are not.
pub(crate) fn open_file(
    path: &Path,
    bytes: u64,
    side: RustodeSide,
) -> Result<Vec<Duration>, String> {
    let mut samples = Vec::new();
    for run in 0..side.warmup + side.repetitions {
        let _span = rustode_workspace::trace::spans::file_op("bench-open", path).entered();
        let mut workspace = Workspace::new();
        let started = Instant::now();
        let opened = workspace.open(path);
        let elapsed = started.elapsed();
        let id = opened.map_err(|err| format!("Workspace::open failed: {err}"))?;
        let document = workspace
            .document(id)
            .ok_or("the opened document is missing")?;
        let chars = u64::try_from(document.len_chars()).unwrap_or(u64::MAX);
        if chars != bytes || document.encoding() != Encoding::Utf8 || document.eol() != Eol::Lf {
            return Err(format!(
                "the opened document has {chars} chars, {:?}, {:?}; expected {bytes} chars, UTF-8, LF",
                document.encoding(),
                document.eol()
            ));
        }
        drop(workspace);
        if run >= side.warmup {
            samples.push(elapsed);
        }
    }
    Ok(samples)
}
