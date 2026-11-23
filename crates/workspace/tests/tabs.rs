//! Workspace seam: editor tabs and the explorer tree.

use rustode_workspace::{Error, ExplorerRow, Workspace};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-tabs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn names(rows: &[ExplorerRow]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            format!(
                "{}{}{}",
                "  ".repeat(r.depth),
                r.name,
                if r.is_dir { "/" } else { "" }
            )
        })
        .collect()
}

#[test]
fn opening_files_adds_tabs_and_reopening_reuses_the_document() {
    let root = dir("open");
    for name in ["a.txt", "b.txt"] {
        write(&root.join(name), name);
    }
    let mut ws = Workspace::new();
    let a = ws.open(&root.join("a.txt")).expect("open a");
    let b = ws.open(&root.join("b.txt")).expect("open b");
    assert_eq!(ws.tabs(), [a, b]);
    assert_eq!(ws.active_document(), Some(b));
    let again = ws.open(&root.join("a.txt")).expect("open a again");
    assert_eq!(again, a, "one document per file");
    assert_eq!(ws.tabs(), [a, b]);
    assert_eq!(ws.active_document(), Some(a));
}

#[test]
fn next_and_previous_tab_wrap_around() {
    let root = dir("cycle");
    let mut ws = Workspace::new();
    let ids: Vec<_> = ["1", "2", "3"]
        .iter()
        .map(|n| {
            let path = root.join(n);
            write(&path, n);
            ws.open(&path).expect("open")
        })
        .collect();
    ws.next_tab();
    assert_eq!(ws.active_document(), Some(ids[0]));
    ws.previous_tab();
    assert_eq!(ws.active_document(), Some(ids[2]));
}

#[test]
fn ctrl_tab_returns_to_the_previous_tab_and_ctrl_shift_tab_to_the_oldest() {
    let root = dir("recent");
    let mut ws = Workspace::new();
    let open = |ws: &mut Workspace, name: &str| {
        let path = root.join(name);
        write(&path, name);
        ws.open(&path).expect("open")
    };
    let a = open(&mut ws, "a");
    ws.recently_used_tab();
    assert_eq!(
        ws.active_document(),
        Some(a),
        "one tab: nothing to switch to"
    );
    let b = open(&mut ws, "b");
    let c = open(&mut ws, "c");
    ws.recently_used_tab();
    assert_eq!(ws.active_document(), Some(b), "the tab used before c");
    ws.recently_used_tab();
    assert_eq!(ws.active_document(), Some(c), "a second tap goes back");
    ws.least_recently_used_tab();
    assert_eq!(ws.active_document(), Some(a), "a was used longest ago");
    assert_eq!(ws.tabs(), [a, b, c], "the tab order does not change");
}

#[test]
fn closing_a_tab_activates_the_most_recently_used_one() {
    let root = dir("close");
    let mut ws = Workspace::new();
    let open = |ws: &mut Workspace, name: &str| {
        let path = root.join(name);
        write(&path, name);
        ws.open(&path).expect("open")
    };
    let a = open(&mut ws, "a");
    let b = open(&mut ws, "b");
    let c = open(&mut ws, "c");
    ws.activate(a).expect("activate");
    ws.activate(c).expect("activate");
    ws.close(c, false).expect("close");
    assert_eq!(
        ws.active_document(),
        Some(a),
        "VS Code focuses the recent editor"
    );
    assert_eq!(ws.tabs(), [a, b]);
    assert!(ws.document(c).is_none());
}

#[test]
fn closing_unsaved_changes_needs_an_explicit_discard() {
    let root = dir("dirty");
    let path = root.join("f.txt");
    write(&path, "x");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    ws.type_text(id, "y").expect("type");
    assert!(matches!(ws.close(id, false), Err(Error::Dirty { id: d, .. }) if d == id));
    assert_eq!(ws.tabs(), [id]);
    ws.close(id, true).expect("discard");
    assert_eq!(ws.tabs(), &[] as &[rustode_workspace::DocumentId]);
    assert_eq!(ws.active_document(), None);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "x");
}

#[test]
fn the_explorer_sorts_folders_first_and_numbers_naturally() {
    let root = dir("explorer");
    for file in [
        "file10.txt",
        "file2.txt",
        "B.txt",
        "a.txt",
        "src/main.rs",
        "docs/x.md",
        ".git/HEAD",
        ".env",
    ] {
        write(&root.join(file), "");
    }
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("open folder");
    let explorer = ws.explorer().expect("explorer");
    assert_eq!(explorer.root(), root.as_path());
    assert_eq!(
        names(&explorer.rows()),
        [
            "docs/",
            "src/",
            ".env",
            "a.txt",
            "B.txt",
            "file2.txt",
            "file10.txt"
        ],
        ".git is excluded by default"
    );
}

#[test]
fn folders_expand_collapse_and_refresh() {
    let root = dir("expand");
    write(&root.join("src/lib.rs"), "");
    write(&root.join("src/inner/deep.rs"), "");
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("open folder");
    assert!(ws.toggle_folder(&root.join("src")).expect("toggle"));
    let rows = ws.explorer().expect("explorer").rows();
    assert_eq!(names(&rows), ["src/", "  inner/", "  lib.rs"]);
    assert!(rows[0].expanded);
    write(&root.join("src/new.rs"), "");
    ws.refresh_explorer();
    assert_eq!(
        names(&ws.explorer().expect("explorer").rows()),
        ["src/", "  inner/", "  lib.rs", "  new.rs"]
    );
    assert!(!ws.toggle_folder(&root.join("src")).expect("toggle"));
    assert_eq!(names(&ws.explorer().expect("explorer").rows()), ["src/"]);
}

#[test]
fn opening_a_folder_that_does_not_exist_is_an_error() {
    let mut ws = Workspace::new();
    assert!(
        ws.open_folder(Path::new("/nonexistent/rustode-folder"))
            .is_err()
    );
    assert!(ws.explorer().is_none());
}

#[test]
fn errors_name_the_file_and_never_an_internal_id() {
    let root = dir("messages");
    let path = root.join("notes.txt");
    write(&path, "x");
    let mut ws = Workspace::new();
    let id = ws.open(&path).expect("open");
    ws.type_text(id, "y").expect("type");
    let dirty = ws.close(id, false).expect_err("an error").to_string();
    assert!(dirty.contains("notes.txt"), "{dirty}");
    ws.close(id, true).expect("discard");
    let unknown = ws.activate(id).expect_err("an error").to_string();
    assert!(!unknown.contains("DocumentId"), "{unknown}");
    assert!(!dirty.contains("DocumentId"), "{dirty}");
}

#[test]
fn an_unknown_group_error_has_no_internal_id() {
    let ws = Workspace::new();
    let group = ws.active_group();
    let message = Error::UnknownGroup(group).to_string();
    assert!(!message.contains("GroupId"), "{message}");
}
