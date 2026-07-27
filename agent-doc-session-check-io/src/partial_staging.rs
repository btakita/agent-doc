use std::path::Path;

use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

pub fn check_partial_closeout_state_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };

    let content = crate::resolve_current_document_content(file, "partial_closeout_state_guard")?;
    let open_backlog_ids = agent_doc_document::tracked_work_projection::open_backlog_ids(&content);
    let candidates = match agent_doc_turn::closeout_signal::partial_closeout_state_decision(
        agent_doc_turn::closeout_signal::PartialCloseoutStateEvidence {
            cycle_open: state.is_open(),
            capture_committed: capture.capture_committed,
            response_body: &capture.response_body,
            directed_ids: &state.expect_done_or_gate_ids,
            pending_done_ids: &state.pending_done_ids,
            reaped_pending_ids: &state.reaped_pending_ids,
            open_backlog_ids: &open_backlog_ids,
        },
    ) {
        agent_doc_turn::closeout_signal::PartialCloseoutStateDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::PartialCloseoutStateDecision::Warn { candidate_ids } => {
            candidate_ids
        }
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "partial_closeout_state_guard_fired file={} candidates={}",
            file.display(),
            candidates.join(",")
        ),
    );

    Ok(agent_doc_workflow::session_check::partial_closeout_state_guard_result(&candidates))
}

/// `#partial-staging-closeout-guard`: a manual repo commit can accidentally
/// stage only the source half of a source+test change. Local verification then
/// passes against the dirty worktree while CI sees only the partial commit.
/// This guard is WARN-only and narrow: it requires a latest-commit source/test
/// path relationship plus overlapping changed string literals in tracked
/// uncommitted or staged companion changes.
pub fn check_partial_staging_closeout_guard(file: &Path) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if !agent_doc_git_io::partial_staging::cycle_committed_beyond_session_document(
        file,
        state.started_at,
    )? {
        return Ok(GuardResult::None);
    }

    let findings = agent_doc_git_io::partial_staging::companion_findings(file)?;
    if findings.is_empty() {
        return Ok(GuardResult::None);
    }

    for finding in findings.iter().take(3) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "partial_staging_closeout_guard_fired file={} repo={} committed_paths={} dirty_paths={} literals={}",
                file.display(),
                finding.repo.display(),
                finding.committed_paths.join(","),
                finding.dirty_paths.join(","),
                finding.literals.join("|")
            ),
        );
    }

    let workflow_findings = findings
        .iter()
        .map(
            |finding| agent_doc_workflow::session_check::PartialStagingCloseoutGuardFinding {
                repo: finding.repo.display().to_string(),
                committed_paths: finding.committed_paths.clone(),
                dirty_paths: finding.dirty_paths.clone(),
                literals: finding.literals.clone(),
            },
        )
        .collect::<Vec<_>>();

    Ok(
        agent_doc_workflow::session_check::partial_staging_closeout_guard_result(
            &workflow_findings,
        ),
    )
}
