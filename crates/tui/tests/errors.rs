//! Input to frame: an error from the user's own action shows in the status
//! bar instead of being swallowed. The triggers are stale: a tab or a picker
//! that outlived its document, which the workspace refuses by name.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rustode_tui::App;
use unicode_width::UnicodeWidthStr;

fn app(name: &str, files: &[&str]) -> App {
    let dir = std::env::temp_dir().join(format!("rustode-tui-err-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let paths: Vec<_> = files
        .iter()
        .map(|file| {
            std::fs::write(dir.join(file), "text\n").expect("fixture");
            dir.join(file)
        })
        .collect();
    App::open_paths(&paths).expect("open")
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

fn key(app: &mut App, terminal: &mut Terminal<TestBackend>, event: KeyEvent) {
    app.step(terminal, &Event::Key(event)).expect("step");
}

#[test]
fn clicking_a_tab_whose_document_is_gone_says_so() {
    let mut app = app("tab", &["a.txt", "b.txt"]);
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");
    app.step(&mut terminal, &Event::FocusGained).expect("frame");
    let b = app.workspace().tabs()[1];
    let tabs = screen(&terminal);
    let column = tabs
        .lines()
        .next()
        .and_then(|row| row.find("b.txt"))
        .expect("b.txt tab");
    app.workspace_mut().close(b, true).expect("close b");
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: u16::try_from(column).expect("column"),
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    app.handle(&click);
    app.step(&mut terminal, &Event::FocusGained).expect("frame");
    let shown = screen(&terminal);
    assert!(shown.contains("no longer open"), "{shown}");
    assert!(!shown.contains("DocumentId"), "{shown}");
}

#[test]
fn a_picker_that_outlived_its_document_shows_why_it_has_no_rows() {
    let mut app = app("language", &["a.txt"]);
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
    key(
        &mut app,
        &mut terminal,
        KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    for c in "language mode".chars() {
        key(
            &mut app,
            &mut terminal,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    key(
        &mut app,
        &mut terminal,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    let doc = app.document().expect("document");
    assert!(screen(&terminal).contains("Auto Detect"), "picker is open");
    app.workspace_mut().close(doc, true).expect("close");
    key(
        &mut app,
        &mut terminal,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
    );
    let shown = screen(&terminal);
    assert!(shown.contains("no longer open"), "{shown}");
}
