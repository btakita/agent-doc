//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` inspects `.agent-doc/logs/ops.log` and exits nonzero
//!   if the last log entry is `preflight_diff_start` — i.e. a preflight
//!   cycle started but never reached a write/commit. This signals an
//!   interrupted cycle (agent killed, session lost, or pending response
//!   that never landed).
//! - Exit 0 when the last entry is a terminal event (`ipc_write_consumed`,
//!   `disk_write`, `commit`, `recover_...`), when the log is empty, or when
//!   the log file does not exist.
//! - Exit 1 when the last entry is `preflight_diff_start`.
//! - Exit 2 on unexpected I/O errors.
//!
//! ## Agentic Contracts
//! - Read-only — never mutates the log or any document state.
//! - Called by supervisors / watchdogs (and directly from skill) to
//!   detect the "started but never wrote" invariant violation flagged
//!   as bug #a011.
//!
//! ## Evals
//! - `session_check_empty_log_exits_zero`
//! - `session_check_last_preflight_start_exits_one`
//! - `session_check_last_write_consumed_exits_zero`
//! - `session_check_missing_log_exits_zero`

use anyhow::Result;
use std::path::Path;

/// Event name prefix emitted by `preflight::run` that indicates a cycle
/// started but may have been abandoned. If this is the final entry in
/// ops.log, the previous cycle did not complete.
pub const PREFLIGHT_START_EVENT: &str = "preflight_diff_start";

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path) -> Result<()> {
    match last_ops_event(file)? {
        None => {
            println!("[session-check] ops.log is empty or missing — ok");
            Ok(())
        }
        Some(event) if event.starts_with(PREFLIGHT_START_EVENT) => {
            println!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — no write/commit followed",
                PREFLIGHT_START_EVENT
            );
            std::process::exit(1);
        }
        Some(event) => {
            println!("[session-check] ok — last event: {}", event);
            Ok(())
        }
    }
}

/// Return the message portion of the last non-empty line in `ops.log`,
/// stripped of the `[epoch_secs] ` timestamp prefix.
///
/// Returns `Ok(None)` when the log file is missing or empty.
pub fn last_ops_event(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    if !log_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&log_path)?;
    let last = content
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|l| strip_timestamp_prefix(l).to_string());
    Ok(last)
}

/// Strip a leading `[NNN] ` timestamp prefix from a log line.
fn strip_timestamp_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(close) = rest.find("] ")
    {
        return &rest[close + 2..];
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_project(tmp: &Path) -> std::path::PathBuf {
        fs::create_dir_all(tmp.join(".agent-doc/logs")).unwrap();
        let doc = tmp.join("doc.md");
        fs::write(&doc, "body").unwrap();
        doc
    }

    #[test]
    fn strip_timestamp_prefix_handles_well_formed_line() {
        assert_eq!(
            strip_timestamp_prefix("[1700000000] preflight_diff_start file=/x"),
            "preflight_diff_start file=/x"
        );
    }

    #[test]
    fn strip_timestamp_prefix_passes_through_malformed() {
        assert_eq!(strip_timestamp_prefix("no bracket"), "no bracket");
    }

    #[test]
    fn last_ops_event_missing_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_empty_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(tmp.path().join(".agent-doc/logs/ops.log"), "\n\n").unwrap();
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_returns_final_event_stripped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] preflight_diff_start file=x\n[101] ipc_write_consumed file=x patches=1\n",
        )
        .unwrap();
        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            "ipc_write_consumed file=x patches=1"
        );
    }

    #[test]
    fn last_ops_event_detects_preflight_start_as_last_line() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] ipc_write_consumed file=x\n[101] preflight_diff_start file=x\n",
        )
        .unwrap();
        let last = last_ops_event(&doc).unwrap().unwrap();
        assert!(last.starts_with(PREFLIGHT_START_EVENT));
    }
}
