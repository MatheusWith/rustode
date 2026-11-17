//! Workspace seam: quick open ranking, the command registry and its
//! keybindings, and going to a line.

use rustode_workspace::{Position, Selection, Workspace};
use std::path::{Path, PathBuf};

fn project(name: &str, files: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rustode-qo-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for file in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(&path, "one\ntwo\nthree\n").expect("write");
    }
    root
}

fn labels(ws: &mut Workspace, query: &str) -> Vec<String> {
    ws.quick_open(query, 10)
        .into_iter()
        .map(|m| {
            if m.description.is_empty() {
                m.label
            } else {
                format!("{}/{}", m.description, m.label)
            }
        })
        .collect()
}

#[test]
fn a_file_name_prefix_wins_and_shorter_names_win_ties() {
    // VS Code's example: "window" ranks window.ts over windowActions.ts.
    let root = project(
        "rank",
        &[
            "src/window.ts",
            "src/windowActions.ts",
            "src/browser/browserWindow.ts",
            "readme.md",
        ],
    );
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("folder");
    assert_eq!(
        labels(&mut ws, "window"),
        [
            "src/window.ts",
            "src/windowActions.ts",
            "src/browser/browserWindow.ts"
        ]
    );
    assert_eq!(
        labels(&mut ws, "srcwa"),
        ["src/windowActions.ts"],
        "matches span the folder"
    );
    assert_eq!(labels(&mut ws, "zzz"), Vec::<String>::new());
}

#[test]
fn quick_open_skips_ignored_files_and_lists_recent_editors_when_empty() {
    let root = project("ignore", &["src/a.rs", "target/b.rs", "c.rs"]);
    std::fs::write(root.join(".gitignore"), "target/\n").expect("gitignore");
    let mut ws = Workspace::new();
    ws.open_folder(&root).expect("folder");
    assert_eq!(labels(&mut ws, ".rs"), ["c.rs", "src/a.rs"]);
    ws.open(&root.join("c.rs")).expect("open");
    ws.open(&root.join("src/a.rs")).expect("open");
    assert_eq!(
        labels(&mut ws, ""),
        ["src/a.rs", "c.rs"],
        "most recently used first"
    );
}

#[test]
fn commands_match_their_labels_and_show_keybindings() {
    let ws = Workspace::new();
    let found = ws.commands("save");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].command.id, "workbench.action.files.save");
    assert_eq!(found[0].keybinding.as_deref(), Some("Ctrl+S"));
    assert!(ws.commands("").len() > 10);
}

#[test]
fn user_keybindings_change_and_remove_labels() {
    let root = project("keys", &[]);
    std::fs::create_dir_all(&root).expect("root");
    let keybindings = root.join("keybindings.json");
    std::fs::write(
        &keybindings,
        r#"[
            { "key": "ctrl+alt+s", "command": "workbench.action.files.save" },
            { "key": "ctrl+d", "command": "-editor.action.addSelectionToNextFindMatch" }
        ]"#,
    )
    .expect("keybindings");
    let mut ws = Workspace::new();
    let _ = ws.load_config(Path::new("/nonexistent/settings.json"), &keybindings);
    assert_eq!(
        ws.keybinding_label("workbench.action.files.save")
            .as_deref(),
        Some("Ctrl+Alt+S")
    );
    assert_eq!(
        ws.keybinding_label("editor.action.addSelectionToNextFindMatch"),
        None
    );
}

#[test]
fn editor_commands_run_through_the_workspace() {
    let root = project("exec", &["f.txt"]);
    let mut ws = Workspace::new();
    let id = ws.open(&root.join("f.txt")).expect("open");
    assert!(ws.execute("editor.action.selectAll").expect("execute"));
    assert_eq!(
        ws.document(id).expect("doc").selections(),
        [Selection::new(0, 14)]
    );
    assert!(
        !ws.execute("workbench.action.quit").expect("execute"),
        "the frontend quits"
    );
    ws.go_to_line(id, 2, Some(3)).expect("go to line");
    assert_eq!(
        ws.document(id).expect("doc").cursor_position(),
        Position::new(1, 2)
    );
    ws.go_to_line(id, 99, None).expect("go to line");
    assert_eq!(
        ws.document(id).expect("doc").cursor_position(),
        Position::new(3, 0)
    );
}
