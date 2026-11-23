//! A `when` clause nested thousands of parentheses deep is refused with an
//! error that has a position. Before the depth limit it recursed once per
//! parenthesis, overflowed the stack and aborted the whole process, so a
//! hostile or corrupted `keybindings.json` could kill the editor.
//!
//! A stack overflow aborts the process; run these one at a time when
//! checking a change to the parser (`cargo nextest run` already does).

use rustode_config::{DiagnosticKind, parse_keybindings, parse_when};

fn nested(open: &str, depth: usize) -> String {
    format!("{}a{}", open.repeat(depth), ")".repeat(depth))
}

#[test]
fn a_clause_nested_100000_parentheses_deep_is_an_error_not_a_stack_overflow() {
    let error = parse_when(&nested("(", 100_000)).expect_err("too deep");
    assert!(error.message.contains("nested"), "{error}");
    assert_eq!(error.line, 1);
    assert!(error.column >= 1, "{error}");
}

#[test]
fn negated_parentheses_nested_100000_deep_are_an_error_too() {
    let error = parse_when(&nested("!(", 100_000)).expect_err("too deep");
    assert!(error.message.contains("nested"), "{error}");
}

#[test]
fn a_person_s_nesting_still_parses() {
    parse_when(&nested("(", 20)).expect("20 levels");
    parse_when(&nested("!(", 20)).expect("20 negated levels");
}

#[test]
fn a_keybinding_with_a_hostile_when_is_a_diagnostic_not_a_crash() {
    let json = format!(
        "[{{ \"key\": \"ctrl+s\", \"command\": \"x\", \"when\": \"{}\" }}]",
        nested("(", 100_000)
    );
    let parsed = parse_keybindings(&json);
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidWhen),
        "{:?}",
        parsed.diagnostics
    );
}
