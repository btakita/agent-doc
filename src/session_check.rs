//! # Module: session_check
//!
//! ## Spec
//! - `run(file)` inspects the persisted per-document cycle state in
//!   `.agent-doc/state/cycles/<hash>.json` and exits nonzero when the most
//!   recent cycle is still open (`preflight_started`, `response_captured`, or
//!   `write_applied`).
//! - Falls back to the last `ops.log` event only when no cycle-state file
//!   exists yet, preserving compatibility for older repos.
//! - Distinguishes "cycle started but no write/commit followed" from
//!   "response write landed but no commit followed" in both cycle-state and
//!   ops-log fallback paths.
//! - When an open `preflight_started` cycle already has a visible response
//!   patchback in the working tree, reports that manual-repair / commit-boundary
//!   state explicitly instead of collapsing it into the generic open-cycle
//!   message.
//! - Also fails closed when the current document diverges from its snapshot in
//!   a way that looks like a direct assistant patchback (`### Re:` or
//!   `## Assistant`) without a corresponding `agent-doc` cycle.
//! - Also fails closed when the current document already has unresolved
//!   prompt-bearing user edits (`prompt_target` / `content_edit`) relative to
//!   the snapshot, but no new `agent-doc` cycle ever started for them.
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
//!   `ops.log` event is `preflight_diff_start`, when a likely direct
//!   assistant patchback bypassed `agent-doc write` / `finalize`, or when
//!   the cycle state says `committed` but the snapshot does not match HEAD
//!   in the owning git root (response patchback visible but never committed).
//! - Exit 2 on unexpected I/O errors.
//!
//! ## Agentic Contracts
//! - May also clear a persisted startup-miss marker when the marker is proven
//!   stale because a later registered session start has already superseded it.
//! - Otherwise mutates only the snapshot in the narrow committed-historical-drift
//!   repair case above.
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
//! - `session_check_snapshot_committed_guard_fails_when_snapshot_differs`
//! - `session_check_snapshot_committed_guard_passes_when_committed`

use anyhow::Result;
use std::path::Path;

use crate::component::{is_backlog_component, is_tracked_work_component};

/// Event name prefix emitted by `preflight::run` that indicates a cycle
/// started but may have been abandoned. If this is the final entry in
/// ops.log, the previous cycle did not complete.
pub const PREFLIGHT_START_EVENT: &str = "preflight_diff_start";
pub const IPC_WRITE_CONSUMED_EVENT: &str = "ipc_write_consumed";
pub const SNAPSHOT_SAVED_FILE_IPC_EVENT: &str = "snapshot_saved_file_ipc";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCheckStatus {
    Ok(String),
    Interrupted(String),
}

pub struct SessionCheckReport {
    pub status: SessionCheckStatus,
    pub warnings: Vec<String>,
}

enum GuardResult {
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
        if let Some(message) = check_completed_pending_reap_guard(file)? {
            report.status = SessionCheckStatus::Interrupted(message);
            return Ok(report);
        }
        match check_shadow_backlog_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_backlog_replay_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        match check_snapshot_committed_guard(file)? {
            GuardResult::None => {}
            GuardResult::Warn(lines) => report.warnings.extend(lines),
            GuardResult::Error(message) => {
                report.status = SessionCheckStatus::Interrupted(message);
                return Ok(report);
            }
        }
        for guard in [
            check_pending_capture_guard(file)?,
            check_pending_done_guard(file)?,
        ] {
            match guard {
                GuardResult::None => {}
                GuardResult::Warn(lines) => report.warnings.extend(lines),
                GuardResult::Error(message) => {
                    report.status = SessionCheckStatus::Interrupted(message);
                    break;
                }
            }
        }
        if let Ok(Some(miss)) = crate::startup_miss::load(file) {
            if let Some(supersession) =
                crate::startup_miss::superseded_by_newer_registered_start(file, &miss)?
            {
                crate::startup_miss::clear(file)?;
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "session_check_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} latest_open_timestamp={}",
                        file.display(),
                        miss.pane_id,
                        supersession.registered_pane,
                        supersession.latest_open_timestamp
                    ),
                );
            } else {
                let detail = crate::startup_miss::session_log_diagnostic(file, &miss.session_id)
                    .ok()
                    .flatten()
                    .map(|detail| format!("; {detail}"))
                    .unwrap_or_default();
                report.warnings.push(format!(
                    "[session-check] WARNING: startup-miss marker exists for pane {} ({:?}) — the last {} start never acknowledged a document cycle{}",
                    miss.pane_id, miss.origin, miss.harness, detail
                ));
            }
        }
    }
    Ok(report)
}

fn check_completed_pending_reap_guard(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(None);
    };
    let completed: Vec<crate::pending::PendingItem> = components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| completed_pending_items(component.content(&content)))
        .collect();
    if completed.is_empty() {
        return Ok(None);
    }

    let refs = completed
        .into_iter()
        .map(|item| {
            if item.id.is_empty() {
                format!("<missing-id> {}", item.text)
            } else {
                format!("#{}", item.id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if refs.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "[session-check] INTERRUPTED: document still contains completed tracked item(s) after closeout: {}. Re-run preflight/repair so the reap is persisted through the snapshot + commit boundary",
        refs
    )))
}

fn completed_pending_items(body: &str) -> Vec<crate::pending::PendingItem> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(crate::pending::PendingItem::is_done)
        .collect()
}

fn check_snapshot_committed_guard(file: &Path) -> Result<GuardResult> {
    use crate::git::SnapshotCommitStatus;
    match crate::git::verify_snapshot_committed(file)? {
        SnapshotCommitStatus::Committed
        | SnapshotCommitStatus::NoSnapshot
        | SnapshotCommitStatus::NoHead
        | SnapshotCommitStatus::NotInGitRepo => Ok(GuardResult::None),
        SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            let side_effects = tracked_side_effect_note(file)?;
            let msg = format!(
                "[session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (snapshot_len={}, head_len={}). The response patchback is visible but was never committed{} {}",
                snapshot_len, head_len
                ,
                side_effects,
                closeout_recovery_hint(file)
            );
            eprintln!("{}", msg);
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_committed_guard_failed file={} snapshot_len={} head_len={}",
                    file.display(),
                    snapshot_len,
                    head_len
                ),
            );
            Ok(GuardResult::Error(msg))
        }
    }
}

fn closeout_recovery_hint(file: &Path) -> String {
    format!(
        "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
        file.display(),
        file.display()
    )
}

fn tracked_side_effect_paths(file: &Path) -> Result<Vec<String>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let doc_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(crate::git::tracked_modified_paths(file)?
        .into_iter()
        .filter(|path| !path.starts_with(".agent-doc/"))
        .filter(|path| path != &doc_name && !path.ends_with(&format!("/{doc_name}")))
        .collect())
}

fn tracked_side_effect_note(file: &Path) -> Result<String> {
    let mut paths = tracked_side_effect_paths(file)?;
    if paths.is_empty() {
        return Ok(String::new());
    }
    let overflow = paths.len().saturating_sub(3);
    paths.truncate(3);
    let mut note = format!("; tracked side-effect edits: {}", paths.join(", "));
    if overflow > 0 {
        note.push_str(&format!(" (+{} more)", overflow));
    }
    Ok(note)
}

pub(crate) fn detect_uncommitted_closeout_drift(file: &Path) -> Result<Option<String>> {
    if crate::git::repair_committed_historical_snapshot_drift(file)?.is_some() {
        return Ok(None);
    }
    if let Some(marker) = detect_bypassed_response_write(file)? {
        return Ok(Some(format!(
            "found likely direct response patchback without agent-doc cycle: {}{} {}",
            marker,
            tracked_side_effect_note(file)?,
            closeout_recovery_hint(file)
        )));
    }
    match crate::git::verify_snapshot_committed(file)? {
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
                return Ok(None);
            }
            Ok(Some(format!(
                "snapshot differs from HEAD without an open or recoverable agent-doc cycle (snapshot_len={}, head_len={}){} {}",
                snapshot_len,
                head_len,
                tracked_side_effect_note(file)?,
                closeout_recovery_hint(file)
            )))
        }
        crate::git::SnapshotCommitStatus::Committed
        | crate::git::SnapshotCommitStatus::NoSnapshot
        | crate::git::SnapshotCommitStatus::NoHead
        | crate::git::SnapshotCommitStatus::NotInGitRepo => Ok(None),
    }
}

fn check_shadow_backlog_guard(file: &Path) -> Result<GuardResult> {
    let content = std::fs::read_to_string(file)?;
    let report = crate::pending::detect_shadow_open_items(&content)?;
    if !report.shadow_only.is_empty() {
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: {}. Re-run preflight/repair after restoring them to the live backlog or marking them complete",
            format_shadow_refs(&report.shadow_only)
        )));
    }
    if !report.duplicated_in_live_backlog.is_empty() {
        return Ok(GuardResult::Warn(vec![format!(
            "[session-check] warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        )]));
    }
    Ok(GuardResult::None)
}

fn format_shadow_refs(items: &[crate::pending::ShadowPendingItem]) -> String {
    items
        .iter()
        .map(crate::pending::ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_backlog_replay_guard(file: &Path) -> Result<GuardResult> {
    let current_content = std::fs::read_to_string(file)?;

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let hash = crate::snapshot::doc_hash(&canonical).unwrap_or_default();
    let baseline_content = crate::snapshot::find_project_root(&canonical)
        .map(|root| root.join(format!(".agent-doc/baselines/{}.md", hash)))
        .and_then(|p| std::fs::read_to_string(p).ok());

    let baseline = match baseline_content {
        Some(content) => content,
        None => match crate::git::show_head(file)? {
            Some(content) => content,
            None => return Ok(GuardResult::None),
        },
    };

    let done_ids: std::collections::HashSet<String> = crate::cycle_state::load(file)?
        .map(|state| state.pending_done_ids.into_iter().collect())
        .unwrap_or_default();

    let report =
        crate::pending::detect_dropped_from_history(&current_content, &baseline, &done_ids)?;

    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(crate::pending::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done",
            refs
        )));
    }

    Ok(GuardResult::None)
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
            if let Some(reason) = crate::repair::recover_missing_commit_boundary(
                file,
                "session_check_commit_boundary_recovered",
            )? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` was `{}` ({}), recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        phase_name(state.phase),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` was `{}` ({}); recovered the missing commit boundary from {}",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason
                )));
            }
            if let Some(message) = open_cycle_manual_patchback_message(file, &state)? {
                return Ok(SessionCheckStatus::Interrupted(message));
            }
            return Ok(SessionCheckStatus::Interrupted(open_cycle_message(&state)));
        }
        if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
            if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason,
                    prompt_marker
                )));
            }
            return Ok(SessionCheckStatus::Ok(format!(
                "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                reason
            )));
        }
        if let Some(marker) = detect_bypassed_response_write(file)? {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                        state.cycle_id,
                        phase_name(state.phase),
                        state.last_event,
                        reason,
                        prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — cycle `{}` is `{}` ({}); repaired committed historical {} snapshot drift",
                    state.cycle_id,
                    phase_name(state.phase),
                    state.last_event,
                    reason
                )));
            }
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                marker,
                tracked_side_effect_note(file)?,
                closeout_recovery_hint(file)
            )));
        }
        if let Some(marker) = detect_active_session_post_commit_drift(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the active Codex session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the Stop hook recover it from the final assistant message.",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
                marker
            )));
        }
        if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
            return Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: cycle `{}` is `{}` ({}), but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: {}",
                state.cycle_id,
                phase_name(state.phase),
                state.last_event,
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
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — repaired committed historical {} snapshot drift",
                    reason
                )));
            }
            if let Some(marker) = detect_bypassed_response_write(file)? {
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no agent-doc cycle ever started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — repaired committed historical {} snapshot drift",
                        reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    tracked_side_effect_note(file)?,
                    closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: the active Codex session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the Stop hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: document has unresolved prompt-bearing user changes but no agent-doc cycle ever started: {}",
                    marker
                )));
            }
            Ok(SessionCheckStatus::Ok(
                "[session-check] no cycle state or ops.log — ok".to_string(),
            ))
        }
        Some(event) if event.starts_with(PREFLIGHT_START_EVENT) => {
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — cycle started but no write/commit followed",
                PREFLIGHT_START_EVENT
            )))
        }
        Some(event) if is_write_completed_commit_missing_event(&event) => {
            if let Some(reason) = crate::repair::recover_missing_commit_boundary(
                file,
                "session_check_commit_boundary_recovered",
            )? {
                let repaired_cycle = crate::cycle_state::load(file)?;
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log entry was `{}`, recovered the missing commit boundary from {}, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        event_name(&event),
                        reason,
                        prompt_marker
                    )));
                }
                if let Some(state) = repaired_cycle {
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; recovered the missing commit boundary from {} into cycle `{}`",
                        event, reason, state.cycle_id
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; recovered the missing commit boundary from {}",
                    event, reason
                )));
            }
            Ok(SessionCheckStatus::Interrupted(format!(
                "[session-check] INTERRUPTED: last ops.log entry is `{}` — response write landed but no commit followed",
                event_name(&event)
            )))
        }
        Some(event) => {
            if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)? {
                if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                    return Ok(SessionCheckStatus::Interrupted(format!(
                        "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                        reason, prompt_marker
                    )));
                }
                return Ok(SessionCheckStatus::Ok(format!(
                    "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                    event, reason
                )));
            }
            if let Some(marker) = detect_bypassed_response_write(file)? {
                if let Some(reason) = crate::git::repair_committed_historical_snapshot_drift(file)?
                {
                    if let Some(prompt_marker) = detect_unstarted_prompt_bearing_diff(file)? {
                        return Ok(SessionCheckStatus::Interrupted(format!(
                            "[session-check] INTERRUPTED: last ops.log event is terminal, repaired committed historical {} snapshot drift, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
                            reason, prompt_marker
                        )));
                    }
                    return Ok(SessionCheckStatus::Ok(format!(
                        "[session-check] ok — last event: {}; repaired committed historical {} snapshot drift",
                        event, reason
                    )));
                }
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: found likely direct response patchback without agent-doc cycle: {}{} {}",
                    marker,
                    tracked_side_effect_note(file)?,
                    closeout_recovery_hint(file)
                )));
            }
            if let Some(marker) = detect_active_session_post_commit_drift(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the active Codex session changed this document after the last committed closeout without reopening the binary-owned write/commit path: {}. Reopen closeout for this turn or let the Stop hook recover it from the final assistant message.",
                    marker
                )));
            }
            if let Some(marker) = detect_unstarted_prompt_bearing_diff(file)? {
                return Ok(SessionCheckStatus::Interrupted(format!(
                    "[session-check] INTERRUPTED: last ops.log event is terminal, but the document still has unresolved prompt-bearing user changes with no newer agent-doc cycle started: {}",
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

fn check_pending_capture_guard(file: &Path) -> Result<GuardResult> {
    let mode = resolve_pending_capture_guard_mode(file)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() || state.had_pending_mutations {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }
    let missing_targets = crate::write::unresolved_backlog_capture_targets(file, &state);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response came from a prompt that required backlog capture in {}, but those tracked-work surfaces did not change this cycle",
            missing_targets.join(", ")
        )));
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract described at least {} distinct issue(s), but the committed response only enumerated {} explicit backlog item(s) for target(s) {}",
            expected_count,
            promised_count,
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_plan_reference_shortfall(file, &state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract required at least {} explicit plan reference(s), but the committed response only cited {} existing plan path(s)",
            expected_count, promised_count,
        )));
    }
    let missing_ids =
        crate::write::unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response promised new tracked item(s) {} for explicit backlog target(s) {}, but those ids are still missing after this cycle",
            missing_ids.join(", "),
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        return Ok(GuardResult::Error(
            "[session-check] error: committed response came from a prompt that required backlog capture, but this cycle recorded no backlog mutations and did not explicitly state that there were no actionable follow-up items"
                .to_string(),
        ));
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(GuardResult::None);
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
            GuardResult::Warn(vec![warn_line, hint_line])
        }
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub(crate) fn resolve_pending_capture_guard_mode(
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

pub(crate) fn resolve_pending_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = crate::project_config::load_project_for_doc(file)
        .guards
        .pending_done
    {
        return Ok(mode);
    }
    if fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
    {
        return Ok(crate::frontmatter::PendingCaptureGuardMode::Strict);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Warn)
}

fn check_pending_done_guard(file: &Path) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode(file)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    let missing = detect_missing_pending_done_ids(file, &response_text, &state.pending_done_ids)?;
    if missing.is_empty() {
        return Ok(GuardResult::None);
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
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: re-run with {} or add `pending_done_guard: off` for this document when the item should stay open",
                hint
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: re-run with {} or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
            hint
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub(crate) fn detect_missing_pending_done_ids(
    file: &Path,
    response_text: &str,
    recorded_done_ids: &[String],
) -> Result<Vec<String>> {
    if response_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?;
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(open_ids
        .into_iter()
        .filter(|id| response_clearly_completes_pending_id(response_text, id))
        .filter(|id| !recorded_done_ids.iter().any(|done| done == id))
        .collect())
}

pub(crate) fn response_text_for_guards(response: &str) -> String {
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

fn open_tracked_work_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = crate::pending::parse_items(component.content(&content));
            items
        })
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

fn detect_active_session_post_commit_drift(file: &Path) -> Result<Option<String>> {
    let Some(session) = crate::codex_hook::load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
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
    if crate::git::normalize_committed_exchange_artifacts(&current)
        == crate::git::normalize_committed_exchange_artifacts(&snapshot)
    {
        return Ok(None);
    }

    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    let prompt_preview = session
        .last_prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("agent-doc session");
    let prompt_preview = prompt_preview.trim();

    let detail = match prompt_marker {
        Some(marker) => format!(
            "{}; active_session={} turn={} prompt={}",
            marker, session.session_id, session.last_turn_id, prompt_preview
        ),
        None => format!(
            "active_session={} turn={} prompt={}",
            session.session_id, session.last_turn_id, prompt_preview
        ),
    };
    Ok(Some(detail))
}

fn open_cycle_message(state: &crate::cycle_state::CycleState) -> String {
    let detail = match state.phase {
        crate::cycle_state::CyclePhase::PreflightStarted => {
            "cycle started but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::ResponseCaptured => {
            "response was captured but no write/commit followed"
        }
        crate::cycle_state::CyclePhase::WriteApplied => {
            "response write landed but no terminal commit followed"
        }
        crate::cycle_state::CyclePhase::Committed => "no terminal commit followed",
    };
    format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — {}",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        detail
    )
}

fn open_cycle_manual_patchback_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<Option<String>> {
    if !matches!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    ) {
        return Ok(None);
    }
    let Some(marker) = detect_bypassed_response_write(file)? else {
        return Ok(None);
    };
    Ok(Some(format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — found visible response patchback {} that is still outside the commit boundary. This looks like a manual repair that stopped before commit; finish it with `agent-doc write --commit {}` if you still have the response body, or commit the repaired document manually once the response is correct.",
        state.cycle_id,
        phase_name(state.phase),
        state.last_event,
        marker,
        file.display()
    )))
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
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    let last = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rfind(|line| {
            line.contains(&format!("file={canonical_display}"))
                || line.contains(&format!("file={requested_display}"))
        })
        .or_else(|| content.lines().rfind(|l| !l.trim().is_empty()))
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

pub(crate) fn detect_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    Ok(last_ops_event(file)?.filter(|event| is_write_completed_commit_missing_event(event)))
}

fn is_write_completed_commit_missing_event(event: &str) -> bool {
    event.starts_with(IPC_WRITE_CONSUMED_EVENT) || event.starts_with(SNAPSHOT_SAVED_FILE_IPC_EVENT)
}

fn event_name(event: &str) -> &str {
    event.split_whitespace().next().unwrap_or(event)
}

pub(crate) fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    Ok(detect_bypassed_response_write_between(&snapshot, &current))
}

pub(crate) fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    // Normalize transient markers before comparison — (HEAD) annotations and
    // boundary IDs legitimately differ between snapshot (clean) and working tree
    // (preserves HEAD). Without this, preserved (HEAD) markers cause false-positive
    // "direct response patchback" detection.
    let norm = |s: &str| crate::git::normalize_transient_agent_doc_markers(s);
    let snap_norm = norm(snapshot_doc);
    let cur_norm = norm(current_doc);
    if cur_norm == snap_norm {
        return None;
    }
    if !has_new_response_heading_marker(&snap_norm, &cur_norm) {
        return None;
    }

    let diff_text = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm)?;

    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
            if let Some(bare_target) = crate::diff::first_bare_prompt_prefix_target(&diff_text) {
                return Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                ));
            }
            return Some(trimmed.to_string());
        }
    }
    None
}

fn has_new_response_heading_marker(snapshot_doc: &str, current_doc: &str) -> bool {
    use std::collections::BTreeMap;

    fn marker_counts(doc: &str) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for line in doc.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Re:") || trimmed == "## Assistant" {
                *counts.entry(trimmed.to_string()).or_insert(0) += 1;
            }
        }
        counts
    }

    let snapshot_counts = marker_counts(snapshot_doc);
    let current_counts = marker_counts(current_doc);
    current_counts
        .into_iter()
        .any(|(marker, count)| count > snapshot_counts.get(&marker).copied().unwrap_or(0))
}

pub(crate) fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let Some(change) = first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let label = match change.kind {
        crate::diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        crate::diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        crate::diff::PromptBearingChangeKind::RecoveryArtifact
        | crate::diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    Ok(Some(format!("{label}: {preview}")))
}

pub(crate) fn first_unstarted_prompt_bearing_change(
    file: &Path,
) -> Result<Option<crate::diff::PromptBearingChange>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let norm = |s: &str| crate::git::normalize_committed_exchange_artifacts(s);
    let snap_norm = norm(&snapshot);
    let cur_norm = norm(&current);
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };

    Ok(crate::diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .find(|change| {
            matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::PromptTarget
                    | crate::diff::PromptBearingChangeKind::ContentEdit
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process::Command;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn make_project(tmp: &Path) -> std::path::PathBuf {
        fs::create_dir_all(tmp.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(tmp.join(".agent-doc/snapshots")).unwrap();
        let doc = tmp.join("doc.md");
        fs::write(&doc, "body").unwrap();
        doc
    }

    fn track_active_codex_session(root: &Path, doc: &Path, prompt: &str) {
        let session_id = "codex-session";
        let state_dir = root.join(".agent-doc/codex-hooks/sessions");
        fs::create_dir_all(&state_dir).unwrap();
        let hash = crate::ops_log::content_hash(session_id);
        let state_path = state_dir.join(format!("{hash}.json"));
        let payload = serde_json::json!({
            "session_id": session_id,
            "doc_path": doc.display().to_string(),
            "last_turn_id": "turn-1",
            "last_prompt": prompt,
            "updated_at": 1u64
        });
        fs::write(state_path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();
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
        setup_committed_capture_with_tracked_work(
            root,
            frontmatter,
            response,
            had_pending_mutations,
            pending_body,
            None,
            pending_done_ids,
        )
    }

    fn setup_committed_capture_with_tracked_work(
        root: &Path,
        frontmatter: Option<&str>,
        response: &str,
        had_pending_mutations: bool,
        pending_body: Option<&str>,
        icebox_body: Option<&str>,
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
        if let Some(icebox_body) = icebox_body {
            current.push_str("\n<!-- agent:icebox -->\n");
            current.push_str(icebox_body);
            if !icebox_body.ends_with('\n') {
                current.push('\n');
            }
            current.push_str("<!-- /agent:icebox -->\n");
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

    fn write_backlog_doc(path: &Path, backlog_body: &str) {
        let content = format!(
            "---\nagent_doc_session: target\n---\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(path, content).unwrap();
    }

    fn backlog_component_hash(path: &Path) -> String {
        let content = fs::read_to_string(path).unwrap();
        let components = crate::component::parse(&content).unwrap();
        let component = components
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
            .unwrap();
        crate::ops_log::content_hash(component.content(&content))
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
    fn last_ops_event_prefers_matching_file_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let other = tmp.path().join("other.md");
        fs::write(&other, "body").unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n[101] preflight_diff_start file={}\n",
                doc.display(),
                other.display()
            ),
        )
        .unwrap();
        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            format!("ipc_write_consumed file={} patches=1", doc.display())
        );
    }

    #[test]
    fn detect_write_completed_commit_missing_returns_last_write_event() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] snapshot_saved_file_ipc file=x snap_len=10\n",
        )
        .unwrap();
        assert_eq!(
            detect_write_completed_commit_missing(&doc)
                .unwrap()
                .unwrap(),
            "snapshot_saved_file_ipc file=x snap_len=10"
        );
    }

    #[test]
    fn session_check_open_cycle_state_exits_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("cycle started but no write/commit followed"));
            }
            other => panic!("expected interrupted state, got {other:?}"),
        }
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
    fn detect_bypassed_response_write_between_ignores_non_response_local_drift() {
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            detect_bypassed_response_write_between(snapshot, current),
            None,
            "ordinary local drift over HEAD should not look like a bypassed response write"
        );
    }

    #[test]
    fn session_check_interrupts_on_prompt_bearing_diff_without_cycle_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        let snapshot = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please investigate this startup miss.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::write(&doc, current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("prompt-bearing user changes"));
                assert!(message.contains("prompt_target"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_committed_state_has_new_prompt_diff() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n\n",
            "❯ Follow up on the remaining gap.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("no new agent-doc cycle started"));
                assert!(message.contains("prompt_target"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_on_active_session_post_commit_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Done. Manual active-turn drift.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: done — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("active Codex session changed this document"));
                assert!(message.contains("binary-owned write/commit path"));
                assert!(message.contains("agent-doc"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_ignores_active_session_canonicalization_only_drift() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do #closeout-bypass. spec-test-build-install-commit-push\n",
            "### Re: #closeout-bypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Do #closeout-bypass. spec-test-build-install-commit-push\n",
            "### Re: #closeout-bypass — gpt-5 (HEAD)\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, current).unwrap();
        track_active_codex_session(
            root,
            &doc,
            &format!(
                "agent-doc {}\nDo #closeout-bypass. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let _thread = EnvGuard::set("CODEX_THREAD_ID", "codex-session");

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_reports_missing_commit_after_ipc_write() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] ipc_write_consumed file=x patches=1\n",
        )
        .unwrap();
        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("response write landed but no commit followed"));
                assert!(message.contains("ipc_write_consumed"));
            }
            other => panic!("expected interrupted state, got {other:?}"),
        }
    }

    #[test]
    fn session_check_recovers_open_write_applied_cycle_from_committed_exchange_head() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(
                    message.contains("recovered the missing commit boundary from committed historical exchange snapshot drift"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected repaired ok status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "session_check_commit_boundary_recovered");
        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(repaired_snapshot.contains("### Re: #patchbypass — gpt-5"));
    }

    #[test]
    fn session_check_surfaces_manual_patchback_follow_through_for_open_preflight_cycle() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #mcrc. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let manual_patchback = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #mcrc. spec-test-build-install-commit-push\n",
            "### Re: #mcrc — gpt-5\n\n",
            "Recovered body.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, manual_patchback).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("visible response patchback"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("agent-doc write --commit"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("manual repair that stopped before commit"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
    }

    #[test]
    fn session_check_recovers_missing_commit_log_from_committed_exchange_head() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        fs::write(
            root.join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n",
                doc.display()
            ),
        )
        .unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(
                    message.contains("recovered the missing commit boundary from committed historical exchange snapshot drift"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected repaired ok status, got {other:?}"),
        }

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "session_check_commit_boundary_recovered");
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
    fn session_check_repairs_committed_historical_prompt_and_response_before_new_prompt() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#7mqc] Acceptance contract\n",
            "- [ ] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #7mqc. spec-test-news-commit-push\n",
            "### Re: do `#7mqc` — codex\n\n",
            "Done.\n\n",
            "do #sgzy. #spec-test-news-commit-push\n",
            "### Re: do `#sgzy` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#7mqc] Acceptance contract\n",
            "- [x] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "agent updates", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(head)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(head), Some(head)).unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #7mqc. spec-test-news-commit-push\n",
            "### Re: do `#7mqc` — codex\n\n",
            "Done.\n\n",
            "do #sgzy. #spec-test-news-commit-push\n",
            "### Re: do `#sgzy` — codex\n\n",
            "Done.\n\n",
            "What are the next steps?\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#7mqc] Acceptance contract\n",
            "- [x] [#sgzy] Fixture matrix\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("direct response patchback"),
                    "unexpected session-check message: {message}"
                );
                assert!(
                    message.contains("bare prompt target"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(!repaired_snapshot.contains("### Re: do `#sgzy` — codex"));
        assert!(!repaired_snapshot.contains("What are the next steps?"));
    }

    #[test]
    fn session_check_repairs_committed_historical_answered_prompt_prefix_drift() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #wdiv. spec-test-news-commit-push\n",
            "### Re: #wdiv — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do #wdiv. spec-test-news-commit-push\n",
            "### Re: #wdiv — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "normalize prompt", "--no-verify"])
            .output()
            .unwrap();

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "repair_preflight_committed_historical",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();
        fs::write(&doc, committed).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Ok(message) => {
                assert!(message.contains("repaired committed historical exchange snapshot drift"));
            }
            other => panic!("expected ok status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            repaired_snapshot, committed,
            "snapshot should follow the committed prompt-prefix normalization"
        );
    }

    #[test]
    fn session_check_fails_closed_when_committed_historical_patchback_mutates_status() {
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
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, head).unwrap();
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

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(head)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(head), Some(head)).unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();

        match inspect(&doc).unwrap() {
            SessionCheckStatus::Interrupted(message) => {
                assert!(
                    message.contains("direct response patchback"),
                    "unexpected session-check message: {message}"
                );
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }

        let repaired_snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(!repaired_snapshot.contains("### Re: do `#done` — codex"));
        assert!(!repaired_snapshot.contains("Tuned manually."));
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
    fn session_check_clears_startup_miss_superseded_by_newer_registered_owner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        fs::create_dir_all(tmp.path().join(".agent-doc/state/startup-miss")).unwrap();
        let miss = crate::startup_miss::StartupMiss {
            file: doc.display().to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };
        let miss_path = tmp
            .path()
            .join(".agent-doc/state/startup-miss")
            .join(format!("{}.json", crate::snapshot::doc_hash(&doc).unwrap()));
        fs::write(&miss_path, serde_json::to_string_pretty(&miss).unwrap()).unwrap();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            "session-123".to_string(),
            crate::sessions::SessionEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-123".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save_in(tmp.path(), &registry).unwrap();
        fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            concat!(
                "[1] session_start file=doc.md pane=%401 session=session-123\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[10] session_start file=doc.md pane=%408 session=session-123\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
        assert!(
            !miss_path.exists(),
            "session-check should clear stale superseded startup-miss markers"
        );
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
    fn session_check_warns_on_unconditional_followup_remaining_work() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: transfer status — opus-4-6\n\nCompleted 5 of 23 diagrams. 18 remaining to transfer.\n\nOptions to continue:\n1. Retry with rate limiting\n2. Use manual upload\n3. Wait for quota reset\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(!report.warnings.is_empty());
        assert!(report.warnings[0].contains("recommendation-like items"));
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
    fn session_check_warns_on_single_unresolved_bug_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            None,
            "### Re: tmux pane closure — gpt-5\n\nBecause that session was still hitting the older tmux route/sync cleanup bug that #4qgx was meant to close.\n",
            false,
        );

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(!report.warnings.is_empty());
        assert!(report.warnings[0].contains("recommendation-like items"));
    }

    #[test]
    fn session_check_blocks_backlog_required_review_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: code review — gpt-5\n\n1. High: Queue closeout can drift.\n2. Medium: Snapshot repair is too permissive.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("required backlog capture"));
            }
            other => panic!("expected backlog-required failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_allows_backlog_required_review_with_explicit_no_followups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: code review — gpt-5\n\nNo actionable follow-up items remained after this pass.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn session_check_blocks_when_bug_transfer_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n- [ ] [#old1] Existing item\n",
        );
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("baseline".to_string()),
            baseline_item_ids: vec!["old1".to_string()],
        };

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();
        crate::cycle_state::record_required_explicit_backlog_item_count(&doc, 4).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("described at least 4 distinct issue(s)"));
                assert!(message.contains("only enumerated 2 explicit backlog item(s)"));
            }
            other => panic!("expected bug-transfer inventory failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_blocks_when_response_promises_multiple_new_target_items_but_only_some_exist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#mcrc] Uncommitted repair follow-up\n- [ ] [#lvls] Preserve list-shape constraint\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#old1] Existing item\n",
        );

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("promised new tracked item(s)"));
                assert!(message.contains("#mcrc"));
                assert!(message.contains("#lvls"));
            }
            other => panic!("expected promised-transfer failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_blocks_when_bug_plan_reference_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n"),
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\nPlan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n",
            false,
        );
        let plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# Plan\n").unwrap();

        crate::cycle_state::record_required_plan_reference_count(&doc, 2).unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("required at least 2 explicit plan reference(s)"));
                assert!(message.contains("only cited 1 existing plan path(s)"));
            }
            other => panic!("expected plan-reference inventory failure, got {other:?}"),
        }
    }

    #[test]
    fn session_check_warns_on_missing_pending_done_for_completed_task() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: warn\n---\n\n"),
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
    fn session_check_pending_done_defaults_to_strict_for_session_docs() {
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
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("[session-check] error:"));
                assert!(message.contains("--pending-done 4qja"));
                assert!(message.contains("pending_done_guard = \"warn\""));
            }
            other => panic!("expected default strict-mode failure for session doc, got {other:?}"),
        }
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
    fn session_check_pending_done_detects_icebox_only_open_item() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_tracked_work(
            tmp.path(),
            None,
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented the parked follow-up and verified it.\n",
            false,
            Some("- [ ] [#keep1] Keep backlog item\n"),
            Some("- [ ] [#ice01] Parked follow-up\n"),
            &[],
        );

        let report = inspect_with_warnings(&doc).unwrap();
        match report.status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("--pending-done ice01"));
                assert!(message.contains("#ice01"));
            }
            other => {
                panic!("expected strict-mode failure for icebox-only tracked work, got {other:?}")
            }
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_backlog_items_remain() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: `#reap1` — gpt-5\n\nImplemented.\n",
            false,
            Some("- [x] [#reap1] Completed but not reaped\n"),
            &[],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#reap1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_backlog_items_were_recorded_this_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            None,
            "### Re: `#reap1` — gpt-5\n\nImplemented.\n",
            false,
            Some("- [x] [#reap1] Completed but stranded after closeout\n"),
            &["reap1"],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#reap1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_completed_icebox_items_remain() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_tracked_work(
            tmp.path(),
            None,
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented.\n",
            false,
            Some("- [ ] [#keep1] Keep backlog item\n"),
            Some("- [x] [#ice01] Completed but not reaped\n"),
            &["ice01"],
        );

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("completed tracked item(s)"));
                assert!(message.contains("#ice01"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_interrupts_when_open_backlog_item_exists_only_in_shadow_copy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: off\n---\n\n"),
            "### Re: backlog shadow check — gpt-5\n\nInvestigated.\n",
            false,
            Some("- [ ] [#keep1] Keep live\n"),
            &[],
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&doc)
            .unwrap()
            .write_all(b"\n<!-- parked digest\n- [ ] [#lost1] Drifted copy\n-->\n")
            .unwrap();

        match inspect_with_warnings(&doc).unwrap().status {
            SessionCheckStatus::Interrupted(message) => {
                assert!(message.contains("open backlog item(s) exist only outside"));
                assert!(message.contains("#lost1"));
            }
            other => panic!("expected interrupted status, got {other:?}"),
        }
    }

    #[test]
    fn session_check_warns_when_live_backlog_item_has_shadow_copy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_committed_capture_with_pending(
            tmp.path(),
            Some("---\nagent_doc_session: test\npending_done_guard: off\n---\n\n"),
            "### Re: backlog shadow check — gpt-5\n\nInvestigated.\n",
            false,
            Some("- [ ] [#keep1] Keep live\n"),
            &[],
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&doc)
            .unwrap()
            .write_all(b"\n<!-- parked digest\n- [ ] [#keep1] Duplicate copy\n-->\n")
            .unwrap();

        let report = inspect_with_warnings(&doc).unwrap();
        assert!(matches!(report.status, SessionCheckStatus::Ok(_)));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("outside live agent:backlog"));
        assert!(report.warnings[0].contains("#keep1"));
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

    #[test]
    fn session_check_snapshot_committed_guard_fails_when_snapshot_differs() {
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
        let old_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n";
        fs::write(&doc, old_content).unwrap();
        crate::snapshot::save(&doc, old_content).unwrap();
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

        // Simulate: write applied a response to the snapshot but commit never happened
        let new_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: test\nresponse\n";
        fs::write(&doc, new_content).unwrap();
        crate::snapshot::save(&doc, new_content).unwrap();

        // Mark cycle as committed (simulating a bug where cycle_state lied)
        crate::cycle_state::start_preflight(&doc, Some(old_content), Some(old_content)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(new_content),
            Some(new_content),
        )
        .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(
                    msg.contains("snapshot does not match HEAD"),
                    "expected snapshot-committed guard failure, got: {msg}"
                );
            }
            SessionCheckStatus::Ok(msg) => {
                panic!("expected Interrupted, got Ok: {msg}");
            }
        }
    }

    #[test]
    fn session_check_snapshot_committed_guard_reports_side_effect_recovery_hint() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join("news/2026-05-01")).unwrap();

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
        let news_index = root.join("news/README.md");
        let news_day = root.join("news/2026-05-01/README.md");
        fs::write(&doc, "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n").unwrap();
        fs::write(&news_index, "old news index\n").unwrap();
        fs::write(&news_day, "old daily news\n").unwrap();
        crate::snapshot::save(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n",
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md", "news/README.md", "news/2026-05-01/README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let new_content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\nold body\n### Re: create today's news — codex\nresponse\n";
        fs::write(&doc, new_content).unwrap();
        crate::snapshot::save(&doc, new_content).unwrap();
        fs::write(&news_index, "new news index\n").unwrap();
        fs::write(&news_day, "new daily news\n").unwrap();

        crate::cycle_state::start_preflight(&doc, Some(new_content), Some(new_content)).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(new_content),
            Some(new_content),
        )
        .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Interrupted(msg) => {
                assert!(msg.contains("tracked side-effect edits"));
                assert!(msg.contains("news/README.md"));
                assert!(msg.contains("news/2026-05-01/README.md"));
                assert!(msg.contains("agent-doc write --commit"));
            }
            SessionCheckStatus::Ok(msg) => {
                panic!("expected Interrupted, got Ok: {msg}");
            }
        }
    }

    #[test]
    fn session_check_snapshot_committed_guard_passes_when_committed() {
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
        let content =
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\nbody\n### Re: test\nresponse\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
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

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();

        let status = inspect(&doc).unwrap();
        match status {
            SessionCheckStatus::Ok(_) => {}
            SessionCheckStatus::Interrupted(msg) => {
                panic!("expected Ok, got Interrupted: {msg}");
            }
        }
    }
}
