//! Startup to first frame: the time from the process's `main` to the first
//! frame a frontend presents, reported on stderr when asked.
//!
//! [`init_from_env`](crate::init_from_env) stamps the process start, so every
//! binary that follows the startup rule has it. A frontend calls
//! [`FirstFrame::presented`] each time a frame is presented; the first call
//! returns the elapsed time, every later one returns `None`. The report line
//! is printed only when [`REPORT_ENV`] is set (the benchmark runner sets it),
//! so a normal run prints nothing.
//!
//! Clocks: [`Instant`] (monotonic). The interval does not include the
//! process's exec and dynamic linking before `main`; `rustode bench` times
//! the whole launch from outside with its own clock.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Set to any value to have the GUI print [`FirstFrame::line`] on stderr.
pub const REPORT_ENV: &str = "RUSTODE_REPORT_FIRST_FRAME";

/// The start of the `rustode_trace` clock: the first call stamps it.
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Stamps the process start, once; later calls change nothing. Called by
/// `init_from_env` and `init`.
pub fn mark_process_start() {
    PROCESS_START.get_or_init(Instant::now);
}

/// When the process start was stamped; stamps it now if it never was.
#[must_use]
pub fn process_start() -> Instant {
    *PROCESS_START.get_or_init(Instant::now)
}

/// Reports the first presented frame once.
#[derive(Debug, Clone, Copy)]
pub struct FirstFrame {
    started: Instant,
    done: bool,
}

impl FirstFrame {
    /// Measures from `started`.
    #[must_use]
    pub fn since(started: Instant) -> Self {
        Self {
            started,
            done: false,
        }
    }

    /// Measures from the process start ([`process_start`]).
    #[must_use]
    pub fn since_process_start() -> Self {
        Self::since(process_start())
    }

    /// A frame was presented at `at`. Returns the time since the start the
    /// first time, and `None` every time after.
    pub fn presented(&mut self, at: Instant) -> Option<Duration> {
        if self.done {
            return None;
        }
        self.done = true;
        Some(at.saturating_duration_since(self.started))
    }

    /// The stderr line for a first frame after `elapsed`, in microseconds so
    /// a script reads it without parsing units.
    #[must_use]
    pub fn line(elapsed: Duration) -> String {
        format!(
            "rustode-gui: first frame presented {} us after main",
            elapsed.as_micros()
        )
    }

    /// Whether the report was asked for through [`REPORT_ENV`].
    #[must_use]
    pub fn requested() -> bool {
        std::env::var_os(REPORT_ENV).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_presented_frame_is_reported() {
        let start = Instant::now();
        let mut first = FirstFrame::since(start);
        let at = start + Duration::from_millis(250);
        assert_eq!(first.presented(at), Some(Duration::from_millis(250)));
        assert_eq!(first.presented(at + Duration::from_millis(16)), None);
        assert_eq!(first.presented(at + Duration::from_millis(32)), None);
    }

    #[test]
    fn a_frame_before_the_start_counts_as_zero() {
        let start = Instant::now() + Duration::from_secs(1);
        let mut first = FirstFrame::since(start);
        assert_eq!(first.presented(Instant::now()), Some(Duration::ZERO));
    }

    #[test]
    fn the_line_is_in_microseconds() {
        assert_eq!(
            FirstFrame::line(Duration::from_micros(123_456)),
            "rustode-gui: first frame presented 123456 us after main"
        );
    }

    #[test]
    fn the_process_start_is_stamped_once() {
        mark_process_start();
        let first = process_start();
        mark_process_start();
        assert_eq!(process_start(), first);
    }
}
