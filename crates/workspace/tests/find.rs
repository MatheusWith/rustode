//! Workspace seam: find and replace in the current file, with the regex,
//! match case and whole word options of VS Code's find widget.
//!
//! Many cases mirror `src/vs/editor/contrib/find/test/browser/findModel.test.ts`
//! in the Source tree: the same text (`:24-37`) and the expected ranges
//! written as VS Code writes them, `[line, column, line, column]`, 1-based.

use rustode_workspace::{
    DocumentId, FindOptions, FindQuery, Position, Range, Selection, Workspace,
};

/// The text of VS Code's find model tests (`findModel.test.ts:24-37`).
const FIXTURE: &str = "// my cool header\n\
                       #include \"cool.h\"\n\
                       #include <iostream>\n\
                       \n\
                       int main() {\n    \
                       cout << \"hello world, Hello!\" << endl;\n    \
                       cout << \"hello world again\" << endl;\n    \
                       cout << \"Hello world again\" << endl;\n    \
                       cout << \"helloworld again\" << endl;\n\
                       }\n\
                       // blablablaciao\n";

fn open(name: &str, text: &str) -> (Workspace, DocumentId) {
    let dir = std::env::temp_dir().join(format!("rustode-find-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id)
}

/// A VS Code range, `[line, column, line, column]` (1-based), as chars.
fn vs(ws: &Workspace, id: DocumentId, [l1, c1, l2, c2]: [usize; 4]) -> Range {
    let doc = ws.document(id).expect("document");
    Range::new(
        doc.offset_of(Position::new(l1 - 1, c1 - 1)),
        doc.offset_of(Position::new(l2 - 1, c2 - 1)),
    )
}

fn vs_all(ws: &Workspace, id: DocumentId, ranges: &[[usize; 4]]) -> Vec<Range> {
    ranges.iter().map(|r| vs(ws, id, *r)).collect()
}

fn plain(pattern: &str, match_case: bool, whole_word: bool) -> FindQuery {
    let options = FindOptions {
        regex: false,
        match_case,
        whole_word,
    };
    FindQuery::new(pattern, options).expect("plain text always compiles")
}

fn find_all(ws: &Workspace, id: DocumentId, query: &FindQuery) -> Vec<Range> {
    ws.find_all(id, query).expect("find all")
}

#[test]
fn find_all_follows_the_case_and_whole_word_options() {
    // findModel.test.ts:91-240, "incremental find from beginning of file".
    let (ws, id) = open("options", FIXTURE);
    let h = [
        [1, 12, 1, 13],
        [2, 16, 2, 17],
        [6, 14, 6, 15],
        [6, 27, 6, 28],
        [7, 14, 7, 15],
        [8, 14, 8, 15],
        [9, 14, 9, 15],
    ];
    assert_eq!(
        find_all(&ws, id, &plain("H", false, false)),
        vs_all(&ws, id, &h)
    );
    let hello = [
        [6, 14, 6, 19],
        [6, 27, 6, 32],
        [7, 14, 7, 19],
        [8, 14, 8, 19],
        [9, 14, 9, 19],
    ];
    assert_eq!(
        find_all(&ws, id, &plain("Hello", false, false)),
        vs_all(&ws, id, &hello)
    );
    assert_eq!(
        find_all(&ws, id, &plain("Hello", true, false)),
        vs_all(&ws, id, &[[6, 27, 6, 32], [8, 14, 8, 19]])
    );
    assert_eq!(
        find_all(&ws, id, &plain("hello", true, false)),
        vs_all(&ws, id, &[[6, 14, 6, 19], [7, 14, 7, 19], [9, 14, 9, 19]])
    );
    assert_eq!(
        find_all(&ws, id, &plain("hello", true, true)),
        vs_all(&ws, id, &[[6, 14, 6, 19], [7, 14, 7, 19]]),
        "helloworld is not a whole word"
    );
    assert_eq!(
        find_all(&ws, id, &plain("hello", false, true)),
        vs_all(&ws, id, &hello[..4])
    );
}

/// The text of VS Code's model search tests (`textModelSearch.test.ts:53-59`).
const REGULAR_TEXT: &str = "This is some foo - bar text which contains foo and bar - as in Barcelona.\n\
     Now it begins a word fooBar and now it is caps Foo-isn't this great?\n\
     And here's a dull line with nothing interesting in it\n\
     It is also interesting if it's part of a word like amazingFooBar\n\
     Again nothing interesting here";

fn query(pattern: &str, regex: bool, match_case: bool, whole_word: bool) -> FindQuery {
    let options = FindOptions {
        regex,
        match_case,
        whole_word,
    };
    FindQuery::new(pattern, options).expect("valid pattern")
}

/// Checks `find_all` on `text` with LF and with CRLF line endings, as VS
/// Code's `assertFindMatches` does (`textModelSearch.test.ts:41-51`).
fn assert_find(text: &str, query: &FindQuery, expected: &[[usize; 4]]) {
    for (eol, text) in [
        ("lf", text.to_owned()),
        ("crlf", text.replace('\n', "\r\n")),
    ] {
        let name = format!("{eol}-{:x}", fingerprint(&text, query.pattern()));
        let (ws, id) = open(&name, &text);
        assert_eq!(
            find_all(&ws, id, query),
            vs_all(&ws, id, expected),
            "{:?} in {eol} text",
            query.pattern()
        );
    }
}

/// A short name for a fixture directory.
fn fingerprint(text: &str, pattern: &str) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one((text, pattern))
}

#[test]
fn regexes_match_each_line_on_its_own_like_vs_code() {
    // textModelSearch.test.ts:115-172 and :272-324.
    let anchors = [
        [1, 1, 1, 1],
        [2, 1, 2, 1],
        [3, 1, 3, 1],
        [4, 1, 4, 1],
        [5, 1, 5, 1],
    ];
    assert_find(REGULAR_TEXT, &query("^", true, false, false), &anchors);
    let ends = [
        [1, 74, 1, 74],
        [2, 69, 2, 69],
        [3, 54, 3, 54],
        [4, 65, 4, 65],
        [5, 31, 5, 31],
    ];
    assert_find(REGULAR_TEXT, &query("$", true, false, false), &ends);
    let lines = [
        [1, 1, 1, 74],
        [2, 1, 2, 69],
        [3, 1, 3, 54],
        [4, 1, 4, 65],
        [5, 1, 5, 31],
    ];
    assert_find(REGULAR_TEXT, &query(".*", true, false, false), &lines);
    let gaps = "one\n\nthree\n\nfive";
    assert_find(
        gaps,
        &query("^$", true, false, false),
        &[[2, 1, 2, 1], [4, 1, 4, 1]],
    );
    assert_find(
        gaps,
        &query("^.*$", true, false, false),
        &[
            [1, 1, 1, 4],
            [2, 1, 2, 1],
            [3, 1, 3, 6],
            [4, 1, 4, 1],
            [5, 1, 5, 5],
        ],
    );
    assert_find(
        "a if b\na\nab\neb",
        &query("^a.*b$", true, false, false),
        &[[1, 1, 1, 7], [3, 1, 3, 3]],
    );
}

#[test]
fn plain_text_regex_and_unicode_escapes_find_the_same_matches() {
    // textModelSearch.test.ts:77-113 and :728-740.
    let foo = [
        [1, 14, 1, 17],
        [1, 44, 1, 47],
        [2, 22, 2, 25],
        [2, 48, 2, 51],
        [4, 59, 4, 62],
    ];
    assert_find(REGULAR_TEXT, &query("foo", false, false, false), &foo);
    assert_find(
        REGULAR_TEXT,
        &query(r"\u{0066}\u006f\u006F", true, false, false),
        &foo,
    );
    assert_find(REGULAR_TEXT, &query("foo", false, true, false), &foo[..3]);
    assert_find(
        REGULAR_TEXT,
        &query("foo", false, false, true),
        &[[1, 14, 1, 17], [1, 44, 1, 47], [2, 48, 2, 51]],
    );
    assert_find(REGULAR_TEXT, &query("f.o", false, false, false), &[]);
}

#[test]
fn whole_word_uses_vs_codes_word_separators() {
    // textModelSearch.test.ts:343-385: non-Latin letters, and matches that
    // start or end with a separator.
    assert_find(
        "я\nкомпилятор\nобфускация\n:я-я",
        &query("я", false, false, true),
        &[[1, 1, 1, 2], [4, 2, 4, 3], [4, 4, 4, 5]],
    );
    assert_find(
        "this._register(this._textAreaInput.onKeyDown((e: IKeyboardEvent) => {\n\
         \tthis._viewController.emitKeyDown(e);\n}));",
        &query("((e: ", false, false, true),
        &[[1, 45, 1, 50]],
    );
    assert_find(
        "this.server.listen(0);",
        &query("listen(", false, false, true),
        &[[1, 13, 1, 20]],
    );
}

#[test]
fn an_empty_match_moves_the_search_on_by_one_char() {
    // textModelSearch.test.ts:780-797: unlike the regex crate's own
    // iterator, VS Code keeps the empty match right after "10".
    assert_find(
        "10.243.30.10",
        &query(r"\d*", true, false, false),
        &[
            [1, 1, 1, 3],
            [1, 3, 1, 3],
            [1, 4, 1, 7],
            [1, 7, 1, 7],
            [1, 8, 1, 10],
            [1, 10, 1, 10],
            [1, 11, 1, 13],
        ],
    );
    // textModelSearch.test.ts:799-823: never inside an emoji (columns here
    // count chars; VS Code's count UTF-16 units).
    assert_find(
        "1\u{1F4BB}1",
        &query("()", true, false, false),
        &[[1, 1, 1, 1], [1, 2, 1, 2], [1, 3, 1, 3], [1, 4, 1, 4]],
    );
}

#[test]
fn an_empty_pattern_finds_nothing_and_a_bad_regex_is_an_error() {
    // textModelSearch.test.ts:612-615; findWidget.ts:1005-1016.
    let (ws, id) = open("empty", FIXTURE);
    for options in [
        FindOptions::default(),
        FindOptions {
            regex: true,
            ..FindOptions::default()
        },
    ] {
        let empty = FindQuery::new("", options).expect("an empty pattern is valid");
        assert_eq!(find_all(&ws, id, &empty), []);
    }
    let regex = FindOptions {
        regex: true,
        ..FindOptions::default()
    };
    let error = FindQuery::new("(", regex).expect_err("unclosed group");
    assert_eq!(
        error.to_string(),
        "invalid regular expression: unclosed group"
    );
    assert!(
        FindQuery::new("(", FindOptions::default()).is_ok(),
        "plain text"
    );
    // VS Code's JavaScript regexes accept these; the regex crate refuses
    // them, so they are reported instead of searched.
    for (pattern, reason) in [
        (
            "a(?=b)",
            "look-around, including look-ahead and look-behind, is not supported",
        ),
        (r"(a)\1", "backreferences are not supported"),
    ] {
        let error = FindQuery::new(pattern, regex).expect_err(pattern);
        assert_eq!(
            error.to_string(),
            format!("invalid regular expression: {reason}")
        );
    }
}

#[test]
fn a_line_break_in_the_pattern_searches_across_lines() {
    // textModelSearch.test.ts:174-252, :288-304 and :326-341.
    let text = "Just some text text\nJust some text text\nsome text again\nagain some text";
    let regex = |p| query(p, true, false, false);
    assert_find(text, &regex(r"text\n"), &[[1, 16, 2, 1], [2, 16, 3, 1]]);
    assert_find(text, &regex(r"text\nJust"), &[[1, 16, 2, 5]]);
    assert_find(text, &regex(r"\nagain"), &[[3, 16, 4, 6]]);
    assert_find(text, &regex(r".*\nJust.*\n"), &[[1, 1, 3, 1]]);
    let blocks = "if\nelse\n\nif\nelse";
    assert_find(blocks, &regex(r"^if\nelse"), &[[1, 1, 2, 5], [4, 1, 5, 5]]);
    assert_find(
        "if\n\nelse\n  \nif\n \nelse",
        &regex(r"^\s*$\n"),
        &[[2, 1, 3, 1], [4, 1, 5, 1], [6, 1, 7, 1]],
    );
    assert_find(
        "if\nelse\n\nif\nelseif\nelse",
        &regex(r"if\nelse$"),
        &[[1, 1, 2, 5], [5, 5, 6, 5]],
    );
    assert_find(
        "Just some text text\nsome text text\nsome text again\nagain some text\nbut not some",
        &query("text\nsome", false, false, false),
        &[[1, 16, 2, 5], [2, 11, 3, 5]],
    );
}

#[test]
fn a_crlf_is_one_lf_to_a_multiline_regex_and_cr_is_never_found() {
    // textModelSearch.test.ts:559-610.
    let (ws, id) = open("crlf", "a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni");
    let regex = |p| query(p, true, false, false);
    assert_eq!(
        find_all(&ws, id, &regex(r"h\n")),
        vs_all(&ws, id, &[[8, 1, 9, 1]])
    );
    assert_eq!(
        find_all(&ws, id, &regex(r"g\nh\n")),
        vs_all(&ws, id, &[[7, 1, 9, 1]])
    );
    assert_eq!(
        find_all(&ws, id, &regex(r"\ni")),
        vs_all(&ws, id, &[[8, 2, 9, 2]])
    );
    assert_eq!(find_all(&ws, id, &regex(r"\r\n")), []);
    assert_eq!(find_all(&ws, id, &regex(r"\r")), []);
}

#[test]
fn non_word_escapes_search_across_lines_and_whitespace_escapes_do_not() {
    // textModelSearch.test.ts:639-726 (issue #53415) and the multiline
    // heuristic of :69-99 (`\W`, `\n`, `\r` cross lines, `\s` does not).
    let regex = |p| query(p, true, false, false);
    assert_find(
        "text\n180702-\n180703-180704",
        &regex(r"\d{6}-\W"),
        &[[2, 1, 3, 1]],
    );
    assert_find(
        "Just some text\n\nJust",
        &regex(r"\W"),
        &[[1, 5, 1, 6], [1, 10, 1, 11], [1, 15, 2, 1], [2, 1, 3, 1]],
    );
    assert_find(
        "Just some text\n\tJust\nJust",
        &regex(r"\W"),
        &[
            [1, 5, 1, 6],
            [1, 10, 1, 11],
            [1, 15, 2, 1],
            [2, 1, 2, 2],
            [2, 6, 3, 1],
        ],
    );
    assert_find(
        "Just  some text\n\nJust",
        &regex(r"\W{2}"),
        &[[1, 5, 1, 7], [1, 16, 3, 1]],
    );
    assert_find("some text\nnext", &regex(r"text\s"), &[]);
}

fn selections(ws: &Workspace, id: DocumentId) -> Vec<Selection> {
    ws.document(id).expect("document").selections().to_vec()
}

/// A VS Code range as the selection VS Code makes of it: anchored at the
/// start, with the cursor at the end.
fn selected(ws: &Workspace, id: DocumentId, range: [usize; 4]) -> Vec<Selection> {
    let range = vs(ws, id, range);
    vec![Selection::new(range.start, range.end)]
}

/// Runs `step` once per expected range; each time, the selection must be
/// that range, and `step` must have returned it.
fn assert_steps(
    ws: &mut Workspace,
    id: DocumentId,
    mut step: impl FnMut(&mut Workspace) -> Option<Range>,
    expected: &[[usize; 4]],
) {
    for (n, &range) in expected.iter().enumerate() {
        let found = step(ws);
        assert_eq!(found, Some(vs(ws, id, range)), "step {n}");
        assert_eq!(selections(ws, id), selected(ws, id, range), "step {n}");
    }
}

#[test]
fn find_next_selects_each_match_in_turn_and_wraps_around() {
    // findModel.test.ts:356-440.
    let (mut ws, id) = open("next", FIXTURE);
    let hello = plain("hello", false, true);
    let next = |ws: &mut Workspace| ws.find_next(id, &hello).expect("find next");
    let order = [
        [6, 14, 6, 19],
        [6, 27, 6, 32],
        [7, 14, 7, 19],
        [8, 14, 8, 19],
        [6, 14, 6, 19],
    ];
    assert_steps(&mut ws, id, next, &order);
}

#[test]
fn find_previous_goes_back_and_wraps_to_the_last_match() {
    // findModel.test.ts:619-704.
    let (mut ws, id) = open("previous", FIXTURE);
    let hello = plain("hello", false, true);
    let previous = |ws: &mut Workspace| ws.find_previous(id, &hello).expect("find previous");
    let order = [
        [8, 14, 8, 19],
        [7, 14, 7, 19],
        [6, 27, 6, 32],
        [6, 14, 6, 19],
        [8, 14, 8, 19],
    ];
    assert_steps(&mut ws, id, previous, &order);
}

#[test]
fn find_next_and_previous_start_from_the_primary_selection() {
    // findModel.test.ts:789-836: from (6, 20), just after the first hello.
    let (mut ws, id) = open("from-cursor", FIXTURE);
    let hello = plain("hello", false, true);
    let cursor = vs(&ws, id, [6, 20, 6, 20]).start;
    ws.set_cursor(id, cursor).expect("cursor");
    assert_eq!(
        ws.find_next(id, &hello).expect("next"),
        Some(vs(&ws, id, [6, 27, 6, 32]))
    );
    ws.set_cursor(id, cursor).expect("cursor");
    assert_eq!(
        ws.find_previous(id, &hello).expect("previous"),
        Some(vs(&ws, id, [6, 14, 6, 19]))
    );
    let two_cursors = vec![Selection::cursor(0), Selection::cursor(cursor)];
    ws.set_selections(id, two_cursors, 1).expect("select");
    ws.find_next(id, &hello).expect("next");
    assert_eq!(
        selections(&ws, id),
        selected(&ws, id, [6, 27, 6, 32]),
        "one selection, from the primary cursor"
    );
}

#[test]
fn without_a_match_the_selection_stays() {
    // findModel.test.ts:757-787.
    let (mut ws, id) = open("no-match", FIXTURE);
    let helloo = plain("helloo", false, true);
    assert_eq!(ws.find_next(id, &helloo).expect("next"), None);
    assert_eq!(ws.find_previous(id, &helloo).expect("previous"), None);
    assert_eq!(selections(&ws, id), [Selection::cursor(0)]);
}

#[test]
fn empty_matches_at_the_cursor_are_stepped_over() {
    // findModel.test.ts:838-907 (^), :909-999 ($), :1001-1051 (^$),
    // :1082-1151 and :1153-1222 (^.*$), :1224-1274 (^$ backwards).
    let regex = |p| query(p, true, false, false);
    for (n, (pattern, forward, expected)) in [
        ("^", true, vec![[2, 1, 2, 1], [3, 1, 3, 1]]),
        (
            "$",
            true,
            vec![[1, 18, 1, 18], [2, 18, 2, 18], [3, 20, 3, 20]],
        ),
        ("^$", true, vec![[4, 1, 4, 1], [12, 1, 12, 1], [4, 1, 4, 1]]),
        ("^.*$", true, vec![[1, 1, 1, 18], [2, 1, 2, 18]]),
        ("^.*$", false, vec![[12, 1, 12, 1], [11, 1, 11, 17]]),
        (
            "^$",
            false,
            vec![[12, 1, 12, 1], [4, 1, 4, 1], [12, 1, 12, 1]],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (mut ws, id) = open(&format!("anchors-{n}"), FIXTURE);
        let q = regex(pattern);
        let step = |ws: &mut Workspace| {
            if forward {
                ws.find_next(id, &q).expect("next")
            } else {
                ws.find_previous(id, &q).expect("previous")
            }
        };
        assert_steps(&mut ws, id, step, &expected);
    }
}

#[test]
fn multiline_matches_are_found_in_turn_too() {
    let (mut ws, id) = open("next-multiline", "a\r\nb\r\na\r\nb");
    let q = query(r"a\nb", true, false, false);
    let next = |ws: &mut Workspace| ws.find_next(id, &q).expect("next");
    assert_steps(
        &mut ws,
        id,
        next,
        &[[1, 1, 2, 2], [3, 1, 4, 2], [1, 1, 2, 2]],
    );
    let previous = |ws: &mut Workspace| ws.find_previous(id, &q).expect("previous");
    assert_steps(&mut ws, id, previous, &[[3, 1, 4, 2], [1, 1, 2, 2]]);
}

fn line(ws: &Workspace, id: DocumentId, number: usize) -> String {
    ws.document(id).expect("document").line(number - 1)
}

fn cursor_at(ws: &Workspace, id: DocumentId, line: usize, column: usize) -> Vec<Selection> {
    vec![Selection::cursor(
        vs(ws, id, [line, column, line, column]).start,
    )]
}

#[test]
fn replace_selects_a_match_first_then_replaces_it_and_selects_the_next() {
    // findModel.test.ts:1276-1371.
    let (mut ws, id) = open("replace-one", FIXTURE);
    let hello = plain("hello", false, true);
    let cursor = vs(&ws, id, [6, 20, 6, 20]).start;
    ws.set_cursor(id, cursor).expect("cursor");
    assert!(
        !ws.replace_one(id, &hello, "hi").expect("replace"),
        "only selects"
    );
    assert_eq!(selections(&ws, id), selected(&ws, id, [6, 27, 6, 32]));
    assert_eq!(
        line(&ws, id, 6),
        "    cout << \"hello world, Hello!\" << endl;"
    );
    for (n, (changed_line, text, next)) in [
        (
            6,
            "    cout << \"hello world, hi!\" << endl;",
            [7, 14, 7, 19],
        ),
        (7, "    cout << \"hi world again\" << endl;", [8, 14, 8, 19]),
        (8, "    cout << \"hi world again\" << endl;", [6, 14, 6, 19]),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            ws.replace_one(id, &hello, "hi").expect("replace"),
            "step {n}"
        );
        assert_eq!(line(&ws, id, changed_line), text, "step {n}");
        assert_eq!(selections(&ws, id), selected(&ws, id, next), "step {n}");
    }
    assert!(ws.replace_one(id, &hello, "hi").expect("replace"));
    assert_eq!(line(&ws, id, 6), "    cout << \"hi world, hi!\" << endl;");
    assert_eq!(
        selections(&ws, id),
        cursor_at(&ws, id, 6, 16),
        "no match left: the cursor stays after the replacement"
    );
    assert!(!ws.replace_one(id, &hello, "hi").expect("replace"));
}

#[test]
fn replace_moves_along_adjacent_matches_and_undoes_one_at_a_time() {
    // findModel.test.ts:1372-1435.
    let (mut ws, id) = open("replace-bla", FIXTURE);
    let bla = plain("bla", false, false);
    assert!(!ws.replace_one(id, &bla, "ciao").expect("replace"));
    assert_eq!(selections(&ws, id), selected(&ws, id, [11, 4, 11, 7]));
    for (text, next) in [
        ("// ciaoblablaciao", [11, 8, 11, 11]),
        ("// ciaociaoblaciao", [11, 12, 11, 15]),
    ] {
        assert!(ws.replace_one(id, &bla, "ciao").expect("replace"));
        assert_eq!(line(&ws, id, 11), text);
        assert_eq!(selections(&ws, id), selected(&ws, id, next));
    }
    assert!(ws.replace_one(id, &bla, "ciao").expect("replace"));
    assert_eq!(line(&ws, id, 11), "// ciaociaociaociao");
    assert_eq!(selections(&ws, id), cursor_at(&ws, id, 11, 16));
    ws.undo(id).expect("undo");
    assert_eq!(
        line(&ws, id, 11),
        "// ciaociaoblaciao",
        "each replace is one undo step"
    );
    assert_eq!(selections(&ws, id), selected(&ws, id, [11, 12, 11, 15]));
}

fn text(ws: &Workspace, id: DocumentId) -> String {
    ws.document(id).expect("document").text()
}

#[test]
fn replace_all_replaces_every_match_and_keeps_the_cursor_in_place() {
    // findModel.test.ts:1437-1483: the cursor after the first hello moves
    // left with the text before it.
    let (mut ws, id) = open("replace-all", FIXTURE);
    let hello = plain("hello", false, true);
    ws.set_cursor(id, vs(&ws, id, [6, 20, 6, 20]).start)
        .expect("cursor");
    assert_eq!(ws.replace_all(id, &hello, "hi").expect("replace all"), 4);
    assert_eq!(line(&ws, id, 6), "    cout << \"hi world, hi!\" << endl;");
    assert_eq!(line(&ws, id, 7), "    cout << \"hi world again\" << endl;");
    assert_eq!(line(&ws, id, 8), "    cout << \"hi world again\" << endl;");
    assert_eq!(
        line(&ws, id, 9),
        "    cout << \"helloworld again\" << endl;"
    );
    assert_eq!(selections(&ws, id), cursor_at(&ws, id, 6, 17));
    assert_eq!(find_all(&ws, id, &hello), []);
}

#[test]
fn replace_all_handles_adjacent_and_empty_replacements() {
    // findModel.test.ts:1485-1554 and :2111-2141.
    let (mut ws, id) = open("replace-all-adjacent", FIXTURE);
    assert_eq!(
        ws.replace_all(id, &plain("  ", false, false), " ")
            .expect("all"),
        8
    );
    assert_eq!(
        line(&ws, id, 6),
        "  cout << \"hello world, Hello!\" << endl;"
    );
    assert_eq!(line(&ws, id, 9), "  cout << \"helloworld again\" << endl;");
    assert_eq!(
        ws.replace_all(id, &plain("bla", false, false), "ciao")
            .expect("all"),
        3
    );
    assert_eq!(line(&ws, id, 11), "// ciaociaociaociao");
    assert_eq!(
        ws.replace_all(id, &plain("hello", false, true), "")
            .expect("all"),
        4
    );
    assert_eq!(line(&ws, id, 6), "  cout << \" world, !\" << endl;");
    assert_eq!(selections(&ws, id), [Selection::cursor(0)]);
    assert_eq!(
        ws.replace_all(id, &plain("nothing", false, false), "x")
            .expect("all"),
        0
    );
}

#[test]
fn replace_all_undoes_in_one_step_with_the_selections_from_before() {
    let (mut ws, id) = open("replace-all-undo", FIXTURE);
    let before = vec![
        Selection::new(3, 7),
        Selection::cursor(40),
        Selection::new(120, 100),
    ];
    ws.set_selections(id, before.clone(), 1).expect("select");
    let _ = ws.take_events();
    assert_eq!(
        ws.replace_all(id, &plain("o", false, false), "0")
            .expect("all"),
        19
    );
    assert_eq!(
        ws.take_events(),
        [rustode_workspace::Event::DocumentChanged { id }]
    );
    assert!(!text(&ws, id).contains('o'));
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(text(&ws, id), FIXTURE);
    assert_eq!(selections(&ws, id), before);
    assert!(!ws.document(id).expect("document").is_dirty());
    assert!(!ws.undo(id).expect("undo"), "replace all was one step");
    assert!(ws.redo(id).expect("redo"));
    assert!(!text(&ws, id).contains('o'));
}

/// `line0` to `line1099`, one per line, each after `prefix`.
fn numbered_lines(prefix: &str) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for i in 0..1100 {
        writeln!(text, "{prefix}line{i}").expect("write to a string");
    }
    text
}

#[test]
fn replace_all_with_an_anchor_inserts_at_every_line() {
    // findModel.test.ts:2143-2164: `^` on more than 1000 lines.
    let (mut ws, id) = open("replace-all-anchor", &numbered_lines(""));
    let caret = query("^", true, false, false);
    assert_eq!(ws.replace_all(id, &caret, "a ").expect("all"), 1101);
    assert_eq!(text(&ws, id), format!("{}a ", numbered_lines("a ")));
}

#[test]
fn replacements_take_the_documents_line_ending() {
    // VS Code's buffer normalizes inserted line breaks
    // (pieceTreeTextBuffer.ts:276-287).
    let (mut ws, id) = open("replace-all-crlf", "a,b\r\nc");
    assert_eq!(
        ws.replace_all(id, &plain(",", false, false), "\n")
            .expect("all"),
        1
    );
    assert_eq!(text(&ws, id), "a\r\nb\r\nc");
}

#[test]
fn replace_all_moves_the_selection_like_vs_codes_tracked_ranges() {
    // A selection never grows at its edges; a cursor sticks to the char
    // before it at a line's end and to the char after it elsewhere
    // (cursor.ts:940-961); each end follows an edit as nodeAcceptEdit
    // moves it (intervalTree.ts:396-490).
    let text = "say hello, hello\nhello";
    let hello = plain("hello", false, false);
    for (n, (before, replacement, after)) in [
        (Selection::new(4, 9), "howdy!!", Selection::new(4, 9)),
        (Selection::new(4, 9), "hi", Selection::new(4, 6)),
        (Selection::new(9, 4), "hi", Selection::new(6, 4)),
        (Selection::cursor(6), "hi", Selection::cursor(6)),
        (Selection::cursor(6), "", Selection::cursor(4)),
        (Selection::cursor(9), "howdy!!", Selection::cursor(11)),
        (Selection::cursor(22), "howdy!!", Selection::cursor(26)),
    ]
    .into_iter()
    .enumerate()
    {
        let (mut ws, id) = open(&format!("track-{n}"), text);
        ws.set_selections(id, vec![before], 0).expect("select");
        assert_eq!(
            ws.replace_all(id, &hello, replacement).expect("all"),
            3,
            "case {n}"
        );
        assert_eq!(selections(&ws, id), [after], "case {n}");
    }
}

/// The text after a Replace All of `pattern` (a regex, matching case) by
/// `replacement` in `text`.
fn replaced(text: &str, pattern: &str, replacement: &str) -> String {
    let name = format!(
        "pattern-{:x}",
        fingerprint(text, &format!("{pattern}/{replacement}"))
    );
    let (mut ws, id) = open(&name, text);
    ws.replace_all(id, &query(pattern, true, true, false), replacement)
        .expect("replace all");
    ws.document(id).expect("document").text()
}

#[test]
fn regex_replacements_insert_groups_like_vs_code() {
    // replacePattern.test.ts:121-154 and findModel.test.ts:2166-2195.
    let import = r#"let\s+(\w+)\s*=\s*require\s*\(\s*['"]([\w\.\-/]+)\s*['"]\s*\)\s*"#;
    let source = "let something = require('fs')";
    let cases = [
        (
            r"import * as $1 from '$2';",
            "import * as something from 'fs';",
        ),
        (
            r"import * as $2 from '$1';",
            "import * as fs from 'something';",
        ),
        (r"[$0]", "[let something = require('fs')]"),
        (r"[$&]", "[let something = require('fs')]"),
    ];
    for (replacement, expected) in cases {
        assert_eq!(
            replaced(source, import, replacement),
            expected,
            "{replacement}"
        );
    }
    assert_eq!(replaced("for ()", "for(.*)", "cat$1"), "cat ()");
    assert_eq!(
        replaced("abcd", "a(z)?", "a{$1}"),
        "a{}bcd",
        "a group that took no part"
    );
    assert_eq!(replaced("hello", "hello(z)?", "hi$1"), "hi");
}

#[test]
fn group_numbers_fall_back_like_javascript_replace() {
    // replacePattern.test.ts:100-119: `$nn` is a group only if it exists.
    assert_eq!(replaced("hi", "(hi)", "hello$10"), "hellohi0");
    assert_eq!(
        replaced("hi", "(hi)()()()()()()()()()", "hello$10"),
        "hello"
    );
    assert_eq!(replaced("hi", "(hi)", "hello$100"), "hellohi00");
    assert_eq!(replaced("hi", "(hi)", "hello$20"), "hello$20");
    assert_eq!(replaced("hi", "hi", "hello$01"), "hellohi1");
    assert_eq!(replaced("hi", "hi", "hello$&1"), "hellohi1");
}

#[test]
fn replace_strings_escape_like_vs_code() {
    // replacePattern.test.ts:17-73 (parse) and findModel.test.ts:1556-1586.
    for (replacement, expected) in [
        (r"\thello", "\thello"),
        (r"\nhello", "\nhello"),
        (r"\\thello", r"\thello"),
        (r"\\\thello", "\\\thello"),
        (r"\\\\thello", r"\\thello"),
        (r"hello\", r"hello\"),
        (r"hello\x", r"hello\x"),
        (r"hello\0", r"hello\0"),
        (r"hello$$", "hello$"),
        (r"hello$$0", "hello$0"),
        ("hello$`", "hello$`"),
        ("hello$'", "hello$'"),
        ("hello$", "hello$"),
        (r"$\n", r"$\n"),
    ] {
        assert_eq!(replaced("x", "x", replacement), expected, "{replacement}");
    }
    assert_eq!(
        replaced("// blablablaciao\n", "bla", r"<\n\t>"),
        "// <\n\t><\n\t><\n\t>ciao\n"
    );
}

#[test]
fn case_escapes_change_the_next_group() {
    // replacePattern.test.ts:76-99.
    let func = r"func (\w+)\(";
    assert_eq!(
        replaced("func privateFunc(", func, r"func \U$1("),
        "func PRIVATEFUNC("
    );
    assert_eq!(
        replaced("func privateFunc(", func, r"func \u$1("),
        "func PrivateFunc("
    );
    assert_eq!(
        replaced("func privateFunc(", func, r"func \L$1("),
        "func privatefunc("
    );
    assert_eq!(
        replaced("func PrivateFunc(", func, r"func \l$1("),
        "func privateFunc("
    );
    assert_eq!(
        replaced("hellogooDbye", r"hello(\w+)", r"hello\u\u\l\l\U$1"),
        "helloGOodBYE"
    );
    assert_eq!(
        replaced("ab", "(a)(b)", r"\u$1-$2"),
        "A-b",
        "only the next group"
    );
    assert_eq!(
        replaced("ab", "(a)(b)", r"\u-$2"),
        "-B",
        "even past other text"
    );
    assert_eq!(
        replaced("straße", "stra(ße)", r"\U$1"),
        "SSE",
        "full case mapping"
    );
}

#[test]
fn plain_text_queries_replace_literally() {
    // findModel.ts:474-479: only regex queries parse the replace string.
    let (mut ws, id) = open("literal-replace", "a.b");
    ws.replace_all(id, &plain(".", false, false), r"$0\n")
        .expect("all");
    assert_eq!(text(&ws, id), r"a$0\nb");
}

#[test]
fn find_all_stops_at_vs_codes_limit_but_replace_all_does_not() {
    // findModel.ts:81 (MATCHES_LIMIT) and :572-584 (Replace All takes
    // every match).
    let lines = rustode_workspace::MATCHES_LIMIT + 51;
    let (mut ws, id) = open("limit", &"x\n".repeat(lines));
    let x = plain("x", false, false);
    let found = find_all(&ws, id, &x);
    assert_eq!(found.len(), 19_999);
    assert_eq!(found.last(), Some(&Range::new(39_996, 39_997)));
    let last = Range::new(2 * lines - 2, 2 * lines - 1);
    ws.set_cursor(id, last.start).expect("cursor");
    assert_eq!(
        ws.find_next(id, &x).expect("next"),
        Some(last),
        "past the limit too"
    );
    assert_eq!(ws.replace_all(id, &x, "yy").expect("all"), lines);
    assert_eq!(text(&ws, id), "yy\n".repeat(lines));
}

/// Replace All, checking that one undo brings back the text and the
/// selections from before, and that redo replaces again. Replacing empty
/// matches with nothing changes nothing and records no undo step.
fn replace_all_undoes_in_one_step(ws: &mut Workspace, id: DocumentId, q: &FindQuery, by: &str) {
    let (before, before_selections) = (text(ws, id), selections(ws, id));
    let _ = ws.take_events();
    ws.replace_all(id, q, by).expect("replace all");
    if ws.take_events().is_empty() {
        assert_eq!(text(ws, id), before, "{:?} -> {by:?}", q.pattern());
        assert_eq!(selections(ws, id), before_selections);
        return;
    }
    let (after, after_selections) = (text(ws, id), selections(ws, id));
    assert!(ws.undo(id).expect("undo"));
    assert_eq!(text(ws, id), before, "{:?} -> {by:?}", q.pattern());
    assert_eq!(selections(ws, id), before_selections, "{:?}", q.pattern());
    assert!(ws.redo(id).expect("redo"));
    assert_eq!(text(ws, id), after);
    assert_eq!(selections(ws, id), after_selections);
}

#[test]
fn any_find_and_replace_sequence_followed_by_undo_restores_the_original() {
    let original = "fn main() {\r\n    let olá = \"a.b\"; // ab\n\tab 日本 é\r\n}\n\nab";
    let patterns = [
        ("a", false),
        ("ab", false),
        (".", false),
        ("\n", false),
        ("^", true),
        ("$", true),
        (r"\w+", true),
        (r"(\w)(\w)", true),
        (r"\s*", true),
        (r"b\n", true),
        (r"\W", true),
        ("[áé日]", true),
    ];
    let replacements = [
        "", "x", "$0$0", r"\u$1", "$2$1", r"\n", "日\r\n", r"<\t>", "$",
    ];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % (bound as u64)).expect("fits")
    };
    for round in 0..60 {
        let (mut ws, id) = open(&format!("prop{round}"), original);
        for _ in 0..16 {
            let (pattern, regex) = patterns[next(patterns.len())];
            let options = FindOptions {
                regex,
                match_case: next(2) == 0,
                whole_word: next(3) == 0,
            };
            let q = FindQuery::new(pattern, options).expect("valid");
            let by = replacements[next(replacements.len())];
            let len = ws.document(id).expect("document").len_chars();
            match next(5) {
                0 => ws.set_cursor(id, next(len + 1)).expect("cursor"),
                1 => drop(ws.find_next(id, &q).expect("next")),
                2 => drop(ws.find_previous(id, &q).expect("previous")),
                3 => drop(ws.replace_one(id, &q, by).expect("replace")),
                _ => replace_all_undoes_in_one_step(&mut ws, id, &q, by),
            }
        }
        while ws.undo(id).expect("undo") {}
        assert_eq!(text(&ws, id), original, "round {round}");
        assert!(
            !ws.document(id).expect("document").is_dirty(),
            "round {round}"
        );
    }
}
