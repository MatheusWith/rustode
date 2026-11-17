//! Input-to-present latency, kept in the workspace's [`LatencyRecorder`].
//!
//! - **Start:** the [`Instant`] at which the event loop dispatches a key
//!   press (or an IME commit) to eframe, before egui sees it.
//! - **End:** the [`Instant`] right after eframe returns from the
//!   `RedrawRequested` that ran the frame consuming that key. eframe paints
//!   that frame and returns from `wgpu::Queue::present` inside that call.
//!
//! Both ends are clock readings in this process, so the interval is
//! measured. It covers the wait for the frame, the UI pass, tessellation,
//! rendering, the queue submission and the present call. It does not cover
//! the compositor or the display: the photons come later, by the compositor's
//! latency plus up to one refresh. That part is not measured here; any
//! figure for it is an estimate.
//!
//! Only keys that changed the editor (an edit, a cursor move, a save) are
//! recorded; a key that changed nothing is dropped.

use rustode_workspace::trace::{LatencyRecorder, LatencyReport};
use std::time::Instant;

/// Pairs key presses with the frame that showed them.
#[derive(Debug, Default)]
pub struct PresentLatency {
    /// Keys dispatched since the last present.
    pending: Vec<Instant>,
    /// Whether a frame ran since the last present, and if its input
    /// changed the editor.
    frame: Option<bool>,
    recorder: LatencyRecorder,
}

impl PresentLatency {
    /// No samples yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A key press was dispatched at `at`.
    pub fn key(&mut self, at: Instant) {
        self.pending.push(at);
    }

    /// The app ran a frame (an egui pass); `changed` tells whether its input
    /// changed the editor.
    pub fn frame(&mut self, changed: bool) {
        self.frame = Some(self.frame.unwrap_or(false) || changed);
    }

    /// The handler that ran the frame returned at `at`, after the present.
    /// Records the pending keys if that frame changed the editor.
    pub fn presented(&mut self, at: Instant) {
        let changed = self.frame.take().unwrap_or(false);
        for key in self.pending.drain(..) {
            if changed {
                let latency = at.saturating_duration_since(key);
                tracing::debug!(latency_us = latency.as_micros(), "input to present");
                self.recorder.record(latency);
            }
        }
    }

    /// The samples so far.
    #[must_use]
    pub fn recorder(&self) -> &LatencyRecorder {
        &self.recorder
    }

    /// Percentiles of the samples so far; `None` before the first one.
    #[must_use]
    pub fn report(&self) -> Option<LatencyReport> {
        self.recorder.report()
    }
}
