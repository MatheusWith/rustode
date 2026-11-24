//! Input to frame: comparing the file on disk with the unsaved text after a
//! save hit a conflict. The diff is the workspace's
//! (`crates/workspace/tests/conflict_compare.rs`); these tests check what the
//! terminal draws and which keys do what. Snapshots are in
//! `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to rewrite them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::App;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// A document that held `base`, now holding `mine` unsaved, whose file now
/// holds `disk`, with the conflict raised.
fn conflicted(
    name: &str,
    base: &str,
    mine: &str,
    disk: &str,
    size: (u16, u16),
) -> (App, Terminal<TestBackend>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-tui-cmp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, base).expect("fixture");
    let mut app = App::open(&path).expect("open");
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).expect("terminal");
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    );
    app.step(&mut terminal, &Event::Paste(mine.to_owned()))
        .expect("paste");
    std::fs::write(&path, disk).expect("external write");
    let id = app.document().expect("document");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app
        .workspace()
        .document(id)
        .is_some_and(rustode_workspace::Document::has_conflict)
    {
        assert!(Instant::now() < deadline, "no conflict raised");
        app.workspace_mut().poll_external_changes();
        std::thread::sleep(Duration::from_millis(20));
    }
    (app, terminal, path)
}

fn simple(name: &str) -> (App, Terminal<TestBackend>, PathBuf) {
    conflicted(name, "one\n", "mine one\n", "theirs\n", (100, 12))
}

fn press(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode, mods: KeyModifiers) {
    app.step(terminal, &Event::Key(KeyEvent::new(code, mods)))
        .expect("step");
}

fn key(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode) {
    press(app, terminal, code, KeyModifiers::NONE);
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut x = 0;
        while x < buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            out.push_str(symbol);
            x += u16::try_from(symbol.width().max(1)).unwrap_or(1);
        }
        out.push('\n');
    }
    out
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.txt"));
    if std::env::var_os("RUSTODE_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run with RUSTODE_UPDATE_SNAPSHOTS=1",
            path.display()
        )
    });
    assert_eq!(actual, expected, "snapshot {name} differs");
}

/// Ctrl+S, then C: the prompt's Compare.
fn open_compare(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    press(app, terminal, KeyCode::Char('s'), KeyModifiers::CONTROL);
    key(app, terminal, KeyCode::Char('c'));
}

#[test]
fn the_prompt_offers_compare_first() {
    let (mut app, mut terminal, _) =
        conflicted("prompt", "one\n", "mine one\n", "theirs\n", (130, 12));
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    );
    let shown = screen(&terminal);
    assert!(
        shown.contains("f.txt changed on disk: [C]ompare [O]verwrite [R]evert [Esc]"),
        "{shown}"
    );
}

#[test]
fn c_opens_a_unified_comparison_and_changes_nothing() {
    let (mut app, mut terminal, path) = simple("open");
    open_compare(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert_snapshot("compare-100x12", &shown);
    assert!(
        shown.contains("- theirs") && shown.contains("+ mine one"),
        "{shown}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    let id = app.document().expect("document");
    assert!(app.workspace().document(id).expect("doc").has_conflict());
}

#[test]
fn the_palette_opens_the_comparison_too() {
    let (mut app, mut terminal, _) = simple("palette");
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    for c in "file: compare".chars() {
        key(&mut app, &mut terminal, KeyCode::Char(c));
    }
    assert!(screen(&terminal).contains("File: Compare"));
    key(&mut app, &mut terminal, KeyCode::Enter);
    let shown = screen(&terminal);
    assert!(shown.contains("(in file) ↔ f.txt (in rustode)"), "{shown}");
}

#[test]
fn the_palette_command_without_a_conflict_names_the_file() {
    let dir = std::env::temp_dir().join(format!("rustode-tui-cmp-{}-none", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, "one\n").expect("fixture");
    let mut app = App::open(&path).expect("open");
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    for c in "file: compare".chars() {
        key(&mut app, &mut terminal, KeyCode::Char(c));
    }
    key(&mut app, &mut terminal, KeyCode::Enter);
    let shown = screen(&terminal);
    assert!(
        shown.contains("f.txt has no conflict to resolve"),
        "{shown}"
    );
    assert!(!shown.contains("Resolve save conflict"), "{shown}");
}

#[test]
fn esc_closes_the_comparison_and_the_conflict_stays() {
    let (mut app, mut terminal, path) = simple("esc");
    open_compare(&mut app, &mut terminal);
    key(&mut app, &mut terminal, KeyCode::Esc);
    let shown = screen(&terminal);
    assert!(!shown.contains("Resolve save conflict"), "{shown}");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    let id = app.document().expect("document");
    assert!(app.workspace().document(id).expect("doc").has_conflict());
}

#[test]
fn o_in_the_comparison_overwrites_the_file() {
    let (mut app, mut terminal, path) = simple("overwrite");
    open_compare(&mut app, &mut terminal);
    key(&mut app, &mut terminal, KeyCode::Char('o'));
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
    let shown = screen(&terminal);
    assert!(
        shown.contains("Saved") && !shown.contains("Resolve save conflict"),
        "{shown}"
    );
}

#[test]
fn r_in_the_comparison_reverts_to_the_file() {
    let (mut app, mut terminal, _) = simple("revert");
    open_compare(&mut app, &mut terminal);
    key(&mut app, &mut terminal, KeyCode::Char('r'));
    let id = app.document().expect("document");
    assert_eq!(
        app.workspace().document(id).expect("doc").text(),
        "theirs\n"
    );
    let shown = screen(&terminal);
    assert!(!shown.contains("Resolve save conflict"), "{shown}");
}

/// Forty lines; the unsaved text edits lines 3 and 30.
fn long() -> (String, String, String) {
    let base = (1..=40)
        .map(|n| format!("line {n}\n"))
        .collect::<Vec<_>>()
        .concat();
    let mine = base
        .replace("line 3\n", "mine 3\n")
        .replace("line 30\n", "mine 30\n");
    let disk = base.replace("line 30\n", "disk 30\n");
    (base, mine, disk)
}

#[test]
fn n_and_p_move_between_changes_and_say_when_there_are_no_more() {
    let (base, mine, disk) = long();
    let (mut app, mut terminal, _) = conflicted("nav", &base, &mine, &disk, (100, 12));
    open_compare(&mut app, &mut terminal);
    let first = screen(&terminal);
    assert!(
        first.contains("change 1/2") && first.contains("+ mine 3"),
        "{first}"
    );
    assert!(!first.contains("mine 30"), "{first}");
    key(&mut app, &mut terminal, KeyCode::Char('n'));
    let second = screen(&terminal);
    assert!(
        second.contains("change 2/2") && second.contains("+ mine 30"),
        "{second}"
    );
    assert!(second.contains("- disk 30"), "{second}");
    key(&mut app, &mut terminal, KeyCode::Char('n'));
    assert!(screen(&terminal).contains("This is the last change"));
    key(&mut app, &mut terminal, KeyCode::Char('p'));
    assert!(screen(&terminal).contains("change 1/2"));
    key(&mut app, &mut terminal, KeyCode::Char('p'));
    assert!(screen(&terminal).contains("This is the first change"));
}

#[test]
fn the_arrow_keys_scroll_and_never_leave_the_text() {
    let (base, mine, disk) = long();
    let (mut app, mut terminal, _) = conflicted("scroll", &base, &mine, &disk, (100, 12));
    open_compare(&mut app, &mut terminal);
    key(&mut app, &mut terminal, KeyCode::End);
    let end = screen(&terminal);
    assert!(end.contains("line 40"), "{end}");
    for _ in 0..5 {
        key(&mut app, &mut terminal, KeyCode::PageDown);
    }
    assert!(
        screen(&terminal).contains("line 40"),
        "paging past the end stays"
    );
    key(&mut app, &mut terminal, KeyCode::Home);
    key(&mut app, &mut terminal, KeyCode::Down);
    key(&mut app, &mut terminal, KeyCode::Up);
    key(&mut app, &mut terminal, KeyCode::PageUp);
    assert!(screen(&terminal).contains("line 1"));
}

#[test]
fn a_narrow_terminal_clips_instead_of_failing() {
    let (mut app, mut terminal, _) =
        conflicted("narrow", "one\n", "mine one\n", "theirs\n", (40, 10));
    open_compare(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains('+'), "{shown}");
}

#[test]
fn a_file_over_the_limit_shows_the_message_and_still_resolves() {
    let big = "l\n".repeat(rustode_workspace::COMPARE_MAX_LINES + 1);
    let (mut app, mut terminal, path) = conflicted("big", "one\n", "mine\n", &big, (110, 12));
    open_compare(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains("too large to compare"), "{shown}");
    key(&mut app, &mut terminal, KeyCode::Char('o'));
    assert_eq!(std::fs::read(&path).expect("read"), b"mine\n");
}

#[test]
fn identical_lines_say_so() {
    let (mut app, mut terminal, _) = conflicted("same", "one\n", "a\nb\n", "a\r\nb\r\n", (110, 12));
    open_compare(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains("The lines are the same"), "{shown}");
}
