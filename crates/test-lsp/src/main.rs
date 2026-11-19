//! `rustode-test-lsp`: a language server for rustode's tests only, never
//! shipped. It speaks LSP over stdio with `Content-Length` framing and
//! publishes diagnostics from a scripted rule:
//!
//! - every `ERROR`, `WARN`, `INFO` and `HINT` in a document gets a diagnostic
//!   of that severity over the word, with the message `<word> found` and the
//!   source `test-lsp`; every `EMPTY` gets a warning with an empty range at
//!   the word's start;
//! - a change whose text contains `CRASH` makes the server exit at once with
//!   code 3, without answering anything;
//! - `textDocument/hover` answers the identifier under the position with a
//!   markdown hover (its name in bold, then a fenced `let` line) and that
//!   word's range; the word `NOHOVER`, or no word, answers `null`;
//! - `textDocument/completion` answers a fixed vocabulary (see [`ITEMS`]):
//!   `label`, `kind`, `detail`, `sortText` and either a `textEdit` over the
//!   identifier before the position or only an `insertText`; right after a
//!   `.` it answers the member items instead ([`MEMBERS`]), and `.` and `:`
//!   are its trigger characters;
//! - `textDocument/definition`, `references`, `rename` and `formatting` work
//!   on the `.rs` files under the workspace root and on a naive rule, see
//!   the [`project`] module: a declaration is a word after `fn ` or `let `,
//!   references and renames cover every whole-word match, and formatting
//!   deletes trailing whitespace;
//! - a request for something else gets error -32601.
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
//! - `--slow MS`: answer hover and completion requests MS milliseconds late,
//!   so a client can cancel them or see the document change first. A
//!   `$/cancelRequest` for one that waits answers it with error -32800;
//! - `--incomplete`: mark completion lists `isIncomplete`;
//! - `--no-hover`, `--no-completion`, `--no-navigation` (definition and
//!   references), `--no-rename`, `--no-formatting`: do not offer that
//!   capability;
//! - `--rename MODE`: `changes`, `stale`, `fileop`, `hostile`, `overlap` or
//!   `null`, a different answer to a rename (see [`Server::rename`]);
//! - `--definition-links`: answer definitions as `LocationLink`s;
//! - `--hostile-format`: add an edit past the end of the document to the
//!   formatting answer;
//! - `--log PATH`: append every message received, one JSON object per line,
//!   then `{"synced": URI, "text": TEXT}` after each change it applies.

mod project;

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What the command line asked for.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // one bool per command-line flag
struct Options {
    full_sync: bool,
    encoding: Option<String>,
    publish_on_change: bool,
    crash_on_open: bool,
    report: Option<String>,
    log: Option<String>,
    slow: Option<Duration>,
    incomplete: bool,
    no_hover: bool,
    no_completion: bool,
    no_navigation: bool,
    no_rename: bool,
    no_formatting: bool,
    rename: Option<String>,
    definition_links: bool,
    hostile_format: bool,
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
            "--slow" => {
                options.slow = args
                    .next()
                    .and_then(|ms| ms.parse().ok())
                    .map(Duration::from_millis);
            }
            "--incomplete" => options.incomplete = true,
            "--no-hover" => options.no_hover = true,
            "--no-completion" => options.no_completion = true,
            "--no-navigation" => options.no_navigation = true,
            "--no-rename" => options.no_rename = true,
            "--no-formatting" => options.no_formatting = true,
            "--rename" => options.rename = args.next(),
            "--definition-links" => options.definition_links = true,
            "--hostile-format" => options.hostile_format = true,
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
    /// The version of each open document, as the client last sent it.
    versions: HashMap<String, i64>,
    /// The workspace root's path, from `initialize`.
    root: Option<String>,
    shutdown: bool,
    out: io::Stdout,
    /// Answers waiting for their time: when, the request id, the result.
    delayed: Vec<(Instant, Value, Value)>,
}

fn main() {
    let options = options(std::env::args().skip(1));
    let mut server = Server {
        options,
        encoding: "utf-16".to_owned(),
        documents: HashMap::new(),
        versions: HashMap::new(),
        root: None,
        shutdown: false,
        out: io::stdout(),
        delayed: Vec::new(),
    };
    // A reader thread feeds the loop, so that late answers are sent on time
    // and a cancel arrives while an answer waits.
    let (messages, incoming) = mpsc::channel();
    std::thread::spawn(move || {
        let mut input = BufReader::new(io::stdin().lock());
        while let Some(message) = read_message(&mut input) {
            if messages.send(message).is_err() {
                break;
            }
        }
    });
    loop {
        server.flush_delayed();
        let next = server.delayed.iter().map(|(due, ..)| *due).min();
        let received = match next {
            Some(due) => incoming.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => incoming
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(message) => {
                server.log(&message);
                server.handle(&message);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => server.flush_delayed(),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
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
                self.versions.remove(uri);
                self.send_diagnostics(uri, &[]);
            }
            "textDocument/hover" if !self.options.no_hover => {
                let result = self.hover(params);
                self.reply_later(message, result);
            }
            "textDocument/completion" if !self.options.no_completion => {
                let result = self.completion(params);
                self.reply_later(message, result);
            }
            "textDocument/definition" if !self.options.no_navigation => {
                let result = self.definition(params);
                self.reply_later(message, result);
            }
            "textDocument/references" if !self.options.no_navigation => {
                let result = self.references(params);
                self.reply_later(message, result);
            }
            "textDocument/rename" if !self.options.no_rename => match self.rename(params) {
                Ok(result) => self.reply_later(message, result),
                Err(error) => {
                    let id = message["id"].clone();
                    self.send(&json!({"jsonrpc": "2.0", "id": id, "error": error}));
                }
            },
            "textDocument/formatting" if !self.options.no_formatting => {
                let result = self.formatting(params);
                self.reply_later(message, result);
            }
            "$/cancelRequest" => self.cancel(&params["id"]),
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

    /// Answers now, or after `--slow` milliseconds.
    fn reply_later(&mut self, request: &Value, result: Value) {
        match self.options.slow {
            Some(wait) => {
                self.delayed
                    .push((Instant::now() + wait, request["id"].clone(), result));
            }
            None => self.reply(request, &result),
        }
    }

    fn flush_delayed(&mut self) {
        let now = Instant::now();
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed)
            .into_iter()
            .partition(|(at, ..)| *at <= now);
        self.delayed = waiting;
        for (_, id, result) in due {
            self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
        }
    }

    /// Cancels a waiting answer: LSP's `RequestCancelled`.
    fn cancel(&mut self, id: &Value) {
        if let Some(at) = self
            .delayed
            .iter()
            .position(|(_, waiting, _)| waiting == id)
        {
            self.delayed.remove(at);
            let error = json!({"code": -32800, "message": "cancelled"});
            self.send(&json!({"jsonrpc": "2.0", "id": id, "error": error}));
        }
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
        if !self.options.no_hover {
            capabilities["hoverProvider"] = json!(true);
        }
        if !self.options.no_completion {
            capabilities["completionProvider"] = json!({"triggerCharacters": [".", ":"]});
        }
        self.root = params["rootUri"]
            .as_str()
            .and_then(|uri| uri.strip_prefix("file://"))
            .map(str::to_owned);
        if !self.options.no_navigation {
            capabilities["definitionProvider"] = json!(true);
            capabilities["referencesProvider"] = json!(true);
        }
        if !self.options.no_rename {
            capabilities["renameProvider"] = json!(true);
        }
        if !self.options.no_formatting {
            capabilities["documentFormattingProvider"] = json!(true);
        }
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
        if let Some(version) = document["version"].as_i64() {
            self.versions.insert(uri.clone(), version);
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
        if let Some(version) = params["textDocument"]["version"].as_i64() {
            self.versions.insert(uri.clone(), version);
        }
        self.log(&json!({"synced": uri, "text": text}));
        self.documents.insert(uri.clone(), text);
        if self.options.publish_on_change {
            self.publish(&uri);
        }
    }

    /// The text of `params`' document and the line and byte of its position.
    fn locate<'a>(&'a self, params: &Value) -> Option<(&'a str, usize, usize)> {
        let text = self
            .documents
            .get(params["textDocument"]["uri"].as_str()?)?;
        let at = byte_offset(text, &params["position"], &self.encoding);
        let line = lines(text)
            .into_iter()
            .rev()
            .find(|(start, _)| *start <= at)?;
        Some((line.1, line.0, at - line.0))
    }

    /// The `textDocument/hover` result.
    fn hover(&self, params: &Value) -> Value {
        let Some((line, _, column)) = self.locate(params) else {
            return Value::Null;
        };
        let Some((start, end)) = word_around(line, column) else {
            return Value::Null;
        };
        let word = &line[start..end];
        if word == "NOHOVER" {
            return Value::Null;
        }
        let number = params["position"]["line"].clone();
        let units_to =
            |byte: usize| -> usize { line[..byte].chars().map(|c| units(c, &self.encoding)).sum() };
        json!({
            "contents": {
                "kind": "markdown",
                "value": format!("**{word}**\n\n```rust\nlet {word}\n```"),
            },
            "range": {
                "start": {"line": number, "character": units_to(start)},
                "end": {"line": number, "character": units_to(end)},
            },
        })
    }

    /// The `textDocument/completion` result.
    fn completion(&self, params: &Value) -> Value {
        let Some((line, _, column)) = self.locate(params) else {
            return Value::Null;
        };
        let start = line[..column]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(column, |(at, _)| at);
        let members = line[..start].ends_with('.');
        let number = params["position"]["line"].clone();
        let units_to =
            |byte: usize| -> usize { line[..byte].chars().map(|c| units(c, &self.encoding)).sum() };
        let range = json!({
            "start": {"line": number, "character": units_to(start)},
            "end": {"line": number, "character": units_to(column)},
        });
        let vocabulary = if members { MEMBERS } else { ITEMS };
        let items: Vec<Value> = vocabulary
            .iter()
            .map(|item| {
                let mut value = json!({
                    "label": item.label,
                    "kind": item.kind,
                    "detail": item.detail,
                    "sortText": item.sort,
                });
                if item.edit {
                    value["textEdit"] = json!({"range": range, "newText": item.insert});
                } else {
                    value["insertText"] = json!(item.insert);
                }
                if let Some(filter) = item.filter {
                    value["filterText"] = json!(filter);
                }
                if item.snippet {
                    value["insertTextFormat"] = json!(2);
                }
                value
            })
            .collect();
        json!({"isIncomplete": self.options.incomplete, "items": items})
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

/// One completion the server offers.
struct Item {
    label: &'static str,
    /// LSP `CompletionItemKind`.
    kind: u8,
    detail: &'static str,
    sort: &'static str,
    insert: &'static str,
    /// Whether it carries a `textEdit` over the word; else only `insertText`.
    edit: bool,
    filter: Option<&'static str>,
    snippet: bool,
}

const fn item(
    label: &'static str,
    kind: u8,
    detail: &'static str,
    sort: &'static str,
    insert: &'static str,
    edit: bool,
) -> Item {
    Item {
        label,
        kind,
        detail,
        sort,
        insert,
        edit,
        filter: None,
        snippet: false,
    }
}

/// The vocabulary offered anywhere but after a `.`.
const ITEMS: &[Item] = &[
    item("println", 3, "macro println!", "1", "println!()", true),
    item("print", 3, "fn print()", "2", "print", false),
    item("process", 3, "fn process()", "3", "process", true),
    item("Point", 22, "struct Point", "4", "Point", true),
    item(
        "point_count",
        6,
        "let point_count: usize",
        "5",
        "point_count",
        true,
    ),
    item(
        "PrimaryKey",
        7,
        "trait PrimaryKey",
        "6",
        "PrimaryKey",
        false,
    ),
    Item {
        filter: Some("tuple"),
        ..item("pair", 6, "let pair: (i32, i32)", "7", "pair", true)
    },
    Item {
        snippet: true,
        ..item(
            "fmt",
            15,
            "snippet",
            "8",
            "fmt(${1:value}, ${2|a,b|}$0)",
            true,
        )
    },
];

/// The vocabulary offered right after a `.`.
const MEMBERS: &[Item] = &[
    item("len", 2, "fn len(&self) -> usize", "1", "len()", true),
    item("push", 2, "fn push(&mut self, value)", "2", "push", true),
    item("pop", 2, "fn pop(&mut self)", "3", "pop", true),
];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier around byte `column` of `line`, as byte bounds.
fn word_around(line: &str, column: usize) -> Option<(usize, usize)> {
    let start = line[..column]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(column, |(at, _)| at);
    let end = line[column..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(line.len(), |(at, _)| column + at);
    (start < end).then_some((start, end))
}
