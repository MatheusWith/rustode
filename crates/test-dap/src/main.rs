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
//!   `variables` and `evaluate` with oversized and malformed bodies;
//! - `--log PATH`: append every message received, one JSON object per line.

mod program;

use program::{Local, Mode, Outcome, Program, Special};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Write};

/// What the command line asked for.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // one flag per command-line option
struct Options {
    no_config_done: bool,
    fail_launch: bool,
    no_terminate: bool,
    hostile_replies: bool,
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
    /// The breakpoint lines, 1-based.
    breakpoints: BTreeSet<usize>,
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
        breakpoints: BTreeSet::new(),
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
                });
                self.respond(request, true, &body, None);
            }
            "launch" => self.launch(request, args),
            "setBreakpoints" => self.set_breakpoints(request, args),
            "configurationDone" => {
                self.respond(request, true, &json!({}), None);
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
        self.event("initialized", &json!({}));
        if self.options.no_config_done {
            self.respond(request, true, &json!({}), None);
            self.run(Mode::Continue);
        } else {
            self.launch = Some(request.clone());
        }
    }

    fn set_breakpoints(&mut self, request: &Value, args: &Value) {
        let lines: Vec<usize> = args["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|bp| bp["line"].as_u64())
            .filter_map(|line| usize::try_from(line).ok())
            .collect();
        if args["source"]["path"].as_str() == Some(self.path.as_str()) || self.path.is_empty() {
            self.breakpoints = lines.iter().copied().collect();
        }
        let verified: Vec<Value> = lines
            .iter()
            .map(|line| json!({"verified": true, "line": line}))
            .collect();
        self.respond(request, true, &json!({"breakpoints": verified}), None);
    }

    fn stop(&mut self, reason: &str) {
        self.stopped = true;
        self.event(
            "stopped",
            &json!({"reason": reason, "threadId": 1, "allThreadsStopped": true}),
        );
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
                            "source": {"path": self.path},
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
