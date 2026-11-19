//! The minimap: drawn at the editor's right edge, navigating on a press and
//! a drag, hidden by `editor.minimap.enabled` and toggled from the palette.

use super::{fixture, open, press, status, type_text, window};
use eframe::egui::{
    self, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, ViewportId,
    accesskit::Role,
};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::EditorApp;
use rustode_workspace::Workspace;
use std::time::Duration;

fn editor<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::MultilineTextInput)
}

fn shown(harness: &Harness<'_, EditorApp>) -> String {
    editor(harness).accesskit_node().document_range().text()
}

fn numbered_lines(count: usize) -> String {
    use std::fmt::Write as _;
    (1..=count).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "line {n}");
        text
    })
}

fn rust_source(count: usize) -> String {
    use std::fmt::Write as _;
    (0..count).fold(String::new(), |mut text, n| {
        let _ = write!(
            text,
            "fn function_{n}(value: u32) -> u32 {{\n    // step {n}\n    value + {n}\n}}\n\n"
        );
        text
    })
}

/// The text area's right edge, where the minimap starts, and its top.
fn minimap_origin(harness: &Harness<'_, EditorApp>) -> Pos2 {
    let rect = editor(harness).rect();
    Pos2::new(rect.right(), rect.top())
}

fn press_at(harness: &mut Harness<'_, EditorApp>, pos: Pos2, pressed: bool) {
    harness.event(Event::PointerMoved(pos));
    harness.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

fn with_settings(
    name: &str,
    settings: &str,
    text: &str,
    size: Vec2,
) -> Harness<'static, EditorApp> {
    let file = fixture(name, "f.rs", text);
    let user = file.parent().expect("dir").join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    std::fs::write(user.join("settings.json"), settings).expect("settings");
    let mut app = EditorApp::open_paths_in(Workspace::new(), &[file]).expect("open");
    app.load_user_config(&user);
    window(app, size, 1.0)
}

#[test]
fn the_minimap_shows_the_highlighted_document_at_the_right_edge() {
    let mut harness = open(&fixture("shown", "main.rs", &rust_source(40)));
    harness.snapshot("minimap");
    let text_right = editor(&harness).rect().right();
    assert!(
        text_right < super::WIDTH - 20.0,
        "the text leaves room: {text_right}"
    );
}

#[test]
fn the_slider_shows_on_mouseover_over_the_lines_in_view() {
    let mut harness = open(&fixture("slider", "main.rs", &rust_source(60)));
    let origin = minimap_origin(&harness);
    harness.hover_at(origin + Vec2::new(20.0, 120.0));
    harness.run();
    harness.snapshot("minimap_slider");
}

#[test]
fn a_press_scrolls_the_line_under_it_to_the_middle_without_moving_the_caret() {
    let mut harness = open(&fixture("press", "f.txt", &numbered_lines(300)));
    let origin = minimap_origin(&harness);
    // 2 points a line, from the first line: line 100 (zero-based) is at 200.
    let at = origin + Vec2::new(20.0, 201.0);
    press_at(&mut harness, at, true);
    press_at(&mut harness, at, false);
    let text = shown(&harness);
    assert!(text.contains("line 101\n"), "{text}");
    assert!(!text.starts_with("line 1\n"), "{text}");
    let first: usize = text["line ".len()..text.find('\n').expect("line")]
        .parse()
        .expect("number");
    assert!(
        (90..=100).contains(&first),
        "line 101 near the middle: first is {first}"
    );
    super::assert_status(&harness, "Ln 1, Col 1");
}

#[test]
fn dragging_the_slider_scrolls_in_proportion() {
    let mut harness = open(&fixture("drag", "f.txt", &numbered_lines(300)));
    let origin = minimap_origin(&harness);
    let grab = origin + Vec2::new(20.0, 3.0);
    press_at(&mut harness, grab, true);
    press_at(&mut harness, grab + Vec2::new(0.0, 40.0), true);
    // The slider travels about 0.8 points a line of 300: 40 points are ~50.
    assert!(
        shown(&harness).starts_with("line 51\n"),
        "{}",
        shown(&harness)
    );
    press_at(&mut harness, grab + Vec2::new(0.0, 40.0), false);
    press_at(&mut harness, grab + Vec2::new(0.0, 400.0), false);
    assert!(
        shown(&harness).starts_with("line 51\n"),
        "a released slider stays"
    );
}

#[test]
fn the_wheel_over_the_minimap_scrolls_the_editor() {
    let mut harness = open(&fixture("wheel", "f.txt", &numbered_lines(300)));
    harness.hover_at(minimap_origin(&harness) + Vec2::new(20.0, 100.0));
    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: Vec2::new(0.0, -2.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    assert!(
        shown(&harness).starts_with("line 7\n"),
        "{}",
        shown(&harness)
    );
}

#[test]
fn the_setting_hides_the_minimap_and_gives_its_room_to_the_text() {
    let size = Vec2::new(super::WIDTH, super::HEIGHT);
    let on = with_settings("on", "{}", &numbered_lines(50), size);
    let off = with_settings(
        "off",
        "{\"editor.minimap.enabled\": false}",
        &numbered_lines(50),
        size,
    );
    assert!(editor(&on).rect().right() < editor(&off).rect().right());
    assert!(
        (editor(&off).rect().right() - super::WIDTH).abs() < 1.0,
        "the text reaches the window's edge"
    );
    let mut off = off;
    off.snapshot("minimap_off");
}

#[test]
fn the_toggle_command_hides_and_shows_it_again() {
    let mut harness = open(&fixture("toggle", "f.txt", &numbered_lines(50)));
    let with = editor(&harness).rect().right();
    for expect_hidden in [true, false] {
        press(&mut harness, Modifiers::CTRL | Modifiers::SHIFT, Key::P);
        type_text(&mut harness, "toggle minimap");
        press(&mut harness, Modifiers::NONE, Key::Enter);
        let right = editor(&harness).rect().right();
        if expect_hidden {
            assert!(right > with, "{right} > {with}");
        } else {
            assert!((right - with).abs() < 0.5, "{right} = {with}");
        }
    }
}

#[test]
fn a_press_on_the_minimap_leaves_the_keyboard_with_the_editor() {
    let mut harness = open(&fixture("focus", "f.txt", &numbered_lines(50)));
    let at = minimap_origin(&harness) + Vec2::new(20.0, 30.0);
    press_at(&mut harness, at, true);
    press_at(&mut harness, at, false);
    assert!(editor(&harness).is_focused());
    type_text(&mut harness, "x");
    assert!(shown(&harness).contains("xline 1"), "{}", shown(&harness));
}

#[test]
fn an_idle_window_schedules_no_frame_and_the_slider_leaves_after_a_scroll() {
    let mut harness = open(&fixture("idle", "f.txt", &numbered_lines(300)));
    let delay = |harness: &Harness<'_, EditorApp>| {
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay
    };
    assert_eq!(
        delay(&harness),
        Duration::MAX,
        "nothing scheduled when idle"
    );
    harness.hover_at(Pos2::new(250.0, 100.0));
    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -100.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.snapshot("minimap_after_scroll");
    harness.run();
    harness.run_steps(8);
    assert_eq!(
        delay(&harness),
        Duration::MAX,
        "idle again, with no timer left"
    );
    assert!(status(&harness).contains(&"Ln 1, Col 1".to_owned()));
    let _ = egui::Id::NULL;
}
