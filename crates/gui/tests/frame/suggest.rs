//! Input to frame for hover and completion: Ctrl+Space and typing open the
//! suggest list under the word, Enter, Tab or a click accept, Escape closes;
//! Ctrl+K Ctrl+I and the resting pointer show the hover. The AccessKit tree
//! has the list as a list box with the selected option as its active
//! descendant, and the hover as a tooltip. The server is `rustode-test-lsp`.
//! What is asked of the server, filtered and put in is tested at the
//! workspace seam (`crates/workspace/tests/lsp.rs`).

use super::{lsp, press, type_text};
use eframe::egui::{Event, FontId, Key, Modifiers, Pos2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use rustode_gui::{EditorApp, FONT_SIZE, LINE_HEIGHT};
use rustode_workspace::{ServerState, Workspace};
use std::time::{Duration, Instant};

const CODE: &str = "fn main() {\n    let point = 1;\n    pri\n}\n";

/// A window on `main.rs` with the test server running and the cursor after
/// the `pri` at the end.
fn open(name: &str) -> Harness<'static, EditorApp> {
    let base =
        std::env::temp_dir().join(format!("rustode-gui-suggest-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("project");
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::create_dir_all(base.join("user")).expect("temp dir");
    let file = dir.join("main.rs");
    std::fs::write(&file, CODE).expect("fixture");
    // A plain path's Debug form is a valid JSON string.
    let command = format!("{:?}", lsp::server().display().to_string());
    let settings =
        format!("{{\"rustode.languageServers\": {{\"rust\": {{\"command\": {command}}}}}}}");
    std::fs::write(base.join("user/settings.json"), settings).expect("settings");
    let mut workspace = Workspace::new();
    let id = workspace.open(&file).expect("open");
    workspace
        .set_cursor(id, CODE.find("pri").expect("pri") + 3)
        .expect("cursor");
    let app = EditorApp::new(workspace);
    let mut harness = Harness::builder()
        .with_size([640.0, 360.0])
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    harness.run();
    harness.state_mut().load_user_config(&base.join("user"));
    wait(&mut harness, |h| {
        h.state()
            .workspace()
            .language_servers()
            .iter()
            .any(|s| s.state == ServerState::Running)
    });
    harness
}

/// Steps the window whenever it asks for a repaint, until `done` holds.
#[track_caller]
fn wait(harness: &mut Harness<'_, EditorApp>, done: impl Fn(&Harness<'_, EditorApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(harness) {
        assert!(Instant::now() < deadline, "timed out");
        if harness.ctx.has_requested_repaint() {
            harness.step();
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    harness.run();
}

fn options(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::ListBoxOption)
        .filter_map(|node| node.accesskit_node().label())
        .collect()
}

fn selected(harness: &Harness<'_, EditorApp>) -> Vec<bool> {
    harness
        .query_all_by_role(Role::ListBoxOption)
        .map(|node| node.accesskit_node().is_selected() == Some(true))
        .collect()
}

fn text(harness: &Harness<'_, EditorApp>) -> String {
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    workspace.document(id).expect("doc").text()
}

fn open_list(harness: &mut Harness<'_, EditorApp>) {
    press(harness, Modifiers::COMMAND, Key::Space);
    wait(harness, |h| h.query_by_role(Role::ListBox).is_some());
}

#[test]
fn ctrl_space_shows_the_list_as_a_list_box_with_the_selection_active() {
    let mut harness = open("list");
    assert!(harness.query_by_role(Role::ListBox).is_none());
    open_list(&mut harness);
    let list = harness.get_by_role(Role::ListBox);
    assert_eq!(
        list.accesskit_node().label().as_deref(),
        Some("Suggestions")
    );
    assert_eq!(
        options(&harness),
        [
            "println, function, macro println!",
            "print, function, fn print()",
            "PrimaryKey, class, trait PrimaryKey",
        ]
    );
    assert_eq!(selected(&harness), [true, false, false]);
    let active = list
        .accesskit_node()
        .active_descendant()
        .and_then(|node| node.label());
    assert_eq!(active.as_deref(), Some("println, function, macro println!"));
    harness.snapshot("suggest_list");
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(selected(&harness), [false, true, false]);
    let active = harness
        .get_by_role(Role::ListBox)
        .accesskit_node()
        .active_descendant()
        .and_then(|node| node.label());
    assert_eq!(active.as_deref(), Some("print, function, fn print()"));
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(selected(&harness), [false, false, true], "up wraps");
}

#[test]
fn typing_opens_and_narrows_it_and_enter_accepts() {
    let mut harness = open("typing");
    type_text(&mut harness, "n");
    wait(&mut harness, |h| h.query_by_role(Role::ListBox).is_some());
    assert_eq!(options(&harness).len(), 2);
    type_text(&mut harness, "l");
    assert_eq!(options(&harness), ["println, function, macro println!"]);
    harness.snapshot("suggest_narrowed");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(harness.query_by_role(Role::ListBox).is_none());
    assert!(
        text(&harness).contains("    println!()\n"),
        "{}",
        text(&harness)
    );
    press(&mut harness, Modifiers::COMMAND, Key::Z);
    assert!(
        text(&harness).contains("    prinl\n"),
        "one undo step: {}",
        text(&harness)
    );
}

#[test]
fn escape_closes_it_and_tab_accepts() {
    let mut harness = open("keys");
    open_list(&mut harness);
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(harness.query_by_role(Role::ListBox).is_none());
    assert_eq!(text(&harness), CODE, "nothing changed");
    open_list(&mut harness);
    press(&mut harness, Modifiers::NONE, Key::PageDown);
    press(&mut harness, Modifiers::NONE, Key::Tab);
    assert!(
        text(&harness).contains("    PrimaryKey\n"),
        "{}",
        text(&harness)
    );
}

#[test]
fn a_click_on_a_row_accepts_it() {
    let mut harness = open("click");
    open_list(&mut harness);
    harness
        .get_by(|node| {
            node.label()
                .is_some_and(|l| l.starts_with("print, function"))
        })
        .click();
    harness.run();
    assert!(harness.query_by_role(Role::ListBox).is_none());
    assert!(text(&harness).contains("    print\n"), "{}", text(&harness));
}

#[test]
fn ctrl_k_ctrl_i_shows_the_hover_as_a_tooltip() {
    let mut harness = open("hover");
    // Move to the word `point` on the second line.
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    for _ in 0..3 {
        press(&mut harness, Modifiers::NONE, Key::ArrowRight);
    }
    press(&mut harness, Modifiers::COMMAND, Key::K);
    press(&mut harness, Modifiers::COMMAND, Key::I);
    wait(&mut harness, |h| h.query_by_role(Role::Tooltip).is_some());
    let tooltip = harness.get_by_role(Role::Tooltip);
    assert_eq!(
        tooltip.accesskit_node().label().as_deref(),
        Some("point\n\nlet point")
    );
    harness.snapshot("hover");
    press(&mut harness, Modifiers::NONE, Key::ArrowRight);
    assert!(
        harness.query_by_role(Role::Tooltip).is_none(),
        "a key hides it"
    );
}

#[allow(clippy::cast_possible_truncation)] // window coordinates are far below f32 limits
#[test]
fn the_pointer_resting_on_a_word_shows_its_hover_after_the_delay() {
    let mut harness = open("pointer");
    let area = harness
        .get_by_role(Role::MultilineTextInput)
        .accesskit_node()
        .raw_bounds()
        .expect("bounds");
    let width = harness
        .ctx
        .fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(FONT_SIZE), ' '));
    // Line 2, column 10 of the text: inside `point`, after a gutter of five
    // chars.
    let at = Pos2::new(
        area.x0 as f32 + width * 15.5,
        area.y0 as f32 + LINE_HEIGHT * 1.5,
    );
    harness.hover_at(at);
    harness.run_steps(1);
    assert!(harness.query_by_role(Role::Tooltip).is_none(), "not yet");
    harness.run_steps(3);
    wait(&mut harness, |h| h.query_by_role(Role::Tooltip).is_some());
    assert_eq!(
        harness
            .get_by_role(Role::Tooltip)
            .accesskit_node()
            .label()
            .as_deref(),
        Some("point\n\nlet point")
    );
    harness.hover_at(Pos2::new(at.x, at.y + LINE_HEIGHT * 3.0));
    harness.run_steps(2);
    assert!(
        harness.query_by_role(Role::Tooltip).is_none(),
        "moving away hides it"
    );
    let _ = Event::Text(String::new());
}
