use std::path::Path;

use agent_doc_run_context_io::RunContext;
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

use crate::resolve_pending_done_guard_mode_with_context;

pub fn check_expect_done_or_gate_guard(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.expect_done_or_gate_ids.is_empty() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = agent_doc_capture_io::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };

    let doc = crate::resolve_current_document(file, "expect_done_or_gate_guard")?;
    let file = doc.key().as_path();
    let open_backlog_ids =
        agent_doc_document::tracked_work_projection::open_backlog_ids(doc.content());
    let unresolved = match agent_doc_turn::closeout_signal::expect_done_or_gate_decision(
        agent_doc_turn::closeout_signal::ExpectDoneOrGateEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.state
                == agent_doc_workflow::capture::CaptureState::Committed,
            response_body: &capture.response_body,
            directed_ids: &state.expect_done_or_gate_ids,
            pending_done_ids: &state.pending_done_ids,
            pending_kept_open_ids: &state.pending_kept_open_ids,
            reaped_pending_ids: &state.reaped_pending_ids,
            open_backlog_ids: &open_backlog_ids,
        },
    ) {
        agent_doc_turn::closeout_signal::ExpectDoneOrGateDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::ExpectDoneOrGateDecision::Warn { unresolved_ids } => {
            unresolved_ids
        }
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "expect_done_or_gate_guard_fired file={} unresolved={}",
            file.display(),
            unresolved.join(",")
        ),
    );

    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::expect_done_or_gate_guard_result(
            &file_display,
            &unresolved,
            mode,
        ),
    )
}

/// `#queue-clear-unrun-items`: an active `agent:queue` head is executable user
/// intent. A closeout / reset / commit may delete a runnable `do [#id]` head
/// only with durable proof that it was consumed (this cycle's directive target,
/// owned by `#do-id-closeout-open-backlog`), resolved (its `#id` left
/// `agent:backlog` via done/gate/reap), or removed by an explicit user edit.
/// When a head present in the visible queue at preflight disappears from the
/// committed queue while its `#id` is STILL OPEN in `agent:backlog` and the
/// cycle never targeted it, fail closed and name each lost id so the queue can
/// be restored. Suppress an intentional user removal with
/// `<!-- no-queue-removal-guard -->`.
pub fn check_queue_head_removal_guard(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    // Only enforce on a committed closeout; an open cycle is still mid-flight and
    // may not have written the final queue yet.
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let doc = crate::resolve_current_document(file, "queue_head_removal_guard")?;
    let file = doc.key().as_path();
    let content = doc.content();
    // Explicit user removal already reconciled — do not second-guess it.
    if content.contains("<!-- no-queue-removal-guard -->") {
        return Ok(GuardResult::None);
    }

    let open_backlog: std::collections::HashSet<String> =
        agent_doc_document::tracked_work_projection::open_backlog_ids(content)
            .into_iter()
            .collect();
    // Lifecycle proof: ids the cycle explicitly resolved (done/reaped/gated) or
    // chose to keep open via an explicit edit. A done/gate/reap also removes the
    // id from `open_backlog`, so this set is a defensive superset.
    let mut resolved: std::collections::HashSet<String> =
        agent_doc_cycle_state_io::resolved_pending_ids(file)?;
    resolved.extend(
        state
            .pending_gated_ids
            .iter()
            .chain(state.pending_kept_open_ids.iter())
            .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id)),
    );
    // This cycle's `do [#id]` directive targets are owned by the
    // `expect_done_or_gate` guard, which reports the open-target class with a
    // more specific repair. Skip them here to avoid double-firing.
    let directive_targets: std::collections::HashSet<String> = state
        .expect_done_or_gate_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .collect();

    let decision = agent_doc_queue::queue_closeout_guard::queue_head_removal_decision(
        &state.active_queue_heads,
        &content,
        &open_backlog,
        &resolved,
        &directive_targets,
    );

    for proof in &decision.removal_proofs {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_head_removal_guard_proof file={} removed=#{} proof_source={}",
                file.display(),
                proof.id,
                proof.source.as_str()
            ),
        );
    }

    if decision.lost.is_empty() {
        return Ok(GuardResult::None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_head_removal_guard_fired file={} lost={} proof_source=missing",
            file.display(),
            decision.lost.join(",")
        ),
    );

    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::queue_head_removal_guard_result(
            &file_display,
            &decision.lost,
            mode,
        ),
    )
}

/// `#lr-queue-patchback-miss`: require committed-response / deferral
/// proof for each free-text (non-`do [#id]`) queue head recorded at preflight.
/// Free-text heads have no backlog id, so the guard checks that: (a) the head
/// text is still present in the committed queue (deferral / not yet consumed),
/// or (b) a committed `### Re:` response exists that plausibly answers it. A
/// binary consume marker by itself is not proof: the answer must be visible in
/// committed `agent:exchange` history, normally via the queue-prompt echo.
pub fn check_free_text_queue_head_provenance(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.active_free_text_queue_heads.is_empty() {
        return Ok(GuardResult::None);
    }
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let content = rc.doc_content();
    let Some(decision) =
        agent_doc_queue::queue_closeout_guard::free_text_queue_head_provenance_decision(
            &state.active_free_text_queue_heads,
            &content,
        )
    else {
        return Ok(GuardResult::None);
    };
    if decision.suppressed {
        if decision.bare_heading_residue {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "free_text_queue_marker_residue_fired file={} residue=bare_heading",
                    file.display()
                ),
            );
            let file_display = file.display().to_string();
            return Ok(
                agent_doc_workflow::session_check::free_text_queue_marker_residue_result(
                    &file_display,
                ),
            );
        }
        return Ok(GuardResult::None);
    }
    if !decision.completed_residue.is_empty() {
        let heads_text = decision
            .completed_residue
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
            .join("; ");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "free_text_queue_completed_residue_guard_fired file={} residue={}",
                file.display(),
                heads_text
            ),
        );
        let file_display = file.display().to_string();
        return Ok(
            agent_doc_workflow::session_check::free_text_queue_completed_residue_result(
                &file_display,
                &decision.completed_residue,
            ),
        );
    }
    if !decision.response_proven_removed.is_empty() {
        let heads_text = decision
            .response_proven_removed
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
            .join("; ");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "free_text_queue_head_provenance_proof file={} removed={} proof_source=committed_response",
                file.display(),
                heads_text
            ),
        );
    }
    if decision.unresolved.is_empty() {
        return Ok(GuardResult::None);
    }
    let heads_text = decision
        .unresolved
        .iter()
        .map(|h| format!("\"{}\"", h))
        .collect::<Vec<_>>()
        .join(", ");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "free_text_queue_head_provenance_guard_fired file={} unresolved={}",
            file.display(),
            heads_text
        ),
    );
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::free_text_queue_head_provenance_guard_result(
            &file_display,
            &decision.unresolved,
            mode,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct NoopPipelineFrontmatterEffects;

    impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
        for NoopPipelineFrontmatterEffects
    {
        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> Result<()> {
            fs::write(file, target_content)?;
            Ok(())
        }

        fn log_op(&self, file: &Path, message: &str) {
            agent_doc_ops_log_io::log_op(file, message);
        }
    }

    fn make_doc(root: &Path, content: &str) -> PathBuf {
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("doc.md");
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        doc
    }

    fn mark_cycle_committed(doc: &Path, preflight: &str, committed: &str) {
        agent_doc_cycle_state_io::start_preflight(doc, Some(preflight), Some(preflight)).unwrap();
        fs::write(doc, committed).unwrap();
        agent_doc_snapshot_io::save(doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &NoopPipelineFrontmatterEffects,
            doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
    }

    fn run_context(doc: &Path, content: &str) -> RunContext {
        let rc = RunContext::new(doc.to_path_buf());
        rc.set_doc_content(content.to_string());
        rc
    }

    fn ops_log(root: &Path) -> String {
        fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default()
    }

    #[test]
    fn queue_head_removal_guard_logs_proof_source_for_authorized_id_removals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preflight = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done] done this cycle\n",
            "- [ ] [#keep] still queued\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#done]\n",
            "- do [#resolved]\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n",
        );
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: do #done\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done] done this cycle\n",
            "- [ ] [#keep] still queued\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), preflight);
        mark_cycle_committed(&doc, preflight, committed);
        agent_doc_cycle_state_io::record_pending_done_ids(&doc, &["done".to_string()]).unwrap();

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_queue_head_removal_guard(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "authorized queue-head removals should not interrupt"
        );
        let log = ops_log(tmp.path());
        assert!(
            log.contains("removed=#done proof_source=cycle_lifecycle_outcome"),
            "done removal proof should name the id and source:\n{log}"
        );
        assert!(
            log.contains("removed=#resolved proof_source=backlog_resolved_or_removed"),
            "resolved backlog removal proof should name the id and source:\n{log}"
        );
    }

    #[test]
    fn free_text_queue_head_guard_logs_response_proof_source_for_removed_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let preflight = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- Please explain the churn\n",
            "<!-- /agent:queue -->\n",
        );
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: Please explain the churn\n\n",
            "The churn comes from stale queue convergence.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), preflight);
        mark_cycle_committed(&doc, preflight, committed);

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_free_text_queue_head_provenance(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "answered free-text queue head should not interrupt"
        );
        let log = ops_log(tmp.path());
        assert!(
            log.contains("free_text_queue_head_provenance_proof")
                && log.contains("Please explain the churn")
                && log.contains("proof_source=committed_response"),
            "free-text removal proof should name the head and source:\n{log}"
        );
    }

    #[test]
    fn completed_free_text_queue_residue_guard_fires_for_answered_active_head() {
        // A genuine ONE-TIME prompt head that the response answered, but which is
        // still active in the committed queue, is completed residue and must fire
        // the `#qheadresidue` guard. (A recurring-imperative command head is the
        // exception — see the `#qimpstrike` test below.)
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\npending_done_guard: warn\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: explain the queue churn -- gpt-5\n\n",
            "> **Queue prompt:**\n>\n> explain the queue churn\n\n",
            "The churn comes from stale convergence.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- explain the queue churn\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), committed);
        mark_cycle_committed(&doc, committed, committed);

        let rc = run_context(&doc, committed);
        match check_free_text_queue_head_provenance(&doc, &rc).unwrap() {
            GuardResult::Error(message) => {
                assert!(message.contains("completed free-text"), "got: {message}");
                assert!(message.contains("#qheadresidue"), "got: {message}");
                assert!(
                    message.contains("explain the queue churn"),
                    "got: {message}"
                );
            }
            other => panic!("completed queue residue must interrupt, got {other:?}"),
        }
        let log = ops_log(tmp.path());
        assert!(
            log.contains("free_text_queue_completed_residue_guard_fired"),
            "residue guard should log the proved completed head:\n{log}"
        );
    }

    #[test]
    fn residue_guard_exempts_recurring_imperative_deploy_head() {
        // #qimpstrike: a recurring-imperative command head (`deploy`) is an
        // executable directive that stays valid every cycle. A response that
        // echoed it as a `> **Queue prompt:**` quote does NOT retire a standing
        // `deploy` directive, so the `#qheadresidue` residue guard must NOT fire
        // — the head remains active/drainable for the next dispatch.
        let tmp = tempfile::TempDir::new().unwrap();
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\npending_done_guard: warn\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: deploy -- gpt-5\n\n",
            "> **Queue prompt:**\n>\n> deploy\n\n",
            "Deployed successfully.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- deploy\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = make_doc(tmp.path(), committed);
        mark_cycle_committed(&doc, committed, committed);

        let rc = run_context(&doc, committed);
        assert!(
            matches!(
                check_free_text_queue_head_provenance(&doc, &rc).unwrap(),
                GuardResult::None
            ),
            "recurring-imperative `deploy` head must not be struck as completed residue"
        );
        let log = ops_log(tmp.path());
        assert!(
            !log.contains("free_text_queue_completed_residue_guard_fired"),
            "residue guard must not fire for a recurring-imperative head:\n{log}"
        );
    }
}
