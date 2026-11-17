//! A local stand-in for Open VSX, for tests: an HTTP/1.1 server on
//! 127.0.0.1 that answers the REST API with canned replies, and builds
//! VSIX files. The editor itself never uses it: it is compiled only with
//! the `stand-in` feature, which the workspace, TUI and GUI enable in their
//! dev-dependencies, so it never ships in a binary.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// A canned HTTP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The status code.
    pub status: u16,
    /// Extra headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl Reply {
    /// 200 with a JSON body.
    #[must_use]
    pub fn json(value: &Value) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: value.to_string().into_bytes(),
        }
    }

    /// 200 with these bytes.
    #[must_use]
    pub fn bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
        }
    }

    /// An empty reply with this status.
    #[must_use]
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// A 302 redirect to `location`.
    #[must_use]
    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
        }
    }
}

/// An extension the stand-in publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// The Open VSX namespace (publisher).
    pub namespace: String,
    /// The name.
    pub name: String,
    /// The version.
    pub version: String,
    /// The display name.
    pub display_name: String,
    /// The description.
    pub description: String,
    /// `universal`, `linux-x64`, …
    pub target_platform: String,
    /// Extra VSIX entries (full paths, e.g. `extension/out/main.js`).
    pub files: Vec<(String, Vec<u8>)>,
}

impl Fixture {
    /// A universal extension with a package.json and a README.
    #[must_use]
    pub fn new(namespace: &str, name: &str, version: &str) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            version: version.into(),
            display_name: format!("{name} display"),
            description: format!("About {name}"),
            target_platform: "universal".into(),
            files: Vec::new(),
        }
    }

    /// Its package.json.
    #[must_use]
    pub fn manifest(&self) -> Value {
        json!({
            "name": self.name,
            "publisher": self.namespace,
            "version": self.version,
            "displayName": self.display_name,
            "description": self.description,
            "engines": { "vscode": "^1.80.0" },
            "main": "./out/main.js",
        })
    }

    /// Its VSIX: the manifest files, `extension/package.json`, a README,
    /// a script, and [`Fixture::files`].
    #[must_use]
    pub fn vsix(&self) -> Vec<u8> {
        let manifest = self.manifest().to_string();
        let mut files: Vec<(String, Vec<u8>)> = vec![
            (
                "extension.vsixmanifest".into(),
                b"<PackageManifest/>".to_vec(),
            ),
            ("[Content_Types].xml".into(), b"<Types/>".to_vec()),
            ("extension/package.json".into(), manifest.into_bytes()),
            (
                "extension/README.md".into(),
                format!("# {}\n", self.display_name).into_bytes(),
            ),
            (
                "extension/out/main.js".into(),
                b"exports.activate = () => {};\n".to_vec(),
            ),
        ];
        files.extend(self.files.iter().cloned());
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        vsix(&entries)
    }

    /// The folder of its release files on the stand-in.
    fn file_path(&self) -> String {
        let platform = if self.target_platform == "universal" {
            String::new()
        } else {
            format!("/{}", self.target_platform)
        };
        format!(
            "/api/{}/{}{platform}/{}/file",
            self.namespace, self.name, self.version
        )
    }

    /// The file name of its VSIX.
    fn vsix_name(&self) -> String {
        format!("{}.{}-{}.vsix", self.namespace, self.name, self.version)
    }

    /// The download path of its VSIX on the stand-in.
    #[must_use]
    pub fn download_path(&self) -> String {
        format!("{}/{}", self.file_path(), self.vsix_name())
    }

    /// The path of its SHA-256 on the stand-in.
    #[must_use]
    pub fn sha256_path(&self) -> String {
        format!("{}/{}.sha256", self.file_path(), self.vsix_name())
    }

    /// Its entry in search results.
    #[must_use]
    pub fn search_entry(&self, base: &str) -> Value {
        json!({
            "url": format!("{base}/api/{}/{}", self.namespace, self.name),
            "files": { "download": format!("{base}{}", self.download_path()) },
            "name": self.name,
            "namespace": self.namespace,
            "version": self.version,
            "timestamp": "2026-01-01T00:00:00.000Z",
            "verified": true,
            "downloadCount": 1234,
            "displayName": self.display_name,
            "description": self.description,
        })
    }

    /// Its extension metadata, as `GET /api/{namespace}/{name}` answers.
    #[must_use]
    pub fn metadata(&self, base: &str) -> Value {
        json!({
            "namespace": self.namespace,
            "name": self.name,
            "version": self.version,
            "targetPlatform": self.target_platform,
            "displayName": self.display_name,
            "description": self.description,
            "namespaceDisplayName": format!("{} Inc.", self.namespace),
            "preRelease": false,
            "files": {
                "download": format!("{base}{}", self.download_path()),
                "sha256": format!("{base}{}", self.sha256_path()),
            },
        })
    }
}

/// Builds a VSIX (a zip archive, deflated) from `(path, bytes)` entries;
/// a path ending in `/` is a folder.
///
/// # Panics
/// Never for in-memory archives with valid names.
#[must_use]
pub fn vsix(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, options).expect("zip folder");
        } else {
            writer.start_file(*name, options).expect("zip entry");
            writer.write_all(bytes).expect("zip write");
        }
    }
    writer.finish().expect("zip finish").into_inner()
}

/// The SHA-256 of `bytes`, in lower-case hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    super::registry::hex(&Sha256::digest(bytes))
}

#[derive(Default)]
struct State {
    routes: Mutex<Vec<(String, Reply)>>,
    requests: Mutex<Vec<String>>,
    held: Mutex<bool>,
    released: Condvar,
}

impl State {
    fn reply(&self, path: &str) -> Reply {
        let routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        routes
            .iter()
            .rev()
            .find(|(route, _)| route == path)
            .map_or_else(|| Reply::status(404), |(_, reply)| reply.clone())
    }

    fn wait_while_held(&self) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        while *held {
            held = self
                .released
                .wait(held)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The server. It stops when dropped.
pub struct StandIn {
    addr: SocketAddr,
    url: String,
    state: Arc<State>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for StandIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StandIn")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl StandIn {
    /// Starts a server on a free port of 127.0.0.1.
    ///
    /// # Panics
    /// When no port can be bound.
    #[must_use]
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1");
        let addr = listener.local_addr().expect("local address");
        let state = Arc::new(State::default());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (state, stop) = (Arc::clone(&state), Arc::clone(&stop));
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let state = Arc::clone(&state);
                        std::thread::spawn(move || serve(&stream, &state));
                    }
                }
            })
        };
        Self {
            addr,
            url: format!("http://{addr}"),
            state,
            stop,
            thread: Some(thread),
        }
    }

    /// The base URL, e.g. `http://127.0.0.1:41234`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Answers requests for `path` (without the query) with `reply`; a
    /// later route for the same path wins.
    pub fn route(&self, path: &str, reply: Reply) {
        self.state
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((path.to_owned(), reply));
    }

    /// The request targets received (path and query), oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Holds every reply until [`StandIn::release`].
    pub fn hold(&self) {
        *self
            .state
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
    }

    /// Sends the held replies.
    pub fn release(&self) {
        *self
            .state
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = false;
        self.state.released.notify_all();
    }

    /// Publishes `fixture`: its metadata (also under `/linux-x64` for a
    /// platform build), its VSIX ([`Fixture::vsix`]) and its SHA-256.
    pub fn publish(&self, fixture: &Fixture) {
        let vsix = fixture.vsix();
        let base = format!("/api/{}/{}", fixture.namespace, fixture.name);
        let metadata = Reply::json(&fixture.metadata(&self.url));
        self.route(&base, metadata.clone());
        if fixture.target_platform != "universal" {
            self.route(&format!("{base}/{}", fixture.target_platform), metadata);
        }
        self.route(&fixture.download_path(), Reply::bytes(vsix.clone()));
        let sha = format!("{}  {}\n", sha256_hex(&vsix), fixture.vsix_name());
        self.route(&fixture.sha256_path(), Reply::bytes(sha.into_bytes()));
    }

    /// Makes `/api/-/search` answer with these extensions.
    pub fn search_results(&self, fixtures: &[Fixture]) {
        let entries: Vec<Value> = fixtures.iter().map(|f| f.search_entry(&self.url)).collect();
        let body = json!({ "offset": 0, "totalSize": entries.len() + 40, "extensions": entries });
        self.route("/api/-/search", Reply::json(&body));
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.release();
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Answers one connection.
fn serve(stream: &TcpStream, state: &State) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line.trim_end() != "" {
        line.clear();
    }
    let target = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    if target.is_empty() {
        return;
    }
    state
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(target.clone());
    state.wait_while_held();
    let path = target.split('?').next().unwrap_or_default();
    let reply = state.reply(path);
    let mut head = format!(
        "HTTP/1.1 {} Stand-in\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let mut stream = stream;
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}
