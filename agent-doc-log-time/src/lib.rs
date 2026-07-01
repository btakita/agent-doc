//! # Module: log_time
//!
//! Human-readable log timestamps (`#opslogts`) and low-level ops-log line
//! formatting. Operators read the supervisor session log and
//! `.agent-doc/logs/ops.log` to verify reported issues, and a bare Unix epoch
//! like `1781771180` is not legible. This module formats every log entry's
//! timestamp as ISO-8601 UTC and parses it back, with **no external date
//! dependency** (Howard Hinnant's civil-date algorithms).
//!
//! ## Spec
//! - [`format_log_timestamp`] renders a Unix epoch (seconds) as
//!   `YYYY-MM-DDTHH:MM:SSZ` (UTC).
//! - [`parse_log_timestamp`] is the matching reader: it accepts **either** a
//!   bare Unix epoch (pre-`#opslogts` log lines and cycles.jsonl timestamps) or
//!   an ISO-8601 UTC string and returns epoch seconds. This backward
//!   compatibility keeps the staleness / accretion / startup-miss /
//!   `gate_verify` scanners working across the format switch — old `[<epoch>]`
//!   lines still parse, and `parse(format(x)) == x` round-trips.
//! - [`current_log_timestamp`] samples the system clock and returns the current
//!   timestamp in the same ISO-8601 UTC format.
//! - [`format_ops_log_tracking_suffix`] renders the appended
//!   ` doc=<stem>[ session=<id>][ turn=<cycle_id>]` suffix without knowing how
//!   callers found those values.
//! - [`format_ops_log_line`] renders the final bracketed ops-log line.
//!
//! ## Agentic Contracts
//! - Formatting/parsing helpers are pure and deterministic for unit testing; no
//!   I/O and no orchestration dependencies.
//! - Clock helpers fail closed to Unix epoch `0` when the system clock is before
//!   `UNIX_EPOCH`.
//! - `parse_log_timestamp` fails closed (`None`) on garbage rather than mapping
//!   it to `0`, so a malformed line is skipped, not mis-windowed.

use std::time::{SystemTime, UNIX_EPOCH};

/// Return the current Unix epoch seconds, defaulting to `0` when the system
/// clock is earlier than `UNIX_EPOCH`.
pub fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Return the current timestamp in the log format `YYYY-MM-DDTHH:MM:SSZ`.
pub fn current_log_timestamp() -> String {
    format_log_timestamp(current_epoch_secs())
}

/// Format a Unix epoch (seconds) as a human-readable ISO-8601 UTC timestamp
/// `YYYY-MM-DDTHH:MM:SSZ` (`#opslogts`).
pub fn format_log_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (hh, mm, ss) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);

    // civil_from_days: days since 1970-01-01 -> (year, month, day).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Render the `#opslogtrack` suffix appended to each ops-log line.
///
/// The caller owns data collection. This helper only preserves the wire format:
/// ` doc=<stem>[ session=<id>][ turn=<cycle_id>]`.
pub fn format_ops_log_tracking_suffix(
    doc_stem: Option<&str>,
    session: Option<&str>,
    turn: Option<&str>,
) -> String {
    let mut suffix = String::new();
    if let Some(stem) = doc_stem {
        suffix.push_str(" doc=");
        suffix.push_str(stem);
    }
    if let Some(session) = session.filter(|session| !session.is_empty()) {
        suffix.push_str(" session=");
        suffix.push_str(session);
    }
    if let Some(turn) = turn.filter(|turn| !turn.is_empty()) {
        suffix.push_str(" turn=");
        suffix.push_str(turn);
    }
    suffix
}

/// Render a full bracketed `.agent-doc/logs/ops.log` line.
pub fn format_ops_log_line(timestamp_secs: u64, message: &str, tracking_suffix: &str) -> String {
    format!(
        "[{}] {}{}",
        format_log_timestamp(timestamp_secs),
        message,
        tracking_suffix
    )
}

/// Parse a log timestamp that is either a bare Unix epoch (seconds) or an
/// ISO-8601 UTC string `YYYY-MM-DDTHH:MM:SSZ`, returning epoch seconds
/// (`#opslogts`). The trailing `Z` is optional. Returns `None` on anything that
/// is neither, so the staleness/gate scanners skip a malformed line.
pub fn parse_log_timestamp(raw: &str) -> Option<u64> {
    let s = raw.trim();
    if let Ok(epoch) = s.parse::<u64>() {
        return Some(epoch);
    }
    let s = s.strip_suffix('Z').unwrap_or(s);
    let (date, time) = s.split_once('T')?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: u64 = dp.next()?.parse().ok()?;
    let day: u64 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut tp = time.split(':');
    let hh: u64 = tp.next()?.parse().ok()?;
    let mm: u64 = tp.next()?.parse().ok()?;
    let ss: u64 = tp.next()?.parse().ok()?;
    if tp.next().is_some() || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }

    // days_from_civil: (year, month, day) -> days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if month > 2 { month - 3 } else { month + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146_097 + doe as i64 - 719_468;
    let secs = days * 86_400 + (hh * 3_600 + mm * 60 + ss) as i64;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_matches_known_iso8601_utc() {
        // 1781771180 = 2026-06-18T08:26:20Z UTC (the #tsiftmdcrash SIGTERM moment).
        assert_eq!(format_log_timestamp(1_781_771_180), "2026-06-18T08:26:20Z");
        assert_eq!(format_log_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_log_timestamp(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn parse_accepts_epoch_and_iso_and_round_trips() {
        // Backward-compat: bare epoch (pre-#opslogts log lines) still parse.
        assert_eq!(parse_log_timestamp("1781771180"), Some(1_781_771_180));
        // ISO-8601 UTC parses back to the same epoch.
        assert_eq!(
            parse_log_timestamp("2026-06-18T08:26:20Z"),
            Some(1_781_771_180)
        );
        // Round-trip across a range of epochs, including a leap day.
        for secs in [0_u64, 1, 1_709_164_800, 1_781_915_847, 4_102_444_800] {
            assert_eq!(
                parse_log_timestamp(&format_log_timestamp(secs)),
                Some(secs),
                "round-trip failed for epoch {secs}"
            );
        }
        // Garbage is rejected, not silently mapped to 0.
        assert_eq!(parse_log_timestamp("not-a-timestamp"), None);
        assert_eq!(parse_log_timestamp("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn current_log_timestamp_is_parseable() {
        let timestamp = current_log_timestamp();
        assert!(
            parse_log_timestamp(&timestamp).is_some(),
            "current timestamp should parse: {timestamp:?}"
        );
    }

    #[test]
    fn tracking_suffix_preserves_ops_log_wire_format() {
        assert_eq!(
            format_ops_log_tracking_suffix(Some("plan"), Some("sess-1"), Some("turn-2")),
            " doc=plan session=sess-1 turn=turn-2"
        );
        assert_eq!(
            format_ops_log_tracking_suffix(Some("plan"), None, Some("")),
            " doc=plan"
        );
    }

    #[test]
    fn ops_log_line_uses_bracketed_iso_timestamp() {
        assert_eq!(
            format_ops_log_line(0, "event happened", " doc=plan"),
            "[1970-01-01T00:00:00Z] event happened doc=plan"
        );
    }
}
