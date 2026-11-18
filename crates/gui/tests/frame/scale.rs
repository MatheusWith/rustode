//! The window at the scale factors of common monitors, 1, 1.25, 1.5 and 2
//! physical pixels per point, as the platform reports them. The layout
//! keeps its size in points; the editor's lines, selections and caret are
//! whole pixels, which the rendered image shows: every pixel of a selection
//! is the selection color or the background, with nothing blended at the
//! edges, and every line is equally tall.

use super::{temp_dir, window};
use eframe::egui::{Vec2, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, SnapshotResults};
use rustode_gui::{EditorApp, line_height};
use rustode_workspace::{Selection, Workspace};

const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];
const SIZE: Vec2 = Vec2::new(480.0, 220.0);
/// Chars per line of the blank fixture.
const WIDTH: usize = 20;

const BACKGROUND: [u8; 3] = [0x1f, 0x1f, 0x1f];
const SELECTION: [u8; 3] = [0x26, 0x4f, 0x78];
const CARET: [u8; 3] = [0xae, 0xaf, 0xad];

/// `lines` lines of spaces, so nothing is drawn but selections and carets,
/// open with `selections` (char offsets) at `scale`.
fn blank(
    name: &str,
    lines: usize,
    selections: Vec<Selection>,
    scale: f32,
) -> Harness<'static, EditorApp> {
    let path = temp_dir(name).join("blank.txt");
    std::fs::write(&path, format!("{}\n", " ".repeat(WIDTH)).repeat(lines)).expect("fixture");
    let mut workspace = Workspace::new();
    let id = workspace.open(&path).expect("open");
    workspace.set_selections(id, selections, 0).expect("select");
    window(EditorApp::new(workspace), SIZE, scale)
}

/// The rendered frame, in physical pixels.
struct Pixels {
    width: u32,
    rgba: Vec<u8>,
}

impl Pixels {
    fn of(harness: &mut Harness<'_, EditorApp>) -> Self {
        let image = harness.render().expect("render");
        Self {
            width: image.width(),
            rgba: image.into_raw(),
        }
    }

    /// Which of `colors` the pixel at `(x, y)` is, within rounding; `None`
    /// for any other color, such as a blend at a blurred edge.
    fn which(&self, x: u32, y: u32, colors: &[[u8; 3]]) -> Option<usize> {
        let at = 4 * (y * self.width + x) as usize;
        let rgb = &self.rgba[at..at + 3];
        colors.iter().position(|color| {
            color
                .iter()
                .zip(rgb)
                .all(|(&want, &got)| want.abs_diff(got) <= 2)
        })
    }
}

/// A point coordinate as a pixel index at `scale`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small positive coordinates
fn px(points: f32, scale: f32) -> u32 {
    (points * scale).round() as u32
}

/// The runs of equal pixels down the column `x` from `top` to `bottom`, as
/// (color index or `None`, length).
fn column_runs(pixels: &Pixels, x: u32, top: u32, bottom: u32) -> Vec<(Option<usize>, u32)> {
    let mut runs: Vec<(Option<usize>, u32)> = Vec::new();
    for y in top..bottom {
        let color = pixels.which(x, y, &[SELECTION, BACKGROUND]);
        match runs.last_mut() {
            Some((last, length)) if *last == color => *length += 1,
            _ => runs.push((color, 1)),
        }
    }
    runs
}

#[test]
fn lines_are_whole_pixels_tall_and_selections_have_no_seams_at_every_scale() {
    let line = WIDTH + 1;
    for scale in SCALES {
        // Lines 0, 2 and 4 selected, 1, 3 and 5 not.
        let selections = [0, 2, 4]
            .map(|n| Selection::new(n * line, (n + 1) * line))
            .to_vec();
        let mut harness = blank(&format!("seams-{scale}"), 8, selections, scale);
        let editor = harness.get_by_role(Role::MultilineTextInput).rect();
        let pixels = Pixels::of(&mut harness);
        let height = px(line_height(scale), scale);
        assert_eq!(
            height,
            px(19.0, scale),
            "19 points in whole pixels at {scale}"
        );
        let x = px(editor.left() + 100.0, scale);
        let top = px(editor.top(), scale);
        let runs = column_runs(&pixels, x, top, top + 7 * height);
        assert!(
            runs.iter().all(|(color, _)| color.is_some()),
            "a blended pixel at {scale}: {runs:?}"
        );
        let selected: Vec<u32> = runs
            .iter()
            .filter(|(color, _)| *color == Some(0))
            .map(|&(_, length)| length)
            .collect();
        assert_eq!(selected, [height; 3], "selected lines at {scale}: {runs:?}");
        let gaps: Vec<u32> = runs[1..runs.len() - 1]
            .iter()
            .filter(|(color, _)| *color == Some(1))
            .map(|&(_, length)| length)
            .collect();
        assert_eq!(gaps, [height; 2], "lines between at {scale}: {runs:?}");
    }
}

#[test]
fn the_caret_is_whole_pixels_wide_as_in_vs_code() {
    for scale in SCALES {
        let mut harness = blank(
            &format!("caret-{scale}"),
            1,
            vec![Selection::new(4, 4)],
            scale,
        );
        let editor = harness.get_by_role(Role::MultilineTextInput).rect();
        let pixels = Pixels::of(&mut harness);
        let y = px(editor.top() + line_height(scale) / 2.0, scale);
        let row: Vec<Option<usize>> = (px(editor.left(), scale)..px(editor.right(), scale))
            .map(|x| pixels.which(x, y, &[CARET, BACKGROUND]))
            .collect();
        let caret = row.iter().filter(|&&color| color == Some(0)).count();
        // VS Code: 2 points rounded down to whole pixels, at least one.
        let expected = match scale {
            1.0 | 1.25 => 2,
            1.5 => 3,
            _ => 4,
        };
        assert_eq!(caret, expected, "caret pixels at {scale}");
        let start = row
            .iter()
            .position(|&color| color == Some(0))
            .expect("a caret");
        assert_eq!(row[start - 1], Some(1), "a crisp left edge at {scale}");
        assert_eq!(row[start + caret], Some(1), "a crisp right edge at {scale}");
    }
}

#[test]
fn the_layout_keeps_its_size_in_points_at_every_scale() {
    let dir = temp_dir("points");
    let root = dir.join("demo");
    std::fs::create_dir_all(&root).expect("dir");
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {\n    let answer = 42;\n}\n").expect("fixture");
    for scale in SCALES {
        let app = EditorApp::open_paths(&[root.clone(), file.clone()]).expect("open");
        let mut harness = window(app, Vec2::new(800.0, 220.0), scale);
        let editor = harness.get_by_role(Role::MultilineTextInput).rect();
        assert!((editor.left() - 200.0).abs() < 0.5, "{editor:?} at {scale}");
        let tabs = harness.get_by_role(Role::TabList).rect();
        assert!((tabs.height() - 35.0).abs() < 0.5, "{tabs:?} at {scale}");
        let pixels = Pixels::of(&mut harness);
        assert_eq!(pixels.width, px(800.0, scale), "drawn at {scale}");
    }
}

#[test]
fn snapshots_at_each_scale() {
    let dir = temp_dir("snapshots");
    let root = dir.join("demo");
    std::fs::create_dir_all(&root).expect("dir");
    let file = root.join("main.rs");
    std::fs::write(
        &file,
        "fn main() {\n    let answer = 42;\n    println!(\"{answer}\");\n}\n",
    )
    .expect("fixture");
    // One test, several windows: their snapshot results are gathered.
    let mut results = SnapshotResults::default();
    for (scale, name) in SCALES.into_iter().zip(["100", "125", "150", "200"]) {
        let mut workspace = Workspace::new();
        workspace.open_folder(&root).expect("folder");
        let id = workspace.open(&file).expect("open");
        workspace
            .set_selections(id, vec![Selection::new(16, 40)], 0)
            .expect("select");
        let mut harness = window(EditorApp::new(workspace), SIZE, scale);
        harness.snapshot(format!("scale_{name}"));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}
