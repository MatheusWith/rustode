//! How paths, commands and times appear in plans and reports.
//!
//! Paths under the home folder are shown as `~/…`, so a report can be quoted
//! in an analysis without the owner's user name (AGENTS.md §2.2).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A path for people: `~/…` under the home folder.
pub(crate) fn path(path: &Path) -> String {
    text(&path.to_string_lossy())
}

/// Replaces the home folder at the start of `s`, or after an `=`, with `~`.
pub(crate) fn text(s: &str) -> String {
    let Some(home) = std::env::var_os("HOME").filter(|h| h.len() > 1) else {
        return s.to_owned();
    };
    let home = home.to_string_lossy().trim_end_matches('/').to_owned();
    let tilde = |rest: &str| -> Option<String> {
        let tail = rest.strip_prefix(home.as_str())?;
        (tail.is_empty() || tail.starts_with('/')).then(|| format!("~{tail}"))
    };
    if let Some(shown) = tilde(s) {
        return shown;
    }
    if let Some((flag, value)) = s.split_once('=')
        && let Some(shown) = tilde(value)
    {
        return format!("{flag}={shown}");
    }
    s.to_owned()
}

/// A command line, each argument shell-quoted when needed.
pub(crate) fn command(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote(&text(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%~{}".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// A UTC date and time, `2026-11-17T16:05:01Z`.
pub(crate) fn utc(time: SystemTime) -> String {
    let (date, [h, m, s]) = civil(time);
    format!("{date}T{h:02}:{m:02}:{s:02}Z")
}

/// The run-id prefix for `time` in UTC, `2026-11-17-160501`.
pub(crate) fn run_stamp(time: SystemTime) -> String {
    let (date, [h, m, s]) = civil(time);
    format!("{date}-{h:02}{m:02}{s:02}")
}

/// The UTC date (`YYYY-MM-DD`) and time of day of `time`.
fn civil(time: SystemTime) -> (String, [u64; 3]) {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days to a proleptic Gregorian date (H. Hinnant, "chrono-Compatible
    // Low-Level Date Algorithms", civil_from_days), for days >= 0.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        [rest / 3_600, rest % 3_600 / 60, rest % 60],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_dates_match_known_instants() {
        let at = |secs| UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(utc(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(utc(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(utc(at(1_794_931_501)), "2026-11-17T16:05:01Z");
        assert_eq!(run_stamp(at(1_794_931_501)), "2026-11-17-160501");
    }

    #[test]
    fn arguments_with_spaces_or_quotes_are_quoted() {
        let args = ["a b", "it's", "--x=1", ""].map(str::to_owned);
        assert_eq!(command(&args), r"'a b' 'it'\''s' --x=1 ''");
    }
}
