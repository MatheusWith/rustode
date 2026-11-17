//! Both files together, reloaded on demand with a report of what changed.

use crate::diagnostic::{Diagnostic, DiagnosticKind, Location, Severity};
use crate::jsonc::Value;
use crate::keybindings::{Keybinding, Keybindings, parse_keybindings};
use crate::settings::{Schema, SettingsFile, parse_settings};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Which configuration file a diagnostic belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigFile {
    /// `settings.json`.
    Settings,
    /// `keybindings.json`.
    Keybindings,
}

/// A setting whose value changed in a reload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SettingChange {
    /// The language of a `"[language]"` block, or `None` for a top-level setting.
    pub language: Option<String>,
    /// The setting id.
    pub key: String,
}

/// What a load or reload changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigChange {
    /// Settings whose value changed: top-level ones first, then overrides by language, each
    /// sorted by id.
    pub settings: Vec<SettingChange>,
    /// Whether any keybinding was added, removed or changed. Moving entries around or editing
    /// comments and whitespace is not a change.
    pub keybindings: bool,
    /// Whether the diagnostics of either file changed.
    pub diagnostics: bool,
}

impl ConfigChange {
    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && !self.keybindings && !self.diagnostics
    }
}

/// The user's settings and keybindings, read from their files.
///
/// VS Code reloads both files when the file service reports a change and fires an event only if
/// the parsed content differs (`UserKeybindings.reload`,
/// `src/vs/workbench/services/keybinding/browser/keybindingService.ts:809`). Here the caller
/// decides when to reload (from a file watcher, or after saving one of the files) and gets back
/// the difference; [`Config::reload`] never needs a clock, so tests are deterministic.
#[derive(Debug, Clone, Default)]
pub struct Config {
    schema: Schema,
    settings_path: Option<PathBuf>,
    keybindings_path: Option<PathBuf>,
    settings: SettingsFile,
    keybindings: Keybindings,
    settings_io: Option<Diagnostic>,
    keybindings_io: Option<Diagnostic>,
}

impl Config {
    /// An empty configuration checked against `schema`.
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            ..Self::default()
        }
    }

    /// Reads both files from these paths and remembers them for [`Config::reload`].
    pub fn load(&mut self, settings: &Path, keybindings: &Path) -> ConfigChange {
        self.settings_path = Some(settings.to_owned());
        self.keybindings_path = Some(keybindings.to_owned());
        self.reload()
    }

    /// Reads both files again. A missing file counts as empty. When a file cannot be read for
    /// another reason, its previous content is kept and an [`DiagnosticKind::Io`] error is
    /// reported.
    pub fn reload(&mut self) -> ConfigChange {
        let settings = self.settings_path.as_deref().map(read);
        let keybindings = self.keybindings_path.as_deref().map(read);
        let before = self
            .diagnostics()
            .map(|(f, d)| (f, d.clone()))
            .collect::<Vec<_>>();

        let mut change = ConfigChange::default();
        if let Some(read) = settings {
            match read {
                Ok(text) => {
                    let new = parse_settings(text.as_deref().unwrap_or(""), &self.schema);
                    change.settings = setting_changes(&self.settings, &new);
                    self.settings = new;
                    self.settings_io = None;
                }
                Err(message) => self.settings_io = Some(io_diagnostic(&message)),
            }
        }
        if let Some(read) = keybindings {
            match read {
                Ok(text) => {
                    let new = parse_keybindings(text.as_deref().unwrap_or(""));
                    change.keybindings = !same_bindings(&self.keybindings.bindings, &new.bindings);
                    self.keybindings = new;
                    self.keybindings_io = None;
                }
                Err(message) => self.keybindings_io = Some(io_diagnostic(&message)),
            }
        }
        change.diagnostics = !self.diagnostics().map(|(f, d)| (f, d.clone())).eq(before);
        change
    }

    /// Parses settings from text instead of a file, as [`Config::reload`] would.
    pub fn set_settings_text(&mut self, text: &str) -> ConfigChange {
        let new = parse_settings(text, &self.schema);
        let change = ConfigChange {
            settings: setting_changes(&self.settings, &new),
            keybindings: false,
            diagnostics: new.diagnostics != self.settings.diagnostics,
        };
        self.settings = new;
        change
    }

    /// Parses keybindings from text instead of a file, as [`Config::reload`] would.
    pub fn set_keybindings_text(&mut self, text: &str) -> ConfigChange {
        let new = parse_keybindings(text);
        let change = ConfigChange {
            settings: Vec::new(),
            keybindings: !same_bindings(&self.keybindings.bindings, &new.bindings),
            diagnostics: new.diagnostics != self.keybindings.diagnostics,
        };
        self.keybindings = new;
        change
    }

    /// The schema settings are checked against.
    #[must_use]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The parsed settings.
    #[must_use]
    pub fn settings(&self) -> &SettingsFile {
        &self.settings
    }

    /// The usable keybindings, in file order.
    #[must_use]
    pub fn keybindings(&self) -> &[Keybinding] {
        &self.keybindings.bindings
    }

    /// The value of a setting: the user's, or the schema default. Unknown settings have a value
    /// only when the user set one.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&Value> {
        self.settings
            .settings
            .values
            .get(key)
            .or_else(|| self.schema.get(key).map(|s| &s.default))
    }

    /// The value of a setting for a language: its `"[language]"` override, or [`Config::value`].
    #[must_use]
    pub fn language_value(&self, language: &str, key: &str) -> Option<&Value> {
        self.settings
            .settings
            .overrides
            .get(language)
            .and_then(|o| o.get(key))
            .or_else(|| self.value(key))
    }

    /// Every diagnostic: settings first, then keybindings, each in file order.
    pub fn diagnostics(&self) -> impl Iterator<Item = (ConfigFile, &Diagnostic)> {
        let settings = self
            .settings_io
            .iter()
            .chain(&self.settings.diagnostics)
            .map(|d| (ConfigFile::Settings, d));
        let keybindings = self
            .keybindings_io
            .iter()
            .chain(&self.keybindings.diagnostics)
            .map(|d| (ConfigFile::Keybindings, d));
        settings.chain(keybindings)
    }
}

/// The file's text, `None` when it does not exist, or why it could not be read. Invalid UTF-8 is
/// replaced rather than refused.
fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

fn io_diagnostic(message: &str) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Io,
        severity: Severity::Error,
        position: Location { line: 1, column: 1 },
        message: format!("Could not read the file; the previous content is kept ({message})"),
    }
}

fn same_bindings(a: &[Keybinding], b: &[Keybinding]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.same_binding(y))
}

fn setting_changes(old: &SettingsFile, new: &SettingsFile) -> Vec<SettingChange> {
    let (old, new) = (&old.settings, &new.settings);
    let mut changes = BTreeSet::new();
    for key in old.values.keys().chain(new.values.keys()) {
        if old.values.get(key) != new.values.get(key) {
            changes.insert(SettingChange {
                language: None,
                key: key.clone(),
            });
        }
    }
    for language in old.overrides.keys().chain(new.overrides.keys()) {
        let empty = std::collections::BTreeMap::new();
        let before = old.overrides.get(language).unwrap_or(&empty);
        let after = new.overrides.get(language).unwrap_or(&empty);
        for key in before.keys().chain(after.keys()) {
            if before.get(key) != after.get(key) {
                changes.insert(SettingChange {
                    language: Some(language.clone()),
                    key: key.clone(),
                });
            }
        }
    }
    changes.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_updates_report_changes() {
        let mut config = Config::new(Schema::builtin());
        let change = config.set_settings_text("{\"editor.tabSize\": 2}");
        assert_eq!(change.settings.len(), 1);
        assert!(
            config
                .set_settings_text("{ \"editor.tabSize\": 2 } // same")
                .is_empty()
        );
        let change = config.set_keybindings_text("[{\"key\": \"f5\", \"command\": \"x\"}]");
        assert!(change.keybindings && change.settings.is_empty());
        let change = config.set_keybindings_text("[{\"key\": \"f5\", \"command\": \"x\"} 1]");
        assert!(!change.keybindings && change.diagnostics);
    }

    #[test]
    fn unreadable_files_keep_the_previous_content() {
        let dir = std::env::temp_dir().join(format!("rustode-config-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let settings = dir.join("settings.json");
        let keybindings = dir.join("keybindings.json");
        std::fs::write(&settings, "{\"editor.tabSize\": 2}").expect("write");
        let mut config = Config::default();
        config.load(&settings, &keybindings);

        // A directory where the file should be cannot be read as a file.
        std::fs::remove_file(&settings).expect("remove");
        std::fs::create_dir(&settings).expect("dir in place of file");
        let change = config.reload();
        assert!(change.diagnostics && change.settings.is_empty());
        assert_eq!(config.value("editor.tabSize"), Some(&Value::Number(2.0)));
        let (file, d) = config.diagnostics().next().expect("io diagnostic");
        assert_eq!((file, d.kind), (ConfigFile::Settings, DiagnosticKind::Io));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_settings_have_no_default() {
        let config = Config::default();
        assert_eq!(config.value("nope"), None);
        assert_eq!(config.language_value("rust", "nope"), None);
    }
}
