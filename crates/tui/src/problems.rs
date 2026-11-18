//! The problems panel, below the editor: every problem the language servers
//! reported, grouped by file, as VS Code's Problems view lists them
//! (Code - OSS, studied and reimplemented; see `ATTRIBUTION.md`). A file row
//! shows the name, the folder and the count; a problem row the severity
//! letter, the message, the source and code, and `[Ln 3, Col 5]`
//! (`src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts:384-411`,
//! `src/vs/workbench/contrib/markers/browser/messages.ts:54`); with no
//! problems it says so (`messages.ts:27`). Ctrl+Shift+M opens and focuses
//! it, or hides it when it has focus
//! (`src/vs/workbench/services/views/browser/viewsService.ts:515-535`).
//! Up and Down select, Enter goes to the problem, Escape returns to the
//! editor. The order is the workspace's.

use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use rustode_workspace::Problem;
use std::fmt::Write as _;
use std::path::Path;

/// The panel's state, kept while it is hidden.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ProblemsPanel {
    pub visible: bool,
    /// The selected problem, an index into the workspace's problems.
    pub selected: usize,
    /// The first row shown.
    pub top: usize,
}

/// One row of the panel.
enum Row<'a> {
    File { path: &'a Path, count: usize },
    Problem(usize),
}

fn rows(problems: &[Problem]) -> Vec<Row<'_>> {
    let mut rows = Vec::new();
    for (index, problem) in problems.iter().enumerate() {
        if index == 0 || problems[index - 1].path != problem.path {
            let count = problems[index..]
                .iter()
                .take_while(|p| p.path == problem.path)
                .count();
            rows.push(Row::File {
                path: &problem.path,
                count,
            });
        }
        rows.push(Row::Problem(index));
    }
    rows
}

/// The panel's height for an editor area `height` rows tall: a third of it,
/// at least 4 rows, leaving the editor at least 3.
pub(crate) fn height(height: u16) -> u16 {
    (height / 3).max(4).min(height.saturating_sub(3))
}

/// Draws the panel in `area`; records which screen row shows which problem.
pub(crate) fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    let theme = app.theme;
    frame.buffer_mut().set_style(area, theme.sidebar);
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(theme.separator)
        .title(Span::styled(" PROBLEMS ", theme.sidebar_header));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let problems = app.workspace.problems();
    let buffer = frame.buffer_mut();
    if problems.is_empty() {
        let text = "  No problems have been detected in the workspace.";
        buffer.set_line(inner.x, inner.y, &Line::from(text), inner.width);
        app.hits.problems = Some((inner, Vec::new()));
        return;
    }
    let panel = &mut app.problems;
    panel.selected = panel.selected.min(problems.len() - 1);
    let rows = rows(&problems);
    let selected_row = rows
        .iter()
        .position(|r| matches!(r, Row::Problem(i) if *i == panel.selected))
        .unwrap_or(0);
    let height = usize::from(inner.height);
    panel.top = scroll_to(panel.top, selected_row, height).min(rows.len().saturating_sub(1));
    let focused = app.focus == crate::app::Focus::Problems;
    let root = app.workspace.explorer().map(|e| e.root().to_owned());
    let width = usize::from(inner.width);
    let mut shown = Vec::new();
    for (offset, row) in rows.iter().enumerate().skip(panel.top).take(height) {
        let mut line = match row {
            Row::File { path, count } => file_line(path, *count, root.as_deref(), &theme),
            Row::Problem(index) => problem_line(&problems[*index], &theme),
        };
        if focused && offset == selected_row {
            let used = line.width();
            line.spans
                .push(Span::raw(" ".repeat(width.saturating_sub(used))));
            line = line.patch_style(theme.quick_selected);
        }
        let y = inner.y + u16::try_from(offset - panel.top).unwrap_or(u16::MAX);
        buffer.set_line(inner.x, y, &line, inner.width);
        shown.push(match row {
            Row::Problem(index) => Some(*index),
            Row::File { .. } => None,
        });
    }
    app.hits.problems = Some((inner, shown));
}

fn file_line(
    path: &Path,
    count: usize,
    root: Option<&Path>,
    theme: &crate::theme::Theme,
) -> Line<'static> {
    let name = path
        .file_name()
        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let folder = path.parent().map_or_else(String::new, |parent| {
        match root.and_then(|root| parent.strip_prefix(root).ok()) {
            Some(relative) => relative.display().to_string(),
            None => parent.display().to_string(),
        }
    });
    Line::from(vec![
        Span::styled(
            format!(" {name}"),
            Style::new().add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(format!("  {folder}"), theme.description),
        Span::raw(format!("  {count}")),
    ])
}

fn problem_line(problem: &Problem, theme: &crate::theme::Theme) -> Line<'static> {
    let (letter, style) = theme.letter(problem.severity);
    let source = match (&problem.source, &problem.code) {
        (Some(source), Some(code)) => format!("  {source}({code})"),
        (Some(source), None) => format!("  {source}"),
        (None, Some(code)) => format!("  ({code})"),
        (None, None) => String::new(),
    };
    let position = format!(
        "  [Ln {}, Col {}]",
        problem.start.line + 1,
        problem.start.column + 1
    );
    let message = problem
        .message
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    Line::from(vec![
        Span::raw("   "),
        Span::styled(letter, style),
        Span::raw(format!(" {message}")),
        Span::styled(format!("{source}{position}"), theme.description),
    ])
}

/// The first row to show so that `target` is inside a window of `size`.
fn scroll_to(top: usize, target: usize, size: usize) -> usize {
    if size == 0 || target < top {
        target
    } else if target >= top + size {
        target + 1 - size
    } else {
        top
    }
}

/// The text of the status bar's problems item: `E 1 W 2`, with `I 3` when
/// there are infos, as VS Code's counts read
/// (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:669-691`),
/// with letters for its icons.
pub(crate) fn counts_label(app: &App) -> String {
    let counts = app.workspace.problem_counts();
    let pack = |n: usize| match n {
        0..=999 => n.to_string(),
        1000..=9999 => format!("{}K", n / 1000),
        _ => "10K+".to_owned(),
    };
    let mut label = format!("E {} W {}", pack(counts.errors), pack(counts.warnings));
    if counts.infos > 0 {
        let _ = write!(label, " I {}", pack(counts.infos));
    }
    label
}
