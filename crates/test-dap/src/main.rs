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
//! - `threads` and `stackTrace` answer thread 1 and one frame at the line
//!   the program is stopped on.
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
//! - `--log PATH`: append every message received, one JSON object per line.

use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Write};

/// What the command line asked for.
#[derive(Debug, Default)]
struct Options {
    no_config_done: bool,
    fail_launch: bool,
    no_terminate: bool,
    log: Option<String>,
}

fn options(mut args: impl Iterator<Item = String>) -> Options {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-config-done" => options.no_config_done = true,
            "--fail-launch" => options.fail_launch = true,
            "--no-terminate" => options.no_terminate = true,
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
    /// The lines of the program, once launched.
    program: Vec<String>,
    path: String,
    /// The breakpoint lines, 1-based.
    breakpoints: BTreeSet<usize>,
    /// The next line to run, 0-based.
    next: usize,
    stopped: bool,
    /// Set by `continue`: the line it stopped on runs without stopping again.
    skip: bool,
    /// The `launch` request waiting for `configurationDone`.
    launch: Option<Value>,
}

fn main() {
    let mut adapter = Adapter {
        options: options(std::env::args().skip(1)),
        seq: 0,
        out: io::stdout(),
        program: Vec::new(),
        path: String::new(),
        breakpoints: BTreeSet::new(),
        next: 0,
        stopped: false,
        skip: false,
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
                self.run();
            }
            "continue" => {
                self.stopped = false;
                self.skip = true;
                self.respond(request, true, &json!({"allThreadsContinued": true}), None);
                self.event(
                    "continued",
                    &json!({"threadId": 1, "allThreadsContinued": true}),
                );
                self.run();
            }
            "threads" => {
                let body = json!({"threads": [{"id": 1, "name": "main"}]});
                self.respond(request, true, &body, None);
            }
            "stackTrace" => {
                let line = self.next + 1;
                let body = json!({
                    "stackFrames": [{
                        "id": 1,
                        "name": "main",
                        "source": {"path": self.path},
                        "line": line,
                        "column": 1,
                    }],
                    "totalFrames": 1,
                });
                self.respond(request, true, &body, None);
            }
            "terminate" => {
                self.respond(request, true, &json!({}), None);
                self.event("terminated", &json!({}));
            }
            "disconnect" => {
                self.respond(request, true, &json!({}), None);
                std::process::exit(0);
            }
            other => {
                let message = format!("unsupported request {other}");
                self.respond(request, false, &json!({}), Some(&message));
            }
        }
    }

    fn launch(&mut self, request: &Value, args: &Value) {
        if self.options.fail_launch {
            self.respond(
                request,
                false,
                &json!({}),
                Some("cannot launch the program"),
            );
            return;
        }
        args["program"]
            .as_str()
            .unwrap_or_default()
            .clone_into(&mut self.path);
        match std::fs::read_to_string(&self.path) {
            Ok(text) => self.program = text.lines().map(str::to_owned).collect(),
            Err(err) => {
                let message = format!("cannot read {}: {err}", self.path);
                self.respond(request, false, &json!({}), Some(&message));
                return;
            }
        }
        self.event("initialized", &json!({}));
        if self.options.no_config_done {
            self.respond(request, true, &json!({}), None);
            self.run();
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

    /// Runs lines until a breakpoint or the end of the program.
    fn run(&mut self) {
        while self.next < self.program.len() {
            if !self.skip && self.breakpoints.contains(&(self.next + 1)) {
                self.stopped = true;
                self.event(
                    "stopped",
                    &json!({"reason": "breakpoint", "threadId": 1, "allThreadsStopped": true}),
                );
                return;
            }
            let line = self.program[self.next].clone();
            self.next += 1;
            self.skip = false;
            if line == "CRASH" {
                std::process::exit(3);
            } else if line == "HOSTILE" {
                self.hostile();
            } else if let Some(text) = line.strip_prefix("print ") {
                self.event("output", &json!({"category": "stdout", "output": text}));
            }
        }
        self.event("exited", &json!({"exitCode": 0}));
        self.event("terminated", &json!({}));
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
