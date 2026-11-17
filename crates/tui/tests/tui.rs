//! Input to frame: keys, pastes and resizes go into the TUI, and the cells
//! it draws come out of ratatui's test backend. Each width band has a text
//! snapshot in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1` to
//! write them, then review the diff.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use rustode_tui::{App, ColorMode};
use std::fmt::Write as _;
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
    app.set_color_mode(ColorMode::None);
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

/// The screen with each row's style runs: `x0-x1 fg/bg modifiers`.
fn styled_screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut row = String::new();
        let mut runs: Vec<(u16, u16, String)> = Vec::new();
        let mut hidden = 0;
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            if hidden > 0 {
                hidden -= 1;
            } else {
                row.push_str(cell.symbol());
                hidden = cell.symbol().width().saturating_sub(1);
            }
            let style = format!("{:?}/{:?} {:?}", cell.fg, cell.bg, cell.modifier);
            match runs.last_mut() {
                Some(last) if last.2 == style => last.1 = x,
                _ => runs.push((x, x, style)),
            }
        }
        let _ = writeln!(out, "{:>2}|{}", y, row.trim_end());
        for (start, end, style) in runs {
            let _ = writeln!(out, "   {start:>3}-{end:<3} {style}");
        }
    }
    out
}

#[test]
fn each_color_mode_has_its_styles() {
    for (name, mode) in [
        ("truecolor", ColorMode::TrueColor),
        ("256", ColorMode::Ansi256),
        ("16", ColorMode::Ansi16),
        ("none", ColorMode::None),
    ] {
        let (mut app, _) = open_sample(&format!("colors-{name}"));
        app.set_color_mode(mode);
        let mut terminal = terminal(70, 10);
        let shift_right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        send(&mut app, &mut terminal, &[shift_right.clone(), shift_right]);
        assert_snapshot(&format!("colors-{name}-70x10"), &styled_screen(&terminal));
    }
}

#[test]
fn truecolor_selection_uses_the_dark_modern_selection_color() {
    let (mut app, _) = open_sample("truecolor-selection");
    app.set_color_mode(ColorMode::TrueColor);
    let mut terminal = terminal(100, 12);
    let shift_right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    send(&mut app, &mut terminal, &[shift_right]);
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(5, 1)].bg,
        ratatui::style::Color::Rgb(0x26, 0x4F, 0x78)
    );
    assert_eq!(
        buffer[(6, 1)].bg,
        ratatui::style::Color::Rgb(0x1F, 0x1F, 0x1F)
    );
}

fn mouse(
    kind: ratatui::crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
    modifiers: KeyModifiers,
) -> Event {
    Event::Mouse(ratatui::crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn selections(app: &App) -> Vec<rustode_workspace::Selection> {
    app.workspace()
        .document(app.document().expect("doc"))
        .expect("doc")
        .selections()
        .to_vec()
}

#[test]
fn clicks_place_the_cursor_and_drags_select() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use rustode_workspace::Selection;
    let (mut app, _) = open_sample("mouse-click");
    let mut terminal = terminal(100, 12);
    draw(&mut app, &mut terminal);
    // Row 2 is line 2 ("    println!…"); the text starts at column 5 after the gutter.
    let none = KeyModifiers::NONE;
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Down(MouseButton::Left), 9, 2, none)],
    );
    assert_eq!(
        selections(&app),
        [Selection::cursor(16)],
        "line 2, column 4"
    );
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Drag(MouseButton::Left), 12, 2, none)],
    );
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Up(MouseButton::Left), 12, 2, none)],
    );
    assert_eq!(selections(&app), [Selection::new(16, 19)]);
    send(
        &mut app,
        &mut terminal,
        &[mouse(
            MouseEventKind::Down(MouseButton::Left),
            5,
            1,
            KeyModifiers::ALT,
        )],
    );
    assert_eq!(
        selections(&app),
        [Selection::cursor(0), Selection::new(16, 19)],
        "Alt+click adds a cursor"
    );
    // Past the end of the text: the end of the document.
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Down(MouseButton::Left), 30, 9, none)],
    );
    assert_eq!(
        selections(&app),
        [Selection::cursor(
            app.workspace()
                .document(app.document().expect("doc"))
                .expect("doc")
                .len_chars()
        )]
    );
}

#[test]
fn the_wheel_scrolls_without_moving_the_cursor() {
    use ratatui::crossterm::event::MouseEventKind;
    let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
    let path = fixture("wheel", "long.txt", &(lines.join("\n") + "\n"));
    let mut app = App::open(&path).expect("open");
    let mut terminal = terminal(100, 12);
    draw(&mut app, &mut terminal);
    let none = KeyModifiers::NONE;
    send(
        &mut app,
        &mut terminal,
        &[
            mouse(MouseEventKind::ScrollDown, 20, 5, none),
            mouse(MouseEventKind::ScrollDown, 20, 5, none),
        ],
    );
    let shown = screen(&terminal);
    assert!(
        shown.contains("  7 line 7") && !shown.contains("  1 line 1"),
        "{shown}"
    );
    assert!(shown.contains("Ln 1, Col 1"), "the cursor stays: {shown}");
    send(&mut app, &mut terminal, &[key(KeyCode::Down)]);
    assert!(
        screen(&terminal).contains("  2 line 2"),
        "moving the cursor (now on line 2) brings it back into view"
    );
}

#[test]
fn clicking_a_tab_or_an_explorer_row_opens_it() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let root = project("mouse-tabs");
    let mut app = App::open_paths(&[
        root.clone(),
        root.join("README.md"),
        root.join("src/main.rs"),
    ])
    .expect("open");
    let mut terminal = terminal(130, 14);
    draw(&mut app, &mut terminal);
    let none = KeyModifiers::NONE;
    // Tabs start after the 25-column sidebar: " README.md │ main.rs │".
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Down(MouseButton::Left), 28, 0, none)],
    );
    assert!(
        screen(&terminal).contains("Markdown"),
        "{}",
        screen(&terminal)
    );
    // Explorer rows start on screen row 2: "▸ src", then "README.md".
    send(
        &mut app,
        &mut terminal,
        &[mouse(MouseEventKind::Down(MouseButton::Left), 4, 2, none)],
    );
    assert!(
        screen(&terminal).contains("▾ src"),
        "a click toggles a folder: {}",
        screen(&terminal)
    );
}

#[test]
fn user_keybindings_add_remove_and_respect_when() {
    let (mut app, path) = open_sample("user-keys");
    let dir = path.parent().expect("dir").join("User");
    std::fs::create_dir_all(&dir).expect("user dir");
    std::fs::write(
        dir.join("keybindings.json"),
        r#"[
            // Ctrl+S no longer saves; Ctrl+Alt+S does.
            { "key": "ctrl+s", "command": "-workbench.action.files.save" },
            { "key": "ctrl+alt+s", "command": "workbench.action.files.save" },
            { "key": "ctrl+k", "command": "cursorDown", "when": "editorTextFocus && editorLangId == rust" },
            { "key": "ctrl+j", "command": "cursorDown", "when": "filesExplorerFocus" },
        ]"#,
    )
    .expect("keybindings");
    app.load_user_config(&dir);
    let mut terminal = terminal(100, 12);
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Char('x')), ctrl('s')],
    );
    assert!(
        std::fs::read_to_string(&path)
            .expect("read")
            .starts_with("fn main"),
        "Ctrl+S was removed"
    );
    let ctrl_alt_s = Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    send(&mut app, &mut terminal, &[ctrl_alt_s]);
    assert!(
        std::fs::read_to_string(&path)
            .expect("read")
            .starts_with("xfn main"),
        "Ctrl+Alt+S saves"
    );
    send(&mut app, &mut terminal, &[ctrl('k')]);
    assert!(screen(&terminal).contains("Ln 2,"), "{}", screen(&terminal));
    send(&mut app, &mut terminal, &[ctrl('j')]);
    assert!(
        screen(&terminal).contains("Ln 2,"),
        "the when clause is false in the editor"
    );
}
