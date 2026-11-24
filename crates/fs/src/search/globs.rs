//! "files to include" and "files to exclude" as VS Code reads them
//! (`src/vs/workbench/services/search/common/queryBuilder.ts:354-404`,
//! studied and reimplemented): the text is split at commas outside braces
//! and brackets (`src/vs/base/common/glob.ts:66-110`), backslashes become
//! slashes, and each part is a glob over paths relative to the folder:
//!
//! - a part that starts with `./`, `../` or `/` is a path from the folder
//!   (absolute paths must lie inside it), matched with and without `/**`
//!   (`queryBuilder.ts:459-542`; `./` alone matches everything);
//! - any other part may match at any depth: `src` becomes `**/src/**` and
//!   `**/src`, and a leading `.` means an extension, `.rs` being `*.rs`
//!   (`queryBuilder.ts:378-389`, `:678-685`).
//!
//! In a glob, `*` and `?` stay within one path segment, `**` spans any
//! number of segments, `{a,b}` is either alternative and `[...]` a class of
//! chars. Globs are case-sensitive, as VS Code's on Linux
//! (`searchView.ts:1675`). Exclude globs from the settings are already
//! relative globs and are used as they are.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::fmt;
use std::path::{Path, PathBuf};

/// An include or exclude glob that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobError {
    pattern: String,
    message: String,
}

impl GlobError {
    /// The glob as typed.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

impl fmt::Display for GlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pattern, self.message)
    }
}

impl std::error::Error for GlobError {}

/// The deepest `{}` nesting a glob may have. The glob parser recurses into
/// each brace, so a typed glob of thousands of them would overflow the
/// stack, which aborts the process; a person writes two or three.
const MAX_BRACE_DEPTH: usize = 32;

/// What the walk lets through.
#[derive(Debug)]
pub(super) struct Filter {
    /// Typed includes; files must match one when there are any.
    include: Option<GlobSet>,
    /// Typed excludes.
    exclude: Option<GlobSet>,
    /// Excludes from the settings.
    settings: Option<GlobSet>,
}

impl Filter {
    pub(super) fn new(
        root: Option<&Path>,
        include: &str,
        exclude: &str,
        settings: &[String],
    ) -> Result<Self, GlobError> {
        Ok(Self {
            include: typed(root, include)?,
            exclude: typed(root, exclude)?,
            settings: set(settings.iter().map(|g| (g.clone(), g.clone())))?,
        })
    }

    /// Whether a folder or file under `root` is excluded, by the typed
    /// excludes or the settings.
    pub(super) fn excludes(&self, root: &Path, path: &Path) -> bool {
        let relative = relative(root, path);
        [&self.exclude, &self.settings]
            .into_iter()
            .flatten()
            .any(|set| set.is_match(&relative))
    }

    /// Whether a file under `root` passes the typed includes.
    pub(super) fn includes(&self, root: &Path, path: &Path) -> bool {
        self.include
            .as_ref()
            .is_none_or(|set| set.is_match(relative(root, path)))
    }

    /// Whether an open text passes the typed includes and excludes. Paths
    /// inside the folder are matched from it, others whole.
    pub(super) fn includes_open(&self, root: Option<&Path>, path: &Path) -> bool {
        let shown = match root.and_then(|root| path.strip_prefix(root).ok()) {
            Some(inside) => slashed(inside),
            None => slashed(path),
        };
        self.include.as_ref().is_none_or(|set| set.is_match(&shown))
            && !self
                .exclude
                .as_ref()
                .is_some_and(|set| set.is_match(&shown))
    }
}

/// `path` relative to `root`, with slashes.
fn relative(root: &Path, path: &Path) -> String {
    slashed(path.strip_prefix(root).unwrap_or(path))
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The globs of a typed field; `None` when it is empty.
fn typed(root: Option<&Path>, text: &str) -> Result<Option<GlobSet>, GlobError> {
    let mut globs = Vec::new();
    for part in split_glob_aware(text, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        for glob in expand(root, &part.replace('\\', "/"))? {
            globs.push((part.to_owned(), glob));
        }
    }
    set(globs.into_iter())
}

/// A set of `(as typed, glob)` pairs; `None` when there are none.
fn set(globs: impl Iterator<Item = (String, String)>) -> Result<Option<GlobSet>, GlobError> {
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for (typed, glob) in globs {
        if brace_depth(&glob) > MAX_BRACE_DEPTH {
            return Err(GlobError {
                pattern: typed,
                message: format!("braces nested more than {MAX_BRACE_DEPTH} deep"),
            });
        }
        let compiled = GlobBuilder::new(&glob)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|err| GlobError {
                pattern: typed,
                message: err.kind().to_string(),
            })?;
        builder.add(compiled);
        any = true;
    }
    if !any {
        return Ok(None);
    }
    builder.build().map(Some).map_err(|err| GlobError {
        pattern: err.glob().unwrap_or_default().to_owned(),
        message: err.kind().to_string(),
    })
}

/// How deep the `{}` of a glob nest, counting an unclosed brace as open.
fn brace_depth(glob: &str) -> usize {
    let (mut depth, mut deepest) = (0_usize, 0_usize);
    for c in glob.chars() {
        match c {
            '{' => {
                depth = depth.saturating_add(1);
                deepest = deepest.max(depth);
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// VS Code's `splitGlobAware`: splits at `split` outside `{}` and `[]`.
fn split_glob_aware(text: &str, split: char) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut braces, mut brackets) = (false, false);
    let mut current = String::new();
    for c in text.chars() {
        match c {
            c if c == split && !braces && !brackets => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            '{' => braces = true,
            '}' => braces = false,
            '[' => brackets = true,
            ']' => brackets = false,
            _ => {}
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// The globs one typed part stands for.
fn expand(root: Option<&Path>, part: &str) -> Result<Vec<String>, GlobError> {
    let from_folder = part.starts_with('/')
        || part == "."
        || part == ".."
        || part.starts_with("./")
        || part.starts_with("../");
    if !from_folder {
        let trimmed = part.trim_end_matches('/');
        let pattern = if trimmed.starts_with('.') {
            format!("*{trimmed}")
        } else {
            trimmed.to_owned()
        };
        return Ok([format!("**/{pattern}/**"), format!("**/{pattern}")]
            .into_iter()
            .map(|glob| glob.replace("**/**", "**"))
            .collect());
    }
    let relative = folder_relative(root, part)?;
    let relative = relative.trim_end_matches('/');
    if relative.is_empty() {
        return Ok(vec!["**".to_owned()]);
    }
    let mut globs = vec![relative.to_owned()];
    if !relative.ends_with("**") {
        globs.push(format!("{relative}/**"));
    }
    Ok(globs)
}

/// A part that names a path from the folder, made relative to it.
fn folder_relative(root: Option<&Path>, part: &str) -> Result<String, GlobError> {
    let outside = || GlobError {
        pattern: part.to_owned(),
        message: "the path is outside the opened folder".to_owned(),
    };
    if let Some(rest) = part.strip_prefix("./") {
        return Ok(rest.to_owned());
    }
    if part == "." {
        return Ok(String::new());
    }
    let root = root.ok_or_else(outside)?;
    let absolute = if part.starts_with('/') {
        PathBuf::from(part)
    } else {
        normalize(&root.join(part))
    };
    absolute
        .strip_prefix(root)
        .map(slashed)
        .map_err(|_| outside())
}

/// `path` with `.` and `..` segments resolved, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn globs(text: &str) -> Vec<String> {
        split_glob_aware(text, ',')
            .iter()
            .flat_map(|part| expand(Some(Path::new("/w")), part.trim()).expect("glob"))
            .collect()
    }

    #[test]
    fn parts_expand_as_in_vs_code() {
        assert_eq!(globs("src"), ["**/src/**", "**/src"]);
        assert_eq!(globs(".rs"), ["**/*.rs/**", "**/*.rs"]);
        assert_eq!(globs("**/*.ts"), ["**/*.ts/**", "**/*.ts"]);
        assert_eq!(globs("a/b/"), ["**/a/b/**", "**/a/b"]);
        assert_eq!(
            globs("{a,b}, c"),
            ["**/{a,b}/**", "**/{a,b}", "**/c/**", "**/c"]
        );
        assert_eq!(globs("./src"), ["src", "src/**"]);
        assert_eq!(globs("./src/**"), ["src/**"]);
        assert_eq!(globs("./"), ["**"]);
        assert_eq!(globs("/w/lib"), ["lib", "lib/**"]);
        assert_eq!(globs("sub/../x"), ["**/sub/../x/**", "**/sub/../x"]);
        assert!(expand(Some(Path::new("/w")), "../elsewhere").is_err());
        assert!(expand(Some(Path::new("/w")), "/elsewhere").is_err());
    }

    #[test]
    fn deeply_nested_braces_are_an_error_not_a_stack_overflow() {
        let nested = "{".repeat(10_000) + &"}".repeat(10_000);
        let err = Filter::new(None, &nested, "", &[]).expect_err("too deep");
        assert!(err.to_string().contains("nested"), "{err}");
        Filter::new(None, "{a,{b,{c,d}}}", "", &[]).expect("a person's nesting is fine");
    }

    #[test]
    fn invalid_globs_are_reported_with_the_typed_part() {
        let err = Filter::new(None, "ok, a[b", "", &[]).expect_err("invalid");
        assert_eq!(err.pattern(), "a[b");
        assert!(err.to_string().starts_with("a[b: "), "{err}");
    }
}
