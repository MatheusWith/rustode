//! Editor groups in the window: a split shows two tab strips, the
//! editors side by side or stacked, one document in both, focus by key and
//! by click, and the window sizes at which a split shows only the active
//! group. The layout itself is tested at the workspace seam
//! (`crates/workspace/tests/groups.rs`).

use super::{fixture, open, press, quiet, status};
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;

const TEXT: &str = "first line\nsecond line\nthird line\nfourth line\n";

fn split_window(name: &str, width: f32, height: f32) -> Harness<'static, EditorApp> {
    let mut harness = open(&fixture(name, "notes.txt", TEXT));
    harness.set_size(Vec2::new(width, height));
    quiet(&mut harness);
    press(&mut harness, Modifiers::COMMAND, Key::Backslash);
    harness
}

fn strips(harness: &Harness<'_, EditorApp>) -> usize {
    harness.query_all_by_role(Role::TabList).count()
}

fn click_at(harness: &mut Harness<'_, EditorApp>, pos: Pos2) {
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    quiet(harness);
}

#[test]
fn ctrl_backslash_splits_to_the_right_and_focuses_the_new_group() {
    let mut harness = split_window("split", 900.0, 400.0);
    assert_eq!(strips(&harness), 2, "one tab strip per group");
    let app = harness.state();
    let ids = app.workspace().group_ids();
    assert_eq!(ids.len(), 2);
    assert_eq!(app.workspace().active_group(), ids[1]);
    let tabs: Vec<_> = harness
        .query_all_by_role(Role::TabList)
        .map(|list| list.rect())
        .collect();
    assert!(
        tabs[0].right() <= tabs[1].left() + 1.0,
        "side by side: {tabs:?}"
    );
    assert!((tabs[0].top() - tabs[1].top()).abs() < 0.5);
    assert_eq!(app.workspace().layout().groups(), ids);
    harness.snapshot("groups_split_right");
}

#[test]
fn a_split_down_stacks_the_groups() {
    let mut harness = open(&fixture("down", "notes.txt", TEXT));
    harness.set_size(Vec2::new(900.0, 500.0));
    quiet(&mut harness);
    harness
        .state_mut()
        .workspace_mut()
        .execute("workbench.action.splitEditorDown")
        .expect("split");
    quiet(&mut harness);
    let tabs: Vec<_> = harness
        .query_all_by_role(Role::TabList)
        .map(|list| list.rect())
        .collect();
    assert_eq!(tabs.len(), 2);
    assert!(tabs[0].bottom() <= tabs[1].top(), "stacked: {tabs:?}");
    assert!((tabs[0].left() - tabs[1].left()).abs() < 0.5);
    harness.snapshot("groups_split_down");
}

#[test]
fn one_document_edited_in_one_group_shows_in_the_other() {
    let mut harness = split_window("shared", 900.0, 400.0);
    super::type_text(&mut harness, "XY");
    harness.snapshot("groups_same_file_edited");
    let app = harness.state();
    let ids = app.workspace().group_ids();
    let doc = app.workspace().active_document().expect("document");
    assert_eq!(app.workspace().view_cursor(ids[1], doc), Some(2));
    assert_eq!(
        app.workspace().view_cursor(ids[0], doc),
        Some(2),
        "the other cursor follows the text"
    );
    assert!(
        app.workspace()
            .document(doc)
            .expect("doc")
            .text()
            .starts_with("XYfirst")
    );
}

#[test]
fn a_window_too_narrow_for_two_groups_shows_the_active_one() {
    let mut harness = split_window("narrow", 600.0, 400.0);
    assert_eq!(
        strips(&harness),
        1,
        "two groups need 441 points: 600 less the sidebar leaves 430"
    );
    harness.snapshot("groups_narrow_active_only");
    harness.set_size(Vec2::new(1000.0, 400.0));
    quiet(&mut harness);
    assert_eq!(strips(&harness), 2, "they come back when the window grows");
}

#[test]
fn keys_move_the_focus_and_clicks_do_too() {
    let mut harness = split_window("focus", 900.0, 400.0);
    let ids = harness.state().workspace().group_ids();
    press(&mut harness, Modifiers::COMMAND, Key::Num1);
    assert_eq!(harness.state().workspace().active_group(), ids[0]);
    press(&mut harness, Modifiers::COMMAND, Key::Num2);
    assert_eq!(harness.state().workspace().active_group(), ids[1]);
    // Ctrl+K, then Ctrl+Left.
    press(&mut harness, Modifiers::COMMAND, Key::K);
    press(&mut harness, Modifiers::COMMAND, Key::ArrowLeft);
    assert_eq!(harness.state().workspace().active_group(), ids[0]);
    // A click on the other group's text focuses it and puts the caret there.
    let strips: Vec<_> = harness
        .query_all_by_role(Role::TabList)
        .map(|list| list.rect())
        .collect();
    let at = Pos2::new(strips[1].left() + 80.0, strips[1].bottom() + 19.0 * 1.5);
    click_at(&mut harness, at);
    assert_eq!(harness.state().workspace().active_group(), ids[1]);
    let doc = harness
        .state()
        .workspace()
        .active_document()
        .expect("document");
    let cursor = harness
        .state()
        .workspace()
        .view_cursor(ids[1], doc)
        .expect("cursor");
    assert!(
        cursor >= 11,
        "the click landed on the second line: {cursor}"
    );
    assert_eq!(
        harness.state().workspace().view_cursor(ids[0], doc),
        Some(0)
    );
    // A click on the first group's tab strip focuses it.
    click_at(
        &mut harness,
        Pos2::new(strips[0].left() + 20.0, strips[0].center().y),
    );
    assert_eq!(harness.state().workspace().active_group(), ids[0]);
}

#[test]
fn ctrl_w_closes_the_editor_and_its_empty_group() {
    let mut harness = split_window("close", 900.0, 400.0);
    press(&mut harness, Modifiers::COMMAND, Key::W);
    assert_eq!(harness.state().workspace().group_ids().len(), 1);
    assert_eq!(strips(&harness), 1);
    assert!(harness.state().workspace().active_document().is_some());
    let _ = status(&harness);
}

#[test]
fn the_palette_runs_the_group_commands() {
    let mut harness = open(&fixture("palette", "notes.txt", TEXT));
    harness.set_size(Vec2::new(900.0, 400.0));
    quiet(&mut harness);
    press(&mut harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    super::type_text(&mut harness, "Split Editor Right");
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(strips(&harness), 2);
    assert_eq!(harness.state().workspace().group_ids().len(), 2);
}

#[test]
fn each_pane_draws_its_own_selection() {
    let mut harness = split_window("own", 900.0, 400.0);
    let doc = harness
        .state()
        .workspace()
        .active_document()
        .expect("document");
    let ids = harness.state().workspace().group_ids();
    let workspace = harness.state_mut().workspace_mut();
    workspace
        .set_selections(doc, vec![rustode_workspace::Selection::new(0, 5)], 0)
        .expect("select in the right group");
    workspace.focus_group(ids[0]).expect("left");
    quiet(&mut harness);
    // The right pane keeps "first" selected; the left has nothing selected.
    harness.snapshot("groups_own_selections");
    assert_eq!(
        harness
            .state()
            .workspace()
            .view_selections(ids[1], doc)
            .map(|s| s.primary()),
        Some(rustode_workspace::Selection::new(0, 5))
    );
}
