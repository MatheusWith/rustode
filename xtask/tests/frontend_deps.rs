//! Frontends (CLI, TUI, GUI) reach the editor only through the workspace
//! crate: among rustode's own crates, `rustode-workspace` is the only one they
//! may depend on (normal, dev or build). Third-party crates are not limited
//! here. Spec: "Frontends depend only on workspace, enforced by a test."

use std::process::Command;

const FRONTENDS: [&str; 3] = ["rustode-cli", "rustode-tui", "rustode-gui"];

fn metadata() -> serde_json::Value {
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("metadata is JSON")
}

/// The rustode crates each frontend depends on, other than the workspace.
fn forbidden(metadata: &serde_json::Value) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let packages = metadata["packages"].as_array().expect("packages");
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default();
        if !FRONTENDS.contains(&name) {
            continue;
        }
        for dep in package["dependencies"].as_array().expect("dependencies") {
            let dep_name = dep["name"].as_str().unwrap_or_default();
            if dep_name.starts_with("rustode-") && dep_name != "rustode-workspace" {
                found.push((name.to_owned(), dep_name.to_owned()));
            }
        }
    }
    found
}

#[test]
fn frontends_depend_on_no_rustode_crate_but_the_workspace() {
    let metadata = metadata();
    let names: Vec<&str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    for frontend in FRONTENDS {
        assert!(
            names.contains(&frontend),
            "{frontend} is a workspace member"
        );
    }
    assert_eq!(forbidden(&metadata), Vec::<(String, String)>::new());
}

#[test]
fn the_check_catches_a_frontend_that_reaches_past_the_workspace() {
    let fake = serde_json::json!({ "packages": [
        { "name": "rustode-tui", "dependencies": [
            { "name": "rustode-workspace" }, { "name": "rustode-core" }, { "name": "ratatui" }
        ] },
        { "name": "rustode-core", "dependencies": [ { "name": "rustode-fs" } ] }
    ] });
    assert_eq!(
        forbidden(&fake),
        [("rustode-tui".to_owned(), "rustode-core".to_owned())]
    );
}
