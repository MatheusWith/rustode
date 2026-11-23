//! Input to frame: the benchmark hook (`RUSTODE_BENCH_SCRIPT`). The window
//! runs a script on itself, one step per frame, and reports on stderr; no key
//! is sent. The test plays the window system: it steps the harness and tells
//! the driver each frame was presented (the real loop does that after eframe's
//! `RedrawRequested`, which a test harness has none of), so it steps by hand
//! instead of with `quiet`, which waits for the repaints the script asks for.

use super::{HEIGHT, WIDTH, fixture, temp_dir};
use eframe::egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use rustode_gui::EditorApp;
use rustode_workspace::bench::{Script, SearchScript};
use std::time::Instant;

/// Steps `harness`, presenting each frame, until the script reports.
#[track_caller]
fn until_reported(harness: &mut Harness<'static, EditorApp>, driver: &impl Fn(Instant)) -> String {
    for _ in 0..100 {
        harness.step();
        driver(Instant::now());
        if let Some(line) = harness.state().bench_report() {
            return line.to_owned();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the script never reported");
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key} in {line}"))
}

fn window(app: EditorApp) -> (Harness<'static, EditorApp>, impl Fn(Instant)) {
    let driver = app.bench_driver().expect("a script is set");
    let harness = Harness::builder()
        .with_size(Vec2::new(WIDTH, HEIGHT))
        .build_eframe(move |cc| {
            rustode_gui::fonts::install(&cc.egui_ctx);
            app
        });
    (harness, move |at| driver.borrow_mut().after_present(at))
}

#[test]
fn the_edit_script_runs_on_the_open_file_and_leaves_it_as_it_was() {
    let text = "line\n".repeat(50);
    let path = fixture("bench-edits", "f.txt", &text);
    let mut app = EditorApp::open(&path).expect("open");
    app.run_bench_script(Script::Edits { seed: 4, count: 25 });
    let (mut harness, present) = window(app);

    let line = until_reported(&mut harness, &present);
    assert_eq!(field(&line, "count"), "25");
    assert_eq!(field(&line, "undo_steps"), "25");
    assert_eq!(field(&line, "start_chars"), text.len().to_string());
    assert_eq!(field(&line, "applied_chars"), (text.len() + 25).to_string());
    assert_eq!(field(&line, "undone_chars"), text.len().to_string());
    for key in ["apply_us", "apply_frame_us", "undo_us", "undo_frame_us"] {
        field(&line, key).parse::<u64>().expect(key);
    }
    // The window shows the text as it was: nothing is left dirty by the undo.
    let workspace = harness.state().workspace();
    let id = workspace.active_document().expect("doc");
    assert_eq!(workspace.document(id).expect("doc").text(), text);
}

#[test]
fn a_hook_script_that_is_not_set_changes_nothing() {
    let path = fixture("bench-none", "f.txt", "a\nb\n");
    let mut harness = super::open(&path);
    super::quiet(&mut harness);
    assert!(harness.state().bench_report().is_none());
    assert!(harness.state().bench_driver().is_none());
}

#[test]
fn the_search_script_opens_the_search_view_and_reports_what_it_found() {
    let dir = temp_dir("bench-search");
    for n in 0..6 {
        std::fs::write(
            dir.join(format!("f{n}.txt")),
            format!("one\nhandle_{n:03} two\n"),
        )
        .expect("write");
    }
    let app = EditorApp::open_paths(&[dir]).expect("open the folder");
    let mut app = app;
    app.run_bench_script(Script::Search(SearchScript {
        pattern: r"handle_\d{3}".to_owned(),
        regex: true,
        include: String::new(),
        exclude: String::new(),
    }));
    let (mut harness, present) = window(app);

    let line = until_reported(&mut harness, &present);
    assert_eq!(field(&line, "files"), "6");
    assert_eq!(field(&line, "matches"), "6");
    assert_eq!(field(&line, "state"), "Done");
    // The Search view is the sidebar's view, so the results were drawn.
    harness.step();
    assert!(
        harness
            .query_by_label_contains("6 results in 6 files")
            .is_some(),
        "the Search view shows its message"
    );
}
