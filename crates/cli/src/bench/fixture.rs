//! Generated, deterministic fixtures: never real user files (spec, "Testing
//! Decisions"). The same scenario always produces the same bytes.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use super::scenario::Fixture;

const FILLER: &str = "the quick brown fox jumps over the lazy dog ";

/// Line `n` (counting from 1) of a fixture with `line_bytes`-byte lines,
/// without its LF: an 8-digit line number, a space and filler text, never
/// ending in a space. Every line differs, so the first one is unambiguous.
pub(crate) fn line(n: u64, line_bytes: u32) -> String {
    let width = usize::try_from(line_bytes).map_or(usize::MAX, |b| b.saturating_sub(1));
    let mut line = format!("{n:08} ");
    while line.len() < width {
        line.push_str(FILLER);
    }
    line.truncate(width);
    if line.ends_with(' ') {
        line.pop();
        line.push('.');
    }
    line
}

/// Writes the fixture: whole lines with LF, the last one cut so the file
/// has exactly `fixture.bytes` bytes.
pub(crate) fn write(path: &Path, fixture: Fixture) -> io::Result<()> {
    let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
    let mut left = fixture.bytes;
    let mut n = 1;
    while left > 0 {
        let mut text = line(n, fixture.line_bytes);
        text.push('\n');
        let take = usize::try_from(left).map_or(text.len(), |left| left.min(text.len()));
        out.write_all(&text.as_bytes()[..take])?;
        left -= u64::try_from(take).map_err(io::Error::other)?;
        n += 1;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_numbered_and_sized() {
        let first = line(1, 80);
        assert_eq!(first.len(), 79);
        assert!(first.starts_with("00000001 the quick brown fox"), "{first}");
        assert!(!first.ends_with(' '));
        assert_ne!(line(2, 80), first);
        assert_eq!(line(13_108, 16), "00013108 the qu");
        assert_eq!(line(1, 14), "00000001 the.", "a cut on a space ends in '.'");
    }
}
