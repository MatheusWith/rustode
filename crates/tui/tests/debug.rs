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
use rustode_workspace::{ConsoleKind, DebugState, DebugTree};
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
    open_program(name, "print one\nprint two\nprint three\n")
}

/// As [`open`], with this program.
fn open_program(name: &str, program: &str) -> (App, PathBuf) {
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

// ---- ticket 38: stepping, call stack, variables, watch and console ----

const NESTED: &str = "print starting\nglobal g 9\nvar a 1\nobj p k=v\nenter outer\nvar b 2\n\
obj o x=1 inner=@p\nprint in outer\nleave\nprint done\n";

fn chars(text: &str) -> Vec<Event> {
    text.chars()
        .map(|c| key(KeyCode::Char(c), KeyModifiers::NONE))
        .collect()
}

/// F1, the words, Enter: runs a command from the palette.
fn palette(app: &mut App, terminal: &mut Terminal<TestBackend>, words: &str) {
    send(app, terminal, &[key(KeyCode::F(1), KeyModifiers::NONE)]);
    send(app, terminal, &chars(words));
    send(app, terminal, &[key(KeyCode::Enter, KeyModifiers::NONE)]);
}

/// Stopped on the line of `print in outer`, views loaded.
fn stopped_in_outer(name: &str) -> (App, Terminal<TestBackend>) {
    let (mut app, _dir) = open_program(name, NESTED);
    let mut terminal = terminal(100, 24);
    let id = app.workspace().tabs()[0];
    app.workspace_mut()
        .toggle_breakpoint(id, 7)
        .expect("breakpoint");
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    wait(&mut app, "the stop and the views", |app| {
        let rows = app.workspace().debug_rows(DebugTree::Variables);
        state(app) == DebugState::Stopped
            && app.workspace().debug_frames().len() == 2
            && rows.len() > 2
            && rows.iter().all(|r| !r.loading)
    });
    (app, terminal)
}

#[test]
fn the_stopped_state_shows_call_stack_variables_watch_and_console() {
    let (mut app, mut terminal) = stopped_in_outer("views");
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("debug-views-stopped-100x24", &screen(&terminal));

    // Watch: focus the column from the palette, type an expression, Enter.
    palette(&mut app, &mut terminal, "focus on watch");
    send(&mut app, &mut terminal, &chars("b"));
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Enter, KeyModifiers::NONE)],
    );
    // Console: Tab to it, evaluate an object.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Tab, KeyModifiers::NONE)],
    );
    send(&mut app, &mut terminal, &chars("o"));
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Enter, KeyModifiers::NONE)],
    );
    wait(&mut app, "the watch value and the console result", |app| {
        let watch = app.workspace().debug_rows(DebugTree::Watch);
        let console = app.workspace().debug_console();
        watch.first().is_some_and(|r| r.value == "2")
            && console.iter().any(|l| l.kind == ConsoleKind::Result)
    });
    // Variables: open `o` from the keyboard (Tab wraps to the call stack).
    send(
        &mut app,
        &mut terminal,
        &[
            key(KeyCode::Tab, KeyModifiers::NONE),
            key(KeyCode::Tab, KeyModifiers::NONE),
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Right, KeyModifiers::NONE),
        ],
    );
    wait(&mut app, "o's members", |app| {
        app.workspace().debug_rows(DebugTree::Variables).len() > 5
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert_snapshot("debug-views-inspected-100x24", &screen(&terminal));
}

#[test]
fn f10_f11_and_shift_f11_step_and_enter_on_a_frame_moves_the_editor() {
    let (mut app, mut terminal) = stopped_in_outer("keys");
    let id = app.workspace().tabs()[0];
    assert_eq!(app.workspace().stopped_line(id), Some(7));
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(10), KeyModifiers::NONE)],
    );
    wait(&mut app, "the step", |app| {
        state(app) == DebugState::Stopped && app.workspace().stopped_line(id) == Some(8)
    });
    // Shift+F11 leaves `outer`: its `leave` runs, main goes on at `print done`.
    wait(&mut app, "the stack", |app| {
        app.workspace().debug_frames().len() == 2
    });
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(11), KeyModifiers::SHIFT)],
    );
    wait(&mut app, "the step out", |app| {
        state(app) == DebugState::Stopped && app.workspace().stopped_line(id) == Some(9)
    });
    wait(&mut app, "main only", |app| {
        app.workspace().debug_frames().len() == 1
    });
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(11), KeyModifiers::NONE)],
    );
    wait(&mut app, "the end", |app| {
        state(app) == DebugState::Inactive
    });
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    assert!(screen(&terminal).contains("exited with code 0"));
}

#[test]
fn selecting_a_frame_with_enter_goes_to_its_line() {
    let (mut app, mut terminal) = stopped_in_outer("frame");
    let id = app.workspace().tabs()[0];
    palette(&mut app, &mut terminal, "focus on call stack");
    send(
        &mut app,
        &mut terminal,
        &[
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Enter, KeyModifiers::NONE),
        ],
    );
    assert_eq!(app.workspace().debug_selected_frame(), 1);
    assert_eq!(
        app.workspace().stopped_line(id),
        Some(4),
        "the call in main"
    );
    let cursor = app.workspace().document(id).expect("doc").cursor_position();
    assert_eq!(cursor.line, 4);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Esc, KeyModifiers::NONE)],
    );
}

#[test]
fn f6_pauses_while_the_program_runs_and_moves_the_focus_otherwise() {
    let (mut app, _dir) = open_program("f6", "print a\nWAIT\nprint b\n");
    let mut terminal = terminal(100, 20);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(5), KeyModifiers::NONE)],
    );
    wait(&mut app, "running", |app| state(app) == DebugState::Running);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(6), KeyModifiers::NONE)],
    );
    wait(&mut app, "the pause", |app| {
        state(app) == DebugState::Stopped
    });
    assert_eq!(
        app.workspace().debug_status().reason.as_deref(),
        Some("pause")
    );
    assert!(
        !screen(&terminal).contains("EXPLORER"),
        "the key paused, it did not move the focus"
    );
    // Stopped: F6 is "focus the next part" again.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(6), KeyModifiers::NONE)],
    );
    assert_eq!(state(&app), DebugState::Stopped);
    assert!(screen(&terminal).contains("EXPLORER"));
}
