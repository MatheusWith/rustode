//! `settings.json`: an object of dotted setting ids, with `"[language]"` override blocks.

use crate::diagnostic::{Diagnostic, DiagnosticKind, LineIndex, Severity};
use crate::jsonc::{self, Node, NodeKind, Property, Value};
use crate::syntax_diagnostics;
use std::collections::{BTreeMap, HashSet};

/// The type a setting's value must have.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingType {
    /// `true` or `false`.
    Boolean,
    /// Any number within the bounds.
    Number {
        /// Smallest accepted value.
        minimum: Option<f64>,
        /// Largest accepted value.
        maximum: Option<f64>,
    },
    /// A whole number within the bounds.
    Integer {
        /// Smallest accepted value.
        minimum: Option<f64>,
        /// Largest accepted value.
        maximum: Option<f64>,
    },
    /// Any string.
    String,
    /// One of these strings.
    Enum(Vec<String>),
    /// Any array.
    Array,
    /// Any object.
    Object,
    /// Anything.
    Any,
}

/// A registered setting.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSchema {
    /// The dotted id, such as `editor.tabSize`.
    pub key: String,
    /// What values are accepted.
    pub kind: SettingType,
    /// The value used when the user sets none, or an invalid one.
    pub default: Value,
}

/// The registered settings. [`Schema::default`] holds the settings rustode implements, with VS
/// Code's types and defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    settings: BTreeMap<String, SettingSchema>,
}

impl Default for Schema {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Schema {
    /// A schema with no settings: every setting is unknown.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            settings: BTreeMap::new(),
        }
    }

    /// The settings rustode implements so far, with VS Code's types, bounds and defaults:
    /// - `editor.tabSize`, `editor.insertSpaces`, `editor.detectIndentation`:
    ///   `src/vs/editor/common/config/editorConfigurationSchema.ts:25`, `:46`, `:51`, defaults
    ///   from `src/vs/editor/common/core/misc/textModelDefaults.ts:6`;
    /// - `files.encoding`, `files.eol`, `files.trimTrailingWhitespace`,
    ///   `files.insertFinalNewline`, `files.autoSave`, `files.autoSaveDelay`:
    ///   `src/vs/workbench/contrib/files/browser/files.contribution.ts:197`, `:223`, `:244`,
    ///   `:256`, `:268`, `:282`. `files.encoding` accepts any string here; VS Code restricts it
    ///   to its list of encodings;
    /// - `rustode.languageServers`, rustode's own (VS Code has no built-in setting for language
    ///   servers): an object of server entries by language id, which the workspace checks.
    #[must_use]
    pub fn builtin() -> Self {
        let mut schema = Self::empty();
        let number = |minimum, maximum| SettingType::Number { minimum, maximum };
        let enumeration =
            |values: &[&str]| SettingType::Enum(values.iter().map(|v| (*v).to_owned()).collect());
        let text = |s: &str| Value::String(s.to_owned());
        for (key, kind, default) in [
            (
                "editor.tabSize",
                number(Some(1.0), Some(100.0)),
                Value::Number(4.0),
            ),
            (
                "editor.insertSpaces",
                SettingType::Boolean,
                Value::Bool(true),
            ),
            (
                "editor.detectIndentation",
                SettingType::Boolean,
                Value::Bool(true),
            ),
            ("files.encoding", SettingType::String, text("utf8")),
            (
                "files.eol",
                enumeration(&["\n", "\r\n", "auto"]),
                text("auto"),
            ),
            (
                "files.trimTrailingWhitespace",
                SettingType::Boolean,
                Value::Bool(false),
            ),
            (
                "files.insertFinalNewline",
                SettingType::Boolean,
                Value::Bool(false),
            ),
            (
                "files.autoSave",
                enumeration(&["off", "afterDelay", "onFocusChange", "onWindowChange"]),
                text("off"),
            ),
            (
                "files.autoSaveDelay",
                number(Some(0.0), None),
                Value::Number(1000.0),
            ),
            (
                "rustode.languageServers",
                SettingType::Object,
                Value::Object(BTreeMap::new()),
            ),
        ] {
            schema.register(SettingSchema {
                key: key.to_owned(),
                kind,
                default,
            });
        }
        schema
    }

    /// Adds or replaces a setting.
    pub fn register(&mut self, setting: SettingSchema) {
        self.settings.insert(setting.key.clone(), setting);
    }

    /// A registered setting.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SettingSchema> {
        self.settings.get(key)
    }

    /// Every registered setting, sorted by id.
    pub fn iter(&self) -> impl Iterator<Item = &SettingSchema> {
        self.settings.values()
    }
}

/// Checks a value against a setting type; the error is a sentence for the user.
///
/// # Errors
/// When the value has the wrong type, is out of bounds or is not one of the allowed strings.
pub fn validate(kind: &SettingType, value: &Value) -> Result<(), String> {
    let wrong = |expected: &str| {
        Err(format!(
            "Incorrect type. Expected \"{expected}\", found {}",
            value.type_name()
        ))
    };
    match (kind, value) {
        (SettingType::Any, _)
        | (SettingType::Boolean, Value::Bool(_))
        | (SettingType::String, Value::String(_))
        | (SettingType::Array, Value::Array(_))
        | (SettingType::Object, Value::Object(_)) => Ok(()),
        (
            SettingType::Number { minimum, maximum } | SettingType::Integer { minimum, maximum },
            Value::Number(n),
        ) => {
            if matches!(kind, SettingType::Integer { .. }) && n.fract() != 0.0 {
                return wrong("integer");
            }
            if let Some(min) = minimum
                && n < min
            {
                return Err(format!("Value is below the minimum of {min}"));
            }
            if let Some(max) = maximum
                && n > max
            {
                return Err(format!("Value is above the maximum of {max}"));
            }
            Ok(())
        }
        (SettingType::Enum(values), Value::String(s)) if values.contains(s) => Ok(()),
        (SettingType::Enum(values), _) => {
            let accepted: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
            Err(format!(
                "Value is not accepted. Valid values: {}",
                accepted.join(", ")
            ))
        }
        (SettingType::Boolean, _) => wrong("boolean"),
        (SettingType::Number { .. }, _) => wrong("number"),
        (SettingType::Integer { .. }, _) => wrong("integer"),
        (SettingType::String, _) => wrong("string"),
        (SettingType::Array, _) => wrong("array"),
        (SettingType::Object, _) => wrong("object"),
    }
}

/// The values the user set, after validation: invalid values are left out so the default applies,
/// unknown settings are kept as written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    /// Top-level values by setting id.
    pub values: BTreeMap<String, Value>,
    /// Values from `"[language]"` blocks, by language id, then setting id.
    pub overrides: BTreeMap<String, BTreeMap<String, Value>>,
}

/// The parsed file: the values and every problem found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsFile {
    /// The values.
    pub settings: Settings,
    /// Problems in file order.
    pub diagnostics: Vec<Diagnostic>,
}

/// The language ids of an override key such as `[rust]` or `[rust][toml]`
/// (`OVERRIDE_PROPERTY_PATTERN`, `src/vs/platform/configuration/common/configurationRegistry.ts:1136`).
#[must_use]
pub fn override_languages(key: &str) -> Option<Vec<String>> {
    let mut rest = key;
    let mut languages = Vec::new();
    while !rest.is_empty() {
        let inner = rest.strip_prefix('[')?;
        let close = inner.find(']')?;
        if close == 0 {
            return None;
        }
        languages.push(inner[..close].to_owned());
        rest = &inner[close + 1..];
    }
    (!languages.is_empty()).then_some(languages)
}

/// Parses `settings.json` against a schema. Never fails: problems become diagnostics.
///
/// The file is read with error recovery, so a typo only loses the value it touches, as in
/// `ConfigurationModelParser.doParseContent`
/// (`src/vs/platform/configuration/common/configurationModels.ts:361`). Unknown settings are
/// hints and keep their value, as in VS Code's settings editor (`preferencesRenderers.ts:742`).
/// Values that do not match the schema are warnings and are not used; VS Code keeps them and
/// leaves each consumer to cope.
#[must_use]
pub fn parse_settings(text: &str, schema: &Schema) -> SettingsFile {
    let parsed = jsonc::parse(text);
    let index = LineIndex::new(text);
    let mut file = SettingsFile {
        settings: Settings::default(),
        diagnostics: syntax_diagnostics(&index, &parsed.errors),
    };
    let Some(root) = parsed.root else {
        return file;
    };
    let NodeKind::Object(properties) = &root.kind else {
        file.diagnostics.push(index.diagnostic(
            DiagnosticKind::InvalidValue,
            Severity::Error,
            root.span.start,
            format!(
                "settings.json must be an object, not {}",
                root.to_value().type_name()
            ),
        ));
        return file;
    };
    let mut reader = Reader {
        schema,
        index: &index,
        diagnostics: &mut file.diagnostics,
    };
    reader.duplicates(properties);
    for property in properties {
        let Some(value) = &property.value else {
            continue;
        };
        if let Some(languages) = override_languages(&property.key) {
            let NodeKind::Object(inner) = &value.kind else {
                reader.invalid(
                    value,
                    format!(
                        "A language override must be an object, not {}",
                        value.to_value().type_name()
                    ),
                );
                continue;
            };
            reader.duplicates(inner);
            let mut block = BTreeMap::new();
            for property in inner {
                reader.read(property, &mut block);
            }
            for language in languages {
                file.settings
                    .overrides
                    .entry(language)
                    .or_default()
                    .extend(block.clone());
            }
        } else {
            reader.read(property, &mut file.settings.values);
        }
    }
    file.diagnostics.sort_by_key(|d| d.position);
    file
}

struct Reader<'a> {
    schema: &'a Schema,
    index: &'a LineIndex<'a>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Reader<'_> {
    fn invalid(&mut self, node: &Node, message: String) {
        self.diagnostics.push(self.index.diagnostic(
            DiagnosticKind::InvalidValue,
            Severity::Warning,
            node.span.start,
            message,
        ));
    }

    /// Validates one property and stores its value when it is usable.
    fn read(&mut self, property: &Property, into: &mut BTreeMap<String, Value>) {
        let Some(node) = &property.value else {
            return;
        };
        let value = node.to_value();
        match self.schema.get(&property.key) {
            None => {
                self.diagnostics.push(self.index.diagnostic(
                    DiagnosticKind::UnknownSetting,
                    Severity::Hint,
                    property.key_span.start,
                    format!("Unknown Configuration Setting \"{}\"", property.key),
                ));
            }
            Some(setting) => {
                if let Err(message) = validate(&setting.kind, &value) {
                    self.invalid(node, message);
                    into.remove(&property.key);
                    return;
                }
            }
        }
        into.insert(property.key.clone(), value);
    }

    /// Reports every repeated key but the last, which is the one that counts.
    fn duplicates(&mut self, properties: &[Property]) {
        let mut seen = HashSet::new();
        for property in properties.iter().rev() {
            if !seen.insert(property.key.as_str()) {
                self.diagnostics.push(self.index.diagnostic(
                    DiagnosticKind::DuplicateKey,
                    Severity::Warning,
                    property.key_span.start,
                    format!(
                        "Duplicate key \"{}\"; a later one replaces it",
                        property.key
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(file: &SettingsFile) -> Vec<(DiagnosticKind, usize, usize)> {
        file.diagnostics
            .iter()
            .map(|d| (d.kind, d.position.line, d.position.column))
            .collect()
    }

    #[test]
    fn values_are_validated_against_the_schema() {
        let integer = SettingType::Integer {
            minimum: None,
            maximum: Some(3.0),
        };
        assert!(validate(&integer, &Value::Number(2.0)).is_ok());
        assert!(validate(&integer, &Value::Number(2.5)).is_err());
        assert!(validate(&integer, &Value::Number(4.0)).is_err());
        assert!(validate(&SettingType::Array, &Value::Array(vec![])).is_ok());
        assert!(validate(&SettingType::Object, &Value::Null).is_err());
        assert!(validate(&SettingType::String, &Value::Bool(true)).is_err());
        assert!(validate(&SettingType::Any, &Value::Null).is_ok());
        let message = validate(&SettingType::Boolean, &Value::Number(1.0))
            .expect_err("a number is not a boolean");
        assert_eq!(
            message,
            "Incorrect type. Expected \"boolean\", found number"
        );
    }

    #[test]
    fn override_keys() {
        assert_eq!(override_languages("[rust]"), Some(vec!["rust".into()]));
        assert_eq!(
            override_languages("[rust][toml]"),
            Some(vec!["rust".into(), "toml".into()])
        );
        assert_eq!(override_languages("[]"), None);
        assert_eq!(override_languages("[rust"), None);
        assert_eq!(override_languages("[rust]x"), None);
        assert_eq!(override_languages("editor.tabSize"), None);
    }

    #[test]
    fn duplicate_keys_keep_the_last_value() {
        let file = parse_settings(
            "{\"editor.tabSize\": 2, \"editor.tabSize\": 3}",
            &Schema::builtin(),
        );
        assert_eq!(file.settings.values["editor.tabSize"], Value::Number(3.0));
        assert_eq!(kinds(&file), vec![(DiagnosticKind::DuplicateKey, 1, 2)]);
    }

    #[test]
    fn a_later_invalid_value_does_not_keep_an_earlier_valid_one() {
        let file = parse_settings(
            "{\"editor.tabSize\": 2, \"editor.tabSize\": \"x\"}",
            &Schema::builtin(),
        );
        assert!(!file.settings.values.contains_key("editor.tabSize"));
    }

    #[test]
    fn the_root_must_be_an_object() {
        let file = parse_settings("[1]", &Schema::builtin());
        assert_eq!(kinds(&file), vec![(DiagnosticKind::InvalidValue, 1, 1)]);
        assert_eq!(file.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn an_empty_schema_reports_everything_as_unknown() {
        let file = parse_settings("{\"a\": 1, \"[x]\": {\"b\": 2}}", &Schema::empty());
        assert_eq!(
            kinds(&file),
            vec![
                (DiagnosticKind::UnknownSetting, 1, 2),
                (DiagnosticKind::UnknownSetting, 1, 18),
            ]
        );
        assert_eq!(file.settings.overrides["x"]["b"], Value::Number(2.0));
    }

    #[test]
    fn builtin_defaults_are_valid() {
        for setting in Schema::builtin().iter() {
            assert!(
                validate(&setting.kind, &setting.default).is_ok(),
                "{}",
                setting.key
            );
        }
    }
}
