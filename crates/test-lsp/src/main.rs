//! `rustode-test-lsp`: a language server for rustode's tests only, never
//! shipped. It speaks LSP over stdio with `Content-Length` framing and
//! publishes diagnostics from a scripted rule:
//!
//! - every `ERROR`, `WARN`, `INFO` and `HINT` in a document gets a diagnostic
//!   of that severity over the word, with the message `<word> found` and the
//!   source `test-lsp`; every `EMPTY` gets a warning with an empty range at
//!   the word's start;
//! - a change whose text contains `CRASH` makes the server exit at once with
//!   code 3, without answering anything.
//!
//! Options:
//!
//! - `--sync full|incremental` (default `incremental`): the change sync kind
//!   it asks for; it applies incremental changes to its own copy of the text;
//! - `--encoding utf-8|utf-16|utf-32`: the position encoding it picks when the
//!   client offers it; without this option its reply names none, so UTF-16
//!   applies;
//! - `--publish change|save` (default `change`): publish on open and on every
//!   change, or on open and on save only;
//! - `--crash-on-open`: also crash when an opened document contains `CRASH`;
//! - `--report PATH`: after `initialized`, publish diagnostics for the file at
//!   PATH, read from disk, as a project-wide checker would;
//! - `--log PATH`: append every message received, one JSON object per line,
//!   then `{"synced": URI, "text": TEXT}` after each change it applies.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};

/// What the command line asked for.
#[derive(Debug, Default)]
struct Options {
    full_sync: bool,
    encoding: Option<String>,
    publish_on_change: bool,
    crash_on_open: bool,
    report: Option<String>,
    log: Option<String>,
}

fn options(mut args: impl Iterator<Item = String>) -> Options {
    let mut options = Options {
        publish_on_change: true,
        ..Options::default()
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sync" => options.full_sync = args.next().as_deref() == Some("full"),
            "--encoding" => options.encoding = args.next(),
            "--publish" => options.publish_on_change = args.next().as_deref() != Some("save"),
            "--crash-on-open" => options.crash_on_open = true,
            "--report" => options.report = args.next(),
            "--log" => options.log = args.next(),
            other => eprintln!("rustode-test-lsp: ignoring {other}"),
        }
    }
    options
}

/// The server's state.
struct Server {
    options: Options,
    /// The negotiated position encoding: `utf-8`, `utf-16` or `utf-32`.
    encoding: String,
    documents: HashMap<String, String>,
    shutdown: bool,
    out: io::Stdout,
}

fn main() {
    let options = options(std::env::args().skip(1));
    let mut server = Server {
        options,
        encoding: "utf-16".to_owned(),
        documents: HashMap::new(),
        shutdown: false,
        out: io::stdout(),
    };
    let mut input = BufReader::new(io::stdin().lock());
    while let Some(message) = read_message(&mut input) {
        server.log(&message);
        server.handle(&message);
    }
    std::process::exit(i32::from(!server.shutdown));
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

impl Server {
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

    fn handle(&mut self, message: &Value) {
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = &message["params"];
        match method.as_str() {
            "initialize" => {
                let result = self.initialize(params);
                self.reply(message, &result);
            }
            "initialized" => self.report(),
            "shutdown" => {
                self.shutdown = true;
                self.reply(message, &Value::Null);
            }
            "exit" => std::process::exit(i32::from(!self.shutdown)),
            "textDocument/didOpen" => self.did_open(params),
            "textDocument/didChange" => self.did_change(params),
            "textDocument/didSave" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                self.publish(&uri.to_owned());
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                self.documents.remove(uri);
                self.send_diagnostics(uri, &[]);
            }
            _ if message.get("id").is_some() && !method.is_empty() => {
                let error =
                    json!({"code": -32601, "message": format!("{method} is not supported")});
                let id = message["id"].clone();
                self.send(&json!({"jsonrpc": "2.0", "id": id, "error": error}));
            }
            _ => {}
        }
    }

    fn reply(&mut self, request: &Value, result: &Value) {
        let id = request["id"].clone();
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let offered: Vec<&str> = params["capabilities"]["general"]["positionEncodings"]
            .as_array()
            .map_or_else(Default::default, |list| {
                list.iter().filter_map(Value::as_str).collect()
            });
        let mut capabilities = json!({
            "textDocumentSync": {
                "openClose": true,
                "change": if self.options.full_sync { 1 } else { 2 },
                "save": {"includeText": false},
            },
        });
        if let Some(wanted) = &self.options.encoding
            && offered.contains(&wanted.as_str())
        {
            self.encoding.clone_from(wanted);
            capabilities["positionEncoding"] = json!(wanted);
        }
        json!({"capabilities": capabilities, "serverInfo": {"name": "rustode-test-lsp"}})
    }

    fn report(&mut self) {
        let Some(path) = self.options.report.clone() else {
            return;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let uri = format!("file://{path}");
        let diagnostics = diagnostics(&text, &self.encoding);
        self.send_diagnostics(&uri, &diagnostics);
    }

    fn did_open(&mut self, params: &Value) {
        let document = &params["textDocument"];
        let uri = document["uri"].as_str().unwrap_or_default().to_owned();
        let text = document["text"].as_str().unwrap_or_default().to_owned();
        if self.options.crash_on_open && text.contains("CRASH") {
            std::process::exit(3);
        }
        self.documents.insert(uri.clone(), text);
        self.publish(&uri);
    }

    fn did_change(&mut self, params: &Value) {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut text = self.documents.remove(&uri).unwrap_or_default();
        for change in params["contentChanges"].as_array().into_iter().flatten() {
            let new = change["text"].as_str().unwrap_or_default();
            if new.contains("CRASH") {
                std::process::exit(3);
            }
            match change.get("range") {
                Some(range) => {
                    let start = byte_offset(&text, &range["start"], &self.encoding);
                    let end = byte_offset(&text, &range["end"], &self.encoding);
                    text.replace_range(start..end.max(start), new);
                }
                None => new.clone_into(&mut text),
            }
        }
        self.log(&json!({"synced": uri, "text": text}));
        self.documents.insert(uri.clone(), text);
        if self.options.publish_on_change {
            self.publish(&uri);
        }
    }

    fn publish(&mut self, uri: &String) {
        let diagnostics = self
            .documents
            .get(uri)
            .map_or_else(Default::default, |text| diagnostics(text, &self.encoding));
        self.send_diagnostics(uri, &diagnostics);
    }

    fn send_diagnostics(&mut self, uri: &str, diagnostics: &[Value]) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "diagnostics": diagnostics},
        }));
    }
}

/// The text's lines, split at CRLF, LF and CR as LSP splits them, with the
/// byte offset each one starts at.
fn lines(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push((start, &text[start..i]));
                start = i + 1;
            }
            b'\r' => {
                lines.push((start, &text[start..i]));
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    lines.push((start, &text[start..]));
    lines
}

/// How many code units of `encoding` the char takes.
fn units(c: char, encoding: &str) -> usize {
    match encoding {
        "utf-8" => c.len_utf8(),
        "utf-32" => 1,
        _ => c.len_utf16(),
    }
}

/// The byte offset of an LSP position in `text`.
fn byte_offset(text: &str, position: &Value, encoding: &str) -> usize {
    let line = usize::try_from(position["line"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    let character =
        usize::try_from(position["character"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    let lines = lines(text);
    let Some(&(start, content)) = lines.get(line) else {
        return text.len();
    };
    let mut used = 0;
    for (index, c) in content.char_indices() {
        if used >= character {
            return start + index;
        }
        used += units(c, encoding);
    }
    start + content.len()
}

/// The diagnostics the rule gives `text`.
fn diagnostics(text: &str, encoding: &str) -> Vec<Value> {
    const WORDS: [(&str, u8); 5] = [
        ("ERROR", 1),
        ("WARN", 2),
        ("INFO", 3),
        ("HINT", 4),
        ("EMPTY", 2),
    ];
    let mut found = Vec::new();
    for (number, (_, line)) in lines(text).into_iter().enumerate() {
        for (word, severity) in WORDS {
            for (byte, _) in line.match_indices(word) {
                let start: usize = line[..byte].chars().map(|c| units(c, encoding)).sum();
                let end = if word == "EMPTY" {
                    start
                } else {
                    start + word.len()
                };
                found.push(json!({
                    "range": {
                        "start": {"line": number, "character": start},
                        "end": {"line": number, "character": end},
                    },
                    "severity": severity,
                    "source": "test-lsp",
                    "message": format!("{word} found"),
                }));
            }
        }
    }
    found
}
