//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// `#queue-user-edit-overwrite`: fail closed when this cycle recorded a
/// user-authored `agent:queue` edit dropped during a `content_ours` IPC adoption
/// and that queue line is still absent from the committed `HEAD` — unless the
/// current response legitimately consumed it (its `do [#id]` id reached a
/// lifecycle outcome this cycle). A preserved queue line (reached HEAD's queue
/// or exchange) or a consumed head clears the marker; a silently-deleted user
/// queue edit fails closed.
pub(crate) fn check_dropped_queue_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
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
pub(crate) fn check_queue_response_contamination_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
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
pub(crate) fn check_dropped_exchange_prompt_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
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

pub(crate) fn check_completed_pending_reap_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
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

    let refs = agent_doc_element_backlog::backlog::tracked_item_refs(&completed).join(", ");
    if refs.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        agent_doc_workflow::session_check::completed_pending_reap_guard_message(&refs),
    ))
}

pub(crate) fn check_snapshot_committed_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
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
            // Phase 3 (#jbccc3): silently treat the auto-recoverable cancel
            // pattern as a non-error here. Standalone `session-check` is then
            // free to surface OK while preflight runs the binary-owned commit
            // through `enforce_no_uncommitted_closeout_drift`. Without this
            // skip, the guard would still bail with the misleading "cycle
            // state is committed but the snapshot does not match HEAD"
            // message that masks the JB cache-conflict cancel root cause.
            if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
                return Ok(GuardResult::None);
            }
            let side_effects = agent_doc_git_io::status::tracked_side_effect_note(file)?;
            let recovery_hint = closeout_recovery_hint(file);
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

pub(crate) fn closeout_recovery_hint(file: &Path) -> String {
    // `#closeout-repair-churn`: render one typed recovery instruction for the
    // classified state instead of a single static "try write --commit" line.
    let state = crate::flow::closeout::classify_closeout_recovery_state_for_file(file);
    match crate::flow::closeout::closeout_recovery_command_for_file(file, state) {
        Some(command) => format!("Recovery [{}]: {}.", state.as_str(), command),
        None => format!(
            "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
            file.display(),
            file.display()
        ),
    }
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
pub(crate) fn committed_exchange_has_response_body(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    agent_doc_element::element::parse(&content)?;
    Ok(agent_doc_turn::closeout_guard::exchange_has_assistant_response_body(&content))
}

pub(crate) fn check_committed_without_response_body_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(GuardResult::None);
    };
    let committed_exchange_has_body = committed_exchange_has_response_body(file)?;
    let decision = agent_doc_turn::closeout_guard::committed_without_response_body_decision(
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyEvidence {
            phase: state.phase,
            exchange_has_response_body: committed_exchange_has_body,
            capture_recorded: state.capture_id.is_some(),
            response_hash_recorded: state.response_sha256.is_some(),
            queue_turn: state.queue_task_id.is_some() || !state.active_queue_heads.is_empty(),
            had_pending_mutations: state.had_pending_mutations,
            last_event: &state.last_event,
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
                    state.last_event,
                    state.pending_done_ids.len(),
                    state.reaped_pending_ids.len(),
                ),
            );
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_guard::CommittedWithoutResponseBodyDecision::Interrupt => {}
    }
    let side_effects = agent_doc_git_io::status::tracked_side_effect_note(file)?;
    let recovery_hint = closeout_recovery_hint(file);
    let msg = agent_doc_workflow::session_check::committed_without_response_body_guard_message(
        &state.cycle_id,
        &state.last_event,
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
            state.last_event,
            state.had_pending_mutations,
            state.pending_done_ids.len(),
            state.reaped_pending_ids.len(),
        ),
    );
    Ok(GuardResult::Error(msg))
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process::Command;
    #[test]
    fn committed_without_response_body_guard_passes_recovered_exchange_body_without_capture_metadata()
     {
        // Recovery may commit a visible `### Re:` after the original queue-drain
        // cycle lost its capture metadata. The committed exchange body is still
        // sufficient proof that the missing-response closeout has been repaired.
        let _lock = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nCompacted.\n\n",
            "### Re: do [#ipc1] / do [#39c5]\n\nRecovered.\n",
            "<!-- /agent:exchange -->\n"
        )
        .to_string();
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::save(&doc, &content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();
        agent_doc_cycle_state_io::record_pending_done_ids(
            &doc,
            &["ipc1".to_string(), "39c5".to_string()],
        )
        .unwrap();
        agent_doc_cycle_state_io::record_active_queue_heads(
            &doc,
            &["do [#ipc1]".to_string(), "do [#39c5]".to_string()],
        )
        .unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some(&content),
            Some(&content),
        )
        .unwrap();

        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }
    #[test]
    fn committed_without_response_body_guard_skips_noop_commit_reap_only_cycle() {
        // Deadlock repro (tsift.md cycle-1780257680821): a `finalize --done X` whose
        // only effect was reaping an item already reflected in HEAD commits a no-op
        // (`commit_already_current`) and sets `had_pending_mutations`, but writes no
        // response body. The guard must NOT fire — a no-op commit committed no
        // binary-owned work this turn, so there is nothing a response would
        // accompany; firing wedges the cycle in an infinite
        // session-check-interrupted loop because the `write --commit` recovery is
        // itself a no-op. A real side-effect commit (`commit_success`) still fires.
        let _lock = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let current =
            "---\nagent_doc_session: test\n---\n\n## Exchange\n\ndo [#nsga4verify]\n".to_string();
        fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::save(&doc, &current, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&current), Some(&current)).unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();
        agent_doc_cycle_state_io::record_pending_done_ids(&doc, &["nsga4verify".to_string()])
            .unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_already_current",
            Some(&current),
            Some(&current),
        )
        .unwrap();
        assert!(matches!(
            check_committed_without_response_body_guard(&doc).unwrap(),
            GuardResult::None
        ));
    }
}
