//! The test adapter answers `initialize`, runs a program to its breakpoint,
//! continues to the end and disconnects. Running this also makes
//! `cargo nextest run --workspace` build the binary that the workspace and
//! frontend tests start.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn read(output: &mut impl BufRead) -> Value {
    let mut length = 0;
    loop {
        let mut line = String::new();
        output.read_line(&mut line).expect("header");
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().expect("length");
        }
    }
    let mut body = vec![0; length];
    output.read_exact(&mut body).expect("body");
    serde_json::from_slice(&body).expect("json")
}

#[test]
fn it_runs_to_a_breakpoint_and_on_to_the_end() {
    let dir = std::env::temp_dir().join(format!("rustode-test-dap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let program = dir.join("program.txt");
    std::fs::write(&program, "print one\nprint two\nprint three\n").expect("program");
    let program = program.display().to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_rustode-test-dap"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut output = BufReader::new(child.stdout.take().expect("stdout"));
    let mut send = |seq: i64, command: &str, arguments: Value| {
        let request =
            json!({"seq": seq, "type": "request", "command": command, "arguments": arguments});
        stdin.write_all(&frame(&request)).expect("write");
        stdin.flush().expect("flush");
    };

    send(1, "initialize", json!({}));
    assert_eq!(
        read(&mut output)["body"]["supportsConfigurationDoneRequest"],
        true
    );
    send(2, "launch", json!({"program": program}));
    assert_eq!(read(&mut output)["event"], "initialized");
    send(
        3,
        "setBreakpoints",
        json!({"source": {"path": program}, "breakpoints": [{"line": 2}]}),
    );
    assert_eq!(
        read(&mut output)["body"]["breakpoints"][0]["verified"],
        true
    );
    send(4, "configurationDone", json!({}));
    assert_eq!(read(&mut output)["command"], "configurationDone");
    assert_eq!(read(&mut output)["command"], "launch");
    assert_eq!(read(&mut output)["event"], "output");
    let stopped = read(&mut output);
    assert_eq!(stopped["event"], "stopped");
    assert_eq!(stopped["body"]["reason"], "breakpoint");
    send(5, "stackTrace", json!({"threadId": 1}));
    assert_eq!(read(&mut output)["body"]["stackFrames"][0]["line"], 2);
    send(6, "continue", json!({"threadId": 1}));
    let mut events = Vec::new();
    while events.last() != Some(&"terminated".to_owned()) {
        let message = read(&mut output);
        if let Some(event) = message["event"].as_str() {
            events.push(event.to_owned());
        }
    }
    assert_eq!(
        events,
        ["continued", "output", "output", "exited", "terminated"]
    );
    send(7, "disconnect", json!({}));
    assert_eq!(read(&mut output)["command"], "disconnect");
    assert!(child.wait().expect("exit").success());
    std::fs::remove_dir_all(&dir).ok();
}
