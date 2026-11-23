//! An extension's `package.json`, modelled on VS Code's `IExtensionManifest`
//! (`src/vs/platform/extensions/common/extensions.ts:362-394`), and the
//! checks VS Code's scanner runs on it (`validateExtensionManifest`,
//! `src/vs/platform/extensions/common/extensionValidator.ts:242-379`;
//! `src/vs/platform/extensionManagement/common/extensionsScannerService.ts:755-767`).
//!
//! The model is a typed view over the whole JSON object: every field stays
//! as it was read, in its order, so nothing in `package.json` is ever
//! dropped, and the fields VS Code models read through typed accessors. A
//! field whose value has another type than VS Code's reads as absent and
//! stays in the object (and [`ExtensionManifest::validate`] reports it when
//! VS Code would).
//!
//! Validation reports problems; it never fails. As in VS Code, the first
//! error ends the checks, warnings do not, and `engines.vscode` is checked
//! only for extensions with code (`main` or `browser`), against
//! [`VSCODE_API_VERSION`] and [`VSCODE_API_DATE`]: `*`, `x` parts, `^` and
//! `>=` ranges, the rule that `0.x` engines name the minor version and
//! later ones the major, and a `-YYYYMMDD[HH[MM]]` suffix that a build must
//! not predate (`extensionValidator.ts:36-238`, `:333-379`). Versions must
//! be strict `SemVer` as the `semver` 5.5.0 module VS Code bundles reads them
//! (`src/vs/base/common/semver/semver.js`).

use crate::Severity;
use regex::Regex;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::LazyLock;

/// The VS Code API version installed extensions are checked against: the
/// App's, `VSCodium` 1.147.0 (`02-package-analysis/product.json:478`). A
/// later extension host implements this API.
pub const VSCODE_API_VERSION: &str = "1.147.0";
/// The date of that API, in UTC: the App's build, `2026-11-12T17:39:10+01:00`
/// (`02-package-analysis/product.json:465`), as year, month, day, hour,
/// minute and second.
pub const VSCODE_API_DATE: [u32; 6] = [2026, 11, 12, 16, 39, 10];

/// A built-in pattern that does not compile: a rustode bug, reported as a
/// problem so that validation never panics.
fn pattern_bug(err: &regex::Error) -> ManifestProblem {
    ManifestProblem::error(format!(
        "rustode cannot check this manifest because one of its own patterns does not compile \
         ({err}); this is a rustode bug, please report it"
    ))
}

/// Why `package.json` text is not a manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// The text is JSON, but not an object.
    NotAnObject,
    /// The text is not JSON.
    NotJson(serde_json::Error),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => f.write_str("the manifest is not a JSON object"),
            Self::NotJson(err) => write!(f, "the manifest is not valid JSON: {err}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Why `engines.vscode` does not accept the API.
#[derive(Debug, PartialEq, Eq)]
enum EngineError {
    /// Not a version requirement.
    Unparsable(String),
    /// Names too little of the version (`before_one`: below 1.0.0).
    NotSpecific { engine: String, before_one: bool },
    /// Asks for another API than rustode's.
    Unsupported { engine: String, version: String },
    /// A built-in pattern does not compile.
    Pattern(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparsable(engine) => write!(
                f,
                "cannot parse `engines.vscode` value {engine}; use, for example, ^1.22.0 or ^1.22.x"
            ),
            Self::NotSpecific {
                engine,
                before_one: true,
            } => write!(
                f,
                "`engines.vscode` ({engine}) is not specific enough: before 1.0.0, name at least the major and minor version, e.g. ^0.10.0 or 0.10.x"
            ),
            Self::NotSpecific { engine, .. } => write!(
                f,
                "`engines.vscode` ({engine}) is not specific enough: name at least the major version, e.g. ^1.10.0, 1.10.x or 1.x.x"
            ),
            Self::Unsupported { engine, version } => write!(
                f,
                "the extension needs VS Code {engine}; rustode's extension API is VS Code {version}"
            ),
            Self::Pattern(message) => f.write_str(message),
        }
    }
}

/// Something validation found in a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestProblem {
    /// [`Severity::Error`] makes the extension invalid; a warning does not.
    pub severity: Severity,
    /// What is wrong, as a sentence.
    pub message: String,
}

impl ManifestProblem {
    fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    /// Whether it makes the extension invalid.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// The fields of `package.json` that [`ExtensionManifest`] reads through
/// typed accessors, in `IExtensionManifest`'s order.
const MODELLED: &[&str] = &[
    "name",
    "displayName",
    "publisher",
    "version",
    "engines",
    "description",
    "main",
    "browser",
    "activationEvents",
    "extensionDependencies",
    "extensionPack",
    "extensionKind",
    "contributes",
];

/// An extension's `package.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtensionManifest {
    fields: Map<String, Value>,
}

impl ExtensionManifest {
    /// Reads `package.json` text.
    ///
    /// # Errors
    /// When it is not JSON, or not a JSON object.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(fields)) => Ok(Self { fields }),
            Ok(_) => Err(ManifestError::NotAnObject),
            Err(err) => Err(ManifestError::NotJson(err)),
        }
    }

    /// A manifest made of these fields.
    #[must_use]
    pub fn from_fields(fields: Map<String, Value>) -> Self {
        Self { fields }
    }

    /// Every field, as read and in its order.
    #[must_use]
    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    /// Any field, modelled or not.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    /// The fields the model has no accessor for, in their order.
    pub fn unknown_fields(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.fields
            .iter()
            .filter(|(key, _)| !MODELLED.contains(&key.as_str()))
            .map(|(key, value)| (key.as_str(), value))
    }

    fn string(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(Value::as_str)
    }

    fn strings(&self, key: &str) -> Option<Vec<&str>> {
        string_array(self.fields.get(key)?)
    }

    /// `name`.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.string("name")
    }

    /// `displayName`, as written (`%key%` placeholders unresolved).
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.string("displayName")
    }

    /// `publisher`.
    #[must_use]
    pub fn publisher(&self) -> Option<&str> {
        self.string("publisher")
    }

    /// `version`.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.string("version")
    }

    /// `engines.vscode`: the VS Code versions the extension supports.
    #[must_use]
    pub fn engine(&self) -> Option<&str> {
        self.fields.get("engines")?.get("vscode")?.as_str()
    }

    /// `description`, as written.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.string("description")
    }

    /// `main`: the entry point for the Node extension host.
    #[must_use]
    pub fn main(&self) -> Option<&str> {
        self.string("main")
    }

    /// `browser`: the entry point for the web extension host.
    #[must_use]
    pub fn browser(&self) -> Option<&str> {
        self.string("browser")
    }

    /// `activationEvents`.
    #[must_use]
    pub fn activation_events(&self) -> Option<Vec<&str>> {
        self.strings("activationEvents")
    }

    /// `extensionDependencies`: ids of extensions it needs.
    #[must_use]
    pub fn extension_dependencies(&self) -> Option<Vec<&str>> {
        self.strings("extensionDependencies")
    }

    /// `extensionPack`: ids of extensions it installs with it.
    #[must_use]
    pub fn extension_pack(&self) -> Option<Vec<&str>> {
        self.strings("extensionPack")
    }

    /// `extensionKind`: where it prefers to run (`ui`, `workspace`, `web`),
    /// one kind or a list of them.
    #[must_use]
    pub fn extension_kind(&self) -> Option<Vec<&str>> {
        match self.fields.get("extensionKind")? {
            Value::String(kind) => Some(vec![kind.as_str()]),
            other => string_array(other),
        }
    }

    /// `contributes`, kept as JSON: a later extension host reads it.
    #[must_use]
    pub fn contributes(&self) -> Option<&Value> {
        self.fields.get("contributes")
    }

    /// The problems VS Code's scanner reports for this manifest, for an
    /// extension in `location`, checked against [`VSCODE_API_VERSION`].
    #[must_use]
    pub fn validate(&self, location: &Path) -> Vec<ManifestProblem> {
        validate(&self.fields, location)
    }
}

/// `value` as a list of strings, if it is one.
fn string_array(value: &Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(Value::as_str).collect()
}

/// Whether JavaScript treats `value` as true.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn mandatory(property: &str, kind: &str) -> ManifestProblem {
    ManifestProblem::error(format!(
        "property `{property}` is mandatory and must be of type `{kind}`"
    ))
}

fn optional_strings(property: &str) -> ManifestProblem {
    ManifestProblem::error(format!(
        "property `{property}` can be omitted or must be of type `string[]`"
    ))
}

/// `publisher`, `name`, `version` and `engines.vscode`, in VS Code's order.
fn check_identity(fields: &Map<String, Value>) -> Result<(), ManifestProblem> {
    // A missing or empty publisher is allowed: VS Code reads it as
    // `undefined_publisher` (`extensionsScannerService.ts:702-705`).
    if fields
        .get("publisher")
        .is_some_and(|p| truthy(p) && !p.is_string())
    {
        return Err(ManifestProblem::error(
            "property `publisher` must be of type `string`",
        ));
    }
    if !fields.get("name").is_some_and(Value::is_string) {
        return Err(mandatory("name", "string"));
    }
    if !fields.get("version").is_some_and(Value::is_string) {
        return Err(mandatory("version", "string"));
    }
    let Some(engines) = fields.get("engines").filter(|e| truthy(e)) else {
        return Err(mandatory("engines", "object"));
    };
    if !engines.get("vscode").is_some_and(Value::is_string) {
        return Err(mandatory("engines.vscode", "string"));
    }
    Ok(())
}

/// Whether the extension has code to run: `main` or `browser`.
fn has_code(fields: &Map<String, Value>) -> bool {
    fields.contains_key("main") || fields.contains_key("browser")
}

/// The fields that must be lists of strings.
fn check_lists(fields: &Map<String, Value>) -> Result<(), ManifestProblem> {
    for property in [
        "extensionDependencies",
        "extensionAffinity",
        "activationEvents",
    ] {
        if fields
            .get(property)
            .is_some_and(|v| string_array(v).is_none())
        {
            return Err(optional_strings(property));
        }
    }
    if fields.contains_key("activationEvents") && !has_code(fields) {
        return Err(ManifestProblem::error(
            "property `activationEvents` should be omitted if the extension has no `main` or `browser`",
        ));
    }
    Ok(())
}

/// `extensionKind`, `main` and `browser`; warnings go to `problems`.
fn check_entry_points(
    fields: &Map<String, Value>,
    location: &Path,
    problems: &mut Vec<ManifestProblem>,
) -> Result<(), ManifestProblem> {
    if fields.contains_key("extensionKind") && !fields.contains_key("main") {
        problems.push(ManifestProblem::warning(
            "property `extensionKind` can be defined only if property `main` is also defined",
        ));
    }
    for property in ["main", "browser"] {
        match fields.get(property) {
            None => {}
            Some(Value::String(entry)) if stays_inside(location, entry) => {}
            Some(Value::String(entry)) => problems.push(ManifestProblem::warning(format!(
                "`{property}` ({entry}) should be inside the extension's folder ({}); the extension may not be portable",
                location.display()
            ))),
            Some(_) => {
                return Err(ManifestProblem::error(format!(
                    "property `{property}` can be omitted or must be of type `string`"
                )));
            }
        }
    }
    Ok(())
}

/// The problems of a manifest's fields (see the module comment).
fn validate(fields: &Map<String, Value>, location: &Path) -> Vec<ManifestProblem> {
    let mut problems = Vec::new();
    let checked = check_identity(fields)
        .and_then(|()| check_lists(fields))
        .and_then(|()| check_entry_points(fields, location, &mut problems));
    if let Err(error) = checked {
        problems.push(error);
        return problems;
    }
    let version = fields["version"].as_str().unwrap_or_default();
    match is_semver(version) {
        Ok(true) => {}
        Ok(false) => {
            problems.push(ManifestProblem::error(
                "the extension's version is not semver compatible",
            ));
            return problems;
        }
        Err(err) => {
            problems.push(pattern_bug(&err));
            return problems;
        }
    }
    let engine = fields["engines"]["vscode"].as_str().unwrap_or_default();
    if has_code(fields)
        && let Err(err) = check_engine(engine, VSCODE_API_VERSION, VSCODE_API_DATE)
    {
        problems.push(ManifestProblem::error(err.to_string()));
    }
    problems
}

/// Whether `entry`, joined to the extension's folder as a URI path and
/// normalized, stays in it (`isEqualOrParent(joinPath(location, entry),
/// location)`).
fn stays_inside(location: &Path, entry: &str) -> bool {
    let folder: Vec<String> = location
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let mut path = folder.clone();
    for part in entry.split('/') {
        match part {
            "" | "." => {}
            ".." => drop(path.pop()),
            _ => path.push(part.to_owned()),
        }
    }
    path.starts_with(&folder)
}

/// A version as the scanner reads it: strict `SemVer`, an optional leading
/// `v`, at most 256 chars, numbers below 2^53.
fn is_semver(version: &str) -> Result<bool, regex::Error> {
    static SEMVER: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
        let number = "0|[1-9][0-9]*";
        let pre = "(?:0|[1-9][0-9]*|[0-9]*[a-zA-Z-][a-zA-Z0-9-]*)";
        let build = "[0-9A-Za-z-]+";
        Regex::new(&format!(
            r"^v?({number})\.({number})\.({number})(?:-{pre}(?:\.{pre})*)?(?:\+{build}(?:\.{build})*)?$"
        ))
    });
    const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
    let semver = SEMVER.as_ref().map_err(Clone::clone)?;
    Ok(version.len() <= 256
        && semver.captures(version).is_some_and(|parts| {
            (1..=3).all(|i| parts[i].parse::<u64>().is_ok_and(|n| n <= MAX_SAFE_INTEGER))
        }))
}

/// An `engines.vscode` value or a product version, read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Requirement {
    caret: bool,
    minimum: bool,
    /// Major, minor and patch: the number, and whether it must match.
    parts: [(u64, bool); 3],
    /// The `-YYYYMMDD[HH[MM]]` suffix, if any: year, month, day, hour, minute.
    not_before: Option<[u32; 5]>,
}

/// Trims as JavaScript does: white space and the byte order mark.
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// `Ok(None)` when `text` is not a requirement; an error only if a pattern
/// does not compile, which no input can cause.
fn parse_requirement(text: &str) -> Result<Option<Requirement>, regex::Error> {
    static VERSION: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
        Regex::new(r"^(\^|>=)?([0-9]+|x)\.([0-9]+|x)\.([0-9]+|x)(-[^\n\r\u{2028}\u{2029}]*)?$")
    });
    static NOT_BEFORE: LazyLock<Result<Regex, regex::Error>> =
        LazyLock::new(|| Regex::new(r"^-([0-9]{4})([0-9]{2})([0-9]{2})([0-9]{2})?([0-9]{2})?$"));
    let text = js_trim(text);
    if text == "*" {
        return Ok(Some(Requirement {
            caret: false,
            minimum: false,
            parts: [(0, false); 3],
            not_before: None,
        }));
    }
    let version = VERSION.as_ref().map_err(Clone::clone)?;
    let not_before_pattern = NOT_BEFORE.as_ref().map_err(Clone::clone)?;
    let Some(found) = version.captures(text) else {
        return Ok(None);
    };
    let part = |i: usize| match &found[i] {
        "x" => (0, false),
        digits => (digits.parse::<u64>().unwrap_or(u64::MAX), true),
    };
    let not_before = found
        .get(5)
        .and_then(|pre| not_before_pattern.captures(pre.as_str()))
        .map(|date| {
            let field = |i: usize| {
                date.get(i)
                    .and_then(|d| d.as_str().parse::<u32>().ok())
                    .unwrap_or(0)
            };
            [field(1), field(2), field(3), field(4), field(5)]
        });
    Ok(Some(Requirement {
        caret: found.get(1).is_some_and(|m| m.as_str() == "^"),
        minimum: found.get(1).is_some_and(|m| m.as_str() == ">="),
        parts: [part(2), part(3), part(4)],
        not_before,
    }))
}

/// The requirement with `^` applied: the parts after the first non-zero
/// one need not match.
fn normalized(requirement: Requirement) -> Requirement {
    let mut out = requirement;
    if requirement.caret {
        if requirement.parts[0].0 == 0 {
            out.parts[2].1 = false;
        } else {
            out.parts[1].1 = false;
            out.parts[2].1 = false;
        }
    }
    out
}

/// Whether a build dated `date` (UTC) is too old for `not_before`.
fn predates(date: [u32; 6], not_before: Option<[u32; 5]>) -> bool {
    not_before.is_some_and(|nb| date[..5] < nb[..])
}

/// Whether `product` satisfies `wanted` (`isValidVersion`,
/// `extensionValidator.ts:124-238`).
fn satisfies(product: [u64; 3], date: [u32; 6], wanted: Requirement) -> bool {
    let [
        (mut major, mut major_eq),
        (mut minor, mut minor_eq),
        (mut patch, mut patch_eq),
    ] = wanted.parts;
    let [have_major, have_minor, have_patch] = product;
    if wanted.minimum {
        // A newer or older major, then minor, decides; then the date and
        // the patch.
        if (have_major, have_minor) != (major, minor) {
            return (have_major, have_minor) > (major, minor);
        }
        return !predates(date, wanted.not_before) && have_patch >= patch;
    }
    // Anything below 1.0.0 is compatible with 1.x, except exact matches.
    if have_major == 1 && major == 0 && !(major_eq && minor_eq && patch_eq) {
        (major, minor, patch) = (1, 0, 0);
        (major_eq, minor_eq, patch_eq) = (true, false, false);
    }
    for (have, want, must_equal) in [
        (have_major, major, major_eq),
        (have_minor, minor, minor_eq),
        (have_patch, patch, patch_eq),
    ] {
        if have < want {
            return false;
        }
        if have > want {
            return !must_equal;
        }
    }
    !predates(date, wanted.not_before)
}

/// Checks `engines.vscode` against the API `version` of `date`
/// (`isVersionValid`, `extensionValidator.ts:348-379`).
fn check_engine(engine: &str, version: &str, date: [u32; 6]) -> Result<(), EngineError> {
    let wanted =
        parse_requirement(engine).map_err(|err| EngineError::Pattern(pattern_bug(&err).message))?;
    let Some(wanted) = wanted.map(normalized) else {
        return Err(EngineError::Unparsable(engine.to_owned()));
    };
    let [(major, major_eq), (_, minor_eq), _] = wanted.parts;
    if major == 0 && !(major_eq && minor_eq) {
        return Err(EngineError::NotSpecific {
            engine: engine.to_owned(),
            before_one: true,
        });
    }
    if major != 0 && !major_eq {
        return Err(EngineError::NotSpecific {
            engine: engine.to_owned(),
            before_one: false,
        });
    }
    let product = parse_requirement(version)
        .map_err(|err| EngineError::Pattern(pattern_bug(&err).message))?
        .map_or([0; 3], |p| p.parts.map(|(n, _)| n));
    if satisfies(product, date, wanted) {
        Ok(())
    } else {
        Err(EngineError::Unsupported {
            engine: engine.to_owned(),
            version: version.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: [u32; 6] = VSCODE_API_DATE;

    fn ok(engine: &str, version: &str) -> bool {
        check_engine(engine, version, DATE).is_ok()
    }

    #[test]
    fn engines_follow_vs_codes_ranges() {
        // Worked examples of VS Code's rules, checked by hand: (engine,
        // product version, compatible).
        let cases = [
            ("^1.80.0", "1.147.0", true),
            ("^1.147.0", "1.147.0", true),
            ("^1.148.0", "1.147.0", false),
            ("^2.0.0", "1.147.0", false),
            ("1.x.x", "1.147.0", true),
            ("1.147.x", "1.147.3", true),
            ("1.146.x", "1.147.0", false),
            ("1.147.0", "1.147.0", true),
            // An exact version must match.
            ("1.146.0", "1.147.0", false),
            (">=1.100.0", "1.147.0", true),
            (">=1.147.0", "1.147.0", true),
            (">=1.147.1", "1.147.0", false),
            (">=0.9.0", "1.147.0", true),
            // 0.x is compatible with 1.x, except exact versions.
            ("^0.10.0", "1.147.0", true),
            ("0.10.x", "1.147.0", true),
            ("0.10.5", "1.147.0", false),
            (" ^1.80.0 ", "1.147.0", true),
        ];
        for (engine, version, compatible) in cases {
            assert_eq!(ok(engine, version), compatible, "{engine} on {version}");
        }
    }

    #[test]
    fn engines_must_be_specific_enough() {
        for engine in ["*", "x.x.x", "^0.x.0", "0.x.x"] {
            let err = check_engine(engine, "1.147.0", DATE).expect_err(engine);
            assert!(
                err.to_string().contains("not specific enough"),
                "{engine}: {err}"
            );
        }
        for engine in ["", "1.2", "latest", "~1.2.3", "^1.2.3.4"] {
            let err = check_engine(engine, "1.147.0", DATE).expect_err(engine);
            assert!(err.to_string().contains("cannot parse"), "{engine}: {err}");
        }
    }

    #[test]
    fn a_dated_engine_needs_a_build_of_that_date() {
        assert!(ok("^1.147.0-20261112", "1.147.0"));
        assert!(ok("^1.147.0-2026111216", "1.147.0"));
        assert!(
            !ok("^1.147.0-202611121700", "1.147.0"),
            "built at 16:39 UTC"
        );
        assert!(!ok("^1.147.0-20261113", "1.147.0"));
        assert!(ok("^1.146.0-20991231", "1.147.0"), "a newer minor wins");
        assert!(!ok(">=1.147.0-20261201", "1.147.0"));
        assert!(ok("^1.147.0-insider", "1.147.0"));
    }

    #[test]
    fn versions_are_strict_semver() {
        for good in [
            "1.0.0",
            "v1.2.3",
            "0.0.1-alpha.1",
            "1.2.3+build.5",
            "1.2.3-rc.1+b",
        ] {
            assert_eq!(is_semver(good), Ok(true), "{good}");
        }
        for bad in [
            "1.0",
            "01.0.0",
            "1.0.0 ",
            " 1.0.0",
            "1.0.0-",
            "=1.0.0",
            "1.0.0-01",
            "9007199254740992.0.0",
            "١.٠.٠",
        ] {
            assert_eq!(is_semver(bad), Ok(false), "{bad}");
        }
        assert_eq!(is_semver(&format!("1.0.0-{}", "a".repeat(260))), Ok(false));
    }

    #[test]
    fn entry_points_stay_in_the_folder() {
        let folder = Path::new("/data/ext/acme.hello-1.0.0");
        for inside in [
            "./out/main.js",
            "out/../main.js",
            "/abs/main.js",
            "",
            "../acme.hello-1.0.0/main.js",
        ] {
            assert!(stays_inside(folder, inside), "{inside}");
        }
        for outside in [
            "../main.js",
            "out/../../main.js",
            "./..",
            "../acme.other/x.js",
        ] {
            assert!(!stays_inside(folder, outside), "{outside}");
        }
    }
}
