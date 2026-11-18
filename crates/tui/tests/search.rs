//! Input to frame for the Search view: Ctrl+Shift+F, typing a query and
//! Enter, the results streaming into the list with their count, the
//! toggles, collapsing a file, and opening a match from the keyboard or
//! with a click. Snapshots are in `tests/snapshots/`; run with
//! `RUSTODE_UPDATE_SNAPSHOTS=1` to write them. The search itself is tested
//! at the workspace seam (`crates/workspace/tests/search.rs`).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rustode_tui::App;
use rustode_workspace::Position;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// A folder with a few files, opened in the explorer, with `notes.txt` in
/// a tab.
fn app(name: &str) -> (App, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("rustode-tui-search-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in [
        ("notes.txt", "hello\n"),
        (
            "src/lib.rs",
            "pub fn needle() {}\n// a needle and a Needle\n",
        ),
        ("src/main.rs", "fn main() { needle(); }\n"),
        ("README.md", "No match here.\n"),
    ] {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, text).expect("fixture");
    }
    let app = App::open_paths(&[dir.clone(), dir.join("notes.txt")]).expect("open");
    (app, dir)
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut row = String::new();
        let mut x = 0;
        while x < buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            row.push_str(symbol);
            x += u16::try_from(symbol.width().max(1)).unwrap_or(1);
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT))
}

fn ctrl_shift_f() -> Event {
    Event::Key(KeyEvent::new(
        KeyCode::Char('F'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ))
}

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

fn type_text(app: &mut App, terminal: &mut Terminal<TestBackend>, text: &str) {
    let events: Vec<Event> = text.chars().map(|c| key(KeyCode::Char(c))).collect();
    send(app, terminal, &events);
}

/// Polls until the search is over, then draws.
fn settle(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.workspace().search_results().is_running() {
        app.poll();
        assert!(Instant::now() < deadline, "the search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    app.poll();
    terminal.draw(|frame| app.draw(frame)).expect("draw");
}

/// Opens the view and searches for `text`.
fn search(app: &mut App, terminal: &mut Terminal<TestBackend>, text: &str) {
    send(app, terminal, &[ctrl_shift_f()]);
    type_text(app, terminal, text);
    send(app, terminal, &[key(KeyCode::Enter)]);
    settle(app, terminal);
}

#[test]
fn ctrl_shift_f_shows_the_view_and_enter_lists_the_results() {
    let (mut app, _) = app("results");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_f()]);
    assert_snapshot("search-empty-100x16", &screen(&terminal));
    type_text(&mut app, &mut terminal, "needle");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    assert_snapshot("search-results-100x16", &screen(&terminal));
}

#[test]
fn toggles_fields_and_errors_show_in_the_view() {
    let (mut app, _) = app("toggles");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    search(&mut app, &mut terminal, "Needle");
    assert_eq!(app.workspace().search_results().match_count(), 4);
    send(&mut app, &mut terminal, &[alt('c')]);
    settle(&mut app, &mut terminal);
    assert_eq!(
        app.workspace().search_results().match_count(),
        1,
        "Alt+C searches again with Match Case"
    );
    send(&mut app, &mut terminal, &[key(KeyCode::Tab)]);
    type_text(&mut app, &mut terminal, "main.rs");
    send(&mut app, &mut terminal, &[alt('c')]);
    settle(&mut app, &mut terminal);
    assert_eq!(app.workspace().search_results().match_count(), 1);
    assert_snapshot("search-include-100x16", &screen(&terminal));
    send(&mut app, &mut terminal, &[key(KeyCode::BackTab), alt('r')]);
    type_text(&mut app, &mut terminal, "(");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    let shown = screen(&terminal);
    assert!(shown.contains("invalid regular expression"), "{shown}");
}

#[test]
fn enter_on_a_match_opens_it_and_left_collapses_a_file() {
    let (mut app, dir) = app("open");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    search(&mut app, &mut terminal, "needle");
    // Rows: lib.rs, its three matches, main.rs, its match.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Left)],
    );
    let shown = screen(&terminal);
    assert!(shown.contains("▸ lib.rs"), "{shown}");
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Down), key(KeyCode::Enter)],
    );
    let id = app.document().expect("a tab");
    let document = app.workspace().document(id).expect("document");
    assert_eq!(document.path(), dir.join("src/main.rs"));
    let selection = document.primary_selection().range();
    assert_eq!(document.position_of(selection.start), Position::new(0, 12));
    assert_eq!(document.position_of(selection.end), Position::new(0, 18));
    type_text(&mut app, &mut terminal, "x");
    let text = app.workspace().document(id).expect("document").text();
    assert_eq!(text, "fn main() { x(); }\n", "focus moved to the editor");
}

#[test]
fn escape_goes_back_to_the_editor_and_a_click_opens_a_match() {
    let (mut app, dir) = app("click");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    search(&mut app, &mut terminal, "needle");
    send(&mut app, &mut terminal, &[key(KeyCode::Esc)]);
    type_text(&mut app, &mut terminal, "a");
    let notes = app.document().expect("a tab");
    assert_eq!(
        app.workspace().document(notes).expect("doc").text(),
        "ahello\n"
    );
    send(&mut app, &mut terminal, &[ctrl_shift_f()]);
    // The sixth result row, main.rs's match: rows start below the header.
    let click = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 10,
            row: 5 + 5,
            modifiers: KeyModifiers::NONE,
        })
    };
    send(
        &mut app,
        &mut terminal,
        &[
            click(MouseEventKind::Down(MouseButton::Left)),
            click(MouseEventKind::Up(MouseButton::Left)),
        ],
    );
    let id = app.document().expect("a tab");
    assert_eq!(
        app.workspace().document(id).expect("doc").path(),
        dir.join("src/main.rs")
    );
    assert!(
        screen(&terminal).contains("SEARCH"),
        "a click keeps the view focused"
    );
}
