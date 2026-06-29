use super::*;

pub(crate) fn check_no_response_active_queue_head(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_core::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
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
    let recorded_ids = do_directive_target_ids(&state.active_queue_heads);
    if recorded_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let content = rc.doc_content();
    let current_head_ids: std::collections::HashSet<String> =
        committed_current_queue_head_ids(&content)
            .into_iter()
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
            .collect();
    let open_backlog: std::collections::HashSet<String> =
        open_backlog_ids(file)?.into_iter().collect();
    let mut resolved_or_deferred = crate::cycle_state::resolved_pending_ids(file)?;
    resolved_or_deferred.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id)),
    );
    // #goqueuestall/#qcontdrain: a queue head whose backlog id is DEFERRED (never
    // agent-drainable — `[operator-verify]` only) is not a "runnable" head that a
    // no-response reap-only closeout silently dropped. Exclude it from the live set,
    // reusing the SAME deferred set that `queue_continuation` uses so the
    // session-check guard and continuation logic agree. After excluding deferred
    // heads, only genuinely drainable heads can still trip this guard.
    let deferred: std::collections::HashSet<String> =
        crate::queue_continuation::deferred_backlog_ids(&content)
            .into_iter()
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
            .collect();

    let mut live: Vec<String> = Vec::new();
    for id in recorded_ids {
        let norm = agent_doc_element_backlog::backlog::normalize_pending_id(&id);
        if norm.is_empty() {
            continue;
        }
        if !current_head_ids.contains(&norm) || !open_backlog.contains(&norm) {
            continue;
        }
        if resolved_or_deferred.contains(&norm) || deferred.contains(&norm) {
            continue;
        }
        if !live.iter().any(|existing| existing == &norm) {
            live.push(norm);
        }
    }

    if live.is_empty() {
        return Ok(GuardResult::None);
    }

    let ids = live
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "no_response_active_queue_head_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            live.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: cycle `{}` committed without an assistant response body while runnable agent:queue head(s) {} remained queued and open in agent:backlog; this was a no-response repair/reap-only closeout, not a completed queue turn",
        state.cycle_id, ids
    );
    let repair = format!(
        "run `agent-doc {}` from the owning session so the queued head is answered, or resolve each id through `agent-doc write --commit {}` with `--done`, `--pending-gate`, or `--pending-edit` proof before closing",
        file.display(),
        file.display()
    );

    Ok(match mode {
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #nochange-after-stall-breadth)"),
        ]),
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Strict => {
            GuardResult::Error(format!(
                "{}\n[session-check] hint: {repair} (see #nochange-after-stall-breadth)",
                warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            ))
        }
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
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
pub(crate) fn check_reaped_queue_head_without_response(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_core::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if !matches!(state.phase, crate::cycle_state::CyclePhase::Committed) {
        return Ok(GuardResult::None);
    }
    if state.reaped_pending_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    let directive_ids: std::collections::HashSet<String> =
        do_directive_target_ids(&state.active_queue_heads)
            .into_iter()
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
            .filter(|id| !id.is_empty())
            .collect();
    if directive_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    let reaped: std::collections::HashSet<String> = state
        .reaped_pending_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    let content = rc.doc_content();
    let head = crate::git::show_head(file).ok().flatten();
    let archives: Vec<String> = head
        .as_deref()
        .map(|head| {
            crate::flow::closeout::compact_archive_pointers(head)
                .into_iter()
                .filter_map(|pointer| {
                    crate::flow::closeout::read_head_compact_archive(file, pointer)
                })
                .collect()
        })
        .unwrap_or_default();

    // Reaped `do #id` directive heads, deterministically ordered so the `bkx9`
    // diagnostic and the detector input are stable across runs.
    let mut ordered_ids: Vec<String> = directive_ids
        .into_iter()
        .filter(|id| reaped.contains(id))
        .collect();
    ordered_ids.sort();
    if ordered_ids.is_empty() {
        return Ok(GuardResult::None);
    }

    // #bkx9wire: per-id response-loss diagnostic. Emitted even when a response was
    // captured this cycle, so a reproduced `#ipc-crdt-response-drift` (found=false)
    // is catchable from ops.log and a multi-id-under-one-heading cycle (found=true
    // for each id) proves no false positive — no live-verify needed.
    for id in &ordered_ids {
        let source = directive_response_source(&content, &archives, id);
        crate::ops_log::log_op(
            file,
            &format!(
                "bkx9 directive_response_materialized id={} found={} source={}",
                id,
                source.is_some(),
                source.map_or("none", ResponseSource::as_str),
            ),
        );
    }

    // Canonical lost set via the now-wired per-id detector (#z2jy bkx9-pure-detector).
    let lost = reaped_directive_ids_without_response(&ReapedResponseLossInput {
        directive_ids: &ordered_ids,
        reaped_ids: &ordered_ids,
        content: &content,
        archives: &archives,
    });

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

    let ids = lost
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    crate::ops_log::log_op(
        file,
        &format!(
            "reaped_queue_head_without_response_fired file={} cycle_id={} last_event={} ids={}",
            file.display(),
            state.cycle_id,
            state.last_event,
            lost.join(",")
        ),
    );
    let warn_line = format!(
        "[session-check] warn: cycle `{}` reaped `do` queue-directive head(s) {} into agent:done without an assistant response landing in agent:exchange (no response body this cycle and no `### Re:` for the id in the exchange or a HEAD compact archive); the response record was silently lost",
        state.cycle_id, ids
    );
    let repair = format!(
        "recover the lost response by re-running `agent-doc {}` so the directive id is answered, or restore the missing `### Re:` block through `agent-doc write --commit {}` before closing",
        file.display(),
        file.display()
    );

    Ok(match mode {
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!("[session-check] hint: {repair} (see #compact-reap-no-response-record)"),
        ]),
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Strict => {
            GuardResult::Error(format!(
                "{}\n[session-check] hint: {repair} (see #compact-reap-no-response-record)",
                warn_line.replacen("[session-check] warn:", "[session-check] INTERRUPTED:", 1),
            ))
        }
        agent_doc_core::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}
