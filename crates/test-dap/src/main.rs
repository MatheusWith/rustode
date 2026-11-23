//! `rustode-test-dap`: a debug adapter for rustode's tests only, never
//! shipped. It speaks DAP over stdio with `Content-Length` framing and
//! "runs" a program that is a text file, one statement per line, from line 1:
//!
//! - a line the client set a breakpoint on (`setBreakpoints` for the
//!   program's path) stops it before the line runs: a `stopped` event with
//!   reason `breakpoint`, thread 1;
//! - `continue` resumes; after the last line the adapter sends `exited`
//!   (exit code 0) and `terminated`;
//! - `print TEXT` sends an `output` event with `TEXT`;
//! - `CRASH` makes the adapter exit at once with code 3, saying nothing;
//! - `HOSTILE` sends malformed events and responses (a `stopped` whose body
//!   is a string, a response nobody asked for, a thread id out of range),
//!   then goes on;
//! - `threads`, `stackTrace`, `scopes`, `variables` and `evaluate` answer
//!   from the program's frames and objects, `next`, `stepIn`, `stepOut` and
//!   `pause` move it (see [`program`] for the statements).
//!
//! `evaluate` answers a variable's name with its value; the expression
//! `!cancel` is answered with a `cancelled` failure and `!hang` is never
//! answered.
//!
//! The order follows the protocol as VS Code drives it: `initialize` is
//! answered with the capabilities, `launch` is answered only after
//! `configurationDone`, and the `initialized` event is sent when `launch`
//! arrives, so a client that sends breakpoints too early or waits for the
//! launch answer first gets it wrong.
//!
//! Options:
//!
//! - `--no-config-done`: do not offer `supportsConfigurationDoneRequest`;
//!   the program then starts when `launch` arrives;
//! - `--fail-launch`: answer `launch` with `success: false` and a message;
//! - `--no-terminate`: do not offer `supportsTerminateRequest`;
//! - `--hostile-replies`: answer `threads`, `stackTrace`, `scopes`,
//!   `variables` and `evaluate` with oversized and malformed bodies, and
//!   `setBreakpoints` with malformed breakpoints;
//! - `--no-conditions`, `--no-hit-conditions`, `--no-logpoints`: do not offer
//!   `supportsConditionalBreakpoints`, `supportsHitConditionalBreakpoints` or
//!   `supportsLogPoints`; the adapter then ignores those fields;
//! - `--late-verify`: answer `setBreakpoints` with unverified breakpoints
//!   ("waiting for the program") and verify the ones on a line of the program
//!   with `breakpoint` events when an `evaluate` of `!verify` arrives; a
//!   breakpoint past the end of the program stays unverified. Without the
//!   option the response itself says "no code on this line" for those;
//! - `--initialized-first`: send `initialized` before the `initialize`
//!   response, an order the protocol forbids and a client should survive;
//! - `--initialized-early`: send it right after the `initialize` response,
//!   as lldb-dap does, instead of when `launch` arrives;
//! - `--exit-without-terminated`: after the last line send `exited` and
//!   exit, with no `terminated` event;
//! - `--stop-without-thread`: leave `threadId` out of `stopped` events;
//! - `--reverse-requests`: send `runInTerminal` and `startDebugging`
//!   requests when `configurationDone` arrives, as debugpy and lldb-dap do;
//! - `--relative-paths`: name the program in `stackTrace` by its file name
//!   only, relative to the folder it is in, as adapters that run the program
//!   from its `cwd` do;
//! - `--log PATH`: append every message received, one JSON object per line.
//!
//! `attach` is answered like `launch`; the program is its `program`.
//! `setBreakpoints` takes the `condition`, `hitCondition` and `logMessage` of
//! each breakpoint and answers with an `id` per breakpoint (`1000` + its
//! line), `verified` and, when not verified, a `message`.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::unimplemented,
        clippy::todo
    )
)]

mod program;

use program::{Breakpoints, Local, Mode, Outcome, Program, Spec, Special};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};

/// What the command line asked for.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // one flag per command-line option
struct Options {
    no_config_done: bool,
    fail_launch: bool,
    no_terminate: bool,
    hostile_replies: bool,
    no_conditions: bool,
    no_hit_conditions: bool,
    no_logpoints: bool,
    late_verify: bool,
    initialized_first: bool,
    initialized_early: bool,
    exit_without_terminated: bool,
    stop_without_thread: bool,
    reverse_requests: bool,
    relative_paths: bool,
    log: Option<String>,
}

fn options(mut args: impl Iterator<Item = String>) -> Options {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-config-done" => options.no_config_done = true,
            "--fail-launch" => options.fail_launch = true,
            "--no-terminate" => options.no_terminate = true,
            "--hostile-replies" => options.hostile_replies = true,
            "--no-conditions" => options.no_conditions = true,
            "--no-hit-conditions" => options.no_hit_conditions = true,
            "--no-logpoints" => options.no_logpoints = true,
            "--late-verify" => options.late_verify = true,
            "--initialized-first" => options.initialized_first = true,
            "--initialized-early" => options.initialized_early = true,
            "--exit-without-terminated" => options.exit_without_terminated = true,
            "--stop-without-thread" => options.stop_without_thread = true,
            "--reverse-requests" => options.reverse_requests = true,
            "--relative-paths" => options.relative_paths = true,
            "--log" => options.log = args.next(),
            other => eprintln!("rustode-test-dap: ignoring {other}"),
        }
    }
    options
}

/// The adapter's state.
struct Adapter {
    options: Options,
    seq: i64,
    out: io::Stdout,
    /// The program, once launched.
    program: Program,
    path: String,
    /// The breakpoints of the program, by 1-based line.
    breakpoints: Breakpoints,
    /// The program is stopped (at a breakpoint, a step or a pause).
    stopped: bool,
    /// The `launch` request waiting for `configurationDone`.
    launch: Option<Value>,
}

fn main() {
    let mut adapter = Adapter {
        options: options(std::env::args().skip(1)),
        seq: 0,
        out: io::stdout(),
        program: Program::default(),
        path: String::new(),
        breakpoints: Breakpoints::new(),
        stopped: false,
        launch: None,
    };
    let mut input = BufReader::new(io::stdin().lock());
    while let Some(message) = read_message(&mut input) {
        adapter.log(&message);
        adapter.handle(&message);
    }
}

/// One message from `input`; `None` at the end of the stream.
fn read_message(input: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

impl Adapter {
    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        let _ = write!(self.out, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = self.out.flush();
    }

    fn log(&self, entry: &Value) {
        if let Some(path) = &self.options.log
            && let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let _ = writeln!(file, "{entry}");
        }
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    fn respond(&mut self, request: &Value, success: bool, body: &Value, message: Option<&str>) {
        let seq = self.next_seq();
        let mut response = json!({
            "seq": seq,
            "type": "response",
            "request_seq": request["seq"],
            "success": success,
            "command": request["command"],
            "body": body,
        });
        if let Some(message) = message {
            response["message"] = json!(message);
        }
        self.send(&response);
    }

    fn event(&mut self, event: &str, body: &Value) {
        let seq = self.next_seq();
        self.send(&json!({"seq": seq, "type": "event", "event": event, "body": body}));
    }

    fn fail(&mut self, request: &Value, message: &str) {
        self.respond(request, false, &json!({}), Some(message));
    }

    fn handle(&mut self, request: &Value) {
        let args = &request["arguments"];
        match request["command"].as_str().unwrap_or_default() {
            "initialize" => {
                let body = json!({
                    "supportsConfigurationDoneRequest": !self.options.no_config_done,
                    "supportsTerminateRequest": !self.options.no_terminate,
                    "supportsConditionalBreakpoints": !self.options.no_conditions,
                    "supportsHitConditionalBreakpoints": !self.options.no_hit_conditions,
                    "supportsLogPoints": !self.options.no_logpoints,
                });
                if self.options.initialized_first {
                    self.event("initialized", &json!({}));
                }
                self.respond(request, true, &body, None);
                if self.options.initialized_early {
                    self.event("initialized", &json!({}));
                }
            }
            "launch" | "attach" => self.launch(request, args),
            "setBreakpoints" => self.set_breakpoints(request, args),
            "configurationDone" => {
                self.respond(request, true, &json!({}), None);
                if self.options.reverse_requests {
                    self.reverse_requests();
                }
                if let Some(launch) = self.launch.take() {
                    self.respond(&launch, true, &json!({}), None);
                }
                self.run(Mode::Continue);
            }
            "continue" => {
                self.program.skip = true;
                self.respond(request, true, &json!({"allThreadsContinued": true}), None);
                self.event(
                    "continued",
                    &json!({"threadId": 1, "allThreadsContinued": true}),
                );
                self.run(Mode::Continue);
            }
            "next" => self.step(request, Mode::Over),
            "stepIn" => self.step(request, Mode::In),
            "stepOut" => self.step(request, Mode::Out),
            "pause" => self.pause(request),
            "threads" | "stackTrace" | "scopes" | "variables" | "evaluate" => {
                if self.options.hostile_replies {
                    self.hostile_reply(request);
                } else {
                    self.inspect(request, args);
                }
            }
            "terminate" => {
                self.respond(request, true, &json!({}), None);
                self.event("terminated", &json!({}));
            }
            "disconnect" => {
                self.respond(request, true, &json!({}), None);
                std::process::exit(0);
            }
            other => self.fail(request, &format!("unsupported request {other}")),
        }
    }

    fn step(&mut self, request: &Value, mode: Mode) {
        if !self.stopped {
            self.fail(request, "the program is not stopped");
            return;
        }
        self.respond(request, true, &json!({}), None);
        self.run(mode);
    }

    fn pause(&mut self, request: &Value) {
        let Some(line) = self.waiting_line() else {
            self.fail(request, "the program is not running");
            return;
        };
        self.respond(request, true, &json!({}), None);
        self.program.pause(line);
        self.program.next = line;
        self.stop("pause");
    }

    /// The `WAIT` line the program is waiting at, if it is not stopped.
    fn waiting_line(&self) -> Option<usize> {
        let line = self.program.lines.get(self.program.next)?;
        (!self.stopped && line == "WAIT").then_some(self.program.next)
    }

    fn launch(&mut self, request: &Value, args: &Value) {
        if self.options.fail_launch {
            self.fail(request, "cannot launch the program");
            return;
        }
        args["program"]
            .as_str()
            .unwrap_or_default()
            .clone_into(&mut self.path);
        match std::fs::read_to_string(&self.path) {
            Ok(text) => self.program = Program::load(&text),
            Err(err) => {
                let message = format!("cannot read {}: {err}", self.path);
                self.fail(request, &message);
                return;
            }
        }
        if !self.options.initialized_first && !self.options.initialized_early {
            self.event("initialized", &json!({}));
        }
        if self.options.no_config_done {
            self.respond(request, true, &json!({}), None);
            self.run(Mode::Continue);
        } else {
            self.launch = Some(request.clone());
        }
    }

    fn set_breakpoints(&mut self, request: &Value, args: &Value) {
        let text = |bp: &Value, key: &str| {
            bp[key]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        let sent: Vec<(usize, Spec)> = args["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|bp| {
                let line = usize::try_from(bp["line"].as_u64()?).ok()?;
                let spec = Spec {
                    condition: text(bp, "condition").filter(|_| !self.options.no_conditions),
                    hit_condition: text(bp, "hitCondition")
                        .filter(|_| !self.options.no_hit_conditions),
                    log_message: text(bp, "logMessage").filter(|_| !self.options.no_logpoints),
                };
                Some((line, spec))
            })
            .collect();
        if self.same_file(args["source"]["path"].as_str()) || self.path.is_empty() {
            self.breakpoints = sent.iter().cloned().collect();
            let kept: Vec<usize> = self.breakpoints.keys().copied().collect();
            self.program.hits.retain(|line, _| kept.contains(line));
        }
        let verified: Vec<Value> = sent
            .iter()
            .map(|(line, _)| {
                let id = 1000 + line;
                if self.options.late_verify {
                    json!({"id": id, "verified": false, "line": line, "message": "waiting for the program"})
                } else if self.has_code(*line) {
                    json!({"id": id, "verified": true, "line": line})
                } else {
                    json!({"id": id, "verified": false, "line": line, "message": "no code on this line"})
                }
            })
            .collect();
        if self.options.hostile_replies {
            let body = json!({"breakpoints": [
                null,
                5,
                {"id": "x", "verified": "yes", "message": ["no"], "line": -1},
                {"id": -3, "verified": 1},
                {"id": 99_999_999_999_i64, "message": "m".repeat(5_000)},
            ]});
            self.respond(request, true, &body, None);
            return;
        }
        self.respond(request, true, &json!({"breakpoints": verified}), None);
    }

    /// The program's path as `stackTrace` names it.
    fn frame_path(&self) -> String {
        if self.options.relative_paths
            && let Some(name) = std::path::Path::new(&self.path).file_name()
        {
            return name.to_string_lossy().into_owned();
        }
        self.path.clone()
    }

    /// Whether `path` names the program, by link or not.
    fn same_file(&self, path: Option<&str>) -> bool {
        let Some(path) = path else { return false };
        path == self.path
            || std::fs::canonicalize(path)
                .ok()
                .zip(std::fs::canonicalize(&self.path).ok())
                .is_some_and(|(a, b)| a == b)
    }

    /// Whether a one-based line is inside the program (or no program was
    /// loaded yet).
    fn has_code(&self, line: usize) -> bool {
        self.program.lines.is_empty() || line <= self.program.lines.len()
    }

    /// With `--late-verify`: the breakpoints on lines with code are verified.
    fn verify_late(&mut self) {
        let lines: Vec<usize> = self
            .breakpoints
            .keys()
            .copied()
            .filter(|line| self.has_code(*line))
            .collect();
        for line in lines {
            self.event(
                "breakpoint",
                &json!({"reason": "changed", "breakpoint": {"id": 1000 + line, "verified": true}}),
            );
        }
    }

    /// With `--reverse-requests`: the adapter's own requests.
    fn reverse_requests(&mut self) {
        let seq = self.next_seq();
        self.send(&json!({
            "seq": seq, "type": "request", "command": "runInTerminal",
            "arguments": {"kind": "integrated", "cwd": ".", "args": ["true"]},
        }));
        let seq = self.next_seq();
        self.send(&json!({
            "seq": seq, "type": "request", "command": "startDebugging",
            "arguments": {"request": "attach", "configuration": {"name": "Subprocess"}},
        }));
    }

    fn stop(&mut self, reason: &str) {
        self.stopped = true;
        let mut body = json!({"reason": reason, "threadId": 1, "allThreadsStopped": true});
        if self.options.stop_without_thread {
            body = json!({"reason": reason, "allThreadsStopped": true});
        }
        self.event("stopped", &body);
    }

    /// Runs the program as `mode` says and reports how it ended.
    fn run(&mut self, mode: Mode) {
        self.stopped = false;
        let mut printed = Vec::new();
        let outcome = self
            .program
            .run(mode, &self.breakpoints.clone(), &mut printed);
        for line in printed {
            self.event(
                "output",
                &json!({"category": line.category, "output": format!("{}\n", line.text)}),
            );
        }
        match outcome {
            Outcome::Stopped(reason) => self.stop(reason),
            Outcome::Waiting => {}
            Outcome::Finished => {
                self.event("exited", &json!({"exitCode": 0}));
                if self.options.exit_without_terminated {
                    std::process::exit(0);
                }
                self.event("terminated", &json!({}));
            }
            Outcome::Special(Special::Crash) => std::process::exit(3),
            Outcome::Special(Special::Hostile) => {
                self.hostile();
                self.run(mode);
            }
        }
    }
}

/// Answers to `threads`, `stackTrace`, `scopes`, `variables` and `evaluate`.
impl Adapter {
    fn inspect(&mut self, request: &Value, args: &Value) {
        match request["command"].as_str().unwrap_or_default() {
            "threads" => {
                let body = json!({"threads": [
                    {"id": 1, "name": "main"},
                    {"id": 2, "name": "worker"},
                ]});
                self.respond(request, true, &body, None);
            }
            "stackTrace" => {
                let frames: Vec<Value> = self
                    .program
                    .stack()
                    .iter()
                    .map(|(depth, frame, line)| {
                        json!({
                            "id": depth + 1,
                            "name": frame.name,
                            "source": {"path": self.frame_path()},
                            "line": line,
                            "column": 1,
                        })
                    })
                    .collect();
                let body = json!({"totalFrames": frames.len(), "stackFrames": frames});
                self.respond(request, true, &body, None);
            }
            "scopes" => self.scopes(request, args),
            "variables" => self.variables(request, args),
            _ => self.evaluate(request, args),
        }
    }

    /// The frame index (depth) a `frameId` names; the innermost without one.
    fn depth_of(&self, args: &Value) -> Option<usize> {
        match args["frameId"].as_u64() {
            Some(id) => usize::try_from(id)
                .ok()
                .and_then(|id| id.checked_sub(1))
                .filter(|depth| *depth < self.program.frames.len()),
            None => self.program.frames.len().checked_sub(1),
        }
    }

    fn scopes(&mut self, request: &Value, args: &Value) {
        let Some(depth) = self.depth_of(args) else {
            self.fail(request, "no such frame");
            return;
        };
        let body = json!({"scopes": [
            {"name": "Locals", "variablesReference": 100 + depth, "expensive": false},
            {"name": "Globals", "variablesReference": 90, "expensive": true},
        ]});
        self.respond(request, true, &body, None);
    }

    fn variables(&mut self, request: &Value, args: &Value) {
        let reference = args["variablesReference"].as_i64().unwrap_or_default();
        let list: Option<Vec<Value>> = match reference {
            90 => Some(
                self.program
                    .globals
                    .iter()
                    .map(|(name, value)| Self::variable(name, value, None))
                    .collect(),
            ),
            100..=999 => usize::try_from(reference - 100)
                .ok()
                .and_then(|depth| self.program.frames.get(depth))
                .map(|frame| {
                    frame
                        .locals
                        .iter()
                        .map(|(name, local)| match local {
                            Local::Value(value) => Self::variable(name, value, None),
                            Local::Object(index) => Self::variable(name, "Object", Some(*index)),
                        })
                        .collect()
                }),
            1000.. => usize::try_from(reference - 1000)
                .ok()
                .and_then(|index| self.program.objects.get(index))
                .map(|object| {
                    object
                        .members
                        .iter()
                        .map(|(name, value)| match self.program.target(value) {
                            Some(index) => Self::variable(name, "Object", Some(index)),
                            None => Self::variable(name, value, None),
                        })
                        .collect()
                }),
            _ => None,
        };
        match list {
            Some(variables) => self.respond(request, true, &json!({"variables": variables}), None),
            None => self.fail(request, "unknown variables reference"),
        }
    }

    /// A DAP variable; `object` is the structured value it expands to.
    fn variable(name: &str, value: &str, object: Option<usize>) -> Value {
        let kind = if object.is_some() {
            "object"
        } else if value.parse::<i64>().is_ok() {
            "int"
        } else {
            "string"
        };
        json!({
            "name": name,
            "value": value,
            "type": kind,
            "variablesReference": object.map_or(0, |index| 1000 + index),
        })
    }

    fn evaluate(&mut self, request: &Value, args: &Value) {
        let expression = args["expression"].as_str().unwrap_or_default().trim();
        match expression {
            "!cancel" => return self.fail(request, "cancelled"),
            "!hang" => return,
            "!verify" => {
                self.verify_late();
                return self.respond(request, true, &json!({"result": "verified"}), None);
            }
            _ => {}
        }
        let found = self.depth_of(args).and_then(|depth| {
            self.program.frames[depth]
                .locals
                .iter()
                .rev()
                .find(|(name, _)| name == expression)
                .map(|(_, local)| local.clone())
        });
        match found {
            Some(Local::Value(value)) => {
                let body = json!({"result": value, "variablesReference": 0});
                self.respond(request, true, &body, None);
            }
            Some(Local::Object(index)) => {
                let body = json!({
                    "result": "Object",
                    "type": "object",
                    "variablesReference": 1000 + index,
                });
                self.respond(request, true, &body, None);
            }
            None => self.fail(request, &format!("name '{expression}' is not defined")),
        }
    }

    /// With `--hostile-replies`: garbage that is still valid JSON.
    fn hostile_reply(&mut self, request: &Value) {
        let many = |make: fn(usize) -> Value| (0..300).map(make).collect::<Vec<Value>>();
        let body = match request["command"].as_str().unwrap_or_default() {
            "threads" => json!({"threads": "none"}),
            "stackTrace" => json!({"stackFrames": many(|i| json!({
                "id": if i % 2 == 0 { i64::MAX } else { 7 },
                "name": ["not", "text"],
                "source": {"path": "x".repeat(4_200)},
                "line": if i % 3 == 0 { json!("one") } else { json!(-1) },
            }))}),
            "scopes" => json!({"scopes": many(|i| json!({
                "name": format!("scope {i}"),
                "variablesReference": if i % 2 == 0 { -1 } else { 1 },
            }))}),
            "variables" => json!({"variables": many(|i| json!({
                "name": if i % 5 == 0 { json!(null) } else { json!(format!("v{i}")) },
                "value": "y".repeat(1_000),
                "variablesReference": 1,
            }))}),
            _ => json!({"result": 5, "variablesReference": "x"}),
        };
        self.respond(request, true, &body, None);
    }
}

impl Adapter {
    /// Malformed traffic a client must survive.
    fn hostile(&mut self) {
        self.event("stopped", &json!("not an object"));
        self.event("stopped", &json!({"reason": 7, "threadId": -5}));
        self.event("output", &json!({"output": ["not", "text"]}));
        self.event("thread", &Value::Null);
        for body in [
            json!("not an object"),
            json!({"reason": "changed", "breakpoint": "x"}),
            json!({"reason": "changed", "breakpoint": {"id": -1, "verified": false}}),
            json!({"reason": "changed", "breakpoint": {"id": "7", "verified": false}}),
            json!({"reason": 5, "breakpoint": {"id": 1000, "verified": "no"}}),
        ] {
            self.event("breakpoint", &body);
        }
        let seq = self.next_seq();
        self.send(&json!({
            "seq": seq,
            "type": "response",
            "request_seq": 9_999_999,
            "success": true,
            "command": "nothing",
        }));
        let seq = self.next_seq();
        self.send(&json!({"seq": seq, "type": "request", "command": "runInTerminal"}));
        self.send(&json!([1, 2, 3]));
    }
}
