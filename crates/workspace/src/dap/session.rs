//! One debug session: the adapter process, the requests waiting for an
//! answer and what the adapter reported. Everything that arrives is
//! hostile input: a field of the wrong type is skipped, a number is checked
//! before it is used, and a message nobody asked for is ignored.

use super::breakpoints::{Breakpoint, Capabilities};
use super::launch::LaunchConfiguration;
use super::model::{self, ConsoleKind, FrameInfo, Node, ThreadInfo, short};
use super::{DebugState, StepKind, StopLocation};
use crate::Workspace;
use crate::child::Process;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Requests allowed to wait for an answer; past it, new stack trace
/// requests are not sent, so an adapter that never answers cannot grow it.
const MAX_PENDING: usize = 256;

/// What a request in flight was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    Initialize,
    Launch,
    SetBreakpoints,
    ConfigurationDone,
    Continue,
    Step,
    Pause,
    Terminate,
    Threads,
    StackTrace(i64),
    Scopes,
    Variables(i64),
    /// A watch expression, by the id of the watch.
    Watch(u64),
    /// An expression typed in the console.
    Repl,
}

impl Pending {
    /// Whether the answer is about the program's state at one stop: it is
    /// dropped, and never applied, once execution continues or the selected
    /// frame changes.
    fn is_state(self) -> bool {
        matches!(
            self,
            Self::Threads
                | Self::StackTrace(_)
                | Self::Scopes
                | Self::Variables(_)
                | Self::Watch(_)
        )
    }
}

/// A running session.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // the adapter's capabilities and the session's flags
pub(super) struct Session {
    pub name: String,
    pub state: DebugState,
    pub reason: Option<String>,
    pub location: Option<StopLocation>,
    pub process: Process,
    root: PathBuf,
    arguments: Value,
    next_seq: i64,
    pending: HashMap<i64, Pending>,
    /// `supportsConfigurationDoneRequest`.
    config_done: bool,
    /// `supportsTerminateRequest`.
    terminate: bool,
    /// The files whose breakpoints were sent: only they are sent again.
    initialized: bool,
    /// What the adapter does with breakpoints, once `initialize` answered.
    caps: Option<Capabilities>,
    /// The `setBreakpoints` requests in flight: the file and the lines
    /// (zero-based) they carried, by sequence number.
    sent: HashMap<i64, (PathBuf, Vec<usize>)>,
    /// The request is `attach`, so the program is not ours to terminate.
    attach: bool,
    /// `launch` or `attach` was sent; `initialized` before that waits.
    started: bool,
    /// `initialized` came before `launch` was sent.
    initialized_early: bool,
    /// A `terminate` was sent and the adapter has not ended yet.
    stopping: bool,
    pub thread: Option<i64>,
    exit_code: Option<i64>,
    /// A `continue` or step was sent and the adapter has not answered.
    resuming: bool,
    pub threads: Vec<ThreadInfo>,
    pub frames: Vec<FrameInfo>,
    /// The frame whose scopes and watches are shown: an index into `frames`.
    pub selected: usize,
    pub scopes: Vec<Node>,
}

impl Session {
    pub(super) fn new(
        configuration: &LaunchConfiguration,
        process: Process,
        arguments: Value,
        root: &Path,
    ) -> Self {
        // Relative paths in the adapter's frames are relative to where it
        // runs the program: the configuration's `cwd`, else the folder.
        let root = arguments["cwd"]
            .as_str()
            .filter(|cwd| !cwd.is_empty())
            .map_or_else(|| root.to_owned(), |cwd| super::resolve(root, cwd));
        Self {
            name: configuration.name.clone(),
            state: DebugState::Initializing,
            reason: None,
            location: None,
            process,
            root,
            arguments,
            next_seq: 1,
            pending: HashMap::new(),
            config_done: false,
            terminate: false,
            initialized: false,
            caps: None,
            sent: HashMap::new(),
            attach: configuration.request == "attach",
            started: false,
            initialized_early: false,
            stopping: false,
            thread: None,
            exit_code: None,
            resuming: false,
            threads: Vec::new(),
            frames: Vec::new(),
            selected: 0,
            scopes: Vec::new(),
        }
    }

    pub(super) fn request(&mut self, command: &str, arguments: &Value, what: Pending) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.pending.insert(seq, what);
        self.process.send(&json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        }));
    }

    /// What the adapter does with breakpoints, once it said.
    pub(super) fn capabilities(&self) -> Option<Capabilities> {
        self.caps
    }

    /// Sends a file's breakpoints once the adapter is configured: only the
    /// condition, hit count and log message it supports (VS Code sends them
    /// all and shows the unsupported ones as such,
    /// `debugSession.ts:488-520`; here an unsupported text is not sent, the
    /// breakpoint stays a plain one and shows as unverified).
    pub(super) fn send_breakpoints(
        &mut self,
        path: &Path,
        breakpoints: Option<&BTreeMap<usize, Breakpoint>>,
    ) {
        if !self.initialized {
            return;
        }
        let caps = self.caps.unwrap_or_default();
        let sent: Vec<(usize, &Breakpoint)> = breakpoints
            .into_iter()
            .flatten()
            .map(|(line, bp)| (*line, bp))
            .collect();
        let lines: Vec<usize> = sent.iter().map(|(line, _)| *line).collect();
        let seq = self.next_seq;
        self.request(
            "setBreakpoints",
            &json!({
                "source": {"path": path.display().to_string()},
                "breakpoints": sent.iter().map(|(line, bp)| bp.to_dap(*line, caps)).collect::<Vec<_>>(),
                "lines": lines.iter().map(|n| n + 1).collect::<Vec<_>>(),
                "sourceModified": false,
            }),
            Pending::SetBreakpoints,
        );
        if self.sent.len() < MAX_PENDING {
            self.sent.insert(seq, (path.to_owned(), lines));
        }
    }

    pub(super) fn resume(&mut self) {
        let thread = self.thread.unwrap_or(1);
        self.invalidate();
        self.resuming = true;
        self.request("continue", &json!({"threadId": thread}), Pending::Continue);
    }

    pub(super) fn step(&mut self, kind: StepKind) {
        let thread = self.thread.unwrap_or(1);
        self.invalidate();
        self.resuming = true;
        let command = match kind {
            StepKind::Over => "next",
            StepKind::Into => "stepIn",
            StepKind::Out => "stepOut",
        };
        self.request(command, &json!({"threadId": thread}), Pending::Step);
    }

    pub(super) fn pause(&mut self) {
        let thread = self.thread.unwrap_or(1);
        self.request("pause", &json!({"threadId": thread}), Pending::Pause);
    }

    /// Whether the program is stopped and no resume is on its way.
    pub(super) fn can_resume(&self) -> bool {
        self.state == DebugState::Stopped && !self.resuming
    }

    /// Forgets everything about the program's state at the last stop, and
    /// the requests still waiting for an answer about it: a late answer is
    /// one nobody asked for. Execution moved on.
    pub(super) fn invalidate(&mut self) {
        self.pending.retain(|_, what| !what.is_state());
        self.location = None;
        self.threads.clear();
        self.frames.clear();
        self.selected = 0;
        self.scopes.clear();
    }

    /// As [`Session::invalidate`] for a frame change: the call stack stays,
    /// what depends on the frame goes.
    pub(super) fn invalidate_frame(&mut self) {
        self.pending.retain(|_, what| {
            !matches!(
                what,
                Pending::Scopes | Pending::Variables(_) | Pending::Watch(_)
            )
        });
        self.scopes.clear();
    }

    /// Sends a request about the program's state, unless too many requests
    /// wait: an adapter that never answers cannot grow the table.
    pub(super) fn request_state(
        &mut self,
        command: &str,
        arguments: &Value,
        what: Pending,
    ) -> bool {
        if self.pending.len() >= MAX_PENDING {
            return false;
        }
        self.request(command, arguments, what);
        true
    }

    /// Whether another request about the program's state may be sent.
    pub(super) fn has_room(&self) -> bool {
        self.pending.len() < MAX_PENDING
    }

    /// The id of the selected frame.
    pub(super) fn frame_id(&self) -> Option<i64> {
        self.frames.get(self.selected).map(|f| f.id)
    }

    /// Asks for the threads and the call stack of the stopped thread.
    pub(super) fn refresh(&mut self) {
        self.request_state("threads", &json!({}), Pending::Threads);
        self.refresh_stack();
    }

    /// Asks for the call stack of the stopped thread.
    fn refresh_stack(&mut self) {
        if let Some(thread) = self.thread {
            self.request_state(
                "stackTrace",
                &json!({"threadId": thread, "startFrame": 0, "levels": model::MAX_FRAMES}),
                Pending::StackTrace(thread),
            );
        }
    }

    /// What the program's exit says, once the adapter reported it.
    pub(super) fn exit_message(&self) -> Option<String> {
        self.exit_code
            .map(|code| format!("the program exited with code {code}"))
    }

    /// Asks the adapter to end the program. Returns `true` when the
    /// session should be dropped now: the adapter was disconnected, or was
    /// already asked and has not obeyed.
    pub(super) fn stop(&mut self) -> bool {
        if self.terminate && !self.attach && !self.stopping {
            self.stopping = true;
            self.request("terminate", &json!({}), Pending::Terminate);
            return false;
        }
        true
    }

    /// Disconnects and waits for the adapter to exit, killing it if it
    /// lingers; blocks for up to about two seconds.
    pub(super) fn close_now(mut self) {
        let seq = self.send_disconnect();
        self.process.wait_reply(|r| r["request_seq"] == seq);
        self.process.finish();
    }

    /// Disconnects and leaves the adapter to a thread that waits for it.
    pub(super) fn close_later(mut self) {
        let seq = self.send_disconnect();
        let mut process = self.process;
        let _ = std::thread::Builder::new()
            .name("dap-reaper".to_owned())
            .spawn(move || {
                process.wait_reply(|r| r["request_seq"] == seq);
                process.finish();
            });
    }

    fn send_disconnect(&mut self) -> i64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.process.send(&json!({
            "seq": seq,
            "type": "request",
            "command": "disconnect",
            "arguments": {"terminateDebuggee": !self.attach},
        }));
        seq
    }

    /// How the adapter's process ended, after its output closed.
    pub(super) fn reap(self) -> String {
        self.process.reap()
    }
}

impl Workspace {
    /// One message from the adapter.
    pub(super) fn dap_message(&mut self, message: &Value) {
        match message["type"].as_str() {
            Some("response") => self.dap_response(message),
            Some("event") => self.dap_event(message),
            Some("request") => self.dap_refuse(message),
            _ => tracing::debug!(%message, "ignored an adapter message"),
        }
    }

    /// The adapter's own requests (`runInTerminal`, `startDebugging`) are
    /// not supported: `initialize` says so (`supportsRunInTerminalRequest`
    /// and `supportsStartDebuggingRequest` are false), and an adapter that
    /// asks anyway gets a failure that says why, which it needs to go on:
    /// debugpy and lldb-dap then run the program themselves. The console
    /// shows what was refused.
    fn dap_refuse(&mut self, request: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let command = request["command"].as_str().unwrap_or_default();
        let why = match command {
            "runInTerminal" => {
                "rustode has no integrated terminal: the program runs in the adapter's own console"
            }
            "startDebugging" => {
                "rustode debugs one process at a time: the subprocess is not debugged"
            }
            _ => "unsupported request",
        };
        let seq = session.next_seq;
        session.next_seq += 1;
        session.process.send(&json!({
            "seq": seq,
            "type": "response",
            "request_seq": request["seq"],
            "success": false,
            "command": request["command"],
            "message": why,
        }));
        self.dap_console(ConsoleKind::Error, &format!("{}: {why}", short(command)));
    }

    fn dap_response(&mut self, response: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let seq = response["request_seq"].as_i64().unwrap_or_default();
        let Some(what) = response["request_seq"]
            .as_i64()
            .and_then(|seq| session.pending.remove(&seq))
        else {
            tracing::debug!(%response, "an answer nobody asked for");
            return;
        };
        let success = response["success"] == true;
        let why = response["message"]
            .as_str()
            .map_or_else(|| "no reason given".to_owned(), short);
        let body = &response["body"];
        match what {
            Pending::Initialize | Pending::Launch | Pending::ConfigurationDone => {
                self.dap_startup(what, success, body, &why);
            }
            Pending::Continue | Pending::Step | Pending::Pause => {
                self.dap_resumed(what, success, &why);
            }
            Pending::Terminate => {
                if !success {
                    self.dap_end(Some(format!("cannot terminate: {why}")), false);
                }
            }
            Pending::SetBreakpoints => self.dap_breakpoints_reply(seq, success, body, &why),
            state => self.dap_state_reply(state, success.then_some(body), &why),
        }
    }

    /// The answers of the start: `initialize`, `launch`, `configurationDone`.
    fn dap_startup(&mut self, what: Pending, success: bool, body: &Value, why: &str) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        match (what, success) {
            (Pending::Initialize, true) => {
                session.config_done = body["supportsConfigurationDoneRequest"] == true;
                session.terminate = body["supportsTerminateRequest"] == true;
                session.caps = Some(Capabilities::parse(body));
                let arguments = session.arguments.clone();
                let command = if session.attach { "attach" } else { "launch" };
                session.request(command, &arguments, Pending::Launch);
                session.started = true;
                if std::mem::take(&mut session.initialized_early) {
                    self.dap_initialized();
                }
            }
            (_, false) => {
                self.dap_end(Some(format!("the debug adapter refused: {why}")), false);
            }
            _ => {}
        }
    }

    /// The answers to `continue`, a step and `pause`.
    fn dap_resumed(&mut self, what: Pending, success: bool, why: &str) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        match (what, success) {
            (Pending::Continue | Pending::Step, true) => {
                session.resuming = false;
                session.state = DebugState::Running;
                session.reason = None;
                session.location = None;
            }
            (Pending::Continue | Pending::Step, false) => {
                // Still stopped, and the views were emptied for nothing.
                session.resuming = false;
                session.refresh();
                let verb = if what == Pending::Step {
                    "step"
                } else {
                    "continue"
                };
                self.dap.message = Some(format!("cannot {verb}: {why}"));
            }
            (_, false) => self.dap.message = Some(format!("cannot pause: {why}")),
            _ => {}
        }
    }

    /// The answers about the program's state at the stop: `body` is `None`
    /// when the adapter failed or cancelled the request.
    fn dap_state_reply(&mut self, what: Pending, body: Option<&Value>, why: &str) {
        let root = self.dap_root();
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        match (what, body) {
            (Pending::Threads, Some(body)) => {
                session.threads = model::parse_threads(body);
                // A `stopped` without a thread: the first one is the one.
                if session.thread.is_none()
                    && session.state == DebugState::Stopped
                    && let Some(first) = session.threads.first()
                {
                    session.thread = Some(first.id);
                    session.refresh_stack();
                }
            }
            (Pending::StackTrace(thread), Some(body))
                if session.state == DebugState::Stopped && session.thread == Some(thread) =>
            {
                session.frames = model::parse_frames(body, &root);
                session.selected = 0;
                self.dap_frame_changed();
            }
            (Pending::Scopes, Some(body)) => self.dap_scopes(body),
            (Pending::Variables(reference), body) => self.dap_variables(reference, body),
            (Pending::Watch(id), body) => self.dap_watch_result(id, body, why),
            (Pending::Repl, body) => self.dap_repl_result(body, why),
            _ => {}
        }
    }

    /// The selected frame changed: the editor goes to its line, its scopes
    /// and the watch expressions are asked for.
    pub(super) fn dap_frame_changed(&mut self) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let frame = session.frames.get(session.selected).cloned();
        session.location = frame.as_ref().and_then(|f| {
            Some(StopLocation {
                path: f.path.clone()?,
                line: f.line,
            })
        });
        let Some(frame) = frame else {
            return;
        };
        session.request_state("scopes", &json!({"frameId": frame.id}), Pending::Scopes);
        for watch in &mut self.dap.watches {
            watch.reset();
            session.request_state(
                "evaluate",
                &json!({"expression": watch.name, "frameId": frame.id, "context": "watch"}),
                Pending::Watch(watch.id),
            );
        }
        self.dap_reveal(frame.path.as_deref(), frame.line);
    }

    /// Opens the file of a frame in the editor, on its line, if it is a
    /// reasonable file to open.
    fn dap_reveal(&mut self, path: Option<&std::path::Path>, line: usize) {
        const LARGEST: u64 = 64 * 1024 * 1024;
        let Some(path) = path else {
            return;
        };
        let reasonable = std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= LARGEST);
        if reasonable && let Ok(id) = self.open(path) {
            let _ = self.go_to_line(id, line + 1, None);
        }
    }

    fn dap_scopes(&mut self, body: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        if session.state != DebugState::Stopped {
            return;
        }
        let mut scopes = model::parse_scopes(body);
        // The first scope that is cheap to read opens by itself, as in VS Code.
        let first = scopes.iter().position(|(_, expensive)| !expensive);
        let mut request = None;
        if let Some(index) = first {
            let node = &mut scopes[index].0;
            node.expanded = true;
            node.loading = true;
            request = Some(node.reference);
        }
        session.scopes = scopes.into_iter().map(|(node, _)| node).collect();
        if let Some(reference) = request {
            session.request_state(
                "variables",
                &json!({"variablesReference": reference}),
                Pending::Variables(reference),
            );
        }
    }

    fn dap_variables(&mut self, reference: i64, body: Option<&Value>) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let used = model::count(&session.scopes).max(model::count(&self.dap.watches));
        let budget = model::MAX_NODES.saturating_sub(used);
        model::fill(&mut session.scopes, reference, body, budget);
        model::fill(&mut self.dap.watches, reference, body, budget);
    }

    fn dap_watch_result(&mut self, id: u64, body: Option<&Value>, why: &str) {
        if let Some(watch) = self.dap.watches.iter_mut().find(|w| w.id == id) {
            if let Some(body) = body {
                watch.value = short(body["result"].as_str().unwrap_or_default());
                watch.kind = body["type"].as_str().map(short);
                watch.reference = model::reference(&body["variablesReference"]);
            } else {
                why.clone_into(&mut watch.value);
                watch.reference = 0;
            }
        }
    }

    fn dap_repl_result(&mut self, body: Option<&Value>, why: &str) {
        match body {
            Some(body) => {
                let text = body["result"].as_str().unwrap_or_default();
                self.dap_console(ConsoleKind::Result, text);
            }
            None => self.dap_console(ConsoleKind::Error, why),
        }
    }

    /// Adds the lines of `text` to the debug console.
    pub(super) fn dap_console(&mut self, kind: ConsoleKind, text: &str) {
        let text = text.strip_suffix('\n').unwrap_or(text);
        for line in text.split('\n').take(model::MAX_CONSOLE) {
            self.dap.console.push(model::ConsoleLine {
                kind,
                text: short(line.trim_end_matches('\r')),
            });
        }
        let extra = self.dap.console.len().saturating_sub(model::MAX_CONSOLE);
        self.dap.console.drain(..extra);
    }

    /// The answer to a `setBreakpoints`: each breakpoint takes what the
    /// adapter said about the one at its index (`debugSession.ts:512-519`).
    /// A refused request leaves them all unverified, with the reason.
    fn dap_breakpoints_reply(&mut self, seq: i64, success: bool, body: &Value, why: &str) {
        let Some((path, lines)) = self
            .dap
            .session
            .as_mut()
            .and_then(|session| session.sent.remove(&seq))
        else {
            return;
        };
        let Some(file) = self.dap.breakpoints.get_mut(&path) else {
            return;
        };
        let answers = body["breakpoints"].as_array();
        for (index, line) in lines.iter().enumerate() {
            let Some(bp) = file.get_mut(line) else {
                continue;
            };
            if !success {
                bp.verified = Some(false);
                bp.message = Some(why.to_owned());
            } else if let Some(answer) = answers.and_then(|a| a.get(index)) {
                bp.apply(answer);
            }
        }
    }

    /// A `breakpoint` event: the adapter changed what it says about one of
    /// ours, found by id (`debugSession.ts:1000-1040`).
    fn dap_breakpoint_event(&mut self, body: &Value) {
        if body["reason"].as_str() != Some("changed") {
            return;
        }
        let data = &body["breakpoint"];
        let Some(id) = model::id(&data["id"]) else {
            return;
        };
        for file in self.dap.breakpoints.values_mut() {
            for bp in file.values_mut().filter(|bp| bp.id == Some(id)) {
                bp.apply(data);
            }
        }
    }

    fn dap_root(&self) -> PathBuf {
        self.dap
            .session
            .as_ref()
            .map_or_else(PathBuf::new, |s| s.root.clone())
    }

    fn dap_event(&mut self, event: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let body = &event["body"];
        match event["event"].as_str() {
            Some("initialized") if !session.started => session.initialized_early = true,
            Some("initialized") => self.dap_initialized(),
            Some("breakpoint") => self.dap_breakpoint_event(body),
            Some("stopped") => {
                session.invalidate();
                session.resuming = false;
                session.state = DebugState::Stopped;
                session.reason = Some(
                    body["reason"]
                        .as_str()
                        .map_or_else(|| "unknown".to_owned(), short),
                );
                session.thread = model::id(&body["threadId"]).or(session.thread);
                session.refresh();
            }
            Some("continued") if session.state == DebugState::Stopped => {
                session.invalidate();
                session.state = DebugState::Running;
                session.reason = None;
            }
            Some("exited") => session.exit_code = body["exitCode"].as_i64(),
            Some("terminated") => {
                let message = session.exit_message();
                self.dap_end(message, false);
            }
            Some("output") => {
                let kind = match body["category"].as_str() {
                    Some("stderr") => ConsoleKind::Error,
                    Some("telemetry") => return,
                    _ => ConsoleKind::Output,
                };
                if let Some(text) = body["output"].as_str() {
                    self.dap_console(kind, text);
                }
            }
            _ => {}
        }
    }

    /// The adapter is ready for the breakpoints and `configurationDone`.
    fn dap_initialized(&mut self) {
        let files: Vec<(PathBuf, BTreeMap<usize, Breakpoint>)> = self
            .dap
            .breakpoints
            .iter()
            .map(|(path, lines)| (path.clone(), lines.clone()))
            .collect();
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        session.initialized = true;
        for (path, lines) in &files {
            session.send_breakpoints(path, Some(lines));
        }
        if session.config_done {
            session.request("configurationDone", &json!({}), Pending::ConfigurationDone);
        }
        session.state = DebugState::Running;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn hostile_numbers_and_shapes_are_refused() {
        for bad in [
            json!(-1),
            json!(4_294_967_296_i64),
            json!("1"),
            json!(1.5),
            Value::Null,
        ] {
            assert_eq!(model::id(&bad), None, "{bad}");
        }
        assert_eq!(model::id(&json!(7)), Some(7));
        let root = Path::new("/w");
        for bad in [
            json!(null),
            json!({"stackFrames": "x"}),
            json!({"stackFrames": []}),
            json!({"stackFrames": [{"id": 1, "line": 0, "source": {"path": "/a"}}]}),
            json!({"stackFrames": [{"id": 1, "line": -3, "source": {"path": "/a"}}]}),
            json!({"stackFrames": [{"id": -1, "line": 2, "source": {"path": "/a"}}]}),
        ] {
            let frames = model::parse_frames(&bad, root);
            assert!(frames.iter().all(|f| f.path.is_none()), "{bad}");
        }
        let frame = json!({"stackFrames": [{"id": 4, "name": "f", "line": 3, "source": {"path": "rel.txt"}}]});
        let frames = model::parse_frames(&frame, root);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].path.as_deref(), Some(Path::new("/w/rel.txt")));
        assert_eq!(frames[0].line, 2);
        assert_eq!(
            short(&"é".repeat(model::MAX_TEXT + 10)).chars().count(),
            model::MAX_TEXT
        );
    }
}
