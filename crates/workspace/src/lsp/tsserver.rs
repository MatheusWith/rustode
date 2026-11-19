//! Which `tsserver.js` the built-in typescript-language-server runs on.
//!
//! The owner's decision (ticket 32 follow-up): it runs on a TypeScript 6
//! `tsserver.js`, never on TypeScript 7, which is a native compiler and ships
//! no `tsserver.js`. The first of these that holds a 6.x package wins:
//!
//! 1. the workspace's own `node_modules/typescript/lib`;
//! 2. the folder in `rustode.typescript.tsdk`, a setting that mirrors VS
//!    Code's `typescript.tsdk`: the folder that holds `tsserver.js`, absolute,
//!    `~/`-relative or relative to the workspace folder
//!    (`extensions/typescript-language-features/src/configuration/configuration.electron.ts:17-25`,
//!    `src/tsServer/versionProvider.electron.ts:100-120`);
//! 3. a global install next to the language server's own: the
//!    `node_modules` folder (npm's global root, what `npm root -g` prints)
//!    the program lives in, and the one inside its package. No process is
//!    run and nothing else on the disk is searched.
//!
//! The version is read the way VS Code reads it: the `version` of the
//! `package.json` two folders above `tsserver.js`
//! (`versionProvider.electron.ts:163-198`). A `tsserver.js` whose version
//! cannot be read is unknown, and unknown is not accepted: it may be 7.
//! The chosen file goes to the server as `initializationOptions.tsserver.path`.
//! When none qualifies the server is not started, and the message says what
//! was found and what to set.
//!
//! VS Code's behavior is reimplemented, not translated; the order of the
//! candidates is rustode's own.

use std::path::{Path, PathBuf};

/// The setting: the folder with a TypeScript 6 `tsserver.js`.
pub(crate) const TSDK_KEY: &str = "rustode.typescript.tsdk";

/// Largest `package.json` read (it is a hostile input like any file).
const MAX_PACKAGE_JSON: u64 = 1 << 20;

/// What the built-in typescript-language-server needs to find a TypeScript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TypeScriptRule {
    /// `rustode.typescript.tsdk`, as the user wrote it.
    pub tsdk: Option<String>,
    /// Global `typescript` package folders to try last.
    pub discovered: Vec<PathBuf>,
}

/// The major version of a TypeScript package, from `version` in its
/// `package.json`; `None` when it cannot be read.
fn major_of(package: &Path) -> Option<u64> {
    let file = package.join("package.json");
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > MAX_PACKAGE_JSON {
        return None;
    }
    let text = std::fs::read_to_string(&file).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json["version"].as_str()?.split('.').next()?.parse().ok()
}

/// What one candidate `tsserver.js` turned out to be.
enum Verdict {
    /// A 6.x server: use it.
    Usable(PathBuf),
    /// Not usable, and why, for the message.
    Refused(String),
}

/// Checks the `tsserver.js` in `dir` (a `lib` folder).
fn check(dir: &Path) -> Verdict {
    let server = dir.join("tsserver.js");
    let package = dir.parent().unwrap_or(dir);
    if !server.is_file() {
        // TypeScript 7 is a native compiler: its package has no tsserver.js.
        return Verdict::Refused(match major_of(package) {
            Some(major) if major != 6 => format!(
                "{} is TypeScript {major}, not 6 (it has no tsserver.js)",
                package.display()
            ),
            _ => format!("{} has no tsserver.js", dir.display()),
        });
    }
    match major_of(package) {
        Some(6) => Verdict::Usable(server),
        Some(major) => {
            Verdict::Refused(format!("{} is TypeScript {major}, not 6", server.display()))
        }
        None => Verdict::Refused(format!(
            "the version of {} could not be read",
            server.display()
        )),
    }
}

/// `tsdk` as a folder: `~/` is the home folder, a relative path is relative
/// to `root`.
fn tsdk_folder(tsdk: &str, root: &Path) -> PathBuf {
    if let Some(rest) = tsdk.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return Path::new(&home).join(rest);
    }
    root.join(tsdk) // an absolute `tsdk` replaces `root`
}

/// The global `typescript` package folders next to the program at `program`
/// (a path, followed through links): beside its package in `node_modules`
/// and inside it.
pub(crate) fn discover(program: &Path) -> Vec<PathBuf> {
    let Ok(real) = program.canonicalize() else {
        return Vec::new();
    };
    let Some(package) = real.ancestors().find(|dir| {
        dir.file_name()
            .is_some_and(|n| n == "typescript-language-server")
    }) else {
        return Vec::new();
    };
    let mut found = vec![package.join("node_modules/typescript")];
    if let Some(modules) = package.parent() {
        found.push(modules.join("typescript"));
    }
    found
}

/// The `tsserver.js` to run for a server rooted at `root`, or the sentence
/// that says why there is none.
pub(crate) fn resolve(root: &Path, rule: &TypeScriptRule) -> Result<PathBuf, String> {
    let mut candidates = vec![root.join("node_modules/typescript/lib")];
    if let Some(tsdk) = rule.tsdk.as_deref().filter(|t| !t.is_empty()) {
        candidates.push(tsdk_folder(tsdk, root));
    }
    candidates.extend(rule.discovered.iter().map(|p| p.join("lib")));
    let mut refused = Vec::new();
    for dir in &candidates {
        match check(dir) {
            Verdict::Usable(server) => return Ok(server),
            Verdict::Refused(why) => refused.push(why),
        }
    }
    // The local folder missing is the common case: say it last, and only
    // what was actually present.
    let present: Vec<String> = refused
        .into_iter()
        .filter(|why| !why.ends_with("has no tsserver.js"))
        .collect();
    let found = if present.is_empty() {
        "no tsserver.js was found".to_owned()
    } else {
        present.join("; ")
    };
    Err(format!(
        "TypeScript language server needs TypeScript 6 (tsserver.js): {found}; \
         set {TSDK_KEY} to the lib folder of a TypeScript 6 install, \
         or install typescript@6 in the workspace"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(dir: &Path, version: Option<&str>, server: bool) {
        std::fs::create_dir_all(dir.join("lib")).expect("lib");
        if let Some(version) = version {
            std::fs::write(
                dir.join("package.json"),
                format!("{{\"name\":\"typescript\",\"version\":\"{version}\"}}"),
            )
            .expect("package.json");
        }
        if server {
            std::fs::write(dir.join("lib/tsserver.js"), "").expect("tsserver");
        }
    }

    #[test]
    fn versions_are_read_from_the_package_and_unknown_is_refused() {
        let base =
            std::env::temp_dir().join(format!("rustode-tsserver-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        package(&base.join("six"), Some("6.0.3"), true);
        package(&base.join("seven"), Some("7.0.2"), false);
        package(&base.join("broken"), None, true);
        std::fs::write(base.join("broken/package.json"), "{ not json").expect("write");
        package(&base.join("huge"), None, true);
        std::fs::write(base.join("huge/package.json"), vec![b' '; 2 << 20]).expect("write");
        assert_eq!(major_of(&base.join("six")), Some(6));
        assert_eq!(major_of(&base.join("seven")), Some(7));
        assert_eq!(major_of(&base.join("broken")), None);
        assert_eq!(major_of(&base.join("huge")), None);
        assert!(matches!(check(&base.join("six/lib")), Verdict::Usable(_)));
        assert!(matches!(
            check(&base.join("seven/lib")),
            Verdict::Refused(_)
        ));
        assert!(matches!(
            check(&base.join("broken/lib")),
            Verdict::Refused(_)
        ));
        assert!(matches!(check(&base.join("nowhere")), Verdict::Refused(_)));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn tsdk_folders_are_absolute_or_relative_to_the_root() {
        let root = Path::new("/work/app");
        assert_eq!(tsdk_folder("/opt/ts/lib", root), Path::new("/opt/ts/lib"));
        assert_eq!(
            tsdk_folder("./node_modules/typescript/lib", root),
            Path::new("/work/app/./node_modules/typescript/lib")
        );
    }
}
