//! The "program" the test adapter runs: a text file, one statement per
//! line. It is deterministic, so a test knows the call stack, the variables
//! and the order of events at every stop.
//!
//! Statements (anything else is a no-op):
//!
//! - `print TEXT`, `error TEXT`, `console TEXT`: an `output` event on the
//!   `stdout`, `stderr` or `console` category;
//! - `enter NAME` and `leave`: push and pop a stack frame named `NAME` (the
//!   program starts in the frame `main`);
//! - `var NAME VALUE`: a local of the current frame;
//! - `obj NAME K=V K=V…`: a structured local; a value `@OTHER` is the object
//!   `OTHER` (declared earlier, or the object itself), so objects nest and
//!   can point at themselves;
//! - `global NAME VALUE`: a variable of the `Globals` scope, which the
//!   adapter marks `expensive`;
//! - `WAIT`: the program runs and waits until the client sends `pause`;
//! - `loop N` and `end`: the lines between run `N` times (at most 1,000);
//! - `inc NAME`: adds one to a local of the current frame (it becomes 1 when
//!   there is none), so a condition can tell the iterations apart;
//! - `CRASH` and `HOSTILE` as in the module documentation of `main.rs`.
//!
//! A breakpoint may carry a condition, a hit condition and a log message
//! ([`Spec`]). They are evaluated here, the same way each time:
//!
//! - a condition is `NAME`, true unless its value is empty, `0` or `false`,
//!   or `LEFT OP RIGHT` with `OP` one of `==`, `!=`, `>=`, `<=`, `>`, `<`,
//!   each side a variable of the innermost frame (then globals) or a literal
//!   (quotes are stripped); numbers compare as numbers. A name that is not
//!   defined is an error: the program stops anyway and an `error` line says
//!   why, as debugpy does;
//! - the hit count of a line goes up each time it is reached with the
//!   condition true; the hit condition is `N` or `== N` (the Nth time),
//!   `>`, `>=`, `<`, `<=` against `N`, or `% N` (every Nth time);
//! - a log message prints (category `console`, `{NAME}` replaced by the
//!   variable's value) and does not stop the program.

/// What a stopped program is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Local {
    /// A plain value.
    Value(String),
    /// An index into [`Program::objects`].
    Object(usize),
}

/// One frame of the call stack.
#[derive(Debug)]
pub struct Frame {
    pub name: String,
    /// The zero-based line of the `enter` that made the frame.
    pub call_line: usize,
    pub locals: Vec<(String, Local)>,
}

/// A structured value: `(member, value)` pairs.
#[derive(Debug)]
pub struct Object {
    pub name: String,
    pub members: Vec<(String, String)>,
}

/// What the client set on a breakpoint line besides the line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spec {
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

/// The breakpoints, by one-based line.
pub type Breakpoints = std::collections::BTreeMap<usize, Spec>;

/// A `loop` being run.
#[derive(Debug)]
struct Loop {
    start: usize,
    remaining: usize,
}

/// How far to run before stopping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Continue,
    In,
    Over,
    Out,
}

/// Why `run` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Stopped before the next line, for this reason.
    Stopped(&'static str),
    /// Waiting for `pause` at a `WAIT` line.
    Waiting,
    /// Past the last line.
    Finished,
    /// A `HOSTILE` or `CRASH` line asks the adapter to act.
    Special(Special),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    Crash,
    Hostile,
}

/// Something the program printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub category: &'static str,
    pub text: String,
}

/// The program and where it is.
#[derive(Debug, Default)]
pub struct Program {
    pub lines: Vec<String>,
    /// The next line to run, zero-based.
    pub next: usize,
    pub frames: Vec<Frame>,
    pub objects: Vec<Object>,
    pub globals: Vec<(String, String)>,
    /// `WAIT` lines the client already paused at.
    waited: Vec<usize>,
    /// Set after a stop: the line it stopped on runs without stopping again.
    pub skip: bool,
    /// How many times each breakpoint line (one-based) was reached with its
    /// condition true.
    pub hits: std::collections::BTreeMap<usize, u64>,
    loops: Vec<Loop>,
}

impl Program {
    pub fn load(text: &str) -> Self {
        Self {
            lines: text.lines().map(str::to_owned).collect(),
            frames: vec![Frame {
                name: "main".to_owned(),
                call_line: 0,
                locals: Vec::new(),
            }],
            ..Self::default()
        }
    }

    /// Runs lines as `mode` says; `breakpoints` are one-based. `out` gets
    /// everything printed on the way.
    pub fn run(&mut self, mode: Mode, breakpoints: &Breakpoints, out: &mut Vec<Output>) -> Outcome {
        let start_depth = self.frames.len();
        let mut first = true;
        while self.next < self.lines.len() {
            let skip = std::mem::take(&mut self.skip) || (first && mode != Mode::Continue);
            if !skip
                && let Some(spec) = breakpoints.get(&(self.next + 1))
                && self.breakpoint_stops(self.next + 1, spec, out)
            {
                return Outcome::Stopped("breakpoint");
            }
            first = false;
            let line = self.lines[self.next].clone();
            if line == "WAIT" && !self.waited.contains(&self.next) {
                return Outcome::Waiting;
            }
            self.next += 1;
            if let Some(special) = self.execute(&line, out) {
                return Outcome::Special(special);
            }
            let depth = self.frames.len();
            let done = match mode {
                Mode::Continue => false,
                Mode::In => true,
                Mode::Over => depth <= start_depth,
                Mode::Out => depth < start_depth,
            };
            if done && self.next < self.lines.len() {
                return Outcome::Stopped("step");
            }
        }
        Outcome::Finished
    }

    /// Whether the breakpoint on `line` (one-based) stops the program now:
    /// its condition holds and its hit condition is met. A log message
    /// prints instead of stopping.
    fn breakpoint_stops(&mut self, line: usize, spec: &Spec, out: &mut Vec<Output>) -> bool {
        if let Some(condition) = &spec.condition {
            match self.condition(condition) {
                Ok(false) => return false,
                Ok(true) => {}
                Err(why) => {
                    out.push(output("stderr", &format!("condition error: {why}")));
                    return true;
                }
            }
        }
        let count = self.hits.entry(line).or_insert(0);
        *count += 1;
        let count = *count;
        if let Some(hit) = &spec.hit_condition {
            match hit_met(count, hit) {
                Ok(true) => {}
                Ok(false) => return false,
                Err(why) => {
                    out.push(output("stderr", &format!("hit condition error: {why}")));
                    return true;
                }
            }
        }
        if let Some(message) = &spec.log_message {
            let text = self.interpolate(message);
            out.push(output("console", &text));
            return false;
        }
        true
    }

    /// The value of a variable of the innermost frame, then the globals.
    fn lookup(&self, name: &str) -> Option<String> {
        let local = self.frames.last().and_then(|frame| {
            frame
                .locals
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, local)| match local {
                    Local::Value(value) => value.clone(),
                    Local::Object(_) => "Object".to_owned(),
                })
        });
        local.or_else(|| {
            self.globals
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, value)| value.clone())
        })
    }

    /// A side of a comparison: a variable's value or the literal itself.
    fn operand(&self, token: &str) -> String {
        let token = token.trim();
        self.lookup(token)
            .unwrap_or_else(|| token.trim_matches(['"', '\'']).to_owned())
    }

    fn condition(&self, text: &str) -> Result<bool, String> {
        const OPS: [&str; 6] = ["==", "!=", ">=", "<=", ">", "<"];
        for op in OPS {
            if let Some((left, right)) = text.split_once(op) {
                let (left, right) = (self.operand(left), self.operand(right));
                if let (Ok(a), Ok(b)) = (left.parse::<i64>(), right.parse::<i64>()) {
                    return Ok(compare(op, a.cmp(&b)));
                }
                return match op {
                    "==" => Ok(left == right),
                    "!=" => Ok(left != right),
                    _ => Err(format!("cannot order '{left}' and '{right}'")),
                };
            }
        }
        let name = text.trim();
        match self.lookup(name) {
            Some(value) => Ok(!matches!(value.as_str(), "" | "0" | "false")),
            None => Err(format!("name '{name}' is not defined")),
        }
    }

    /// `message` with each `{NAME}` replaced by the variable's value.
    fn interpolate(&self, message: &str) -> String {
        let mut result = String::new();
        let mut rest = message;
        while let Some(start) = rest.find('{') {
            result.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let Some((name, tail)) = after.split_once('}') else {
                result.push_str(&rest[start..]);
                rest = "";
                break;
            };
            result.push_str(
                &self
                    .lookup(name.trim())
                    .unwrap_or_else(|| format!("{{{name}}}")),
            );
            rest = tail;
        }
        result.push_str(rest);
        result
    }

    /// The client paused a program that waits: it stops on the `WAIT`.
    pub fn pause(&mut self, line: usize) {
        self.waited.push(line);
    }

    fn execute(&mut self, line: &str, out: &mut Vec<Output>) -> Option<Special> {
        let index = self.next - 1;
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "CRASH" => return Some(Special::Crash),
            "HOSTILE" => return Some(Special::Hostile),
            "print" => out.push(output("stdout", rest)),
            "error" => out.push(output("stderr", rest)),
            "console" => out.push(output("console", rest)),
            "enter" => self.frames.push(Frame {
                name: rest.to_owned(),
                call_line: index,
                locals: Vec::new(),
            }),
            "leave" if self.frames.len() > 1 => drop(self.frames.pop()),
            "var" => self.declare_value(rest),
            "obj" => self.declare_object(rest),
            "loop" => self.loops.push(Loop {
                start: index,
                remaining: rest.trim().parse::<usize>().unwrap_or(1).clamp(1, 1_000),
            }),
            "end" => {
                if let Some(current) = self.loops.last_mut() {
                    current.remaining -= 1;
                    if current.remaining > 0 {
                        self.next = current.start + 1;
                    } else {
                        self.loops.pop();
                    }
                }
            }
            "inc" => {
                let name = rest.trim();
                if let Some(frame) = self.frames.last_mut() {
                    match frame.locals.iter_mut().rev().find(|(n, _)| n == name) {
                        Some((_, Local::Value(value))) => {
                            *value = value
                                .parse::<i64>()
                                .unwrap_or(0)
                                .saturating_add(1)
                                .to_string();
                        }
                        Some(_) => {}
                        None => frame
                            .locals
                            .push((name.to_owned(), Local::Value("1".to_owned()))),
                    }
                }
            }
            "global" => {
                let (name, value) = rest.split_once(' ').unwrap_or((rest, ""));
                self.globals.push((name.to_owned(), value.to_owned()));
            }
            _ => {}
        }
        None
    }

    fn declare_value(&mut self, rest: &str) {
        let (name, value) = rest.split_once(' ').unwrap_or((rest, ""));
        if let Some(frame) = self.frames.last_mut() {
            frame
                .locals
                .push((name.to_owned(), Local::Value(value.to_owned())));
        }
    }

    fn declare_object(&mut self, rest: &str) {
        let mut parts = rest.split_whitespace();
        let name = parts.next().unwrap_or_default().to_owned();
        let members = parts
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        self.objects.push(Object {
            name: name.clone(),
            members,
        });
        let index = self.objects.len() - 1;
        if let Some(frame) = self.frames.last_mut() {
            frame.locals.push((name, Local::Object(index)));
        }
    }

    /// The object a member value points at, `@NAME`.
    pub fn target(&self, value: &str) -> Option<usize> {
        let name = value.strip_prefix('@')?;
        self.objects.iter().rposition(|o| o.name == name)
    }

    /// The frames from the innermost out, as `(depth, frame, line)` with the
    /// one-based line each is at.
    pub fn stack(&self) -> Vec<(usize, &Frame, usize)> {
        let mut line = self.next + 1;
        let mut stack = Vec::new();
        for (depth, frame) in self.frames.iter().enumerate().rev() {
            stack.push((depth, frame, line));
            line = frame.call_line + 1;
        }
        stack
    }
}

fn output(category: &'static str, text: &str) -> Output {
    Output {
        category,
        text: text.to_owned(),
    }
}

fn compare(op: &str, ordering: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match op {
        "==" => ordering == Equal,
        "!=" => ordering != Equal,
        ">=" => ordering != Less,
        "<=" => ordering != Greater,
        ">" => ordering == Greater,
        _ => ordering == Less,
    }
}

/// Whether the hit `count` meets `condition`: `N`, `== N`, `> N`, `>= N`,
/// `< N`, `<= N` or `% N`.
fn hit_met(count: u64, condition: &str) -> Result<bool, String> {
    let condition = condition.trim();
    let (op, number) = ["==", ">=", "<=", ">", "<", "%"]
        .iter()
        .find_map(|op| condition.strip_prefix(op).map(|n| (*op, n)))
        .unwrap_or(("==", condition));
    let n: u64 = number
        .trim()
        .parse()
        .map_err(|_| format!("'{condition}' is not a hit count condition"))?;
    Ok(match op {
        "==" => count == n,
        ">=" => count >= n,
        "<=" => count <= n,
        ">" => count > n,
        "<" => count < n,
        _ => n != 0 && count.is_multiple_of(n),
    })
}
