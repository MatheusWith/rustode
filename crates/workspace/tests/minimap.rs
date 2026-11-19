//! Workspace seam: what the GUI's minimap needs: the `editor.minimap.enabled`
//! setting, the toggle command, and the colored runs of the lines it draws.

use rustode_workspace::{COMMANDS, DocumentId, MinimapRun, TokenKind, Workspace};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-minimap-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open(name: &str, file: &str, text: &str) -> (Workspace, DocumentId) {
    let path = dir(name).join(file);
    std::fs::write(&path, text).expect("fixture");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    (ws, id)
}

fn run(column: usize, len: usize, kind: Option<TokenKind>) -> MinimapRun {
    MinimapRun { column, len, kind }
}

fn with_settings(name: &str, settings: &str) -> Workspace {
    let dir = dir(name);
    let file = dir.join("settings.json");
    std::fs::write(&file, settings).expect("settings");
    let mut ws = Workspace::new();
    ws.load_config(&file, &dir.join("keybindings.json"));
    ws
}

#[test]
fn the_minimap_is_on_by_default() {
    assert!(Workspace::new().minimap_enabled());
}

#[test]
fn the_setting_turns_it_off_and_a_bad_value_is_ignored() {
    assert!(
        !with_settings("off", "{\"editor.minimap.enabled\": false}").minimap_enabled(),
        "false hides it"
    );
    let ws = with_settings("bad", "{\"editor.minimap.enabled\": \"no\"}");
    assert!(ws.minimap_enabled(), "a value of the wrong type is ignored");
    assert!(ws.config().diagnostics().count() > 0, "and reported");
}

#[test]
fn the_toggle_command_is_registered_and_flips_it_without_a_document() {
    assert!(
        COMMANDS
            .iter()
            .any(|c| c.id == "editor.action.toggleMinimap")
    );
    let mut ws = Workspace::new();
    assert_eq!(ws.execute("editor.action.toggleMinimap").ok(), Some(true));
    assert!(!ws.minimap_enabled());
    assert!(ws.toggle_minimap());
    assert!(ws.minimap_enabled());
}

#[test]
fn editing_the_setting_overrides_an_earlier_toggle() {
    let dir = dir("override");
    let settings = dir.join("settings.json");
    std::fs::write(&settings, "{}").expect("settings");
    let mut ws = Workspace::new();
    ws.load_config(&settings, &dir.join("keybindings.json"));
    assert!(!ws.toggle_minimap());
    std::fs::write(&settings, "{\"editor.minimap.enabled\": true}").expect("settings");
    ws.reload_config();
    assert!(ws.minimap_enabled(), "the user's edit wins over a toggle");
    std::fs::write(
        &settings,
        "{\"editor.minimap.enabled\": true, \"editor.tabSize\": 2}",
    )
    .expect("settings");
    assert!(!ws.toggle_minimap());
    ws.reload_config();
    assert!(
        !ws.minimap_enabled(),
        "a change to another setting keeps the toggle"
    );
}

#[test]
fn plain_text_is_runs_of_non_blank_chars_with_no_kind() {
    let (ws, id) = open("plain", "a.txt", "ab  cd\n\n  x");
    let lines = ws.minimap_lines(id, 0..3, 120).expect("lines");
    assert_eq!(
        lines,
        vec![
            vec![run(0, 2, None), run(4, 2, None)],
            vec![],
            vec![run(2, 1, None)],
        ]
    );
}

#[test]
fn tabs_advance_columns_to_the_next_tab_stop() {
    let (ws, id) = open("tabs", "a.txt", "\tx\ny\tz");
    // The default tab size is 4.
    let lines = ws.minimap_lines(id, 0..2, 120).expect("lines");
    assert_eq!(lines[0], vec![run(4, 1, None)]);
    assert_eq!(lines[1], vec![run(0, 1, None), run(4, 1, None)]);
}

#[test]
fn columns_past_the_maximum_are_not_returned() {
    let (ws, id) = open("max", "a.txt", "abcdefgh");
    assert_eq!(
        ws.minimap_lines(id, 0..1, 5).expect("lines"),
        vec![vec![run(0, 5, None)]]
    );
    assert_eq!(
        ws.minimap_lines(id, 0..1, 0).expect("lines"),
        vec![vec![]],
        "no columns, no runs"
    );
}

#[test]
fn a_range_past_the_end_is_clamped_and_an_empty_one_is_empty() {
    let (ws, id) = open("range", "a.txt", "a\nb");
    assert_eq!(ws.minimap_lines(id, 1..50, 120).expect("lines").len(), 1);
    assert!(ws.minimap_lines(id, 5..9, 120).expect("lines").is_empty());
    assert!(ws.minimap_lines(id, 2..1, 120).expect("lines").is_empty());
}

#[test]
fn highlighted_text_carries_its_token_kind() {
    let (ws, id) = open("rust", "main.rs", "fn main() {\n    // hi\n}\n");
    let lines = ws.minimap_lines(id, 0..3, 120).expect("lines");
    let kinds =
        |line: &[MinimapRun]| -> Vec<Option<TokenKind>> { line.iter().map(|r| r.kind).collect() };
    assert_eq!(lines[0][0], run(0, 2, Some(TokenKind::Keyword)));
    assert!(
        kinds(&lines[0]).contains(&Some(TokenKind::Function)),
        "main is a function: {:?}",
        lines[0]
    );
    assert_eq!(
        lines[1],
        vec![
            run(4, 2, Some(TokenKind::Comment)),
            run(7, 2, Some(TokenKind::Comment))
        ],
        "a blank ends a run, whatever the highlight"
    );
    assert_eq!(lines[2], vec![run(0, 1, Some(TokenKind::Punctuation))]);
}

#[test]
fn a_very_long_line_is_not_walked_past_the_maximum() {
    let long = "x".repeat(2_000_000);
    let (ws, id) = open("long", "a.txt", &long);
    let lines = ws.minimap_lines(id, 0..1, 120).expect("lines");
    assert_eq!(lines, vec![vec![run(0, 120, None)]]);
}
