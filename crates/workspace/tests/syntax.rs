//! Workspace seam: Rust documents keep a syntax tree in step with every
//! edit, and expose highlights for the lines a frontend shows.

use rustode_workspace::{
    DocumentId, Eol, Position, Range, Selection, TokenKind, Workspace, syntax_tree_of,
};
use std::path::PathBuf;

fn fixture(name: &str, file: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-syntax-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(file);
    std::fs::write(&path, text).expect("fixture");
    path
}

fn open(name: &str, file: &str, text: &str) -> (Workspace, DocumentId, PathBuf) {
    let path = fixture(name, file, text);
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id, path)
}

/// The highlights of `lines` as (line, text, kind), read back from the text.
fn tokens(
    ws: &Workspace,
    id: DocumentId,
    lines: std::ops::Range<usize>,
) -> Vec<(usize, String, TokenKind)> {
    let doc = ws.document(id).expect("doc");
    let chars: Vec<char> = doc.text().chars().collect();
    doc.highlights(lines)
        .into_iter()
        .map(|h| {
            let text: String = chars[h.range.start..h.range.end].iter().collect();
            (doc.position_of(h.range.start).line, text, h.kind)
        })
        .collect()
}

fn owned(expected: &[(usize, &str, TokenKind)]) -> Vec<(usize, String, TokenKind)> {
    expected
        .iter()
        .map(|&(line, text, kind)| (line, text.to_owned(), kind))
        .collect()
}

const SAMPLE: &str = "// Adds.
fn add(a: u32, b: u32) -> u32 {
    let s = \"hi\\n\";
    if a > b { return a; }
    std::cmp::max(a, b)
}
";

#[test]
fn rust_documents_are_highlighted_with_the_kinds_vs_code_colors() {
    use TokenKind::{
        Comment, ControlKeyword, Escape, Function, Keyword, Operator, Punctuation, String, Type,
        Variable,
    };
    let (ws, id, _) = open("sample", "lib.rs", SAMPLE);
    let expected = [
        (0, "// Adds.", Comment),
        (1, "fn", Keyword),
        (1, "add", Function),
        (1, "(", Punctuation),
        (1, "a", Variable),
        (1, ":", Operator),
        (1, "u32", Type),
        (1, ",", Punctuation),
        (1, "b", Variable),
        (1, ":", Operator),
        (1, "u32", Type),
        (1, ")", Punctuation),
        (1, "u32", Type),
        (1, "{", Punctuation),
        (2, "let", Keyword),
        (2, "s", Variable),
        (2, "\"hi", String),
        (2, "\\n", Escape),
        (2, "\"", String),
        (2, ";", Punctuation),
        (3, "if", ControlKeyword),
        (3, "a", Variable),
        (3, "b", Variable),
        (3, "{", Punctuation),
        (3, "return", ControlKeyword),
        (3, "a", Variable),
        (3, ";", Punctuation),
        (3, "}", Punctuation),
        (4, "std", Type),
        (4, "::", Operator),
        (4, "cmp", Type),
        (4, "::", Operator),
        (4, "max", Function),
        (4, "(", Punctuation),
        (4, "a", Variable),
        (4, ",", Punctuation),
        (4, "b", Variable),
        (4, ")", Punctuation),
        (5, "}", Punctuation),
    ];
    assert_eq!(tokens(&ws, id, 0..6), owned(&expected));
}

#[test]
fn highlights_cover_only_the_lines_asked_for_in_chars() {
    let (ws, id, _) = open("chars", "main.rs", "// é\nlet é = \"日本\";\nlet x = 1;\n");
    assert_eq!(
        tokens(&ws, id, 1..2),
        owned(&[
            (1, "let", TokenKind::Keyword),
            (1, "é", TokenKind::Variable),
            (1, "\"日本\"", TokenKind::String),
            (1, ";", TokenKind::Punctuation),
        ])
    );
    let doc = ws.document(id).expect("doc");
    let first = doc.highlights(1..2)[0].range;
    assert_eq!(first, Range::new(5, 8), "char offsets, not bytes");
    assert!(doc.highlights(3..9).is_empty(), "past the end");
}

#[test]
fn a_comment_opened_above_the_view_colors_the_lines_in_it() {
    let (ws, id, _) = open("block", "main.rs", "/*\n hidden\n*/\nfn f() {}\n");
    assert_eq!(
        tokens(&ws, id, 1..2),
        owned(&[(1, " hidden\n", TokenKind::Comment)])
    );
}

#[test]
fn other_languages_stay_plain() {
    for (file, text) in [
        ("notes.txt", "fn main() {}\n"),
        ("data.ini", "a = 1\n"),
        ("build.log", "fn main() {}\n"),
    ] {
        let (ws, id, _) = open(file, file, text);
        let doc = ws.document(id).expect("doc");
        assert!(doc.highlights(0..1).is_empty(), "{file}");
        assert_eq!(doc.syntax_tree(), None, "{file}");
    }
    assert_eq!(syntax_tree_of("plaintext", "x"), None);
}

#[test]
fn typing_undo_and_redo_update_the_highlights() {
    let (mut ws, id, _) = open("typing", "main.rs", "fn f() {}\nlet x = 1;\n");
    let before = tokens(&ws, id, 1..2);
    ws.set_cursor(id, 10).expect("cursor");
    ws.type_text(id, "/").expect("type");
    ws.type_text(id, "/").expect("type");
    assert_eq!(
        tokens(&ws, id, 1..2),
        owned(&[(1, "//let x = 1;", TokenKind::Comment)])
    );
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(tokens(&ws, id, 1..2), before);
    assert!(ws.redo(id).expect("redo"));
    assert_eq!(tokens(&ws, id, 1..2)[0].2, TokenKind::Comment);
}

/// A xorshift generator, so every run makes the same edits.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % (bound.max(1) as u64)).expect("fits")
    }
}

const PIECES: &[&str] = &[
    "x",
    " ",
    "\n",
    "\r\n",
    "{",
    "}",
    "(",
    ")",
    ";",
    "\"",
    "'",
    "/*",
    "*/",
    "//",
    "é",
    "日",
    "fn g() {}",
    "let y = \"s\\n\";",
    "match v { _ => 1 }",
    "#[test]",
    "r#\"raw\"#",
];

/// One random command: typing, deleting, replacing, several cursors, undo,
/// redo or a line-ending change.
fn random_command(ws: &mut Workspace, id: DocumentId, rng: &mut Seeded) {
    let len = ws.document(id).expect("doc").len_chars();
    let piece = PIECES[rng.below(PIECES.len())];
    match rng.below(9) {
        0 => ws.set_cursor(id, rng.below(len + 1)).expect("cursor"),
        1 | 2 => ws.type_text(id, piece).expect("type"),
        3 => ws.delete_left(id).expect("delete left"),
        4 => ws.delete_right(id).expect("delete right"),
        5 => {
            let start = rng.below(len + 1);
            let end = (start + rng.below(12)).min(len);
            ws.replace(id, Range::new(start, end), piece)
                .expect("replace");
        }
        6 => {
            let cursors: Vec<Selection> = (0..3)
                .map(|_| Selection::cursor(rng.below(len + 1)))
                .collect();
            ws.set_selections(id, cursors, 0).expect("cursors");
            ws.type_text(id, piece).expect("type at each cursor");
        }
        7 => {
            if rng.below(2) == 0 {
                ws.undo(id).expect("undo");
            } else {
                ws.redo(id).expect("redo");
            }
        }
        _ => {
            let eol = if rng.below(2) == 0 {
                Eol::Lf
            } else {
                Eol::Crlf
            };
            ws.set_eol(id, eol).expect("eol");
        }
    }
}

#[test]
fn after_random_edits_undo_and_redo_the_tree_equals_a_fresh_parse() {
    let (mut ws, id, _) = open("random", "main.rs", SAMPLE);
    let mut rng = Seeded(0x2545_F491_4F6C_DD1D);
    for step in 0..400 {
        random_command(&mut ws, id, &mut rng);
        let doc = ws.document(id).expect("doc");
        let fresh = syntax_tree_of("rust", &doc.text());
        assert_eq!(doc.syntax_tree(), fresh, "step {step}: {:?}", doc.text());
    }
    while ws.undo(id).expect("undo") {}
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.text(), SAMPLE);
    assert_eq!(doc.syntax_tree(), syntax_tree_of("rust", SAMPLE));
    let lines = doc.line_count();
    let (fresh_ws, fresh_id, _) = open("random-fresh", "main.rs", SAMPLE);
    assert_eq!(
        tokens(&ws, id, 0..lines),
        tokens(&fresh_ws, fresh_id, 0..lines)
    );
}

#[test]
fn highlights_after_edits_match_a_freshly_opened_copy() {
    let (mut ws, id, _) = open("ranges", "main.rs", SAMPLE);
    let mut rng = Seeded(0x9E37_79B9_7F4A_7C15);
    for round in 0..8 {
        for _ in 0..25 {
            random_command(&mut ws, id, &mut rng);
        }
        let text = ws.document(id).expect("doc").text();
        let (fresh_ws, fresh_id, _) = open(&format!("ranges-{round}"), "main.rs", &text);
        let lines = ws.document(id).expect("doc").line_count();
        assert_eq!(
            tokens(&ws, id, 0..lines),
            tokens(&fresh_ws, fresh_id, 0..lines),
            "round {round}: {text:?}"
        );
    }
}

#[test]
fn a_reload_from_disk_updates_the_tree() {
    let (mut ws, id, path) = open("reload", "main.rs", SAMPLE);
    let changed = SAMPLE.replace("// Adds.", "/* Adds */ const N: u8 = 2;");
    std::fs::write(&path, &changed).expect("write");
    ws.check_disk(id).expect("check");
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.text(), changed);
    assert_eq!(doc.syntax_tree(), syntax_tree_of("rust", &changed));
    assert_eq!(
        tokens(&ws, id, 0..1)[0],
        (0, "/* Adds */".to_owned(), TokenKind::Comment)
    );
}

#[test]
fn files_too_large_for_tokenization_are_never_highlighted() {
    // VS Code's limit is 300,000 lines (`textModel.ts:190`); this has one more.
    let text = format!("fn f() {{}}\n{}", "\n".repeat(300_000));
    let (mut ws, id, _) = open("large", "big.rs", &text);
    let doc = ws.document(id).expect("doc");
    assert_eq!(doc.line_count(), 300_002);
    assert_eq!(doc.syntax_tree(), None);
    assert!(doc.highlights(0..1).is_empty(), "too large when opened");
    // The decision is made once: shrinking the file does not change it.
    let end = ws.document(id).expect("doc").len_chars();
    ws.replace(id, Range::new(11, end), "").expect("replace");
    let doc = ws.document(id).expect("doc");
    assert!(doc.highlights(0..1).is_empty(), "still never highlighted");
    assert_eq!(doc.offset_of(Position::new(0, 2)), 2);
}
