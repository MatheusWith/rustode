//! Generated, deterministic fixtures: never real user files (spec, "Testing
//! Decisions"). The same scenario always produces the same bytes.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use super::scenario::{Fixture, SearchSpec};

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

/// Words for the filler of a source file: lower-case ASCII, none of them
/// containing `handle_`, so only the tokens the generator plants can match.
const WORDS: [&str; 32] = [
    "alpha", "bravo", "cache", "delta", "echo", "frame", "gamma", "hotel", "index", "joint",
    "kernel", "lambda", "merge", "node", "orbit", "parse", "query", "route", "state", "token",
    "unit", "value", "watch", "xenon", "yield", "zone", "buffer", "cursor", "editor", "window",
    "render", "layout",
];

/// `SplitMix64`: a small seeded generator, the same on every machine.
#[derive(Debug, Clone, Copy)]
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn word(&mut self) -> &'static str {
        WORDS[usize::try_from(self.below(WORDS.len() as u64)).unwrap_or(0)]
    }
}

/// What the generated monorepo holds and what a search of it must find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeSummary {
    /// Files written, every folder.
    pub files: u64,
    /// Bytes written.
    pub bytes: u64,
    /// FNV-1a 64 over every file's relative path and bytes, in write order.
    pub digest: u64,
    /// Files a search with the canonical include and exclude must look in.
    pub searched_files: u64,
    /// Files among them with at least one match.
    pub expected_files: u64,
    /// Matches of the canonical pattern in those files.
    pub expected_matches: u64,
}

/// The include globs the expected counts assume.
pub(crate) const CANONICAL_INCLUDE: &str = "**/*.ts,**/*.rs";
/// The exclude globs the expected counts assume.
pub(crate) const CANONICAL_EXCLUDE: &str = "**/legacy/**";
/// The pattern the expected counts assume (a regular expression, case-insensitive).
pub(crate) const CANONICAL_PATTERN: &str = r"\bhandle_[a-z]+_[0-9]{3}\b";

/// The `.gitignore` of the monorepo's root.
const GITIGNORE: &str = "generated/\n*.log\n";

impl TreeSummary {
    /// Whether `spec` is the query the expected counts were worked out for.
    pub(crate) fn expects(spec: &SearchSpec) -> bool {
        spec.regex
            && spec.pattern == CANONICAL_PATTERN
            && spec.include == CANONICAL_INCLUDE
            && spec.exclude == CANONICAL_EXCLUDE
    }
}

struct Writer {
    summary: TreeSummary,
    root: std::path::PathBuf,
}

impl Writer {
    /// Writes one file and folds it into the digest.
    fn file(&mut self, relative: &str, text: &str) -> io::Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, text)?;
        let mut digest = self.summary.digest;
        for byte in relative.bytes().chain(text.bytes()) {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3);
        }
        self.summary.digest = digest;
        self.summary.files += 1;
        self.summary.bytes += text.len() as u64;
        Ok(())
    }
}

/// A source file of `bytes` bytes in lines of `line_bytes`, with `planted`
/// matching tokens (`handle_<word>_<3 digits>`) and as many decoys that must
/// not match (two digits, four digits, no word boundary before it).
fn source(rng: &mut SplitMix, bytes: u64, line_bytes: u32, planted: u64) -> String {
    let width = usize::try_from(line_bytes).map_or(80, |b| b.saturating_sub(1).max(16));
    let lines = usize::try_from(bytes / u64::from(line_bytes))
        .unwrap_or(1)
        .max(1);
    let mut tokens: Vec<(usize, String)> = Vec::new();
    for _ in 0..planted {
        let line = usize::try_from(rng.below(lines as u64)).unwrap_or(0);
        let token = format!("handle_{}_{:03}", rng.word(), 100 + rng.below(900));
        tokens.push((line, format!("    call {token}(x);")));
    }
    for n in 0..planted.min(2) {
        let line = usize::try_from(rng.below(lines as u64)).unwrap_or(0);
        let word = rng.word();
        let decoy = match n {
            0 => format!("    call handle_{word}_{:02}(x);", 10 + rng.below(90)),
            _ => format!("    call xhandle_{word}_{:03}(x);", 100 + rng.below(900)),
        };
        tokens.push((line, decoy));
    }
    let mut out = String::with_capacity(lines * (width + 1));
    for line in 0..lines {
        let mut text = tokens
            .iter()
            .find(|(at, _)| *at == line)
            .map_or_else(String::new, |(_, t)| t.clone());
        while text.len() < width {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(rng.word());
        }
        text.truncate(width);
        while text.ends_with(' ') {
            text.pop();
            text.push('.');
        }
        out.push_str(&text);
        out.push('\n');
    }
    out
}

/// How many times `\bhandle_[a-z]+_[0-9]{3}\b` matches `text`, worked out by
/// hand (case-sensitively: the generator plants only lower-case words), so the
/// oracle does not use a regex engine it is checking.
fn planted_in(text: &str) -> u64 {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut found = 0;
    for (at, _) in text.match_indices("handle_") {
        let before_ok = text[..at].chars().next_back().is_none_or(|c| !word(c));
        let rest = &text[at + "handle_".len()..];
        let letters = rest.chars().take_while(char::is_ascii_lowercase).count();
        let Some(after_letters) = rest[letters..].strip_prefix('_') else {
            continue;
        };
        let digits = after_letters
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        let end_ok = after_letters[digits..]
            .chars()
            .next()
            .is_none_or(|c| !word(c));
        if before_ok && letters > 0 && digits == 3 && end_ok {
            found += 1;
        }
    }
    found
}

/// Writes the monorepo of a `search` scenario under `root` (which must be
/// new or empty) and returns what it holds.
///
/// - `src/pkg<N>/mod<M>/file<I>.<ext>`: `spec.files` files, extensions in the
///   rotation ts, rs, md, ts, rs; every tenth goes under `src/legacy/`;
/// - `node_modules/`, `generated/` and `logs/*.log`: a twentieth to a
///   fortieth as many files, with matching tokens too, which the default
///   excludes and `.gitignore` must keep out.
///
/// # Errors
/// When a file or folder cannot be written.
pub(crate) fn write_tree(
    root: &Path,
    spec: &SearchSpec,
    fixture: Fixture,
) -> io::Result<TreeSummary> {
    fs::create_dir_all(root)?;
    let mut w = Writer {
        summary: TreeSummary {
            files: 0,
            bytes: 0,
            digest: 0xCBF2_9CE4_8422_2325,
            searched_files: 0,
            expected_files: 0,
            expected_matches: 0,
        },
        root: root.to_path_buf(),
    };
    w.file(".gitignore", GITIGNORE)?;
    let mut rng = SplitMix(u64::from(spec.seed));
    for i in 0..spec.files {
        let ext = ["ts", "rs", "md", "ts", "rs"][(i % 5) as usize];
        let planted = rng.below(7);
        let text = source(&mut rng, fixture.bytes, fixture.line_bytes, planted);
        let legacy = i % 10 == 9;
        let dir = if legacy {
            "src/legacy".to_owned()
        } else {
            format!("src/pkg{:02}/mod{:02}", i % 23, (i / 23) % 17)
        };
        let included = !legacy && matches!(ext, "ts" | "rs");
        if included {
            w.summary.searched_files += 1;
            let found = planted_in(&text);
            w.summary.expected_matches += found;
            w.summary.expected_files += u64::from(found > 0);
        }
        w.file(&format!("{dir}/file{i:05}.{ext}"), &text)?;
    }
    for (folder, divisor, ext) in [
        ("node_modules/dep", 20, "ts"),
        ("generated", 20, "ts"),
        ("logs", 40, "log"),
    ] {
        for i in 0..(spec.files / divisor).max(1) {
            let text = source(&mut rng, fixture.bytes, fixture.line_bytes, 3);
            w.file(&format!("{folder}/extra{i:04}.{ext}"), &text)?;
        }
    }
    Ok(w.summary)
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

    fn spec(files: u32) -> SearchSpec {
        SearchSpec {
            files,
            seed: 7,
            pattern: CANONICAL_PATTERN.to_owned(),
            regex: true,
            include: CANONICAL_INCLUDE.to_owned(),
            exclude: CANONICAL_EXCLUDE.to_owned(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustode-fixture-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_tree_is_deterministic_and_has_what_the_excludes_must_hide() {
        let fixture = Fixture {
            bytes: 2048,
            line_bytes: 64,
        };
        let (a, b) = (temp("a"), temp("b"));
        let first = write_tree(&a, &spec(60), fixture).expect("tree a");
        let second = write_tree(&b, &spec(60), fixture).expect("tree b");
        assert_eq!(first, second, "same seed, same tree");
        assert_ne!(
            first.digest,
            write_tree(
                &temp("c"),
                &SearchSpec {
                    seed: 8,
                    ..spec(60)
                },
                fixture
            )
            .expect("tree c")
            .digest
        );
        // 60 sources + 3 node_modules + 3 generated + 1 log + .gitignore.
        assert_eq!(first.files, 60 + 3 + 3 + 1 + 1);
        assert!(a.join(".gitignore").is_file());
        assert!(a.join("node_modules/dep/extra0000.ts").is_file());
        assert!(a.join("generated/extra0000.ts").is_file());
        assert!(a.join("logs/extra0000.log").is_file());
        assert!(a.join("src/legacy/file00009.rs").is_file());
        // 60 files, rotation ts rs md ts rs: 4 of 5 are searchable, and the 6
        // legacy ones (every tenth) are not: 54 files outside legacy, of which
        // the md ones (every fifth) are not either.
        assert!(
            first.searched_files > 35 && first.searched_files < 50,
            "{first:?}"
        );
        assert!(first.expected_files <= first.searched_files);
        assert!(first.expected_matches >= first.expected_files);
        let on_disk: u64 = fs::read_dir(a.join("src/pkg00/mod00")).map_or(0, |d| d.count() as u64);
        assert!(on_disk >= 1);
    }

    #[test]
    fn the_oracle_counts_what_the_pattern_matches_and_not_the_decoys() {
        let text = "a handle_abc_123 b\nhandle_abc_12 x\nxhandle_abc_123\nhandle_abc_1234\nhandle_abc_999.\nz handle_q_000 handle_r_001";
        // abc_123, abc_999, q_000 and r_001 match (two on the last line); two
        // digits, four digits and no boundary before the token do not.
        assert_eq!(planted_in(text), 4);
    }

    #[test]
    fn only_the_canonical_query_has_expected_counts() {
        assert!(TreeSummary::expects(&spec(1)));
        assert!(!TreeSummary::expects(&SearchSpec {
            include: "**/*.ts".to_owned(),
            ..spec(1)
        }));
        assert!(!TreeSummary::expects(&SearchSpec {
            regex: false,
            ..spec(1)
        }));
    }
}
