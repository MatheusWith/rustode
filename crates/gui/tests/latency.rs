//! The input-to-present bookkeeping: keys are paired with the present of the
//! frame that consumed them, and only when that frame changed the editor.

use rustode_gui::latency::PresentLatency;
use std::time::{Duration, Instant};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn keys_are_timed_to_the_present_of_the_frame_that_showed_them() {
    let start = Instant::now();
    let mut latency = PresentLatency::new();
    latency.key(start);
    latency.key(start + ms(3));
    latency.frame(true);
    latency.presented(start + ms(10));
    let report = latency.report().expect("two samples");
    assert_eq!(report.count, 2);
    assert_eq!(report.max, ms(10));
    assert_eq!(report.p50, ms(7));
}

#[test]
fn keys_that_changed_nothing_are_dropped() {
    let start = Instant::now();
    let mut latency = PresentLatency::new();
    latency.key(start);
    latency.frame(false);
    latency.presented(start + ms(5));
    latency.frame(true);
    latency.presented(start + ms(9));
    assert!(latency.recorder().is_empty());
}

#[test]
fn a_present_without_a_frame_records_nothing() {
    let start = Instant::now();
    let mut latency = PresentLatency::new();
    latency.key(start);
    latency.presented(start + ms(2));
    latency.key(start + ms(4));
    latency.frame(false);
    latency.frame(true);
    latency.presented(start + ms(6));
    let report = latency.report().expect("one sample");
    assert_eq!(
        (report.count, report.max),
        (1, ms(2)),
        "a second pass counts too"
    );
}
