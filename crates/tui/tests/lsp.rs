//! Input to frame with a language server: diagnostics drawn inline, the
//! status bar's counts and server, and the problems panel (Ctrl+Shift+M),
//! whose rows go to their problem. The server is `rustode-test-lsp`, which
//! reports every `ERROR` and `WARN` and exits on `CRASH`. Snapshots are in
//! `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to write them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::style::{Color, Modifier};
use rustode_tui::{App, ColorMode};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

const SAMPLE: &str = "fn main() {\n    let x = ERROR;\n    WARN\n}\n";

/// The test server, built next to this test binary: in one of the folders
/// above it, whatever layout cargo gives its build folder. When only this
/// crate's tests were built, it is built now.
fn server() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary");
    if let Some(path) = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("rustode-test-lsp"))
        .find(|path| path.is_file())
    {
        return path;
    }
    let profile = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "debug"))
        .expect("a debug profile folder");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "-p", "rustode-test-lsp"])
        .arg("--target-dir")
        .arg(profile.parent().expect("target dir"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building rustode-test-lsp failed");
    profile.join("rustode-test-lsp")
}

/// A folder with `main.rs` and a `user` folder whose settings start the
/// test server for Rust; the app opens the folder and the file.
fn open(name: &str) -> (App, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-tui-lsp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, SAMPLE).expect("fixture");
    // A plain path's Debug form is a valid JSON string.
    let command = format!("{:?}", server().display().to_string());
    let settings =
        format!("{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}}}}}}}");
    std::fs::write(dir.join("user/settings.json"), settings).expect("settings");
    let mut app = App::open_paths(&[dir.clone(), file]).expect("open");
    app.load_user_config(&dir.join("user"));
    app.set_color_mode(ColorMode::TrueColor);
    wait(&mut app, |app| app.workspace().problems().len() == 2);
    (app, dir)
}

#[track_caller]
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out");
        app.poll_language_servers();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(width, height)).expect("terminal")
}

fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.draw(frame)).expect("draw");
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

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn ctrl_shift_m() -> Event {
    key(
        KeyCode::Char('M'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )
}

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

/// Where `needle` is on screen: column and row of its first char.
fn find(terminal: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
    screen(terminal)
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find(needle).map(|byte| {
                let x = line[..byte].width();
                (u16::try_from(x).expect("x"), u16::try_from(y).expect("y"))
            })
        })
        .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{}", screen(terminal)))
}

#[test]
fn diagnostics_are_underlined_lettered_and_counted() {
    let (mut app, _dir) = open("inline");
    let mut terminal = terminal(100, 12);
    draw(&mut app, &mut terminal);
    assert_snapshot("lsp-markers-100x12", &screen(&terminal));
    let (x, y) = find(&terminal, "ERROR");
    let cell = &terminal.backend().buffer()[(x, y)];
    assert!(cell.modifier.contains(Modifier::UNDERLINED));
    assert_eq!(cell.underline_color, Color::Rgb(0xF1, 0x4C, 0x4C));
    assert_eq!(terminal.backend().buffer()[(0, y)].symbol(), "E");

    app.set_color_mode(ColorMode::None);
    draw(&mut app, &mut terminal);
    let buffer = terminal.backend().buffer();
    let error = &buffer[(x, y)];
    assert!(
        error
            .modifier
            .contains(Modifier::UNDERLINED | Modifier::BOLD),
        "without color an error is underlined and bold"
    );
    let (x, y) = find(&terminal, "WARN");
    let warning = &terminal.backend().buffer()[(x, y)];
    assert!(warning.modifier.contains(Modifier::UNDERLINED));
    assert!(!warning.modifier.contains(Modifier::BOLD));
    assert_eq!(terminal.backend().buffer()[(0, y)].symbol(), "W");
}

#[test]
fn ctrl_shift_m_shows_the_problems_and_enter_goes_to_one() {
    let (mut app, _dir) = open("panel");
    let mut terminal = terminal(100, 16);
    send(&mut app, &mut terminal, &[ctrl_shift_m()]);
    assert_snapshot("lsp-problems-100x16", &screen(&terminal));
    send(
        &mut app,
        &mut terminal,
        &[
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Enter, KeyModifiers::NONE),
        ],
    );
    let shown = screen(&terminal);
    assert!(
        shown.contains("Ln 3, Col 9"),
        "at the warning's end: {shown}"
    );
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Char('x'), KeyModifiers::NONE)],
    );
    assert!(
        screen(&terminal).contains("    x\n"),
        "the editor has focus and the warning was selected"
    );
    send(&mut app, &mut terminal, &[ctrl_shift_m(), ctrl_shift_m()]);
    assert!(
        !screen(&terminal).contains("PROBLEMS"),
        "a second Ctrl+Shift+M in the panel hides it"
    );
}

#[test]
fn the_status_bar_counts_toggle_the_panel_and_a_row_click_goes_to_it() {
    let (mut app, _dir) = open("click");
    let mut terminal = terminal(100, 16);
    draw(&mut app, &mut terminal);
    let (x, y) = find(&terminal, "E 1 W 1");
    let click = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    send(&mut app, &mut terminal, &[click(x, y)]);
    let (x, y) = find(&terminal, "ERROR found");
    send(&mut app, &mut terminal, &[click(x, y)]);
    let shown = screen(&terminal);
    assert!(
        shown.contains("Ln 2, Col 18"),
        "the error is selected: {shown}"
    );
}

#[test]
fn a_crash_is_reported_and_the_server_comes_back() {
    let (mut app, _dir) = open("crash");
    let mut terminal = terminal(200, 12);
    send(
        &mut app,
        &mut terminal,
        &[Event::Paste("CRASH ".to_owned())],
    );
    wait(&mut app, |app| {
        app.workspace()
            .language_servers()
            .first()
            .is_some_and(|s| s.restarts == 1)
    });
    draw(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(
        shown.contains("Language server for rust: rustode-test-lsp exited with code 3"),
        "{shown}"
    );
}
