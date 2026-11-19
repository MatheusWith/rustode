//! One debug session: the adapter process, the requests waiting for an
//! answer and what the adapter reported. Everything that arrives is
//! hostile input: a field of the wrong type is skipped, a number is checked
//! before it is used, and a message nobody asked for is ignored.

use super::launch::LaunchConfiguration;
use super::{DebugState, StopLocation, resolve};
use crate::Workspace;
use crate::child::Process;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Requests allowed to wait for an answer; past it, new stack trace
/// requests are not sent, so an adapter that never answers cannot grow it.
const MAX_PENDING: usize = 256;
/// Longest message text kept from an adapter.
const MAX_TEXT: usize = 500;

/// What a request in flight was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pending {
    Initialize,
    Launch,
    SetBreakpoints,
    ConfigurationDone,
    Continue,
    StackTrace(i64),
    Terminate,
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
    /// A `terminate` was sent and the adapter has not ended yet.
    stopping: bool,
    thread: Option<i64>,
    exit_code: Option<i64>,
}

impl Session {
    pub(super) fn new(
        configuration: &LaunchConfiguration,
        process: Process,
        arguments: Value,
        root: PathBuf,
    ) -> Self {
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
            stopping: false,
            thread: None,
            exit_code: None,
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

    /// Sends a file's breakpoints once the adapter is configured.
    pub(super) fn send_breakpoints(&mut self, path: &Path, lines: Option<&BTreeSet<usize>>) {
        if !self.initialized {
            return;
        }
        let lines: Vec<usize> = lines.map_or_else(Vec::new, |l| l.iter().map(|n| n + 1).collect());
        self.request(
            "setBreakpoints",
            &json!({
                "source": {"path": path.display().to_string()},
                "breakpoints": lines.iter().map(|line| json!({"line": line})).collect::<Vec<_>>(),
                "lines": lines,
                "sourceModified": false,
            }),
            Pending::SetBreakpoints,
        );
    }

    pub(super) fn resume(&mut self) {
        let thread = self.thread.unwrap_or(1);
        self.request("continue", &json!({"threadId": thread}), Pending::Continue);
    }

    /// Asks the adapter to end the program. Returns `true` when the
    /// session should be dropped now: the adapter was disconnected, or was
    /// already asked and has not obeyed.
    pub(super) fn stop(&mut self) -> bool {
        if self.terminate && !self.stopping {
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
            "arguments": {"terminateDebuggee": true},
        }));
        seq
    }

    /// How the adapter's process ended, after its output closed.
    pub(super) fn reap(self) -> String {
        self.process.reap()
    }
}

/// `text` cut to what is worth showing.
fn short(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

/// A thread id an adapter sent, if it is a sane one.
fn thread_id(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|id| (0..=i64::from(i32::MAX)).contains(id))
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
    /// not supported: answering keeps it from waiting for us.
    fn dap_refuse(&mut self, request: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
        let seq = session.next_seq;
        session.next_seq += 1;
        session.process.send(&json!({
            "seq": seq,
            "type": "response",
            "request_seq": request["seq"],
            "success": false,
            "command": request["command"],
            "message": "unsupported request",
        }));
    }

    fn dap_response(&mut self, response: &Value) {
        let Some(session) = self.dap.session.as_mut() else {
            return;
        };
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
        match (what, success) {
            (Pending::Initialize, true) => {
                let body = &response["body"];
                session.config_done = body["supportsConfigurationDoneRequest"] == true;
                session.terminate = body["supportsTerminateRequest"] == true;
                let arguments = session.arguments.clone();
                session.request("launch", &arguments, Pending::Launch);
            }
            (Pending::Initialize | Pending::Launch | Pending::ConfigurationDone, false) => {
                self.dap_end(Some(format!("the debug adapter refused: {why}")), false);
            }
            (Pending::Continue, true) => {
                session.state = DebugState::Running;
                session.reason = None;
                session.location = None;
            }
            (Pending::Continue, false) => {
                self.dap.message = Some(format!("cannot continue: {why}"));
            }
            (Pending::StackTrace(thread), true) => {
                let location = first_frame(&response["body"], &self.dap_root());
                if let Some(session) = self.dap.session.as_mut()
                    && session.state == DebugState::Stopped
                    && session.thread == Some(thread)
                {
                    session.location = location;
                }
            }
            (Pending::Terminate, false) => {
                self.dap_end(Some(format!("cannot terminate: {why}")), false);
            }
            _ => {}
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
            Some("initialized") => {
                session.initialized = true;
                let files: Vec<(PathBuf, BTreeSet<usize>)> = self
                    .dap
                    .breakpoints
                    .iter()
                    .map(|(path, lines)| (path.clone(), lines.clone()))
                    .collect();
                let Some(session) = self.dap.session.as_mut() else {
                    return;
                };
                for (path, lines) in &files {
                    session.send_breakpoints(path, Some(lines));
                }
                if session.config_done {
                    session.request("configurationDone", &json!({}), Pending::ConfigurationDone);
                }
                session.state = DebugState::Running;
            }
            Some("stopped") => {
                session.state = DebugState::Stopped;
                session.reason = Some(
                    body["reason"]
                        .as_str()
                        .map_or_else(|| "unknown".to_owned(), short),
                );
                session.location = None;
                session.thread = thread_id(&body["threadId"]).or(session.thread);
                if let Some(thread) = session.thread
                    && session.pending.len() < MAX_PENDING
                {
                    session.request(
                        "stackTrace",
                        &json!({"threadId": thread, "startFrame": 0, "levels": 1}),
                        Pending::StackTrace(thread),
                    );
                }
            }
            Some("continued") if session.state == DebugState::Stopped => {
                session.state = DebugState::Running;
                session.reason = None;
                session.location = None;
            }
            Some("exited") => session.exit_code = body["exitCode"].as_i64(),
            Some("terminated") => {
                let message = session
                    .exit_code
                    .map(|code| format!("the program exited with code {code}"));
                self.dap_end(message, false);
            }
            Some("output") => {
                if let Some(text) = body["output"].as_str() {
                    tracing::debug!(output = %short(text), "from the debuggee");
                }
            }
            _ => {}
        }
    }
}

/// The first stack frame's file and line, from a `stackTrace` body.
fn first_frame(body: &Value, root: &Path) -> Option<StopLocation> {
    let frame = body["stackFrames"].as_array()?.first()?;
    let path = resolve(root, frame["source"]["path"].as_str()?);
    let line = frame["line"].as_u64().filter(|line| *line >= 1)?;
    Some(StopLocation {
        path,
        line: usize::try_from(line - 1).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_numbers_and_shapes_are_refused() {
        for bad in [
            json!(-1),
            json!(4_294_967_296_i64),
            json!("1"),
            json!(1.5),
            Value::Null,
        ] {
            assert_eq!(thread_id(&bad), None, "{bad}");
        }
        assert_eq!(thread_id(&json!(7)), Some(7));
        let root = Path::new("/w");
        for bad in [
            json!(null),
            json!({"stackFrames": "x"}),
            json!({"stackFrames": []}),
            json!({"stackFrames": [{"line": 0, "source": {"path": "/a"}}]}),
            json!({"stackFrames": [{"line": -3, "source": {"path": "/a"}}]}),
            json!({"stackFrames": [{"line": 2, "source": {"path": 5}}]}),
            json!({"stackFrames": [{"line": u64::MAX, "source": {"name": "x"}}]}),
        ] {
            assert_eq!(first_frame(&bad, root), None, "{bad}");
        }
        let frame = json!({"stackFrames": [{"line": 3, "source": {"path": "rel.txt"}}]});
        assert_eq!(
            first_frame(&frame, root),
            Some(StopLocation {
                path: PathBuf::from("/w/rel.txt"),
                line: 2
            })
        );
        assert_eq!(short(&"é".repeat(MAX_TEXT + 10)).chars().count(), MAX_TEXT);
    }
}
