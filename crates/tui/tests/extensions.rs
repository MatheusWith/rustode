//! Input to frame for the Extensions view: Ctrl+Shift+X, a search against
//! a local Open VSX stand-in, Install, the Installed list with its
//! enablement and not-loaded notice, Enter to disable or enable, and Delete
//! to uninstall. Snapshots are in
//! `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to write them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rustode_tui::App;
use rustode_workspace::extensions::stand_in::{Fixture, StandIn};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-tui-ext-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The editor on a small file, with its extensions in a temp folder and
/// the stand-in as registry.
fn app(name: &str, stand_in: &StandIn) -> (App, PathBuf) {
    let dir = temp(name);
    let file = dir.join("notes.txt");
    std::fs::write(&file, "hello\n").expect("fixture");
    let mut app = App::open(&file).expect("open");
    let extensions = dir.join("extensions");
    app.workspace_mut().set_extensions_location(&extensions);
    app.workspace_mut()
        .set_global_storage_location(&dir.join("globalStorage"));
    app.workspace_mut()
        .set_extension_registry(stand_in.url())
        .expect("registry");
    (app, extensions)
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

fn ctrl_shift_x() -> Event {
    Event::Key(KeyEvent::new(
        KeyCode::Char('X'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ))
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

/// Polls until the background work is done, then draws.
fn settle(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.workspace().extensions_busy() {
        app.poll();
        assert!(Instant::now() < deadline, "extension work did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll();
    terminal.draw(|frame| app.draw(frame)).expect("draw");
}

fn publish(stand_in: &StandIn) {
    let mut hello = Fixture::new("acme", "hello", "1.2.3");
    hello.display_name = "Hello World".into();
    let mut rainbow = Fixture::new("acme", "rainbow", "0.4.0");
    rainbow.display_name = "Rainbow Brackets".into();
    stand_in.publish(&hello);
    stand_in.publish(&rainbow);
    stand_in.search_results(&[hello, rainbow]);
}

#[test]
fn ctrl_shift_x_shows_the_view_and_a_search_lists_results() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut app, _) = app("search", &stand_in);
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    settle(&mut app, &mut terminal);
    assert_snapshot("extensions-empty-100x14", &screen(&terminal));
    type_text(&mut app, &mut terminal, "hello");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    assert_snapshot("extensions-results-100x14", &screen(&terminal));
}

#[test]
fn enter_installs_and_the_installed_list_says_not_loaded() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut app, extensions) = app("install", &stand_in);
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    type_text(&mut app, &mut terminal, "hello");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Enter)],
    );
    settle(&mut app, &mut terminal);
    assert!(
        extensions
            .join("acme.hello-1.2.3-universal/package.json")
            .exists()
    );
    assert_snapshot("extensions-installed-100x14", &screen(&terminal));

    // The installed row is the third: two results, then Installed.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::End), key(KeyCode::Delete)],
    );
    settle(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains("No extensions installed"), "{shown}");
    assert!(shown.contains("Uninstalled acme.hello"), "{shown}");
    assert!(!extensions.join("acme.hello-1.2.3-universal").exists());
}

#[test]
fn ctrl_shift_x_again_and_escape_go_back_to_the_editor() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut app, _) = app("focus", &stand_in);
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    settle(&mut app, &mut terminal);
    send(
        &mut app,
        &mut terminal,
        &[ctrl_shift_x(), key(KeyCode::Char('a'))],
    );
    let text = |app: &App| {
        let id = app.document().expect("document");
        app.workspace().document(id).expect("doc").text()
    };
    assert_eq!(text(&app), "ahello\n", "typing goes to the editor again");
    send(
        &mut app,
        &mut terminal,
        &[ctrl_shift_x(), key(KeyCode::Char('b'))],
    );
    assert_eq!(text(&app), "ahello\n", "typing goes to the search box");
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Esc), key(KeyCode::Char('c'))],
    );
    assert_eq!(text(&app), "achello\n");
    // The palette reaches the view too, and the explorer key leaves it.
    send(
        &mut app,
        &mut terminal,
        &[Event::Key(KeyEvent::new(
            KeyCode::Char('E'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))],
    );
    assert!(!screen(&terminal).contains("EXTENSIONS"));
}

#[test]
fn a_failed_install_is_reported_in_the_status_bar() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let hello = Fixture::new("acme", "hello", "1.2.3");
    stand_in.search_results(std::slice::from_ref(&hello));
    let (mut app, _) = app("failed", &stand_in);
    let mut terminal = Terminal::new(TestBackend::new(160, 12)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    send(
        &mut app,
        &mut terminal,
        &[Event::Paste("hello\nignored".into())],
    );
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Enter)],
    );
    settle(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains("acme.hello: "), "{shown}");
    assert!(shown.contains("HTTP 404"), "{shown}");
}

#[test]
fn enter_disables_and_enables_an_installed_extension() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut app, extensions) = app("enable", &stand_in);
    let state = extensions
        .with_file_name("globalStorage")
        .join("state.json");
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    type_text(&mut app, &mut terminal, "hello");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Enter)],
    );
    settle(&mut app, &mut terminal);
    // The installed row is the third: two results, then Installed.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::End), key(KeyCode::Enter)],
    );
    settle(&mut app, &mut terminal);
    assert_snapshot("extensions-disabled-100x14", &screen(&terminal));
    assert!(
        std::fs::read_to_string(&state)
            .expect("state")
            .contains("acme.hello")
    );

    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(
        shown.contains("Enabled · Not loaded in this phase"),
        "{shown}"
    );
    assert!(
        shown.contains("Enabled acme.hello (not loaded in this phase)"),
        "{shown}"
    );
    assert_eq!(std::fs::read_to_string(&state).expect("state"), "{}");
}

#[test]
fn an_invalid_extension_says_why_on_enter() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut app, extensions) = app("invalid", &stand_in);
    let folder = extensions.join("acme.broken-1.0.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("package.json"),
        r#"{"name":"broken","displayName":"Broken","publisher":"acme","version":"1.0.0"}"#,
    )
    .expect("manifest");
    std::fs::write(
        extensions.join("extensions.json"),
        r#"[{"identifier":{"id":"acme.broken"},"version":"1.0.0","location":{"$mid":1,"path":"/x","scheme":"file"},"relativeLocation":"acme.broken-1.0.0"}]"#,
    )
    .expect("index");
    let mut terminal = Terminal::new(TestBackend::new(120, 12)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    settle(&mut app, &mut terminal);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Enter)],
    );
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    let shown = screen(&terminal);
    assert!(shown.contains(" Broken 1.0.0"), "{shown}");
    assert!(
        shown.contains("Invalid · Not loaded in this phase"),
        "{shown}"
    );
    assert!(
        shown.contains("Broken is invalid: property `engines` is mandatory"),
        "{shown}"
    );
    assert!(!app.workspace().extensions_busy(), "nothing was started");
}

/// `App::handle` polls the workers after every key, so a fast job can
/// finish inside the key's own step and no key-driven test may expect to see
/// its in-flight label. The workspace polls only when asked, and a draw
/// never polls, so starting the job through the workspace shows the label
/// deterministically.
#[test]
fn a_running_operation_is_labelled_until_it_is_polled() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut app, _) = app("running", &stand_in);
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    type_text(&mut app, &mut terminal, "hello");
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    settle(&mut app, &mut terminal);
    let drawn = |app: &mut App, terminal: &mut Terminal<TestBackend>| {
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        screen(terminal)
    };
    app.workspace_mut()
        .install_extension("acme.hello")
        .expect("install");
    let shown = drawn(&mut app, &mut terminal);
    assert!(shown.contains("Installing"), "{shown}");
    settle(&mut app, &mut terminal);
    app.workspace_mut()
        .disable_extension("acme.hello")
        .expect("disable");
    let shown = drawn(&mut app, &mut terminal);
    assert!(shown.contains("Disabling"), "{shown}");
    settle(&mut app, &mut terminal);
    app.workspace_mut()
        .uninstall_extension("acme.hello")
        .expect("uninstall");
    let shown = drawn(&mut app, &mut terminal);
    assert!(shown.contains("Uninstalling"), "{shown}");
    settle(&mut app, &mut terminal);
}

#[test]
fn an_unusable_obsolete_file_is_reported_in_the_status_bar_by_a_listing() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut app, extensions) = app("obsolete", &stand_in);
    std::fs::create_dir_all(&extensions).expect("extensions folder");
    std::fs::write(extensions.join(".obsolete"), "{ not json").expect(".obsolete");
    let mut terminal = Terminal::new(TestBackend::new(200, 12)).expect("terminal");
    send(&mut app, &mut terminal, &[ctrl_shift_x()]);
    settle(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(shown.contains(".obsolete"), "{shown}");
    assert!(shown.contains("left unchanged"), "{shown}");
    assert_eq!(
        std::fs::read_to_string(extensions.join(".obsolete")).expect("read"),
        "{ not json"
    );
}
