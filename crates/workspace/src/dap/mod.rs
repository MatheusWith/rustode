//! The debug adapter client, the tracer of milestone M7: a launch
//! configuration from `launch.json` starts a program under a debug adapter,
//! which stops at the breakpoints set from the gutter and continues
//! ([ADR 0030](../../../../docs/adr/0030-dap-client.md)).
//!
//! The adapter is a child process over stdio, supervised with the pieces the
//! language servers use ([`crate::child`], [ADR 0015](../../../../docs/adr/0015-child-process-servers.md)):
//! messages arrive on a reader thread and wait in a channel until a frontend
//! calls [`Workspace::poll_debug`]; the waker set with
//! [`Workspace::set_waker`] says when. Every message is traced in a
//! `child_message` span.
//!
//! The session follows VS Code's order (`src/vs/workbench/contrib/debug/browser/debugSession.ts:395-406`
//! and `:1065-1090`): `initialize`, then `launch`; when the adapter sends
//! `initialized` the client sends the breakpoints of every file with
//! `setBreakpoints`, then `configurationDone`. The adapter's `stopped` event
//! is answered with a `stackTrace` request for the stop's location;
//! `continue` resumes it. Stopping asks for `terminate` when the adapter can
//! and the request is `launch`, else `disconnect` with `terminateDebuggee`
//! (`debugSession.ts:420-445`). A crashed adapter ends the session with a
//! message and is not restarted: restarting would run the program again.
//!
//! Everything the adapter sends is untrusted: fields are read leniently,
//! numbers are checked, unknown messages are ignored, and the adapter's own
//! requests are refused politely.
//!
//! Stepping, the call stack, variables, watch expressions and the console
//! (ticket 38, [`model`] and [`views`]) follow `debugModel.ts` and the views
//! of `src/vs/workbench/contrib/debug/browser/`: when the program stops the
//! client asks for `threads` and `stackTrace`, then for the `scopes` of the
//! selected frame, the variables of the first cheap scope and the watch
//! expressions (`evaluate`, context `watch`); deeper variables are asked for
//! when expanded. Whatever execution moves past is dropped: continuing, a
//! step or another frame forgets the state and the requests waiting for it
//! (VS Code does the same on `stopped` and `continued`, `debugSession.ts:964-990`, `:1107-1165`), so
//! a late answer is never applied.
//!
//! Conditions, hit counts and logpoints (ticket 39, [`breakpoints`]) are
//! sent with `setBreakpoints` when the adapter's capabilities allow;
//! otherwise the breakpoint stays a plain one and shows as unverified with
//! the reason. What the adapter says about each one (`verified`, `message`,
//! `id`) comes from the response and from `breakpoint` events. `attach`
//! configurations send `attach` and are never terminated, only disconnected.
//!
//! Not done yet: compound configurations, other variables than
//! `${workspaceFolder}`, breakpoints that follow edits or survive a restart,
//! the line an adapter moves a breakpoint to, `runInTerminal` and
//! `startDebugging` (refused with a reason).

mod breakpoints;
mod launch;
mod model;
mod session;
mod settings;
mod views;

pub use breakpoints::{
    BreakpointDraft, BreakpointInfo, BreakpointInput, BreakpointKind, BreakpointSpec,
    MAX_BREAKPOINT_TEXT,
};
pub use launch::LaunchConfiguration;
pub use model::{ConsoleKind, ConsoleLine, DebugTree, FrameInfo, ThreadInfo, VariableRow};

use crate::child::{Process, Waker};
use crate::{DocumentId, Error, Event, Workspace};
use breakpoints::Breakpoint;
use launch::Launch;
use model::Node;
use serde_json::json;
use session::Session;
use settings::AdapterConfig;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// What a debug session is doing, in VS Code's terms
/// (`src/vs/workbench/contrib/debug/common/debug.ts:217-222`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugState {
    /// No session.
    Inactive,
    /// The adapter started and is being configured.
    Initializing,
    /// The program runs.
    Running,
    /// The program is stopped, at a breakpoint for example.
    Stopped,
}

impl DebugState {
    /// The lower-case word the status bar shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Initializing => "initializing",
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }
}

/// Where the program is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopLocation {
    /// The source file, absolute.
    pub path: PathBuf,
    /// The zero-based line.
    pub line: usize,
}

/// The debug session as the frontends show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugStatus {
    /// What the session is doing.
    pub state: DebugState,
    /// The launch configuration's name, while there is a session.
    pub configuration: Option<String>,
    /// Why the program stopped (`breakpoint`, `step`, ...), while stopped.
    pub reason: Option<String>,
    /// Where it stopped, once the adapter said so.
    pub location: Option<StopLocation>,
    /// The id of the thread that stopped, while stopped.
    pub thread: Option<i64>,
    /// Why the last session ended or what went wrong, e.g. `the debug
    /// adapter exited with code 3`.
    pub message: Option<String>,
}

/// Why a debug command was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DebugError {
    /// No folder is open, so there is no `launch.json`.
    NoFolder,
    /// `launch.json` has no usable configuration.
    NoConfiguration,
    /// A session is already running.
    AlreadyRunning,
    /// There is no session.
    NotRunning,
    /// The program is not stopped.
    NotStopped,
    /// The program is not running, so it cannot be paused.
    NotRunningProgram,
    /// There is no such frame, variable or watch expression.
    NoSuchRow,
    /// The adapter has too many requests waiting for an answer.
    Busy,
    /// A watch expression that is empty, or one too many.
    BadWatch(String),
    /// A breakpoint text that is not acceptable.
    BadBreakpoint(String),
    /// There is no breakpoint on that line to edit.
    NoBreakpoint,
    /// No debug adapter is configured for this configuration type, in
    /// `rustode.debugAdapters`.
    NoAdapter(String),
    /// Only `launch` and `attach` requests exist.
    UnsupportedRequest(String),
    /// The adapter could not be started.
    Spawn(String),
}

impl fmt::Display for DebugError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFolder => f.write_str("open a folder with a .vscode/launch.json to debug"),
            Self::NoConfiguration => f.write_str("launch.json has no launch configuration"),
            Self::AlreadyRunning => f.write_str("a debug session is already running"),
            Self::NotRunning => f.write_str("there is no debug session"),
            Self::NotStopped => f.write_str("the program is not stopped"),
            Self::NotRunningProgram => f.write_str("the program is not running"),
            Self::NoSuchRow => f.write_str("there is nothing there"),
            Self::Busy => f.write_str("the debug adapter has too many requests waiting"),
            Self::BadWatch(why) | Self::BadBreakpoint(why) => f.write_str(why),
            Self::NoBreakpoint => f.write_str("there is no breakpoint on this line"),
            Self::NoAdapter(kind) => write!(
                f,
                "no debug adapter for type \"{kind}\"; set rustode.debugAdapters[\"{kind}\"]"
            ),
            Self::UnsupportedRequest(request) => {
                write!(f, "\"{request}\" requests are not supported yet")
            }
            Self::Spawn(why) => write!(f, "cannot start the debug adapter: {why}"),
        }
    }
}

impl std::error::Error for DebugError {}

/// VS Code's id for "Start Debugging" (F5).
pub const START_COMMAND: &str = "workbench.action.debug.start";
/// VS Code's id for "Continue" (F5 while stopped).
pub const CONTINUE_COMMAND: &str = "workbench.action.debug.continue";

/// VS Code's id for "Step Over" (F10).
pub const STEP_OVER_COMMAND: &str = "workbench.action.debug.stepOver";
/// VS Code's id for "Step Into" (F11).
pub const STEP_INTO_COMMAND: &str = "workbench.action.debug.stepInto";
/// VS Code's id for "Step Out" (Shift+F11).
pub const STEP_OUT_COMMAND: &str = "workbench.action.debug.stepOut";
/// VS Code's id for "Pause" (F6 while the program runs).
pub const PAUSE_COMMAND: &str = "workbench.action.debug.pause";

/// How far a step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// To the next line of this frame (`next`).
    Over,
    /// Into the call on the line (`stepIn`).
    Into,
    /// Out of this frame (`stepOut`).
    Out,
}

/// The debugging state of a workspace.
#[derive(Debug, Default)]
pub(crate) struct Debug {
    adapters: BTreeMap<String, AdapterConfig>,
    problems: Vec<String>,
    launch: Launch,
    selected: Option<String>,
    /// Breakpoints by absolute path and zero-based line.
    breakpoints: BTreeMap<PathBuf, BTreeMap<usize, Breakpoint>>,
    session: Option<Session>,
    /// What the last session said when it ended.
    message: Option<String>,
    waker: Waker,
    /// The watch expressions, which outlive sessions.
    watches: Vec<Node>,
    next_watch: u64,
    /// The debug console, cleared when a session starts.
    console: Vec<ConsoleLine>,
}

impl Debug {
    pub(crate) fn waker(&self) -> &Waker {
        &self.waker
    }
}

impl Drop for Debug {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            session.close_now();
        }
    }
}

impl Workspace {
    /// The configurations of `.vscode/launch.json` in the opened folder.
    #[must_use]
    pub fn launch_configurations(&self) -> &[LaunchConfiguration] {
        &self.dap.launch.configurations
    }

    /// What is wrong in `launch.json` and in `rustode.debugAdapters`, one
    /// sentence each.
    #[must_use]
    pub fn debug_problems(&self) -> Vec<String> {
        let mut problems: Vec<String> = self
            .dap
            .launch
            .problems
            .iter()
            .map(ToString::to_string)
            .collect();
        problems.extend(self.dap.problems.iter().cloned());
        problems
    }

    /// Reads `launch.json` again; a frontend calls it after the file was
    /// saved. Opening a folder and starting a session read it too.
    pub fn reload_launch_configurations(&mut self) {
        let path = self
            .explorer
            .as_ref()
            .map(|e| e.root().join(".vscode").join("launch.json"));
        self.dap.launch = path.map_or_else(Launch::default, |p| launch::load(&p));
        for problem in &self.dap.launch.problems {
            tracing::warn!(%problem, "launch.json");
        }
        if self.dap.selected.as_ref().is_some_and(|name| {
            !self
                .dap
                .launch
                .configurations
                .iter()
                .any(|c| &c.name == name)
        }) {
            self.dap.selected = None;
        }
    }

    /// Picks the configuration F5 starts; returns whether there is one by
    /// that name. The first one is used until one is picked.
    pub fn select_launch_configuration(&mut self, name: &str) -> bool {
        let exists = self
            .dap
            .launch
            .configurations
            .iter()
            .any(|c| c.name == name);
        if exists {
            self.dap.selected = Some(name.to_owned());
        }
        exists
    }

    /// The configuration F5 would start.
    #[must_use]
    pub fn selected_launch_configuration(&self) -> Option<&LaunchConfiguration> {
        let configurations = &self.dap.launch.configurations;
        self.dap
            .selected
            .as_ref()
            .and_then(|name| configurations.iter().find(|c| &c.name == name))
            .or_else(|| configurations.first())
    }

    /// The session as the frontends show it.
    #[must_use]
    pub fn debug_status(&self) -> DebugStatus {
        match &self.dap.session {
            Some(session) => DebugStatus {
                state: session.state,
                configuration: Some(session.name.clone()),
                reason: session.reason.clone(),
                location: session.location.clone(),
                thread: session
                    .thread
                    .filter(|_| session.state == DebugState::Stopped),
                message: self.dap.message.clone(),
            },
            None => DebugStatus {
                state: DebugState::Inactive,
                configuration: None,
                reason: None,
                location: None,
                thread: None,
                message: self.dap.message.clone(),
            },
        }
    }

    /// The command F5 runs now: "Continue" while the program is stopped,
    /// else "Start Debugging", as VS Code's `when` clauses have it
    /// (`src/vs/workbench/contrib/debug/browser/debugCommands.ts:764-770`,
    /// `:1166-1170`).
    #[must_use]
    pub fn debug_key_command(&self) -> &'static str {
        match self.dap.session.as_ref().map(|s| s.state) {
            Some(DebugState::Stopped) => CONTINUE_COMMAND,
            _ => START_COMMAND,
        }
    }

    /// Starts the selected launch configuration under its debug adapter.
    /// The session then moves on as the adapter's messages are
    /// [polled](Self::poll_debug).
    ///
    /// # Errors
    /// When there is no folder, configuration or adapter, a session is
    /// running, or the adapter cannot start.
    pub fn start_debugging(&mut self) -> Result<(), DebugError> {
        if self.dap.session.is_some() {
            return Err(DebugError::AlreadyRunning);
        }
        self.reload_launch_configurations();
        let root = self
            .explorer
            .as_ref()
            .map(|e| e.root().to_owned())
            .ok_or(DebugError::NoFolder)?;
        let configuration = self
            .selected_launch_configuration()
            .cloned()
            .ok_or(DebugError::NoConfiguration)?;
        if !matches!(configuration.request.as_str(), "launch" | "attach") {
            return Err(DebugError::UnsupportedRequest(configuration.request));
        }
        let adapter = self
            .dap
            .adapters
            .get(&configuration.adapter_type)
            .ok_or_else(|| DebugError::NoAdapter(configuration.adapter_type.clone()))?;
        let process = Process::spawn(
            &adapter.command,
            &adapter.args,
            &root,
            &adapter.env,
            &configuration.adapter_type,
            &self.dap.waker,
        )
        .map_err(|err| DebugError::Spawn(err.to_string()))?;
        let arguments = launch::substitute(&configuration.body, &root);
        let mut session = Session::new(&configuration, process, arguments, &root);
        for file in self.dap.breakpoints.values_mut() {
            file.values_mut().for_each(Breakpoint::reset);
        }
        session.request(
            "initialize",
            &json!({
                "clientID": "rustode",
                "clientName": "rustode",
                "adapterID": configuration.adapter_type,
                "pathFormat": "path",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "supportsRunInTerminalRequest": false,
                "supportsStartDebuggingRequest": false,
                "supportsVariableType": true,
            }),
            session::Pending::Initialize,
        );
        self.dap.message = None;
        self.dap.console.clear();
        for watch in &mut self.dap.watches {
            watch.reset();
        }
        self.dap.session = Some(session);
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// Resumes the stopped program.
    ///
    /// # Errors
    /// When there is no session or the program is not stopped.
    pub fn continue_debugging(&mut self) -> Result<(), DebugError> {
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        if !session.can_resume() {
            return Err(DebugError::NotStopped);
        }
        session.resume();
        self.events.push(Event::DebugChanged);
        Ok(())
    }

    /// Stops the session: asks the adapter to end the program. Asked again
    /// while it has not obeyed, the session is dropped at once.
    ///
    /// # Errors
    /// When there is no session.
    pub fn stop_debugging(&mut self) -> Result<(), DebugError> {
        let session = self.dap.session.as_mut().ok_or(DebugError::NotRunning)?;
        if session.stop() {
            self.dap_end(None, false);
        }
        Ok(())
    }

    /// Sets or clears a breakpoint on a zero-based line of an open
    /// document, as a click on the gutter or F9 does
    /// (`src/vs/workbench/contrib/debug/browser/debugEditorActions.ts:35-60`).
    /// A line that has one, whatever its kind, loses it. Returns whether
    /// there is a breakpoint now. While a session runs the file's
    /// breakpoints are sent again.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn toggle_breakpoint(&mut self, id: DocumentId, line: usize) -> Result<bool, Error> {
        let path = self.breakpoint_path(id)?;
        let lines = self.dap.breakpoints.entry(path.clone()).or_default();
        let set = if lines.remove(&line).is_some() {
            false
        } else {
            lines.insert(line, Breakpoint::default());
            true
        };
        self.dap_breakpoints_changed(&path);
        Ok(set)
    }

    /// Sets the breakpoint on a zero-based line with a condition, a hit
    /// count condition and a log message, replacing what was there; texts
    /// that are all empty make a plain breakpoint (`breakpointWidget.ts:490-505`).
    ///
    /// # Errors
    /// When there is no such document, or a text has a line break.
    pub fn set_breakpoint(
        &mut self,
        id: DocumentId,
        line: usize,
        spec: BreakpointSpec,
    ) -> Result<(), Error> {
        let spec = spec.clean().map_err(Error::Debug)?;
        let path = self.breakpoint_path(id)?;
        self.dap
            .breakpoints
            .entry(path.clone())
            .or_default()
            .insert(line, Breakpoint::new(spec));
        self.dap_breakpoints_changed(&path);
        Ok(())
    }

    /// Opens the breakpoint editor on a line: the three texts of the
    /// breakpoint there (empty if there is none), showing `input`, or the
    /// one the breakpoint uses when `None` (`breakpointWidget.ts:160-200`).
    ///
    /// # Errors
    /// When there is no such document, or `input` is `None` and the line has
    /// no breakpoint to edit.
    pub fn begin_breakpoint_edit(
        &mut self,
        id: DocumentId,
        line: usize,
        input: Option<BreakpointInput>,
    ) -> Result<BreakpointDraft, Error> {
        let path = self.breakpoint_path(id)?;
        let existing = self
            .dap
            .breakpoints
            .get(&path)
            .and_then(|lines| lines.get(&line))
            .map(|bp| bp.spec.clone());
        let input = match (input, &existing) {
            (Some(input), _) => input,
            (None, None) => return Err(Error::Debug(DebugError::NoBreakpoint)),
            (None, Some(spec)) if spec.log_message.is_some() => BreakpointInput::LogMessage,
            (None, Some(spec)) if spec.hit_condition.is_some() && spec.condition.is_none() => {
                BreakpointInput::HitCount
            }
            (None, Some(_)) => BreakpointInput::Expression,
        };
        Ok(BreakpointDraft::new(
            id,
            line,
            input,
            &existing.unwrap_or_default(),
        ))
    }

    /// Applies an edited draft: [`Workspace::set_breakpoint`] with its texts.
    ///
    /// # Errors
    /// As [`Workspace::set_breakpoint`].
    pub fn finish_breakpoint_edit(&mut self, draft: &BreakpointDraft) -> Result<(), Error> {
        self.set_breakpoint(draft.document, draft.line, draft.spec())
    }

    fn breakpoint_path(&self, id: DocumentId) -> Result<PathBuf, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        std::path::absolute(doc.path()).map_err(|source| Error::Io {
            path: doc.path().to_owned(),
            source,
        })
    }

    /// The breakpoints of a file changed: forget it if none is left, send
    /// them again to the session and tell the frontends.
    fn dap_breakpoints_changed(&mut self, path: &Path) {
        if self
            .dap
            .breakpoints
            .get(path)
            .is_some_and(BTreeMap::is_empty)
        {
            self.dap.breakpoints.remove(path);
        }
        if let Some(session) = self.dap.session.as_mut() {
            session.send_breakpoints(path, self.dap.breakpoints.get(path));
        }
        self.events.push(Event::DebugChanged);
    }

    /// The zero-based lines of an open document that have a breakpoint.
    #[must_use]
    pub fn breakpoints(&self, id: DocumentId) -> Vec<usize> {
        self.document(id)
            .and_then(|doc| std::path::absolute(doc.path()).ok())
            .and_then(|path| self.dap.breakpoints.get(&path))
            .map_or_else(Vec::new, |lines| lines.keys().copied().collect())
    }

    /// The breakpoints of an open document with their kind and whether the
    /// adapter can stop on them, by line.
    #[must_use]
    pub fn breakpoint_details(&self, id: DocumentId) -> Vec<BreakpointInfo> {
        let caps = self.dap.session.as_ref().and_then(Session::capabilities);
        self.document(id)
            .and_then(|doc| std::path::absolute(doc.path()).ok())
            .and_then(|path| self.dap.breakpoints.get(&path))
            .map_or_else(Vec::new, |lines| {
                lines
                    .iter()
                    .map(|(line, bp)| bp.info(*line, caps.as_ref()))
                    .collect()
            })
    }

    /// The breakpoint on a line of an open document.
    #[must_use]
    pub fn breakpoint_at(&self, id: DocumentId, line: usize) -> Option<BreakpointInfo> {
        self.breakpoint_details(id)
            .into_iter()
            .find(|bp| bp.line == line)
    }

    /// The zero-based line the program is stopped on, if it is stopped in
    /// this document.
    #[must_use]
    pub fn stopped_line(&self, id: DocumentId) -> Option<usize> {
        let location = self.dap.session.as_ref()?.location.as_ref()?;
        let path = std::path::absolute(self.document(id)?.path()).ok()?;
        // An adapter may name the file through another path than the one it
        // was opened with (a symlink, `..`): compare the files, not the text.
        let same = path == location.path
            || std::fs::canonicalize(&path)
                .ok()
                .zip(std::fs::canonicalize(&location.path).ok())
                .is_some_and(|(a, b)| a == b);
        same.then_some(location.line)
    }

    /// After the settings loaded: reads `rustode.debugAdapters`.
    pub(crate) fn dap_config_changed(&mut self) {
        let (adapters, problems) = settings::parse(self.config.value(settings::KEY));
        for problem in &problems {
            tracing::warn!(%problem, "debug adapter settings");
        }
        self.dap.adapters = adapters;
        self.dap.problems = problems;
    }

    /// Processes what the debug adapter sent since the last call: answers,
    /// events (the program stopped, ended) and the adapter's exit. Outcomes
    /// are reported as [`Event::DebugChanged`].
    pub fn poll_debug(&mut self) {
        let mut messages = Vec::new();
        let mut closed = false;
        if let Some(session) = &self.dap.session {
            while let Some(item) = session.process.try_recv() {
                match item {
                    crate::child::Incoming::Message(message) => messages.push(message),
                    crate::child::Incoming::Closed => {
                        closed = true;
                        break;
                    }
                }
            }
        }
        let before = self.debug_status();
        for message in &messages {
            if self.dap.session.is_none() {
                break;
            }
            self.dap_message(message);
        }
        if closed && let Some(session) = &self.dap.session {
            // An adapter that exits right after `exited`, with no
            // `terminated`, ended the session normally.
            match session.exit_message() {
                Some(message) => self.dap_end(Some(message), false),
                None => self.dap_end(None, true),
            }
        }
        if !messages.is_empty() || closed || self.debug_status() != before {
            self.events.push(Event::DebugChanged);
        }
    }

    /// Ends the session: `message` says why, `crashed` that the adapter's
    /// output ended without being asked.
    pub(crate) fn dap_end(&mut self, message: Option<String>, crashed: bool) {
        let Some(session) = self.dap.session.take() else {
            return;
        };
        for watch in &mut self.dap.watches {
            watch.reset();
        }
        for file in self.dap.breakpoints.values_mut() {
            file.values_mut().for_each(Breakpoint::reset);
        }
        self.dap.message = if crashed {
            Some(format!("the debug adapter {}", session.reap()))
        } else {
            session.close_later();
            message
        };
        self.events.push(Event::DebugChanged);
    }
}

/// The folder a path is in, for relative paths an adapter sends.
pub(crate) fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}
