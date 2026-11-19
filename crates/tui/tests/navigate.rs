//! Input to frame for go to definition (F12), find references (Shift+F12),
//! rename (F2) and format (Ctrl+Shift+I). The server is `rustode-test-lsp`,
//! which answers them over the `.rs` files of the folder. Snapshots are in
//! `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to write them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::{App, ColorMode};
use rustode_workspace::ServerState;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

const A: &str = "fn helper() {}\nfn main() {\n    helper();   \n}\n";
const B: &str = "fn other() {\n    helper();\n}\n";

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

#[track_caller]
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out");
        app.poll_language_servers();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// An app on `main.rs`, next to `other.rs`, with the test server running and
/// the cursor inside the call of `helper`.
fn open(name: &str, server_args: &[&str]) -> App {
    let dir = std::env::temp_dir().join(format!("rustode-tui-nav-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, A).expect("fixture");
    std::fs::write(dir.join("other.rs"), B).expect("fixture");
    // A plain string's Debug form is a valid JSON string.
    let command = format!("{:?}", server().display().to_string());
    let args: Vec<String> = server_args.iter().map(|a| format!("{a:?}")).collect();
    let settings = format!(
        "{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}, \"args\": [{}]}}}}}}",
        args.join(", ")
    );
    std::fs::write(dir.join("user/settings.json"), settings).expect("settings");
    let mut app = App::open_paths(&[file]).expect("open");
    app.load_user_config(&dir.join("user"));
    app.set_color_mode(ColorMode::TrueColor);
    wait(&mut app, |app| {
        app.workspace()
            .language_servers()
            .iter()
            .any(|s| s.state == ServerState::Running)
    });
    let id = app.document().expect("document");
    let at = A.match_indices("helper").nth(1).expect("call").0 + 2;
    app.workspace_mut().set_cursor(id, at).expect("cursor");
    app
}

fn shift(code: KeyCode) -> Event {
    key(code, KeyModifiers::SHIFT)
}

/// Draws until `needle` is on the screen.
#[track_caller]
fn wait_for(app: &mut App, terminal: &mut Terminal<TestBackend>, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_language_servers();
        draw(app, terminal);
        if screen(terminal).contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {needle:?} on\n{}",
            screen(terminal)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn f12_goes_to_the_definition() {
    let mut app = open("def", &[]);
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[press(KeyCode::F(12))]);
    let id = app.document().expect("document");
    wait(&mut app, |app| !app.workspace().navigation_pending());
    assert_eq!(app.document(), Some(id), "the declaration is in this file");
    let doc = app.workspace().document(id).expect("doc");
    assert_eq!(doc.primary_selection().anchor, 3);
    assert_eq!(doc.primary_selection().head, 9);
}

#[test]
fn shift_f12_lists_the_references_and_enter_goes_to_one() {
    let mut app = open("refs", &[]);
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[shift(KeyCode::F(12))]);
    wait_for(&mut app, &mut terminal, "References of 'helper': 3");
    assert_snapshot("references_list", &screen(&terminal));
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Down), press(KeyCode::Down)],
    );
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);
    draw(&mut app, &mut terminal);
    let screen = screen(&terminal);
    assert!(
        screen.contains("other.rs"),
        "the second file opened:\n{screen}"
    );
    let id = app.document().expect("document");
    let doc = app.workspace().document(id).expect("doc");
    assert_eq!(doc.cursor_position().line, 1);
    assert_eq!(doc.text(), B);
}

#[test]
fn typing_narrows_the_list_and_escape_closes_it() {
    let mut app = open("narrow", &[]);
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[shift(KeyCode::F(12))]);
    wait_for(&mut app, &mut terminal, "References of 'helper': 3");
    send(
        &mut app,
        &mut terminal,
        &[press(KeyCode::Char('o')), press(KeyCode::Char('t'))],
    );
    draw(&mut app, &mut terminal);
    let narrowed = screen(&terminal);
    assert!(!narrowed.contains("main.rs:1"), "{narrowed}");
    assert!(narrowed.contains("other.rs:2"), "{narrowed}");
    send(&mut app, &mut terminal, &[press(KeyCode::Esc)]);
    assert!(app.workspace().locations().is_none());
}

#[test]
fn f2_asks_for_the_new_name_and_enter_renames_in_every_file() {
    let mut app = open("rename", &[]);
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[press(KeyCode::F(2))]);
    draw(&mut app, &mut terminal);
    assert_snapshot("rename_box", &screen(&terminal));
    for _ in 0..6 {
        send(&mut app, &mut terminal, &[press(KeyCode::Backspace)]);
    }
    for c in "assist".chars() {
        send(&mut app, &mut terminal, &[press(KeyCode::Char(c))]);
    }
    draw(&mut app, &mut terminal);
    assert!(screen(&terminal).contains("rename to 'assist'"));
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);
    wait_for(&mut app, &mut terminal, "Renamed 'helper' to 'assist'");
    assert_snapshot("rename_done", &screen(&terminal));
    assert_eq!(
        text(&app),
        "fn assist() {}\nfn main() {\n    assist();   \n}\n"
    );
    send(&mut app, &mut terminal, &[ctrl('z')]);
    assert_eq!(text(&app), A, "one undo step");
}

#[test]
fn ctrl_shift_i_formats_and_says_when_there_is_nothing_to_do() {
    let mut app = open("format", &[]);
    let mut terminal = terminal();
    let chord = key(
        KeyCode::Char('I'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    send(&mut app, &mut terminal, &[chord.clone()]);
    wait_for(&mut app, &mut terminal, "Formatted the document");
    assert_eq!(
        text(&app),
        "fn helper() {}\nfn main() {\n    helper();\n}\n"
    );
    send(&mut app, &mut terminal, &[chord]);
    wait_for(&mut app, &mut terminal, "Nothing to format");
}

#[test]
fn the_palette_runs_them_and_a_missing_server_is_said() {
    let mut app = open(
        "palette",
        &["--no-rename", "--no-formatting", "--no-navigation"],
    );
    let mut terminal = terminal();
    for (key, said) in [
        (press(KeyCode::F(12)), "No language server can do that"),
        (press(KeyCode::F(2)), "No language server can rename here"),
        (
            key(
                KeyCode::Char('I'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            "No language server can do that",
        ),
    ] {
        send(&mut app, &mut terminal, &[key]);
        draw(&mut app, &mut terminal);
        assert!(screen(&terminal).contains(said), "{said}");
    }
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::F(1), KeyModifiers::NONE)],
    );
    for c in "format doc".chars() {
        send(&mut app, &mut terminal, &[press(KeyCode::Char(c))]);
    }
    draw(&mut app, &mut terminal);
    assert!(screen(&terminal).contains("Format Document"));
    assert!(screen(&terminal).contains("Ctrl+Shift+I"));
}

fn terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(119, 14)).expect("terminal")
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

fn press(code: KeyCode) -> Event {
    key(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> Event {
    key(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

fn text(app: &App) -> String {
    let id = app.document().expect("document");
    app.workspace().document(id).expect("doc").text()
}

fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.draw(frame)).expect("draw");
}
