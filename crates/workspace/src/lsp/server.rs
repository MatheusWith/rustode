//! One supervised language server: its configuration, its process when it
//! runs, what it said it can do, and its restart history.
//!
//! Restart policy: a crashed server is restarted at once, up to four times
//! within three minutes; at the fifth crash in that window it stays crashed
//! until it is restarted by hand. This is a design choice modeled on what
//! `vscode-languageclient` is known to do; that module is not in the source
//! tree, so the numbers are not a verified VS Code fact.

use super::encoding::{self, PositionEncoding};
use super::settings::ServerConfig;
use super::uri;
use crate::child::{Process, Waker};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Automatic restarts allowed within [`RESTART_WINDOW`].
const MAX_RESTARTS: usize = 4;
/// The window crashes are counted in.
const RESTART_WINDOW: Duration = Duration::from_secs(180);

/// Identifies a server for the life of the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ServerId(pub u64);

/// What a language server is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerState {
    /// Started, not initialized yet.
    Starting,
    /// Initialized: it receives the documents and sends diagnostics.
    Running,
    /// It exited without being asked, or could not start.
    Crashed,
    /// Stopped on purpose, e.g. when its settings were removed.
    Stopped,
}

impl ServerState {
    /// The lower-case word the status bar shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Crashed => "crashed",
            Self::Stopped => "stopped",
        }
    }
}

/// A language server as the status bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    /// The language id it serves, e.g. `rust`.
    pub language: String,
    /// The folder it was started for.
    pub root: PathBuf,
    /// Its command's file name, e.g. `rust-analyzer`.
    pub name: String,
    /// What it is doing.
    pub state: ServerState,
    /// Automatic restarts since it was last started by hand.
    pub restarts: usize,
    /// Why it crashed or stopped.
    pub message: Option<String>,
}

/// How a server wants document changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SyncKind {
    /// Not at all.
    #[default]
    None,
    /// The whole text each time.
    Full,
    /// The changed range.
    Incremental,
}

/// What a server said it can do, in its `initialize` result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Capabilities {
    pub encoding: PositionEncoding,
    pub open_close: bool,
    pub change: SyncKind,
    /// `Some(include_text)` when it wants `didSave`.
    pub save: Option<bool>,
}

impl Capabilities {
    /// Reads `textDocumentSync` and `positionEncoding`. A bare sync kind
    /// means open, close and save notifications too, as LSP clients
    /// commonly read it.
    pub(crate) fn from_lsp(capabilities: &Value) -> Self {
        let sync = &capabilities["textDocumentSync"];
        let kind = |value: &Value| match value.as_u64() {
            Some(1) => SyncKind::Full,
            Some(2) => SyncKind::Incremental,
            _ => SyncKind::None,
        };
        let encoding = PositionEncoding::from_name(capabilities["positionEncoding"].as_str());
        if sync.is_number() {
            return Self {
                encoding,
                open_close: true,
                change: kind(sync),
                save: Some(false),
            };
        }
        let save = match &sync["save"] {
            Value::Bool(true) => Some(false),
            Value::Object(options) => Some(options.get("includeText") == Some(&Value::Bool(true))),
            _ => None,
        };
        Self {
            encoding,
            open_close: sync["openClose"] == Value::Bool(true),
            change: kind(&sync["change"]),
            save,
        }
    }
}

/// The language features a server offers beyond syncing documents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Features {
    /// `hoverProvider`.
    pub hover: bool,
    /// `completionProvider`: its trigger characters, `None` without one.
    pub completion: Option<Vec<char>>,
    /// `definitionProvider`.
    pub definition: bool,
    /// `referencesProvider`.
    pub references: bool,
    /// `renameProvider`.
    pub rename: bool,
    /// `documentFormattingProvider`.
    pub formatting: bool,
}

/// A provider capability: a flag or an options object.
fn provider(value: &Value) -> bool {
    match value {
        Value::Bool(on) => *on,
        Value::Object(_) => true,
        _ => false,
    }
}

impl Features {
    /// Reads the providers (each a flag or an options object) and
    /// `completionProvider`'s `triggerCharacters`.
    pub(crate) fn from_lsp(capabilities: &Value) -> Self {
        let hover = provider(&capabilities["hoverProvider"]);
        let completion = capabilities["completionProvider"]
            .as_object()
            .map(|options| {
                options
                    .get("triggerCharacters")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(|s| {
                        let mut chars = s.chars();
                        chars.next().filter(|_| chars.next().is_none())
                    })
                    .collect()
            });
        Self {
            hover,
            completion,
            definition: provider(&capabilities["definitionProvider"]),
            references: provider(&capabilities["referencesProvider"]),
            rename: provider(&capabilities["renameProvider"]),
            formatting: provider(&capabilities["documentFormattingProvider"]),
        }
    }
}

/// One supervised server.
#[derive(Debug)]
pub(crate) struct Server {
    pub id: ServerId,
    pub language: String,
    pub root: PathBuf,
    pub config: ServerConfig,
    pub state: ServerState,
    pub message: Option<String>,
    pub restarts: usize,
    pub process: Option<Process>,
    pub capabilities: Capabilities,
    pub features: Features,
    /// The id of the `initialize` request in flight.
    pub initialize: Option<i64>,
    crashes: VecDeque<Instant>,
    next_request: i64,
}

impl Server {
    pub(crate) fn new(id: ServerId, language: &str, root: PathBuf, config: ServerConfig) -> Self {
        Self {
            id,
            language: language.to_owned(),
            root,
            config,
            state: ServerState::Stopped,
            message: None,
            restarts: 0,
            process: None,
            capabilities: Capabilities::default(),
            features: Features::default(),
            initialize: None,
            crashes: VecDeque::new(),
            next_request: 1,
        }
    }

    /// The command's file name.
    pub(crate) fn name(&self) -> String {
        Path::new(&self.config.command).file_name().map_or_else(
            || self.config.command.clone(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub(crate) fn status(&self) -> ServerStatus {
        ServerStatus {
            language: self.language.clone(),
            root: self.root.clone(),
            name: self.name(),
            state: self.state,
            restarts: self.restarts,
            message: self.message.clone(),
        }
    }

    pub(crate) fn request_id(&mut self) -> i64 {
        let id = self.next_request;
        self.next_request += 1;
        id
    }

    /// Starts the process and sends `initialize`: [`ServerState::Starting`],
    /// or [`ServerState::Crashed`] when it cannot start.
    pub(crate) fn start(&mut self, waker: &Waker) {
        self.capabilities = Capabilities::default();
        self.features = Features::default();
        match Process::spawn(
            &self.config.command,
            &self.config.args,
            &self.root,
            &self.name(),
            waker,
        ) {
            Ok(process) => {
                let id = self.request_id();
                process.send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": self.initialize_params(),
                }));
                self.process = Some(process);
                self.initialize = Some(id);
                self.state = ServerState::Starting;
            }
            Err(err) => {
                self.state = ServerState::Crashed;
                self.message = Some(format!("cannot start {}: {err}", self.config.command));
            }
        }
    }

    fn initialize_params(&self) -> Value {
        let root = uri::from_path(&self.root);
        let folder = self
            .root
            .file_name()
            .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
        json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "rustode", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root,
            "rootPath": self.root.display().to_string(),
            "workspaceFolders": [{"uri": root, "name": folder}],
            "initializationOptions": self.config.initialization_options,
            "capabilities": {
                "general": {"positionEncodings": encoding::OFFERED},
                "textDocument": {
                    "synchronization": {"dynamicRegistration": false, "didSave": true},
                    "publishDiagnostics": {"relatedInformation": false},
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["markdown", "plaintext"],
                    },
                    "definition": {"dynamicRegistration": false, "linkSupport": true},
                    "references": {"dynamicRegistration": false},
                    "rename": {"dynamicRegistration": false, "prepareSupport": false},
                    "formatting": {"dynamicRegistration": false},
                    "completion": {
                        "dynamicRegistration": false,
                        "contextSupport": true,
                        "completionItem": {
                            "snippetSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "insertReplaceSupport": false,
                            "commitCharactersSupport": false,
                        },
                    },
                },
                "workspace": {"workspaceFolders": true, "configuration": true},
            },
        })
    }

    /// The `initialize` result arrived: running, with these capabilities.
    pub(crate) fn initialized(&mut self, result: &Value) {
        self.capabilities = Capabilities::from_lsp(&result["capabilities"]);
        self.features = Features::from_lsp(&result["capabilities"]);
        self.initialize = None;
        self.state = ServerState::Running;
        self.message = None;
        self.send(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
    }

    /// Sends a request and returns its id; `None` when the process is not
    /// running.
    pub(crate) fn request(&mut self, method: &str, params: &Value) -> Option<i64> {
        self.process.as_ref()?;
        let id = self.request_id();
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        Some(id)
    }

    /// Asks the server to drop request `id` (`$/cancelRequest`).
    pub(crate) fn cancel(&self, id: i64) {
        self.send(&json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": id}}));
    }

    /// Sends a message if the process runs.
    pub(crate) fn send(&self, message: &Value) {
        if let Some(process) = &self.process {
            process.send(message);
        }
    }

    /// Whether documents are synced with it now.
    pub(crate) fn syncs(&self) -> bool {
        self.state == ServerState::Running && self.capabilities.open_close
    }

    /// Records a crash; returns whether to restart it.
    pub(crate) fn crashed(&mut self, how: &str, now: Instant) -> bool {
        self.state = ServerState::Crashed;
        self.initialize = None;
        self.crashes.push_back(now);
        while self
            .crashes
            .front()
            .is_some_and(|&at| now.duration_since(at) > RESTART_WINDOW)
        {
            self.crashes.pop_front();
        }
        let name = self.name();
        if self.crashes.len() > MAX_RESTARTS {
            self.message = Some(format!(
                "{name} {how}; it crashed {} times in the last 3 minutes and will not be restarted",
                self.crashes.len()
            ));
            false
        } else {
            self.message = Some(format!("{name} {how}; restarting"));
            true
        }
    }

    /// Stops the process, politely; the history is kept.
    pub(crate) fn stop(&mut self) {
        if let Some(process) = self.process.take() {
            let id = self.request_id();
            process.shutdown(id);
        }
        self.state = ServerState::Stopped;
        self.initialize = None;
    }

    /// Forgets the crashes, as a start by hand does.
    pub(crate) fn forget_crashes(&mut self) {
        self.crashes.clear();
        self.restarts = 0;
        self.message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_capabilities_in_both_forms() {
        let bare = Capabilities::from_lsp(&json!({"textDocumentSync": 1}));
        assert_eq!(
            (bare.open_close, bare.change, bare.save),
            (true, SyncKind::Full, Some(false))
        );
        let options = Capabilities::from_lsp(&json!({
            "textDocumentSync": {"openClose": true, "change": 2, "save": {"includeText": true}},
            "positionEncoding": "utf-8",
        }));
        assert_eq!(
            (options.change, options.save, options.encoding),
            (SyncKind::Incremental, Some(true), PositionEncoding::Utf8)
        );
        let none = Capabilities::from_lsp(&json!({}));
        assert_eq!(
            (none.open_close, none.change, none.save),
            (false, SyncKind::None, None)
        );
        let save = Capabilities::from_lsp(&json!({"textDocumentSync": {"save": true}}));
        assert_eq!(save.save, Some(false));
    }

    #[test]
    fn features_read_hover_and_completion() {
        let none = Features::from_lsp(&json!({}));
        assert_eq!((none.hover, none.completion), (false, None));
        let all = Features::from_lsp(&json!({
            "hoverProvider": {"workDoneProgress": true},
            "completionProvider": {"triggerCharacters": [".", "::", ":"]},
        }));
        assert!(all.hover);
        assert_eq!(all.completion, Some(vec!['.', ':']), "only single chars");
        let off = Features::from_lsp(&json!({"hoverProvider": false, "completionProvider": {}}));
        assert_eq!((off.hover, off.completion), (false, Some(vec![])));
    }

    #[test]
    fn old_crashes_fall_out_of_the_window() {
        let config = ServerConfig {
            command: "/usr/bin/x".into(),
            args: vec![],
            initialization_options: None,
        };
        let mut server = Server::new(ServerId(1), "rust", PathBuf::from("/"), config);
        let start = Instant::now();
        for _ in 0..4 {
            assert!(server.crashed("exited", start));
        }
        let later = start + RESTART_WINDOW + Duration::from_secs(1);
        assert!(server.crashed("exited", later), "the first four are old");
        assert_eq!(server.name(), "x");
        assert_eq!(ServerState::Crashed.label(), "crashed");
    }
}
