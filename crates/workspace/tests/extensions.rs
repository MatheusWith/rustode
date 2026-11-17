//! Workspace seam: extension management against a local Open VSX stand-in
//! on 127.0.0.1 — search, install, uninstall and the installed list, the
//! storage layout VS Code uses, and the refusals (Microsoft Marketplace,
//! unchecked downloads, paths that leave the extension folder). No test
//! touches the network or a real profile.

use rustode_workspace::extensions::stand_in::{Fixture, Reply, StandIn, sha256_hex, vsix};
use rustode_workspace::{
    Event, ExtensionError, ExtensionEvent, ExtensionOperation, ExtensionSearch, NOT_LOADED,
    Workspace, default_extensions_dir,
};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-ext-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A workspace whose extensions live in a fresh temp folder and whose
/// registry is the stand-in.
fn workspace(name: &str, stand_in: &StandIn) -> (Workspace, PathBuf) {
    let dir = temp(name).join("extensions");
    let mut ws = Workspace::new();
    ws.set_extensions_location(&dir);
    ws.set_extension_registry(stand_in.url()).expect("registry");
    (ws, dir)
}

/// Polls until no extension work is under way; returns the extension events.
fn settle(ws: &mut Workspace) -> Vec<ExtensionEvent> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while ws.extensions_busy() {
        ws.poll_extensions();
        assert!(Instant::now() < deadline, "extension work did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    ws.take_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Extension(event) => Some(event),
            _ => None,
        })
        .collect()
}

fn index(dir: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join("extensions.json")).expect("extensions.json");
    serde_json::from_str::<Value>(&text)
        .expect("json")
        .as_array()
        .expect("array")
        .clone()
}

/// Folder names in `dir`, sorted.
fn children(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn failure(events: &[ExtensionEvent]) -> String {
    events
        .iter()
        .find_map(|e| match e {
            ExtensionEvent::Failed { message, .. } => Some(message.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no failure in {events:?}"))
}

#[test]
fn a_search_lists_the_registry_results() {
    let stand_in = StandIn::start();
    stand_in.search_results(&[
        Fixture::new("acme", "hello", "1.2.3"),
        Fixture::new("acme", "world", "0.1.0"),
    ]);
    let (mut ws, _) = workspace("search", &stand_in);
    ws.search_extensions("  hello world  ");
    assert_eq!(
        ws.extension_search(),
        &ExtensionSearch::Searching {
            query: "hello world".into()
        }
    );
    let events = settle(&mut ws);
    assert!(events.contains(&ExtensionEvent::Searched {
        query: "hello world".into()
    }));
    let ExtensionSearch::Done {
        query,
        results,
        total,
    } = ws.extension_search()
    else {
        panic!("{:?}", ws.extension_search());
    };
    assert_eq!(query, "hello world");
    assert_eq!(*total, 42);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id(), "acme.hello");
    assert_eq!(results[0].display_name, "hello display");
    assert_eq!(results[0].version, "1.2.3");
    assert_eq!(results[0].description, "About hello");
    assert!(results[0].verified);
    assert_eq!(results[0].download_count, 1234);
    assert_eq!(
        stand_in.requests(),
        ["/api/-/search?query=hello%20world&size=50&offset=0&sortBy=relevance&sortOrder=desc"]
    );
}

#[test]
fn a_search_text_is_cut_at_200_chars_and_an_empty_one_sends_nothing() {
    let stand_in = StandIn::start();
    stand_in.search_results(&[]);
    let (mut ws, _) = workspace("search-long", &stand_in);
    ws.search_extensions("   ");
    assert_eq!(ws.extension_search(), &ExtensionSearch::Idle);
    assert!(!ws.extensions_busy());
    ws.search_extensions(&"x".repeat(250));
    settle(&mut ws);
    let requests = stand_in.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].contains(&format!("query={}&", "x".repeat(200))),
        "{requests:?}"
    );
}

#[test]
fn a_search_never_blocks_the_caller() {
    let stand_in = StandIn::start();
    stand_in.search_results(&[Fixture::new("acme", "hello", "1.0.0")]);
    stand_in.hold();
    let (mut ws, _) = workspace("non-blocking", &stand_in);
    ws.search_extensions("hello");
    // The stand-in answers nothing while held, yet the call has returned.
    assert!(ws.extensions_busy());
    assert!(!ws.poll_extensions());
    stand_in.release();
    settle(&mut ws);
    assert!(
        matches!(ws.extension_search(), ExtensionSearch::Done { results, .. } if results.len() == 1)
    );
}

#[test]
fn only_the_latest_search_counts() {
    let stand_in = StandIn::start();
    stand_in.search_results(&[Fixture::new("acme", "hello", "1.0.0")]);
    stand_in.hold();
    let (mut ws, _) = workspace("latest-search", &stand_in);
    ws.search_extensions("first");
    ws.search_extensions("second");
    stand_in.release();
    settle(&mut ws);
    assert!(
        matches!(ws.extension_search(), ExtensionSearch::Done { query, .. } if query == "second")
    );
}

#[test]
fn a_failed_search_is_reported() {
    let stand_in = StandIn::start();
    stand_in.route("/api/-/search", Reply::status(500));
    let (mut ws, _) = workspace("search-500", &stand_in);
    ws.search_extensions("hello");
    settle(&mut ws);
    let ExtensionSearch::Failed { message, .. } = ws.extension_search() else {
        panic!("{:?}", ws.extension_search());
    };
    assert!(message.contains("HTTP 500"), "{message}");
}

/// The extension's folder holds only the VSIX's `extension/` contents, and
/// its package.json gained VS Code's `__metadata`.
fn assert_folder(folder: &Path) {
    assert_eq!(
        children(folder),
        ["README.md", "out", "package.json"],
        "only extension/ is extracted"
    );
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("package.json")).expect("read"))
            .expect("json");
    assert_eq!(manifest["name"], "Hello");
    let metadata = &manifest["__metadata"];
    assert_eq!(metadata["targetPlatform"], "universal");
    assert!(metadata["installedTimestamp"].is_u64(), "{metadata}");
    assert!(
        metadata["size"].as_u64().is_some_and(|size| size > 0),
        "{metadata}"
    );
}

/// `extensions.json` has VS Code's entry for the extension.
fn assert_index(dir: &Path, folder: &Path) {
    let entries = index(dir);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["identifier"]["id"], "acme.hello");
    assert_eq!(entry["version"], "1.2.3");
    assert_eq!(entry["relativeLocation"], "acme.hello-1.2.3-universal");
    let location = &entry["location"];
    assert_eq!(location["scheme"], "file");
    assert_eq!(location["$mid"], 1);
    assert_eq!(location["path"], folder.to_string_lossy().as_ref());
    let metadata = &entry["metadata"];
    assert_eq!(metadata["source"], "gallery");
    assert_eq!(metadata["targetPlatform"], "universal");
    assert_eq!(metadata["publisherDisplayName"], "Acme Inc.");
    assert_eq!(metadata["updated"], false);
}

#[test]
fn an_install_lays_the_extension_out_as_vs_code_does() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("Acme", "Hello", "1.2.3"));
    let (mut ws, dir) = workspace("install", &stand_in);
    ws.install_extension("Acme.Hello").expect("install");
    assert_eq!(
        ws.extension_operation("acme.hello"),
        Some(ExtensionOperation::Installing)
    );
    let events = settle(&mut ws);
    let installed = ExtensionEvent::Installed {
        id: "acme.hello".into(),
        version: "1.2.3".into(),
    };
    assert!(events.contains(&installed), "{events:?}");
    assert_eq!(ws.extension_operation("acme.hello"), None);
    assert_eq!(
        children(&dir),
        ["acme.hello-1.2.3-universal", "extensions.json"]
    );
    let folder = dir.join("acme.hello-1.2.3-universal");
    assert_folder(&folder);
    assert_index(&dir, &folder);
}

#[test]
fn installed_extensions_are_listed_as_not_loaded() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("Acme", "Hello", "1.2.3"));
    let (mut ws, dir) = workspace("listed", &stand_in);
    ws.install_extension("Acme.Hello").expect("install");
    settle(&mut ws);
    let installed = ws.installed_extensions();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "acme.hello");
    assert_eq!(installed[0].display_name, "Hello display");
    assert_eq!(
        installed[0].location,
        dir.join("acme.hello-1.2.3-universal")
    );
    assert!(!installed[0].is_loaded());
    assert_eq!(installed[0].notice(), NOT_LOADED);
    assert_eq!(NOT_LOADED, "Not loaded in this phase");
    assert!(ws.installed_extension("ACME.HELLO").is_some());
}

#[test]
fn installing_the_installed_version_again_downloads_nothing() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let (mut ws, _) = workspace("reinstall", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    settle(&mut ws);
    let before = stand_in.requests().len();
    ws.install_extension("acme.hello").expect("install again");
    let events = settle(&mut ws);
    assert!(events.contains(&ExtensionEvent::Installed {
        id: "acme.hello".into(),
        version: "1.0.0".into()
    }));
    assert_eq!(
        stand_in.requests()[before..],
        ["/api/acme/hello"],
        "only the metadata is asked for"
    );
}

#[test]
fn a_new_version_replaces_the_old_one() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    let (mut ws, dir) = workspace("update", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    settle(&mut ws);
    stand_in.publish(&Fixture::new("acme", "hello", "1.1.0"));
    ws.install_extension("acme.hello").expect("update");
    settle(&mut ws);
    assert_eq!(
        children(&dir),
        ["acme.hello-1.1.0-universal", "extensions.json"]
    );
    let entries = index(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["version"], "1.1.0");
    assert_eq!(entries[0]["metadata"]["updated"], true);
    assert_eq!(ws.installed_extensions()[0].version, "1.1.0");
}

#[test]
fn a_platform_specific_extension_gets_its_linux_x64_build() {
    let stand_in = StandIn::start();
    let mut windows = Fixture::new("acme", "native", "2.0.0");
    windows.target_platform = "win32-x64".into();
    let mut linux = Fixture::new("acme", "native", "2.0.0");
    linux.target_platform = "linux-x64".into();
    linux
        .files
        .push(("extension/bin/server".into(), b"#!/bin/sh\n".to_vec()));
    stand_in.publish(&linux);
    // The latest release is the Windows build; the Linux one is asked for.
    stand_in.route(
        "/api/acme/native",
        Reply::json(&windows.metadata(stand_in.url())),
    );
    let (mut ws, dir) = workspace("platform", &stand_in);
    ws.install_extension("acme.native").expect("install");
    let events = settle(&mut ws);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ExtensionEvent::Installed { .. })),
        "{events:?}"
    );
    assert_eq!(
        children(&dir),
        ["acme.native-2.0.0-linux-x64", "extensions.json"]
    );
    assert!(
        stand_in
            .requests()
            .contains(&"/api/acme/native/linux-x64".to_owned())
    );
    assert_eq!(
        ws.installed_extensions()[0].target_platform.as_deref(),
        Some("linux-x64")
    );
}

#[test]
fn an_extension_without_a_linux_build_is_not_installed() {
    let stand_in = StandIn::start();
    let mut windows = Fixture::new("acme", "native", "2.0.0");
    windows.target_platform = "win32-x64".into();
    stand_in.route(
        "/api/acme/native",
        Reply::json(&windows.metadata(stand_in.url())),
    );
    let (mut ws, dir) = workspace("no-platform", &stand_in);
    ws.install_extension("acme.native").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("no linux-x64 build"), "{message}");
    assert_eq!(children(&dir), Vec::<String>::new());
}

#[test]
fn a_download_whose_sha256_differs_is_refused_and_leaves_nothing() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    stand_in.route(
        &fixture.sha256_path(),
        Reply::bytes(sha256_hex(b"something else").into_bytes()),
    );
    let (mut ws, dir) = workspace("sha-mismatch", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("SHA-256"), "{message}");
    assert_eq!(
        children(&dir),
        Vec::<String>::new(),
        "no temporary file or folder is left"
    );
    assert_eq!(ws.installed_extensions(), &[]);
}

#[test]
fn a_download_without_a_published_sha256_is_refused() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let mut metadata = fixture.metadata(stand_in.url());
    metadata["files"]
        .as_object_mut()
        .expect("files")
        .remove("sha256");
    stand_in.route("/api/acme/hello", Reply::json(&metadata));
    let (mut ws, _) = workspace("sha-missing", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("no SHA-256"), "{message}");
    assert!(
        !stand_in.requests().iter().any(|r| r.contains(".vsix")),
        "nothing is downloaded"
    );
}

#[test]
fn an_entry_that_leaves_the_extension_folder_is_refused() {
    let stand_in = StandIn::start();
    let mut fixture = Fixture::new("acme", "slip", "1.0.0");
    fixture
        .files
        .push(("extension/../../escaped.txt".into(), b"owned".to_vec()));
    stand_in.publish(&fixture);
    let (mut ws, dir) = workspace("zip-slip", &stand_in);
    ws.install_extension("acme.slip").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("leaves the extension folder"), "{message}");
    let root = dir.parent().expect("parent");
    assert!(!root.join("escaped.txt").exists() && !dir.join("escaped.txt").exists());
    assert_eq!(children(&dir), Vec::<String>::new());
}

#[test]
fn a_vsix_for_another_extension_is_refused() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let other = Fixture::new("acme", "other", "1.0.0").vsix();
    stand_in.route(&fixture.download_path(), Reply::bytes(other.clone()));
    stand_in.route(
        &fixture.sha256_path(),
        Reply::bytes(sha256_hex(&other).into_bytes()),
    );
    let (mut ws, dir) = workspace("identity", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("acme.other"), "{message}");
    assert_eq!(children(&dir), Vec::<String>::new());
}

#[test]
fn a_vsix_without_a_manifest_or_not_a_zip_is_refused() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let (mut ws, _) = workspace("bad-vsix", &stand_in);
    for (bytes, expected) in [
        (
            vsix(&[("extension/README.md", b"no manifest")]),
            "no extension/package.json",
        ),
        (b"not a zip".to_vec(), "not a VSIX"),
    ] {
        stand_in.route(&fixture.download_path(), Reply::bytes(bytes.clone()));
        stand_in.route(
            &fixture.sha256_path(),
            Reply::bytes(sha256_hex(&bytes).into_bytes()),
        );
        ws.install_extension("acme.hello").expect("install");
        let message = failure(&settle(&mut ws));
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn the_microsoft_marketplace_is_never_contacted() {
    // Mixed case on purpose: hosts are compared without case.
    let mut ws = Workspace::new();
    for url in [
        "https://Marketplace.VisualStudio.Com/_apis/public/gallery",
        "https://publisher.gallery.VSAssets.io",
        "https://cdn.vsassets.io./x",
        "https://www.vscode-unpkg.net",
    ] {
        assert!(
            matches!(
                ws.set_extension_registry(url),
                Err(ExtensionError::Refused(_))
            ),
            "{url}"
        );
    }
    assert_eq!(ws.extension_registry(), rustode_workspace::OPEN_VSX);

    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let mut metadata = fixture.metadata(stand_in.url());
    metadata["files"]["download"] = "https://acme.GalleryCDN.vsassets.io/acme.hello.vsix".into();
    stand_in.route("/api/acme/hello", Reply::json(&metadata));
    let (mut ws, _) = workspace("marketplace-link", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("never contacts it"), "{message}");

    stand_in.publish(&fixture);
    stand_in.route(
        &fixture.download_path(),
        Reply::redirect("https://Marketplace.VisualStudio.Com/x.vsix"),
    );
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("never contacts it"), "{message}");
    assert!(
        !stand_in.requests().iter().any(|r| r.contains("x.vsix")),
        "the redirect is not followed"
    );
}

#[test]
fn redirects_are_followed_and_checked() {
    let stand_in = StandIn::start();
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let moved = "/storage/acme.hello-1.0.0.vsix";
    stand_in.route(moved, Reply::bytes(fixture.vsix()));
    stand_in.route(&fixture.download_path(), Reply::redirect(moved));
    let (mut ws, _) = workspace("redirect", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    let events = settle(&mut ws);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ExtensionEvent::Installed { .. })),
        "{events:?}"
    );
    assert!(stand_in.requests().contains(&moved.to_owned()));

    stand_in.route("/loop", Reply::redirect("/loop"));
    stand_in.route("/api/-/search", Reply::redirect("/loop"));
    ws.search_extensions("x");
    settle(&mut ws);
    assert!(
        matches!(ws.extension_search(), ExtensionSearch::Failed { message, .. } if message.contains("too many redirects"))
    );
}

#[test]
fn registries_must_be_https_or_on_this_machine() {
    let mut ws = Workspace::new();
    for url in [
        "http://example.org",
        "ftp://example.org",
        "open-vsx.org",
        "",
    ] {
        assert!(
            matches!(
                ws.set_extension_registry(url),
                Err(ExtensionError::InvalidRegistry(_))
            ),
            "{url}"
        );
    }
    ws.set_extension_registry("https://registry.example/")
        .expect("https");
    assert_eq!(ws.extension_registry(), "https://registry.example");
    ws.set_extension_registry("http://localhost:3000")
        .expect("loopback");
}

#[test]
fn uninstall_removes_the_folder_and_the_index_entry() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    stand_in.publish(&Fixture::new("acme", "world", "3.0.0"));
    let (mut ws, dir) = workspace("uninstall", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    ws.install_extension("acme.world").expect("install");
    settle(&mut ws);
    assert_eq!(ws.installed_extensions().len(), 2);
    ws.uninstall_extension("ACME.hello").expect("uninstall");
    assert_eq!(
        ws.extension_operation("acme.hello"),
        Some(ExtensionOperation::Uninstalling)
    );
    let events = settle(&mut ws);
    assert!(
        events.contains(&ExtensionEvent::Uninstalled {
            id: "acme.hello".into()
        }),
        "{events:?}"
    );
    assert_eq!(
        children(&dir),
        ["acme.world-3.0.0-universal", "extensions.json"],
        "no .obsolete left"
    );
    let entries = index(&dir);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["identifier"]["id"], "acme.world");
    assert_eq!(ws.installed_extensions().len(), 1);

    ws.uninstall_extension("acme.hello").expect("queued");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("not installed"), "{message}");
}

#[test]
fn a_listing_finishes_interrupted_deletions() {
    let stand_in = StandIn::start();
    let (mut ws, dir) = workspace("cleanup", &stand_in);
    std::fs::create_dir_all(dir.join("acme.gone-1.0.0-universal.1234.vsctmp/sub")).expect("vsctmp");
    std::fs::create_dir_all(dir.join("acme.old-1.0.0-universal")).expect("obsolete folder");
    std::fs::write(
        dir.join(".obsolete"),
        r#"{"acme.old-1.0.0-universal":true,"acme.missing-1.0.0":true}"#,
    )
    .expect(".obsolete");
    ws.refresh_extensions().expect("refresh");
    let events = settle(&mut ws);
    assert!(events.contains(&ExtensionEvent::Listed));
    assert_eq!(children(&dir), Vec::<String>::new());
}

#[test]
fn an_index_written_by_vs_code_is_listed() {
    let stand_in = StandIn::start();
    let (mut ws, dir) = workspace("vs-code-index", &stand_in);
    let folder = dir.join("acme.nls-1.0.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("package.json"),
        r#"{"name":"nls","publisher":"acme","version":"1.0.0","displayName":"%name%","description":"%about%"}"#,
    )
    .expect("manifest");
    std::fs::write(
        folder.join("package.nls.json"),
        r#"{"name":"Localized Name","about":{"message":"Localized description","comment":["x"]}}"#,
    )
    .expect("nls");
    // The old format: an absolute location and no relativeLocation.
    let entry = serde_json::json!([{
        "identifier": {"id": "acme.nls", "uuid": "0000"},
        "version": "1.0.0",
        "location": {"$mid": 1, "path": folder.to_string_lossy(), "scheme": "file"},
    }, {
        "identifier": {"id": "acme.vanished"},
        "version": "1.0.0",
        "relativeLocation": "acme.vanished-1.0.0",
        "location": {"$mid": 1, "path": "/nowhere", "scheme": "file"},
    }]);
    std::fs::write(dir.join("extensions.json"), entry.to_string()).expect("index");
    ws.refresh_extensions().expect("refresh");
    settle(&mut ws);
    let installed = ws.installed_extensions();
    assert_eq!(
        installed.len(),
        1,
        "a missing folder is skipped: {installed:?}"
    );
    assert_eq!(installed[0].display_name, "Localized Name");
    assert_eq!(installed[0].description, "Localized description");
}

#[test]
fn an_invalid_index_is_reported_and_never_overwritten() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    let (mut ws, dir) = workspace("bad-index", &stand_in);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("extensions.json"), "{ not json").expect("index");
    ws.refresh_extensions().expect("refresh");
    let events = settle(&mut ws);
    assert!(events.iter().any(|e| matches!(e, ExtensionEvent::ListFailed { message } if message.contains("not a valid extensions index"))), "{events:?}");
    ws.install_extension("acme.hello").expect("install");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("left unchanged"), "{message}");
    assert_eq!(
        std::fs::read_to_string(dir.join("extensions.json")).expect("read"),
        "{ not json"
    );
}

#[test]
fn commands_are_refused_before_they_start() {
    let mut ws = Workspace::new();
    assert_eq!(
        ws.install_extension("acme.hello"),
        Err(ExtensionError::NoLocation)
    );
    assert_eq!(ws.refresh_extensions(), Err(ExtensionError::NoLocation));
    let stand_in = StandIn::start();
    stand_in.hold();
    let (mut ws, _) = workspace("refused", &stand_in);
    for bad in ["hello", "../../etc.passwd", "a.b.c", "acme/x.y"] {
        assert!(
            matches!(ws.install_extension(bad), Err(ExtensionError::InvalidId(_))),
            "{bad}"
        );
    }
    ws.install_extension("acme.hello").expect("install");
    assert_eq!(
        ws.install_extension("acme.hello"),
        Err(ExtensionError::Busy("acme.hello".into()))
    );
    assert_eq!(
        ws.uninstall_extension("Acme.Hello"),
        Err(ExtensionError::Busy("acme.hello".into()))
    );
    stand_in.release();
    settle(&mut ws);
}

#[test]
fn extension_commands_run_by_id_with_an_argument() {
    let stand_in = StandIn::start();
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    stand_in.search_results(&[Fixture::new("acme", "hello", "1.0.0")]);
    let (mut ws, _) = workspace("execute-with", &stand_in);
    assert_eq!(
        ws.execute_with("workbench.extensions.search", "hello"),
        Ok(true)
    );
    assert_eq!(
        ws.execute_with("workbench.extensions.installExtension", "acme.hello"),
        Ok(true)
    );
    settle(&mut ws);
    assert_eq!(ws.installed_extensions().len(), 1);
    assert!(matches!(
        ws.extension_search(),
        ExtensionSearch::Done { .. }
    ));
    assert_eq!(
        ws.execute_with("workbench.extensions.uninstallExtension", "acme.hello"),
        Ok(true)
    );
    settle(&mut ws);
    assert_eq!(ws.installed_extensions(), &[]);
    assert_eq!(ws.execute_with("no.such.command", "x"), Ok(false));
    assert!(
        ws.commands("show extensions")
            .iter()
            .any(|m| m.command.id == "workbench.view.extensions"
                && m.keybinding.as_deref() == Some("Ctrl+Shift+X"))
    );
}

#[test]
fn the_waker_is_called_when_results_arrive() {
    let stand_in = StandIn::start();
    stand_in.search_results(&[]);
    let (mut ws, _) = workspace("waker", &stand_in);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    ws.set_extensions_waker(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    ws.search_extensions("x");
    ws.refresh_extensions().expect("refresh");
    settle(&mut ws);
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_default_folder_is_rustodes_own() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    };
    assert_eq!(
        default_extensions_dir(env(&[
            ("RUSTODE_EXTENSIONS_DIR", "/x/ext"),
            ("HOME", "/home/u")
        ])),
        Some(PathBuf::from("/x/ext"))
    );
    assert_eq!(
        default_extensions_dir(env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/data/rustode/extensions"))
    );
    assert_eq!(
        default_extensions_dir(env(&[("XDG_DATA_HOME", "relative"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.local/share/rustode/extensions"))
    );
    assert_eq!(
        default_extensions_dir(env(&[("RUSTODE_EXTENSIONS_DIR", "")])),
        None
    );
    let mut ws = Workspace::new();
    assert_eq!(ws.extensions_location(), None);
    ws.set_extensions_location(Path::new("/x"));
    assert_eq!(ws.extensions_location(), Some(Path::new("/x")));
}
