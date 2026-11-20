//! Input to frame: key, mouse and wheel events go into the window, and the
//! AccessKit tree, the window title and image snapshots come out. These tests
//! cover presentation only; the editing, tab and explorer logic is tested at
//! the workspace seam (`crates/workspace/tests/`).
//!
//! One test binary holds them all, since each GUI test binary links wgpu.
//! Snapshots live in `tests/snapshots/`. Regenerate them with
//! `UPDATE_SNAPSHOTS=1 cargo nextest run -p rustode-gui`.

mod debug;
mod editor;
mod extensions;
mod find;
mod groups;
mod keyboard;
mod language;
mod layout;
mod lsp;
mod minimap;
mod navigate;
mod quick_input;
mod scale;
mod search;
mod suggest;
mod workbench;

use eframe::egui::{Key, Modifiers, Vec2, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use std::path::{Path, PathBuf};

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 320.0;

/// A fresh temporary folder for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-gui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn fixture(name: &str, file: &str, text: &str) -> PathBuf {
    let path = temp_dir(name).join(file);
    std::fs::write(&path, text).expect("fixture");
    path
}

/// A window of `size` on `paths`, after its first frames.
fn open_paths(paths: &[PathBuf], size: Vec2) -> Harness<'static, EditorApp> {
    window(EditorApp::open_paths(paths).expect("open"), size, 1.0)
}

/// A window of `size` points on `app`, at `pixels_per_point` physical
/// pixels per point (the platform's scale factor), after its first frames.
fn window(app: EditorApp, size: Vec2, pixels_per_point: f32) -> Harness<'static, EditorApp> {
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(pixels_per_point)
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    quiet(&mut harness);
    harness
}

fn open(path: &Path) -> Harness<'static, EditorApp> {
    open_paths(&[path.to_owned()], Vec2::new(WIDTH, HEIGHT))
}

fn open_text(name: &str, text: &str) -> Harness<'static, EditorApp> {
    open(&fixture(name, "f.txt", text))
}

/// Steps the window until it asks for no more frames, at most 200 times.
///
/// `Harness::run` panics after four steps, but a background thread (the
/// search worker, a language server) that answers while the window runs
/// asks for a repaint through the workspace's waker, and on a slow machine
/// that can outlast four steps. A window that is truly stuck repainting
/// still fails here, after 200 steps.
#[track_caller]
fn quiet(harness: &mut Harness<'_, EditorApp>) {
    for _ in 0..200 {
        harness.step();
        if !harness.ctx.has_requested_repaint() {
            return;
        }
    }
    panic!("the window is still asking for repaints after 200 steps");
}

fn type_text(harness: &mut Harness<'_, EditorApp>, text: &str) {
    harness.event(eframe::egui::Event::Text(text.to_owned()));
    quiet(harness);
}

fn press(harness: &mut Harness<'_, EditorApp>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    quiet(harness);
}

/// The status bar items, in the order they are drawn.
fn status(harness: &Harness<'_, EditorApp>) -> Vec<String> {
    harness
        .get_by_role(Role::Status)
        .query_all_by_role(Role::Label)
        .filter_map(|item| item.value())
        .collect()
}

#[track_caller]
fn assert_status(harness: &Harness<'_, EditorApp>, item: &str) {
    let items = status(harness);
    assert!(
        items.iter().any(|shown| shown == item),
        "the status bar shows {items:?}, not {item:?}"
    );
}
