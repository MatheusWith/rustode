//! Shared tracing and metrics setup.
//!
//! Every rustode binary calls [`init_from_env`] once at startup, so logging
//! behaves the same everywhere: `RUSTODE_LOG` sets the filter (default
//! `warn`), `RUSTODE_LOG_FORMAT` picks `pretty` or `json`, and output goes to
//! stderr. [`spans`] names the common operations, and [`LatencyRecorder`]
//! measures input-to-frame latency for frontends. [`FirstFrame`] reports
//! the time from `main` to the first presented frame.

mod config;
mod first_frame;
mod latency;
pub mod spans;

pub use config::{
    DEFAULT_FILTER, FORMAT_ENV, InitError, LOG_ENV, LogFormat, TraceConfig, build_filter, init,
    init_from_env,
};
pub use first_frame::{FirstFrame, REPORT_ENV, mark_process_start, process_start};
pub use latency::{LatencyRecorder, LatencyReport};
