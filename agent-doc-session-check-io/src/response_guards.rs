use std::path::Path;

use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

/// `#queue-user-edit-overwrite`: fail closed when this cycle recorded a
/// user-authored `agent:queue` edit dropped during a `content_ours` IPC adoption
/// and that queue line is still absent from the committed `HEAD` — unless the
/// current response legitimately consumed it (its `do [#id]` id reached a
/// lifecycle outcome this cycle). A preserved queue line (reached HEAD's queue
/// or exchange) or a consumed head clears the marker; a silently-deleted user
/// queue edit fails closed.
pub fn check_dropped_queue_prompt_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_queue_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    // Unlike the exchange guard, a user queue edit is SUPPOSED to stay out of
    // HEAD: `content_ours` adoption preserves it on disk so it re-surfaces as a
    // next-cycle diff. The loss case is the edit vanishing from the visible
    // document, so check the current file (and HEAD as a committed fallback).
    // Phase 6 (#lr-content-6): cached document content via `DocContentCell`.
    let visible = rc.doc_content();
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let resolved_ids = agent_doc_cycle_state_io::resolved_pending_ids(file)?;
    let still_missing = agent_doc_turn::closeout_signal::still_missing_dropped_queue_prompts(
        &visible,
        head,
        &state.dropped_queue_prompts,
        &resolved_ids,
    );
    if still_missing.is_empty() {
        agent_doc_cycle_state_io::clear_dropped_queue_prompts(file)?;
        return Ok(GuardResult::None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "dropped_queue_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::dropped_queue_prompt_guard_result(
            &file_display,
            &still_missing,
        ),
    )
}

/// `#jb-run-agent-doc-response-queue-contamination`: `Run Agent Doc` / queue
/// synthesis must never enqueue assistant response prose. The live repro added
/// `- Yes. I drove the already-authenticated Google Ads browser session ...`
/// (copied from a `### Re:` body) to `agent:queue auto`. Detect a free-text
/// queue prompt whose text appears inside an assistant response body and fail
/// closed naming the contaminating candidate.
pub fn check_queue_response_contamination_guard(
    file: &Path,
    rc: &CycleContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Ok(GuardResult::None);
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(GuardResult::None);
    };

    let queue_body = &content[queue.open_end..queue.close_start];
    let contaminated = agent_doc_workflow::session_check::queue_response_contamination_candidates(
        queue_body,
        exchange.content(&content),
    );

    if contaminated.is_empty() {
        return Ok(GuardResult::None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_response_contamination_guard_failed file={} count={}",
            file.display(),
            contaminated.len()
        ),
    );
    Ok(agent_doc_workflow::session_check::queue_response_contamination_guard_result(&contaminated))
}

/// `#exchange-prompt-dropped-on-merge`: fail closed when this cycle recorded a
/// user-authored exchange prompt dropped during a `content_ours` IPC adoption
/// and that prompt is still absent from the committed `HEAD`. The evidence is
/// persisted at adoption time, so this guard catches the silent-loss class even
/// when the editor overwrote the disk prompt via IPC buffer convergence before
/// the post-commit disk diff could observe it.
pub fn check_dropped_exchange_prompt_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.dropped_exchange_prompts.is_empty() {
        return Ok(GuardResult::None);
    }
    let head_content = rc.head_content();
    let head = head_content
        .as_deref()
        .map(String::as_str)
        .unwrap_or_default();
    let still_missing = agent_doc_turn::closeout_signal::still_missing_dropped_exchange_prompts(
        head,
        &state.dropped_exchange_prompts,
    );
    if still_missing.is_empty() {
        // The dropped prompt reached the committed document — resolved.
        agent_doc_cycle_state_io::clear_dropped_exchange_prompts(file)?;
        return Ok(GuardResult::None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "dropped_exchange_prompt_guard_failed file={} count={}",
            file.display(),
            still_missing.len()
        ),
    );
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::dropped_exchange_prompt_guard_result(
            &file_display,
            &still_missing,
        ),
    )
}

/// `#reappersistloop`: whether a completed `[x]` id is genuinely stuck.
///
/// An id this cycle reaped is awaiting the next preflight's archive pass, which
/// is the normal post-closeout state and not an interruption. Anything else has
/// survived its own archive pass and still fails closed.
///
/// Ids are compared with the leading `#` stripped and case-folded, because the
/// document writes `[#Foo]` while the cycle records `foo`; a literal comparison
/// silently reports every healthy closeout as stuck.
pub fn completed_item_is_stuck(item_id: &str, reaped_this_cycle: &[String]) -> bool {
    fn normalize(id: &str) -> String {
        id.trim().trim_start_matches('#').to_ascii_lowercase()
    }
    let needle = normalize(item_id);
    if needle.is_empty() {
        // An id-less completed item can never be matched against the reap record,
        // so it stays reportable rather than silently passing.
        return true;
    }
    !reaped_this_cycle
        .iter()
        .any(|reaped| normalize(reaped) == needle)
}

pub fn check_completed_pending_reap_guard(
    file: &Path,
    rc: &CycleContext,
) -> Result<Option<String>> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let completed = agent_doc_element_backlog::backlog::completed_tracked_items_in_components(
        &content,
        &components,
    );
    if completed.is_empty() {
        return Ok(None);
    }

    // `#reappersistloop`: BOTH the archive move and the `[x]` line removal happen
    // in the NEXT preflight's maintenance pass, so a healthy closeout always
    // leaves its own just-completed items visible as `[x]`. Flagging that as
    // INTERRUPTED fires on the happy path by construction -- measured on
    // tasks/agent-doc/agent-doc-bugs.md, six consecutive successful closeouts each
    // reported an interruption with nothing wrong, and the auto-loop skip list
    // treats a session-check interruption as a valid reason to stop draining. It
    // also put three commands into open disagreement: `session-check` said
    // INTERRUPTED, `repair --apply-recovery` said the same document was clean, and
    // `repair` warned that the `commit` those messages named would commit
    // unreviewed drift as metadata.
    //
    // The cycle already records which ids it reaped, so scope the guard to items
    // it did NOT: one that survived its own archive pass is genuinely stuck and
    // still fails closed. Fail SAFE on a missing/unreadable cycle state by keeping
    // the original behavior -- an unprovable reap is the case worth reporting.
    let reaped_this_cycle: Vec<String> =
        agent_doc_cycle_state_io::load_with_closeout_projection(file)
            .ok()
            .flatten()
            .map(|state| state.reaped_pending_ids)
            .unwrap_or_default();

    let stuck: Vec<_> = completed
        .into_iter()
        .filter(|item| completed_item_is_stuck(&item.id, &reaped_this_cycle))
        .collect();
    if stuck.is_empty() {
        return Ok(None);
    }

    let refs = agent_doc_element_backlog::backlog::tracked_item_refs(&stuck).join(", ");
    if refs.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        agent_doc_workflow::session_check::completed_pending_reap_guard_message(&refs),
    ))
}

pub fn check_snapshot_committed_guard(
    file: &Path,
    rc: &CycleContext,
    recovery_hint: impl FnOnce(&Path) -> String,
) -> Result<GuardResult> {
    use agent_doc_snapshot_io::SnapshotCommitStatus;
    match rc.snapshot_commit_status() {
        SnapshotCommitStatus::Committed
        | SnapshotCommitStatus::NoSnapshot
        | SnapshotCommitStatus::NoHead
        | SnapshotCommitStatus::NotInGitRepo => Ok(GuardResult::None),
        SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            if latest_head_response_visible_in_operator_live_buffer(file)? {
                return Ok(GuardResult::None);
            }
            // Phase 3 (#jbccc3): silently treat the auto-recoverable cancel
            // pattern as a non-error here. Standalone `session-check` is then
            // free to surface OK while preflight runs the binary-owned commit
            // through `enforce_no_uncommitted_closeout_drift`. Without this
            // skip, the guard would still bail with the misleading "cycle
            // state is committed but the snapshot does not match HEAD"
            // message that masks the JB cache-conflict cancel root cause.
            if crate::detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
                return Ok(GuardResult::None);
            }
            if current_document_matches_head(file)? {
                return Ok(GuardResult::None);
            }
            let side_effects = agent_doc_git_io::status::tracked_side_effect_note(file)?;
            let recovery_hint = recovery_hint(file);
            let msg = agent_doc_workflow::session_check::snapshot_committed_guard_message(
                snapshot_len,
                head_len,
                &side_effects,
                &recovery_hint,
            );
            eprintln!("{}", msg);
            agent_doc_ops_log_io::log_op(
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

fn current_document_matches_head(file: &Path) -> Result<bool> {
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let current = match crate::resolve_current_document_content(file, "snapshot_committed_guard") {
        Ok(current) => current,
        Err(_) => return Ok(false),
    };
    let comparison =
        agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(&head, &current);
    if !(comparison.is_equal() || comparison.normalized_exchange_equal()) {
        return Ok(false);
    }
    if let Some(heading) =
        agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
            &head, "",
        )
    {
        return Ok(crate::operator_live_buffer_contains_heading(file, &heading));
    }
    Ok(false)
}

fn latest_head_response_visible_in_operator_live_buffer(file: &Path) -> Result<bool> {
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let Some(snapshot) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(false);
    };
    let Some(heading) =
        agent_doc_document::write_normalization::latest_response_heading_missing_from_current(
            &head, &snapshot,
        )
    else {
        return Ok(false);
    };
    Ok(crate::operator_live_buffer_contains_heading(file, &heading))
}

/// `#codex-final-response-not-written`: a completed turn that committed real
/// binary-owned work this cycle but never captured an assistant response body.
///
/// Symptom: an agent (notably a Codex/direct-exec run, or any cycle whose
/// `finalize` landed pending mutations + the commit but lost the response — e.g.
/// a malformed/empty patchback) reaches `Committed` with side effects applied,
/// yet `agent:exchange` has no new `### Re:` close-out. The cycle-state proves
/// it: a real binary write turn sets `had_pending_mutations`, and a captured
/// response always sets `capture_id`/`response_sha256` (see
/// `capture::record` → `cycle_state::mark_response_captured`). So
/// `Committed` + `had_pending_mutations` + no `capture_id` means the write path
/// processed this turn's mutations and committed without ever persisting a
/// response — the missing close-out.
///
/// This is precise rather than broad: a no-op sweep close
/// (`closing cycle as already committed`) never sets `had_pending_mutations`,
/// and any normal response cycle sets `capture_id`, so neither false-fires.
/// Recovery is non-destructive — land the visible response through
/// `agent-doc write --commit`, which sets `capture_id` and clears the guard.
/// True when the committed `agent:exchange` contains at least one assistant
/// `### Re:` response heading (`#codex-queue-drain-no-response-body`). Used to
/// verify a queue-drain turn actually landed a response body in the document
/// rather than only mutating status/queue/backlog. A doc with no exchange
/// component, or an exchange holding only a compacted `### Session Summary`,
/// returns false.
pub fn committed_exchange_has_response_body(file: &Path) -> Result<bool> {
    let doc = crate::resolve_current_document(file, "committed_exchange_has_response_body")?;
    agent_doc_element::element::parse(doc.content())?;
    Ok(agent_doc_turn::closeout_guard::exchange_has_assistant_response_body(doc.content()))
}

pub fn check_committed_without_response_body_guard(
    file: &Path,
    recovery_hint: impl FnOnce(&Path) -> String,
) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    let detail_last_event = state.last_event.as_str();
    let committed_exchange_has_body = committed_exchange_has_response_body(file)?;
    let decision = agent_doc_turn::closeout_guard::committed_without_response_body_decision(
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyEvidence {
            phase: state.phase,
            exchange_has_response_body: committed_exchange_has_body,
            capture_recorded: state.capture_id.is_some(),
            response_hash_recorded: state.response_sha256.is_some(),
            queue_turn: state.queue_task_id.is_some() || !state.active_queue_heads.is_empty(),
            had_pending_mutations: state.had_pending_mutations,
            last_event: detail_last_event,
        },
    );
    match decision {
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyDecision::SkipNoopCommit => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "committed_without_response_body_guard_skipped_noop_commit file={} cycle_id={} last_event={} pending_done={} reaped={}",
                    file.display(),
                    state.cycle_id,
                    detail_last_event,
                    state.pending_done_ids.len(),
                    state.reaped_pending_ids.len(),
                ),
            );
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyDecision::Interrupt => {}
    }
    let side_effects = agent_doc_git_io::status::tracked_side_effect_note(file)?;
    let recovery_hint = recovery_hint(file);
    let msg = agent_doc_workflow::session_check::committed_without_response_body_guard_message(
        &state.cycle_id,
        detail_last_event,
        &side_effects,
        &recovery_hint,
    );
    eprintln!("{}", msg);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "committed_without_response_body_guard_failed file={} cycle_id={} last_event={} had_pending_mutations={} pending_done={} reaped={}",
            file.display(),
            state.cycle_id,
            detail_last_event,
            state.had_pending_mutations,
            state.pending_done_ids.len(),
            state.reaped_pending_ids.len(),
        ),
    );
    Ok(GuardResult::Error(msg))
}

#[cfg(test)]
mod reap_guard_tests {
    use super::completed_item_is_stuck;

    /// `#reappersistloop`: the regression. Both the archive move and the `[x]`
    /// line removal happen on the NEXT preflight, so a healthy closeout always
    /// leaves its own reaped items visible. Reporting those as stuck fires on the
    /// happy path -- six consecutive successful closeouts on
    /// tasks/agent-doc/agent-doc-bugs.md each reported an interruption with
    /// nothing wrong, and a session-check interruption is a valid reason to stop
    /// the drain.
    #[test]
    fn an_item_reaped_this_cycle_is_not_stuck() {
        let reaped = vec!["runsubmitautoproof".to_string()];
        assert!(
            !completed_item_is_stuck("runsubmitautoproof", &reaped),
            "the cycle's own reaped item is awaiting the next preflight, not stuck"
        );
    }

    /// The complement: scoping the guard must not blind it. An item that survived
    /// its own archive pass is the real defect and still fails closed.
    #[test]
    fn an_item_not_reaped_this_cycle_is_still_stuck() {
        let reaped = vec!["somethingelse".to_string()];
        assert!(
            completed_item_is_stuck("leftoverfromlastcycle", &reaped),
            "an item outside this cycle's reap record must still be reported"
        );
        assert!(
            completed_item_is_stuck("anything", &[]),
            "no reap record at all must fail safe to reporting, not to silence"
        );
    }

    /// The document writes `[#Foo]`; the cycle records `foo`. A literal compare
    /// would report every healthy closeout as stuck -- the exact bug, reintroduced
    /// through normalization instead of scoping.
    #[test]
    fn ids_match_across_hash_prefix_and_case() {
        let reaped = vec!["#RunSubmitAutoProof".to_string()];
        assert!(!completed_item_is_stuck("runsubmitautoproof", &reaped));
        assert!(!completed_item_is_stuck("#runsubmitautoproof", &reaped));
        assert!(!completed_item_is_stuck("  RunSubmitAutoProof  ", &reaped));
    }

    /// An id-less completed item cannot be matched against the reap record, so it
    /// must stay reportable rather than pass by accident.
    #[test]
    fn an_id_less_completed_item_stays_reportable() {
        assert!(completed_item_is_stuck("", &["anything".to_string()]));
        assert!(completed_item_is_stuck("   ", &["anything".to_string()]));
        assert!(completed_item_is_stuck("#", &["anything".to_string()]));
    }
}
