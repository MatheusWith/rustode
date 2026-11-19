//! Breakpoints with a condition, a hit count or a log message, and what the
//! adapter said about them. Reimplemented from VS Code's `Breakpoint` of
//! `src/vs/workbench/contrib/debug/common/debugModel.ts:1040-1100` (its
//! `condition`, `hitCondition`, `logMessage`, `verified` and `supported`)
//! and the breakpoint widget of
//! `src/vs/workbench/contrib/debug/browser/breakpointWidget.ts:92-94`,
//! `:160-200`, `:490-505` (three texts, one shown at a time, all kept; an
//! empty one means none).
//!
//! The texts are the user's and the adapter's alone to interpret: the client
//! only keeps them, bounded, and sends them when the adapter's capabilities
//! say it understands them (`supportsConditionalBreakpoints`,
//! `supportsHitConditionalBreakpoints`, `supportsLogPoints`). When it does
//! not, the breakpoint stays a plain one and shows as unverified with the
//! reason, as VS Code shows an unsupported breakpoint
//! (`breakpointsView.ts:1923-1925`).

use super::DebugError;
use super::model::short;
use crate::DocumentId;
use serde_json::{Value, json};

/// Longest condition, hit condition or log message kept.
pub const MAX_BREAKPOINT_TEXT: usize = 1_000;

/// What a breakpoint does besides stopping, in VS Code's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakpointKind {
    /// Stops every time.
    Plain,
    /// Stops when an expression is true.
    Condition,
    /// Stops when the hit count meets a condition.
    HitCount,
    /// Prints a message and does not stop.
    LogPoint,
}

impl BreakpointKind {
    /// The word VS Code uses for it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "Breakpoint",
            Self::Condition => "Conditional Breakpoint",
            Self::HitCount => "Hit Count Breakpoint",
            Self::LogPoint => "Logpoint",
        }
    }
}

/// The three texts of a breakpoint; an empty one is no text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreakpointSpec {
    /// An expression: the program stops when it is true.
    pub condition: Option<String>,
    /// A hit count condition, e.g. `> 5` or `% 2`.
    pub hit_condition: Option<String>,
    /// A message printed instead of stopping; `{expression}` is evaluated.
    pub log_message: Option<String>,
}

impl BreakpointSpec {
    pub(super) fn clean(self) -> Result<Self, DebugError> {
        let one = |text: Option<String>, what: &str| -> Result<Option<String>, DebugError> {
            let Some(text) = text else { return Ok(None) };
            if text.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
                return Err(DebugError::BadBreakpoint(format!(
                    "the {what} must be on one line"
                )));
            }
            let text = text.trim();
            Ok((!text.is_empty()).then(|| short_text(text)))
        };
        Ok(Self {
            condition: one(self.condition, "condition")?,
            hit_condition: one(self.hit_condition, "hit count")?,
            log_message: one(self.log_message, "log message")?,
        })
    }

    /// Which kind it is: a log message makes a logpoint, then a hit count,
    /// then a condition (VS Code's icons do the same,
    /// `breakpointsView.ts:1923-1960`).
    #[must_use]
    pub fn kind(&self) -> BreakpointKind {
        if self.log_message.is_some() {
            BreakpointKind::LogPoint
        } else if self.hit_condition.is_some() {
            BreakpointKind::HitCount
        } else if self.condition.is_some() {
            BreakpointKind::Condition
        } else {
            BreakpointKind::Plain
        }
    }
}

fn short_text(text: &str) -> String {
    text.chars().take(MAX_BREAKPOINT_TEXT).collect()
}

/// A breakpoint as the frontends show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointInfo {
    /// The zero-based line.
    pub line: usize,
    /// Its texts.
    pub spec: BreakpointSpec,
    /// What it is.
    pub kind: BreakpointKind,
    /// Whether the adapter can stop on it. True with no session and until the
    /// adapter says otherwise.
    pub verified: bool,
    /// Why it is not verified, or what the adapter said about it.
    pub message: Option<String>,
}

/// A breakpoint with the state the adapter reported.
#[derive(Debug, Clone, Default)]
pub(super) struct Breakpoint {
    pub spec: BreakpointSpec,
    /// The adapter's id, from the last `setBreakpoints` answer.
    pub id: Option<i64>,
    /// `None` until the adapter answered: shown as verified.
    pub verified: Option<bool>,
    pub message: Option<String>,
}

impl Breakpoint {
    pub(super) fn new(spec: BreakpointSpec) -> Self {
        Self {
            spec,
            ..Self::default()
        }
    }

    /// Forgets what a session said.
    pub(super) fn reset(&mut self) {
        self.id = None;
        self.verified = None;
        self.message = None;
    }

    /// The reason the adapter cannot do what this breakpoint asks, if any.
    pub(super) fn unsupported(&self, caps: Capabilities) -> Option<&'static str> {
        if self.spec.log_message.is_some() && !caps.log_points {
            Some("the debug adapter does not support logpoints")
        } else if self.spec.condition.is_some() && !caps.conditions {
            Some("the debug adapter does not support conditional breakpoints")
        } else if self.spec.hit_condition.is_some() && !caps.hit_conditions {
            Some("the debug adapter does not support hit count conditions")
        } else {
            None
        }
    }

    pub(super) fn info(&self, line: usize, caps: Option<&Capabilities>) -> BreakpointInfo {
        let unsupported = caps.and_then(|caps| self.unsupported(*caps));
        BreakpointInfo {
            line,
            spec: self.spec.clone(),
            kind: self.spec.kind(),
            verified: unsupported.is_none() && self.verified != Some(false),
            message: unsupported
                .map(str::to_owned)
                .or_else(|| self.message.clone()),
        }
    }

    /// The DAP `SourceBreakpoint` for `line` (zero-based): only the texts the
    /// adapter understands.
    pub(super) fn to_dap(&self, line: usize, caps: Capabilities) -> Value {
        let mut bp = json!({"line": line + 1});
        let texts = [
            ("condition", &self.spec.condition, caps.conditions),
            (
                "hitCondition",
                &self.spec.hit_condition,
                caps.hit_conditions,
            ),
            ("logMessage", &self.spec.log_message, caps.log_points),
        ];
        for (key, text, supported) in texts {
            if let Some(text) = text
                && supported
            {
                bp[key] = json!(text);
            }
        }
        bp
    }

    /// What the adapter answered for this breakpoint, or sent in a
    /// `breakpoint` event.
    pub(super) fn apply(&mut self, data: &Value) {
        if let Some(id) = super::model::id(&data["id"]) {
            self.id = Some(id);
        }
        if let Some(verified) = data["verified"].as_bool() {
            self.verified = Some(verified);
        }
        self.message = data["message"].as_str().map(short);
    }
}

/// What the adapter said it can do with breakpoints, from `initialize`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // one flag per capability
pub(super) struct Capabilities {
    pub conditions: bool,
    pub hit_conditions: bool,
    pub log_points: bool,
}

impl Capabilities {
    pub(super) fn parse(body: &Value) -> Self {
        Self {
            conditions: body["supportsConditionalBreakpoints"] == true,
            hit_conditions: body["supportsHitConditionalBreakpoints"] == true,
            log_points: body["supportsLogPoints"] == true,
        }
    }
}

/// Which of the three texts the breakpoint editor shows, in VS Code's order
/// (`breakpointWidget.ts:215-217`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakpointInput {
    /// The condition expression.
    Expression,
    /// The hit count condition.
    HitCount,
    /// The log message.
    LogMessage,
}

impl BreakpointInput {
    /// The name of the choice.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Expression => "Expression",
            Self::HitCount => "Hit Count",
            Self::LogMessage => "Log Message",
        }
    }

    /// What the box says it expects (`breakpointWidget.ts:150-165`).
    #[must_use]
    pub fn hint(self) -> &'static str {
        match self {
            Self::Expression => "Break when the expression is true",
            Self::HitCount => "Break when hit count condition is met",
            Self::LogMessage => "Message to log; expressions within {} are interpolated",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Expression => Self::HitCount,
            Self::HitCount => Self::LogMessage,
            Self::LogMessage => Self::Expression,
        }
    }
}

/// A breakpoint being edited: the three texts, one of them shown. A
/// frontend owns it while its box is open, edits [`BreakpointDraft::text_mut`]
/// and hands it back to [`crate::Workspace::finish_breakpoint_edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointDraft {
    /// The document.
    pub document: DocumentId,
    /// The zero-based line.
    pub line: usize,
    /// Which text is shown.
    pub input: BreakpointInput,
    condition: String,
    hit_count: String,
    log_message: String,
}

impl BreakpointDraft {
    pub(super) fn new(
        document: DocumentId,
        line: usize,
        input: BreakpointInput,
        spec: &BreakpointSpec,
    ) -> Self {
        let text = |t: &Option<String>| t.clone().unwrap_or_default();
        Self {
            document,
            line,
            input,
            condition: text(&spec.condition),
            hit_count: text(&spec.hit_condition),
            log_message: text(&spec.log_message),
        }
    }

    /// The text of the choice that is shown.
    #[must_use]
    pub fn text(&self) -> &str {
        match self.input {
            BreakpointInput::Expression => &self.condition,
            BreakpointInput::HitCount => &self.hit_count,
            BreakpointInput::LogMessage => &self.log_message,
        }
    }

    /// The text of the choice that is shown, to edit.
    pub fn text_mut(&mut self) -> &mut String {
        match self.input {
            BreakpointInput::Expression => &mut self.condition,
            BreakpointInput::HitCount => &mut self.hit_count,
            BreakpointInput::LogMessage => &mut self.log_message,
        }
    }

    /// Replaces the text of the choice that is shown.
    pub fn set_text(&mut self, new: &str) {
        let text = self.text_mut();
        text.clear();
        text.push_str(&short_text(new));
    }

    /// Shows the next choice; the others keep their text.
    pub fn next_input(&mut self) {
        self.input = self.input.next();
    }

    pub(super) fn spec(&self) -> BreakpointSpec {
        let some = |t: &str| Some(t.to_owned());
        BreakpointSpec {
            condition: some(&self.condition),
            hit_condition: some(&self.hit_count),
            log_message: some(&self.log_message),
        }
    }
}
