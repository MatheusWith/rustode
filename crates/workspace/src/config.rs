//! Settings and keybindings, loaded from VS Code-format files and reloaded on demand.

pub use rustode_config::{
    Chord, Config, ConfigChange, ConfigFile, ContextValue, Diagnostic, DiagnosticKind, Keybinding,
    Location, SettingChange, Severity, Value, When,
};

use crate::Workspace;
use std::path::Path;

impl Workspace {
    /// Reads `settings.json` and `keybindings.json` from these paths and remembers them for
    /// [`Workspace::reload_config`]. A missing file means "no user configuration"; every
    /// problem in a file is reported through [`Config::diagnostics`], never as a failure.
    pub fn load_config(&mut self, settings: &Path, keybindings: &Path) -> ConfigChange {
        let change = self.config.load(settings, keybindings);
        log_change(&self.config, &change);
        self.lsp_config_changed();
        self.dap_config_changed();
        self.associations_changed();
        change
    }

    /// Reads both files again and reports what changed since the last load. Frontends call this
    /// when a file watcher fires, or after the user saves either file in the editor.
    pub fn reload_config(&mut self) -> ConfigChange {
        let change = self.config.reload();
        log_change(&self.config, &change);
        if !change.settings.is_empty() {
            self.lsp_config_changed();
            self.dap_config_changed();
            self.associations_changed();
        }
        change
    }

    /// The current settings, keybindings and their diagnostics.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }
}

fn log_change(config: &Config, change: &ConfigChange) {
    if !change.is_empty() {
        tracing::debug!(
            settings = change.settings.len(),
            keybindings = change.keybindings,
            diagnostics = config.diagnostics().count(),
            "config reloaded"
        );
    }
}
