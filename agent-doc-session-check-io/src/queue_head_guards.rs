use std::path::Path;

use agent_doc_run_context_io::RunContext;
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

use crate::resolve_pending_done_guard_mode_with_context;

pub fn check_no_response_active_queue_head(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, agent_doc_turn::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        return Ok(GuardResult::None);
    }
    let bookkeeping_evidence = state.had_pending_mutations
        || !state.pending_done_ids.is_empty()
        || !state.pending_kept_open_ids.is_empty()
        || !state.reaped_pending_ids.is_empty()
        || !state.pending_gated_ids.is_empty()
        || state.pending_added_this_cycle;
    if !bookkeeping_evidence {
        return Ok(GuardResult::None);
    }
    let content = rc.doc_content();
    let open_backlog: std::collections::HashSet<String> =
        agent_doc_document::tracked_work_projection::open_backlog_ids(&content)
            .into_iter()
            .collect();
    let mut resolved_or_deferred = agent_doc_cycle_state_io::resolved_pending_ids(file)?;
    resolved_or_deferred.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id)),
    );
    let live = agent_doc_queue::queue_closeout_guard::no_response_live_queue_head_ids(
        &state.active_queue_heads,
        &content,
        &open_backlog,
        &resolved_or_deferred,
    );

    if live.is_empty() {
        return Ok(GuardResult::None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "no_response_active_queue_head_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            live.join(",")
        ),
    );
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::no_response_active_queue_head_result(
            &file_display,
            &state.cycle_id,
            &live,
            mode,
        ),
    )
}

/// `#compact-reap-no-response-record`: a reap-only / no-response-body closeout
/// that reaps a `do #id` queue-directive head this cycle, where that id's
/// `### Re:` response is absent from both the live exchange and any
/// HEAD-referenced compact archive, has silently lost the response record.
///
/// This is the gap left by [`check_no_response_active_queue_head`], which only
/// fires while the head is still queued *and* open in `agent:backlog`. Once a
/// maintenance / compaction reap removes the id from `agent:backlog` (and strikes
/// it from the queue), that guard's `current_head_ids ∧ open_backlog` condition is
/// false, so the silent loss goes undetected and `finalize --done` later fails
/// with "id not found in backlog".
///
/// The precondition `capture_id.is_none() && response_sha256.is_none()` scopes the
/// guard to reap-only / bookkeeping closeouts: a real response cycle records a
/// capture, so its reaps are answered (not lost) and never reach this guard. A
/// legitimate prior-cycle reap (the id was answered in an earlier cycle and only
/// reaped now) is filtered out by [`reaped_directive_ids_without_response`], which
/// finds the `### Re: ... #id` heading in the live exchange or a HEAD compact archive.
pub fn check_reaped_queue_head_without_response(
    file: &Path,
    rc: &RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, agent_doc_turn::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    if state.reaped_pending_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let ordered_ids = agent_doc_queue::queue_closeout_guard::reaped_queue_directive_head_ids(
        &state.active_queue_heads,
        &state.reaped_pending_ids,
    );
    if ordered_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let content = rc.doc_content();
    let head = agent_doc_git_io::revision::show_head(file).ok().flatten();
    let archives: Vec<String> = head
        .as_deref()
        .map(|head| agent_doc_archive_io::read_head_compact_archives(file, head))
        .unwrap_or_default();

    // #bkx9wire: per-id response-loss diagnostic. Emitted even when a response was
    // captured this cycle, so a reproduced `#ipc-crdt-response-drift` (found=false)
    // is catchable from ops.log and a multi-id-under-one-heading cycle (found=true
    // for each id) proves no false positive — no live-verify needed.
    for id in &ordered_ids {
        let source =
            agent_doc_turn::closeout_signal::directive_response_source(&content, &archives, id);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "bkx9 directive_response_materialized id={} found={} source={}",
                id,
                source.is_some(),
                source.map_or(
                    "none",
                    agent_doc_turn::closeout_signal::ResponseSource::as_str
                ),
            ),
        );
    }

    // Canonical lost set via the now-wired per-id detector (#z2jy bkx9-pure-detector).
    let lost = agent_doc_turn::closeout_signal::reaped_directive_ids_without_response(
        &agent_doc_turn::closeout_signal::ReapedResponseLossInput {
            directive_ids: &ordered_ids,
            reaped_ids: &ordered_ids,
            content: &content,
            archives: &archives,
        },
    );

    // Guard ESCALATION stays scoped to reap-only / bookkeeping closeouts: when a
    // response was captured this cycle the diagnostic above still records any
    // captured-but-id-lost residual, but a false positive on the known multi-id
    // single-heading class must never wedge a committed closeout — so do not escalate.
    if state.capture_id.is_some() || state.response_sha256.is_some() {
        return Ok(GuardResult::None);
    }

    if lost.is_empty() {
        return Ok(GuardResult::None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "reaped_queue_head_without_response_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            lost.join(",")
        ),
    );
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::reaped_queue_head_without_response_result(
            &file_display,
            &state.cycle_id,
            &lost,
            mode,
        ),
    )
}
