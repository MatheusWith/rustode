//! Workspace seam: the benchmark hook (`rustode_workspace::bench`), a script
//! a frontend runs on itself so `rustode bench` can time it without a key
//! being injected. The tests play the frontend: they call `before_frame` and
//! `after_present` as a window would.

use rustode_workspace::bench::{
    Driver, MAX_EDITS, Script, SearchScript, Step, edit_offsets, fnv1a, search_query,
};
use rustode_workspace::{SearchState, Workspace};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-bench-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The fixture shape of `bench/scenarios.toml`: numbered 80-byte lines.
fn numbered(lines: usize) -> String {
    let mut text = String::new();
    for n in 1..=lines {
        let _ = writeln!(text, "{n:07} {}", "x".repeat(71));
    }
    text
}

/// One frame as a frontend runs it: the step, then the present.
fn frame(driver: &mut Driver, ws: &mut Workspace) -> Step {
    let step = driver.before_frame(ws);
    driver.after_present(Instant::now());
    step
}

/// Frames until the driver reports; the report line or a panic.
fn report(driver: &mut Driver, ws: &mut Workspace) -> String {
    for _ in 0..100 {
        ws.poll_search();
        if let Step::Report(line) = frame(driver, ws) {
            return line;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the driver never reported");
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key} in {line}"))
}

#[test]
fn the_edit_generator_is_deterministic_and_pinned() {
    // The reference values come from the JavaScript mulberry32 that the App
    // side runs (`bench/app_internals.cjs`, whose self-check pins the same six
    // edits): both editors must apply exactly these edits.
    let first: Vec<(usize, char)> = edit_offsets(1, 6, 1000).collect();
    assert_eq!(first, edit_offsets(1, 6, 1000).collect::<Vec<_>>());
    assert_eq!(
        first,
        vec![
            (496, 'r'),
            (95, 'h'),
            (270, 'u'),
            (320, 'e'),
            (559, 'v'),
            (330, 'i')
        ]
    );
    assert_ne!(first, edit_offsets(2, 6, 1000).collect::<Vec<_>>());
}

#[test]
fn every_offset_is_inside_the_document_that_has_grown() {
    for (i, (at, letter)) in edit_offsets(7, 5_000, 80).enumerate() {
        assert!(at <= 80 + i, "edit {i} at {at}");
        assert!(letter.is_ascii_lowercase());
    }
    assert_eq!(edit_offsets(7, 0, 80).count(), 0);
    // Even on an empty document: the first insert goes to 0.
    assert_eq!(edit_offsets(7, 1, 0).next().map(|(at, _)| at), Some(0));
}

#[test]
fn fnv1a_has_its_reference_values() {
    assert_eq!(fnv1a(""), 0x811C_9DC5);
    assert_eq!(fnv1a("a"), 0xE40C_292C);
    assert_eq!(fnv1a("foobar"), 0xBF9C_F968);
}

#[test]
fn a_script_is_read_from_json_and_refused_when_it_is_hostile() {
    assert_eq!(
        Script::parse(r#"{"kind":"edits","seed":3,"count":10}"#),
        Ok(Script::Edits { seed: 3, count: 10 })
    );
    assert_eq!(
        Script::parse(r#"{"kind":"search","pattern":"a+","regex":true}"#),
        Ok(Script::Search(SearchScript {
            pattern: "a+".to_owned(),
            regex: true,
            include: String::new(),
            exclude: String::new(),
        }))
    );
    for bad in [
        "",
        "null",
        "{}",
        r#"{"kind":"edits","seed":1}"#,
        r#"{"kind":"edits","seed":1,"count":0}"#,
        r#"{"kind":"edits","seed":-1,"count":5}"#,
        r#"{"kind":"edits","seed":1,"count":5,"extra":1}"#,
        r#"{"kind":"search","pattern":""}"#,
        r#"{"kind":"unknown"}"#,
        &format!(r#"{{"kind":"edits","seed":1,"count":{}}}"#, MAX_EDITS + 1),
        &format!(r#"{{"kind":"search","pattern":"{}"}}"#, "a".repeat(5000)),
    ] {
        let error = Script::parse(bad).expect_err(bad);
        assert!(error.to_string().starts_with("RUSTODE_BENCH_SCRIPT: "));
    }
    assert_eq!(Script::from_env_value(None), Ok(None));
    assert!(Script::from_env_value(Some("not json".as_ref())).is_err());
}

#[test]
fn the_edit_script_inserts_then_undoes_every_insert() {
    let dir = temp("edits");
    let file = dir.join("big.txt");
    let text = numbered(500);
    std::fs::write(&file, &text).expect("write");
    let mut ws = Workspace::new();
    ws.open(&file).expect("open");
    let mut driver = Driver::new(Script::Edits {
        seed: 9,
        count: 300,
    });

    // Nothing happens until a first frame was presented.
    assert_eq!(driver.before_frame(&mut ws), Step::Wait);
    let id = ws.active_document().expect("document");
    assert_eq!(ws.document(id).expect("doc").text(), text);
    driver.after_present(Instant::now());

    let line = report(&mut driver, &mut ws);
    assert_eq!(field(&line, "count"), "300");
    assert_eq!(field(&line, "start_chars"), text.len().to_string());
    assert_eq!(
        field(&line, "applied_chars"),
        (text.len() + 300).to_string()
    );
    assert_eq!(
        field(&line, "undo_steps"),
        "300",
        "one undo step per insert"
    );
    assert_eq!(field(&line, "undone_chars"), text.len().to_string());
    assert_eq!(
        field(&line, "undone_hash"),
        format!("{:08x}", fnv1a(&text)),
        "undo restores the text"
    );
    assert_ne!(field(&line, "applied_hash"), field(&line, "undone_hash"));
    for key in ["apply_us", "apply_frame_us", "undo_us", "undo_frame_us"] {
        field(&line, key).parse::<u128>().expect(key);
    }
    // The applied text is what the pure generator says.
    let mut expected: Vec<char> = text.chars().collect();
    for (at, letter) in edit_offsets(9, 300, text.chars().count()) {
        expected.insert(at, letter);
    }
    let expected: String = expected.into_iter().collect();
    assert_eq!(
        field(&line, "applied_hash"),
        format!("{:08x}", fnv1a(&expected))
    );
    // And after the report the driver has nothing left to do.
    assert_eq!(frame(&mut driver, &mut ws), Step::Finished);
    assert_eq!(ws.document(id).expect("doc").text(), text);
}

#[test]
fn the_edit_script_reports_an_error_instead_of_failing_without_a_document() {
    let mut ws = Workspace::new();
    let mut driver = Driver::new(Script::Edits { seed: 1, count: 5 });
    driver.after_present(Instant::now());
    let line = report(&mut driver, &mut ws);
    assert_eq!(line, "rustode-gui: bench error: no active document");
    assert_eq!(frame(&mut driver, &mut ws), Step::Finished);
}

#[test]
fn the_search_script_starts_a_search_and_times_the_first_result_and_the_end() {
    let dir = temp("search");
    for n in 0..20 {
        let sub = dir.join(format!("d{}", n % 4));
        std::fs::create_dir_all(&sub).expect("dir");
        std::fs::write(
            sub.join(format!("f{n}.txt")),
            format!("alpha\nhandle_{n:03} here\nomega\n"),
        )
        .expect("write");
    }
    let mut ws = Workspace::new();
    ws.open_folder(&dir).expect("open folder");
    let script = SearchScript {
        pattern: r"handle_\d{3}".to_owned(),
        regex: true,
        include: "**/*.txt".to_owned(),
        exclude: "d3/**".to_owned(),
    };
    let mut driver = Driver::new(Script::Search(script.clone()));
    assert_eq!(driver.before_frame(&mut ws), Step::Wait);
    driver.after_present(Instant::now());
    let started = driver.before_frame(&mut ws);
    assert_eq!(started, Step::StartSearch(script.clone()));
    ws.start_search(search_query(&script).expect("query"))
        .expect("start");
    driver.after_present(Instant::now());
    let line = report(&mut driver, &mut ws);
    assert_eq!(ws.search_results().state(), SearchState::Done);
    assert_eq!(field(&line, "files"), "15");
    assert_eq!(field(&line, "matches"), "15");
    assert_eq!(field(&line, "state"), "Done");
    assert_eq!(field(&line, "limit_hit"), "false");
    let first: u128 = field(&line, "first_result_us").parse().expect("first");
    let complete: u128 = field(&line, "complete_us").parse().expect("complete");
    assert!(first <= complete, "{line}");
}

#[test]
fn a_search_without_results_reports_no_first_result() {
    let dir = temp("search-none");
    std::fs::write(dir.join("a.txt"), "nothing here\n").expect("write");
    let mut ws = Workspace::new();
    ws.open_folder(&dir).expect("open folder");
    let script = SearchScript {
        pattern: "zzz".to_owned(),
        regex: false,
        include: String::new(),
        exclude: String::new(),
    };
    let mut driver = Driver::new(Script::Search(script.clone()));
    driver.after_present(Instant::now());
    assert!(matches!(driver.before_frame(&mut ws), Step::StartSearch(_)));
    ws.start_search(search_query(&script).expect("query"))
        .expect("start");
    driver.after_present(Instant::now());
    let line = report(&mut driver, &mut ws);
    assert_eq!(field(&line, "files"), "0");
    assert_eq!(field(&line, "first_result_us"), "none");
}

#[test]
fn a_bad_regex_in_a_search_script_is_an_error_not_a_panic() {
    let script = SearchScript {
        pattern: "(".to_owned(),
        regex: true,
        include: String::new(),
        exclude: String::new(),
    };
    assert!(search_query(&script).is_err());
}
