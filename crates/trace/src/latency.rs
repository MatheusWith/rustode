//! Input-to-frame latency: record durations, report percentiles.
//!
//! A frontend stamps each input event with an [`Instant`] and, once the frame
//! that reflects it is presented, records the elapsed time. The recorder is
//! pure: it never reads the clock itself, so tests feed it fixed durations.

use std::fmt;
use std::time::{Duration, Instant};

/// Collects input-to-frame durations.
#[derive(Debug, Clone, Default)]
pub struct LatencyRecorder {
    samples: Vec<Duration>,
}

/// Summary of the recorded durations, using the nearest-rank percentile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyReport {
    /// Number of recorded samples.
    pub count: usize,
    /// Median.
    pub p50: Duration,
    /// 95th percentile.
    pub p95: Duration,
    /// 99th percentile.
    pub p99: Duration,
    /// Slowest sample.
    pub max: Duration,
}

impl LatencyRecorder {
    /// Creates an empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one input-to-frame duration.
    pub fn record(&mut self, latency: Duration) {
        self.samples.push(latency);
    }

    /// Records the time from `input` to `presented`; zero if `presented` is earlier.
    pub fn record_between(&mut self, input: Instant, presented: Instant) {
        self.record(presented.saturating_duration_since(input));
    }

    /// Number of recorded samples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether nothing has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Summarizes the samples; `None` when nothing was recorded.
    #[must_use]
    pub fn report(&self) -> Option<LatencyReport> {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let max = *sorted.last()?;
        Some(LatencyReport {
            count: sorted.len(),
            p50: nearest_rank(&sorted, 50),
            p95: nearest_rank(&sorted, 95),
            p99: nearest_rank(&sorted, 99),
            max,
        })
    }
}

/// Nearest-rank percentile of a non-empty, ascending slice: the sample at
/// rank `ceil(percent / 100 * n)`, counting from 1.
fn nearest_rank(sorted: &[Duration], percent: usize) -> Duration {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

impl fmt::Display for LatencyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "count={} p50={:?} p95={:?} p99={:?} max={:?}",
            self.count, self.p50, self.p95, self.p99, self.max
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn recorder_with(millis: impl IntoIterator<Item = u64>) -> LatencyRecorder {
        let mut recorder = LatencyRecorder::new();
        for n in millis {
            recorder.record(ms(n));
        }
        recorder
    }

    #[test]
    fn empty_recorder_has_no_report() {
        let recorder = LatencyRecorder::new();
        assert!(recorder.is_empty());
        assert_eq!(recorder.report(), None);
    }

    #[test]
    fn one_to_hundred_ms_gives_nearest_rank_percentiles() {
        // Recorded in reverse to show that order does not matter.
        let recorder = recorder_with((1..=100).rev());
        let report = recorder.report().expect("samples were recorded");
        assert_eq!(report.count, 100);
        assert_eq!(report.p50, ms(50));
        assert_eq!(report.p95, ms(95));
        assert_eq!(report.p99, ms(99));
        assert_eq!(report.max, ms(100));
    }

    #[test]
    fn a_single_sample_is_every_percentile() {
        let report = recorder_with([7]).report().expect("one sample");
        assert_eq!(report.count, 1);
        assert_eq!([report.p50, report.p95, report.p99, report.max], [ms(7); 4]);
    }

    #[test]
    fn small_sets_round_the_rank_up() {
        // n = 10: p50 -> rank 5, p95 -> rank 10, p99 -> rank 10.
        let report = recorder_with([16, 3, 9, 12, 1, 30, 5, 8, 4, 2])
            .report()
            .expect("ten samples");
        assert_eq!(report.count, 10);
        assert_eq!(report.p50, ms(5));
        assert_eq!(report.p95, ms(30));
        assert_eq!(report.p99, ms(30));
        assert_eq!(report.max, ms(30));
    }

    #[test]
    fn record_between_measures_input_to_presented() {
        let input = Instant::now();
        let mut recorder = LatencyRecorder::new();
        recorder.record_between(input, input + ms(12));
        recorder.record_between(input + ms(5), input);
        let report = recorder.report().expect("two samples");
        assert_eq!(recorder.len(), 2);
        assert_eq!(report.max, ms(12));
        assert_eq!(report.p50, Duration::ZERO);
    }

    #[test]
    fn report_displays_as_key_value_pairs() {
        let report = recorder_with([1, 2]).report().expect("two samples");
        assert_eq!(
            report.to_string(),
            "count=2 p50=1ms p95=2ms p99=2ms max=2ms"
        );
    }
}
