//! `settings.json` and `keybindings.json` in VS Code format.
//!
//! - [`jsonc`]: JSON with comments and trailing commas, with error recovery and positions.
//! - [`when`]: `when` clauses parsed into a [`When`] tree.
//! - [`parse_settings`] and [`parse_keybindings`]: one file each, checked and reported on.
//! - [`Config`]: both files, reloaded on demand with a [`ConfigChange`] report.
//!
//! Nothing here fails on bad input: every problem is a [`Diagnostic`] with a line and column.

mod config;
mod diagnostic;
pub mod jsonc;
mod keybindings;
mod settings;
pub mod when;

pub use config::{Config, ConfigChange, ConfigFile, SettingChange};
pub use diagnostic::{Diagnostic, DiagnosticKind, Location, Severity};
pub use jsonc::{JsonError, Value};
pub use keybindings::{Chord, Keybinding, Keybindings, parse_key, parse_keybindings};
pub use settings::{
    Schema, SettingSchema, SettingType, Settings, SettingsFile, override_languages, parse_settings,
    validate,
};
pub use when::{When, WhenError, parse_when};

use diagnostic::LineIndex;

/// One error diagnostic per syntax error.
fn syntax_diagnostics(index: &LineIndex<'_>, errors: &[jsonc::ParseError]) -> Vec<Diagnostic> {
    errors
        .iter()
        .map(|e| {
            index.diagnostic(
                DiagnosticKind::Syntax(e.error),
                Severity::Error,
                e.span.start,
                e.error.to_string(),
            )
        })
        .collect()
}
