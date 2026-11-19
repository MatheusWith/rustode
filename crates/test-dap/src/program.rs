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
//! - `CRASH` and `HOSTILE` as in the module documentation of `main.rs`.

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
    pub fn run(
        &mut self,
        mode: Mode,
        breakpoints: &std::collections::BTreeSet<usize>,
        out: &mut Vec<Output>,
    ) -> Outcome {
        let start_depth = self.frames.len();
        let mut first = true;
        while self.next < self.lines.len() {
            let skip = std::mem::take(&mut self.skip) || (first && mode != Mode::Continue);
            if !skip && breakpoints.contains(&(self.next + 1)) {
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
