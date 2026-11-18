//! Diagnostics in the window (Code - OSS, studied and reimplemented; see
//! `ATTRIBUTION.md`):
//!
//! - the problems panel below the editor lists every problem in the
//!   workspace's order, grouped by file, each row the severity icon, the
//!   message, the source and code, and `[Ln 3, Col 5]`
//!   (`src/vs/workbench/contrib/markers/browser/markersTreeViewer.ts:384-411`,
//!   `src/vs/workbench/contrib/markers/browser/messages.ts:54`), and tells
//!   screen readers "Error: … at line 2 and character 13" (`messages.ts:57-66`);
//!   a row is a button: a click, or Enter once focused, goes to the problem;
//! - Ctrl+Shift+M shows the panel and focuses its first row, or hides it
//!   when it has focus (`src/vs/workbench/services/views/browser/viewsService.ts:515-535`);
//! - the squiggles' colors are the color registry's
//!   (`src/vs/platform/theme/common/colors/editorColors.ts:70-107`); hints
//!   are three dots.

use eframe::egui::{
    self, Color32, FontId, Id, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo,
    WidgetType,
};
use rustode_workspace::{MarkerSeverity, Problem, ProblemCounts, Workspace};
use std::path::Path;

/// Height of the panel, in points.
pub const PANEL_HEIGHT: f32 = 160.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DESCRIPTION: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x5e);
const FONT_SIZE: f32 = 13.0;
/// Height of a problem row, in points.
const ROW_HEIGHT: f32 = 20.0;
/// Width of a severity icon, in points.
pub(crate) const ICON: f32 = 10.0;

/// Whether the panel shows, and whether its first row takes focus on the
/// next frame.
#[derive(Debug, Default)]
pub(crate) struct Panel {
    pub visible: bool,
    pub focus_first: bool,
}

/// The squiggle and icon color of a severity.
pub(crate) fn color(severity: MarkerSeverity) -> Color32 {
    match severity {
        MarkerSeverity::Error => Color32::from_rgb(0xf1, 0x4c, 0x4c),
        MarkerSeverity::Warning => Color32::from_rgb(0xcc, 0xa7, 0x00),
        MarkerSeverity::Info => Color32::from_rgb(0x59, 0xa4, 0xf9),
        MarkerSeverity::Hint => Color32::from_rgba_unmultiplied(0xee, 0xee, 0xee, 179),
    }
}

/// Paints a severity icon centered at `center`: a circle for errors and
/// infos (an info's hollow), a triangle for warnings, so the shape tells
/// them apart without color.
pub(crate) fn paint_icon(painter: &egui::Painter, center: Pos2, severity: MarkerSeverity) {
    let radius = ICON / 2.0;
    let color = color(severity);
    match severity {
        MarkerSeverity::Warning => {
            let points = vec![
                Pos2::new(center.x, center.y - radius),
                Pos2::new(center.x + radius, center.y + radius),
                Pos2::new(center.x - radius, center.y + radius),
            ];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        MarkerSeverity::Info | MarkerSeverity::Hint => {
            painter.circle_stroke(center, radius - 1.0, Stroke::new(1.5, color));
        }
        MarkerSeverity::Error => {
            painter.circle_filled(center, radius, color);
            let d = radius / 2.0;
            let stroke = Stroke::new(1.5, Color32::BLACK);
            painter.line_segment(
                [center + Vec2::new(-d, -d), center + Vec2::new(d, d)],
                stroke,
            );
            painter.line_segment(
                [center + Vec2::new(-d, d), center + Vec2::new(d, -d)],
                stroke,
            );
        }
    }
}

/// What screen readers hear for the counts, VS Code's tooltip
/// (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:643-667`).
pub(crate) fn counts_label(counts: ProblemCounts) -> String {
    let mut titles = Vec::new();
    for (count, title) in [
        (counts.errors, "Errors"),
        (counts.warnings, "Warnings"),
        (counts.infos, "Infos"),
    ] {
        if count > 0 {
            titles.push(format!("{title}: {count}"));
        }
    }
    if titles.is_empty() {
        "No Problems".to_owned()
    } else {
        titles.join(", ")
    }
}

/// The id of the panel's first row, which Ctrl+Shift+M focuses.
pub(crate) fn first_row_id() -> Id {
    row_id(0)
}

/// Whether a row of the panel has keyboard focus.
pub(crate) fn has_focus(ctx: &egui::Context, count: usize) -> bool {
    ctx.memory(|memory| {
        memory
            .focused()
            .is_some_and(|focused| (0..count).any(|i| row_id(i) == focused))
    })
}

/// Draws the panel over the whole of `ui`; returns the problem whose row was
/// activated.
pub(crate) fn show(ui: &mut Ui, workspace: &Workspace) -> Option<Problem> {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, BORDER));
    let inner = rect.shrink2(Vec2::new(8.0, 4.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    ui.label(
        RichText::new("PROBLEMS")
            .font(FontId::proportional(11.0))
            .color(FOREGROUND),
    );
    let problems = workspace.problems();
    if problems.is_empty() {
        ui.label(
            RichText::new("No problems have been detected in the workspace.")
                .font(FontId::proportional(FONT_SIZE))
                .color(FOREGROUND),
        );
        return None;
    }
    let root = workspace.explorer().map(|e| e.root().to_owned());
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut ui, |ui| {
            for (index, problem) in problems.iter().enumerate() {
                if index == 0 || problems[index - 1].path != problem.path {
                    let count = problems[index..]
                        .iter()
                        .take_while(|p| p.path == problem.path)
                        .count();
                    file_row(ui, &problem.path, count, root.as_deref());
                }
                if problem_row(ui, index, problem) {
                    chosen = Some(problem.clone());
                }
            }
        });
    chosen
}

fn file_row(ui: &mut Ui, path: &Path, count: usize, root: Option<&Path>) {
    let name = path
        .file_name()
        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let folder = path.parent().map_or_else(String::new, |parent| {
        match root.and_then(|root| parent.strip_prefix(root).ok()) {
            Some(relative) => relative.display().to_string(),
            None => parent.display().to_string(),
        }
    });
    ui.horizontal(|ui| {
        let font = FontId::proportional(FONT_SIZE);
        ui.label(
            RichText::new(name)
                .font(font.clone())
                .color(FOREGROUND)
                .strong(),
        );
        ui.label(RichText::new(folder).font(font.clone()).color(DESCRIPTION));
        ui.label(
            RichText::new(count.to_string())
                .font(font)
                .color(FOREGROUND),
        );
    });
}

/// The id of the `index`th problem row.
fn row_id(index: usize) -> Id {
    Id::new("rustode-problem-row").with(index)
}

/// One problem as a focusable row; returns whether it was activated (a
/// click, or Enter or Space while focused).
fn problem_row(ui: &mut Ui, index: usize, problem: &Problem) -> bool {
    let source = match (&problem.source, &problem.code) {
        (Some(source), Some(code)) => format!("{source}({code})"),
        (Some(source), None) => source.clone(),
        (None, Some(code)) => format!("({code})"),
        (None, None) => String::new(),
    };
    let message = problem.message.lines().next().unwrap_or_default();
    let (line, column) = (problem.start.line + 1, problem.start.column + 1);
    let text = format!("{message}   {source}   [Ln {line}, Col {column}]");
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(FONT_SIZE), FOREGROUND);
    let indent = 16.0;
    let width = ui
        .available_width()
        .max(indent + ICON + 8.0 + galley.size().x);
    let (_, rect) = ui.allocate_space(Vec2::new(width, ROW_HEIGHT));
    let response = ui.interact(rect, row_id(index), Sense::click());
    if response.has_focus() || response.hovered() {
        ui.painter().rect_filled(rect, 0.0, SELECTED);
    }
    let center = Pos2::new(rect.left() + indent + ICON / 2.0, rect.center().y);
    paint_icon(ui.painter(), center, problem.severity);
    let at = Pos2::new(
        rect.left() + indent + ICON + 8.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, FOREGROUND);
    let generated = problem
        .source
        .as_ref()
        .map_or_else(String::new, |s| format!(" generated by {s}"));
    let spoken = format!(
        "{}: {message} at line {line} and character {column}.{generated}",
        problem.severity.label()
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &spoken));
    response.clicked()
}

/// Paints the squiggle of a diagnostic of `severity` under `x0..x1` on the
/// line whose bottom is `bottom`: a wave, or three dots for a hint.
pub(crate) fn paint_squiggle(
    painter: &egui::Painter,
    severity: MarkerSeverity,
    x0: f32,
    x1: f32,
    bottom: f32,
) {
    let color = color(severity);
    let y = bottom - 2.5;
    if severity == MarkerSeverity::Hint {
        for i in 0..3u8 {
            let x = x0 + 1.0 + f32::from(i) * 3.0;
            painter.circle_filled(Pos2::new(x, y + 1.0), 0.8, color);
        }
        return;
    }
    let mut points = Vec::new();
    let mut x = x0;
    let mut up = true;
    while x < x1 {
        points.push(Pos2::new(x, if up { y - 1.0 } else { y + 1.0 }));
        x += 2.0;
        up = !up;
    }
    points.push(Pos2::new(x1, if up { y - 1.0 } else { y + 1.0 }));
    painter.add(Shape::line(points, Stroke::new(1.0, color)));
}

/// The status bar's counts, drawn as VS Code draws them, an icon then a
/// number for errors and warnings, and infos when there are any; a click
/// toggles the panel. Returns its response and the width it took.
pub(crate) fn counts_item(
    ui: &mut Ui,
    at: Pos2,
    height: f32,
    counts: ProblemCounts,
) -> (egui::Response, f32) {
    let font = FontId::proportional(12.0);
    let mut items = vec![
        (MarkerSeverity::Error, counts.errors),
        (MarkerSeverity::Warning, counts.warnings),
    ];
    if counts.infos > 0 {
        items.push((MarkerSeverity::Info, counts.infos));
    }
    let galleys: Vec<_> = items
        .iter()
        .map(|(_, n)| {
            ui.painter()
                .layout_no_wrap(n.to_string(), font.clone(), FOREGROUND)
        })
        .collect();
    let width: f32 = galleys
        .iter()
        .map(|g| ICON + 4.0 + g.size().x + 8.0)
        .sum::<f32>()
        + 4.0;
    let rect = Rect::from_min_size(at, Vec2::new(width, height));
    let response = ui.interact(rect, Id::new("rustode-problems-status"), Sense::click());
    let label = counts_label(counts);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    let mut x = rect.left() + 4.0;
    for ((severity, _), galley) in items.iter().zip(galleys) {
        paint_icon(
            ui.painter(),
            Pos2::new(x + ICON / 2.0, rect.center().y),
            *severity,
        );
        x += ICON + 4.0;
        let pos = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
        x += galley.size().x + 8.0;
        ui.painter().galley(pos, galley, FOREGROUND);
    }
    (response, width)
}
