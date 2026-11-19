//! The layout as the window changes size: the sidebar's width follows VS
//! Code's grid and collapses below 390 points, the status bar drops the
//! items that no longer fit and goes when the window is too short. Read
//! from the AccessKit tree's bounds and image snapshots.

use super::{open_paths, press, quiet, status, temp_dir, type_text};
use eframe::egui::{Key, Modifiers, Vec2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use rustode_gui::layout::{EDITOR_MIN_WIDTH, SIDEBAR_BREAKPOINT, SIDEBAR_MIN_WIDTH};
use std::path::PathBuf;

const HEIGHT: f32 = 300.0;

/// A folder named `demo` with `main.rs`, and that file open.
fn folder_and_file(name: &str) -> Vec<PathBuf> {
    let root = temp_dir(name).join("demo");
    std::fs::create_dir_all(&root).expect("dir");
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").expect("fixture");
    vec![root, file]
}

fn resize(harness: &mut Harness<'_, EditorApp>, width: f32) {
    harness.set_size(Vec2::new(width, HEIGHT));
    quiet(harness);
}

/// Where the editor area starts: the sidebar's width, or 0 without it.
fn editor_left(harness: &Harness<'_, EditorApp>) -> f32 {
    harness.get_by_role(Role::MultilineTextInput).rect().left()
}

fn sidebar_shown(harness: &Harness<'_, EditorApp>) -> bool {
    harness.query_by_role(Role::Tree).is_some()
}

#[track_caller]
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.5,
        "{actual} points, not {expected}"
    );
}

#[test]
fn the_sidebar_starts_at_a_quarter_of_the_window_from_170_to_300_points() {
    for (width, sidebar) in [(1600.0, 300.0), (800.0, 200.0), (480.0, SIDEBAR_MIN_WIDTH)] {
        let harness = open_paths(&folder_and_file("start"), Vec2::new(width, HEIGHT));
        assert_close(editor_left(&harness), sidebar);
    }
}

#[test]
fn a_narrowing_window_shrinks_the_editor_to_its_minimum_before_the_sidebar() {
    let mut harness = open_paths(&folder_and_file("shrink"), Vec2::new(1200.0, HEIGHT));
    assert_close(editor_left(&harness), 300.0);
    resize(&mut harness, 600.0);
    assert_close(editor_left(&harness), 300.0);
    resize(&mut harness, 450.0);
    assert_close(editor_left(&harness), 450.0 - EDITOR_MIN_WIDTH);
    resize(&mut harness, 400.0);
    assert_close(editor_left(&harness), 400.0 - EDITOR_MIN_WIDTH);
    resize(&mut harness, 1200.0);
    assert_close(editor_left(&harness), 400.0 - EDITOR_MIN_WIDTH);
    harness.snapshot("layout_1200_after_shrinking");
}

#[test]
fn below_390_points_the_sidebar_collapses_and_comes_back_wider() {
    assert_close(SIDEBAR_BREAKPOINT, 390.0);
    let mut harness = open_paths(&folder_and_file("collapse"), Vec2::new(800.0, HEIGHT));
    assert!(sidebar_shown(&harness));
    resize(&mut harness, 389.0);
    assert!(!sidebar_shown(&harness), "collapsed");
    assert_close(editor_left(&harness), 0.0);
    assert!(
        harness.get_by_role(Role::MultilineTextInput).is_focused(),
        "the editor keeps the focus"
    );
    harness.snapshot("layout_389_collapsed");
    resize(&mut harness, 390.0);
    assert!(sidebar_shown(&harness), "back at 390");
    assert_close(editor_left(&harness), SIDEBAR_MIN_WIDTH);
}

#[test]
fn a_narrow_window_starts_with_the_sidebar_collapsed() {
    let harness = open_paths(&folder_and_file("narrow"), Vec2::new(360.0, HEIGHT));
    assert!(!sidebar_shown(&harness));
    assert_close(editor_left(&harness), 0.0);
}

#[test]
fn asking_for_a_collapsed_sidebar_shows_it_until_the_window_crosses_390_again() {
    let mut harness = open_paths(&folder_and_file("reveal"), Vec2::new(360.0, HEIGHT));
    press(&mut harness, Modifiers::COMMAND, Key::B);
    assert!(sidebar_shown(&harness), "Ctrl+B shows it");
    assert_close(editor_left(&harness), SIDEBAR_MIN_WIDTH);
    resize(&mut harness, 300.0);
    assert!(
        sidebar_shown(&harness),
        "it stays while the window is narrow"
    );
    assert_close(editor_left(&harness), 300.0 / 2.0);
    press(&mut harness, Modifiers::COMMAND, Key::B);
    assert!(!sidebar_shown(&harness), "Ctrl+B hides it again");
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    assert!(sidebar_shown(&harness), "Ctrl+Shift+E shows it");
    assert!(
        harness
            .get_by_role(Role::Tree)
            .query_all_by_role(Role::TreeItem)
            .any(|row| row.is_focused()),
        "and focuses it"
    );
    resize(&mut harness, 600.0);
    resize(&mut harness, 380.0);
    assert!(!sidebar_shown(&harness), "crossing 390 collapses it again");
}

/// The status bar's right-hand items, left to right.
const ITEMS: [&str; 4] = ["Ln 1, Col 1", "UTF-8", "LF", "Plain Text"];

#[test]
fn status_bar_items_drop_from_the_left_as_the_window_narrows() {
    let dir = temp_dir("status-drop");
    let file = dir.join("notes.txt");
    std::fs::write(&file, "x\n").expect("fixture");
    let mut harness = open_paths(&[file], Vec2::new(600.0, HEIGHT));
    assert_eq!(status(&harness), ITEMS);
    let mut previous = ITEMS.len();
    for width in [400.0, 220.0, 180.0, 140.0, 100.0, 60.0] {
        resize(&mut harness, width);
        let items = status(&harness);
        let shown: Vec<&str> = items.iter().map(String::as_str).collect();
        assert!(
            ITEMS.ends_with(&shown),
            "at {width}: {items:?} is not the right end of {ITEMS:?}"
        );
        assert!(items.len() <= previous, "at {width}: {items:?}");
        previous = items.len();
    }
    assert_eq!(
        status(&harness),
        ["Plain Text"],
        "the last one always stays"
    );
    resize(&mut harness, 180.0);
    assert!(status(&harness).len() < ITEMS.len(), "Ln and Col dropped");
    harness.snapshot("layout_status_180");
}

#[test]
fn a_long_message_takes_at_most_40_percent_of_the_status_bar() {
    let dir = temp_dir("status-message");
    let file = dir.join("notes.txt");
    std::fs::write(&file, "x\n").expect("fixture");
    let mut harness = open_paths(&[file], Vec2::new(500.0, HEIGHT));
    type_text(&mut harness, "y");
    press(&mut harness, Modifiers::COMMAND, Key::W);
    let prompt = "Unsaved changes: Ctrl+W again closes without saving, Ctrl+S saves";
    assert_eq!(status(&harness)[0], prompt, "the label reads in full");
    assert_eq!(
        &status(&harness)[1..],
        ["Ln 1, Col 2", "UTF-8", "LF", "Plain Text"],
        "the items still fit"
    );
    let label = harness.get_by_role(Role::Status).get_by_value(prompt);
    assert!(
        label.rect().width() <= 0.4 * 500.0 + 0.5,
        "{:?}",
        label.rect()
    );
}

#[test]
fn a_window_too_short_for_the_editor_hides_the_status_bar() {
    let dir = temp_dir("short");
    let file = dir.join("notes.txt");
    std::fs::write(&file, "x\n").expect("fixture");
    let mut harness = open_paths(&[file], Vec2::new(600.0, 127.0));
    assert!(
        harness.query_by_role(Role::Status).is_some(),
        "127 points fit"
    );
    harness.set_size(Vec2::new(600.0, 126.0));
    quiet(&mut harness);
    assert!(harness.query_by_role(Role::Status).is_none(), "126 do not");
    assert!(harness.query_by_role(Role::MultilineTextInput).is_some());
}
