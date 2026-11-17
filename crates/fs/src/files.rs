//! Every file under a folder, as VS Code's quick open finds them.
//!
//! The walk honors `.gitignore` and `.ignore` files (`search.useIgnoreFiles`,
//! default true; parent folders' and global ignore files are not read, as
//! `search.useParentIgnoreFiles` and `search.useGlobalIgnoreFiles` default to
//! false), keeps hidden files, and skips the default entries of
//! `files.exclude` and `search.exclude`
//! (`src/vs/workbench/contrib/files/browser/files.contribution.ts:154-160`,
//! `src/vs/workbench/contrib/search/browser/search.common.contribution.ts:26-60`).

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// Names hidden by default: `files.exclude` plus `search.exclude`.
const EXCLUDED_NAMES: [&str; 8] = [
    ".git",
    ".svn",
    ".hg",
    ".jj",
    ".DS_Store",
    "Thumbs.db",
    "node_modules",
    "bower_components",
];

fn excluded(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    EXCLUDED_NAMES.contains(&name.as_ref()) || name.ends_with(".code-search")
}

/// Every file under `root`, relative to it, in walk order.
///
/// # Errors
/// When `root` cannot be read.
pub fn list_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let _span = tracing::debug_span!("file_op", op = "walk", path = %root.display()).entered();
    std::fs::read_dir(root)?;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_global(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .require_git(false)
        .filter_entry(|entry| !excluded(entry.file_name()))
        .build();
    let mut files = Vec::new();
    for entry in walker.flatten() {
        if entry.file_type().is_some_and(|t| t.is_file())
            && let Ok(relative) = entry.path().strip_prefix(root)
        {
            files.push(relative.to_owned());
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_names_and_ignored_files_are_skipped() {
        let root = std::env::temp_dir().join(format!("rustode-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in [
            "src/a.rs",
            "target/out.o",
            "node_modules/x/index.js",
            ".git/HEAD",
            ".env",
            "q.code-search",
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(&path, "").expect("write");
        }
        std::fs::write(root.join(".gitignore"), "target/\n").expect("gitignore");
        let mut files = list_files(&root).expect("walk");
        files.sort();
        assert_eq!(
            files,
            [
                PathBuf::from(".env"),
                PathBuf::from(".gitignore"),
                PathBuf::from("src/a.rs")
            ]
        );
        assert!(list_files(Path::new("/nonexistent/rustode-walk")).is_err());
    }
}
