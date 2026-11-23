//! Input to frame: saving a document whose file changed under its unsaved
//! edits asks what to do (VS Code's Overwrite or Revert, as registered
//! commands) and never keeps either version silently. The decision is the
//! workspace's (`crates/workspace/tests/external_changes.rs`).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::App;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

fn conflicted(name: &str) -> (App, Terminal<TestBackend>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustode-tui-conf-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("f.txt");
    std::fs::write(&path, "one\n").expect("fixture");
    let mut app = App::open(&path).expect("open");
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
    for c in "mine ".chars() {
        press(
            &mut app,
            &mut terminal,
            KeyCode::Char(c),
            KeyModifiers::NONE,
        );
    }
    std::fs::write(&path, "theirs\n").expect("external write");
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

fn press(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode, mods: KeyModifiers) {
    app.step(terminal, &Event::Key(KeyEvent::new(code, mods)))
        .expect("step");
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

fn save(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    press(app, terminal, KeyCode::Char('s'), KeyModifiers::CONTROL);
}

#[test]
fn saving_in_conflict_asks_and_writes_nothing() {
    let (mut app, mut terminal, path) = conflicted("ask");
    save(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains("f.txt changed on disk:"), "{shown}");
    assert!(shown.contains("[O]verwrite"), "{shown}");
    assert!(shown.contains("[R]evert"), "{shown}");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    // Another key does not choose for the user.
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    );
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
    assert!(screen(&terminal).contains("[O]verwrite"));
}

#[test]
fn o_overwrites_the_file_with_my_text() {
    let (mut app, mut terminal, path) = conflicted("overwrite");
    save(&mut app, &mut terminal);
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('o'),
        KeyModifiers::NONE,
    );
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
    let shown = screen(&terminal);
    assert!(shown.contains("Saved"), "{shown}");
    assert!(!shown.contains("[O]verwrite"), "{shown}");
}

#[test]
fn r_loads_the_files_text_and_esc_keeps_editing() {
    let (mut app, mut terminal, path) = conflicted("revert");
    save(&mut app, &mut terminal);
    press(&mut app, &mut terminal, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!screen(&terminal).contains("[O]verwrite"));
    let id = app.document().expect("document");
    assert!(app.workspace().document(id).expect("doc").has_conflict());
    save(&mut app, &mut terminal);
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('r'),
        KeyModifiers::NONE,
    );
    let doc = app.workspace().document(id).expect("doc");
    assert_eq!(doc.text(), "theirs\n");
    assert_eq!(std::fs::read(&path).expect("read"), b"theirs\n");
}

#[test]
fn the_palette_runs_the_same_commands() {
    let (mut app, mut terminal, path) = conflicted("palette");
    press(
        &mut app,
        &mut terminal,
        KeyCode::Char('P'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    for c in "overwrite".chars() {
        press(
            &mut app,
            &mut terminal,
            KeyCode::Char(c),
            KeyModifiers::NONE,
        );
    }
    assert!(screen(&terminal).contains("File: Overwrite"));
    press(&mut app, &mut terminal, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(std::fs::read(&path).expect("read"), b"mine one\n");
}
