use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    DispatchOnlyProofOutcomeFacts, DispatchOnlyRecycleInflightMessageFacts,
    DispatchOnlyRecycleSupersededOwnerMessageFacts, DispatchOnlyReopenDelivery,
    DispatchStartProofDecision, DispatchStartProofFacts, RECYCLE_INFLIGHT_MAX_EPOCH_CHANGES,
    RECYCLE_SUPERSEDED_OWNER_UNBLOCKER, RecycleInflightUnsettledVerdict, RoutedDispatchStartProof,
    RoutedReopenGuardReason, SupersededOwnerReplacementStep,
    accepted_only_dispatch_start_log_message, accepted_only_dispatch_start_refusal_message,
    dispatch_only_dispatch_start_proof_required as controller_dispatch_only_dispatch_start_proof_required,
    dispatch_only_recycle_inflight_message, dispatch_only_recycle_superseded_owner_message,
    dispatch_only_sent_console_message, dispatch_only_sent_log_message,
    dispatch_proof_failed_event, recycle_inflight_unsettled_verdict_with_evidence,
    recycle_inflight_unsettled_verdict_with_owner,
    recycle_inflight_unsettled_verdict_with_readiness, routed_dispatch_start_timeout_for_binary,
    superseded_owner_replacement_step,
};
use agent_doc_harness::HarnessConfig;

#[derive(Debug, Clone, Copy)]
pub struct DispatchOnlyBugReportFacts {
    pub elapsed: Duration,
    pub proof: RoutedDispatchStartProof,
}

pub fn wait_for_dispatch_only_recycle_inflight_settle(
    file: &Path,
    file_path: &str,
    pane: &str,
    harness_binary: &str,
) -> Result<()> {
    let status = agent_doc_controller_io::project_controller::supervisor_recycle_status_for_file(
        Path::new(file_path),
    )?;
    if !matches!(
        status.phase,
        agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
    ) {
        return Ok(());
    }

    let started = std::time::Instant::now();
    let mut reason = status.reason.unwrap_or_else(|| "unknown".to_string());
    let mut marked_secs = status.marked_secs;
    let mut recycle_epoch = status.recycle_epoch;
    let mut attempt: u32 = 0;
    let mut epoch_changes: u32 = 0;
    // `#netadv3` RSD-1: consecutive round trips where both the settle wait and
    // the status re-read failed.
    let mut unreachable: u32 = 0;
    // `#runfrontenderror`: when (in this gate's own elapsed time) the turn-safe
    // replacement of a superseded owner was requested.
    let mut superseded_replacement_requested_at_ms: Option<u128> = None;

    // `#recycleinflightwedge` / `#recyclesettlewaitshort`: a recycle older than
    // the settle TTL lost its settle transition — the supervisor died between
    // publishing `InFlight` and its replacement reaching the watch loop. Waiting
    // for it is waiting for an event that will never arrive, and refusing
    // afterwards hands the operator an unblocker they cannot perform. Proceed
    // instead, loudly: the hot-reload boundary this gate protects is long over.
    //
    // Anything younger than the TTL is pending, not lost, so the single blocking
    // `supervisor_recycle_wait_settled` RPC is re-armed rather than read as a
    // verdict. The controller owns that trigger and blocks inside it; this is a
    // re-arm of an external wait, not a poll, so it owns no wait/poll constant of
    // its own (the TTL policy stays in `agent-doc-controller`).
    loop {
        let ttl_secs = recycle_inflight_settle_ttl_secs();
        // `#netadv5` R9: the TTL alone is a timer; abandonment needs the
        // supervisor to be gone.
        let supervisor_pid =
            agent_doc_supervisor_io::process::supervisor_pid_for_doc(Path::new(file_path));
        let supervisor_alive = supervisor_pid.is_some();
        // `#dispatchreadyselfheal`: a supervisor `ready` registration stamped
        // after this recycle started is the settle itself, observed directly.
        let ready_registered_secs =
            agent_doc_controller_io::project_controller::supervisor_ready_registered_secs_for_file(
                Path::new(file_path),
            );
        let mut verdict = recycle_inflight_unsettled_verdict_with_readiness(
            marked_secs,
            now_secs(),
            ttl_secs,
            supervisor_alive,
            ready_registered_secs,
        );
        if verdict == RecycleInflightUnsettledVerdict::ProceedReadyAfterStart {
            match ready_after_start_self_heal(
                file,
                file_path,
                pane,
                harness_binary,
                marked_secs,
                recycle_epoch,
                ready_registered_secs,
                started.elapsed().as_millis(),
                attempt,
            ) {
                ReadyAfterStartHeal::Proceed => return Ok(()),
                ReadyAfterStartHeal::Rekey(projection) => {
                    // A newer recycle replaced the one the evidence covered:
                    // re-key and re-classify against it, exactly as a settle
                    // RPC reporting a new epoch does below.
                    epoch_changes += 1;
                    marked_secs = projection.marked_secs;
                    recycle_epoch = projection.recycle_epoch;
                    if let Some(next) = projection.reason {
                        reason = next;
                    }
                    if epoch_changes > RECYCLE_INFLIGHT_MAX_EPOCH_CHANGES {
                        return Err(recycle_inflight_refusal(
                            file,
                            pane,
                            harness_binary,
                            &reason,
                            marked_secs,
                            recycle_epoch,
                            started.elapsed().as_millis(),
                            attempt,
                            "recycle_epoch_churn",
                        ));
                    }
                    continue;
                }
                ReadyAfterStartHeal::Declined => {
                    // The controller disagreed with the evidence: fall back to
                    // the verdict without it rather than trusting our read.
                    verdict = recycle_inflight_unsettled_verdict_with_owner(
                        marked_secs,
                        now_secs(),
                        ttl_secs,
                        supervisor_alive,
                    );
                }
            }
        }
        // `#runfrontenderror`: an R9 refusal against an owner that maps a
        // superseded binary cannot end by waiting; only the owner's
        // replacement ends it. The freshness probe is read only on this path.
        let mut superseded_owner_pid = None;
        if verdict == RecycleInflightUnsettledVerdict::RefuseOwnerStillRecycling
            && let Some(pid) = supervisor_pid
        {
            let owner_binary_superseded =
                agent_doc_controller_io::project_controller::host_supervisor_pid_binary_is_stale(
                    pid,
                );
            verdict = recycle_inflight_unsettled_verdict_with_evidence(
                marked_secs,
                now_secs(),
                ttl_secs,
                supervisor_alive,
                None,
                owner_binary_superseded,
            );
            superseded_owner_pid = Some(pid);
        }
        if verdict == RecycleInflightUnsettledVerdict::ReplaceSupersededOwner {
            let pid = superseded_owner_pid.unwrap_or_default();
            let wait_secs = recycle_superseded_owner_wait_secs();
            match superseded_owner_replacement_step(
                superseded_replacement_requested_at_ms,
                started.elapsed().as_millis(),
                wait_secs,
            ) {
                SupersededOwnerReplacementStep::RequestReplacement => {
                    // Turn-safe and idempotent: the owner recycles at its
                    // next safe boundary; a live turn is never interrupted.
                    let request_status =
                        agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
                            Path::new(file_path),
                            "dispatch_only_recycle_gate",
                        );
                    superseded_replacement_requested_at_ms = Some(started.elapsed().as_millis());
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_recycle_inflight_superseded_owner file={} pane={} harness={} recycle_cause={} marked_secs={} recycle_epoch={} supervisor_pid={} wait_secs={} action=replacement_requested request_status={:?} (#runfrontenderror)",
                            file.display(),
                            pane,
                            harness_binary,
                            reason,
                            marked_secs,
                            recycle_epoch,
                            pid,
                            wait_secs,
                            request_status
                                .as_deref()
                                .map(|status| status.replace('\n', " "))
                                .unwrap_or_else(|| "not_scheduled".to_string()),
                        ),
                    );
                }
                SupersededOwnerReplacementStep::KeepWaiting => {}
                SupersededOwnerReplacementStep::RefuseRestartStaleSupervisor => {
                    return Err(recycle_superseded_owner_refusal(
                        file,
                        pane,
                        harness_binary,
                        &reason,
                        marked_secs,
                        recycle_epoch,
                        pid,
                        wait_secs,
                        started.elapsed().as_millis(),
                        attempt,
                    ));
                }
            }
        }
        match verdict {
            RecycleInflightUnsettledVerdict::RefuseOwnerStillRecycling => {
                return Err(recycle_inflight_refusal(
                    file,
                    pane,
                    harness_binary,
                    &reason,
                    marked_secs,
                    recycle_epoch,
                    started.elapsed().as_millis(),
                    attempt,
                    "recycle_ttl_elapsed_supervisor_alive (retry later; if it never settles, `agent-doc admin recycle` or restart the supervisor)",
                ));
            }
            RecycleInflightUnsettledVerdict::ProceedAbandoned => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_recycle_inflight_abandoned file={} pane={} harness={} reason={} marked_secs={} ttl_secs={} recycle_epoch={} waited_ms={} attempts={}",
                        file.display(),
                        pane,
                        harness_binary,
                        reason,
                        marked_secs,
                        ttl_secs,
                        recycle_epoch,
                        started.elapsed().as_millis(),
                        attempt
                    ),
                );
                return Ok(());
            }
            RecycleInflightUnsettledVerdict::FailClosed => {
                return Err(recycle_inflight_refusal(
                    file,
                    pane,
                    harness_binary,
                    &reason,
                    marked_secs,
                    recycle_epoch,
                    started.elapsed().as_millis(),
                    attempt,
                    "unstamped_recycle_mark",
                ));
            }
            // `ProceedReadyAfterStart` is resolved above (it proceeds, re-keys,
            // or is re-classified without the evidence), so it cannot reach
            // here; re-arming the wait is the safe reading if it ever did.
            //
            // `ReplaceSupersededOwner` was resolved just above (requested, or
            // still inside its bounded wait): re-arm, never inject.
            RecycleInflightUnsettledVerdict::KeepWaiting
            | RecycleInflightUnsettledVerdict::ProceedReadyAfterStart
            | RecycleInflightUnsettledVerdict::ReplaceSupersededOwner => {}
        }

        attempt += 1;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_recycle_inflight_wait file={} pane={} harness={} reason={} marked_secs={} ttl_secs={} recycle_epoch={} attempt={}",
                file.display(),
                pane,
                harness_binary,
                reason,
                marked_secs,
                ttl_secs,
                recycle_epoch,
                attempt
            ),
        );

        let waited =
            agent_doc_controller_io::project_controller::wait_for_supervisor_recycle_settle_for_file(
                Path::new(file_path),
            );
        match waited {
            Ok(projection) => {
                unreachable = 0;
                if !matches!(
                    projection.phase,
                    agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
                ) {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_recycle_inflight_settled file={} pane={} harness={} reason={} waited_ms={} attempts={}",
                            file.display(),
                            pane,
                            harness_binary,
                            reason,
                            started.elapsed().as_millis(),
                            attempt
                        ),
                    );
                    return Ok(());
                }
                // Settled RPC that still reports `InFlight` is a *new* recycle
                // generation, not this one finishing. Re-key on it so the TTL is
                // measured against the mark that is actually gating dispatch.
                if projection.recycle_epoch != recycle_epoch {
                    epoch_changes += 1;
                }
                marked_secs = projection.marked_secs;
                recycle_epoch = projection.recycle_epoch;
                if let Some(next) = projection.reason {
                    reason = next;
                }
                if epoch_changes > RECYCLE_INFLIGHT_MAX_EPOCH_CHANGES {
                    return Err(recycle_inflight_refusal(
                        file,
                        pane,
                        harness_binary,
                        &reason,
                        marked_secs,
                        recycle_epoch,
                        started.elapsed().as_millis(),
                        attempt,
                        "recycle_epoch_churn",
                    ));
                }
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_recycle_inflight_unsettled file={} pane={} harness={} reason={} waited_ms={} attempt={} error={:?}",
                        file.display(),
                        pane,
                        harness_binary,
                        reason,
                        started.elapsed().as_millis(),
                        attempt,
                        err.to_string()
                    ),
                );
                // Re-read the mark before deciding: the loop head re-classifies
                // against the TTL, and a recycle that settled between the RPC
                // giving up and this read must not be refused.
                match agent_doc_controller_io::project_controller::supervisor_recycle_status_for_file(
                    Path::new(file_path),
                ) {
                    Ok(refreshed) => {
                        unreachable = 0;
                        if !matches!(
                            refreshed.phase,
                            agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
                        ) {
                            agent_doc_ops_log_io::log_op(
                                file,
                                &format!(
                                    "route_dispatch_only_recycle_inflight_settled file={} pane={} harness={} reason={} waited_ms={} attempts={}",
                                    file.display(),
                                    pane,
                                    harness_binary,
                                    reason,
                                    started.elapsed().as_millis(),
                                    attempt
                                ),
                            );
                            return Ok(());
                        }
                        if refreshed.recycle_epoch != recycle_epoch {
                            epoch_changes += 1;
                        }
                        marked_secs = refreshed.marked_secs;
                        recycle_epoch = refreshed.recycle_epoch;
                        if let Some(next) = refreshed.reason {
                            reason = next;
                        }
                        if epoch_changes > RECYCLE_INFLIGHT_MAX_EPOCH_CHANGES {
                            return Err(recycle_inflight_refusal(
                                file,
                                pane,
                                harness_binary,
                                &reason,
                                marked_secs,
                                recycle_epoch,
                                started.elapsed().as_millis(),
                                attempt,
                                "recycle_epoch_churn",
                            ));
                        }
                    }
                    Err(status_err) => {
                        // `#netadv3` RSD-1: both round trips were lost. That
                        // is not a verdict about the recycle (a stamped,
                        // pending recycle must never refuse), so back off and
                        // re-arm. The loop head's verdict on the last mark it
                        // read still bounds the wait.
                        unreachable = unreachable.saturating_add(1);
                        let backoff =
                            agent_doc_controller::dispatch::recycle_settle_unreachable_backoff(
                                unreachable,
                            );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_recycle_inflight_status_unreachable file={} pane={} harness={} reason={} recycle_epoch={} attempt={} consecutive={} backoff_ms={} error={:?}",
                                file.display(),
                                pane,
                                harness_binary,
                                reason,
                                recycle_epoch,
                                attempt,
                                unreachable,
                                backoff.as_millis(),
                                status_err.to_string()
                            ),
                        );
                        std::thread::sleep(backoff);
                        continue;
                    }
                }
            }
        }
    }
}

/// What the gate does after asking the controller to settle a recycle on the
/// strength of a ready registration after its start (`#dispatchreadyselfheal`).
enum ReadyAfterStartHeal {
    /// Settled (or the RPC was unavailable but the evidence stands): inject.
    Proceed,
    /// The controller reports a newer `InFlight` recycle than the one covered.
    Rekey(agent_doc_state_backbone::SupervisorRecycleProjection),
    /// The controller re-verified and declined; classify without the evidence.
    Declined,
}

/// `#dispatchreadyselfheal`: the supervisor registered `ready` after this
/// recycle started, so the hot-reload boundary is over. Re-mint the settle at
/// the in-flight epoch so the durable projection stops refusing every later
/// dispatch too, then proceed.
///
/// An unreachable controller (or one that predates the RPC) does not refuse:
/// the registration evidence alone proves the boundary passed, and the next
/// dispatch retries the re-mint.
#[allow(clippy::too_many_arguments)]
fn ready_after_start_self_heal(
    file: &Path,
    file_path: &str,
    pane: &str,
    harness_binary: &str,
    marked_secs: u64,
    recycle_epoch: u64,
    ready_registered_secs: Option<u64>,
    waited_ms: u128,
    attempts: u32,
) -> ReadyAfterStartHeal {
    let healed =
        agent_doc_controller_io::project_controller::supervisor_recycle_settle_ready_after_start_for_file(
            Path::new(file_path),
            recycle_epoch,
        );
    let (outcome, heal) = match healed {
        Ok(projection)
            if matches!(
                projection.phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            ) && projection.recycle_epoch != recycle_epoch =>
        {
            ("rekey".to_string(), ReadyAfterStartHeal::Rekey(projection))
        }
        Ok(projection)
            if matches!(
                projection.phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            ) =>
        {
            ("declined".to_string(), ReadyAfterStartHeal::Declined)
        }
        Ok(projection) => (
            format!(
                "settled phase={:?} settled_epoch={}",
                projection.phase, projection.recycle_epoch
            ),
            ReadyAfterStartHeal::Proceed,
        ),
        Err(err) => (
            format!("remint_unavailable error={:?}", format!("{err:#}")),
            ReadyAfterStartHeal::Proceed,
        ),
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_recycle_inflight_ready_after_start file={} pane={} harness={} marked_secs={} ready_registered_secs={:?} recycle_epoch={} waited_ms={} attempts={} outcome={} (#dispatchreadyselfheal)",
            file.display(),
            pane,
            harness_binary,
            marked_secs,
            ready_registered_secs,
            recycle_epoch,
            waited_ms,
            attempts,
            outcome
        ),
    );
    heal
}

/// The one refusal shape for the recycle gate, so both fail-closed paths carry
/// the same operator-facing message and the same forensic fields.
#[allow(clippy::too_many_arguments)]
fn recycle_inflight_refusal(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    reason: &str,
    marked_secs: u64,
    recycle_epoch: u64,
    waited_ms: u128,
    attempts: u32,
    refusal: &str,
) -> anyhow::Error {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_recycle_inflight_refused file={} pane={} harness={} reason={} marked_secs={} recycle_epoch={} waited_ms={} attempts={} refusal={}",
            file.display(),
            pane,
            harness_binary,
            reason,
            marked_secs,
            recycle_epoch,
            waited_ms,
            attempts,
            refusal
        ),
    );
    let file_display = file.display().to_string();
    let outcome_fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
        "wait_for_supervisor_recycle_settle",
    );
    anyhow::anyhow!(dispatch_only_recycle_inflight_message(
        DispatchOnlyRecycleInflightMessageFacts {
            harness_binary,
            pane,
            file_display: &file_display,
            reason,
            outcome_fields: &outcome_fields,
        },
    ))
}

/// `#runfrontenderror`: the refusal once a superseded owner outlived its
/// bounded replacement wait. Distinct refusal and unblocker from the R9 shape:
/// the operator is told which process to restart, not to wait for a settle it
/// cannot produce.
#[allow(clippy::too_many_arguments)]
fn recycle_superseded_owner_refusal(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    reason: &str,
    marked_secs: u64,
    recycle_epoch: u64,
    supervisor_pid: u32,
    wait_secs: u64,
    waited_ms: u128,
    attempts: u32,
) -> anyhow::Error {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_recycle_inflight_refused file={} pane={} harness={} recycle_cause={} marked_secs={} recycle_epoch={} waited_ms={} attempts={} refusal=superseded_owner_not_replaced supervisor_pid={} wait_secs={} unblocker={} (#runfrontenderror)",
            file.display(),
            pane,
            harness_binary,
            reason,
            marked_secs,
            recycle_epoch,
            waited_ms,
            attempts,
            supervisor_pid,
            wait_secs,
            RECYCLE_SUPERSEDED_OWNER_UNBLOCKER,
        ),
    );
    let file_display = file.display().to_string();
    let outcome_fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
        RECYCLE_SUPERSEDED_OWNER_UNBLOCKER,
    );
    anyhow::anyhow!(dispatch_only_recycle_superseded_owner_message(
        DispatchOnlyRecycleSupersededOwnerMessageFacts {
            harness_binary,
            pane,
            file_display: &file_display,
            reason,
            supervisor_pid,
            waited_secs: wait_secs,
            outcome_fields: &outcome_fields,
        },
    ))
}

/// Resolve the superseded-owner replacement wait, honoring
/// `AGENT_DOC_RECYCLE_SUPERSEDED_OWNER_WAIT_SECS` so a test can shrink it.
fn recycle_superseded_owner_wait_secs() -> u64 {
    std::env::var(
        agent_doc_controller::dispatch::RECYCLE_SUPERSEDED_OWNER_REPLACEMENT_WAIT_SECS_ENV,
    )
    .ok()
    .and_then(|raw| raw.trim().parse::<u64>().ok())
    .unwrap_or(agent_doc_controller::dispatch::RECYCLE_SUPERSEDED_OWNER_REPLACEMENT_WAIT_SECS)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Resolve the `InFlight` settle TTL, honoring the
/// `AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS` override so a test can shrink it.
fn recycle_inflight_settle_ttl_secs() -> u64 {
    std::env::var(agent_doc_controller::dispatch::RECYCLE_INFLIGHT_SETTLE_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(agent_doc_controller::dispatch::RECYCLE_INFLIGHT_SETTLE_TTL_SECS)
}

pub fn dispatch_only_dispatch_start_proof_required(file: &Path, harness: &HarnessConfig) -> bool {
    if harness.binary == "codex"
        && crate::dispatch_start::codex_dispatch_start_tracking_enabled(file)
    {
        return true;
    }
    controller_dispatch_only_dispatch_start_proof_required(&harness.binary)
}

#[cfg(test)]
fn dispatch_only_dispatch_start_proof_required_with_user_hooks(
    file: &Path,
    harness: &HarnessConfig,
    user_hooks: Option<&Path>,
) -> bool {
    if harness.binary == "codex"
        && crate::dispatch_start::codex_dispatch_start_tracking_enabled_with_user_hooks(
            file, user_hooks,
        )
    {
        return true;
    }
    controller_dispatch_only_dispatch_start_proof_required(&harness.binary)
}

pub fn require_dispatch_only_dispatch_start_proof(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
    report_bug: impl FnMut(DispatchOnlyBugReportFacts),
) -> Result<()> {
    let proof_required = dispatch_only_dispatch_start_proof_required(file, harness);
    require_dispatch_only_dispatch_start_proof_with_requirement(
        file,
        pane,
        harness,
        delivery,
        dispatch_start,
        proof_required,
        report_bug,
    )
}

fn require_dispatch_only_dispatch_start_proof_with_requirement(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
    proof_required: bool,
    mut report_bug: impl FnMut(DispatchOnlyBugReportFacts),
) -> Result<()> {
    let classification =
        agent_doc_controller::dispatch::classify_dispatch_start_proof(DispatchStartProofFacts {
            proof: dispatch_start,
            dispatch_start_proof_required: proof_required,
        });
    if classification.decision == DispatchStartProofDecision::Accepted {
        return Ok(());
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let file_display = file.display().to_string();
    let facts = DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: timeout.as_secs(),
    };
    agent_doc_flow_io::log_flow_event(
        file,
        dispatch_proof_failed_event(RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof),
        agent_doc_ops_log_io::log_op,
    );
    if let Err(err) = agent_doc_controller_io::project_controller::mark_route_submit_blocked(
        file,
        pane,
        &harness.binary,
        "accepted_without_dispatch_start_proof",
    ) {
        eprintln!(
            "[route] warning: failed to mark accepted-without-dispatch route block for {}: {err:#}",
            file.display()
        );
    }
    agent_doc_ops_log_io::log_op(file, &accepted_only_dispatch_start_log_message(facts));
    // `DispatchStartUnproven` is produced only after the transport-specific
    // dispatcher has timed out, captured its diagnostic snapshot, and filed the
    // authoritative route bug. Filing again here creates a second queue item for
    // the same dispatch and can strand a second response capture against the live
    // document. Accepted-only fallbacks have no lower-layer timeout report, so
    // this guard remains their report owner.
    if dispatch_start != RoutedDispatchStartProof::DispatchStartUnproven {
        report_bug(DispatchOnlyBugReportFacts {
            elapsed: timeout,
            proof: dispatch_start,
        });
    }
    anyhow::bail!(accepted_only_dispatch_start_refusal_message(facts));
}

pub fn dispatch_only_sent_log_message_for(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_log_message(DispatchOnlyProofOutcomeFacts {
        file_display: &file_display,
        pane,
        harness_binary: &harness.binary,
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
        .as_secs(),
    })
}

pub fn dispatch_only_sent_console_message_for(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_console_message(DispatchOnlyProofOutcomeFacts {
        file_display: &file_display,
        pane,
        harness_binary: &harness.binary,
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
        .as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_only_codex_requires_start_proof_when_hooks_are_visible() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            dir.path().join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{
                            "type": "command",
                            "command": "agent-doc hook codex-user-prompt-submit",
                        }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(dispatch_only_dispatch_start_proof_required_with_user_hooks(
            &doc,
            &HarnessConfig::codex(),
            None,
        ));
        let err = require_dispatch_only_dispatch_start_proof_with_requirement(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            true,
            |_| {},
        )
        .expect_err("visible Codex hooks make accepted-only delivery insufficient");
        let message = err.to_string();
        assert!(
            message.contains("only pane-input acceptance proof was available"),
            "{message}"
        );
    }

    #[test]
    fn dispatch_only_codex_accepts_enter_delivery_without_visible_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            !dispatch_only_dispatch_start_proof_required_with_user_hooks(
                &doc,
                &HarnessConfig::codex(),
                None,
            )
        );
        require_dispatch_only_dispatch_start_proof_with_requirement(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            false,
            |_| {},
        )
        .expect(
            "Codex without hook tracking may accept text+Enter delivery for dispatch-only reroutes",
        );

        let message = dispatch_only_sent_log_message_for(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_submit_proof_gate_accepts_enter_delivery_without_codex_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        for harness in [
            HarnessConfig::codex(),
            HarnessConfig::opencode(),
            HarnessConfig::claude(),
        ] {
            require_dispatch_only_dispatch_start_proof_with_requirement(
                &doc,
                "%4",
                &harness,
                DispatchOnlyReopenDelivery::DirectPaneSubmit,
                RoutedDispatchStartProof::CommandAcceptedOnly,
                false,
                |_| {},
            )
            .expect("accepted-only delivery remains an explicit success path for this harness");
        }
    }

    #[test]
    fn dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed() {
        let mut reports = 0;
        let err = require_dispatch_only_dispatch_start_proof_with_requirement(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::DispatchStartUnproven,
            true,
            |_| reports += 1,
        )
        .expect_err("tracked dispatch-start timeouts must not report route success");

        let message = format!("{err:#}");
        assert!(
            message.contains("only pane-input acceptance proof"),
            "{message}"
        );
        assert!(
            message.contains("no dispatch-start proof was recorded"),
            "{message}"
        );
        assert_eq!(
            reports, 0,
            "the transport timeout already owns the diagnostic route-bug report"
        );
    }

    #[test]
    fn dispatch_only_accepted_only_failure_still_files_its_fallback_report() {
        let mut reports = 0;
        require_dispatch_only_dispatch_start_proof_with_requirement(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            true,
            |_| reports += 1,
        )
        .expect_err("accepted-only proof must fail closed when start proof is required");

        assert_eq!(
            reports, 1,
            "the outer guard owns accepted-only fallback reports"
        );
    }

    #[test]
    fn dispatch_only_sent_log_marks_claude_accepted_only_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/robert-ross.md"),
            "%7",
            &HarnessConfig::claude(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );

        assert!(message.contains("harness=claude"), "{message}");
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_sent_log_marks_opencode_accepted_only_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        );

        assert!(message.contains("harness=opencode"), "{message}");
        assert!(message.contains("proof=accepted"), "{message}");
        assert!(message.contains("proof_scope=accepted_only"), "{message}");
    }

    #[test]
    fn dispatch_only_sent_log_marks_opencode_pane_state_dispatch_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::PaneStateChanged,
        );

        assert!(message.contains("harness=opencode"), "{message}");
        assert!(message.contains("proof=pane_state_changed"), "{message}");
        assert!(message.contains("proof_scope=dispatch_start"), "{message}");
    }

    #[test]
    fn dispatch_only_opencode_accepted_only_proof_is_successful_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_opencode_pane_state_proof_is_successful_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/sampleorders.md"),
            "%13",
            &HarnessConfig::opencode(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::PaneStateChanged,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_claude_accepted_only_proof_remains_accepted_delivery() {
        require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/robert-ross.md"),
            "%7",
            &HarnessConfig::claude(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
            |_| {},
        )
        .unwrap();
    }

    #[test]
    fn dispatch_only_sent_log_marks_codex_hook_proof_scope() {
        let message = dispatch_only_sent_log_message_for(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%1",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::HookPromptMatched,
        );

        assert!(message.contains("harness=codex"), "{message}");
        assert!(message.contains("proof=consumed"), "{message}");
        assert!(message.contains("proof_scope=dispatch_start"), "{message}");
    }
}
