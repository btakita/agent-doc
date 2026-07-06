use std::path::Path;

use agent_doc_document_realtime::baseline_comparison::BaselineComparison;
use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};
use agent_doc_turn::document_drift::{
    active_session_drift_is_only_exchange_or_backlog_metadata, exchange_has_new_appended_content,
    exchange_only_promptless_content_drift, promptless_comment_only_drift,
};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

pub fn check_blocked_closeout_followup_guard(
    file: &Path,
    rc: &CycleContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = crate::resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };
    let doc = crate::resolve_current_document(file, "blocked_closeout_followup_guard")?;
    let file = doc.key().as_path();
    let mut still_gated =
        agent_doc_document::tracked_work_projection::open_review_ids(doc.content())
            .into_iter()
            .collect::<Vec<_>>();
    still_gated.sort();

    let unresolved = match agent_doc_turn::closeout_signal::blocked_closeout_followup_decision(
        agent_doc_turn::closeout_signal::BlockedCloseoutFollowupEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.capture_committed,
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

    agent_doc_ops_log_io::log_op(
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
pub fn check_gated_phase_split_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
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
            capture_committed: capture.capture_committed,
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

    agent_doc_ops_log_io::log_op(
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
pub fn check_queue_audit_partial_completion_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };
    match agent_doc_turn::closeout_signal::queue_audit_partial_completion_decision(
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.capture_committed,
            response_body: &capture.response_body,
        },
    ) {
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::QueueAuditPartialCompletionDecision::Warn => {}
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_audit_partial_completion_guard_fired file={}",
            file.display()
        ),
    );

    Ok(agent_doc_workflow::session_check::queue_audit_partial_completion_guard_result())
}

pub fn detect_active_session_post_commit_drift(file: &Path) -> Result<Option<String>> {
    let Some(session) = agent_doc_codex_hook_io::load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    let Some(snapshot) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current =
        crate::resolve_current_document_content(file, "active_session_post_commit_drift")?;
    let comparison = BaselineComparison::new(&snapshot, &current);
    if comparison.is_equal() {
        return Ok(None);
    }
    if comparison.normalized_exchange_equal() {
        return Ok(None);
    }

    let prompt_marker = crate::detect_unstarted_prompt_bearing_diff(file)?;
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

pub fn detect_uncommitted_exchange_drift(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current = crate::resolve_current_document_content(file, "uncommitted_exchange_drift")?;
    if let Some(head) = agent_doc_git_io::revision::show_head(file)? {
        let head_comparison = BaselineComparison::new(&head, &current);
        if head_comparison.is_equal()
            || head_comparison.normalized_exchange_equal()
            || !head_comparison.exchange_has_new_appended_content()
        {
            if let Some(heading) =
                agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
                    &head, "",
                )
            {
                crate::operator_live_buffer_contains_heading(file, &heading);
            }
            return Ok(None);
        }
    }
    let comparison = BaselineComparison::new(&snapshot, &current);
    if comparison.is_equal() {
        return Ok(None);
    }
    if comparison.normalized_exchange_equal() {
        return Ok(None);
    }
    let snapshot_exchange =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(&snapshot);
    let current_exchange =
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(&current);
    if !exchange_has_new_appended_content(&snapshot_exchange, &current_exchange) {
        return Ok(None);
    }
    let prompt_marker = crate::detect_unstarted_prompt_bearing_diff(file)?;
    let detail = match prompt_marker {
        Some(marker) => format!(
            "uncommitted working tree drift beyond snapshot with exchange changes; {}",
            marker
        ),
        None => "uncommitted working tree drift beyond snapshot with exchange changes".to_string(),
    };
    Ok(Some(detail))
}

pub fn open_cycle_message(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> Result<String> {
    let ipc_hint = agent_doc_ops_log_io::latest_ipc_proof_diagnostic_hint(file)?
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

pub fn open_cycle_manual_patchback_message(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
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

pub fn detect_bypassed_response_write(file: &Path) -> Result<Option<String>> {
    let Some(snapshot) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current = crate::resolve_current_document_content(file, "bypassed_response_write")?;
    Ok(agent_doc_turn::document_drift::detect_bypassed_response_write_between(&snapshot, &current))
}

pub fn detect_bypassed_response_write_with_force_disk(
    file: &Path,
    force_disk: bool,
) -> Result<Option<String>> {
    crate::with_force_disk_resolution(force_disk, || detect_bypassed_response_write(file))
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
    let content = crate::resolve_current_document_content(file, "unresolved_exchange_prompt")?;
    Ok(agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&content))
}

pub fn exchange_tail_has_response_heading(file: &Path) -> Result<bool> {
    let content =
        crate::resolve_current_document_content(file, "exchange_tail_has_response_heading")?;
    Ok(agent_doc_turn::exchange_tail::exchange_tail_has_response_heading(&content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn queue_audit_guard_prefers_lazily_projection_over_stale_open_sidecar() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\n";
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle state path");
        let stale_open_sidecar = fs::read(&sidecar_path).unwrap();
        let response = "### Re: which queue items are complete?\n\nNone of the six queue items are complete. Same-day QA is complete and the URL validate-only check was clean, but each row still has at least one remaining action.";
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(content),
            Some(content),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        agent_doc_capture_io::mark_committed_with_current_content(&doc, content).unwrap();
        fs::remove_file(agent_doc_capture_io::capture_path_for(&doc, &capture.capture_id).unwrap())
            .unwrap();
        fs::write(&sidecar_path, stale_open_sidecar).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );

        let result = check_queue_audit_partial_completion_guard(&doc).unwrap();
        assert!(
            matches!(result, GuardResult::Warn(_)),
            "lazily committed projection should make the guard evaluate despite stale open JSON and missing capture JSON: {result:?}"
        );
    }
}
