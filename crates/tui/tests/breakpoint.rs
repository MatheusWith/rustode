//! Input to frame for ticket 39: the breakpoint editor (conditional
//! breakpoint, logpoint, edit) from the palette, and the gutter marks of each
//! kind of breakpoint, verified or not. The adapter is `rustode-test-dap`.
//! Snapshots are in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1`
//! to write them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::App;
use rustode_workspace::{BreakpointKind, DebugState, DebugTree};
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
/// As [`open`], with this program.
fn open_program(name: &str, program: &str) -> (App, PathBuf) {
    open_with(name, program, &[])
}

/// As [`open`], with these options for the adapter.
fn open_with(name: &str, program: &str, args: &[&str]) -> (App, PathBuf) {
    let dir = std::env::temp_dir()
        .join(format!("rustode-tui-dap-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("user")).expect("temp dir");
    std::fs::create_dir_all(dir.join(".vscode")).expect("vscode dir");
    let file = dir.join("program.txt");
    std::fs::write(&file, program).expect("program");
    std::fs::write(
        dir.join(".vscode/launch.json"),
        r#"{ "configurations": [ { "name": "Run program", "type": "test", "request": "launch",
              "program": "${workspaceFolder}/program.txt" } ] }"#,
    )
    .expect("launch.json");
    let command = format!("{:?}", adapter().display().to_string());
    let args = format!(
        "[{}]",
        args.iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let settings = format!(
        "{{\"rustode.debugAdapters\": {{\"test\": {{\"command\": {command}, \"args\": {args}}}}}}}"
    );
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

const LOOP: &str = "loop 3\ninc i\nprint tick\nend\n";

fn press(code: KeyCode) -> Event {
    key(code, KeyModifiers::NONE)
}

fn chars(text: &str) -> Vec<Event> {
    text.chars().map(|c| press(KeyCode::Char(c))).collect()
}

/// F1, the words, Enter: runs a command from the palette.
fn palette(app: &mut App, terminal: &mut Terminal<TestBackend>, words: &str) {
    send(app, terminal, &[press(KeyCode::F(1))]);
    send(app, terminal, &chars(words));
    send(app, terminal, &[press(KeyCode::Enter)]);
}

#[test]
fn the_conditional_breakpoint_box_sets_a_condition_and_the_program_stops_when_it_holds() {
    let (mut app, _dir) = open_program("conditional", LOOP);
    let mut terminal = terminal(100, 12);
    let id = app.workspace().tabs()[0];
    // The cursor goes to the third line, `print tick`.
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Down), press(KeyCode::Down)],
    );
    palette(&mut app, &mut terminal, "add conditional");
    send(&mut app, &mut terminal, &chars("i == 2"));
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("breakpoint-box-expression-100x12", &screen(&terminal));

    // Tab shows the hit count text, which is empty; Tab twice more is back.
    send(&mut app, &mut terminal, &[press(KeyCode::Tab)]);
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(screen(&terminal).contains("Hit Count: Break when hit count"));
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Tab), press(KeyCode::Tab)],
    );
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(screen(&terminal).contains("i == 2"), "the text was kept");
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);

    let info = app.workspace().breakpoint_at(id, 2).expect("set");
    assert_eq!(info.kind, BreakpointKind::Condition);
    assert_eq!(info.spec.condition.as_deref(), Some("i == 2"));
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("breakpoint-conditional-mark-100x12", &screen(&terminal));

    send(&mut app, &mut terminal, &[press(KeyCode::F(5))]);
    wait(&mut app, "the stop", |app| {
        state(app) == DebugState::Stopped
            && app.workspace().stopped_line(id) == Some(2)
            && app
                .workspace()
                .debug_rows(DebugTree::Variables)
                .iter()
                .any(|r| r.name.trim() == "i" && r.value == "2")
    });
    // F5 continues; the condition does not hold again.
    send(&mut app, &mut terminal, &[press(KeyCode::F(5))]);
    wait(&mut app, "the end", |app| {
        state(app) == DebugState::Inactive
    });
}

#[test]
fn the_logpoint_box_and_edit_breakpoint_reuse_the_texts_of_the_line() {
    let (mut app, _dir) = open_program("logpoint", LOOP);
    let mut terminal = terminal(100, 12);
    let id = app.workspace().tabs()[0];
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Down), press(KeyCode::Down)],
    );
    palette(&mut app, &mut terminal, "add logpoint");
    send(&mut app, &mut terminal, &chars("tick {i}"));
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(screen(&terminal).contains("Log Message: Message to log"));
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);
    assert_eq!(
        app.workspace().breakpoint_at(id, 2).expect("set").kind,
        BreakpointKind::LogPoint
    );

    // Edit Breakpoint opens on the log message the line already has.
    palette(&mut app, &mut terminal, "edit breakpoint");
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(
        screen(&terminal).contains("tick {i}"),
        "{}",
        screen(&terminal)
    );
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Backspace), press(KeyCode::Char('!'))],
    );
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);
    assert_eq!(
        app.workspace()
            .breakpoint_at(id, 2)
            .expect("set")
            .spec
            .log_message
            .as_deref(),
        Some("tick {i!")
    );

    // Escape cancels and a line with no breakpoint has nothing to edit.
    palette(&mut app, &mut terminal, "edit breakpoint");
    send(&mut app, &mut terminal, &chars("zzz"));
    send(&mut app, &mut terminal, &[press(KeyCode::Esc)]);
    assert_eq!(
        app.workspace()
            .breakpoint_at(id, 2)
            .expect("set")
            .spec
            .log_message
            .as_deref(),
        Some("tick {i!")
    );
    send(&mut app, &mut terminal, &[press(KeyCode::Up)]);
    palette(&mut app, &mut terminal, "edit breakpoint");
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(
        screen(&terminal).contains("there is no breakpoint on this line"),
        "{}",
        screen(&terminal)
    );
}

#[test]
fn each_kind_of_breakpoint_has_its_own_mark_and_an_unsupported_one_is_unverified() {
    let program = "print a\nprint b\nprint c\n";
    let (mut app, _dir) = open_with(
        "marks",
        program,
        &["--no-conditions", "--no-hit-conditions", "--no-logpoints"],
    );
    let mut terminal = terminal(100, 12);
    let id = app.workspace().tabs()[0];
    let spec =
        |c: Option<&str>, h: Option<&str>, l: Option<&str>| rustode_workspace::BreakpointSpec {
            condition: c.map(str::to_owned),
            hit_condition: h.map(str::to_owned),
            log_message: l.map(str::to_owned),
        };
    let ws = app.workspace_mut();
    ws.toggle_breakpoint(id, 0).expect("plain");
    ws.set_breakpoint(id, 1, spec(Some("a == 1"), None, None))
        .expect("condition");
    ws.set_breakpoint(id, 2, spec(None, None, Some("hi")))
        .expect("log");
    ws.toggle_breakpoint(id, 3).expect("past the end");
    send(&mut app, &mut terminal, &[]);
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    let before = screen(&terminal);
    assert_snapshot("breakpoint-marks-before-100x12", &before);

    send(&mut app, &mut terminal, &[press(KeyCode::F(5))]);
    wait(&mut app, "the stop and the answers", |app| {
        state(app) == DebugState::Stopped
            && app.workspace().stopped_line(id) == Some(0)
            && app
                .workspace()
                .breakpoint_at(id, 3)
                .is_some_and(|b| !b.verified)
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("breakpoint-marks-unverified-100x12", &screen(&terminal));
    let details = app.workspace().breakpoint_details(id);
    assert!(details[1..].iter().all(|b| !b.verified), "{details:?}");
}

#[test]
fn the_palette_on_a_short_screen_draws_fewer_rows_instead_of_panicking() {
    let (mut app, _dir) = open_program("short", LOOP);
    let mut terminal = terminal(100, 10);
    send(&mut app, &mut terminal, &[press(KeyCode::F(1))]);
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(screen(&terminal).contains("program.txt"));
}
