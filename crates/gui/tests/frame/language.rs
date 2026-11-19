//! "Change Language Mode": a click on the status bar's language name or
//! Ctrl+K M opens the picker, typing filters it, Enter or a click picks,
//! Escape closes it; read from the AccessKit tree and an image snapshot.
//! Detection and the list are tested at the workspace seam
//! (`crates/workspace/tests/languages.rs`).

use super::{assert_status, fixture, open, press, status, type_text};
use eframe::egui::{Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use rustode_workspace::TokenKind;

const PYTHON: &str = "def f(a):\n    return None\n";

fn window(name: &str) -> Harness<'static, EditorApp> {
    open(&fixture(name, "notes.txt", PYTHON))
}

fn picker_is_open(harness: &Harness<'_, EditorApp>) -> bool {
    harness.query_by_label("Auto Detect (c)").is_none()
        && harness
            .query_by_role_and_label(Role::Button, "Auto Detect")
            .is_some()
}

/// The kinds highlighted on the first two lines of the active document.
fn kinds(harness: &Harness<'_, EditorApp>) -> Vec<TokenKind> {
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("document");
    let document = workspace.document(id).expect("doc");
    document.highlights(0..2).iter().map(|h| h.kind).collect()
}

#[test]
fn a_click_on_the_language_opens_the_picker_and_a_pick_highlights() {
    let mut harness = window("lang-click");
    assert_status(&harness, "Plain Text");
    assert!(kinds(&harness).is_empty(), "plain text has no highlights");
    assert!(!picker_is_open(&harness));

    harness.get_by_value("Plain Text").click();
    harness.run();
    assert!(picker_is_open(&harness), "the picker is open");
    harness.get_by_role_and_label(Role::Button, "Rust (rust)");
    harness.snapshot("language_picker");

    type_text(&mut harness, "pyth");
    harness.get_by_role_and_label(Role::Button, "Python (python)");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Rust (rust)")
            .is_none(),
        "the list is filtered"
    );
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(!picker_is_open(&harness), "a pick closes it");
    assert_status(&harness, "Python");
    assert!(
        kinds(&harness).contains(&TokenKind::Keyword),
        "highlighted as Python: {:?}",
        kinds(&harness)
    );
}

#[test]
fn ctrl_k_then_m_opens_it_and_a_click_on_a_row_picks() {
    let mut harness = window("lang-chord");
    press(&mut harness, Modifiers::COMMAND, Key::K);
    assert!(!picker_is_open(&harness), "half a chord opens nothing");
    press(&mut harness, Modifiers::NONE, Key::M);
    assert!(picker_is_open(&harness));
    assert_eq!(
        harness
            .state()
            .workspace()
            .document(harness.state().workspace().active_document().expect("doc"))
            .expect("doc")
            .text(),
        PYTHON,
        "the chord's keys did not reach the editor"
    );
    type_text(&mut harness, "markdown");
    harness
        .get_by_role_and_label(Role::Button, "Markdown (markdown)")
        .click();
    harness.run();
    assert_status(&harness, "Markdown");
    assert!(!picker_is_open(&harness));
}

#[test]
fn escape_closes_the_picker_and_down_moves_the_selection() {
    let mut harness = window("lang-escape");
    harness.get_by_value("Plain Text").click();
    harness.run();
    assert!(picker_is_open(&harness));
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert!(!picker_is_open(&harness));
    assert_status(&harness, "Plain Text");

    harness.get_by_value("Plain Text").click();
    harness.run();
    type_text(&mut harness, "script");
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::Enter);
    // "script" matches JavaScript, Shell Script, ... sorted by name; the
    // second is picked.
    let shown = status(&harness);
    assert_eq!(
        shown.last().map(String::as_str),
        Some("JavaScript JSX"),
        "{shown:?}"
    );
}
