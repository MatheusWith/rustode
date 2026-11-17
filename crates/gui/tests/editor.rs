//! Input to frame: key, mouse and wheel events go into the window, and the
//! AccessKit tree, the window title and image snapshots come out. These tests
//! cover presentation only; the editing logic is tested at the workspace seam
//! (`crates/workspace/tests/`).
//!
//! Snapshots live in `tests/snapshots/`. Regenerate them with
//! `UPDATE_SNAPSHOTS=1 cargo nextest run -p rustode-gui`.

use eframe::egui::{
    self, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, ViewportId,
    accesskit::Role,
};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::{EditorApp, LINE_HEIGHT, STATUS_BAR_HEIGHT};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 320.0;

fn fixture(name: &str, file: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-gui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(file);
    std::fs::write(&path, text).expect("fixture");
    path
}

fn open(path: &Path) -> Harness<'static, EditorApp> {
    let app = EditorApp::open(path).expect("open");
    let mut harness = Harness::builder()
        .with_size([WIDTH, HEIGHT])
        .build_eframe(move |_| app);
    harness.run();
    harness
}

fn open_text(name: &str, text: &str) -> Harness<'static, EditorApp> {
    open(&fixture(name, "f.txt", text))
}

fn editor<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::MultilineTextInput)
}

/// The text the editor exposes to screen readers: its visible lines.
fn shown(harness: &Harness<'_, EditorApp>) -> String {
    editor(harness).accesskit_node().document_range().text()
}

/// The caret as (line within the exposed lines, char index in the exposed text).
fn caret(harness: &Harness<'_, EditorApp>) -> (usize, usize) {
    let node = editor(harness).accesskit_node();
    let focus = node
        .text_selection_focus()
        .expect("the editor exposes a caret");
    (focus.to_line_index(), focus.to_global_usv_index())
}

/// The exposed selection as char indices (anchor, focus).
fn selection(harness: &Harness<'_, EditorApp>) -> (usize, usize) {
    let node = editor(harness).accesskit_node();
    let anchor = node.text_selection_anchor().expect("anchor");
    let focus = node.text_selection_focus().expect("focus");
    (anchor.to_global_usv_index(), focus.to_global_usv_index())
}

fn exposed_lines(harness: &Harness<'_, EditorApp>) -> usize {
    editor(harness)
        .children()
        .filter(|run| run.accesskit_node().role() == Role::TextRun)
        .count()
}

fn type_text(harness: &mut Harness<'_, EditorApp>, text: &str) {
    harness.event(Event::Text(text.to_owned()));
    harness.run();
}

fn press(harness: &mut Harness<'_, EditorApp>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run();
}

fn click_at(harness: &mut Harness<'_, EditorApp>, pos: Pos2, modifiers: Modifiers) {
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    harness.run();
}

/// The status bar items, in the order they are drawn.
fn status(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Label)
        .filter_map(|label| label.value())
        .collect()
}

#[track_caller]
fn assert_status(harness: &Harness<'_, EditorApp>, item: &str) {
    let items = status(harness);
    assert!(
        items.iter().any(|shown| shown == item),
        "the status bar shows {items:?}, not {item:?}"
    );
}

/// Lines of the editor area (window minus status bar): wholly visible, or
/// visible at least in part.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few positive rows
fn rows(partly: bool) -> usize {
    let rows = (HEIGHT - STATUS_BAR_HEIGHT) / LINE_HEIGHT;
    (if partly { rows.ceil() } else { rows.floor() }) as usize
}

fn visible_rows() -> usize {
    rows(true)
}

#[test]
fn opening_a_file_shows_it_with_the_caret_at_the_start() {
    let mut harness = open_text("open", "fn main() {\n    println!(\"hi\");\n}\n");
    let node = editor(&harness);
    assert_eq!(node.accesskit_node().label().as_deref(), Some("f.txt"));
    assert!(node.is_focused(), "the editor takes keyboard focus");
    assert_eq!(shown(&harness), "fn main() {\n    println!(\"hi\");\n}\n");
    assert_eq!(caret(&harness), (0, 0));
    assert_eq!(
        status(&harness),
        ["Ln 1, Col 1", "UTF-8", "LF", "Plain Text"],
        "the status bar, left to right"
    );
    assert_eq!(harness.state().window_title(), "f.txt - rustode");
    harness.snapshot("open");
}

#[test]
fn typed_text_appears_at_the_caret_and_marks_the_document_dirty() {
    let mut harness = open_text("type", "world");
    type_text(&mut harness, "hello ");
    assert_eq!(shown(&harness), "hello world");
    assert_eq!(caret(&harness), (0, 6));
    assert_status(&harness, "Ln 1, Col 7");
    assert_eq!(harness.state().window_title(), "\u{25cf} f.txt - rustode");
}

#[test]
fn backspace_delete_enter_and_tab_edit_the_text() {
    let mut harness = open_text("keys", "abc");
    press(&mut harness, Modifiers::NONE, Key::End);
    press(&mut harness, Modifiers::NONE, Key::Backspace);
    press(&mut harness, Modifiers::NONE, Key::Home);
    press(&mut harness, Modifiers::NONE, Key::Delete);
    assert_eq!(shown(&harness), "b");
    press(&mut harness, Modifiers::NONE, Key::Tab);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(
        shown(&harness),
        "    \n    b",
        "Enter keeps the indentation"
    );
    assert_eq!(caret(&harness), (1, 9));
    assert_status(&harness, "Ln 2, Col 5");
}

#[test]
fn shift_with_arrows_home_and_end_selects() {
    let mut harness = open_text("select", "one two\nthree");
    press(&mut harness, Modifiers::SHIFT, Key::ArrowRight);
    press(&mut harness, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(selection(&harness), (0, 2));
    press(&mut harness, Modifiers::SHIFT, Key::End);
    assert_eq!(selection(&harness), (0, 7));
    press(&mut harness, Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(selection(&harness), (0, 13));
    press(&mut harness, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(selection(&harness), (0, 0), "Left collapses to the start");
    press(&mut harness, Modifiers::COMMAND, Key::End);
    assert_eq!(caret(&harness), (1, 13), "Ctrl+End goes to the end");
    press(
        &mut harness,
        Modifiers::COMMAND | Modifiers::SHIFT,
        Key::Home,
    );
    assert_eq!(
        selection(&harness),
        (13, 0),
        "Ctrl+Shift+Home selects to the start"
    );
}

#[test]
fn ctrl_a_selects_everything_and_escape_collapses() {
    let mut harness = open_text("select-all", "ab\ncd");
    press(&mut harness, Modifiers::COMMAND, Key::A);
    assert_eq!(selection(&harness), (0, 5));
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert_eq!(selection(&harness), (5, 5));
    assert!(editor(&harness).is_focused(), "Escape keeps the focus");
}

#[test]
fn ctrl_z_undoes_and_ctrl_shift_z_and_ctrl_y_redo() {
    let mut harness = open_text("undo", "");
    type_text(&mut harness, "abc");
    press(&mut harness, Modifiers::COMMAND, Key::Z);
    assert_eq!(shown(&harness), "");
    assert_eq!(
        harness.state().window_title(),
        "f.txt - rustode",
        "clean again"
    );
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(shown(&harness), "abc");
    press(&mut harness, Modifiers::COMMAND, Key::Z);
    press(&mut harness, Modifiers::COMMAND, Key::Y);
    assert_eq!(shown(&harness), "abc");
}

#[test]
fn ctrl_s_writes_the_file_and_clears_the_dirty_mark() {
    let path = fixture("save", "notes.md", "draft");
    let mut harness = open(&path);
    press(&mut harness, Modifiers::NONE, Key::End);
    type_text(&mut harness, " two");
    assert_eq!(
        harness.state().window_title(),
        "\u{25cf} notes.md - rustode"
    );
    press(&mut harness, Modifiers::COMMAND, Key::S);
    assert_eq!(std::fs::read_to_string(&path).expect("saved"), "draft two");
    assert_eq!(harness.state().window_title(), "notes.md - rustode");
    assert_status(&harness, "Markdown");
}

#[test]
fn a_failed_save_is_reported_in_the_status_bar() {
    let path = fixture("save-error", "f.txt", "x");
    let mut harness = open(&path);
    type_text(&mut harness, "y");
    std::fs::remove_dir_all(path.parent().expect("dir")).expect("remove the folder");
    press(&mut harness, Modifiers::COMMAND, Key::S);
    let items = status(&harness);
    assert!(items[0].starts_with("Save failed: "), "{items:?}");
    assert_eq!(harness.state().window_title(), "\u{25cf} f.txt - rustode");
}

fn numbered_lines(count: usize) -> String {
    use std::fmt::Write as _;
    (0..count).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "line {n}");
        text
    })
}

#[test]
fn only_the_visible_lines_are_exposed() {
    let mut harness = open_text("virtual", &numbered_lines(10_000));
    assert_eq!(exposed_lines(&harness), visible_rows());
    let text = shown(&harness);
    assert!(text.starts_with("line 0\nline 1\n"));
    assert!(!text.contains("line 100\n"));
    press(&mut harness, Modifiers::COMMAND, Key::End);
    assert_eq!(
        exposed_lines(&harness),
        visible_rows(),
        "the caret's line is revealed"
    );
    assert!(
        shown(&harness).ends_with("line 9999\n"),
        "{}",
        shown(&harness)
    );
    harness.snapshot("end_of_a_long_file");
}

#[test]
fn page_down_moves_by_the_page_and_reveals_the_caret() {
    let mut harness = open_text("page", &numbered_lines(200));
    let page = rows(false) - 2;
    press(&mut harness, Modifiers::NONE, Key::PageDown);
    assert_status(&harness, &format!("Ln {}, Col 1", page + 1));
    for _ in 0..3 {
        press(&mut harness, Modifiers::NONE, Key::PageDown);
    }
    let line = 4 * page;
    let text = shown(&harness);
    assert!(
        text.ends_with(&format!("\nline {line}")),
        "the caret's line at the bottom: {text}"
    );
    press(&mut harness, Modifiers::SHIFT, Key::PageUp);
    assert_status(
        &harness,
        &format!("Ln {}, Col 1 ({} selected)", line - page + 1, page * 8),
    );
}

#[test]
fn the_mouse_wheel_scrolls_without_moving_the_caret() {
    let mut harness = open_text("wheel", &numbered_lines(200));
    harness.hover_at(Pos2::new(200.0, 100.0));
    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -10.0 * LINE_HEIGHT),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    assert!(
        shown(&harness).starts_with("line 10\n"),
        "{}",
        shown(&harness)
    );
    assert_status(&harness, "Ln 1, Col 1");
}

#[test]
fn clicking_places_the_caret_and_shift_click_selects() {
    let mut harness = open_text("click", "first line\nsecond line\nthird");
    let rect = editor(&harness).rect();
    let second_line = rect.top() + 1.5 * LINE_HEIGHT;
    click_at(
        &mut harness,
        Pos2::new(rect.right() - 2.0, second_line),
        Modifiers::NONE,
    );
    assert_status(&harness, "Ln 2, Col 12");
    let third_line = rect.top() + 2.5 * LINE_HEIGHT;
    click_at(
        &mut harness,
        Pos2::new(rect.right() - 2.0, third_line),
        Modifiers::SHIFT,
    );
    assert_eq!(selection(&harness), (22, 28));
}

#[test]
fn tabs_reach_the_next_tab_stop() {
    let mut harness = open(&fixture("tabs", "Makefile", "a\tb\nab\tc\n\tabc\td\n"));
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::End);
    press(&mut harness, Modifiers::NONE, Key::ArrowLeft);
    assert_status(&harness, "Ln 2, Col 5");
    assert_status(&harness, "Makefile");
    assert_eq!(
        shown(&harness),
        "a\tb\nab\tc\n\tabc\td\n",
        "the text keeps its tabs"
    );
    harness.snapshot("tabs");
}

#[test]
fn the_status_bar_counts_selected_chars_and_cursors() {
    let mut harness = open_text("cursors", "ab\ncd\nef");
    press(&mut harness, Modifiers::COMMAND, Key::A);
    assert_status(&harness, "Ln 3, Col 3 (8 selected)");
    press(&mut harness, Modifiers::COMMAND, Key::Home);
    press(
        &mut harness,
        Modifiers::SHIFT | Modifiers::ALT,
        Key::ArrowDown,
    );
    press(
        &mut harness,
        Modifiers::SHIFT | Modifiers::ALT,
        Key::ArrowDown,
    );
    assert_status(&harness, "3 selections");
    type_text(&mut harness, "-");
    assert_eq!(shown(&harness), "-ab\n-cd\n-ef");
    harness.snapshot("three_cursors");
    press(&mut harness, Modifiers::NONE, Key::Escape);
    let position = &status(&harness)[0];
    assert!(
        position.starts_with("Ln ") && position.ends_with(", Col 2"),
        "Escape leaves one cursor: {position}"
    );
}

#[test]
fn ctrl_d_selects_the_word_then_its_next_occurrence() {
    let mut harness = open_text("ctrl-d", "foo bar foo");
    press(&mut harness, Modifiers::COMMAND, Key::D);
    assert_eq!(selection(&harness), (0, 3));
    press(&mut harness, Modifiers::COMMAND, Key::D);
    assert_status(&harness, "2 selections (6 characters selected)");
}

#[test]
fn resizing_the_window_changes_how_many_lines_are_shown() {
    let mut harness = open_text("resize", &numbered_lines(100));
    harness.set_size(Vec2::new(WIDTH, STATUS_BAR_HEIGHT + 5.0 * LINE_HEIGHT));
    harness.run();
    assert_eq!(exposed_lines(&harness), 5);
    assert_eq!(shown(&harness), "line 0\nline 1\nline 2\nline 3\nline 4");
}

#[test]
fn a_selection_and_the_caret_are_drawn() {
    let mut harness = open_text("drawn", "let answer = 42;\nlet question = None;\n");
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::SHIFT, Key::End);
    harness.snapshot("selection");
}

#[test]
fn the_window_settles_after_input_and_schedules_no_frame() {
    let mut harness = open_text("idle", "abc");
    let idle = |harness: &Harness<'_, EditorApp>| {
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay
    };
    assert_eq!(
        idle(&harness),
        Duration::MAX,
        "nothing scheduled after opening"
    );
    type_text(&mut harness, "x");
    press(&mut harness, Modifiers::SHIFT, Key::ArrowLeft);
    harness.hover_at(Pos2::new(100.0, 50.0));
    harness.run();
    assert_eq!(
        idle(&harness),
        Duration::MAX,
        "nothing scheduled after input"
    );
}

#[test]
fn a_file_changed_on_disk_is_shown_on_the_next_frame() {
    let path = fixture("external", "f.txt", "old");
    let mut harness = open(&path);
    std::fs::write(&path, "new text").expect("external change");
    for _ in 0..150 {
        harness.step();
        if shown(&harness) == "new text" {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the reload was not shown: {:?}", shown(&harness));
}

#[test]
fn ime_commits_are_typed() {
    let mut harness = open_text("ime", "");
    harness.event(Event::Ime(egui::ImeEvent::Commit("çé".to_owned())));
    harness.run();
    assert_eq!(shown(&harness), "çé");
}
