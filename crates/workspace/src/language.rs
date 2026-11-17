//! A document's language, by file name and extension.
//!
//! The table holds about twenty common languages with the names, extensions
//! and file names that the App's built-in extensions declare (generated from
//! `01-app/VSCode-linux-x64/resources/app/extensions/*/package.json`,
//! `contributes.languages`). Matching follows VS Code: an exact file name
//! wins, then the longest matching extension, both compared
//! case-insensitively (`src/vs/editor/common/services/languagesAssociations.ts:196-240`;
//! glob patterns are not supported yet); anything else
//! is Plain Text (`plaintext`, defined in VS Code's core). Full language
//! detection is ticket 28.

/// A language a document can be in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Language {
    /// The id used by settings overrides (`"[rust]"`).
    pub id: &'static str,
    /// The name shown in the status bar.
    pub name: &'static str,
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
}

pub(crate) const PLAIN_TEXT: Language = Language {
    id: "plaintext",
    name: "Plain Text",
    extensions: &[],
    file_names: &[],
};

/// The language of a file named `file_name`.
pub(crate) fn detect(file_name: &str) -> &'static Language {
    let lower = file_name.to_lowercase();
    let named = |l: &&Language| l.file_names.iter().any(|n| n.to_lowercase() == lower);
    if let Some(language) = LANGUAGES.iter().find(named) {
        return language;
    }
    LANGUAGES
        .iter()
        .flat_map(|l| l.extensions.iter().map(move |e| (l, *e)))
        .filter(|(_, extension)| lower.ends_with(extension))
        .max_by_key(|(_, extension)| extension.len())
        .map_or(&PLAIN_TEXT, |(language, _)| language)
}

const LANGUAGES: &[Language] = &[
    Language {
        id: "rust",
        name: "Rust",
        extensions: &[".rs"],
        file_names: &[],
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
    },
    Language {
        id: "javascript",
        name: "JavaScript",
        extensions: &[".cjs", ".es6", ".js", ".mjs", ".pac"],
        file_names: &["jakefile"],
    },
    Language {
        id: "typescript",
        name: "TypeScript",
        extensions: &[".cts", ".mts", ".ts"],
        file_names: &[],
    },
    Language {
        id: "python",
        name: "Python",
        extensions: &[
            ".cpy", ".gyp", ".gypi", ".ipy", ".py", ".pyi", ".pyt", ".pyw", ".rpy",
        ],
        file_names: &["SConscript", "SConstruct"],
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
    },
    Language {
        id: "html",
        name: "HTML",
        extensions: &[
            ".asp", ".aspx", ".ejs", ".htm", ".html", ".jshtm", ".jsp", ".mdoc", ".rhtml",
            ".shtml", ".volt", ".xht", ".xhtml",
        ],
        file_names: &[],
    },
    Language {
        id: "css",
        name: "CSS",
        extensions: &[".css"],
        file_names: &[],
    },
    Language {
        id: "c",
        name: "C",
        extensions: &[".c", ".i"],
        file_names: &[],
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
    },
    Language {
        id: "go",
        name: "Go",
        extensions: &[".go"],
        file_names: &[],
    },
    Language {
        id: "java",
        name: "Java",
        extensions: &[".jav", ".java"],
        file_names: &[],
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
    },
    Language {
        id: "ini",
        name: "Ini",
        extensions: &[".ini"],
        file_names: &[],
    },
    Language {
        id: "dockerfile",
        name: "Docker",
        extensions: &[".containerfile", ".dockerfile"],
        file_names: &["Containerfile", "Dockerfile"],
    },
    Language {
        id: "makefile",
        name: "Makefile",
        extensions: &[".mak", ".mk"],
        file_names: &["GNUmakefile", "Makefile", "OCamlMakefile", "makefile"],
    },
    Language {
        id: "log",
        name: "Log",
        extensions: &[".log"],
        file_names: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_extension_wins() {
        assert_eq!(detect("app.js.map").id, "json");
        assert_eq!(detect("app.js").id, "javascript");
        assert_eq!(detect("config.h.in").id, "cpp");
        assert_eq!(detect("README").id, "plaintext");
        assert_eq!(detect("makefile").id, "makefile");
        assert_eq!(detect("DOCKERFILE").id, "dockerfile");
    }
}
