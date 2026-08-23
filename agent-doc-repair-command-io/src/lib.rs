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
    Committed {
        repair_outcome: String,
    },
    Superseded,
    /// The effect completed successfully enough to observe that a newer
    /// document-state edge is required. Do not put this on a timer.
    WaitingForSignal {
        reason: String,
    },
    /// The effect itself failed transiently; controller reconnect/backoff may
    /// retry this exact capture without inventing a new response operation.
    RetryableEffect {
        reason: String,
    },
    NeedsOperator {
        reason: String,
    },
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
    if let Some(capture) =
        agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
        && let Some(key) = captured_finalize_resume_key_from_capture(&capture)
    {
        return Ok(Some(key));
    }
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
    if capture.cycle_id != state.cycle_id || capture.response_sha256 != response_sha256 {
        return Ok(None);
    }
    Ok(captured_finalize_resume_key_from_capture(&capture))
}

fn captured_finalize_resume_key_from_capture(
    capture: &agent_doc_cycle_state_io::ProjectedCapturedResponse,
) -> Option<CapturedFinalizeResumeKey> {
    (!capture.response_body.trim().is_empty()).then(|| CapturedFinalizeResumeKey {
        cycle_id: capture.cycle_id.clone(),
        capture_id: capture.capture_id.clone(),
        response_sha256: capture.response_sha256.clone(),
    })
}

/// Resume one exact captured finalize operation through the existing strict
/// repair/write/commit path. Disk fallback is explicitly disabled: a live or
/// reconnecting editor remains authoritative, and convergence failures retain
/// the same durable capture until a later controller state edge.
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
                CapturedFinalizeResumeOutcome::WaitingForSignal {
                    reason: format!(
                        "strict repair returned {outcome:?} but the captured cycle is still open"
                    ),
                }
            }
        }
        Err(err) => classify_captured_finalize_resume_error(&format!("{err:#}")),
    }
}

/// Run the operator-facing status-only session check. Durable capture replay is
/// owned by finalize/write/preflight and the route supervisor; observing status
/// must never manufacture a second document mutation.
pub fn run_session_check(file: &Path, codex_final_gate: bool) -> Result<()> {
    agent_doc_session_check_io::run_read_only_with_options(
        file,
        codex_final_gate,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
}

fn resume_captured_finalize_intent(
    file: &Path,
    expected: &CapturedFinalizeResumeKey,
) -> Result<RepairOutcome> {
    let retained = agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
        .filter(|capture| {
            capture.cycle_id == expected.cycle_id
                && capture.capture_id == expected.capture_id
                && capture.response_sha256 == expected.response_sha256
        });
    let capture = match retained {
        Some(capture) => Some(capture),
        None => {
            agent_doc_cycle_state_io::load_projected_captured_response(file, &expected.capture_id)?
                .filter(|capture| {
                    capture.cycle_id == expected.cycle_id
                        && capture.response_sha256 == expected.response_sha256
                })
        }
    };
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
        let mut response = capture.intent_body.unwrap_or(capture.response_body);
        if let Some(normalized) =
            agent_doc_template::response_materialization::normalize_retained_legacy_patch_markers(
                &response,
            )
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "captured_finalize_legacy_patch_markers_normalized file={} cycle_id={} capture_id={} strategy=transient_replay_only",
                    file.display(),
                    expected.cycle_id,
                    expected.capture_id,
                ),
            );
            response = normalized;
        }
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
    // Every needle here matches error *prose* except one. `#retainconv`:
    // `await_editor_replica` was written for the name of the typed error the
    // whole retained-write class is built from — but `Display` prints only the
    // message, so nothing ever carried it and the canonical "wait for a state
    // edge" failure took the default `NeedsOperator` arm instead. The emitting
    // constructor now stamps `AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN`
    // (`agent-doc-document-realtime-io`) into every such refusal, which is what
    // makes this needle true by construction rather than by coincidence of
    // wording — including across the process boundaries (ops-log `reason_head`,
    // retained-capture reason, Codex stop hook) where a typed downcast cannot
    // reach.
    let waiting_for_signal = [
        "retry_without_disk_write",
        "editor_convergence_required",
        "compare_and_swap_raced",
        "await_editor_replica",
        "no relay replica is registered",
        "editor authority stayed",
        "active recovery attempt",
        "operation is already in progress",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if waiting_for_signal {
        return CapturedFinalizeResumeOutcome::WaitingForSignal {
            reason: reason.to_string(),
        };
    }
    let retryable_effect = [
        "controller_model_backpressure",
        "timed out",
        "timeout",
        "resource temporarily unavailable",
        "connection reset",
        "broken pipe",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if retryable_effect {
        return CapturedFinalizeResumeOutcome::RetryableEffect {
            reason: reason.to_string(),
        };
    }
    CapturedFinalizeResumeOutcome::NeedsOperator {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod captured_finalize_resume_tests {
    use super::*;

    #[test]
    fn convergence_failures_wait_for_a_state_edge() {
        for reason in [
            "recovery=retry_without_disk_write",
            "closeout blocked by editor_convergence_required",
            "compare_and_swap_raced",
        ] {
            assert!(matches!(
                classify_captured_finalize_resume_error(reason),
                CapturedFinalizeResumeOutcome::WaitingForSignal { .. }
            ));
        }
    }

    #[test]
    fn transport_and_backpressure_failures_retry_the_effect() {
        for reason in [
            "controller_model_backpressure",
            "controller lookup timed out",
            "connection reset by peer",
        ] {
            assert!(matches!(
                classify_captured_finalize_resume_error(reason),
                CapturedFinalizeResumeOutcome::RetryableEffect { .. }
            ));
        }
    }

    /// `#retainconv` — the 2026-08-23 `agent-doc-bugs.md` latch.
    ///
    /// A retained-delivery refusal is the canonical "wait for a state edge"
    /// failure, yet none of the prose needles matched it, so it took the default
    /// arm and demanded an operator for a write that had already landed on disk.
    /// Bind the assertion to the emitting crate's exported token, not to a copy
    /// of the message, so the two sides cannot drift apart silently.
    #[test]
    fn a_retained_delivery_refusal_waits_for_a_state_edge() {
        let reason = format!(
            "queue consume: failed to write document: visible document write for plan.md is \
             retained by the lazy delivery projection because the editor state projection has not \
             converged; no secondary snapshot/commit or forced disk write was attempted. [{}]",
            agent_doc_document_realtime_io::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
        );
        assert!(
            matches!(
                classify_captured_finalize_resume_error(&reason),
                CapturedFinalizeResumeOutcome::WaitingForSignal { .. }
            ),
            "a deferred write must not demand an operator: {reason}"
        );
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

    #[test]
    fn retained_capture_produces_a_resume_key_without_current_cycle_state() {
        let capture = agent_doc_cycle_state_io::ProjectedCapturedResponse {
            cycle_id: "retained-cycle".to_string(),
            capture_id: "retained-capture".to_string(),
            response_sha256: "retained-response-sha".to_string(),
            response_body: "response body".to_string(),
            intent_body: None,
            mutation_plan_json: None,
            file_hash: None,
            snapshot_hash: None,
            baseline_content: None,
        };

        assert_eq!(
            captured_finalize_resume_key_from_capture(&capture),
            Some(CapturedFinalizeResumeKey {
                cycle_id: "retained-cycle".to_string(),
                capture_id: "retained-capture".to_string(),
                response_sha256: "retained-response-sha".to_string(),
            }),
        );
    }
}
