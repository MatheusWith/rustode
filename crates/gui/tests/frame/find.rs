//! The find widget: Ctrl+F and Ctrl+H, its fields, toggles and buttons, the
//! match count and the highlighted matches, read back from the AccessKit
//! tree (a dialog of text inputs, toggle buttons and labels), the editor's
//! exposed selection and image snapshots. Searching and replacing are
//! tested at the workspace seam (`crates/workspace/tests/find.rs`).

use super::{assert_status, open_text, press, type_text};
use eframe::egui::{
    Event, Key, Modifiers, PointerButton, Pos2, ViewportId,
    accesskit::{Role, Toggled},
};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use rustode_gui::EditorApp;
use std::time::Duration;

const CTRL: Modifiers = Modifiers::COMMAND;
const NONE: Modifiers = Modifiers::NONE;

fn dialog<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role_and_label(Role::Dialog, "Find / Replace")
}

fn field<'h>(harness: &'h Harness<'_, EditorApp>, label: &'h str) -> Node<'h> {
    dialog(harness).get_by_role_and_label(Role::TextInput, label)
}

fn editor<'h>(harness: &'h Harness<'_, EditorApp>) -> Node<'h> {
    harness.get_by_role(Role::MultilineTextInput)
}

/// The count next to the find field.
fn count(harness: &Harness<'_, EditorApp>) -> String {
    dialog(harness)
        .query_all_by_role(Role::Label)
        .find_map(|label| label.value())
        .unwrap_or_default()
}

fn toggled(harness: &Harness<'_, EditorApp>, label: &str) -> Option<Toggled> {
    dialog(harness)
        .get_by_role_and_label(Role::Button, label)
        .accesskit_node()
        .data()
        .toggled()
}

/// The editor's exposed selection as char indices (anchor, focus).
fn selection(harness: &Harness<'_, EditorApp>) -> (usize, usize) {
    let node = editor(harness).accesskit_node();
    let anchor = node.text_selection_anchor().expect("anchor");
    let focus = node.text_selection_focus().expect("focus");
    (anchor.to_global_usv_index(), focus.to_global_usv_index())
}

fn text(harness: &Harness<'_, EditorApp>) -> String {
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("a document");
    workspace.document(id).expect("document").text()
}

/// Alt+`key` in one frame with the text winit sends along with it, which
/// the toggle must not type.
fn alt(harness: &mut Harness<'_, EditorApp>, key: Key, letter: &str) {
    let modifiers = Modifiers::ALT;
    harness.input_mut().events.extend([
        Event::ModifiersChanged(modifiers),
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        Event::Text(letter.to_owned()),
    ]);
    harness.step();
    harness.event(Event::ModifiersChanged(NONE));
    harness.run();
}

fn click_at(harness: &mut Harness<'_, EditorApp>, pos: Pos2) {
    harness.hover_at(pos);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: NONE,
        });
    }
    harness.run();
}

#[test]
fn ctrl_f_shows_the_widget_and_finds_as_you_type() {
    // Two empty lines first, so the matches show below the widget.
    let mut harness = open_text("find", "\n\nFoo foo FOO\nfoo\n");
    assert!(harness.query_by_role(Role::Dialog).is_none());
    press(&mut harness, CTRL, Key::F);
    assert!(field(&harness, "Find").is_focused());
    assert!(
        dialog(&harness)
            .query_by_role_and_label(Role::TextInput, "Replace")
            .is_none(),
        "no replace field on Ctrl+F"
    );
    for toggle in ["Match Case", "Match Whole Word", "Use Regular Expression"] {
        assert_eq!(toggled(&harness, toggle), Some(Toggled::False), "{toggle}");
    }
    assert_eq!(count(&harness), "No results");
    type_text(&mut harness, "foo");
    assert_eq!(field(&harness, "Find").value().as_deref(), Some("foo"));
    assert_eq!(count(&harness), "1 of 4");
    assert_eq!(selection(&harness), (2, 5), "the first match is selected");
    harness.snapshot("find");
}

#[test]
fn enter_steps_through_the_matches_and_alt_c_matches_case() {
    let mut harness = open_text("step", "\n\nFoo foo FOO\nfoo\n");
    press(&mut harness, CTRL, Key::F);
    type_text(&mut harness, "foo");
    alt(&mut harness, Key::C, "c");
    assert_eq!(toggled(&harness, "Match Case"), Some(Toggled::True));
    assert_eq!(count(&harness), "1 of 2", "Match Case");
    assert_eq!(field(&harness, "Find").value().as_deref(), Some("foo"));
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(count(&harness), "2 of 2");
    assert_eq!(selection(&harness), (14, 17));
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(count(&harness), "1 of 2", "wraps around");
    press(&mut harness, Modifiers::SHIFT, Key::Enter);
    assert_eq!(count(&harness), "2 of 2");
    type_text(&mut harness, "zz");
    assert_eq!(count(&harness), "No results");
    press(&mut harness, NONE, Key::Escape);
    assert!(harness.query_by_role(Role::Dialog).is_none(), "closed");
    assert!(editor(&harness).is_focused(), "the editor has focus again");
}

#[test]
fn ctrl_h_replaces_one_then_all_as_one_undo_step() {
    let mut harness = open_text("replace", "a1 a2 a3\n");
    press(&mut harness, CTRL, Key::H);
    assert!(
        field(&harness, "Find").is_focused(),
        "nothing to seed: the find field takes focus"
    );
    let toggle = dialog(&harness).get_by_role_and_label(Role::Button, "Toggle Replace");
    assert_eq!(toggle.accesskit_node().data().is_expanded(), Some(true));
    type_text(&mut harness, "a");
    press(&mut harness, NONE, Key::Tab);
    assert!(
        field(&harness, "Replace").is_focused(),
        "Tab moves to Replace"
    );
    type_text(&mut harness, "b");
    harness.snapshot("replace");
    press(&mut harness, NONE, Key::Enter);
    assert_eq!(text(&harness), "b1 a2 a3\n");
    assert_eq!(count(&harness), "1 of 2", "the next match is selected");
    press(&mut harness, CTRL | Modifiers::ALT, Key::Enter);
    assert_eq!(text(&harness), "b1 b2 b3\n");
    assert_status(&harness, "Replaced 2 occurrence(s)");
    press(&mut harness, Modifiers::SHIFT, Key::Tab);
    assert!(field(&harness, "Find").is_focused(), "Shift+Tab moves back");
    press(&mut harness, NONE, Key::Escape);
    press(&mut harness, CTRL, Key::Z);
    assert_eq!(text(&harness), "b1 a2 a3\n", "Replace All undoes at once");
}

#[test]
fn ctrl_h_seeds_from_a_selection_and_focuses_the_replace_field() {
    let mut harness = open_text("seed", "one two one\n");
    for _ in 0..3 {
        press(&mut harness, Modifiers::SHIFT, Key::ArrowRight);
    }
    press(&mut harness, CTRL, Key::H);
    assert_eq!(field(&harness, "Find").value().as_deref(), Some("one"));
    assert!(field(&harness, "Replace").is_focused());
    assert_eq!(count(&harness), "1 of 2");
}

#[test]
fn f3_repeats_the_last_search_with_the_widget_closed() {
    let mut harness = open_text("f3", "x y x y x\n");
    press(&mut harness, CTRL, Key::F);
    type_text(&mut harness, "x");
    press(&mut harness, NONE, Key::Escape);
    press(&mut harness, NONE, Key::F3);
    assert_eq!(selection(&harness), (4, 5));
    press(&mut harness, Modifiers::SHIFT, Key::F3);
    assert_eq!(selection(&harness), (0, 1));
    assert!(editor(&harness).is_focused());
}

#[test]
fn the_buttons_step_toggle_and_close() {
    let mut harness = open_text("buttons", "ab ab ab\n");
    press(&mut harness, CTRL, Key::F);
    let previous = |harness: &Harness<'_, EditorApp>| {
        dialog(harness)
            .get_by_role_and_label(Role::Button, "Previous Match")
            .accesskit_node()
            .is_disabled()
    };
    assert!(previous(&harness), "disabled without a search");
    type_text(&mut harness, "ab");
    assert!(!previous(&harness));
    dialog(&harness)
        .get_by_role_and_label(Role::Button, "Next Match")
        .click();
    harness.run();
    assert_eq!(count(&harness), "2 of 3");
    dialog(&harness)
        .get_by_role_and_label(Role::Button, "Use Regular Expression")
        .click();
    harness.run();
    assert_eq!(
        toggled(&harness, "Use Regular Expression"),
        Some(Toggled::True)
    );
    type_text(&mut harness, "(");
    assert_eq!(count(&harness), "No results");
    assert!(
        dialog(&harness)
            .query_all_by_role(Role::Label)
            .filter_map(|label| label.value())
            .any(|value| value.starts_with("invalid regular expression")),
        "the regex error shows under the field"
    );
    dialog(&harness)
        .get_by_role_and_label(Role::Button, "Close")
        .click();
    harness.run();
    assert!(harness.query_by_role(Role::Dialog).is_none());
}

#[test]
fn typing_in_the_editor_updates_the_count_while_the_widget_stays() {
    // The matches sit on the fourth line, below the widget.
    let mut harness = open_text("live", "\n\n\na a\n");
    press(&mut harness, CTRL, Key::F);
    type_text(&mut harness, "a");
    assert_eq!(count(&harness), "1 of 2");
    let end = editor(&harness).rect().left_top() + eframe::egui::vec2(200.0, 66.0);
    click_at(&mut harness, end);
    assert!(editor(&harness).is_focused());
    type_text(&mut harness, " a");
    assert_eq!(text(&harness), "\n\n\na a a\n");
    assert_eq!(count(&harness), "? of 3", "the caret is not on a match");
    alt(&mut harness, Key::W, "w");
    assert_eq!(toggled(&harness, "Match Whole Word"), Some(Toggled::True));
    assert_eq!(text(&harness), "\n\n\na a a\n", "Alt+W typed nothing");
    press(&mut harness, NONE, Key::Escape);
    assert!(
        harness.query_by_role(Role::Dialog).is_none(),
        "Escape in the editor closes it"
    );
    assert_eq!(
        harness.output().viewport_output[&ViewportId::ROOT].repaint_delay,
        Duration::MAX,
        "nothing scheduled after input"
    );
}

#[test]
fn the_fields_keep_tab_and_f6_leaves_the_widget_for_the_next_part() {
    let mut harness = open_text("find-focus", "foo\n");
    press(&mut harness, CTRL, Key::H);
    assert!(field(&harness, "Find").is_focused());
    press(&mut harness, NONE, Key::Tab);
    assert!(field(&harness, "Replace").is_focused(), "Tab: Replace");
    press(&mut harness, NONE, Key::Tab);
    assert!(
        field(&harness, "Find").is_focused(),
        "Tab stays between the two fields"
    );
    press(&mut harness, Modifiers::SHIFT, Key::Tab);
    assert!(field(&harness, "Replace").is_focused(), "Shift+Tab");
    press(&mut harness, NONE, Key::F6);
    assert!(
        harness.get_by_role(Role::Status).is_focused(),
        "F6 goes on to the status bar, past the problems panel that is not shown"
    );
    assert!(
        harness.query_by_role(Role::Dialog).is_some(),
        "the widget stays open"
    );
}
