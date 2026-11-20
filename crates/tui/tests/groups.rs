//! Input to frame for editor groups: splits right and down, one document in
//! two panes, focus by key and by click, and the width bands. At 60
//! columns or more two groups sit side by side; where they would be
//! narrower than 30 columns only the group with the focus is shown.
//! Snapshots are in `tests/snapshots/`; run with `RUSTODE_UPDATE_SNAPSHOTS=1`
//! to write them, then review the diff.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rustode_tui::{App, ColorMode};
use rustode_workspace::Selection;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

const TEXT: &str = "first line\nsecond line\nthird line\nfourth line\n";

fn app(name: &str) -> App {
    let dir =
        std::env::temp_dir().join(format!("rustode-tui-groups-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("notes.txt"), TEXT).expect("fixture");
    std::fs::write(dir.join("other.txt"), "other file\n").expect("fixture");
    App::open_paths(&[dir.join("notes.txt")]).expect("open")
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

fn ctrl(c: char) -> Event {
    key(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn plain(c: char) -> Event {
    key(KeyCode::Char(c), KeyModifiers::NONE)
}

fn send(app: &mut App, terminal: &mut Terminal<TestBackend>, events: &[Event]) {
    for event in events {
        app.step(terminal, event).expect("step");
    }
}

fn click(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// Ctrl+\ then a few keys typed in the new group.
fn split_and_type(name: &str, width: u16, height: u16) -> (App, Terminal<TestBackend>) {
    let mut app = app(name);
    let mut terminal = terminal(width, height);
    send(
        &mut app,
        &mut terminal,
        &[
            ctrl('\\'),
            key(KeyCode::Down, KeyModifiers::NONE),
            key(KeyCode::Down, KeyModifiers::NONE),
        ],
    );
    (app, terminal)
}

#[test]
fn each_width_band_has_its_groups() {
    for (name, width, height) in [
        ("groups-wide-130x12", 130, 12),
        ("groups-medium-100x12", 100, 12),
        ("groups-narrow-70x12", 70, 12),
        ("groups-editor-only-50x12", 50, 12),
    ] {
        let (_, terminal) = split_and_type(&format!("band-{name}"), width, height);
        assert_snapshot(name, &screen(&terminal));
    }
}

#[test]
fn a_split_down_stacks_the_groups() {
    let mut app = app("down");
    let mut terminal = terminal(100, 16);
    app.workspace_mut()
        .execute("workbench.action.splitEditorDown")
        .expect("split");
    send(
        &mut app,
        &mut terminal,
        &[key(KeyCode::Down, KeyModifiers::NONE)],
    );
    assert_snapshot("groups-down-100x16", &screen(&terminal));
}

#[test]
fn the_inactive_group_keeps_its_own_cursor_and_follows_edits_in_the_other() {
    let mut app = app("edit");
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[ctrl('\\')]);
    let ids = app.workspace().group_ids();
    let doc = app.workspace().active_document().expect("document");
    let right_cursor = app.workspace().view_cursor(ids[1], doc);
    assert_eq!(right_cursor, Some(0));
    // Type in the new (right) group: the left pane shows the text at once.
    send(&mut app, &mut terminal, &[plain('X'), plain('Y')]);
    let shown = screen(&terminal);
    let lines: Vec<&str> = shown
        .lines()
        .filter(|l| l.contains("XYfirst line"))
        .collect();
    assert_eq!(lines.len(), 1, "one row holds both panes: {shown}");
    assert_eq!(lines[0].matches("XYfirst line").count(), 2, "{shown}");
    // Both cursors were at the start; the one that typed advanced, and the
    // other followed the text it was on, to after what was inserted there.
    assert_eq!(app.workspace().view_cursor(ids[0], doc), Some(2));
    assert_eq!(app.workspace().view_cursor(ids[1], doc), Some(2));
}

#[test]
fn keys_move_the_focus_between_groups() {
    let mut app = app("focus");
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[ctrl('\\')]);
    let ids = app.workspace().group_ids();
    assert_eq!(app.workspace().active_group(), ids[1]);
    // Ctrl+K Ctrl+Left.
    send(
        &mut app,
        &mut terminal,
        &[ctrl('k'), key(KeyCode::Left, KeyModifiers::CONTROL)],
    );
    assert_eq!(app.workspace().active_group(), ids[0]);
    send(
        &mut app,
        &mut terminal,
        &[ctrl('k'), key(KeyCode::Right, KeyModifiers::CONTROL)],
    );
    assert_eq!(app.workspace().active_group(), ids[1]);
    // Ctrl+1 (extended keyboard protocol) and Ctrl+Alt+Left to move the editor.
    send(&mut app, &mut terminal, &[ctrl('1')]);
    assert_eq!(app.workspace().active_group(), ids[0]);
    send(
        &mut app,
        &mut terminal,
        &[key(
            KeyCode::Right,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        )],
    );
    assert_eq!(app.workspace().active_group(), ids[1]);
    assert_eq!(
        app.workspace().group_ids().len(),
        1,
        "the first group was left empty and closed"
    );
}

#[test]
fn a_click_focuses_the_group_and_places_the_cursor() {
    let mut app = app("click");
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[ctrl('\\')]);
    let ids = app.workspace().group_ids();
    let doc = app.workspace().active_document().expect("document");
    // The left group starts at column 0: gutter is 5 wide, the text follows.
    send(&mut app, &mut terminal, &[click(5 + 3, 2)]);
    assert_eq!(app.workspace().active_group(), ids[0]);
    assert_eq!(app.workspace().view_cursor(ids[0], doc), Some(11 + 3));
    // The right pane kept its cursor at the start.
    assert_eq!(app.workspace().view_cursor(ids[1], doc), Some(0));
    // A click on the other pane's tab bar focuses it.
    send(&mut app, &mut terminal, &[click(70, 0)]);
    assert_eq!(app.workspace().active_group(), ids[1]);
}

#[test]
fn ctrl_w_closes_the_editor_and_the_group_with_it() {
    let mut app = app("close");
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[ctrl('\\')]);
    assert_eq!(app.workspace().group_ids().len(), 2);
    send(&mut app, &mut terminal, &[ctrl('w')]);
    assert_eq!(
        app.workspace().group_ids().len(),
        1,
        "the emptied group closed"
    );
    assert!(
        app.workspace().active_document().is_some(),
        "the document is still open in the other group"
    );
}

#[test]
fn the_palette_lists_the_group_commands() {
    let mut app = app("palette");
    let mut terminal = terminal(100, 16);
    send(&mut app, &mut terminal, &[ctrl('p')]);
    let events: Vec<Event> = ">split editor".chars().map(plain).collect();
    // Ctrl+P opens quick open; typing '>' switches to commands.
    send(&mut app, &mut terminal, &events);
    let shown = screen(&terminal);
    assert!(shown.contains("View: Split Editor"), "{shown}");
    assert!(shown.contains("Ctrl+\\"), "{shown}");
}

#[test]
fn selections_of_the_other_group_are_drawn_in_its_pane() {
    let mut app = app("selection");
    app.set_color_mode(ColorMode::None);
    let mut terminal = terminal(100, 12);
    send(&mut app, &mut terminal, &[ctrl('\\')]);
    let doc = app.workspace().active_document().expect("document");
    let ids = app.workspace().group_ids();
    app.workspace_mut()
        .set_selections(doc, vec![Selection::new(0, 5)], 0)
        .expect("select in the right group");
    app.workspace_mut().focus_group(ids[0]).expect("left");
    terminal.draw(|f| app.draw(f)).expect("draw");
    let buffer = terminal.backend().buffer();
    let reversed = |x: u16| {
        buffer[(x, 1)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    };
    // Text starts after the 5-column gutter; the right pane begins at 53.
    let right_start = (0..100u16)
        .find(|&x| buffer[(x, 1)].symbol() == "│")
        .expect("divider")
        + 1;
    assert!(
        reversed(right_start + 5),
        "the right pane draws its own selection"
    );
    assert!(!reversed(5), "the left pane has no selection");
}
