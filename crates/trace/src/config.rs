//! Subscriber setup driven by the environment.
//!
//! - `RUSTODE_LOG` holds an [`EnvFilter`] directive (for example `debug` or
//!   `rustode_core=trace,warn`). Default: [`DEFAULT_FILTER`].
//! - `RUSTODE_LOG_FORMAT` is `pretty` (human-readable, the default) or `json`
//!   (one JSON object per line). Unknown values fall back to `pretty`.
//!
//! Output always goes to stderr, so stdout stays clean for command output.

use std::fmt;
use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

/// Environment variable with the level filter.
pub const LOG_ENV: &str = "RUSTODE_LOG";
/// Environment variable with the output format (`pretty` or `json`).
pub const FORMAT_ENV: &str = "RUSTODE_LOG_FORMAT";
/// Filter used when [`LOG_ENV`] is unset or empty.
pub const DEFAULT_FILTER: &str = "warn";

/// How events are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable lines, colored when stderr is a terminal.
    #[default]
    Pretty,
    /// Newline-delimited JSON, for machines.
    Json,
}

impl LogFormat {
    /// Parses `pretty` or `json`, ignoring case and surrounding spaces.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pretty" => Some(Self::Pretty),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Resolved tracing settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceConfig {
    /// [`EnvFilter`] directive.
    pub filter: String,
    /// Output format.
    pub format: LogFormat,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            filter: DEFAULT_FILTER.to_owned(),
            format: LogFormat::Pretty,
        }
    }
}

impl TraceConfig {
    /// Builds the settings from raw variable values; pure, for testing.
    #[must_use]
    pub fn from_vars(filter: Option<&str>, format: Option<&str>) -> Self {
        let filter = filter
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .unwrap_or(DEFAULT_FILTER);
        Self {
            filter: filter.to_owned(),
            format: format.and_then(LogFormat::parse).unwrap_or_default(),
        }
    }

    /// Reads [`LOG_ENV`] and [`FORMAT_ENV`] from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        let filter = std::env::var(LOG_ENV).ok();
        let format = std::env::var(FORMAT_ENV).ok();
        Self::from_vars(filter.as_deref(), format.as_deref())
    }
}

/// Why the global subscriber could not be installed.
#[derive(Debug)]
pub enum InitError {
    /// The filter directive does not parse.
    Filter(String),
    /// A global subscriber was already set.
    AlreadySet(String),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filter(why) => write!(f, "invalid {LOG_ENV} filter: {why}"),
            Self::AlreadySet(why) => write!(f, "tracing already initialized: {why}"),
        }
    }
}

impl std::error::Error for InitError {}

/// Parses a filter directive.
///
/// # Errors
///
/// [`InitError::Filter`] when the directive is invalid.
pub fn build_filter(directive: &str) -> Result<EnvFilter, InitError> {
    EnvFilter::try_new(directive).map_err(|e| InitError::Filter(e.to_string()))
}

/// Installs the global subscriber described by `config`, writing to stderr.
///
/// # Errors
///
/// [`InitError::Filter`] for a bad filter, [`InitError::AlreadySet`] when a
/// global subscriber already exists.
pub fn init(config: &TraceConfig) -> Result<(), InitError> {
    let filter = build_filter(&config.filter)?;
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    let installed = match config.format {
        LogFormat::Pretty => builder
            .with_ansi(std::io::stderr().is_terminal())
            .try_init(),
        LogFormat::Json => builder.json().with_ansi(false).try_init(),
    };
    installed.map_err(|e| InitError::AlreadySet(e.to_string()))
}

/// Reads the environment and installs the global subscriber.
///
/// # Errors
///
/// Same as [`init`].
pub fn init_from_env() -> Result<(), InitError> {
    init(&TraceConfig::from_env())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_warn_and_pretty() {
        let config = TraceConfig::from_vars(None, None);
        assert_eq!(config, TraceConfig::default());
        assert_eq!(config.filter, "warn");
        assert_eq!(config.format, LogFormat::Pretty);
    }

    #[test]
    fn variables_select_filter_and_format() {
        let config = TraceConfig::from_vars(Some("debug"), Some("JSON"));
        assert_eq!(config.filter, "debug");
        assert_eq!(config.format, LogFormat::Json);
    }

    #[test]
    fn empty_filter_and_unknown_format_fall_back() {
        let config = TraceConfig::from_vars(Some("  "), Some("xml"));
        assert_eq!(config, TraceConfig::default());
    }

    #[test]
    fn format_parsing() {
        assert_eq!(LogFormat::parse(" pretty "), Some(LogFormat::Pretty));
        assert_eq!(LogFormat::parse("json"), Some(LogFormat::Json));
        assert_eq!(LogFormat::parse(""), None);
    }

    #[test]
    fn invalid_filter_is_rejected() {
        assert!(build_filter("debug").is_ok());
        let err = build_filter("rustode=notalevel").expect_err("bad level");
        assert!(matches!(err, InitError::Filter(_)));
        assert!(err.to_string().contains(LOG_ENV));
    }
}
