use agent_doc_turn::repair::RepairOutcome;
use anyhow::Result;
use std::path::Path;

/// Durable identity of a binary-owned finalize operation. The response capture
/// and cycle projection already persist these fields, so the supervisor can
/// resume after the originating agent process has returned or crashed without
/// recapturing (and therefore without duplicating) the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFinalizeResumeKey {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedFinalizeResumeOutcome {
    Committed { repair_outcome: String },
    Superseded,
    Retryable { reason: String },
    NeedsOperator { reason: String },
}

pub fn run_write_command_with_empty_response_recovery(
    options: agent_doc_write_command_io::CommandOptions,
    commit_mode: agent_doc_write_command_io::CommitMode,
) -> Result<()> {
    agent_doc_write_runtime_io::run_command_with_empty_response_recovery(
        options,
        commit_mode,
        recover_empty_response_for_strict_closeout,
    )
}

pub fn recover_empty_response_for_strict_closeout(
    file: &Path,
    strict_closeout: bool,
    has_pending_mutation: bool,
    force_disk: bool,
) -> Result<bool> {
    agent_doc_repair_io::recover_empty_response_for_strict_closeout(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        strict_closeout,
        has_pending_mutation,
        Some(force_disk),
    )
}

#[cfg(test)]
pub fn run(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::run(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::repair(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

/// Return the stable operation key only when the durable cycle contains a
/// non-empty captured response that is eligible for binary-owned closeout.
pub fn captured_finalize_resume_key(file: &Path) -> Result<Option<CapturedFinalizeResumeKey>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !matches!(
        state.phase,
        agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
    ) {
        return Ok(None);
    }
    let (Some(capture_id), Some(response_sha256)) = (
        state.capture_id.as_deref(),
        state.response_sha256.as_deref(),
    ) else {
        return Ok(None);
    };
    let Some(capture) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    else {
        return Ok(None);
    };
    if capture.cycle_id != state.cycle_id
        || capture.response_sha256 != response_sha256
        || capture.response_body.trim().is_empty()
    {
        return Ok(None);
    }
    Ok(Some(CapturedFinalizeResumeKey {
        cycle_id: state.cycle_id,
        capture_id: capture_id.to_string(),
        response_sha256: response_sha256.to_string(),
    }))
}

/// Resume one exact captured finalize operation through the existing strict
/// repair/write/commit path. Disk fallback is explicitly disabled: a live or
/// reconnecting editor remains authoritative, and convergence failures retain
/// the same durable capture for a later retry.
pub fn resume_captured_finalize(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> CapturedFinalizeResumeOutcome {
    let current = match captured_finalize_resume_key(file) {
        Ok(current) => current,
        Err(err) => {
            return classify_captured_finalize_resume_error(&format!("{err:#}"));
        }
    };
    if current.as_ref() != Some(expected) {
        return CapturedFinalizeResumeOutcome::Superseded;
    }

    let result = resume_captured_finalize_intent(file, expected);
    match result {
        Ok(outcome) => {
            let committed = agent_doc_cycle_state_io::load_with_closeout_projection(file)
                .ok()
                .flatten()
                .is_some_and(|state| {
                    state.cycle_id == expected.cycle_id
                        && matches!(state.phase, agent_doc_turn::CyclePhase::Committed)
                });
            if committed {
                CapturedFinalizeResumeOutcome::Committed {
                    repair_outcome: format!("{outcome:?}"),
                }
            } else if captured_finalize_resume_key(file).ok().flatten().as_ref() != Some(expected) {
                CapturedFinalizeResumeOutcome::Superseded
            } else {
                CapturedFinalizeResumeOutcome::Retryable {
                    reason: format!(
                        "strict repair returned {outcome:?} but the captured cycle is still open"
                    ),
                }
            }
        }
        Err(err) => classify_captured_finalize_resume_error(&format!("{err:#}")),
    }
}

fn resume_captured_finalize_intent(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> Result<RepairOutcome> {
    let capture =
        agent_doc_cycle_state_io::load_projected_captured_response(file, &expected.capture_id)?
            .filter(|capture| {
                capture.cycle_id == expected.cycle_id
                    && capture.response_sha256 == expected.response_sha256
            });
    if let Some(capture) = capture
        && let Some(plan_json) = capture.mutation_plan_json.as_deref()
    {
        let plan: agent_doc_write_command_io::CapturedCloseoutMutationPlan =
            serde_json::from_str(plan_json)
                .map_err(|err| anyhow::anyhow!("decode captured closeout mutation plan: {err}"))?;
        let options =
            agent_doc_write_command_io::CommandOptions::recovery_from_captured_closeout_mutation_plan(
                file, plan,
            );
        let response = capture.intent_body.unwrap_or(capture.response_body);
        agent_doc_write_runtime_io::run_command_with_response(
            options,
            agent_doc_write_command_io::CommitMode::Required,
            response,
        )?;
        return Ok(RepairOutcome::ReplayedResponse);
    }

    // Backward-compatible recovery for captures written before the typed
    // mutation plan became part of the Lazily intent.
    agent_doc_repair_io::repair_preserving_live_authority(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

fn classify_captured_finalize_resume_error(reason: &str) -> CapturedFinalizeResumeOutcome {
    let lower = reason.to_ascii_lowercase();
    let retryable = [
        "retry_without_disk_write",
        "editor_convergence_required",
        "controller_model_backpressure",
        "compare_and_swap_raced",
        "timed out",
        "timeout",
        "await_editor_replica",
        "no relay replica is registered",
        "editor authority stayed",
        "active recovery attempt",
        "operation is already in progress",
        "resource temporarily unavailable",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if retryable {
        CapturedFinalizeResumeOutcome::Retryable {
            reason: reason.to_string(),
        }
    } else {
        CapturedFinalizeResumeOutcome::NeedsOperator {
            reason: reason.to_string(),
        }
    }
}

#[cfg(test)]
mod captured_finalize_resume_tests {
    use super::*;

    #[test]
    fn convergence_and_backpressure_failures_are_retryable() {
        for reason in [
            "recovery=retry_without_disk_write",
            "closeout blocked by editor_convergence_required",
            "controller_model_backpressure",
            "compare_and_swap_raced",
            "controller lookup timed out",
        ] {
            assert!(matches!(
                classify_captured_finalize_resume_error(reason),
                CapturedFinalizeResumeOutcome::Retryable { .. }
            ));
        }
    }

    #[test]
    fn ambiguous_content_failure_requires_operator_instead_of_retrying() {
        assert!(matches!(
            classify_captured_finalize_resume_error(
                "visible user-authored content diverged from the captured baseline"
            ),
            CapturedFinalizeResumeOutcome::NeedsOperator { .. }
        ));
    }
}
