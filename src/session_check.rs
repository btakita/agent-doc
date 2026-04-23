//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` inspects the persisted per-document cycle state in
//!   `.agent-doc/state/cycles/<hash>.json` and exits nonzero when the most
//!   recent cycle is still open (`preflight_started`, `response_captured`, or
//!   `write_applied`).
//! - Falls back to the last `ops.log` event only when no cycle-state file
//!   exists yet, preserving compatibility for older repos.
//! - Also fails closed when the current document diverges from its snapshot in
//!   a way that looks like a direct assistant patchback (`### Re:` or
//!   `## Assistant`) without a corresponding `agent-doc` cycle.
//! - Exit 0 when the current cycle state is committed, when state/log files
//!   are missing, or when the fallback `ops.log` event is terminal and no
//!   likely bypassed patchback is present.
//! - Exit 1 when the current cycle state is still open, when the fallback last
//!   `ops.log` event is `preflight_diff_start`, or when a likely direct
//!   assistant patchback bypassed `agent-doc write` / `finalize`.
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
//! - `session_check_open_cycle_state_exits_one`
//! - `session_check_committed_cycle_state_exits_zero`
//! - `detect_bypassed_response_write_flags_template_heading`
//! - `detect_bypassed_response_write_flags_inline_assistant_heading`
//! - `detect_bypassed_response_write_ignores_plain_user_prompt`
//! - `session_check_missing_log_exits_zero`

use anyhow::Result;
use std::path::Path;

/// Event name prefix emitted by `preflight::run` that indicates a cycle
/// started but may have been abandoned. If this is the final entry in
/// ops.log, the previous cycle did not complete.
pub const PREFLIGHT_START_EVENT: &str = "preflight_diff_start";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path) -> Result<()> {
    match inspect(file)? {
        SessionCheckStatus::Ok(message) => {
            println!("{}", message);
            Ok(())
        }
        SessionCheckStatus::Interrupted(message) => {
            println!("{}", message);
            std::process::exit(1);
        }
    }
}

pub fn inspect(file: &Path) -> Result<SessionCheckStatus> {
    if let Some(state) = crate::cycle_state::load(file)? {
        if state.is_open() {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is still `{}` — no terminal commit followed",
                state.cycle_id,
                phase_name(state.phase)
            )));
        }
        if let Some(marker) = detect_bypassed_response_write(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}",
                marker
            )));
        }
        return Ok(SessionCheckStatus::Ok(format!(
            "[session-check] ok — cycle `{}` is `{}` ({})",
            state.cycle_id,
            phase_name(state.phase),
            state.last_event
        )));
    }

    match last_ops_event(file)? {
        None => {
            if let Some(marker) = detect_bypassed_response_write(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(
                "[session-check] no cycle state or ops.log — ok".to_string(),
            ))
        }
        Some(event) if event.starts_with(PREFLIGHT_START_EVENT) => {
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — no write/commit followed",
                PREFLIGHT_START_EVENT
            )))
        }
        Some(event) => {
            if let Some(marker) = detect_bypassed_response_write(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — last event: {}",
                event
            )))
        }
    }
}

fn phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
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

fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current == snapshot {
        return Ok(None);
    }

    let diff = similar::TextDiff::from_lines(&snapshot, &current);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_project(tmp: &Path) -> std::path::PathBuf {
        fs::create_dir_all(tmp.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(tmp.join(".agent-doc/snapshots")).unwrap();
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

    #[test]
    fn session_check_open_cycle_state_exits_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(state.is_open());
    }

    #[test]
    fn session_check_committed_cycle_state_exits_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some("body"), Some("body")).unwrap();
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(!state.is_open());
        assert_eq!(phase_name(state.phase), "committed");
    }

    #[test]
    fn detect_bypassed_response_write_flags_template_heading() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "---\nagent_doc_format: template\n---\n\n## Exchange\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            &doc,
            format!("{snapshot}### Re: test — gpt-5\n\nBody\n"),
        )
        .unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap();
        assert_eq!(marker.as_deref(), Some("### Re: test — gpt-5"));
    }

    #[test]
    fn detect_bypassed_response_write_flags_inline_assistant_heading() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "## User\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            &doc,
            format!("{snapshot}\n## Assistant\n\nResponse\n"),
        )
        .unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap();
        assert_eq!(marker.as_deref(), Some("## Assistant"));
    }

    #[test]
    fn detect_bypassed_response_write_ignores_plain_user_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot = "## User\n\nHello\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(&doc, format!("{snapshot}\nWhy is this still dirty?\n")).unwrap();

        assert!(detect_bypassed_response_write(&doc).unwrap().is_none());
    }
}
