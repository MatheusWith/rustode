//! The Extensions view in the sidebar: Ctrl+Shift+X, a search against
//! a local Open VSX stand-in, the Install, Uninstall, Disable and Enable
//! buttons, and the Installed list with each extension's state and its
//! not-loaded notice, read from the AccessKit tree and image snapshots. The extension logic is tested at the workspace
//! seam (`crates/workspace/tests/extensions.rs`).

use super::quiet;
use eframe::egui::{Event, Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use rustode_workspace::Workspace;
use rustode_workspace::extensions::stand_in::{Fixture, StandIn};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-gui-ext-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The window on a small file, with its extensions in a temp folder and
/// the stand-in as registry.
fn open(name: &str, stand_in: &StandIn) -> (Harness<'static, EditorApp>, PathBuf) {
    let dir = temp(name);
    let file = dir.join("notes.txt");
    std::fs::write(&file, "hello\n").expect("fixture");
    let extensions = dir.join("extensions");
    let mut workspace = Workspace::new();
    workspace.set_extensions_location(&extensions);
    workspace.set_global_storage_location(&dir.join("globalStorage"));
    workspace
        .set_extension_registry(stand_in.url())
        .expect("registry");
    workspace.open(&file).expect("open");
    let app = EditorApp::new(workspace);
    // 1200 points wide, so the sidebar gets VS Code's full 300 points.
    let mut harness = Harness::builder()
        .with_size([1200.0, 400.0])
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    quiet(&mut harness);
    (harness, extensions)
}

/// Steps frames until the background work is done.
fn settle(harness: &mut Harness<'_, EditorApp>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.step();
        if !harness.state().workspace().extensions_busy() {
            break;
        }
        assert!(Instant::now() < deadline, "extension work did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    quiet(harness);
}

fn press(harness: &mut Harness<'_, EditorApp>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    quiet(harness);
}

fn ctrl_shift_x(harness: &mut Harness<'_, EditorApp>) {
    press(harness, Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
}

/// The text of every label, in tree order.
fn labels(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .query_all_by_role(Role::Label)
        .filter_map(|label| label.value())
        .collect()
}

#[track_caller]
fn assert_label(harness: &Harness<'_, EditorApp>, text: &str) {
    let labels = labels(harness);
    assert!(
        labels.iter().any(|label| label == text),
        "no label {text:?} in {labels:?}"
    );
}

fn buttons(harness: &Harness<'_, EditorApp>, caption: &str) -> usize {
    harness.query_all_by_label(caption).count()
}

fn publish(stand_in: &StandIn) {
    let mut hello = Fixture::new("acme", "hello", "1.2.3");
    hello.display_name = "Hello World".into();
    let mut rainbow = Fixture::new("acme", "rainbow", "0.4.0");
    rainbow.display_name = "Rainbow Brackets".into();
    stand_in.publish(&hello);
    stand_in.publish(&rainbow);
    stand_in.search_results(&[hello, rainbow]);
}

/// Opens the panel, searches for "hello" and waits for the results.
fn search(harness: &mut Harness<'_, EditorApp>) {
    ctrl_shift_x(harness);
    settle(harness);
    harness.event(Event::Text("hello".to_owned()));
    quiet(harness);
    press(harness, Modifiers::NONE, Key::Enter);
    settle(harness);
}

#[test]
fn ctrl_shift_x_shows_the_panel_with_its_search_box_focused() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut harness, _) = open("show", &stand_in);
    assert!(labels(&harness).iter().all(|label| label != "EXTENSIONS"));
    ctrl_shift_x(&mut harness);
    settle(&mut harness);
    assert_label(&harness, "EXTENSIONS");
    assert_label(&harness, "INSTALLED · 0");
    assert_label(&harness, "No extensions installed");
    assert!(harness.get_by_role(Role::TextInput).is_focused());
    ctrl_shift_x(&mut harness);
    assert!(
        harness.get_by_role(Role::MultilineTextInput).is_focused(),
        "a second Ctrl+Shift+X gives focus back to the editor"
    );
    assert_label(&harness, "EXTENSIONS");
}

#[test]
fn a_search_shows_install_buttons() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut harness, _) = open("search", &stand_in);
    search(&mut harness);
    assert_label(&harness, "Open VSX: 2 of 42");
    assert_label(&harness, "Hello World");
    assert_label(&harness, "acme · 1.2.3");
    assert_label(&harness, "Rainbow Brackets");
    assert_eq!(buttons(&harness, "Install"), 2);
    harness.snapshot("extensions_results");
}

#[test]
fn install_then_uninstall_with_the_buttons() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut harness, extensions) = open("install", &stand_in);
    search(&mut harness);
    harness
        .query_all_by_label("Install")
        .next()
        .expect("an Install button")
        .click();
    harness.step();
    settle(&mut harness);
    assert!(
        extensions
            .join("acme.hello-1.2.3-universal/package.json")
            .exists()
    );
    assert_eq!(buttons(&harness, "Installed"), 1);
    assert_eq!(buttons(&harness, "Uninstall"), 1);
    assert_eq!(buttons(&harness, "Disable"), 1);
    assert_label(&harness, "INSTALLED · 1");
    assert_label(&harness, "1.2.3 · Enabled · Not loaded in this phase");
    assert_label(&harness, "Not loaded in this phase");
    assert_label(
        &harness,
        "Installed acme.hello 1.2.3 (not loaded in this phase).",
    );
    harness.snapshot("extensions_installed");

    harness.get_by_label("Uninstall").click();
    harness.step();
    settle(&mut harness);
    assert_label(&harness, "No extensions installed");
    assert_label(&harness, "Uninstalled acme.hello.");
    assert_eq!(buttons(&harness, "Install"), 2);
    assert!(!extensions.join("acme.hello-1.2.3-universal").exists());
}

#[test]
fn disable_then_enable_with_the_buttons() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    publish(&stand_in);
    let (mut harness, extensions) = open("enable", &stand_in);
    let state = extensions
        .with_file_name("globalStorage")
        .join("state.json");
    search(&mut harness);
    harness
        .query_all_by_label("Install")
        .next()
        .expect("an Install button")
        .click();
    harness.step();
    settle(&mut harness);

    harness.get_by_label("Disable").click();
    harness.step();
    settle(&mut harness);
    assert_label(&harness, "1.2.3 · Disabled · Not loaded in this phase");
    assert_label(&harness, "Disabled acme.hello.");
    assert_eq!(buttons(&harness, "Enable"), 1);
    assert_eq!(buttons(&harness, "Disable"), 0);
    assert!(
        std::fs::read_to_string(&state)
            .expect("state")
            .contains("acme.hello")
    );
    harness.snapshot("extensions_disabled");

    harness.get_by_label("Enable").click();
    harness.step();
    settle(&mut harness);
    assert_label(&harness, "1.2.3 · Enabled · Not loaded in this phase");
    assert_label(&harness, "Enabled acme.hello (not loaded in this phase).");
    assert_eq!(buttons(&harness, "Disable"), 1);
    assert_eq!(std::fs::read_to_string(&state).expect("state"), "{}");
}

#[test]
fn an_invalid_extension_shows_why_and_cannot_be_enabled() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut harness, extensions) = open("invalid", &stand_in);
    let folder = extensions.join("acme.broken-1.0.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("package.json"),
        r#"{"name":"broken","displayName":"Broken","publisher":"acme","version":"1.0.0"}"#,
    )
    .expect("manifest");
    std::fs::write(
        extensions.join("extensions.json"),
        r#"[{"identifier":{"id":"acme.broken"},"version":"1.0.0","location":{"$mid":1,"path":"/x","scheme":"file"},"relativeLocation":"acme.broken-1.0.0"}]"#,
    )
    .expect("index");
    ctrl_shift_x(&mut harness);
    settle(&mut harness);
    assert_label(&harness, "Broken");
    assert_label(&harness, "1.0.0 · Invalid · Not loaded in this phase");
    assert_label(
        &harness,
        "property `engines` is mandatory and must be of type `object`",
    );
    assert_eq!(
        buttons(&harness, "Enable") + buttons(&harness, "Disable"),
        0
    );
    assert_eq!(buttons(&harness, "Uninstall"), 1);
}

#[test]
fn an_unusable_obsolete_file_is_reported_in_the_status_bar_by_a_listing() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut harness, extensions) = open("obsolete", &stand_in);
    std::fs::create_dir_all(&extensions).expect("extensions folder");
    std::fs::write(extensions.join(".obsolete"), "{ not json").expect(".obsolete");
    ctrl_shift_x(&mut harness);
    settle(&mut harness);
    let shown = labels(&harness).join("\n");
    assert!(shown.contains(".obsolete"), "{shown}");
    assert!(shown.contains("left unchanged"), "{shown}");
    assert_eq!(
        std::fs::read_to_string(extensions.join(".obsolete")).expect("read"),
        "{ not json"
    );
}
