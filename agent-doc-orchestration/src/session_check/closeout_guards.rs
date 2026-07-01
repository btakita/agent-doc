use super::*;
use agent_doc_turn::document_drift::{
    active_session_drift_is_only_exchange_or_backlog_metadata, exchange_has_new_appended_content,
    exchange_only_promptless_content_drift, promptless_comment_only_drift,
};
use agent_doc_turn::op_log::{
    IPC_PROOF_INSUFFICIENT_EVENT, is_write_completed_commit_missing_event, strip_timestamp_prefix,
};

pub(crate) fn check_blocked_closeout_followup_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    let content = std::fs::read_to_string(file)?;
    let mut still_gated = agent_doc_document::tracked_work_projection::open_review_ids(&content)
        .into_iter()
        .collect::<Vec<_>>();
    still_gated.sort();

    let unresolved = match agent_doc_turn::closeout_signal::blocked_closeout_followup_decision(
        agent_doc_turn::closeout_signal::BlockedCloseoutFollowupEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.state
                == agent_doc_workflow::capture::CaptureState::Committed,
            pending_added_this_cycle: state.pending_added_this_cycle,
            response_body: &capture.response_body,
            directed_ids: &state.expect_done_or_gate_ids,
            pending_kept_open_ids: &state.pending_kept_open_ids,
            pending_done_ids: &state.pending_done_ids,
            pending_gated_ids: &state.pending_gated_ids,
            still_gated_ids: &still_gated,
        },
    ) {
        agent_doc_turn::closeout_signal::BlockedCloseoutFollowupDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::BlockedCloseoutFollowupDecision::Warn {
            unresolved_ids,
        } => unresolved_ids,
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "blocked_closeout_followup_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::blocked_closeout_followup_guard_result(
            &file_display,
            &unresolved,
            mode,
        ),
    )
}

/// `#gated-followup-split-enforcement`: when a directed `do [#id]` cycle keeps a
/// multi-phase item open (via `--backlog-edit` / `--review-edit` /
/// `--backlog-gate`) whose body enumerates several gated/remaining phases but
/// never breaks them out into discrete child backlog IDs, the deferred phases
/// stay buried in one parent's narrowed description and are not independently
/// trackable or queueable. Advise splitting each phase into its own child ID
/// (sibling of `#blocked-closeout-followup-capture` and the SKILL "one backlog
/// ID per actionable phase" rule).
///
/// Warn-first advisory only — it never blocks closeout. Suppressible via a
/// `<!-- no-gated-phase-split-guard -->` response marker.
pub(crate) fn check_gated_phase_split_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };

    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let mut tracked_items = Vec::new();
    for component in components.iter() {
        let trackable = agent_doc_element::element::is_backlog_component(&component.name)
            || agent_doc_element::element::is_review_component(&component.name);
        if !trackable {
            continue;
        }
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(component.content(&content));
        for item in items {
            let done = item.is_done();
            let body = format!("{} {}", item.text, item.continuation);
            tracked_items.push(
                agent_doc_turn::closeout_signal::GatedPhaseSplitItemEvidence {
                    id: item.id,
                    done,
                    body,
                },
            );
        }
    }

    let flagged = match agent_doc_turn::closeout_signal::gated_phase_split_decision(
        agent_doc_turn::closeout_signal::GatedPhaseSplitEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.state
                == agent_doc_workflow::capture::CaptureState::Committed,
            response_body: &capture.response_body,
            directed_ids: &state.expect_done_or_gate_ids,
            pending_kept_open_ids: &state.pending_kept_open_ids,
            tracked_items: &tracked_items,
        },
    ) {
        agent_doc_turn::closeout_signal::GatedPhaseSplitDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::GatedPhaseSplitDecision::Warn { flagged_ids } => {
            flagged_ids
        }
    };

    crate::ops_log::log_op(
        file,
        &format!(
            "gated_phase_split_guard_fired file={} flagged={}",
            file.display(),
            flagged.join(",")
        ),
    );

    let file_display = file.display().to_string();
    Ok(agent_doc_workflow::session_check::gated_phase_split_guard_result(&file_display, &flagged))
}

/// `#queue-audit-partial-completion`: detect a queue-completion audit response
/// that collapses meaningful partial progress into a blanket "none complete."
///
/// A queue audit ("which queue items are complete?") should classify each row as
/// complete / partially complete / not-started, naming completed substeps and the
/// exact remaining condition — not answer "none are complete" just because every
/// row still has one remaining action. This warn-first guard fires only on the
/// clearest collapse signal: the response is about the queue, makes a blanket
/// none-complete claim, shows at least two distinct substep-completion signals,
/// and never frames anything as "partial." It is WARN-only (never blocks
/// closeout) and suppressed by a `<!-- no-queue-audit-guard -->` marker.
///
/// The richer per-row state table is response guidance (a natural-language
/// judgment that lives in the skill/spec contract, per the binary-vs-skill rule),
/// so the binary only flags the unambiguous collapse rather than trying to
/// classify free-text rows itself.
pub(crate) fn check_queue_audit_partial_completion_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    match agent_doc_turn::closeout_signal::queue_audit_partial_completion_decision(
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.state
                == agent_doc_workflow::capture::CaptureState::Committed,
            response_body: &capture.response_body,
        },
    ) {
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionDecision::Warn => {}
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "queue_audit_partial_completion_guard_fired file={}",
            file.display()
        ),
    );

    Ok(agent_doc_workflow::session_check::queue_audit_partial_completion_guard_result())
}

pub(crate) fn detect_active_session_post_commit_drift(file: &Path) -> Result<Option<String>> {
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
    if agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(&current)
        == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            &snapshot,
        )
    {
        return Ok(None);
    }

    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    if prompt_marker.is_none()
        && active_session_drift_is_only_exchange_or_backlog_metadata(&snapshot, &current)
    {
        return Ok(None);
    }
    if prompt_marker.is_none() && promptless_comment_only_drift(&snapshot, &current) {
        return Ok(None);
    }
    if prompt_marker.is_none() && exchange_only_promptless_content_drift(&snapshot, &current) {
        return Ok(None);
    }
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

pub(crate) fn detect_uncommitted_exchange_drift(file: &Path) -> Result<Option<String>> {
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
    let norm_current =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(&current);
    let norm_snapshot =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(&snapshot);
    if norm_current == norm_snapshot {
        return Ok(None);
    }
    if !exchange_has_new_appended_content(&norm_snapshot, &norm_current) {
        return Ok(None);
    }
    let prompt_marker = detect_unstarted_prompt_bearing_diff(file)?;
    let detail = match prompt_marker {
        Some(marker) => format!(
            "uncommitted working tree drift beyond snapshot with exchange changes; {}",
            marker
        ),
        None => "uncommitted working tree drift beyond snapshot with exchange changes".to_string(),
    };
    Ok(Some(detail))
}

pub(crate) fn open_cycle_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<String> {
    let ipc_hint = latest_ipc_proof_diagnostic_hint(file)?
        .map(|hint| format!(" {hint}"))
        .unwrap_or_default();
    Ok(agent_doc_workflow::session_check::open_cycle_message(
        agent_doc_workflow::session_check::OpenCycleMessage {
            file: &state.file,
            cycle_id: &state.cycle_id,
            phase: state.phase,
            last_event: &state.last_event,
            ipc_hint: &ipc_hint,
        },
    ))
}

pub(crate) fn open_cycle_manual_patchback_message(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Result<Option<String>> {
    if !matches!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted) {
        return Ok(None);
    }
    let Some(marker) = detect_bypassed_response_write(file)? else {
        return Ok(None);
    };
    let file_display = file.display().to_string();
    Ok(Some(
        agent_doc_workflow::session_check::open_cycle_manual_patchback_message(
            agent_doc_workflow::session_check::OpenCycleManualPatchbackMessage {
                file: &file_display,
                cycle_id: &state.cycle_id,
                phase: state.phase,
                last_event: &state.last_event,
                marker: &marker,
            },
        ),
    ))
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
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = agent_doc_fs::read_optional_text(&log_path)? else {
        return Ok(None);
    };
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

pub fn latest_ipc_proof_diagnostic(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = agent_doc_fs::read_optional_text(&log_path)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let requested_display = file.display().to_string();
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .map(strip_timestamp_prefix)
        .find(|event| {
            event.starts_with(IPC_PROOF_INSUFFICIENT_EVENT)
                && (event.contains(&format!("file={canonical_display}"))
                    || event.contains(&format!("file={requested_display}")))
        })
        .map(str::to_string))
}

pub fn latest_ipc_proof_diagnostic_hint(file: &Path) -> Result<Option<String>> {
    Ok(latest_ipc_proof_diagnostic(file)?
        .map(|event| format!("latest IPC proof diagnostic: {event}")))
}

pub fn detect_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    Ok(last_ops_event(file)?.filter(|event| is_write_completed_commit_missing_event(event)))
}

pub fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = crate::snapshot::load(file)? else {
        return Ok(None);
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    Ok(agent_doc_turn::document_drift::detect_bypassed_response_write_between(&snapshot, &current))
}

/// `#prompt-preempts-auto-queue`: snapshot-independent detection of a live
/// unresolved user prompt in `agent:exchange`. A prompt is unresolved when there
/// is user-authored, non-comment text after the latest `agent:boundary` marker
/// in the exchange and no `### Re:` response heading follows it in that tail
/// segment. Unlike the snapshot-diff path, this fires even when the prompt was
/// already baselined into the snapshot (so the ordinary diff sees only queue
/// bookkeeping). Returns the joined prompt text, or `None` when the tail is
/// empty or already answered.
pub fn unresolved_exchange_prompt(file: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&content))
}

pub(crate) fn exchange_tail_has_response_heading(file: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    agent_doc_turn::exchange_tail::exchange_tail_has_response_heading(&content)
}

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let Some(change) = first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let label = match change.kind {
        agent_doc_diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        agent_doc_diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
        | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    Ok(Some(format!("{label}: {preview}")))
}

pub fn first_unstarted_prompt_bearing_change(
    file: &Path,
) -> Result<Option<agent_doc_diff::PromptBearingChange>> {
    // A fresh session can carry an unanswered exchange tail prompt before any
    // cycle snapshot exists. The queue path activates independently of the
    // snapshot (route queue activation re-saves the snapshot on activation), so
    // a queue write always dispatches; the exchange path relies on this diff, so
    // without a snapshot we must fall back to the committed `HEAD` blob (then to
    // an empty baseline for untracked docs) — otherwise the exchange prompt is
    // invisible and `Run Agent Doc` does nothing while the same write into the
    // queue starts a turn (#codex-exchange-prompt-no-dispatch).
    let baseline = match crate::snapshot::load(file)? {
        Some(snapshot) => snapshot,
        None => crate::git::show_head(file)?.unwrap_or_default(),
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let norm = |s: &str| {
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(s)
    };
    let snap_norm =
        norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(&baseline));
    let cur_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(&current));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };
    Ok(agent_doc_diff::first_unstarted_prompt_bearing_change_from_diff(&diff_text, &current))
}
