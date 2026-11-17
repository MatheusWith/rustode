//! Micro-benchmarks of `rustode-core`: buffer reads, edits and undo/redo,
//! through the crate's public API (ADR 0026).
//!
//! Every input comes from a fixed seed, so each run measures the same work.
//! `cargo xtask bench-gate` compares them with `main`; `cargo bench -p
//! rustode-core` runs them on their own. Each benchmark is short (0.5 s of
//! warm-up, 2 s of measurement), so the suite takes well under a minute.
//!
//! Edits change the text and grow the undo history, so the edit benchmarks
//! run on fresh editors, built outside the timing, each one for a bounded
//! number of steps (`timed`): the work per step stays the same however many
//! iterations criterion asks for, and memory stays at a few megabytes.

use criterion::{Criterion, criterion_group, criterion_main};
use rustode_core::{Buffer, Editor, Range, Selection};
use std::hint::black_box;
use std::time::{Duration, Instant};

/// The large document: 1 MiB of code-like text.
const DOC_BYTES: usize = 1 << 20;
/// The seed of every generated input.
const SEED: u64 = 0x5EED_2026;
/// A word typed one char at a time, ending in a space (two undo steps).
const WORD: [&str; 8] = ["r", "u", "s", "t", "o", "d", "e", " "];
/// How many cursors type at once in the multi-cursor benchmarks.
const CURSORS: usize = 64;
/// How many words the undo benchmark types before undoing them.
const WORDS: usize = 100;

/// `SplitMix64`: a small fixed-seed generator, so no rand crate is needed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound` (the modulo bias does not matter here).
    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("usize fits in u64");
        usize::try_from(self.next() % bound).expect("below a usize bound")
    }

    /// `count` numbers in `0..bound`.
    fn sample(&mut self, bound: usize, count: usize) -> Vec<usize> {
        (0..count).map(|_| self.below(bound)).collect()
    }
}

/// About 1 MiB of indented lines of words, a few of them not ASCII, so that
/// char and byte offsets differ.
fn document() -> String {
    let vocabulary: Vec<&str> = "fn let mut self buffer editor = { } (selection) função // →"
        .split(' ')
        .collect();
    let mut rng = Rng(SEED);
    let mut text = String::with_capacity(DOC_BYTES + 256);
    while text.len() < DOC_BYTES {
        text.extend(std::iter::repeat_n(' ', 4 * rng.below(4)));
        for _ in 0..rng.below(12) {
            text.push_str(vocabulary[rng.below(vocabulary.len())]);
            text.push(' ');
        }
        text.push('\n');
    }
    text
}

/// An editor over `text` with one cursor at the start of each of `count`
/// lines picked from the seed.
fn multi_cursor_editor(text: &str, count: usize) -> Editor {
    let mut editor = Editor::new(text);
    let buffer = editor.buffer();
    let lines = Rng(SEED ^ 3).sample(buffer.len_lines(), count);
    let cursors: Vec<Selection> = lines
        .iter()
        .map(|&line| Selection::cursor(buffer.line_start(line)))
        .collect();
    editor.set_selections(cursors, 0).expect("cursors inside");
    editor
}

fn type_word(editor: &mut Editor) {
    for key in WORD {
        editor.type_text(key);
    }
}

/// Times `iters` calls of `step`, at most `per_editor` of them on each
/// editor made by `setup`. Making each editor, and dropping it with the
/// history it grew, are not timed.
fn timed(
    iters: u64,
    per_editor: u64,
    setup: &impl Fn() -> Editor,
    mut step: impl FnMut(&mut Editor, usize),
) -> Duration {
    let mut total = Duration::ZERO;
    let mut left = iters;
    while left > 0 {
        let steps = left.min(per_editor);
        let mut editor = setup();
        let start = Instant::now();
        for (index, _) in (0..steps).enumerate() {
            step(&mut editor, index);
        }
        total += start.elapsed();
        drop(black_box(editor));
        left -= steps;
    }
    total
}

fn buffer(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("buffer");
    group.bench_function("from_text_1mib", |b| {
        b.iter(|| Buffer::from_text(black_box(&text)));
    });
    let editor = Editor::new(&text);
    let buffer = editor.buffer();
    let lines = Rng(SEED ^ 1).sample(buffer.len_lines(), 256);
    group.bench_function("line_access_256_lines", |b| {
        b.iter(|| {
            lines
                .iter()
                .map(|&line| {
                    buffer.line_start(line) + buffer.line_len(line) + buffer.line_text(line).len()
                })
                .sum::<usize>()
        });
    });
    let offsets = Rng(SEED ^ 2).sample(buffer.len_chars() + 1, 256);
    group.bench_function("position_of_256_offsets", |b| {
        b.iter(|| {
            offsets
                .iter()
                .map(|&offset| editor.position_of(offset).column)
                .sum::<usize>()
        });
    });
    group.finish();
}

fn edit(c: &mut Criterion) {
    let text = document();
    let offsets = Rng(SEED ^ 4).sample(text.chars().count() - 1, 4096);
    let fresh = || Editor::new(&text);
    let mut group = c.benchmark_group("edit");
    group.bench_function("insert_char_at_random_offset", |b| {
        b.iter_custom(|iters| {
            timed(iters, 16_384, &fresh, |editor, i| {
                let at = offsets[i % offsets.len()];
                black_box(editor.replace(Range::empty(at), "x")).expect("inside");
            })
        });
    });
    group.bench_function("delete_char_at_random_offset", |b| {
        b.iter_custom(|iters| {
            timed(iters, 16_384, &fresh, |editor, i| {
                let at = offsets[i % offsets.len()] % (editor.buffer().len_chars() - 1);
                black_box(editor.replace(Range::new(at, at + 1), "")).expect("inside");
            })
        });
    });
    let middle = || {
        let mut editor = fresh();
        let half = editor.buffer().len_chars() / 2;
        editor.set_cursor(half).expect("inside");
        editor
    };
    group.bench_function("type_word", |b| {
        b.iter_custom(|iters| timed(iters, 2_048, &middle, |editor, _| type_word(editor)));
    });
    let cursors = || multi_cursor_editor(&text, CURSORS);
    group.bench_function("type_word_64_cursors", |b| {
        b.iter_custom(|iters| timed(iters, 64, &cursors, |editor, _| type_word(editor)));
    });
    group.finish();
}

/// Undoes every step, then redoes them all; returns how many there were.
fn undo_redo_all(editor: &mut Editor) -> usize {
    let mut steps = 0;
    while editor.undo() {
        steps += 1;
    }
    while editor.redo() {
        steps += 1;
    }
    steps
}

fn undo(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("undo");
    let mut words = Editor::new(&text);
    let half = words.buffer().len_chars() / 2;
    words.set_cursor(half).expect("inside");
    for _ in 0..WORDS {
        type_word(&mut words);
    }
    group.bench_function("undo_redo_100_words", |b| {
        b.iter(|| undo_redo_all(&mut words));
    });
    let mut cursors = multi_cursor_editor(&text, CURSORS);
    type_word(&mut cursors);
    group.bench_function("undo_redo_word_at_64_cursors", |b| {
        b.iter(|| undo_redo_all(&mut cursors));
    });
    group.finish();
}

fn config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .sample_size(50)
        .without_plots()
}

criterion_group! {
    name = benches;
    config = config();
    targets = buffer, edit, undo
}
criterion_main!(benches);
