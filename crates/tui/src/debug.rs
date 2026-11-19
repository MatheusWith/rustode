//! The debug panel, below the editor while a session runs: four columns,
//! CALL STACK, VARIABLES, WATCH and DEBUG CONSOLE, as VS Code's debug views
//! and Debug Console list them
//! (`src/vs/workbench/contrib/debug/browser/callStackView.ts`,
//! `variablesView.ts`, `watchExpressionsView.ts`, `repl.ts`; studied and
//! reimplemented, see `ATTRIBUTION.md`). All state is the workspace's; the
//! panel keeps only which column has focus, the selected rows and the text
//! being typed.
//!
//! Keys, with the panel focused: Tab and Shift+Tab move between the columns,
//! Up, Down, Page Up, Page Down, Home and End move in one, Right and Left
//! open and close a variable, Enter picks a frame, opens or closes a
//! variable, adds the typed watch expression or evaluates the typed console
//! expression, Delete removes the selected watch expression, typing edits the
//! text of the Watch and Console columns, Escape returns to the editor. F10,
//! F11, Shift+F11 and F6 (pause while the program runs) work from anywhere
//! (`workbench.action.debug.stepOver` and the rest); the commands "Debug:
//! Focus on … View" and "Debug: Add Watch Expression" focus a column.

use crate::app::{App, Focus};
use crate::keys;
use crate::view::to_u16;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use rustode_workspace::{ConsoleKind, DebugState, DebugTree, MarkerSeverity, VariableRow};

/// A column of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Pane {
    #[default]
    CallStack,
    Variables,
    Watch,
    Console,
}

const PANES: [Pane; 4] = [Pane::CallStack, Pane::Variables, Pane::Watch, Pane::Console];

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Self::CallStack => " CALL STACK",
            Self::Variables => " VARIABLES",
            Self::Watch => " WATCH",
            Self::Console => " DEBUG CONSOLE",
        }
    }

    fn step(self, forward: bool) -> Self {
        let at = PANES.iter().position(|p| *p == self).unwrap_or(0);
        let next = if forward {
            (at + 1) % PANES.len()
        } else {
            (at + PANES.len() - 1) % PANES.len()
        };
        PANES[next]
    }

    /// The index of a list column in the per-column arrays.
    fn list(self) -> Option<usize> {
        match self {
            Self::CallStack => Some(0),
            Self::Variables => Some(1),
            Self::Watch => Some(2),
            Self::Console => None,
        }
    }
}

/// The panel's state, kept while it is hidden.
#[derive(Debug, Default)]
pub(crate) struct DebugPanel {
    /// Shown even with no session, after a focus command.
    pub pinned: bool,
    pub pane: Pane,
    /// The selected row of each list column.
    selected: [usize; 3],
    /// The first row shown of each list column.
    top: [usize; 3],
    watch_input: String,
    console_input: String,
}

/// Whether the panel is shown: while there is a session, or when pinned.
pub(crate) fn visible(app: &App) -> bool {
    app.debug.pinned || app.workspace.debug_status().state != DebugState::Inactive
}

/// The panel's height for an editor area `height` rows tall: half of it, at
/// least 6 rows, leaving the editor at least 3.
pub(crate) fn height(height: u16) -> u16 {
    (height / 2).max(6).min(height.saturating_sub(3))
}

/// A row of the call stack column: the frame it stands for, if any.
struct StackLine {
    frame: Option<usize>,
    line: Line<'static>,
}

fn stack_lines(app: &App) -> Vec<StackLine> {
    let ws = &app.workspace;
    let status = ws.debug_status();
    let frames = ws.debug_frames();
    let current = ws.debug_selected_frame();
    let mut lines = Vec::new();
    for thread in ws.debug_threads() {
        let stopped = status.state == DebugState::Stopped && status.thread == Some(thread.id);
        let state = if stopped {
            format!(
                "  PAUSED ON {}",
                status.reason.as_deref().unwrap_or("stop").to_uppercase()
            )
        } else if status.state == DebugState::Stopped {
            String::new()
        } else {
            "  RUNNING".to_owned()
        };
        lines.push(StackLine {
            frame: None,
            line: Line::styled(
                format!(" {}{state}", thread.name),
                Style::new().add_modifier(ratatui::style::Modifier::BOLD),
            ),
        });
        if stopped {
            for (index, frame) in frames.iter().enumerate() {
                let marker = if index == current { "▶" } else { " " };
                let place = frame.path.as_ref().map_or_else(String::new, |path| {
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    format!("  {name}:{}", frame.line + 1)
                });
                lines.push(StackLine {
                    frame: Some(index),
                    line: Line::raw(format!("  {marker}{}{place}", frame.name)),
                });
            }
        }
    }
    lines
}

fn variable_line(row: &VariableRow) -> Line<'static> {
    let marker = match (row.expandable, row.expanded) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        (false, _) => "  ",
    };
    let value = if row.value.is_empty() {
        String::new()
    } else {
        format!(": {}", row.value)
    };
    let loading = if row.loading { " …" } else { "" };
    Line::raw(format!(
        " {}{marker}{}{value}{loading}",
        "  ".repeat(row.depth),
        row.name
    ))
}

/// How many rows a list column has.
fn count(app: &App, pane: Pane) -> usize {
    match pane {
        Pane::CallStack => stack_lines(app)
            .iter()
            .filter(|l| l.frame.is_some())
            .count(),
        Pane::Variables => app.workspace.debug_rows(DebugTree::Variables).len(),
        Pane::Watch => app.workspace.debug_rows(DebugTree::Watch).len(),
        Pane::Console => 0,
    }
}

/// Draws the panel in `area`.
pub(crate) fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let theme = app.theme;
    frame.buffer_mut().set_style(area, theme.sidebar);
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(theme.separator);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Percentage(25),
        Constraint::Percentage(20),
        Constraint::Percentage(25),
    ])
    .split(inner);
    let focused = app.focus == Focus::Debug;
    for (pane, rect) in PANES.iter().zip(columns.iter()) {
        let mut title = theme.sidebar_header;
        if focused && app.debug.pane == *pane {
            title = title.patch(theme.quick_selected);
        }
        let header = format!("{:<width$}", pane.title(), width = usize::from(rect.width));
        let buffer = frame.buffer_mut();
        buffer.set_line(rect.x, rect.y, &Line::styled(header, title), rect.width);
        let body = Rect {
            y: rect.y + 1,
            height: rect.height.saturating_sub(1),
            ..*rect
        };
        draw_pane(app, frame, *pane, body, focused);
    }
}

fn draw_pane(app: &mut App, frame: &mut Frame, pane: Pane, body: Rect, focused: bool) {
    let theme = app.theme;
    let rows = usize::from(body.height);
    let current = focused && app.debug.pane == pane;
    let lines: Vec<(Option<usize>, Line<'static>)> = match pane {
        Pane::CallStack => stack_lines(app)
            .into_iter()
            .map(|l| (l.frame, l.line))
            .collect(),
        Pane::Variables => tree_lines(app, DebugTree::Variables),
        Pane::Watch => tree_lines(app, DebugTree::Watch),
        Pane::Console => {
            draw_console(app, frame, body);
            return;
        }
    };
    let Some(column) = pane.list() else { return };
    let final_row = lines.iter().filter_map(|(i, _)| *i).max().unwrap_or(0);
    let selected = app.debug.selected[column].min(final_row);
    app.debug.selected[column] = selected;
    let at = lines
        .iter()
        .position(|(i, _)| *i == Some(selected))
        .unwrap_or(0);
    let top = scroll(app.debug.top[column], at, rows).min(lines.len().saturating_sub(1));
    app.debug.top[column] = top;
    let buffer = frame.buffer_mut();
    let editing = pane == Pane::Watch && current;
    let column_rows = if editing {
        rows.saturating_sub(1)
    } else {
        rows
    };
    for (offset, (index, line)) in lines.into_iter().enumerate().skip(top).take(column_rows) {
        let mut line = line;
        if current && index == Some(selected) {
            let pad = usize::from(body.width).saturating_sub(line.width());
            line.spans.push(Span::raw(" ".repeat(pad)));
            line = line.patch_style(theme.quick_selected);
        }
        let y = body.y + to_u16(offset - top);
        buffer.set_line(body.x, y, &line, body.width);
    }
    if editing && body.height > 0 {
        let text = format!(" + {}", app.debug.watch_input);
        let y = body.y + body.height - 1;
        buffer.set_line(body.x, y, &Line::from(text), body.width);
    }
}

/// The lines of a variables or watch column; each one is selectable, its
/// index is its row.
fn tree_lines(app: &App, tree: DebugTree) -> Vec<(Option<usize>, Line<'static>)> {
    app.workspace
        .debug_rows(tree)
        .iter()
        .enumerate()
        .map(|(i, row)| (Some(i), variable_line(row)))
        .collect()
}

/// The console: the last lines that fit, then the line being typed.
fn draw_console(app: &App, frame: &mut Frame, body: Rect) {
    let theme = app.theme;
    let console = app.workspace.debug_console();
    let rows = usize::from(body.height).saturating_sub(1);
    let skip = console.len().saturating_sub(rows);
    let error = theme.letter(MarkerSeverity::Error).1;
    let buffer = frame.buffer_mut();
    for (offset, line) in console.iter().skip(skip).enumerate() {
        let (text, style) = match line.kind {
            ConsoleKind::Input => (format!(" > {}", line.text), Style::new()),
            ConsoleKind::Output | ConsoleKind::Result => (format!(" {}", line.text), Style::new()),
            ConsoleKind::Error => (format!(" {}", line.text), error),
        };
        buffer.set_line(
            body.x,
            body.y + to_u16(offset),
            &Line::styled(text, style),
            body.width,
        );
    }
    if body.height > 0 {
        let y = body.y + body.height - 1;
        let text = format!(" > {}", app.debug.console_input);
        buffer.set_line(body.x, y, &Line::from(text), body.width);
    }
}

/// The first row to show so that `target` is inside a window of `size`.
fn scroll(top: usize, target: usize, size: usize) -> usize {
    if size == 0 || target < top {
        target
    } else if target >= top + size {
        target + 1 - size
    } else {
        top
    }
}

impl App {
    /// Focuses a column, showing the panel.
    pub(crate) fn focus_debug(&mut self, pane: Pane) {
        self.debug.pinned = true;
        self.debug.pane = pane;
        self.focus = Focus::Debug;
    }

    /// Runs a debug view command by id; returns whether it was one.
    pub(crate) fn run_debug_view(&mut self, id: &str) -> bool {
        match id {
            "workbench.debug.action.focusCallStackView" => self.focus_debug(Pane::CallStack),
            "workbench.debug.action.focusVariablesView" => self.focus_debug(Pane::Variables),
            "workbench.debug.action.focusWatchView"
            | "workbench.debug.viewlet.action.addWatchExpression" => {
                self.focus_debug(Pane::Watch);
            }
            "workbench.debug.action.focusRepl" => self.focus_debug(Pane::Console),
            "debug.removeWatchExpression" => self.remove_selected_watch(),
            _ => return false,
        }
        true
    }

    fn remove_selected_watch(&mut self) {
        let rows = self.workspace.debug_rows(DebugTree::Watch);
        let selected = self.debug.selected[2].min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get(selected) {
            self.workspace.remove_watch(row.root);
        }
    }

    /// Keys while the debug panel has focus.
    pub(crate) fn debug_key(&mut self, key: KeyEvent) {
        if let Some(command) = keys::global(key) {
            self.run_global(command, key);
            return;
        }
        self.message = None;
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Tab => self.debug.pane = self.debug.pane.step(true),
            KeyCode::BackTab => self.debug.pane = self.debug.pane.step(false),
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Enter => self.debug_enter(),
            KeyCode::Char(c) if plain => self.debug_type(Some(c)),
            KeyCode::Backspace => self.debug_type(None),
            KeyCode::Delete if self.debug.pane == Pane::Watch => self.remove_selected_watch(),
            code => self.debug_move(code),
        }
    }

    /// Typing in the Watch and Console columns; `None` is Backspace.
    fn debug_type(&mut self, c: Option<char>) {
        let input = match self.debug.pane {
            Pane::Watch => &mut self.debug.watch_input,
            Pane::Console => &mut self.debug.console_input,
            _ => return,
        };
        match c {
            Some(c) => input.push(c),
            None => drop(input.pop()),
        }
    }

    fn debug_enter(&mut self) {
        let pane = self.debug.pane;
        let result = match pane {
            Pane::Console => {
                let text = std::mem::take(&mut self.debug.console_input);
                let result = self.workspace.evaluate_in_console(&text);
                if result.is_err() {
                    self.debug.console_input = text;
                }
                result
            }
            Pane::Watch if !self.debug.watch_input.is_empty() => {
                let result = self.workspace.add_watch(&self.debug.watch_input);
                if result.is_ok() {
                    self.debug.watch_input.clear();
                }
                result
            }
            Pane::CallStack => self.workspace.select_debug_frame(self.debug.selected[0]),
            Pane::Variables => self
                .workspace
                .toggle_debug_row(DebugTree::Variables, self.debug.selected[1]),
            Pane::Watch => self
                .workspace
                .toggle_debug_row(DebugTree::Watch, self.debug.selected[2]),
        };
        if let Err(err) = result {
            self.message = Some(err.to_string());
        }
    }

    fn debug_move(&mut self, code: KeyCode) {
        let pane = self.debug.pane;
        let Some(column) = pane.list() else { return };
        let final_row = count(self, pane).saturating_sub(1);
        let selected = self.debug.selected[column].min(final_row);
        let jump = self.page.max(1);
        self.debug.selected[column] = match code {
            KeyCode::Up => selected.saturating_sub(1),
            KeyCode::Down => (selected + 1).min(final_row),
            KeyCode::PageUp => selected.saturating_sub(jump),
            KeyCode::PageDown => (selected + jump).min(final_row),
            KeyCode::Home => 0,
            KeyCode::End => final_row,
            KeyCode::Right | KeyCode::Left => {
                self.debug_fold(pane, selected, code == KeyCode::Right);
                selected
            }
            _ => selected,
        };
    }

    /// Right opens the selected variable, Left closes it.
    fn debug_fold(&mut self, pane: Pane, selected: usize, open: bool) {
        let tree = match pane {
            Pane::Variables => DebugTree::Variables,
            Pane::Watch => DebugTree::Watch,
            _ => return,
        };
        let rows = self.workspace.debug_rows(tree);
        if rows
            .get(selected)
            .is_some_and(|r| r.expanded != open && (!open || r.expandable))
            && let Err(err) = self.workspace.toggle_debug_row(tree, selected)
        {
            self.message = Some(err.to_string());
        }
    }
}
