//! Input to frame: keys, pastes and resizes go into the TUI, and the cells
//! it draws come out of ratatui's test backend. Each width band has a text
//! snapshot in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to
//! write them, then review the diff.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use rustode_tui::App;
use std::path::PathBuf;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

const SAMPLE: &str = "fn main() {\n    println!(\"héllo 日本\");\n}\n";

fn fixture(name: &str, file: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-tui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(file);
    std::fs::write(&path, text).expect("fixture");
    path
}

fn open_sample(name: &str) -> (App, PathBuf) {
    let path = fixture(name, "main.rs", SAMPLE);
    (App::open(&path).expect("open"), path)
}

fn terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(width, height)).expect("terminal")
}

fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.draw(frame)).expect("draw");
}

/// The screen as a terminal shows it: a wide char hides the cell after it.
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

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

#[test]
fn each_width_band_has_its_layout() {
    let bands = [
        ("wide-130x12", 130, 12),
        ("medium-100x12", 100, 12),
        ("narrow-70x12", 70, 12),
        ("editor-only-50x12", 50, 12),
        ("too-narrow-39x12", 39, 12),
        ("too-short-60x9", 60, 9),
    ];
    for (name, width, height) in bands {
        let (mut app, _) = open_sample(&format!("band-{name}"));
        let mut terminal = terminal(width, height);
        draw(&mut app, &mut terminal);
        assert_snapshot(name, &screen(&terminal));
    }
}

#[test]
fn resizing_redraws_without_stale_cells() {
    let (mut app, _) = open_sample("resize");
    let mut terminal = terminal(130, 12);
    draw(&mut app, &mut terminal);
    terminal.backend_mut().resize(50, 12);
    draw(&mut app, &mut terminal);
    let (mut fresh, _) = open_sample("resize-fresh");
    let mut expected = self::terminal(50, 12);
    draw(&mut fresh, &mut expected);
    assert_eq!(screen(&terminal), screen(&expected));
}

#[test]
fn typing_moves_the_cursor_and_ctrl_s_saves() {
    let (mut app, path) = open_sample("type-save");
    let mut terminal = terminal(100, 12);
    let mut events = vec![key(KeyCode::Down), key(KeyCode::End), key(KeyCode::Enter)];
    events.extend("x();".chars().map(|c| key(KeyCode::Char(c))));
    send(&mut app, &mut terminal, &events);
    let shown = screen(&terminal);
    assert!(shown.contains("  3     x();"), "{shown}");
    assert!(shown.contains("main.rs ●"), "dirty marker: {shown}");
    assert!(shown.contains("Ln 3, Col 9  UTF-8  LF  Rust"), "{shown}");
    send(&mut app, &mut terminal, &[ctrl('s')]);
    let saved = std::fs::read_to_string(&path).expect("read");
    assert_eq!(
        saved,
        "fn main() {\n    println!(\"héllo 日本\");\n    x();\n}\n"
    );
    assert!(
        screen(&terminal).contains("main.rs  Saved"),
        "{}",
        screen(&terminal)
    );
}

#[test]
fn shift_arrows_select_and_the_selection_is_drawn_reversed() {
    let (mut app, _) = open_sample("select");
    let mut terminal = terminal(100, 12);
    let shift_right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    send(&mut app, &mut terminal, &[shift_right.clone(), shift_right]);
    let buffer = terminal.backend().buffer();
    // The gutter takes 5 columns ("  1 " plus padding); "fn" starts at x = 5.
    assert!(buffer[(5, 0)].modifier.contains(Modifier::REVERSED));
    assert!(buffer[(6, 0)].modifier.contains(Modifier::REVERSED));
    assert!(!buffer[(7, 0)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn ctrl_q_asks_before_discarding_unsaved_changes() {
    let (mut app, _) = open_sample("quit");
    let mut terminal = terminal(100, 12);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Char('a')), ctrl('q')],
    );
    assert!(!app.should_quit());
    assert!(
        screen(&terminal).contains("Unsaved changes"),
        "{}",
        screen(&terminal)
    );
    send(&mut app, &mut terminal, &[ctrl('q')]);
    assert!(app.should_quit());
}

#[test]
fn a_paste_is_inserted_and_undone_as_one_step() {
    let (mut app, _) = open_sample("paste");
    let mut terminal = terminal(100, 12);
    send(
        &mut app,
        &mut terminal,
        &[Event::Paste("one\ntwo ".to_owned())],
    );
    let text = |app: &App| {
        app.workspace()
            .document(app.document())
            .expect("doc")
            .text()
    };
    assert!(text(&app).starts_with("one\ntwo fn main()"));
    send(&mut app, &mut terminal, &[ctrl('z')]);
    assert_eq!(text(&app), SAMPLE);
}

#[test]
fn page_down_moves_by_the_editor_height_minus_two() {
    let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
    let lines = lines.join("\n") + "\n";
    let path = fixture("page", "long.txt", &lines);
    let mut app = App::open(&path).expect("open");
    let mut terminal = terminal(100, 12);
    draw(&mut app, &mut terminal);
    send(&mut app, &mut terminal, &[key(KeyCode::PageDown)]);
    // 11 editor rows above the status bar: VS Code pages by 11 - 2 = 9 lines.
    assert!(
        screen(&terminal).contains("Ln 10, Col 1"),
        "{}",
        screen(&terminal)
    );
}

#[test]
fn key_to_frame_p99_stays_under_16_ms() {
    let (mut app, _) = open_sample("latency");
    let mut terminal = terminal(120, 40);
    draw(&mut app, &mut terminal);
    let mut events = Vec::new();
    for i in 0..600 {
        events.push(match i % 6 {
            0 => key(KeyCode::Right),
            1 => key(KeyCode::Down),
            2 => key(KeyCode::Enter),
            3 => key(KeyCode::Backspace),
            _ => key(KeyCode::Char('x')),
        });
    }
    send(&mut app, &mut terminal, &events);
    let report = app.latency().expect("samples");
    eprintln!("key to frame (test backend, this build): {report}");
    assert_eq!(report.count, 600);
    assert!(report.p99 < Duration::from_millis(16), "{report}");
}
