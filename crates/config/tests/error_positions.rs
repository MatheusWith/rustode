//! Errors in configuration files point at a line and a column.

use rustode_config::{DiagnosticKind, parse_keybindings};

#[test]
fn a_json_syntax_error_message_starts_with_its_line_and_column() {
    let parsed = parse_keybindings("[\n  { \"key\": \"ctrl+s\" \"command\": \"x\" }\n]");
    let error = parsed
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, DiagnosticKind::Syntax(_)))
        .expect("a syntax diagnostic");
    let text = error.to_string();
    assert!(text.starts_with("2:"), "{text}");
    assert!(text.contains("Comma expected"), "{text}");
}

#[test]
fn an_invalid_when_is_reported_at_its_place_in_the_file() {
    let parsed = parse_keybindings(
        "[\n  { \"key\": \"ctrl+s\", \"command\": \"x\", \"when\": \"a &&\" }\n]",
    );
    let error = parsed
        .diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::InvalidWhen)
        .expect("an invalid when diagnostic");
    assert_eq!(error.position.line, 2);
    let text = error.to_string();
    assert!(text.starts_with("2:"), "{text}");
    assert!(!text.contains("offset"), "{text}");
}
