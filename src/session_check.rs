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
//! - When that bypassed patchback also leaves prompt-target lines in the same
//!   diff without the binary-owned `❯ ` transcript prefix, `session-check`
//!   reports the bare prompt target in the failure marker so the write path can
//!   be repaired instead of silently accepted.
//! - Narrow self-heal: when that drift is already committed in `HEAD` and the
//!   current working tree matches `HEAD` modulo transient boundary / `(HEAD)`
//!   markers, `session-check` repairs the stale snapshot instead of reporting
//!   a fresh interruption forever.
//! - Exit 0 when the current cycle state is committed, when state/log files
//!   are missing, or when the fallback `ops.log` event is terminal and no
//!   likely bypassed patchback is present.
//! - Exit 1 when the current cycle state is still open, when the fallback last
//!   `ops.log` event is `preflight_diff_start`, or when a likely direct
//!   assistant patchback bypassed `agent-doc write` / `finalize`.
//! - Exit 2 on unexpected I/O errors.
//!
//! ## Agentic Contracts
//! - Mutates only the snapshot in the narrow committed-historical-drift repair
//!   case above; otherwise read-only.
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
//! - `session_check_repairs_committed_historical_snapshot_drift`
//! - `session_check_missing_log_exits_zero`

use anyhow::Result;
use std::path::Path;

use crate::component::is_backlog_component;

/// Event name prefix emitted by `preflight::run` that indicates a cycle
/// started but may have been abandoned. If this is the final entry in
/// ops.log, the previous cycle did not complete.
pub const PREFLIGHT_START_EVENT: &str = "preflight_diff_start";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

pub struct SessionCheckReport {
    pub status: SessionCheckStatus,
    pub warnings: Vec<String>,
}

enum PendingCaptureGuardResult {
    None,
    Warn(Vec<String>),
    Error(String),
}

/// CLI entry: check the end-of-cycle write invariant for `file`.
///
/// Prints a short status line to stdout and exits with:
/// - `0` — log empty/missing, or last entry is a terminal event
/// - `1` — last entry is `preflight_diff_start` (interrupted cycle)
pub fn run(file: &Path) -> Result<()> {
    let report = inspect_with_warnings(file)?;
    for warning in &report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
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
    Ok(inspect_with_warnings(file)?.status)
}

pub fn inspect_with_warnings(file: &Path) -> Result<SessionCheckReport> {
    let mut report = SessionCheckReport {
        status: inspect_core(file)?,
        warnings: Vec::new(),
    };
    if matches!(report.status, SessionCheckStatus::Ok(_)) {
        for guard in [
            check_pending_capture_guard(file)?,
            check_pending_done_guard(file)?,
        ] {
            match guard {
                PendingCaptureGuardResult::None => {}
                PendingCaptureGuardResult::Warn(lines) => report.warnings.extend(lines),
                PendingCaptureGuardResult::Error(message) => {
                    report.status = SessionCheckStatus::Interrupted(message);
                    break;
                }
            }
        }
    }
    Ok(report)
}

pub fn enforce_clean_closeout(file: &Path) -> Result<()> {
    let report = inspect_with_warnings(file)?;
    for warning in report.warnings {
        eprintln!("{}", warning);
    }
    match report.status {
        SessionCheckStatus::Ok(_) => Ok(()),
        SessionCheckStatus::Interrupted(message) => anyhow::bail!(message),
    }
}

fn inspect_core(file: &Path) -> Result<SessionCheckStatus> {
    if let Some(state) = crate::cycle_state::load(file)? {
        if state.is_open() {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is still `{}` — no terminal commit followed",
                state.cycle_id,
                phase_name(state.phase)
            )));
        }
        if let Some(marker) = detect_bypassed_response_write(file)? {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason
                )));
            }
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
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — repaired committed historical {} snapshot drift",
                        reason
                    )));
                }
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
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                        event, reason
                    )));
                }
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

fn check_pending_capture_guard(file: &Path) -> Result<PendingCaptureGuardResult> {
    let mode = resolve_pending_capture_guard_mode(file)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(PendingCaptureGuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(PendingCaptureGuardResult::None);
    };
    if state.is_open() || state.had_pending_mutations {
        return Ok(PendingCaptureGuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(PendingCaptureGuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(PendingCaptureGuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(PendingCaptureGuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(PendingCaptureGuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(PendingCaptureGuardResult::None);
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
    if signal.estimated_count < 2 || signal.confidence < 0.5 {
        return Ok(PendingCaptureGuardResult::None);
    }

    let warn_line = format!(
        "[session-check] warn: response contains ~{} recommendation-like items but no --pending-add flags were used this cycle",
        signal.estimated_count
    );
    let hint_line =
        "[session-check] hint: consider adding pending items for actionable follow-up work"
            .to_string();

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => {
            PendingCaptureGuardResult::Warn(vec![warn_line, hint_line])
        }
        crate::frontmatter::PendingCaptureGuardMode::Strict => {
            PendingCaptureGuardResult::Error(format!(
                "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
                warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
            ))
        }
        crate::frontmatter::PendingCaptureGuardMode::Off => PendingCaptureGuardResult::None,
    })
}

fn resolve_pending_capture_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_capture_guard {
        return Ok(mode);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .pending_capture
        .unwrap_or_default())
}

fn resolve_pending_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .pending_done
        .unwrap_or_default())
}

fn check_pending_done_guard(file: &Path) -> Result<PendingCaptureGuardResult> {
    let mode = resolve_pending_done_guard_mode(file)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(PendingCaptureGuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(PendingCaptureGuardResult::None);
    };
    if state.is_open() {
        return Ok(PendingCaptureGuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(PendingCaptureGuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(PendingCaptureGuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(PendingCaptureGuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(PendingCaptureGuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(PendingCaptureGuardResult::None);
    }

    let open_ids = open_pending_ids(file)?;
    if open_ids.is_empty() {
        return Ok(PendingCaptureGuardResult::None);
    }

    let missing: Vec<String> = open_ids
        .into_iter()
        .filter(|id| response_clearly_completes_pending_id(&response_text, id))
        .filter(|id| !state.pending_done_ids.iter().any(|done| done == id))
        .collect();
    if missing.is_empty() {
        return Ok(PendingCaptureGuardResult::None);
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--pending-done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let warn_line = format!(
        "[session-check] warn: response appears to complete existing pending {} but no matching `--pending-done` was recorded this cycle",
        ids
    );

    Ok(match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => PendingCaptureGuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: re-run with {} or add `pending_done_guard: off` for this document when the item should stay open",
                hint
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => {
            PendingCaptureGuardResult::Error(format!(
                "{}\n[session-check] hint: re-run with {} or set pending_done_guard = \"warn\" to downgrade",
                warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
                hint
            ))
        }
        crate::frontmatter::PendingCaptureGuardMode::Off => PendingCaptureGuardResult::None,
    })
}

fn response_text_for_guards(response: &str) -> String {
    let Ok((patches, unmatched)) = crate::template::parse_patches(response) else {
        return response.to_string();
    };

    let preferred: Vec<String> = patches
        .iter()
        .filter(|patch| matches!(patch.name.as_str(), "exchange" | "findings"))
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !preferred.is_empty() {
        return preferred.join("\n\n");
    }

    if !unmatched.trim().is_empty() {
        return unmatched.trim().to_string();
    }

    let fallback: Vec<String> = patches
        .iter()
        .filter(|patch| !is_backlog_component(&patch.name))
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !fallback.is_empty() {
        return fallback.join("\n\n");
    }

    response.to_string()
}

fn open_pending_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    let Some(pending) = components
        .into_iter()
        .find(|component| is_backlog_component(&component.name))
    else {
        return Ok(Vec::new());
    };

    let body = &content[pending.open_end..pending.close_start];
    let (_, items, _) = crate::pending::parse_items(body);
    Ok(items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| item.id)
        .collect())
}

fn response_clearly_completes_pending_id(response_text: &str, id: &str) -> bool {
    let lines: Vec<String> = response_text
        .lines()
        .map(|line| line.trim().to_ascii_lowercase())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }

    let needle = format!("#{}", id.to_ascii_lowercase());
    for idx in 0..lines.len() {
        if !lines[idx].contains(&needle) {
            continue;
        }
        let end = (idx + 5).min(lines.len());
        let window = lines[idx..end].join("\n");
        if contains_completion_marker(&window) {
            return true;
        }
    }
    false
}

fn contains_completion_marker(text: &str) -> bool {
    [
        "implemented",
        "fixed",
        "done.",
        "done ",
        "completed",
        "updated",
        "verification:",
        "verified",
        "pushed",
        "commit:",
        "outcome:",
        "what changed:",
        "landed",
        "shipped",
    ]
    .iter()
    .any(|marker| text.contains(marker))
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

pub(crate) fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    // Normalize transient markers before comparison — (HEAD) annotations and
    // boundary IDs legitimately differ between snapshot (clean) and working tree
    // (preserves HEAD). Without this, preserved (HEAD) markers cause false-positive
    // "direct response patchback" detection.
    let norm = |s: &str| crate::git::normalize_transient_agent_doc_markers(s);
    let snap_norm = norm(&snapshot);
    let cur_norm = norm(&current);
    if cur_norm == snap_norm {
        return Ok(None);
    }

    let Some(diff_text) = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };

    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
            if let Some(bare_target) = crate::diff::first_bare_prompt_prefix_target(&diff_text) {
                return Ok(Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                )));
            }
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn make_project(tmp: &Path) -> std::path::PathBuf {
        fs::create_dir_all(tmp.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(tmp.join(".agent-doc/snapshots")).unwrap();
        let doc = tmp.join("doc.md");
        fs::write(&doc, "body").unwrap();
        doc
    }

    fn setup_committed_capture(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
    ) -> std::path::PathBuf {
        setup_committed_capture_with_pending(
            root,
            frontmatter,
            response,
            had_pending_mutations,
            None,
            &[],
        )
    }

    fn setup_committed_capture_with_pending(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
        pending_body: Option<&str>,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let prefix = frontmatter.unwrap_or("---\nagent_doc_session: test\n---\n\n");
        let mut current = format!("{prefix}## Exchange\n\nHello\n");
        if let Some(pending_body) = pending_body {
            current.push_str("\n<!-- agent:pending -->\n");
            current.push_str(pending_body);
            if !pending_body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:pending -->\n");
        }
        fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, &current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if had_pending_mutations {
            crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        }
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(
                &doc,
                &pending_done_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&current), Some(&current))
            .unwrap();
        crate::capture::mark_committed(&doc).unwrap();
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
        fs::write(&doc, format!("{snapshot}### Re: test — gpt-5\n\nBody\n")).unwrap();

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
        fs::write(&doc, format!("{snapshot}\n## Assistant\n\nResponse\n")).unwrap();

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

    #[test]
    fn detect_bypassed_response_write_reports_bare_prompt_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\n<!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\nWhy was this missed?\n### Re: test — gpt-5\n\nBody\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let marker = detect_bypassed_response_write(&doc).unwrap().unwrap();
        assert!(marker.contains("### Re: test — gpt-5"));
        assert!(marker.contains("Why was this missed?"));
    }

    #[test]
    fn session_check_repairs_committed_historical_snapshot_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        let tracked = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, tracked).unwrap();
        crate::snapshot::save(&doc, tracked).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let stale_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let historical = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, historical).unwrap();
        crate::snapshot::save(&doc, stale_snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();
        crate::cycle_state::start_preflight(&doc, Some(stale_snapshot), Some(historical)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(historical),
            Some(historical),
        )
        .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("repaired committed historical exchange snapshot drift"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            repaired_snapshot.contains("### Re: historical"),
            "snapshot should advance to the committed historical response:\n{repaired_snapshot}"
        );
        assert!(
            detect_bypassed_response_write(&doc).unwrap().is_none(),
            "snapshot repair should clear the interrupted marker"
        );
    }

    #[test]
    fn session_check_warns_on_uncaptured_recommendations() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Fix the stale closeout path\n3. Update the command spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 2);
        assert!(report.warnings[0].contains("recommendation-like items"));
    }

    #[test]
    fn session_check_skips_warning_when_pending_was_added() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Fix the stale closeout path\n",
            true,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_strict_mode_blocks_uncaptured_recommendations() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n"),
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("pending_capture_guard = \"warn\""));
            }
            other => panic!("expected strict-mode failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_suppression_marker_disables_guard() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: recommendations — gpt-5\n\n<!-- no-pending-capture -->\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_frontmatter_overrides_project_guard_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "[guards]\npending_capture = \"off\"\n",
        )
        .unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n"),
            "### Re: recommendations — gpt-5\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Interrupted(_)));
    }

    #[test]
    fn session_check_warns_on_missing_pending_done_for_completed_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 2);
        assert!(report.warnings[0].contains("#4qja"));
        assert!(report.warnings[1].contains("--pending-done 4qja"));
    }

    #[test]
    fn session_check_skips_pending_done_warning_when_id_was_recorded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &["4qja"],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_pending_done_strict_mode_blocks_missing_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: strict\n---\n\n"),
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("--pending-done 4qja"));
                assert!(message.contains("pending_done_guard = \"warn\""));
            }
            other => panic!("expected strict-mode failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_pending_done_suppression_marker_disables_guard() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\n<!-- no-pending-done-guard -->\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            false,
            Some("- [ ] [#4qja] Stream orchestrate patchback\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }
}
