//! The language server client: servers configured in `settings.json`
//! (`rustode.languageServers`, see [`settings`]) run as child processes over
//! stdio ([ADR 0015](../../../../docs/adr/0015-child-process-servers.md)),
//! one per workspace root and language, started when the first document of
//! that language opens. The root is the opened folder when the file is in
//! it, else the file's folder.
//!
//! The workspace supervises them: it sends `initialize`, the documents
//! (`didOpen`, `didChange` in full or by range as the server asks,
//! `didSave`, `didClose`) and `shutdown`/`exit` when it is dropped; it notices
//! a crash and restarts the server ([`server`] has the policy). Messages
//! arrive on a reader thread and wait in a channel until a frontend calls
//! [`Workspace::poll_language_servers`]; the waker set with
//! [`Workspace::set_waker`] tells an idle frontend to do so. Every message is
//! traced in a `child_message` span.
//!
//! Go to definition and find references ([`navigate`]), rename and format
//! ([`edits`]) are requests whose answers are applied by the workspace:
//! the frontends only pick a location or ask for a name.
//!
//! Diagnostics become markers ([`markers`]): per document, drawn inline, and
//! in the problems list, which covers every file a server reported.

mod edits;
mod encoding;
mod filter;
mod hover;
mod markers;
mod navigate;
mod requests;
mod server;
mod settings;
mod suggest;
mod sync;
mod tsserver;
mod uri;

pub use hover::Hover;
pub use markers::{Marker, MarkerSeverity, Problem, ProblemCounts};
pub use navigate::{Locations, LocationsKind, RenameTarget, SymbolLocation};
pub use server::{ServerState, ServerStatus};
pub use suggest::{CompletionKind, CompletionList, CompletionMove, CompletionRow};

use crate::child::{Incoming, Waker};
use crate::{Document, DocumentId, Error, Event, Position, Range, Selection, Workspace};
use encoding::PositionEncoding;
use markers::{Anchor, Stored};
use rustode_trace::spans::{self, Direction};
use serde_json::{Value, json};
use server::{Server, ServerId, SyncKind};
use settings::ServerConfig;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The diagnostics one server published for one file.
#[derive(Debug)]
struct Published {
    server: ServerId,
    markers: Vec<Stored>,
}

/// An open document attached to a server.
#[derive(Debug)]
struct Tracked {
    server: ServerId,
    path: PathBuf,
    uri: String,
    version: i64,
    /// The text as of the last change processed: what the server has, and
    /// what the next change is measured against.
    baseline: String,
}

/// Every language server of a workspace and what they reported.
#[derive(Debug, Default)]
pub(crate) struct LanguageServers {
    servers: Vec<Server>,
    next_server: u64,
    configs: BTreeMap<String, ServerConfig>,
    problems: Vec<String>,
    files: BTreeMap<PathBuf, Vec<Published>>,
    tracked: HashMap<DocumentId, Tracked>,
    waker: Waker,
    /// The search path the built-in default servers are looked up in; `None`
    /// until a frontend asks for the defaults.
    default_search_path: Option<std::ffi::OsString>,
    /// Requests sent and not answered yet (hover, completion, navigation,
    /// rename, format).
    pending: HashMap<(ServerId, i64), requests::Kind>,
    /// What the navigation, rename and format requests in flight were about.
    ops: HashMap<(ServerId, i64), requests::Op>,
    /// The navigation request in flight: a newer one cancels it.
    navigation: Option<requests::Request>,
    /// The locations waiting to be picked from.
    locations: Option<Locations>,
    hover: Option<hover::State>,
    suggest: Option<suggest::Session>,
}

impl LanguageServers {
    fn server(&self, id: ServerId) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    fn server_mut(&mut self, id: ServerId) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    /// Forgets what `server` published; returns whether there was anything.
    fn clear(&mut self, server: ServerId) -> bool {
        let mut cleared = false;
        self.files.retain(|_, published| {
            let before = published.len();
            published.retain(|p| p.server != server);
            cleared |= published.len() != before;
            !published.is_empty()
        });
        cleared
    }
}

impl Drop for LanguageServers {
    fn drop(&mut self) {
        for server in &mut self.servers {
            server.stop();
        }
    }
}

impl Workspace {
    /// Sets the callback the workspace's background work calls, from its
    /// own thread, when something arrived: a message from a language server,
    /// an extension result or search results. A frontend uses it to wake its
    /// event loop (the GUI requests a repaint), then calls
    /// [`poll_language_servers`](Self::poll_language_servers) and
    /// [`poll_extensions`](Self::poll_extensions), and
    /// [`poll_search`](Self::poll_search). It covers
    /// [`set_extensions_waker`](Self::set_extensions_waker) and
    /// [`set_search_waker`](Self::set_search_waker) too.
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        self.lsp.waker.set(Arc::clone(&wake));
        self.dap.waker().set(Arc::clone(&wake));
        let extensions = Arc::clone(&wake);
        self.set_extensions_waker(move || extensions());
        self.set_search_waker(move || wake());
    }

    /// Processes what the language servers sent since the last call:
    /// diagnostics, answers, crashes (restarting the server when the policy
    /// allows). Outcomes are reported as [`Event`]s.
    pub fn poll_language_servers(&mut self) {
        let mut incoming = Vec::new();
        for server in &self.lsp.servers {
            if let Some(process) = &server.process {
                while let Some(item) = process.try_recv() {
                    let closed = matches!(item, Incoming::Closed);
                    incoming.push((server.id, item));
                    if closed {
                        break;
                    }
                }
            }
        }
        for (id, item) in incoming {
            match item {
                Incoming::Message(message) => self.lsp_message(id, &message),
                Incoming::Closed => self.lsp_closed(id),
            }
        }
    }

    /// Turns on the built-in language servers: `rust-analyzer` for Rust and
    /// `typescript-language-server` for TypeScript and JavaScript, for each
    /// language the user's `rustode.languageServers` does not mention and
    /// whose program is in a folder of `search_path` (the value of `PATH`).
    /// A workspace made with [`Workspace::new`] has none, so tests and
    /// tools start no server by accident. Open documents of those languages
    /// are attached at once.
    pub fn use_default_language_servers(&mut self, search_path: &std::ffi::OsStr) {
        self.lsp.default_search_path = Some(search_path.to_owned());
        self.lsp_config_changed();
    }

    /// The diagnostics of an open document, where to draw them, sorted by
    /// position.
    #[must_use]
    pub fn markers(&self, id: DocumentId) -> Vec<Marker> {
        let Some(doc) = self.document(id) else {
            return Vec::new();
        };
        let Some(published) = std::path::absolute(doc.path())
            .ok()
            .and_then(|path| self.lsp.files.get(&path))
        else {
            return Vec::new();
        };
        let mut markers: Vec<Marker> = published
            .iter()
            .flat_map(|p| &p.markers)
            .map(|stored| stored.marker(doc))
            .collect();
        markers.sort_by_key(|m| (m.range.start, m.range.end, std::cmp::Reverse(m.severity)));
        markers
    }

    /// Every problem the servers reported, open documents or not, in the
    /// order of VS Code's problems list; hints are left out.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        let files = self
            .lsp
            .files
            .iter()
            .map(|(path, published)| {
                let doc = self.find_open(path).and_then(|id| self.document(id));
                let problems = published
                    .iter()
                    .flat_map(|p| &p.markers)
                    .filter(|stored| stored.severity != MarkerSeverity::Hint)
                    .map(|stored| stored.problem(path, doc))
                    .collect();
                (path.clone(), problems)
            })
            .collect();
        markers::order(files)
    }

    /// How many errors, warnings and infos there are in every file.
    #[must_use]
    pub fn problem_counts(&self) -> ProblemCounts {
        let mut counts = ProblemCounts::default();
        let all = self.lsp.files.values().flatten().flat_map(|p| &p.markers);
        for stored in all {
            match stored.severity {
                MarkerSeverity::Error => counts.errors += 1,
                MarkerSeverity::Warning => counts.warnings += 1,
                MarkerSeverity::Info => counts.infos += 1,
                MarkerSeverity::Hint => {}
            }
        }
        counts
    }

    /// Opens a problem's file, or activates its tab, and selects the
    /// problem's range, as VS Code's problems list does
    /// (`src/vs/workbench/contrib/markers/browser/markersView.ts:300-321`).
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn go_to_problem(&mut self, problem: &Problem) -> Result<DocumentId, Error> {
        let id = self.open(&problem.path)?;
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let start = doc.offset_of(problem.start);
        let end = doc.offset_of(problem.end).max(start);
        self.set_selections(id, vec![Selection::new(start, end)], 0)?;
        Ok(id)
    }

    /// Every language server started so far.
    #[must_use]
    pub fn language_servers(&self) -> Vec<ServerStatus> {
        self.lsp.servers.iter().map(Server::status).collect()
    }

    /// The server of an open document, if its language has one.
    #[must_use]
    pub fn language_server(&self, id: DocumentId) -> Option<ServerStatus> {
        let tracked = self.lsp.tracked.get(&id)?;
        self.lsp.server(tracked.server).map(Server::status)
    }

    /// What is wrong in `rustode.languageServers`, one sentence each.
    #[must_use]
    pub fn language_server_problems(&self) -> &[String] {
        &self.lsp.problems
    }

    /// Stops and starts again the server of a document, forgetting its
    /// crashes; starts it if the document has none yet. Returns whether a
    /// server starts.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn restart_language_server(&mut self, id: DocumentId) -> Result<bool, Error> {
        if self.document(id).is_none() {
            return Err(Error::UnknownDocument(id));
        }
        let Some(server) = self.lsp.tracked.get(&id).map(|t| t.server) else {
            self.lsp_attach(id);
            return Ok(self.lsp.tracked.contains_key(&id));
        };
        let configured = self
            .lsp
            .server(server)
            .is_some_and(|s| self.lsp.configs.contains_key(&s.language));
        if !configured {
            return Ok(false);
        }
        if let Some(s) = self.lsp.server_mut(server) {
            s.stop();
            s.forget_crashes();
        }
        self.lsp_forget_requests(server);
        if self.lsp.clear(server) {
            self.problems_changed();
        }
        self.lsp_start(server);
        Ok(true)
    }

    /// After a document was opened: places the markers published for its
    /// file, then attaches it to its language's server.
    pub(crate) fn lsp_opened(&mut self, id: DocumentId) {
        let Some(doc) = self.documents.get(id.0).and_then(Option::as_ref) else {
            return;
        };
        if let Some(published) = std::path::absolute(doc.path())
            .ok()
            .and_then(|path| self.lsp.files.get_mut(&path))
        {
            for stored in published.iter_mut().flat_map(|p| p.markers.iter_mut()) {
                if matches!(stored.anchor, Anchor::Lines(..)) {
                    stored.anchor = Anchor::Offsets(stored.range_in(doc));
                }
            }
        }
        self.lsp_attach(id);
    }

    /// After a document's text changed: tells its server and moves its
    /// markers.
    pub(crate) fn lsp_changed(&mut self, id: DocumentId) {
        if !self.lsp.tracked.contains_key(&id) {
            return;
        }
        let Some(doc) = self.documents.get(id.0).and_then(Option::as_ref) else {
            return;
        };
        let text = doc.text();
        let LanguageServers {
            tracked,
            servers,
            files,
            ..
        } = &mut self.lsp;
        let Some(tracked) = tracked.get_mut(&id) else {
            return;
        };
        let Some(change) = sync::diff(&tracked.baseline, &text) else {
            return;
        };
        if let Some(published) = files.get_mut(&tracked.path) {
            follow(published, &change);
        }
        if let Some(server) = servers.iter().find(|s| s.id == tracked.server)
            && server.syncs()
        {
            let changes = match server.capabilities.change {
                SyncKind::None => None,
                SyncKind::Full => Some(json!([{ "text": text }])),
                SyncKind::Incremental => Some(json!([{
                    "range": change.lsp_range(&tracked.baseline, server.capabilities.encoding),
                    "text": change.inserted,
                }])),
            };
            if let Some(changes) = changes {
                tracked.version += 1;
                server.send(&json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": {"uri": tracked.uri, "version": tracked.version},
                        "contentChanges": changes,
                    },
                }));
            }
        }
        tracked.baseline = text;
    }

    /// After a document was saved.
    pub(crate) fn lsp_saved(&self, id: DocumentId) {
        let Some(tracked) = self.lsp.tracked.get(&id) else {
            return;
        };
        let Some(server) = self.lsp.server(tracked.server).filter(|s| s.syncs()) else {
            return;
        };
        let Some(include_text) = server.capabilities.save else {
            return;
        };
        let mut params = json!({"textDocument": {"uri": tracked.uri}});
        if include_text {
            params["text"] = json!(tracked.baseline);
        }
        server.send(&json!({"jsonrpc": "2.0", "method": "textDocument/didSave", "params": params}));
    }

    /// Before a document closes: its markers keep their lines and columns,
    /// and its server is told.
    pub(crate) fn lsp_closing(&mut self, id: DocumentId) {
        if let Some(doc) = self.documents.get(id.0).and_then(Option::as_ref)
            && let Some(published) = std::path::absolute(doc.path())
                .ok()
                .and_then(|path| self.lsp.files.get_mut(&path))
        {
            for stored in published.iter_mut().flat_map(|p| p.markers.iter_mut()) {
                let (start, end) = stored.positions(Some(doc));
                stored.anchor = Anchor::Lines(start, end);
            }
        }
        self.lsp_forget_document(id);
        let Some(tracked) = self.lsp.tracked.remove(&id) else {
            return;
        };
        if let Some(server) = self.lsp.server(tracked.server).filter(|s| s.syncs()) {
            server.send(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": {"textDocument": {"uri": tracked.uri}},
            }));
        }
    }

    /// After the settings were (re)loaded: stops the servers whose entry
    /// changed or went away, restarts the changed ones, and attaches the
    /// open documents of newly configured languages.
    pub(crate) fn lsp_config_changed(&mut self) {
        let (mut configs, problems) = settings::parse(self.config.value(settings::KEY));
        if let Some(search_path) = &self.lsp.default_search_path {
            let tsdk = match self.config.value(tsserver::TSDK_KEY) {
                Some(rustode_config::Value::String(folder)) if !folder.is_empty() => {
                    Some(folder.as_str())
                }
                _ => None,
            };
            settings::add_defaults(
                &mut configs,
                self.config.value(settings::KEY),
                tsdk,
                search_path,
            );
        }
        for problem in &problems {
            tracing::warn!(%problem, "language server settings");
        }
        self.lsp.problems = problems;
        if configs != self.lsp.configs {
            let ids: Vec<ServerId> = self.lsp.servers.iter().map(|s| s.id).collect();
            for id in ids {
                self.lsp_reconfigure(id, configs.get(self.lsp_language(id)).cloned());
            }
            self.lsp.configs = configs;
        }
        let open: Vec<DocumentId> = (0..self.documents.len())
            .map(DocumentId)
            .filter(|&id| self.document(id).is_some())
            .collect();
        for id in open {
            self.lsp_attach(id);
        }
    }

    fn lsp_language(&self, id: ServerId) -> &str {
        self.lsp.server(id).map_or("", |s| s.language.as_str())
    }

    fn lsp_reconfigure(&mut self, id: ServerId, config: Option<ServerConfig>) {
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        let unchanged = config.as_ref() == Some(&server.config);
        if unchanged && server.state != ServerState::Stopped {
            return;
        }
        server.stop();
        server.forget_crashes();
        self.lsp_forget_requests(id);
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        server.message = Some(if config.is_some() {
            "restarted: its settings changed".to_owned()
        } else {
            "stopped: its settings were removed".to_owned()
        });
        let language = server.language.clone();
        let restart = config.is_some();
        if let Some(config) = config {
            server.config = config;
        }
        if self.lsp.clear(id) {
            self.problems_changed();
        }
        self.server_changed(&language, ServerState::Stopped);
        if restart {
            self.lsp_start(id);
        }
    }

    /// Attaches an open document to its language's server, starting it when
    /// its root has none yet.
    fn lsp_attach(&mut self, id: DocumentId) {
        if self.lsp.tracked.contains_key(&id) {
            return;
        }
        let Some(doc) = self.document(id) else {
            return;
        };
        let language = doc.language_id();
        let Some(config) = self.lsp.configs.get(language).cloned() else {
            return;
        };
        let Ok(path) = std::path::absolute(doc.path()) else {
            return;
        };
        let baseline = doc.text();
        let root = self.lsp_root(&path);
        let existing = self
            .lsp
            .servers
            .iter()
            .find(|s| s.language == language && s.root == root)
            .map(|s| s.id);
        let server = existing.unwrap_or_else(|| {
            let server = ServerId(self.lsp.next_server);
            self.lsp.next_server += 1;
            self.lsp
                .servers
                .push(Server::new(server, language, root, config));
            self.lsp_start(server);
            server
        });
        self.lsp.tracked.insert(
            id,
            Tracked {
                server,
                uri: uri::from_path(&path),
                path,
                version: 0,
                baseline,
            },
        );
        if self.lsp.server(server).is_some_and(Server::syncs) {
            self.lsp_send_open(id);
        }
    }

    /// The opened folder when `file` is in it, else the file's folder.
    fn lsp_root(&self, file: &Path) -> PathBuf {
        if let Some(root) = self
            .explorer
            .as_ref()
            .and_then(|e| std::path::absolute(e.root()).ok())
            && file.starts_with(&root)
        {
            return root;
        }
        file.parent()
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
    }

    fn lsp_send_open(&mut self, id: DocumentId) {
        let Some(doc) = self.documents.get(id.0).and_then(Option::as_ref) else {
            return;
        };
        let language = doc.language_id();
        let text = doc.text();
        let LanguageServers {
            tracked, servers, ..
        } = &mut self.lsp;
        let Some(tracked) = tracked.get_mut(&id) else {
            return;
        };
        tracked.version += 1;
        if let Some(server) = servers.iter().find(|s| s.id == tracked.server) {
            server.send(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {"textDocument": {
                    "uri": tracked.uri,
                    "languageId": language,
                    "version": tracked.version,
                    "text": text,
                }},
            }));
        }
        tracked.baseline = text;
    }

    fn lsp_start(&mut self, id: ServerId) {
        let waker = self.lsp.waker.clone();
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        server.start(&waker);
        let (language, state) = (server.language.clone(), server.state);
        self.server_changed(&language, state);
    }

    fn lsp_message(&mut self, id: ServerId, message: &Value) {
        if message.get("method").is_none() && self.lsp_response(id, message) {
            return;
        }
        let Some(server) = self.lsp.server(id) else {
            return;
        };
        let method = message["method"].as_str();
        let span = spans::child_message(
            &server.name(),
            Direction::Incoming,
            method.unwrap_or("response"),
        );
        let _entered = span.enter();
        tracing::trace!(%message, "from the server");
        let initialize = server.initialize;
        match (method, message.get("id")) {
            (Some(method), Some(request)) => {
                server.send(&answer(&server.root, request, method, &message["params"]));
            }
            (Some("textDocument/publishDiagnostics"), None) => {
                self.lsp_publish(id, &message["params"]);
            }
            (Some("window/logMessage" | "window/showMessage"), None) => log(server, message),
            (None, Some(request)) if initialize.is_some() && request.as_i64() == initialize => {
                self.lsp_initialized(id, message);
            }
            _ => {}
        }
    }

    fn lsp_initialized(&mut self, id: ServerId, reply: &Value) {
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        let language = server.language.clone();
        if let Some(error) = reply.get("error") {
            let why = error["message"].as_str().unwrap_or("an error").to_owned();
            server.stop();
            server.state = ServerState::Crashed;
            server.message = Some(format!("{} failed to initialize: {why}", server.name()));
            self.server_changed(&language, ServerState::Crashed);
            return;
        }
        server.initialized(&reply["result"]);
        let syncs = server.syncs();
        self.server_changed(&language, ServerState::Running);
        if syncs {
            let documents: Vec<DocumentId> = self
                .lsp
                .tracked
                .iter()
                .filter(|(_, t)| t.server == id)
                .map(|(&doc, _)| doc)
                .collect();
            for doc in documents {
                self.lsp_send_open(doc);
            }
        }
    }

    fn lsp_publish(&mut self, server: ServerId, params: &Value) {
        let Some(path) = params["uri"].as_str().and_then(uri::to_path) else {
            return;
        };
        let Some(encoding) = self.lsp.server(server).map(|s| s.capabilities.encoding) else {
            return;
        };
        let doc = self.find_open(&path).and_then(|id| self.document(id));
        let markers: Vec<Stored> = params["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| Stored::from_lsp(d, |start, end| anchor(doc, start, end, encoding)))
            .collect();
        let published = self.lsp.files.entry(path.clone()).or_default();
        published.retain(|p| p.server != server);
        if !markers.is_empty() {
            published.push(Published { server, markers });
        }
        if published.is_empty() {
            self.lsp.files.remove(&path);
        }
        self.problems_changed();
    }

    fn lsp_closed(&mut self, id: ServerId) {
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        let Some(process) = server.process.take() else {
            return;
        };
        let how = process.reap();
        tracing::warn!(server = %server.name(), %how, "language server crashed");
        self.lsp_forget_requests(id);
        let Some(server) = self.lsp.server_mut(id) else {
            return;
        };
        let restart = server.crashed(&how, Instant::now());
        let language = server.language.clone();
        if self.lsp.clear(id) {
            self.problems_changed();
        }
        self.server_changed(&language, ServerState::Crashed);
        if restart {
            if let Some(server) = self.lsp.server_mut(id) {
                server.restarts += 1;
            }
            self.lsp_start(id);
        }
    }

    fn server_changed(&mut self, language: &str, state: ServerState) {
        self.events.push(Event::LanguageServerChanged {
            language: language.to_owned(),
            state,
        });
    }

    fn problems_changed(&mut self) {
        if self.events.last() != Some(&Event::ProblemsChanged) {
            self.events.push(Event::ProblemsChanged);
        }
    }
}

/// Logs a message the server shows or logs.
fn log(server: &Server, message: &Value) {
    let text = message["params"]["message"].as_str().unwrap_or_default();
    tracing::info!(server = %server.name(), "{text}");
}

/// Moves offset-anchored markers through `change`.
fn follow(published: &mut [Published], change: &sync::Change) {
    let edit = Range::new(change.start, change.old_end);
    let inserted = change.inserted.chars().count();
    for stored in published.iter_mut().flat_map(|p| p.markers.iter_mut()) {
        if let Anchor::Offsets(range) = &mut stored.anchor {
            let start = crate::find::follow(range.start, false, edit, inserted);
            let end = crate::find::follow(range.end, true, edit, inserted).max(start);
            *range = Range::new(start, end);
        }
    }
}

/// Where an LSP range is: offsets in an open document, else its lines.
fn anchor(
    doc: Option<&Document>,
    start: &Value,
    end: &Value,
    encoding: PositionEncoding,
) -> Anchor {
    let number = |value: &Value| usize::try_from(value.as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    match doc {
        Some(doc) => {
            let start = offset(doc, start, encoding);
            Anchor::Offsets(Range::new(start, offset(doc, end, encoding).max(start)))
        }
        None => Anchor::Lines(
            Position::new(number(&start["line"]), number(&start["character"])),
            Position::new(number(&end["line"]), number(&end["character"])),
        ),
    }
}

/// The char offset of an LSP position in `doc`.
fn offset(doc: &Document, position: &Value, encoding: PositionEncoding) -> usize {
    offset_at(
        doc,
        position["line"].as_u64().unwrap_or(0),
        position["character"].as_u64().unwrap_or(0),
        encoding,
    )
}

/// The char offset of line `line`, `units` code units in, of `doc`: a line
/// past the end is the end of the document, a column past the end of its
/// line is the end of the line.
fn offset_at(doc: &Document, line: u64, units: u64, encoding: PositionEncoding) -> usize {
    let line = usize::try_from(line).unwrap_or(usize::MAX);
    if line >= doc.line_count() {
        return doc.len_chars();
    }
    let column = encoding.column(&doc.line(line), units);
    doc.offset_of(Position::new(line, column))
}

/// The answer to a request from the server: the few a client must answer,
/// and "not supported" for the rest.
fn answer(root: &Path, id: &Value, method: &str, params: &Value) -> Value {
    let result = match method {
        "workspace/configuration" => {
            let items = params["items"].as_array().map_or(0, Vec::len);
            Value::Array(vec![Value::Null; items])
        }
        "workspace/workspaceFolders" => {
            let name = root
                .file_name()
                .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
            json!([{"uri": uri::from_path(root), "name": name}])
        }
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "window/showMessageRequest" => Value::Null,
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("{method} is not supported")},
            });
        }
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_requests_get_an_answer() {
        let root = Path::new("/tmp/project");
        let configuration = answer(
            root,
            &json!(1),
            "workspace/configuration",
            &json!({"items": [{}, {}]}),
        );
        assert_eq!(configuration["result"], json!([null, null]));
        let folders = answer(root, &json!(2), "workspace/workspaceFolders", &Value::Null);
        assert_eq!(folders["result"][0]["name"], "project");
        let unknown = answer(root, &json!(3), "x/y", &Value::Null);
        assert_eq!(unknown["error"]["code"], -32601);
        let register = answer(root, &json!(4), "client/registerCapability", &Value::Null);
        assert_eq!(register["result"], Value::Null);
    }
}
