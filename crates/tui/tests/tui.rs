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
    // Row 0 is the tab bar; the gutter takes 5 columns, so "fn" starts at (5, 1).
    assert!(buffer[(5, 1)].modifier.contains(Modifier::REVERSED));
    assert!(buffer[(6, 1)].modifier.contains(Modifier::REVERSED));
    assert!(!buffer[(7, 1)].modifier.contains(Modifier::REVERSED));
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
            .document(app.document().expect("active document"))
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
    // 10 text rows between the tab bar and the status bar: VS Code pages by
    // 10 - 2 = 8 lines.
    assert!(
        screen(&terminal).contains("Ln 9, Col 1"),
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

fn project(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("rustode-tui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // A fixed folder name keeps the explorer's root header stable in snapshots.
    let root = base.join("demo");
    for (file, text) in [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub mod x;\n"),
        ("README.md", "# demo\n"),
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, text).expect("fixture");
    }
    std::fs::create_dir_all(root.join(".git")).expect("git dir");
    root
}

fn shift(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT))
}

#[test]
fn a_folder_opens_in_the_explorer_and_files_open_in_tabs() {
    let root = project("explorer");
    let mut app = App::open(&root).expect("open folder");
    let mut terminal = terminal(130, 14);
    draw(&mut app, &mut terminal);
    // Folder rows first: src, then README.md. Expand src and open main.rs.
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Right), key(KeyCode::Down), key(KeyCode::Down)],
    );
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    assert_snapshot("explorer-130x14", &screen(&terminal));
    let shown = screen(&terminal);
    assert!(shown.contains("Ln 1, Col 1  UTF-8  LF  Rust"), "{shown}");
    // Back to the explorer with F6, open README.md with Space (focus stays).
    send(
        &mut app,
        &mut terminal,
        &[
            key(KeyCode::F(6)),
            key(KeyCode::Down),
            key(KeyCode::Char(' ')),
        ],
    );
    assert_eq!(app.workspace().tabs().len(), 2);
    assert!(
        screen(&terminal).contains("Markdown"),
        "{}",
        screen(&terminal)
    );
}

#[test]
fn ctrl_page_down_cycles_tabs_and_ctrl_w_closes() {
    let root = project("tabs");
    let paths = [root.join("src/main.rs"), root.join("README.md")];
    let mut app = App::open_paths(&paths).expect("open files");
    let mut terminal = terminal(100, 12);
    draw(&mut app, &mut terminal);
    assert!(
        screen(&terminal).contains("Markdown"),
        "the last file opened is active"
    );
    let ctrl_page_down = Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    send(&mut app, &mut terminal, &[ctrl_page_down]);
    assert!(screen(&terminal).contains("Rust"), "{}", screen(&terminal));
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Char('x')), ctrl('w')],
    );
    assert!(
        screen(&terminal).contains("Ctrl+W again closes"),
        "{}",
        screen(&terminal)
    );
    send(&mut app, &mut terminal, &[ctrl('w')]);
    assert_eq!(app.workspace().tabs().len(), 1);
    assert!(
        screen(&terminal).contains("Markdown"),
        "{}",
        screen(&terminal)
    );
}

#[test]
fn the_explorer_takes_the_screen_below_60_columns() {
    let root = project("narrow");
    let mut app = App::open(&root).expect("open folder");
    let mut terminal = terminal(50, 12);
    draw(&mut app, &mut terminal);
    let shown = screen(&terminal);
    assert!(
        shown.contains("▸ src") && shown.contains("README.md"),
        "{shown}"
    );
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down), key(KeyCode::Enter)],
    );
    assert!(
        screen(&terminal).contains("# demo"),
        "Enter opened README.md: {}",
        screen(&terminal)
    );
    let _ = shift(KeyCode::Tab);
}

fn typed(text: &str) -> Vec<Event> {
    text.chars().map(|c| key(KeyCode::Char(c))).collect()
}

#[test]
fn ctrl_p_finds_a_file_and_enter_opens_it() {
    let root = project("quick-open");
    let mut app = App::open(&root).expect("open folder");
    let mut terminal = terminal(100, 16);
    send(&mut app, &mut terminal, &[ctrl('p')]);
    send(&mut app, &mut terminal, &typed("main"));
    assert_snapshot("quick-open-100x16", &screen(&terminal));
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    let shown = screen(&terminal);
    assert!(
        shown.contains("fn main() {}") && shown.contains("Rust"),
        "{shown}"
    );
}

#[test]
fn f1_runs_a_command_from_the_palette() {
    let (mut app, path) = open_sample("palette");
    let mut terminal = terminal(100, 16);
    send(&mut app, &mut terminal, &[key(KeyCode::Char('x'))]);
    send(&mut app, &mut terminal, &[key(KeyCode::F(1))]);
    send(&mut app, &mut terminal, &typed("save"));
    assert_snapshot("palette-100x16", &screen(&terminal));
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    assert!(
        std::fs::read_to_string(&path)
            .expect("read")
            .starts_with("xfn main")
    );
    assert!(screen(&terminal).contains("Saved"), "{}", screen(&terminal));
}

#[test]
fn ctrl_g_goes_to_a_line_and_escape_cancels() {
    let (mut app, _) = open_sample("goto");
    let mut terminal = terminal(100, 16);
    send(&mut app, &mut terminal, &[ctrl('g')]);
    send(&mut app, &mut terminal, &typed("3"));
    assert!(
        screen(&terminal).contains("Go to line 3"),
        "{}",
        screen(&terminal)
    );
    send(&mut app, &mut terminal, &[key(KeyCode::Enter)]);
    assert!(
        screen(&terminal).contains("Ln 3, Col 1"),
        "{}",
        screen(&terminal)
    );
    send(
        &mut app,
        &mut terminal,
        &[ctrl('p'), key(KeyCode::Esc), key(KeyCode::Char('y'))],
    );
    let text = app
        .workspace()
        .document(app.document().expect("doc"))
        .expect("doc")
        .text();
    assert!(
        text.ends_with("}\n") && text.contains("\ny}"),
        "typing goes to the editor again: {text:?}"
    );
}
