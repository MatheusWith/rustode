//! Input to frame for hover and completion: Ctrl+Space and typing open the
//! suggest list under the word, Enter accepts, Escape closes, Ctrl+K Ctrl+I
//! shows the hover. The server is `rustode-test-lsp`, which answers hovers
//! and completions from scripted rules. Snapshots are in `tests/snapshots/`;
//! run with `RUSTODE_UPDATE_SNAPSHOTS=1` to write them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::{App, ColorMode};
use rustode_workspace::ServerState;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

const CODE: &str = "fn main() {\n    let point = 1;\n    pri\n}\n";

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

/// An app on `main.rs` with the test server running, the cursor after `pri`.
fn open(name: &str) -> App {
    let dir =
        std::env::temp_dir().join(format!("rustode-tui-suggest-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, CODE).expect("fixture");
    // A plain path's Debug form is a valid JSON string.
    let command = format!("{:?}", server().display().to_string());
    let settings =
        format!("{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}}}}}}}");
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
    let at = CODE.find("pri").expect("pri") + 3;
    app.workspace_mut().set_cursor(id, at).expect("cursor");
    app
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

#[test]
fn ctrl_space_opens_the_list_under_the_word() {
    let mut app = open("list");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[ctrl(' ')]);
    wait(&mut app, |app| app.workspace().completion_visible());
    draw(&mut app, &mut terminal);
    assert_snapshot("suggest_list", &screen(&terminal));
}

#[test]
fn typing_opens_and_narrows_the_list_and_the_selection_moves() {
    let mut app = open("typing");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[press(KeyCode::Char('n'))]);
    wait(&mut app, |app| app.workspace().completion_visible());
    send(&mut app, &mut terminal, &[press(KeyCode::Down)]);
    draw(&mut app, &mut terminal);
    assert_snapshot("suggest_typing", &screen(&terminal));
    send(&mut app, &mut terminal, &[press(KeyCode::Char('t'))]);
    draw(&mut app, &mut terminal);
    assert_snapshot("suggest_narrowed", &screen(&terminal));
}

#[test]
fn enter_accepts_and_escape_closes() {
    let mut app = open("accept");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[ctrl(' ')]);
    wait(&mut app, |app| app.workspace().completion_visible());
    send(&mut app, &mut terminal, &[press(KeyCode::Esc)]);
    assert!(!app.workspace().completion_visible());
    assert_eq!(text(&app), CODE, "escape changed nothing");

    send(&mut app, &mut terminal, &[ctrl(' ')]);
    wait(&mut app, |app| app.workspace().completion_visible());
    send(&mut app, &mut terminal, &[press(KeyCode::Enter)]);
    assert!(!app.workspace().completion_visible());
    assert!(text(&app).contains("    println!()\n"), "{}", text(&app));
    send(&mut app, &mut terminal, &[ctrl('z')]);
    assert_eq!(text(&app), CODE, "one undo step");
}

#[test]
fn tab_accepts_and_a_page_moves_to_the_end() {
    let mut app = open("tab");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[ctrl(' ')]);
    wait(&mut app, |app| app.workspace().completion_visible());
    send(&mut app, &mut terminal, &[press(KeyCode::PageDown)]);
    send(&mut app, &mut terminal, &[press(KeyCode::Tab)]);
    assert!(text(&app).contains("    PrimaryKey\n"), "{}", text(&app));
}

#[test]
fn ctrl_k_ctrl_i_shows_the_hover_and_a_key_hides_it() {
    let mut app = open("hover");
    let id = app.document().expect("document");
    let at = CODE.find("point").expect("point") + 2;
    app.workspace_mut().set_cursor(id, at).expect("cursor");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[ctrl('k')]);
    draw(&mut app, &mut terminal);
    assert!(screen(&terminal).contains("(Ctrl+K) was pressed"));
    // A terminal sends Ctrl+I as Tab.
    send(&mut app, &mut terminal, &[press(KeyCode::Tab)]);
    wait(&mut app, |app| app.workspace().hover().is_some());
    draw(&mut app, &mut terminal);
    assert_snapshot("hover", &screen(&terminal));
    send(&mut app, &mut terminal, &[press(KeyCode::Left)]);
    assert!(app.workspace().hover().is_none(), "a key hides it");
    draw(&mut app, &mut terminal);
    assert!(!screen(&terminal).contains("let point\n"));
}

#[test]
fn without_a_server_the_keys_say_so() {
    let dir = std::env::temp_dir().join(format!("rustode-tui-suggest-{}-none", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, CODE).expect("fixture");
    let mut app = App::open_paths(&[file]).expect("open");
    let mut terminal = terminal();
    send(&mut app, &mut terminal, &[ctrl(' ')]);
    draw(&mut app, &mut terminal);
    assert!(screen(&terminal).contains("No suggestions here"));
    send(&mut app, &mut terminal, &[ctrl('k'), ctrl('i')]);
    draw(&mut app, &mut terminal);
    assert!(screen(&terminal).contains("Nothing to show a hover for here"));
}
