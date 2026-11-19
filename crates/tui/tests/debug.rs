//! Input to frame with a debug adapter: a click on the gutter sets a
//! breakpoint, F5 starts the launch configuration, the program stops on the
//! breakpoint's line, F5 continues it to the end, Shift+F5 stops it. The
//! adapter is `rustode-test-dap`, whose program is a text file. Snapshots
//! are in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to write
//! them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rustode_tui::App;
use rustode_workspace::DebugState;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// The test adapter, built next to this test binary (see `lsp.rs`).
fn adapter() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary");
    if let Some(path) = exe
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("rustode-test-dap"))
        .find(|path| path.is_file())
    {
        return path;
    }
    let profile = exe
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "debug"))
        .expect("a debug profile folder");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "-p", "rustode-test-dap"])
        .arg("--target-dir")
        .arg(profile.parent().expect("target dir"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building rustode-test-dap failed");
    profile.join("rustode-test-dap")
}

/// A folder with the program, a `launch.json` and settings naming the test
/// adapter; the app opens the folder and the program.
fn open(name: &str) -> (App, PathBuf) {
    let dir = std::env::temp_dir()
        .join(format!("rustode-tui-dap-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("user")).expect("temp dir");
    std::fs::create_dir_all(dir.join(".vscode")).expect("vscode dir");
    let file = dir.join("program.txt");
    std::fs::write(&file, "print one\nprint two\nprint three\n").expect("program");
    std::fs::write(
        dir.join(".vscode/launch.json"),
        r#"{ "configurations": [ { "name": "Run program", "type": "test", "request": "launch",
              "program": "${workspaceFolder}/program.txt" } ] }"#,
    )
    .expect("launch.json");
    let command = format!("{:?}", adapter().display().to_string());
    let settings =
        format!("{{\"rustode.debugAdapters\": {{\"test\": {{\"command\": {command}}}}}}}");
    std::fs::write(dir.join("user/settings.json"), settings).expect("settings");
    let mut app = App::open_paths(&[dir.clone(), file]).expect("open");
    app.load_user_config(&dir.join("user"));
    (app, dir)
}

#[track_caller]
fn wait(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        app.poll_language_servers();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state(app: &App) -> DebugState {
    app.workspace().debug_status().state
}

fn terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(width, height)).expect("terminal")
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

/// A left click on the gutter of the line whose text is `needle`.
fn click_gutter_of(app: &mut App, terminal: &mut Terminal<TestBackend>, needle: &str) {
    let (x, y) = find(terminal, needle);
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x - 2,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    send(app, terminal, &[click]);
}

#[test]
fn a_gutter_click_sets_a_breakpoint_and_f5_stops_on_it_and_continues() {
    let (mut app, _dir) = open("run");
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[]);
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    click_gutter_of(&mut app, &mut terminal, "print two");
    let id = app.workspace().tabs()[0];
    assert_eq!(app.workspace().breakpoints(id), [1]);
    assert_snapshot("debug-breakpoint-100x12", &screen(&terminal));

    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    wait(&mut app, "the stop", |app| {
        state(app) == DebugState::Stopped && app.workspace().stopped_line(id).is_some()
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("debug-stopped-100x12", &screen(&terminal));

    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    wait(&mut app, "the end", |app| {
        state(app) == DebugState::Inactive
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("debug-ended-100x12", &screen(&terminal));
}

#[test]
fn shift_f5_stops_and_f9_toggles_at_the_cursor() {
    let (mut app, _dir) = open("stop");
    let mut terminal = terminal(100, 12);
    let id = app.workspace().tabs()[0];
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(9), KeyModifiers::NONE)],
    );
    assert_eq!(
        app.workspace().breakpoints(id),
        [0],
        "the cursor is on line 1"
    );
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    wait(&mut app, "the stop", |app| {
        state(app) == DebugState::Stopped
    });
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::SHIFT)],
    );
    wait(&mut app, "the end", |app| {
        state(app) == DebugState::Inactive
    });
}

#[test]
fn starting_without_an_adapter_says_why_on_the_status_bar() {
    let (mut app, dir) = open("no-adapter");
    std::fs::write(dir.join("user/settings.json"), "{}").expect("settings");
    app.load_user_config(&dir.join("user"));
    let mut terminal = terminal(120, 12);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    let shown = screen(&terminal);
    assert!(
        shown.contains("no debug adapter for type \"test\""),
        "{shown}"
    );
}
