//! A document's language: detected from its file name and first line, or
//! picked by the user.
//!
//! The table holds about twenty common languages with the names, extensions,
//! file names, file name patterns and first line expressions that the App's
//! built-in extensions declare (`contributes.languages` in
//! `extensions/*/package.json` of the source tree). Detection follows VS
//! Code's order (`getAssociations`,
//! `src/vs/editor/common/services/languagesAssociations.ts:139-190`):
//!
//! 1. the user's `files.associations`, by the longest matching pattern; a
//!    pattern with a path separator matches the absolute path, any other the
//!    file name (the setting's description,
//!    `src/vs/workbench/contrib/files/browser/files.contribution.ts:190-196`;
//!    registered as in `src/vs/workbench/services/language/common/languageService.ts:293-317`);
//! 2. the languages' own associations, in this order of priority: an exact
//!    file name, then the longest matching file name pattern, then the
//!    longest matching extension, all case-insensitive
//!    (`languagesAssociations.ts:196-240`);
//! 3. the first line, matched against each language's `firstLine`
//!    expression, the last registered language winning
//!    (`languagesAssociations.ts:245-270`; the expression gets a leading `^`
//!    if it has none, `languagesRegistry.ts:218-227`);
//! 4. otherwise Plain Text (`plaintext`, defined in VS Code's core).
//!
//! Where several languages claim the same pattern or first line, VS Code's
//! last registered wins; the order of registration is the order of the
//! extension folders, which is assumed here to be the table's order
//! (hypothesis: not checked against the App's extension scan).
//! A `files.associations` entry naming a language rustode does not know is
//! ignored, as VS Code ignores an unregistered language id
//! (`languageService.ts:124-130`).

use globset::{GlobBuilder, GlobMatcher};
use regex::Regex;
use rustode_config::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// A language a document can be in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Language {
    /// The id used by settings overrides (`"[rust]"`).
    pub id: &'static str,
    /// The name shown in the status bar.
    pub name: &'static str,
    pub(crate) extensions: &'static [&'static str],
    file_names: &'static [&'static str],
    /// `filenamePatterns`: globs on the file name, or on the path if they
    /// contain a `/`.
    file_patterns: &'static [&'static str],
    /// `firstLine`: a regular expression the first line of a file matches.
    first_line: Option<&'static str>,
}

const BASE: Language = Language {
    id: "",
    name: "",
    extensions: &[],
    file_names: &[],
    file_patterns: &[],
    first_line: None,
};

/// Plain Text, registered by VS Code's core with the extension `.txt`
/// (`src/vs/editor/common/languages/modesRegistry.ts:54-60`).
pub(crate) const PLAIN_TEXT: Language = Language {
    id: "plaintext",
    name: "Plain Text",
    extensions: &[".txt"],
    ..BASE
};

/// Every language, Plain Text first.
pub(crate) fn all() -> impl Iterator<Item = &'static Language> {
    std::iter::once(&PLAIN_TEXT).chain(LANGUAGES.iter())
}

/// The language with this id.
pub(crate) fn by_id(id: &str) -> Option<&'static Language> {
    all().find(|language| language.id == id)
}

/// A glob as VS Code reads it for associations: case-insensitive, and a
/// `*` never crosses a `/`. Matches the whole path when the pattern has a
/// `/`, the file name otherwise.
struct Pattern {
    matcher: GlobMatcher,
    on_path: bool,
    len: usize,
}

impl Pattern {
    fn new(pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        let matcher = GlobBuilder::new(pattern)
            .case_insensitive(true)
            .literal_separator(true)
            .build()
            .ok()?
            .compile_matcher();
        Some(Self {
            matcher,
            on_path: pattern.contains('/'),
            len: pattern.len(),
        })
    }

    fn matches(&self, path: &str, file_name: &str) -> bool {
        self.matcher
            .is_match(if self.on_path { path } else { file_name })
    }
}

/// The user's `files.associations`: patterns and the languages they name,
/// in the order of the setting.
#[derive(Default)]
pub(crate) struct Associations {
    entries: Vec<(Pattern, &'static Language)>,
}

impl std::fmt::Debug for Associations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Associations")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Associations {
    /// Reads the setting's object (`"*.conf": "ini"`); values that are not
    /// the id of a known language, and patterns that are not globs, are
    /// skipped.
    pub(crate) fn from_setting(value: Option<&Value>) -> Self {
        let Some(Value::Object(map)) = value else {
            return Self::default();
        };
        Self::from_pairs(map)
    }

    fn from_pairs(map: &BTreeMap<String, Value>) -> Self {
        let entries = map
            .iter()
            .filter_map(|(pattern, id)| {
                let Value::String(id) = id else { return None };
                Some((Pattern::new(pattern)?, by_id(id)?))
            })
            .collect();
        Self { entries }
    }

    /// The language of the longest pattern matching; of two equal ones the
    /// later wins (`getAssociationByPath`'s loop runs backwards and keeps
    /// the first strictly longer one).
    fn find(&self, path: &str, file_name: &str) -> Option<&'static Language> {
        longest(
            self.entries
                .iter()
                .rev()
                .filter(|(pattern, _)| pattern.matches(path, file_name))
                .map(|(pattern, language)| (pattern.len, *language)),
        )
    }
}

fn longest<L>(candidates: impl Iterator<Item = (usize, L)>) -> Option<L> {
    let mut best: Option<(usize, L)> = None;
    for (len, language) in candidates {
        if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
            best = Some((len, language));
        }
    }
    best.map(|(_, language)| language)
}

/// The languages' own file name patterns, compiled once.
fn platform_patterns() -> &'static [(Pattern, &'static Language)] {
    static PATTERNS: OnceLock<Vec<(Pattern, &'static Language)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        LANGUAGES
            .iter()
            .flat_map(|l| l.file_patterns.iter().map(move |p| (p, l)))
            .filter_map(|(pattern, language)| Some((Pattern::new(pattern)?, language)))
            .collect()
    })
}

/// The languages' first line expressions, compiled once.
fn first_lines() -> &'static [(Regex, &'static Language)] {
    static EXPRESSIONS: OnceLock<Vec<(Regex, &'static Language)>> = OnceLock::new();
    EXPRESSIONS.get_or_init(|| {
        LANGUAGES
            .iter()
            .filter_map(|l| {
                let source = l.first_line?;
                let source = if source.starts_with('^') {
                    source.to_owned()
                } else {
                    format!("^{source}")
                };
                Some((Regex::new(&source).ok()?, l))
            })
            .collect()
    })
}

/// The language of the file at `path` whose first line is `first_line`.
pub(crate) fn detect(
    path: &Path,
    first_line: &str,
    associations: &Associations,
) -> &'static Language {
    let full = path.to_string_lossy().to_lowercase();
    let file_name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_lowercase());
    if let Some(language) = associations.find(&full, &file_name) {
        return language;
    }
    if let Some(language) = by_name(&file_name)
        .or_else(|| by_pattern(&full, &file_name))
        .or_else(|| by_extension(&file_name))
    {
        return language;
    }
    by_first_line(first_line).unwrap_or(&PLAIN_TEXT)
}

/// The language whose first line expression matches; the last one in the
/// table wins.
pub(crate) fn by_first_line(first_line: &str) -> Option<&'static Language> {
    let line = first_line.strip_prefix('\u{feff}').unwrap_or(first_line);
    if line.is_empty() {
        return None;
    }
    first_lines()
        .iter()
        .rev()
        .find(|(expression, _)| expression.is_match(line))
        .map(|(_, language)| *language)
}

fn by_name(file_name: &str) -> Option<&'static Language> {
    LANGUAGES
        .iter()
        .rev()
        .find(|l| l.file_names.iter().any(|n| n.to_lowercase() == file_name))
}

fn by_pattern(path: &str, file_name: &str) -> Option<&'static Language> {
    longest(
        platform_patterns()
            .iter()
            .rev()
            .filter(|(pattern, _)| pattern.matches(path, file_name))
            .map(|(pattern, language)| (pattern.len, *language)),
    )
}

fn by_extension(file_name: &str) -> Option<&'static Language> {
    longest(
        std::iter::once(&PLAIN_TEXT)
            .chain(LANGUAGES.iter())
            .rev()
            .flat_map(|l| l.extensions.iter().map(move |e| (l, e.to_lowercase())))
            .filter(|(_, extension)| file_name.ends_with(extension.as_str()))
            .map(|(language, extension)| (extension.len(), language)),
    )
}

const LANGUAGES: &[Language] = &[
    Language {
        id: "rust",
        name: "Rust",
        extensions: &[".rs"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "json",
        name: "JSON",
        extensions: &[
            ".bowerrc",
            ".code-profile",
            ".css.map",
            ".geojson",
            ".har",
            ".ipynb",
            ".js.map",
            ".jscsrc",
            ".jslintrc",
            ".json",
            ".jsonld",
            ".ts.map",
            ".tsbuildinfo",
            ".vuerc",
            ".webmanifest",
        ],
        file_names: &[".watchmanconfig", "composer.lock"],
        ..BASE
    },
    Language {
        id: "jsonc",
        name: "JSON with Comments",
        extensions: &[
            ".babelrc",
            ".code-workspace",
            ".eslintrc",
            ".eslintrc.json",
            ".hintrc",
            ".jsfmtrc",
            ".jshintrc",
            ".jsonc",
            ".swcrc",
            ".toolset.jsonc",
            "color-theme.json",
            "icon-theme.json",
            "language-configuration.json",
        ],
        file_names: &[
            ".babelrc.json",
            ".devcontainer.json",
            ".ember-cli",
            "argv.json",
            "babel.config.json",
            "bun.lock",
            "devcontainer.json",
            "extensions.json",
            "jsconfig.json",
            "keybindings.json",
            "launch.json",
            "mcp.json",
            "profiles.json",
            "settings.json",
            "tasks.json",
            "tsconfig.json",
            "typedoc.json",
        ],
        file_patterns: &[
            "**/.github/hooks/*.json",
            "tsconfig.*.json",
            "jsconfig.*.json",
            "tsconfig-*.json",
            "jsconfig-*.json",
        ],
        ..BASE
    },
    Language {
        id: "markdown",
        name: "Markdown",
        extensions: &[
            ".litcoffee",
            ".markdn",
            ".markdown",
            ".md",
            ".mdown",
            ".mdtext",
            ".mdtxt",
            ".mdwn",
            ".mkd",
            ".mkdn",
            ".ron",
            ".ronn",
            ".workbook",
        ],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "javascript",
        name: "JavaScript",
        extensions: &[".cjs", ".es6", ".js", ".mjs", ".pac"],
        file_names: &["jakefile"],
        first_line: Some(r"^#!.*\bnode"),
        ..BASE
    },
    Language {
        id: "typescript",
        name: "TypeScript",
        extensions: &[".cts", ".mts", ".ts"],
        file_names: &[],
        first_line: Some(r"^#!.*\b(deno|bun|ts-node)\b"),
        ..BASE
    },
    Language {
        id: "python",
        name: "Python",
        extensions: &[
            ".cpy", ".gyp", ".gypi", ".ipy", ".py", ".pyi", ".pyt", ".pyw", ".rpy",
        ],
        file_names: &["SConscript", "SConstruct"],
        first_line: Some(r"^#!\s*/?.*\bpython[0-9.-]*\b"),
        ..BASE
    },
    Language {
        id: "shellscript",
        name: "Shell Script",
        extensions: &[
            ".xsession",
            ".bash",
            ".bash_aliases",
            ".bash_login",
            ".bash_logout",
            ".bash_profile",
            ".bashrc",
            ".csh",
            ".cshrc",
            ".ebuild",
            ".eclass",
            ".fish",
            ".ksh",
            ".profile",
            ".sh",
            ".tcshrc",
            ".xprofile",
            ".xsession",
            ".xsessionrc",
            ".yash_profile",
            ".yashrc",
            ".zlogin",
            ".zlogout",
            ".zprofile",
            ".zsh",
            ".zsh-theme",
            ".zshenv",
            ".zshrc",
        ],
        file_names: &[
            ".envrc",
            ".hushlogin",
            "APKBUILD",
            "PKGBUILD",
            "bashrc_Apple_Terminal",
            "zlogin",
            "zlogout",
            "zprofile",
            "zshenv",
            "zshrc",
            "zshrc_Apple_Terminal",
        ],
        first_line: Some(
            r"^#!.*\b(bash|fish|zsh|sh|ksh|dtksh|pdksh|mksh|ash|dash|yash|sh|csh|jcsh|tcsh|itcsh).*|^#\s*-\*-[^*]*mode:\s*shell-script[^*]*-\*-",
        ),
        ..BASE
    },
    Language {
        id: "yaml",
        name: "YAML",
        extensions: &[
            ".cff",
            ".eyaml",
            ".eyml",
            ".winget",
            ".yaml",
            ".yaml-tmlanguage",
            ".yaml-tmpreferences",
            ".yaml-tmtheme",
            ".yml",
        ],
        file_names: &[],
        first_line: Some(r"^#cloud-config"),
        ..BASE
    },
    Language {
        id: "html",
        name: "HTML",
        extensions: &[
            ".asp", ".aspx", ".ejs", ".htm", ".html", ".jshtm", ".jsp", ".mdoc", ".rhtml",
            ".shtml", ".volt", ".xht", ".xhtml",
        ],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "css",
        name: "CSS",
        extensions: &[".css"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "c",
        name: "C",
        extensions: &[".c", ".i"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "cpp",
        name: "C++",
        extensions: &[
            ".c++", ".c++m", ".cc", ".ccm", ".cpp", ".cppm", ".cxx", ".cxxm", ".h", ".h++",
            ".h.in", ".hh", ".hpp", ".hpp.in", ".hxx", ".ii", ".inl", ".ino", ".ipp", ".ixx",
            ".mpp", ".mxx", ".tpp", ".txx",
        ],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "go",
        name: "Go",
        extensions: &[".go"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "java",
        name: "Java",
        extensions: &[".jav", ".java"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "xml",
        name: "XML",
        extensions: &[
            ".ascx",
            ".atom",
            ".axaml",
            ".axml",
            ".bpmn",
            ".cpt",
            ".csl",
            ".csproj",
            ".csproj.user",
            ".dita",
            ".ditamap",
            ".dtd",
            ".dtml",
            ".ent",
            ".fsproj",
            ".fxml",
            ".iml",
            ".isml",
            ".jmx",
            ".launch",
            ".menu",
            ".mod",
            ".mxml",
            ".nuspec",
            ".opml",
            ".owl",
            ".proj",
            ".props",
            ".pt",
            ".publishsettings",
            ".pubxml",
            ".pubxml.user",
            ".rbxlx",
            ".rbxmx",
            ".rdf",
            ".rng",
            ".rss",
            ".shproj",
            ".slnx",
            ".storyboard",
            ".svg",
            ".targets",
            ".tld",
            ".tmx",
            ".vbproj",
            ".vbproj.user",
            ".vcxproj",
            ".vcxproj.filters",
            ".wixproj",
            ".wsdl",
            ".wxi",
            ".wxl",
            ".wxs",
            ".xaml",
            ".xbl",
            ".xib",
            ".xlf",
            ".xliff",
            ".xml",
            ".xoml",
            ".xpdl",
            ".xsd",
            ".xul",
        ],
        file_names: &[],
        first_line: Some(r"(<\?xml.*)|(<svg)|(<!doctype\s+svg)"),
        ..BASE
    },
    Language {
        id: "ini",
        name: "Ini",
        extensions: &[".ini"],
        file_names: &[],
        ..BASE
    },
    Language {
        id: "dockerfile",
        name: "Docker",
        extensions: &[".containerfile", ".dockerfile"],
        file_names: &["Containerfile", "Dockerfile"],
        file_patterns: &["Dockerfile.*", "Containerfile.*"],
        ..BASE
    },
    Language {
        id: "makefile",
        name: "Makefile",
        extensions: &[".mak", ".mk"],
        file_names: &["GNUmakefile", "Makefile", "OCamlMakefile", "makefile"],
        first_line: Some(r"^#!\s*/usr/bin/make"),
        ..BASE
    },
    Language {
        id: "log",
        name: "Log",
        extensions: &[".log"],
        file_names: &[],
        ..BASE
    },
    // The two JSX languages are the App's (`extensions/javascript/package.json`,
    // `extensions/typescript-basics/package.json`).
    Language {
        id: "javascriptreact",
        name: "JavaScript JSX",
        extensions: &[".jsx"],
        ..BASE
    },
    Language {
        id: "typescriptreact",
        name: "TypeScript JSX",
        extensions: &[".tsx"],
        ..BASE
    },
    // TOML is rustode's own addition: the App has no built-in TOML language
    // (its extension `tamasfe.even-better-toml` is a marketplace one), so
    // only `.toml` is claimed.
    Language {
        id: "toml",
        name: "TOML",
        extensions: &[".toml"],
        ..BASE
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: &str, first_line: &str) -> &'static str {
        detect(Path::new(path), first_line, &Associations::default()).id
    }

    #[test]
    fn the_longest_extension_wins() {
        assert_eq!(id("app.js.map", ""), "json");
        assert_eq!(id("app.js", ""), "javascript");
        assert_eq!(id("config.h.in", ""), "cpp");
        assert_eq!(id("README", ""), "plaintext");
        assert_eq!(id("makefile", ""), "makefile");
        assert_eq!(id("DOCKERFILE", ""), "dockerfile");
        assert_eq!(id("/a/b/Cargo.toml", ""), "toml");
    }
}
