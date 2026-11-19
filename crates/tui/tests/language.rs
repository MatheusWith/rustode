//! Input to frame for "Change Language Mode": the palette command and a
//! click on the status bar's language name open the picker; typing filters
//! it, Enter picks, and the document is highlighted with the new grammar.
//! Snapshots are in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1`
//! to write them. Detection and the list are tested at the workspace seam
//! (`crates/workspace/tests/languages.rs`).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::style::Color::Rgb;
use rustode_tui::{App, ColorMode};
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

const SAMPLE: &str = "def f(a):\n    return None\n";

fn app(name: &str, file: &str) -> App {
    let dir = std::env::temp_dir().join(format!("rustode-tui-lang-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join(file), SAMPLE).expect("fixture");
    let mut app = App::open(&dir.join(file)).expect("open");
    app.set_color_mode(ColorMode::TrueColor);
    app
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

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

fn type_text(app: &mut App, terminal: &mut Terminal<TestBackend>, text: &str) {
    let events: Vec<Event> = text.chars().map(|c| key(KeyCode::Char(c))).collect();
    send(app, terminal, &events);
}

fn palette(app: &mut App, terminal: &mut Terminal<TestBackend>, query: &str) {
    send(
        app,
        terminal,
        &[Event::Key(KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))],
    );
    type_text(app, terminal, query);
}

fn fg(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> ratatui::style::Color {
    terminal.backend().buffer()[(x, y)].fg
}

#[test]
fn the_palette_command_opens_the_picker_and_a_pick_highlights_the_document() {
    let mut app = app("palette", "notes.txt");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    palette(&mut app, &mut terminal, "language mode");
    assert_snapshot("language-palette-100x16", &screen(&terminal));
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    assert_snapshot("language-picker-100x16", &screen(&terminal));
    type_text(&mut app, &mut terminal, "pyth");
    assert_snapshot("language-filtered-100x16", &screen(&terminal));
    assert_eq!(fg(&terminal, 5, 1), Rgb(0xCC, 0xCC, 0xCC), "plain text yet");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    let shown = screen(&terminal);
    assert!(shown.trim_end().ends_with("Python"), "{shown}");
    assert_snapshot("language-picked-100x16", &shown);
    assert_eq!(fg(&terminal, 5, 1), Rgb(0x56, 0x9C, 0xD6), "def: keyword");
}

#[test]
fn a_click_on_the_status_bars_language_opens_the_picker() {
    let mut app = app("click", "notes.txt");
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    send(&mut app, &mut terminal, &[key(KeyCode::Esc)]);
    let click = |x: u16, y: u16| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    };
    // The status bar is the last row; "Plain Text " ends it.
    send(&mut app, &mut terminal, &[click(10, 15)]);
    assert!(
        !screen(&terminal).contains("Select"),
        "elsewhere does nothing"
    );
    assert!(!screen(&terminal).contains("Auto Detect"));
    send(&mut app, &mut terminal, &[click(89, 15)]);
    let shown = screen(&terminal);
    assert!(shown.contains("Auto Detect"), "{shown}");
    type_text(&mut app, &mut terminal, "plain");
    let shown = screen(&terminal);
    assert!(
        shown.contains("Plain Text  (plaintext) - Configured Language"),
        "{shown}"
    );
    send(&mut app, &mut terminal, &[key(KeyCode::Esc)]);
    assert!(
        !screen(&terminal).contains("Auto Detect"),
        "Escape closes it"
    );
}
