//! The minimap: the whole document as a column of tiny colored blocks at
//! the editor's right edge, with a slider over the part in view. A press
//! on it scrolls that line to the middle of the editor, and a drag of the
//! slider scrolls with it. The workspace says whether it is shown and gives
//! the colored runs of the lines it draws (`Workspace::minimap_enabled`,
//! `Workspace::minimap_lines`); this file only places and paints them.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`
//! (all in `src/vs/editor/`):
//!
//! - defaults: right side, proportional size, slider on mouseover, scale 1,
//!   at most 120 columns (`common/config/editorOptions.ts:3582-3590`);
//! - a char is `scale` pixels wide and a line `2 × scale` tall; the scale
//!   is the configured one, doubled from a pixel ratio of 2 and rounded up
//!   in between (`editorOptions.ts:2828-2834`, `:2889-2897`, `:2903`);
//! - the width fits the columns the editor shows into what is left, in
//!   proportion to the two char widths, plus an 8 px gutter, and never
//!   passes the maximum columns (`editorOptions.ts:2990-2993`; the gutter is
//!   `MINIMAP_GUTTER_WIDTH`, `:889`). rustode has no vertical scrollbar, so
//!   none of the width is kept for one;
//! - the slider is as many minimap lines tall as the editor shows lines,
//!   it moves in step with the scroll position, and when the minimap does
//!   not fit its height, the lines drawn start where they keep the slider
//!   in proportion (`browser/viewParts/minimap/minimap.ts:283-420`; the
//!   comment at `:353` says the slider at the bottom has the last line at
//!   its top, which is what the scroll range gives here too). Not done: the
//!   rule that keeps the start line from flickering between frames
//!   (`:395-406`);
//! - a block is the token's color halfway to the background
//!   (`browser/viewParts/minimap/minimapCharRenderer.ts:103-117`), a blank
//!   draws nothing (`minimap.ts:2251-2258`);
//! - a press on the minimap reveals the line under it in the middle of the
//!   editor, without moving the caret (`minimap.ts:1254-1288`, `:1140-1150`);
//!   a press on the slider drags it, scrolling by the pointer's distance
//!   over the slider's ratio (`:1291-1362`, `getDesiredScrollTopFromDelta`,
//!   `:254-256`);
//! - the slider shows while the pointer is over the minimap, while it is
//!   dragged, and for 500 ms after the editor scrolls (`minimap.css:6-18`,
//!   `minimap.ts:1244-1245`, `:1498-1501`), in the default scrollbar slider
//!   colors at half their opacity (`platform/theme/common/colors/miscColors.ts:56-65`,
//!   `minimapColors.ts:49-59`).
//!
//! Not done: VS Code's default draws glyphs (`renderCharacters`); this draws
//! the blocks it draws with that option off. Nor the selection, find match
//! and diagnostic marks, the section headers, `editor.minimap.side`, `size`,
//! `scale`, `maxColumn` and `showSlider` settings, or the fade of the slider.
//!
//! The cost of a frame does not grow with the document. The blocks are a
//! mesh built from the workspace's runs for the lines drawn and kept while
//! the document's version, language, the lines drawn and the size stay the
//! same; a frame with no change only moves and paints that mesh.

use crate::editor::{count, float};
use eframe::egui::{
    self, Color32, Mesh, Pos2, Rect, Sense, Ui, Vec2, emath::GuiRounding as _, epaint::Shape,
};
use rustode_workspace::{DocumentId, MinimapRun, TokenKind, Workspace};

/// Width kept on the left of the blocks, in points.
const GUTTER: f32 = 8.0;
/// The most columns drawn (`editor.minimap.maxColumn`).
const MAX_COLUMN: usize = 120;
/// How long the slider stays after a scroll, in seconds.
const SLIDER_AFTER_SCROLL: f64 = 0.5;
const BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
/// `minimapSlider.background`: `#797979` at 0.4 × 0.5.
const SLIDER: Color32 = Color32::from_rgba_unmultiplied_const(0x79, 0x79, 0x79, 0x33);
/// `minimapSlider.hoverBackground`: `#646464` at 0.7 × 0.5.
const SLIDER_HOVER: Color32 = Color32::from_rgba_unmultiplied_const(0x64, 0x64, 0x64, 0x59);
/// `minimapSlider.activeBackground`: `#BFBFBF` at 0.4 × 0.5.
const SLIDER_ACTIVE: Color32 = Color32::from_rgba_unmultiplied_const(0xbf, 0xbf, 0xbf, 0x33);

/// The id of the minimap widget.
fn id() -> egui::Id {
    egui::Id::new("rustode-minimap")
}

/// Sizes that depend on the screen and the editor's font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Metrics {
    /// The whole minimap's width, gutter included, in points.
    pub(crate) width: f32,
    /// One char's width, in points.
    char_width: f32,
    /// One line's height, in points.
    line_height: f32,
}

impl Metrics {
    /// The metrics for `available` points of width to share with the text,
    /// whose chars are `text_char_width` wide, at `pixels_per_point`; `None`
    /// when nothing fits.
    pub(crate) fn new(pixels_per_point: f32, text_char_width: f32, available: f32) -> Option<Self> {
        let ppp = pixels_per_point;
        if !(ppp.is_finite() && ppp > 0.0 && text_char_width > 0.0 && available.is_finite()) {
            return None;
        }
        let scale = if ppp >= 2.0 {
            2.0
        } else {
            (ppp + 0.125).floor().max(1.0)
        };
        let char_width = scale / ppp;
        let line_height = 2.0 * scale / ppp;
        let max = (float(MAX_COLUMN) * char_width).floor();
        let share = ((available - 2.0) * char_width / (text_char_width + char_width))
            .floor()
            .max(0.0);
        let width = max.min(share + GUTTER);
        (width > GUTTER).then_some(Self {
            width,
            char_width,
            line_height,
        })
    }

    /// How many columns fit in the width.
    fn columns(self) -> usize {
        count(((self.width - GUTTER) / self.char_width + 1e-3).floor()).min(MAX_COLUMN)
    }
}

/// What the layout is computed from, in points and lines.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Inputs {
    pub(crate) line_count: usize,
    /// The editor's line height.
    pub(crate) line_height: f32,
    /// The editor's height.
    pub(crate) viewport: f32,
    /// How far the editor is scrolled, in lines.
    pub(crate) scroll: f32,
    /// The minimap's height.
    pub(crate) height: f32,
    pub(crate) pixels_per_point: f32,
}

/// Where the slider is and which lines are drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    /// The first and the end of the lines drawn.
    pub(crate) lines: std::ops::Range<usize>,
    /// The slider's top, from the minimap's top, and its height.
    pub(crate) slider_top: f32,
    pub(crate) slider_height: f32,
    /// Points the slider moves for one line of scroll; zero when it cannot.
    pub(crate) per_line: f32,
    /// The lines of scroll there are: the last line, scrolled to the top.
    last: f32,
    /// The editor's height in lines.
    viewport_lines: f32,
    /// The first line drawn, with the part of it hidden above.
    start: usize,
    line_height: f32,
}

impl Layout {
    /// Places the slider for the editor scrolled to `inputs.scroll`.
    pub(crate) fn new(metrics: Metrics, inputs: Inputs) -> Self {
        let Inputs {
            line_count,
            line_height,
            viewport,
            scroll,
            height,
            pixels_per_point: ppp,
        } = inputs;
        let ml = metrics.line_height;
        let viewport_lines = if line_height > 0.0 {
            (viewport / line_height).max(0.0)
        } else {
            0.0
        };
        let slider_height = ((viewport_lines * ml * ppp).floor() / ppp).max(1.0 / ppp);
        let last = float(line_count.saturating_sub(1));
        let scroll = scroll.clamp(0.0, last);
        let fit = count((height / ml + 1e-3).floor());
        let extra = (viewport_lines - 1.0).max(0.0);
        let (start, per_line, slider_top) = if float(fit) >= float(line_count) + extra {
            (0, ml, scroll * ml)
        } else {
            let travel = (height - slider_height).min(last * ml).max(0.0);
            let per_line = if last > 0.0 { travel / last } else { 0.0 };
            let top = scroll * per_line;
            let start = count((scroll - top / ml).floor()).min(line_count.saturating_sub(1));
            (start, per_line, (scroll - float(start)) * ml)
        };
        Self {
            lines: start..(start + fit).min(line_count),
            slider_top,
            slider_height,
            per_line,
            last,
            viewport_lines,
            start,
            line_height: ml,
        }
    }

    /// The line under `y` points below the minimap's top.
    fn line_at(&self, y: f32) -> usize {
        let below = count((y / self.line_height).floor());
        (self.start + below).min(count(self.last))
    }

    /// The scroll that puts `line` in the middle of the editor.
    fn centering(&self, line: usize) -> f32 {
        (float(line) + 0.5 - self.viewport_lines / 2.0).clamp(0.0, self.last)
    }

    /// The scroll after the slider was dragged `delta` points from where it
    /// was when the scroll was `from`; `None` if it cannot move.
    fn dragged(&self, from: f32, delta: f32) -> Option<f32> {
        (self.per_line > 0.0).then(|| (from + delta / self.per_line).clamp(0.0, self.last))
    }
}

/// What a press started.
#[derive(Debug, Clone)]
struct Drag {
    /// The pointer's y when the slider was pressed.
    from_y: f32,
    /// The scroll then, in lines.
    scroll: f32,
    layout: Layout,
}

/// What the minimap keeps between frames, with the editor's view.
#[derive(Default)]
pub(crate) struct State {
    drag: Option<Drag>,
    /// The scroll last frame, and when it last changed, for the slider.
    scroll: Option<f32>,
    scrolled_at: f64,
    cache: Option<Cache>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("drag", &self.drag)
            .finish_non_exhaustive()
    }
}

/// The blocks of the lines drawn, from the minimap's top left.
struct Cache {
    key: Key,
    mesh: Mesh,
}

/// What the blocks depend on.
#[derive(Debug, PartialEq)]
struct Key {
    version: u64,
    language: &'static str,
    lines: std::ops::Range<usize>,
    columns: usize,
    tab_size: usize,
    /// The points-to-pixels factor and char size, as bits.
    size: [u32; 3],
}

/// The minimap's press and drag: the new scroll, in lines, if they moved
/// the editor.
pub(crate) fn interact(
    ui: &Ui,
    rect: Rect,
    layout: &Layout,
    scroll: f32,
    state: &mut State,
) -> Option<f32> {
    let ctx = ui.ctx();
    let had_focus = ctx.memory(|memory| memory.has_focus(crate::editor::id()));
    let response = ui.interact(rect, id(), Sense::click_and_drag());
    let (pressed_at, latest, down) = ui.input(|input| {
        let pressed = input
            .pointer
            .primary_pressed()
            .then(|| input.pointer.press_origin())
            .flatten()
            .filter(|pos| rect.contains(*pos));
        (
            pressed,
            input.pointer.latest_pos(),
            input.pointer.primary_down(),
        )
    });
    if !down {
        state.drag = None;
    }
    let mut moved = None;
    if let Some(pos) = pressed_at {
        let slider = slider_rect(rect, layout);
        if slider.contains(pos) {
            state.drag = Some(Drag {
                from_y: pos.y,
                scroll,
                layout: layout.clone(),
            });
        } else {
            moved = Some(layout.centering(layout.line_at(pos.y - rect.top())));
        }
        if had_focus {
            ctx.memory_mut(|memory| memory.request_focus(crate::editor::id()));
        }
    } else if let (Some(drag), Some(pos)) = (&state.drag, latest) {
        moved = drag.layout.dragged(drag.scroll, pos.y - drag.from_y);
    }
    drop(response);
    moved
}

fn slider_rect(rect: Rect, layout: &Layout) -> Rect {
    Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + layout.slider_top),
        Vec2::new(rect.width(), layout.slider_height),
    )
}

/// The blocks of `runs`, one list per line from `first`, as a mesh whose
/// origin is the minimap's top left. `pixels_per_point` snaps them to pixels.
fn blocks(runs: &[Vec<MinimapRun>], metrics: Metrics, pixels_per_point: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let snap = |points: f32| (points * pixels_per_point).round() / pixels_per_point;
    for (index, line) in runs.iter().enumerate() {
        let top = snap(float(index) * metrics.line_height);
        let bottom = snap(float(index + 1) * metrics.line_height);
        for run in line {
            let left = GUTTER + snap(float(run.column) * metrics.char_width);
            let right = GUTTER + snap(float(run.column + run.len) * metrics.char_width);
            let rect = Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom));
            mesh.add_colored_rect(rect, block_color(run.kind));
        }
    }
    mesh
}

/// A block of text of token kind `kind`: the token's color halfway to the
/// background; unhighlighted text is the default foreground.
fn block_color(kind: Option<TokenKind>) -> Color32 {
    let rgb = kind.map_or(0xCC_CC_CC, TokenKind::dark_modern);
    let [_, r, g, b] = rgb.to_be_bytes();
    let [br, bg, bb, _] = BACKGROUND.to_array();
    let half = |color: u8, background: u8| u8::midpoint(color, background);
    Color32::from_rgb(half(r, br), half(g, bg), half(b, bb))
}

/// What the painting needs besides the minimap's own state.
pub(crate) struct Paint<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) document: DocumentId,
    pub(crate) rect: Rect,
    pub(crate) metrics: Metrics,
    pub(crate) layout: Layout,
    pub(crate) scroll: f32,
}

/// Paints the minimap: its background, the blocks of the lines drawn, and the
/// slider when it is to be seen.
pub(crate) fn paint(ui: &Ui, paint: &Paint<'_>, state: &mut State) {
    let Paint {
        workspace,
        document,
        rect,
        metrics,
        layout,
        scroll,
    } = paint;
    let painter = ui.painter_at(*rect);
    painter.rect_filled(*rect, 0.0, BACKGROUND);
    let ppp = ui.pixels_per_point();
    let tab_size = workspace.tab_size(*document).unwrap_or(4);
    let Some(doc) = workspace.document(*document) else {
        return;
    };
    let key = Key {
        version: doc.version(),
        language: doc.language_id(),
        lines: layout.lines.clone(),
        columns: metrics.columns(),
        tab_size,
        size: [
            ppp.to_bits(),
            metrics.char_width.to_bits(),
            metrics.line_height.to_bits(),
        ],
    };
    if state.cache.as_ref().is_none_or(|cache| cache.key != key) {
        let runs = workspace
            .minimap_lines(*document, layout.lines.clone(), key.columns)
            .unwrap_or_default();
        state.cache = Some(Cache {
            mesh: blocks(&runs, *metrics, ppp),
            key,
        });
    }
    if let Some(cache) = &state.cache {
        let mut mesh = cache.mesh.clone();
        mesh.translate(rect.min.to_vec2());
        painter.add(Shape::mesh(mesh));
    }
    paint_slider(ui, paint, state, *scroll);
}

/// The slider, on mouseover, while dragged and just after a scroll.
fn paint_slider(ui: &Ui, paint: &Paint<'_>, state: &mut State, scroll: f32) {
    let Paint { rect, layout, .. } = paint;
    let now = ui.input(|input| input.time);
    if state
        .scroll
        .is_some_and(|before| (before - scroll).abs() > 1e-4)
    {
        state.scrolled_at = now;
    }
    state.scroll = Some(scroll);
    let slider = slider_rect(*rect, layout);
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let over = pointer.is_some_and(|pos| rect.contains(pos));
    let remaining = state.scrolled_at + SLIDER_AFTER_SCROLL - now;
    if remaining > 0.0 && state.scrolled_at > 0.0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(remaining));
    }
    let scrolling = remaining > 0.0 && state.scrolled_at > 0.0;
    if layout.per_line <= 0.0 || !(over || state.drag.is_some() || scrolling) {
        return;
    }
    let color = if state.drag.is_some() {
        SLIDER_ACTIVE
    } else if pointer.is_some_and(|pos| slider.contains(pos)) {
        SLIDER_HOVER
    } else {
        SLIDER
    };
    ui.painter_at(*rect).rect_filled(slider, 0.0, color);
}

/// The minimap's rectangle: the right of `area`, `width` wide, snapped to
/// pixels; and the text's, what is left.
pub(crate) fn split(area: Rect, width: f32, pixels_per_point: f32) -> (Rect, Rect) {
    let at = (area.right() - width).round_to_pixels(pixels_per_point);
    area.split_left_right_at_x(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics::new(1.0, 8.0, 800.0).expect("fits")
    }

    fn inputs(lines: usize, scroll: f32) -> Inputs {
        Inputs {
            line_count: lines,
            line_height: 19.0,
            viewport: 190.0,
            scroll,
            height: 400.0,
            pixels_per_point: 1.0,
        }
    }

    #[test]
    fn the_width_follows_vs_codes_proportion_and_caps_at_120_columns() {
        // (800 - 2) × 1 / (8 + 1) = 88.7 → 88, plus the gutter.
        assert_eq!(metrics().width, 96.0);
        assert_eq!(Metrics::new(1.0, 8.0, 3000.0).map(|m| m.width), Some(120.0));
        assert_eq!(metrics().columns(), 88);
    }

    #[test]
    fn the_scale_doubles_on_high_dpi_and_grows_in_between() {
        let at = |ppp| Metrics::new(ppp, 8.0, 800.0).expect("fits");
        assert_eq!(at(1.0).line_height, 2.0);
        assert!((at(1.25).line_height - 1.6).abs() < 1e-5, "2 px at 1.25");
        assert!(
            (at(1.5).line_height - 2.0 / 1.5 * 2.0).abs() < 1e-5,
            "scale 2"
        );
        assert!(
            (at(2.0).line_height - 2.0).abs() < 1e-5,
            "scale 2 at ratio 2"
        );
    }

    #[test]
    fn nothing_fits_in_a_tiny_or_hostile_area() {
        assert_eq!(Metrics::new(1.0, 8.0, 10.0), None);
        assert_eq!(Metrics::new(1.0, 8.0, f32::NAN), None);
        assert_eq!(Metrics::new(0.0, 8.0, 800.0), None);
        assert_eq!(Metrics::new(1.0, 0.0, 800.0), None);
    }

    #[test]
    fn a_short_document_fits_and_the_slider_tracks_the_scroll() {
        let layout = Layout::new(metrics(), inputs(50, 10.0));
        assert_eq!(layout.lines, 0..50);
        assert_eq!(layout.slider_top, 20.0, "2 px a line");
        assert_eq!(layout.slider_height, 20.0, "10 lines of the editor");
    }

    #[test]
    fn a_long_document_scrolls_the_minimap_with_the_slider_in_proportion() {
        let top = Layout::new(metrics(), inputs(10_000, 0.0));
        assert_eq!((top.lines.clone(), top.slider_top), (0..200, 0.0));
        let end = Layout::new(metrics(), inputs(10_000, 9_999.0));
        assert!((end.slider_top - 380.0).abs() < 1e-3, "{end:?}");
        assert_eq!(end.lines.end, 10_000);
        assert!(end.lines.start > 9_700, "{end:?}");
        let middle = Layout::new(metrics(), inputs(10_000, 5_000.0));
        assert!(middle.lines.contains(&5_000), "{middle:?}");
    }

    #[test]
    fn empty_and_degenerate_inputs_give_a_valid_layout() {
        for (lines, scroll, height, viewport) in [
            (0, 0.0, 400.0, 190.0),
            (1, 5.0, 400.0, 190.0),
            (100, f32::NAN, 400.0, 190.0),
            (100, 3.0, 0.0, 190.0),
            (100, 3.0, 400.0, 0.0),
        ] {
            let layout = Layout::new(
                metrics(),
                Inputs {
                    viewport,
                    height,
                    ..inputs(lines, scroll)
                },
            );
            assert!(layout.lines.end <= lines, "{layout:?}");
            assert!(layout.lines.start <= layout.lines.end.max(layout.lines.start));
            assert!(layout.slider_top.is_finite() && layout.slider_height > 0.0);
        }
    }

    #[test]
    fn a_press_centers_the_line_and_a_drag_scales_by_the_slider_ratio() {
        let layout = Layout::new(metrics(), inputs(50, 0.0));
        assert_eq!(layout.line_at(41.0), 20);
        assert_eq!(
            layout.line_at(10_000.0),
            49,
            "past the end is the last line"
        );
        assert_eq!(layout.centering(20), 15.5, "10 lines in view: 5 above");
        assert_eq!(layout.centering(1), 0.0, "never above the top");
        assert_eq!(layout.centering(49), 49.0, "never past the last line");
        assert_eq!(layout.dragged(4.0, 20.0), Some(14.0), "2 points a line");
        assert_eq!(layout.dragged(4.0, -1000.0), Some(0.0));
        let single = Layout::new(metrics(), inputs(1, 0.0));
        assert_eq!(single.dragged(0.0, 10.0), Some(0.0));
        let none = Layout::new(metrics(), inputs(0, 0.0));
        assert_eq!(none.line_at(5.0), 0);
    }

    #[test]
    fn blocks_are_halfway_to_the_background_and_tile_on_pixels() {
        assert_eq!(block_color(None), Color32::from_rgb(0x95, 0x95, 0x95));
        let runs = vec![
            vec![MinimapRun {
                column: 1,
                len: 3,
                kind: None,
            }],
            vec![],
        ];
        let mesh = blocks(&runs, metrics(), 1.0);
        assert_eq!(mesh.vertices.len(), 4, "one rect for one run");
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.pos.x).collect();
        assert_eq!(xs.iter().copied().fold(f32::MAX, f32::min), GUTTER + 1.0);
        assert_eq!(xs.iter().copied().fold(f32::MIN, f32::max), GUTTER + 4.0);
    }
}
