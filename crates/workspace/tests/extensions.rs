//! Workspace seam: extension management against a local Open VSX stand-in
//! on 127.0.0.1 — search, install, uninstall, enable, disable and the
//! installed list, the storage layout and manifest checks VS Code uses, and
//! the refusals (Microsoft Marketplace, unchecked downloads, paths that
//! leave the extension folder). No test touches the network or a real
//! profile.

use rustode_workspace::extensions::stand_in::{Fixture, Reply, StandIn, sha256_hex, vsix};
use rustode_workspace::{
    EnablementState, Event, ExtensionError, ExtensionEvent, ExtensionManifest, ExtensionOperation,
    ExtensionSearch, ManifestProblem, NOT_LOADED, Severity, Workspace, default_extensions_dir,
    default_global_storage_dir,
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

/// A workspace whose extensions live in a fresh temp folder, next to its
/// global storage folder ([`storage`]), and whose registry is the stand-in.
fn workspace(name: &str, stand_in: &StandIn) -> (Workspace, PathBuf) {
    let dir = temp(name).join("extensions");
    let ws = reopen(&dir, stand_in);
    (ws, dir)
}

/// Another workspace on the same folders, as after a restart.
fn reopen(dir: &Path, stand_in: &StandIn) -> Workspace {
    let mut ws = Workspace::new();
    ws.set_extensions_location(dir);
    ws.set_global_storage_location(&storage(dir));
    ws.set_extension_registry(stand_in.url()).expect("registry");
    ws
}

/// The global storage folder next to the extensions folder `dir`.
fn storage(dir: &Path) -> PathBuf {
    dir.parent().expect("parent").join("globalStorage")
}

/// The text of the global state file, if there is one.
fn state(dir: &Path) -> Option<String> {
    std::fs::read_to_string(storage(dir).join("state.json")).ok()
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
    let mut names: Vec<String> = std::fs::read_dir(dir).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        },
    );
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let other = Fixture::new("acme", "other", "1.0.0")
        .vsix()
        .expect("the fixture packs");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let (mut ws, _) = workspace("bad-vsix", &stand_in);
    for (bytes, expected) in [
        (
            vsix(&[("extension/README.md", b"no manifest")]).expect("the archive packs"),
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

    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
    let fixture = Fixture::new("acme", "hello", "1.0.0");
    stand_in.publish(&fixture);
    let moved = "/storage/acme.hello-1.0.0.vsix";
    stand_in.route(
        moved,
        Reply::bytes(fixture.vsix().expect("the fixture packs")),
    );
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    assert_eq!(installed.len(), 2, "{installed:?}");
    assert_eq!(installed[0].display_name, "Localized Name");
    assert_eq!(installed[0].description, "Localized description");
    // A missing folder is listed as invalid, with a manifest made up from
    // the index, as VS Code lists it; it can still be uninstalled.
    let vanished = &installed[1];
    assert_eq!(vanished.id, "acme.vanished");
    assert_eq!(vanished.display_name, "vanished");
    assert_eq!(vanished.enablement, EnablementState::Invalid);
    assert!(
        vanished
            .first_error()
            .is_some_and(|e| e.contains("cannot read") && e.contains("package.json")),
        "{vanished:?}"
    );
    assert_eq!(vanished.manifest.publisher(), Some("acme"));
    assert_eq!(vanished.manifest.version(), Some("1.0.0"));
    ws.uninstall_extension("acme.vanished").expect("uninstall");
    settle(&mut ws);
    assert_eq!(ws.installed_extensions().len(), 1);
}

#[test]
fn an_invalid_index_is_reported_and_never_overwritten() {
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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
    let stand_in = StandIn::start().expect("the stand-in starts");
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

/// The installed extension `id`'s enablement, as last listed.
fn enablement(ws: &Workspace, id: &str) -> EnablementState {
    ws.installed_extension(id)
        .unwrap_or_else(|| panic!("{id} is not installed: {:?}", ws.installed_extensions()))
        .enablement
}

/// A workspace with `acme.hello` 1.0.0 installed.
fn with_hello(name: &str, stand_in: &StandIn) -> (Workspace, PathBuf) {
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    let (mut ws, dir) = workspace(name, stand_in);
    ws.install_extension("acme.hello").expect("install");
    settle(&mut ws);
    (ws, dir)
}

#[test]
fn disabling_is_kept_as_vs_code_keeps_it_and_survives_a_restart() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("disable", &stand_in);
    assert_eq!(enablement(&ws, "acme.hello"), EnablementState::Enabled);
    assert_eq!(
        state(&dir),
        None,
        "nothing is written until something changes"
    );

    ws.disable_extension("Acme.Hello").expect("disable");
    assert_eq!(
        ws.extension_operation("acme.hello"),
        Some(ExtensionOperation::Disabling)
    );
    let events = settle(&mut ws);
    assert!(
        events.contains(&ExtensionEvent::Disabled {
            id: "acme.hello".into()
        }),
        "{events:?}"
    );
    assert_eq!(
        enablement(&ws, "acme.hello"),
        EnablementState::DisabledGlobally
    );
    // VS Code's storage row: the key and the JSON text of the identifiers.
    assert_eq!(
        state(&dir).as_deref(),
        Some(r#"{"extensionsIdentifiers/disabled":"[{\"id\":\"acme.hello\"}]"}"#)
    );
    let installed = ws.installed_extension("acme.hello").expect("installed");
    assert!(!installed.is_enabled());
    assert!(!installed.is_loaded(), "nothing loads in Phase 1");
    assert_eq!(installed.notice(), NOT_LOADED);
}

#[test]
fn a_disabled_extension_stays_disabled_after_a_restart_until_enabled() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("restart", &stand_in);
    ws.disable_extension("acme.hello").expect("disable");
    settle(&mut ws);
    drop(ws);

    let mut restarted = reopen(&dir, &stand_in);
    restarted.refresh_extensions().expect("refresh");
    settle(&mut restarted);
    assert_eq!(
        enablement(&restarted, "acme.hello"),
        EnablementState::DisabledGlobally
    );

    restarted.enable_extension("acme.hello").expect("enable");
    assert_eq!(
        restarted.extension_operation("acme.hello"),
        Some(ExtensionOperation::Enabling)
    );
    let events = settle(&mut restarted);
    assert!(
        events.contains(&ExtensionEvent::Enabled {
            id: "acme.hello".into()
        }),
        "{events:?}"
    );
    assert_eq!(
        enablement(&restarted, "acme.hello"),
        EnablementState::Enabled
    );
    assert_eq!(
        state(&dir).as_deref(),
        Some("{}"),
        "an empty list removes the key, as VS Code does"
    );
    assert!(
        !restarted
            .installed_extension("acme.hello")
            .expect("installed")
            .is_loaded(),
        "enabling still loads nothing"
    );
}

#[test]
fn enabling_an_enabled_extension_changes_nothing() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("enable-twice", &stand_in);
    ws.enable_extension("acme.hello").expect("enable");
    let events = settle(&mut ws);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ExtensionEvent::Enabled { .. })),
        "{events:?}"
    );
    assert_eq!(state(&dir), None);
    ws.disable_extension("acme.hello").expect("disable");
    settle(&mut ws);
    ws.disable_extension("acme.hello").expect("disable again");
    let events = settle(&mut ws);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ExtensionEvent::Disabled { .. })),
        "{events:?}"
    );
    assert_eq!(
        enablement(&ws, "acme.hello"),
        EnablementState::DisabledGlobally
    );
}

#[test]
fn enable_and_disable_run_by_command_id() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, _) = with_hello("enable-commands", &stand_in);
    assert_eq!(
        ws.execute_with("workbench.extensions.disableExtension", "acme.hello"),
        Ok(true)
    );
    settle(&mut ws);
    assert_eq!(
        enablement(&ws, "acme.hello"),
        EnablementState::DisabledGlobally
    );
    assert_eq!(
        ws.execute_with("workbench.extensions.enableExtension", "acme.hello"),
        Ok(true)
    );
    settle(&mut ws);
    assert_eq!(enablement(&ws, "acme.hello"), EnablementState::Enabled);
    assert!(matches!(
        ws.execute_with("workbench.extensions.enableExtension", "nope"),
        Err(ExtensionError::InvalidId(_))
    ));
}

#[test]
fn enabling_an_extension_that_is_not_installed_fails() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = workspace("enable-missing", &stand_in);
    ws.enable_extension("acme.hello").expect("queued");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("not installed"), "{message}");
    ws.disable_extension("acme.hello").expect("queued");
    let message = failure(&settle(&mut ws));
    assert!(message.contains("not installed"), "{message}");
    assert_eq!(state(&dir), None);
}

#[test]
fn a_new_version_stays_disabled_and_uninstalling_forgets_it() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("disabled-update", &stand_in);
    ws.disable_extension("acme.hello").expect("disable");
    settle(&mut ws);
    stand_in.publish(&Fixture::new("acme", "hello", "1.1.0"));
    ws.install_extension("acme.hello").expect("update");
    settle(&mut ws);
    assert_eq!(ws.installed_extensions()[0].version, "1.1.0");
    assert_eq!(
        enablement(&ws, "acme.hello"),
        EnablementState::DisabledGlobally,
        "VS Code keeps enablement by id, not by version"
    );

    ws.uninstall_extension("acme.hello").expect("uninstall");
    settle(&mut ws);
    assert_eq!(state(&dir).as_deref(), Some("{}"), "uninstalling resets it");
    ws.install_extension("acme.hello").expect("install again");
    settle(&mut ws);
    assert_eq!(enablement(&ws, "acme.hello"), EnablementState::Enabled);
}

/// Writes an extension folder and a VS Code-format index entry for it.
fn write_extension(dir: &Path, id: &str, version: &str, manifest: &serde_json::Value) -> PathBuf {
    let key = format!("{id}-{version}");
    let folder = dir.join(&key);
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("package.json"), manifest.to_string()).expect("manifest");
    let mut entries = if dir.join("extensions.json").exists() {
        index(dir)
    } else {
        Vec::new()
    };
    entries.push(serde_json::json!({
        "identifier": {"id": id},
        "version": version,
        "location": {"$mid": 1, "path": folder.to_string_lossy(), "scheme": "file"},
        "relativeLocation": key,
    }));
    std::fs::write(
        dir.join("extensions.json"),
        serde_json::Value::Array(entries).to_string(),
    )
    .expect("index");
    folder
}

#[test]
fn an_invalid_extension_is_listed_as_invalid_and_its_enablement_cannot_change() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = workspace("invalid", &stand_in);
    write_extension(
        &dir,
        "acme.broken",
        "1.0.0",
        &serde_json::json!({"name": "broken", "publisher": "acme", "version": "1.0.0", "main": "x.js"}),
    );
    ws.refresh_extensions().expect("refresh");
    settle(&mut ws);
    let broken = ws.installed_extension("acme.broken").expect("listed");
    assert_eq!(broken.enablement, EnablementState::Invalid);
    assert!(!broken.is_valid() && !broken.is_enabled());
    assert_eq!(broken.problems.len(), 1, "{:?}", broken.problems);
    assert_eq!(broken.problems[0].severity, Severity::Error);
    assert!(
        broken.problems[0].message.contains("`engines`"),
        "{:?}",
        broken.problems
    );
    assert_eq!(broken.notice(), NOT_LOADED);

    for disable in [true, false] {
        let started = if disable {
            ws.disable_extension("acme.broken")
        } else {
            ws.enable_extension("acme.broken")
        };
        started.expect("queued");
        let message = failure(&settle(&mut ws));
        assert!(message.contains("invalid"), "{message}");
        assert!(message.contains("`engines`"), "{message}");
    }
    assert_eq!(state(&dir), None);
}

#[test]
fn an_extension_disabled_before_it_became_invalid_stays_disabled_and_can_be_enabled() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = workspace("invalid-disabled", &stand_in);
    write_extension(
        &dir,
        "acme.old",
        "1.0.0",
        &serde_json::json!({"name": "old", "publisher": "acme", "version": "1.0.0",
            "engines": {"vscode": "^1.999.0"}, "main": "x.js"}),
    );
    std::fs::create_dir_all(storage(&dir)).expect("storage");
    std::fs::write(
        storage(&dir).join("state.json"),
        r#"{"extensionsIdentifiers/disabled":"[{\"id\":\"acme.old\"}]"}"#,
    )
    .expect("state");
    ws.refresh_extensions().expect("refresh");
    settle(&mut ws);
    let old = ws.installed_extension("acme.old").expect("listed");
    assert_eq!(old.enablement, EnablementState::DisabledGlobally);
    assert!(!old.is_valid());
    assert!(
        old.problems[0].message.contains("1.147.0"),
        "{:?}",
        old.problems
    );
    ws.enable_extension("acme.old").expect("enable");
    let events = settle(&mut ws);
    assert!(events.contains(&ExtensionEvent::Enabled {
        id: "acme.old".into()
    }));
    assert_eq!(enablement(&ws, "acme.old"), EnablementState::Invalid);
}

#[test]
fn an_installed_extension_for_a_newer_vs_code_is_invalid() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let mut fixture = Fixture::new("acme", "future", "1.0.0");
    fixture.engine = "^1.200.0".into();
    stand_in.publish(&fixture);
    let (mut ws, _) = workspace("future-engine", &stand_in);
    ws.install_extension("acme.future").expect("install");
    let events = settle(&mut ws);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ExtensionEvent::Installed { .. })),
        "VS Code installs it too, and lists it as invalid: {events:?}"
    );
    let future = ws.installed_extension("acme.future").expect("listed");
    assert_eq!(future.enablement, EnablementState::Invalid);
    assert!(
        future.problems[0].message.contains("^1.200.0"),
        "{:?}",
        future.problems
    );
}

#[test]
fn state_written_by_vs_code_is_read_and_its_other_rows_are_kept() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("vs-code-state", &stand_in);
    std::fs::create_dir_all(storage(&dir)).expect("storage");
    std::fs::write(
        storage(&dir).join("state.json"),
        r#"{"other.key":"kept","extensionsIdentifiers/disabled":"[{\"id\":\"ACME.Hello\",\"uuid\":\"1234\"},{\"id\":\"acme.gone\"}]"}"#,
    )
    .expect("state");
    ws.refresh_extensions().expect("refresh");
    settle(&mut ws);
    assert_eq!(
        enablement(&ws, "acme.hello"),
        EnablementState::DisabledGlobally,
        "ids compare without case"
    );
    ws.enable_extension("acme.hello").expect("enable");
    settle(&mut ws);
    assert_eq!(
        state(&dir).as_deref(),
        Some(r#"{"other.key":"kept","extensionsIdentifiers/disabled":"[{\"id\":\"acme.gone\"}]"}"#)
    );
}

#[test]
fn an_unreadable_obsolete_file_is_reported_and_never_overwritten() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("bad-obsolete", &stand_in);
    let folder = dir.join("acme.hello-1.0.0-universal");
    let index_before = std::fs::read_to_string(dir.join("extensions.json")).expect("index");
    for bad in [
        "{ not json",
        "[]",
        r#"{"acme.hello-1.0.0-universal":"yes"}"#,
    ] {
        std::fs::write(dir.join(".obsolete"), bad).expect(".obsolete");
        // A listing still works: the file is not needed to list.
        ws.refresh_extensions().expect("refresh");
        let events = settle(&mut ws);
        assert!(
            events.contains(&ExtensionEvent::Listed),
            "{bad}: {events:?}"
        );
        // Changing the folder is refused, naming the file and saying what to do.
        ws.uninstall_extension("acme.hello").expect("queued");
        let message = failure(&settle(&mut ws));
        assert!(message.contains(".obsolete"), "{bad}: {message}");
        assert!(message.contains("left unchanged"), "{bad}: {message}");
        assert!(!message.contains("{:?}"), "{message}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".obsolete")).expect("read"),
            bad
        );
        assert!(folder.is_dir(), "{bad}: the folder stays");
        assert_eq!(
            std::fs::read_to_string(dir.join("extensions.json")).expect("index"),
            index_before,
            "{bad}: the index stays"
        );
    }
}

#[test]
fn an_unreadable_state_file_is_reported_and_never_overwritten() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (mut ws, dir) = with_hello("bad-state", &stand_in);
    std::fs::create_dir_all(storage(&dir)).expect("storage");
    for bad in [
        "{ not json",
        r#"{"extensionsIdentifiers/disabled":"[{\"uuid\":\"1\"}]"}"#,
        r#"{"extensionsIdentifiers/disabled":"nope"}"#,
        r#"{"extensionsIdentifiers/disabled":[]}"#,
    ] {
        std::fs::write(storage(&dir).join("state.json"), bad).expect("state");
        ws.refresh_extensions().expect("refresh");
        let events = settle(&mut ws);
        assert!(
            events.iter().any(|e| matches!(e, ExtensionEvent::StateFailed { message } if message.contains("left unchanged"))),
            "{bad}: {events:?}"
        );
        assert_eq!(
            enablement(&ws, "acme.hello"),
            EnablementState::Enabled,
            "listed as VS Code does with empty storage"
        );
        ws.disable_extension("acme.hello").expect("disable");
        let message = failure(&settle(&mut ws));
        assert!(message.contains("left unchanged"), "{message}");
        assert_eq!(state(&dir).as_deref(), Some(bad));
    }
}

#[test]
fn errors_name_the_item_and_say_what_to_do() {
    let no_folder = ExtensionError::NoLocation.to_string();
    assert!(no_folder.contains("RUSTODE_EXTENSIONS_DIR"), "{no_folder}");
    let bad_id = ExtensionError::InvalidId("nope".into()).to_string();
    assert!(bad_id.contains("nope"), "{bad_id}");
    let mut ws = Workspace::new();
    assert_eq!(
        ws.install_extension("acme.hello"),
        Err(ExtensionError::NoLocation)
    );
}

#[test]
fn enablement_needs_a_global_storage_folder() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let dir = temp("no-storage").join("extensions");
    let mut ws = Workspace::new();
    ws.set_extensions_location(&dir);
    assert_eq!(ws.global_storage_location(), None);
    assert_eq!(
        ws.disable_extension("acme.hello"),
        Err(ExtensionError::NoStorageLocation)
    );
    assert_eq!(
        ws.enable_extension("acme.hello"),
        Err(ExtensionError::NoStorageLocation)
    );
    assert!(
        ExtensionError::NoStorageLocation
            .to_string()
            .contains("storage")
    );
    // Without one, everything lists as enabled.
    ws.set_extension_registry(stand_in.url()).expect("registry");
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    ws.install_extension("acme.hello").expect("install");
    settle(&mut ws);
    assert_eq!(enablement(&ws, "acme.hello"), EnablementState::Enabled);
    ws.set_global_storage_location(Path::new("/x/globalStorage"));
    assert_eq!(
        ws.global_storage_location(),
        Some(Path::new("/x/globalStorage"))
    );
}

#[test]
fn enable_and_disable_wait_for_an_install_of_the_same_extension() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    stand_in.publish(&Fixture::new("acme", "hello", "1.0.0"));
    stand_in.hold();
    let (mut ws, _) = workspace("busy-enable", &stand_in);
    ws.install_extension("acme.hello").expect("install");
    assert_eq!(
        ws.disable_extension("acme.hello"),
        Err(ExtensionError::Busy("acme.hello".into()))
    );
    assert_eq!(
        ws.enable_extension("ACME.hello"),
        Err(ExtensionError::Busy("acme.hello".into()))
    );
    stand_in.release();
    settle(&mut ws);
}

#[test]
fn the_default_global_storage_is_rustodes_own() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    };
    assert_eq!(
        default_global_storage_dir(env(&[("RUSTODE_USER_DIR", "/u"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/u/globalStorage"))
    );
    assert_eq!(
        default_global_storage_dir(env(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/cfg/rustode/User/globalStorage"))
    );
    assert_eq!(
        default_global_storage_dir(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.config/rustode/User/globalStorage"))
    );
    assert_eq!(default_global_storage_dir(env(&[])), None);
}

/// The keys of a JSON object, in their order (`serde_json` keeps the order:
/// the workspace enables its `preserve_order` feature).
fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect()
}

/// `acme.hello` 1.2.3, whose package.json has three more fields after the
/// usual ones, installed.
fn with_extra_fields(name: &str, stand_in: &StandIn) -> (Workspace, PathBuf) {
    let mut fixture = Fixture::new("acme", "hello", "1.2.3");
    fixture.manifest_fields = vec![
        ("zebra".into(), serde_json::json!({"z": 1, "a": [2, 1]})),
        (
            "contributes".into(),
            serde_json::json!({"commands": [{"title": "T", "command": "c"}]}),
        ),
        ("alpha".into(), serde_json::json!(null)),
    ];
    stand_in.publish(&fixture);
    let (mut ws, dir) = workspace(name, stand_in);
    ws.install_extension("acme.hello").expect("install");
    settle(&mut ws);
    (ws, dir)
}

#[test]
fn extensions_json_is_written_in_vs_codes_key_order() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (_, dir) = with_extra_fields("index-order", &stand_in);
    let text = std::fs::read_to_string(dir.join("extensions.json")).expect("index");
    let entries: Value = serde_json::from_str(&text).expect("json");
    let entry = &entries[0];
    assert_eq!(
        keys(entry),
        [
            "identifier",
            "version",
            "location",
            "relativeLocation",
            "metadata"
        ]
    );
    assert_eq!(keys(&entry["location"]), ["$mid", "path", "scheme"]);
    assert_eq!(
        keys(&entry["metadata"]),
        [
            "installedTimestamp",
            "source",
            "publisherDisplayName",
            "targetPlatform",
            "updated",
            "private",
            "isPreReleaseVersion",
            "hasPreReleaseVersion",
            "preRelease"
        ]
    );
}

#[test]
fn package_json_keeps_its_field_order_and_gets_metadata_last() {
    let stand_in = StandIn::start().expect("the stand-in starts");
    let (ws, dir) = with_extra_fields("manifest-order", &stand_in);
    // Indented with tabs, as `JSON.stringify(manifest, null, '\t')` does.
    let folder = dir.join("acme.hello-1.2.3-universal");
    let text = std::fs::read_to_string(folder.join("package.json")).expect("manifest");
    let manifest: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        keys(&manifest),
        [
            "name",
            "publisher",
            "version",
            "displayName",
            "description",
            "engines",
            "main",
            "zebra",
            "contributes",
            "alpha",
            "__metadata"
        ]
    );
    assert_eq!(keys(&manifest["zebra"]), ["z", "a"]);
    assert_eq!(
        keys(&manifest["__metadata"]),
        ["installedTimestamp", "targetPlatform", "size"]
    );
    assert!(
        text.starts_with("{\n\t\"name\": \"hello\",\n\t\"publisher\": \"acme\",\n"),
        "{text}"
    );
    assert!(text.contains("\n\t\"alpha\": null,\n"), "{text}");
    assert!(
        text.contains("\"zebra\": {\n\t\t\"z\": 1,\n\t\t\"a\": [\n\t\t\t2,\n\t\t\t1\n\t\t]\n\t}"),
        "{text}"
    );
    assert!(!text.ends_with('\n'), "JSON.stringify adds no newline");
    // The listed manifest model holds every field.
    let installed = ws.installed_extension("acme.hello").expect("installed");
    let unknown: Vec<&str> = installed
        .manifest
        .unknown_fields()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(unknown, ["zebra", "alpha", "__metadata"]);
}

/// A manifest with every field the model reads, some of another type,
/// and two it does not know.
const FULL_MANIFEST: &str = r#"{
    "name": "hello", "publisher": "acme", "version": "1.0.0",
    "engines": {"vscode": "^1.80.0", "node": ">=18"},
    "displayName": "%name%", "description": 7,
    "main": "./out/main.js", "browser": "./web/main.js",
    "activationEvents": ["onStartupFinished", "onCommand:x"],
    "contributes": {"commands": []},
    "extensionDependencies": ["acme.base"], "extensionPack": ["acme.a", "acme.b"],
    "extensionKind": "workspace", "icon": "icon.png", "x-custom": {"deep": [1]}
}"#;

#[test]
fn the_manifest_model_reads_vs_codes_fields() {
    let manifest = ExtensionManifest::parse(FULL_MANIFEST).expect("manifest");
    assert_eq!(manifest.name(), Some("hello"));
    assert_eq!(manifest.publisher(), Some("acme"));
    assert_eq!(manifest.version(), Some("1.0.0"));
    assert_eq!(manifest.engine(), Some("^1.80.0"));
    assert_eq!(manifest.display_name(), Some("%name%"));
    assert_eq!(manifest.main(), Some("./out/main.js"));
    assert_eq!(manifest.browser(), Some("./web/main.js"));
    assert_eq!(
        manifest.activation_events(),
        Some(vec!["onStartupFinished", "onCommand:x"])
    );
    assert_eq!(
        manifest.contributes(),
        Some(&serde_json::json!({"commands": []}))
    );
    assert_eq!(manifest.extension_dependencies(), Some(vec!["acme.base"]));
    assert_eq!(manifest.extension_pack(), Some(vec!["acme.a", "acme.b"]));
    assert_eq!(manifest.extension_kind(), Some(vec!["workspace"]));
    let kinds =
        ExtensionManifest::parse(r#"{"extensionKind": ["ui", "workspace"]}"#).expect("kinds");
    assert_eq!(kinds.extension_kind(), Some(vec!["ui", "workspace"]));
    assert_eq!(kinds.name(), None);
}

#[test]
fn the_manifest_model_keeps_every_field() {
    let manifest = ExtensionManifest::parse(FULL_MANIFEST).expect("manifest");
    assert_eq!(manifest.description(), None, "not a string");
    assert_eq!(
        manifest.get("description"),
        Some(&serde_json::json!(7)),
        "but kept"
    );
    let unknown: Vec<&str> = manifest.unknown_fields().map(|(k, _)| k).collect();
    assert_eq!(unknown, ["icon", "x-custom"]);
    assert_eq!(manifest.fields().len(), 15, "nothing is dropped");
    assert_eq!(manifest.fields()["engines"]["node"], ">=18");
    assert_eq!(
        ExtensionManifest::from_fields(manifest.fields().clone()),
        manifest
    );
    for bad in ["[1]", "\"x\"", "{ nope"] {
        assert!(ExtensionManifest::parse(bad).is_err(), "{bad}");
    }
}

/// The first error validation finds in `manifest`, if any.
fn first_error(manifest: &Value) -> Option<String> {
    ExtensionManifest::parse(&manifest.to_string())
        .expect("object")
        .validate(Path::new("/ext/acme.x-1.0.0"))
        .into_iter()
        .find(ManifestProblem::is_error)
        .map(|p| p.message)
}

#[test]
fn manifests_are_validated_as_vs_codes_scanner_does() {
    use serde_json::json;
    let valid = json!({"name": "x", "publisher": "acme", "version": "1.0.0",
        "engines": {"vscode": "^1.80.0"}, "main": "./out/main.js"});
    assert_eq!(first_error(&valid), None);
    let with = |key: &str, value: Value| {
        let mut manifest = valid.clone();
        manifest[key] = value;
        manifest
    };
    let without = |key: &str| {
        let mut manifest = valid.clone();
        manifest.as_object_mut().expect("object").remove(key);
        manifest
    };
    // Each case breaks one rule; the expected text names the property.
    let cases = [
        (with("publisher", json!(5)), "`publisher`"),
        (without("name"), "`name`"),
        (with("name", json!(["x"])), "`name`"),
        (without("version"), "`version`"),
        (without("engines"), "`engines`"),
        (with("engines", json!("^1.80.0")), "`engines.vscode`"),
        (with("engines", json!({"vscode": 1})), "`engines.vscode`"),
        (
            with("extensionDependencies", json!("acme.y")),
            "`extensionDependencies`",
        ),
        (with("extensionAffinity", json!([1])), "`extensionAffinity`"),
        (with("activationEvents", json!(null)), "`activationEvents`"),
        (with("main", json!(1)), "`main`"),
        (with("browser", json!(false)), "`browser`"),
        (with("version", json!("1.0")), "semver"),
        (
            with("engines", json!({"vscode": "*"})),
            "not specific enough",
        ),
        (with("engines", json!({"vscode": "^2.0.0"})), "1.147.0"),
    ];
    for (manifest, expected) in &cases {
        let error = first_error(manifest).unwrap_or_else(|| panic!("valid: {manifest}"));
        assert!(error.contains(expected), "{manifest}: {error}");
    }
    let no_code = json!({"name": "x", "version": "1.0.0", "engines": {"vscode": "^9.0.0"},
        "activationEvents": ["*"]});
    assert!(
        first_error(&no_code).is_some_and(|e| e.contains("`activationEvents`")),
        "activation events need code"
    );
    // A declarative extension (no main, no browser) skips the engine check,
    // an empty publisher reads as undefined_publisher, and these are
    // warnings only.
    let declarative = json!({"name": "x", "publisher": "", "version": "1.0.0",
        "engines": {"vscode": "^9.0.0"}, "extensionKind": ["ui"]});
    assert_eq!(first_error(&declarative), None);
    let outside = with("main", json!("../../elsewhere/main.js"));
    let problems = ExtensionManifest::parse(&outside.to_string())
        .expect("object")
        .validate(Path::new("/ext/acme.x-1.0.0"));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].severity, Severity::Warning);
    assert!(problems[0].message.contains("`main`"), "{problems:?}");
    let kind = ExtensionManifest::parse(&declarative.to_string())
        .expect("object")
        .validate(Path::new("/ext/x"));
    assert!(
        kind.iter()
            .any(|p| !p.is_error() && p.message.contains("`extensionKind`")),
        "{kind:?}"
    );
}
