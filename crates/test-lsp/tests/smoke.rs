//! The test server answers `initialize`, publishes the scripted diagnostics
//! and exits cleanly after `shutdown` and `exit`. Running this also makes
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
fn it_initializes_publishes_and_shuts_down() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustode-test-lsp"))
        .args(["--encoding", "utf-8"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut input = child.stdin.take().expect("stdin");
    let mut output = BufReader::new(child.stdout.take().expect("stdout"));
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "capabilities": {"general": {"positionEncodings": ["utf-8", "utf-16"]}}}});
    input.write_all(&frame(&initialize)).expect("write");
    let reply = read(&mut output);
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["capabilities"]["positionEncoding"], "utf-8");
    let open = json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": {"uri": "file:///x.rs", "languageId": "rust", "version": 1,
            "text": "é ERROR\nWARN"}}});
    input.write_all(&frame(&open)).expect("write");
    let published = read(&mut output);
    let diagnostics = &published["params"]["diagnostics"];
    assert_eq!(
        diagnostics[0]["range"]["start"]["character"], 3,
        "é is 2 bytes"
    );
    assert_eq!(diagnostics[0]["severity"], 1);
    assert_eq!(diagnostics[1]["range"]["start"]["line"], 1);
    let shutdown = json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"});
    input.write_all(&frame(&shutdown)).expect("write");
    assert_eq!(read(&mut output)["id"], 2);
    let exit = json!({"jsonrpc": "2.0", "method": "exit"});
    input.write_all(&frame(&exit)).expect("write");
    assert!(child.wait().expect("wait").success());
}
