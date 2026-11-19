//! What a stopped program shows: threads, the call stack, the variables
//! tree, the watch expressions and the debug console, as VS Code's
//! `debugModel.ts` keeps them (`src/vs/workbench/contrib/debug/common/debugModel.ts:36-60`,
//! `:454-500`, `:501-600`; reimplemented, see `ATTRIBUTION.md`).
//!
//! Everything here is built from an adapter's replies, so every size is
//! bounded: a reply is cut at [`MAX_CHILDREN`] entries, a tree holds at most
//! [`MAX_NODES`] nodes and is never expanded deeper than [`MAX_DEPTH`], the
//! call stack at most [`MAX_FRAMES`] frames, and the console [`MAX_CONSOLE`]
//! lines.

use serde_json::Value;
use std::path::PathBuf;

/// Frames kept from a `stackTrace` reply.
pub const MAX_FRAMES: usize = 100;
/// Threads kept from a `threads` reply.
pub const MAX_THREADS: usize = 64;
/// Scopes kept from a `scopes` reply.
pub const MAX_SCOPES: usize = 8;
/// Variables kept from one `variables` reply.
pub const MAX_CHILDREN: usize = 200;
/// How deep a variable can be expanded; deeper ones show no arrow.
pub const MAX_DEPTH: usize = 8;
/// Nodes one tree (the scopes, or the watch expressions) can hold.
pub const MAX_NODES: usize = 2_000;
/// Watch expressions.
pub const MAX_WATCHES: usize = 32;
/// Console lines kept; the oldest go first.
pub const MAX_CONSOLE: usize = 1_000;
/// Longest name, value or message kept from an adapter.
pub const MAX_TEXT: usize = 500;

/// A thread of the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadInfo {
    /// The adapter's id.
    pub id: i64,
    /// Its name.
    pub name: String,
}

/// A frame of the call stack, innermost first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    /// The adapter's id, used to ask for its scopes.
    pub id: i64,
    /// The function name.
    pub name: String,
    /// The source file, absolute, when the frame has one.
    pub path: Option<PathBuf>,
    /// The zero-based line (0 when the frame has no source).
    pub line: usize,
}

/// Which tree of values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugTree {
    /// The scopes of the selected frame and their variables.
    Variables,
    /// The watch expressions and their results.
    Watch,
}

/// One visible row of a [`DebugTree`], flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableRow {
    /// The index of the top-level entry the row belongs to: the scope, or
    /// the watch expression.
    pub root: usize,
    /// How deep the row is: 0 for a scope or a watch expression.
    pub depth: usize,
    /// The variable's name, or the watch expression.
    pub name: String,
    /// Its value; empty for a scope.
    pub value: String,
    /// Its type, when the adapter said.
    pub kind: Option<String>,
    /// Whether it can be expanded.
    pub expandable: bool,
    /// Whether it is expanded.
    pub expanded: bool,
    /// Whether its children were asked for and have not arrived.
    pub loading: bool,
}

/// What a console line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleKind {
    /// An expression the user typed.
    Input,
    /// The program's or the adapter's output.
    Output,
    /// The program's standard error, or a failed evaluation.
    Error,
    /// The result of an evaluation.
    Result,
}

/// One line of the debug console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// What it is.
    pub kind: ConsoleKind,
    /// The text, without a line break.
    pub text: String,
}

/// A variable, scope or evaluation result and what is below it.
#[derive(Debug, Clone, Default)]
pub(super) struct Node {
    /// Identifies a watch expression across evaluations; 0 for the rest.
    pub id: u64,
    pub name: String,
    pub value: String,
    pub kind: Option<String>,
    /// The adapter's `variablesReference`; 0 when there is nothing below.
    pub reference: i64,
    pub expanded: bool,
    pub loading: bool,
    /// `None` until they were asked for.
    pub children: Option<Vec<Node>>,
}

/// What a watch expression shows with no value, as VS Code's
/// `Expression.DEFAULT_VALUE` (`debugModel.ts:305-330`).
const NOT_AVAILABLE: &str = "not available";

/// A text cut to what is worth keeping.
pub(super) fn short(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

/// A `variablesReference` an adapter sent, if it names something.
pub(super) fn reference(value: &Value) -> i64 {
    value
        .as_i64()
        .filter(|r| (1..=i64::from(i32::MAX)).contains(r))
        .unwrap_or(0)
}

/// An id an adapter sent, if it is a sane one.
pub(super) fn id(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|id| (0..=i64::from(i32::MAX)).contains(id))
}

impl Node {
    pub(super) fn new(name: &str, value: &str, kind: Option<&str>, reference: i64) -> Self {
        Self {
            name: short(name),
            value: short(value),
            kind: kind.map(short),
            reference,
            ..Self::default()
        }
    }

    /// A watch expression's node before it is evaluated.
    pub(super) fn watch(id: u64, expression: &str) -> Self {
        let mut node = Self::new(expression, "", None, 0);
        node.id = id;
        node.reset();
        node
    }

    /// Back to "not available": nothing evaluated, nothing below.
    pub(super) fn reset(&mut self) {
        NOT_AVAILABLE.clone_into(&mut self.value);
        self.kind = None;
        self.reference = 0;
        self.expanded = false;
        self.loading = false;
        self.children = None;
    }

    fn count(&self) -> usize {
        1 + self
            .children
            .iter()
            .flatten()
            .map(Node::count)
            .sum::<usize>()
    }

    fn expandable(&self, depth: usize) -> bool {
        match &self.children {
            Some(children) => !children.is_empty(),
            None => self.reference > 0 && depth < MAX_DEPTH,
        }
    }
}

/// How many nodes the trees hold.
pub(super) fn count(nodes: &[Node]) -> usize {
    nodes.iter().map(Node::count).sum()
}

/// The visible rows of `nodes`.
pub(super) fn rows(nodes: &[Node]) -> Vec<VariableRow> {
    fn walk(node: &Node, root: usize, depth: usize, out: &mut Vec<VariableRow>) {
        out.push(VariableRow {
            root,
            depth,
            name: node.name.clone(),
            value: node.value.clone(),
            kind: node.kind.clone(),
            expandable: node.expandable(depth),
            expanded: node.expanded,
            loading: node.loading,
        });
        if node.expanded {
            for child in node.children.iter().flatten() {
                walk(child, root, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for (root, node) in nodes.iter().enumerate() {
        walk(node, root, 0, &mut out);
    }
    out
}

/// The node on visible row `row`, and its depth.
pub(super) fn at(nodes: &mut [Node], row: usize) -> Option<(&mut Node, usize)> {
    fn walk<'a>(
        nodes: &'a mut [Node],
        depth: usize,
        row: &mut usize,
    ) -> Option<(&'a mut Node, usize)> {
        for node in nodes {
            if *row == 0 {
                return Some((node, depth));
            }
            *row -= 1;
            if node.expanded
                && let Some(children) = node.children.as_mut()
                && let Some(found) = walk(children, depth + 1, row)
            {
                return Some(found);
            }
        }
        None
    }
    let mut row = row;
    walk(nodes, 0, &mut row)
}

/// Gives the nodes waiting for `reference` their children, parsed from a
/// `variables` reply body (`None` when the reply failed or was cancelled:
/// they end up empty). Returns whether any node was waiting.
pub(super) fn fill(
    nodes: &mut [Node],
    reference: i64,
    body: Option<&Value>,
    budget: usize,
) -> bool {
    let mut filled = false;
    for node in nodes {
        if node.loading && node.reference == reference {
            node.children = Some(parse_children(body, budget));
            node.loading = false;
            filled = true;
        } else if let Some(children) = node.children.as_mut() {
            filled |= fill(children, reference, body, budget);
        }
    }
    filled
}

/// The variables of a reply, at most [`MAX_CHILDREN`] and `budget`.
fn parse_children(body: Option<&Value>, budget: usize) -> Vec<Node> {
    body.and_then(|b| b["variables"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| {
            Some(Node::new(
                v["name"].as_str()?,
                v["value"].as_str().unwrap_or_default(),
                v["type"].as_str(),
                reference(&v["variablesReference"]),
            ))
        })
        .take(MAX_CHILDREN.min(budget))
        .collect()
}

/// The scopes of a `scopes` reply: name and reference, whether the adapter
/// said reading it is expensive.
pub(super) fn parse_scopes(body: &Value) -> Vec<(Node, bool)> {
    body["scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let reference = reference(&s["variablesReference"]);
            if reference == 0 {
                return None;
            }
            let node = Node::new(s["name"].as_str()?, "", None, reference);
            Some((node, s["expensive"] == true))
        })
        .take(MAX_SCOPES)
        .collect()
}

/// The threads of a `threads` reply.
pub(super) fn parse_threads(body: &Value) -> Vec<ThreadInfo> {
    body["threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(ThreadInfo {
                id: id(&t["id"])?,
                name: short(t["name"].as_str().unwrap_or("thread")),
            })
        })
        .take(MAX_THREADS)
        .collect()
}

/// The frames of a `stackTrace` reply, with relative paths made absolute
/// against `root`.
pub(super) fn parse_frames(body: &Value, root: &std::path::Path) -> Vec<FrameInfo> {
    body["stackFrames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let path = f["source"]["path"]
                .as_str()
                .filter(|p| p.len() <= 4_096)
                .map(|p| super::resolve(root, p));
            let line = f["line"]
                .as_u64()
                .filter(|l| *l >= 1)
                .and_then(|l| usize::try_from(l - 1).ok());
            Some(FrameInfo {
                id: id(&f["id"])?,
                name: short(f["name"].as_str().unwrap_or("<unknown>")),
                path: path.filter(|_| line.is_some()),
                line: line.unwrap_or(0),
            })
        })
        .take(MAX_FRAMES)
        .collect()
}
