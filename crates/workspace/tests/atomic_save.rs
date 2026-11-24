//! Workspace seam: a save is atomic. It never leaves a temporary file behind,
//! whatever step fails, writes through a symbolic link to the file it points
//! to, keeps the file's permissions, and names the path when it fails.
//!
//! The `fsync` of the temporary file and of the folder cannot be observed
//! from a test; review covers them (`crates/fs/src/atomic.rs`).

#![cfg(unix)]

use rustode_workspace::Workspace;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-atomic-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The names in a folder, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Opens `path` and types `text` at its start, so the document is dirty.
fn open_and_edit(ws: &mut Workspace, path: &Path, text: &str) -> rustode_workspace::DocumentId {
    let id = ws.open(path).expect("open");
    ws.type_text(id, text).expect("edit");
    id
}

/// Whether this process can create files in a folder whose write bit is off
/// (root can), in which case the unwritable-folder tests have nothing to show.
fn can_bypass_permissions(dir: &Path) -> bool {
    let probe = dir.join("probe");
    let bypass = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    bypass
}

#[test]
fn a_save_leaves_only_the_file_in_its_folder() {
    let dir = temp_dir("clean");
    let path = dir.join("a.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    ws.save(id).expect("save");
    assert_eq!(std::fs::read(&path).expect("read"), b"new old\n");
    assert_eq!(names(&dir), ["a.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_planted_at_the_temporary_name_is_never_written() {
    // A link planted where a fixed temporary name would be must not redirect
    // the save into another file.
    let dir = temp_dir("planted");
    let path = dir.join("a.txt");
    let victim = dir.join("victim.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    std::fs::write(&victim, b"victim\n").expect("victim");
    std::os::unix::fs::symlink(&victim, dir.join(".a.txt.rustode-save")).expect("plant");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    ws.save(id).expect("save");
    assert_eq!(std::fs::read(&path).expect("read"), b"new old\n");
    assert_eq!(std::fs::read(&victim).expect("victim"), b"victim\n");
    assert_eq!(names(&dir), [".a.txt.rustode-save", "a.txt", "victim.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_through_a_symlink_writes_the_target_and_keeps_the_link() {
    let dir = temp_dir("symlink");
    let real = dir.join("real");
    let links = dir.join("links");
    std::fs::create_dir_all(&real).expect("real dir");
    std::fs::create_dir_all(&links).expect("links dir");
    let target = real.join("target.txt");
    std::fs::write(&target, b"old\n").expect("target");
    let link = links.join("link.txt");
    std::os::unix::fs::symlink("../real/target.txt", &link).expect("link");

    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &link, "new ");
    ws.save(id).expect("save");

    let meta = std::fs::symlink_metadata(&link).expect("link meta");
    assert!(meta.file_type().is_symlink(), "the link is kept");
    assert_eq!(
        std::fs::read_link(&link).expect("read link"),
        Path::new("../real/target.txt")
    );
    assert_eq!(std::fs::read(&target).expect("target"), b"new old\n");
    assert_eq!(names(&real), ["target.txt"]);
    assert_eq!(names(&links), ["link.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_through_a_chain_of_symlinks_writes_the_last_target() {
    let dir = temp_dir("chain");
    let target = dir.join("target.txt");
    std::fs::write(&target, b"old\n").expect("target");
    std::os::unix::fs::symlink("target.txt", dir.join("middle.txt")).expect("middle");
    std::os::unix::fs::symlink("middle.txt", dir.join("first.txt")).expect("first");

    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &dir.join("first.txt"), "new ");
    ws.save(id).expect("save");

    assert_eq!(std::fs::read(&target).expect("target"), b"new old\n");
    assert_eq!(names(&dir), ["first.txt", "middle.txt", "target.txt"]);
    for link in ["first.txt", "middle.txt"] {
        let meta = std::fs::symlink_metadata(dir.join(link)).expect("meta");
        assert!(meta.file_type().is_symlink(), "{link} is kept");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_symlink_loop_is_an_error_naming_the_path() {
    let dir = temp_dir("loop");
    let path = dir.join("a.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    // Replace the opened file with a loop of links before saving.
    std::fs::remove_file(&path).expect("remove");
    std::os::unix::fs::symlink("b.txt", &path).expect("a -> b");
    std::os::unix::fs::symlink("a.txt", dir.join("b.txt")).expect("b -> a");

    let err = ws.save(id).expect_err("a loop cannot be saved through");
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "{err}"
    );
    assert_eq!(names(&dir), ["a.txt", "b.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_save_keeps_the_file_permissions() {
    let dir = temp_dir("mode");
    let path = dir.join("script.sh");
    std::fs::write(&path, b"echo old\n").expect("fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).expect("chmod");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "# ");
    ws.save(id).expect("save");
    let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o7777;
    assert_eq!(mode, 0o750);
    assert_eq!(names(&dir), ["script.sh"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unwritable_folder_is_an_error_naming_the_path_and_leaves_nothing() {
    let dir = temp_dir("readonly");
    let path = dir.join("a.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    if can_bypass_permissions(&dir) {
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(dir);
        eprintln!("skipped: this user writes into read-only folders");
        return;
    }

    let err = ws.save(id).expect_err("the folder is not writable");
    let names_after = names(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod back");

    assert!(
        err.to_string().contains(&path.display().to_string()),
        "{err}"
    );
    assert_eq!(names_after, ["a.txt"]);
    assert_eq!(std::fs::read(&path).expect("read"), b"old\n");
    assert!(
        ws.document(id).expect("doc").is_dirty(),
        "unsaved work is kept"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_rename_removes_the_temporary_file() {
    // The file was replaced by a folder after it was opened: the temporary
    // file is written, the rename over a folder fails, and the temporary
    // file must go.
    let dir = temp_dir("rename");
    let path = dir.join("a.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    std::fs::remove_file(&path).expect("remove");
    std::fs::create_dir(&path).expect("folder in its place");
    std::fs::write(path.join("inside.txt"), b"x").expect("non-empty folder");

    let err = ws.save(id).expect_err("cannot rename over a folder");
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "{err}"
    );
    assert_eq!(names(&dir), ["a.txt"]);
    assert_eq!(names(&path), ["inside.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_folder_is_an_error_naming_the_path() {
    let dir = temp_dir("gone");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("sub");
    let path = sub.join("a.txt");
    std::fs::write(&path, b"old\n").expect("fixture");
    let mut ws = Workspace::new();
    let id = open_and_edit(&mut ws, &path, "new ");
    std::fs::remove_dir_all(&sub).expect("remove folder");

    let err = ws.save(id).expect_err("the folder is gone");
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "{err}"
    );
    assert_eq!(names(&dir), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(dir);
}
