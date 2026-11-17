//! Workspace seam: settings.json and keybindings.json in VS Code's format.

use rustode_workspace::{
    ConfigFile, DiagnosticKind, SettingChange, Severity, Value, When, Workspace,
};
use std::path::{Path, PathBuf};

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rustode-cfg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self { dir }
    }

    fn settings(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn keybindings(&self) -> PathBuf {
        self.dir.join("keybindings.json")
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).expect("write fixture");
    }

    fn workspace(&self, settings: &str, keybindings: &str) -> Workspace {
        Self::write(&self.settings(), settings);
        Self::write(&self.keybindings(), keybindings);
        let mut ws = Workspace::new();
        ws.load_config(&self.settings(), &self.keybindings());
        ws
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    }
}

#[test]
fn settings_with_comments_and_trailing_commas_load() {
    let fx = Fixture::new("jsonc");
    let ws = fx.workspace(
        "// user settings\n{\n  /* indentation */\n  \"editor.tabSize\": 2,\n  \"editor.insertSpaces\": false,\n}\n",
        "[]",
    );
    let config = ws.config();
    assert_eq!(number(config.value("editor.tabSize")), Some(2.0));
    assert_eq!(
        config.value("editor.insertSpaces"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        config.diagnostics().count(),
        0,
        "{:?}",
        config.diagnostics().collect::<Vec<_>>()
    );
}

#[test]
fn defaults_apply_when_files_are_missing() {
    let fx = Fixture::new("missing");
    let mut ws = Workspace::new();
    ws.load_config(&fx.settings(), &fx.keybindings());
    assert_eq!(number(ws.config().value("editor.tabSize")), Some(4.0));
    assert_eq!(ws.config().keybindings().len(), 0);
    assert_eq!(ws.config().diagnostics().count(), 0);
}

#[test]
fn unknown_setting_is_reported_at_its_key_and_kept() {
    let fx = Fixture::new("unknown");
    let ws = fx.workspace(
        "{\n  \"editor.tabSize\": 2,\n  \"editr.fontSize\": 14\n}",
        "[]",
    );
    let diagnostics: Vec<_> = ws.config().diagnostics().collect();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let (file, d) = diagnostics[0];
    assert_eq!(file, ConfigFile::Settings);
    assert_eq!(d.kind, DiagnosticKind::UnknownSetting);
    assert_eq!(d.severity, Severity::Hint);
    assert_eq!((d.position.line, d.position.column), (3, 3));
    assert_eq!(number(ws.config().value("editr.fontSize")), Some(14.0));
}

#[test]
fn invalid_values_are_reported_at_the_value_and_fall_back_to_the_default() {
    let fx = Fixture::new("invalid");
    let ws = fx.workspace(
        "{\n  \"editor.tabSize\": \"two\",\n  \"files.eol\": \"lf\",\n  \"editor.insertSpaces\": 1\n}",
        "[]",
    );
    let config = ws.config();
    let found: Vec<_> = config
        .diagnostics()
        .map(|(_, d)| (d.kind, d.severity, d.position.line, d.position.column))
        .collect();
    assert_eq!(
        found,
        vec![
            (DiagnosticKind::InvalidValue, Severity::Warning, 2, 21),
            (DiagnosticKind::InvalidValue, Severity::Warning, 3, 16),
            (DiagnosticKind::InvalidValue, Severity::Warning, 4, 26),
        ]
    );
    assert_eq!(number(config.value("editor.tabSize")), Some(4.0));
    assert_eq!(
        config.value("files.eol"),
        Some(&Value::String("auto".into()))
    );
    assert_eq!(
        config.value("editor.insertSpaces"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn out_of_range_number_is_reported() {
    let fx = Fixture::new("range");
    let ws = fx.workspace("{ \"editor.tabSize\": 0 }", "[]");
    let (_, d) = ws.config().diagnostics().next().expect("one diagnostic");
    assert_eq!(d.kind, DiagnosticKind::InvalidValue);
    assert!(d.message.contains("minimum"), "{}", d.message);
    assert_eq!(number(ws.config().value("editor.tabSize")), Some(4.0));
}

#[test]
fn syntax_errors_are_reported_with_location_and_the_rest_still_loads() {
    let fx = Fixture::new("syntax");
    let ws = fx.workspace(
        "{\n  \"editor.tabSize\": 2\n  \"editor.insertSpaces\": false,\n  \"files.eol\": \n}",
        "[]",
    );
    let config = ws.config();
    let syntax: Vec<_> = config
        .diagnostics()
        .filter(|(_, d)| matches!(d.kind, DiagnosticKind::Syntax(_)))
        .map(|(_, d)| (d.severity, d.position.line, d.position.column))
        .collect();
    assert_eq!(
        syntax,
        vec![(Severity::Error, 3, 3), (Severity::Error, 5, 1)]
    );
    assert_eq!(number(config.value("editor.tabSize")), Some(2.0));
    assert_eq!(
        config.value("editor.insertSpaces"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn garbage_never_crashes() {
    let fx = Fixture::new("garbage");
    let deep = "[".repeat(100_000);
    for text in [
        "",
        "   ",
        "\u{feff}{}",
        "{\"a\": \"unterminated",
        "/* open comment",
        "}{][,:",
        "[1, 2",
        "nul",
        "{\"editor.tabSize\": -}",
        deep.as_str(),
    ] {
        let ws = fx.workspace(text, text);
        let _ = ws.config().diagnostics().count();
    }
    Fixture::write(&fx.settings(), "");
    std::fs::write(fx.keybindings(), [0xFF, 0xFE, 0x00, 0x5B]).expect("write");
    let mut ws = Workspace::new();
    ws.load_config(&fx.settings(), &fx.keybindings());
}

#[test]
fn keybindings_with_when_clauses_are_parsed() {
    let fx = Fixture::new("keys");
    let ws = fx.workspace(
        "{}",
        r#"// keybindings
[
  { "key": "ctrl+shift+k", "command": "editor.action.deleteLines",
    "when": "textInputFocus && !editorReadonly" },
  { "key": "ctrl+k ctrl+c", "command": "editor.action.addCommentLine",
    "when": "editorLangId in supportedLangs || resourceExtname =~ /\\.rs$/i" },
  { "key": "ctrl+d", "command": "-editor.action.addSelectionToNextFindMatch" },
  { "key": "alt+f1", "command": "workbench.action.tasks.runTask", "args": "build", },
]
"#,
    );
    let config = ws.config();
    assert_eq!(
        config.diagnostics().count(),
        0,
        "{:?}",
        config.diagnostics().collect::<Vec<_>>()
    );
    let bindings = config.keybindings();
    assert_eq!(bindings.len(), 4);

    let first = &bindings[0];
    assert_eq!(first.command.as_deref(), Some("editor.action.deleteLines"));
    let chord = &first.key.as_ref().expect("key")[0];
    assert!(chord.ctrl && chord.shift && !chord.alt && !chord.meta);
    assert_eq!(chord.key, "k");
    assert_eq!(
        first.when,
        Some(When::And(vec![
            When::Has("textInputFocus".into()),
            When::Not("editorReadonly".into()),
        ]))
    );

    let second = &bindings[1];
    assert_eq!(second.key.as_ref().expect("key").len(), 2);
    assert_eq!(
        second.when,
        Some(When::Or(vec![
            When::In {
                key: "editorLangId".into(),
                container: "supportedLangs".into()
            },
            When::Regex {
                key: "resourceExtname".into(),
                pattern: "\\.rs$".into(),
                flags: "i".into(),
            },
        ]))
    );

    assert!(bindings[2].is_removal());
    assert_eq!(bindings[3].args, Some(Value::String("build".into())));
}

#[test]
fn invalid_when_clause_is_reported_inside_the_string() {
    let fx = Fixture::new("badwhen");
    let ws = fx.workspace(
        "{}",
        "[\n  { \"key\": \"f5\", \"command\": \"run\", \"when\": \"a && && b\" },\n  { \"key\": \"f6\", \"command\": \"ok\" }\n]",
    );
    let config = ws.config();
    let diagnostics: Vec<_> = config.diagnostics().collect();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let (file, d) = diagnostics[0];
    assert_eq!(file, ConfigFile::Keybindings);
    assert_eq!(d.kind, DiagnosticKind::InvalidWhen);
    assert_eq!(d.severity, Severity::Error);
    // The string after `"when": ` opens at column 44; the second `&&` is at column 50.
    assert_eq!((d.position.line, d.position.column), (2, 50));
    let commands: Vec<_> = config
        .keybindings()
        .iter()
        .filter_map(|k| k.command.as_deref())
        .collect();
    assert_eq!(commands, vec!["ok"]);
}

#[test]
fn keybindings_that_are_not_an_array_are_reported() {
    let fx = Fixture::new("notarray");
    let ws = fx.workspace("{}", "{ \"key\": \"f5\" }");
    let (file, d) = ws.config().diagnostics().next().expect("diagnostic");
    assert_eq!(file, ConfigFile::Keybindings);
    assert_eq!(d.kind, DiagnosticKind::InvalidKeybinding);
    assert_eq!(ws.config().keybindings().len(), 0);
}

#[test]
fn language_overrides_apply_to_that_language_only() {
    let fx = Fixture::new("override");
    let ws = fx.workspace(
        "{ \"editor.tabSize\": 2, \"[rust][toml]\": { \"editor.tabSize\": 4, }, \"[markdown]\": 3 }",
        "[]",
    );
    let config = ws.config();
    assert_eq!(number(config.value("editor.tabSize")), Some(2.0));
    assert_eq!(
        number(config.language_value("rust", "editor.tabSize")),
        Some(4.0)
    );
    assert_eq!(
        number(config.language_value("toml", "editor.tabSize")),
        Some(4.0)
    );
    assert_eq!(
        number(config.language_value("go", "editor.tabSize")),
        Some(2.0)
    );
    let (_, d) = config.diagnostics().next().expect("bad override reported");
    assert_eq!(d.kind, DiagnosticKind::InvalidValue);
}

#[test]
fn reload_reports_exactly_what_changed() {
    let fx = Fixture::new("reload");
    let mut ws = fx.workspace(
        "{ \"editor.tabSize\": 2, \"files.eol\": \"\\n\" }",
        "[{ \"key\": \"f5\", \"command\": \"a\" }]",
    );

    assert!(ws.reload_config().is_empty(), "nothing changed on disk");

    Fixture::write(
        &fx.settings(),
        "{ \"editor.tabSize\": 8, \"files.eol\": \"\\n\", \"[rust]\": { \"editor.tabSize\": 4 } }",
    );
    let change = ws.reload_config();
    assert_eq!(
        change.settings,
        vec![
            SettingChange {
                language: None,
                key: "editor.tabSize".into()
            },
            SettingChange {
                language: Some("rust".into()),
                key: "editor.tabSize".into()
            },
        ]
    );
    assert!(!change.keybindings);
    assert_eq!(number(ws.config().value("editor.tabSize")), Some(8.0));

    Fixture::write(
        &fx.keybindings(),
        "[{ \"key\": \"f5\", \"command\": \"b\" }]",
    );
    let change = ws.reload_config();
    assert_eq!(change.settings, vec![]);
    assert!(change.keybindings);
    assert_eq!(ws.config().keybindings()[0].command.as_deref(), Some("b"));

    // Only formatting and comments changed: no setting or keybinding changed.
    Fixture::write(
        &fx.keybindings(),
        "[\n  // same\n  { \"command\": \"b\", \"key\": \"f5\" },\n]",
    );
    assert!(ws.reload_config().is_empty());
}

#[test]
fn reload_after_a_broken_edit_reports_errors_then_recovers() {
    let fx = Fixture::new("recover");
    let mut ws = fx.workspace("{ \"editor.tabSize\": 2 }", "[]");
    Fixture::write(&fx.settings(), "{ \"editor.tabSize\": 3,, }");
    let change = ws.reload_config();
    assert!(change.diagnostics);
    assert_eq!(number(ws.config().value("editor.tabSize")), Some(3.0));
    assert_ne!(ws.config().diagnostics().count(), 0);

    Fixture::write(&fx.settings(), "{ \"editor.tabSize\": 3 }");
    let change = ws.reload_config();
    assert!(change.diagnostics);
    assert_eq!(change.settings, vec![]);
    assert_eq!(ws.config().diagnostics().count(), 0);
}

#[test]
fn deleting_settings_resets_to_defaults() {
    let fx = Fixture::new("delete");
    let mut ws = fx.workspace("{ \"editor.tabSize\": 2 }", "[]");
    std::fs::remove_file(fx.settings()).expect("remove");
    let change = ws.reload_config();
    assert_eq!(
        change.settings,
        vec![SettingChange {
            language: None,
            key: "editor.tabSize".into()
        }]
    );
    assert_eq!(number(ws.config().value("editor.tabSize")), Some(4.0));
}
