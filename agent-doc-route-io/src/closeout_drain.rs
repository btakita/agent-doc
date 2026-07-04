//! Route closeout drain and closeout-block dispatch I/O.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    CloseoutBlockDispatchDecision, CloseoutBlockDispatchFacts, DispatchDrainRetryDecision,
    RouteCloseoutDrainOutcome, classify_closeout_block_dispatch, dispatch_drain_retry_decision,
};
use agent_doc_session_check_io::SessionCheckStatus;
use agent_doc_turn::closeout_recovery::{CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput};

pub type DecideRouteCloseoutRecoveryFn =
    for<'a> fn(&Path, CloseoutRecoveryDecisionInput<'a>) -> CloseoutRecoveryDecision;

#[derive(Clone, Copy)]
pub struct RouteCloseoutDrainEffects {
    pub force_disk_route_writes: fn() -> bool,
    pub run_pending_maintenance: fn(&Path, bool) -> Result<()>,
    pub repair_closeout: fn(&Path) -> Result<String>,
    pub inspect_session: fn(&Path) -> Result<SessionCheckStatus>,
    pub decide_closeout_recovery: DecideRouteCloseoutRecoveryFn,
}

pub fn drain_open_closeout_before_routed_dispatch(
    file: &Path,
    effects: RouteCloseoutDrainEffects,
) -> Result<RouteCloseoutDrainOutcome> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_started file={} cycle_id={} phase={:?}",
            file.display(),
            state.cycle_id,
            state.phase
        ),
    );

    // #pcp3a: a concurrent finalize in another process (the route-owned supervisor
    // self-race) can move the document/cycle baseline mid-drain, so `repair` +
    // `session_check` observe a transient "captured response baseline no longer
    // matches current document" mismatch. Rather than fail closed on the first
    // such block (the "could not drain the active closeout" / exit 75 the user
    // hit, which self-resolves once the finalize completes), retry a bounded
    // number of times when there is positive evidence the cycle is concurrently
    // progressing (phase/cycle_id advanced) or has just closed. A genuine,
    // non-advancing block still fails closed after the first attempt.
    const DRAIN_MAX_ATTEMPTS: u32 = 3;
    const DRAIN_RETRY_BACKOFF_MS: u64 = 200;
    let mut last_reason = String::new();

    for attempt in 0..DRAIN_MAX_ATTEMPTS {
        // Reap completed tracked items across ALL surfaces (backlog, review,
        // icebox) and re-sync the snapshot before the focused repair, matching
        // what a manual re-run's full preflight maintenance does. The repair
        // sub-step only reaps the backlog, so a deployed/completed `[x]` item left
        // in review or icebox would make that reap a no-op, the post-repair
        // session-check would still find the completed item, and route would
        // refuse dispatch until the user manually retried (the "JB Run Agent Doc
        // failed; repeat succeeded" report). run_pending_maintenance is
        // idempotent, so this is safe even when there is nothing to reap.
        if let Err(e) = (effects.run_pending_maintenance)(file, (effects.force_disk_route_writes)())
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_drain_pending_maintenance_warning file={} error={}",
                    file.display(),
                    agent_doc_secret_redact::redact(&e.to_string())
                ),
            );
        }

        let block_reason = match (effects.repair_closeout)(file) {
            Ok(label) => match (effects.inspect_session)(file)? {
                SessionCheckStatus::Ok(_) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_drain_closeout_recovered file={} cycle_id={} outcome={}",
                            file.display(),
                            state.cycle_id,
                            label
                        ),
                    );
                    return Ok(RouteCloseoutDrainOutcome::Recovered(label));
                }
                SessionCheckStatus::Interrupted(reason) => reason,
            },
            Err(err) => err.to_string(),
        };

        // Concurrent-finalize detection: re-read the cycle after the failed check.
        let reloaded = agent_doc_cycle_state_io::load(file)?;
        let decision = dispatch_drain_retry_decision(
            &state.cycle_id,
            state.phase,
            reloaded
                .as_ref()
                .map(|s| (s.cycle_id.as_str(), s.phase, s.is_open())),
            attempt,
            DRAIN_MAX_ATTEMPTS,
        );
        last_reason = block_reason;
        match decision {
            DispatchDrainRetryDecision::ConcurrentlyClosed => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_concurrent_finalize_closed file={} cycle_id={}",
                        file.display(),
                        state.cycle_id
                    ),
                );
                return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
            }
            DispatchDrainRetryDecision::Retry => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_retry_concurrent_progress file={} cycle_id={} attempt={}",
                        file.display(),
                        state.cycle_id,
                        attempt + 1
                    ),
                );
                std::thread::sleep(Duration::from_millis(DRAIN_RETRY_BACKOFF_MS));
                continue;
            }
            DispatchDrainRetryDecision::GiveUp => break,
        }
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_blocked file={} cycle_id={} blocker={}",
            file.display(),
            state.cycle_id,
            agent_doc_secret_redact::redact(&last_reason)
        ),
    );
    Ok(RouteCloseoutDrainOutcome::Blocked(last_reason))
}

pub fn classify_route_closeout_block(
    file: &Path,
    reason: String,
    has_prompt_context: bool,
    effects: RouteCloseoutDrainEffects,
) -> (CloseoutRecoveryDecision, CloseoutBlockDispatchDecision) {
    let recovery_decision = (effects.decide_closeout_recovery)(
        file,
        CloseoutRecoveryDecisionInput {
            prompt_context_available: has_prompt_context,
            blocker_reason: Some(&reason),
            stale_capture_supersession_proof: None,
        },
    );
    let recovery_queues_prompt_for_after_closeout = matches!(
        recovery_decision,
        CloseoutRecoveryDecision::QueuePromptForAfterCloseout { .. }
    );
    let active_queue_head = if recovery_queues_prompt_for_after_closeout {
        None
    } else {
        std::fs::read_to_string(file).ok().and_then(|content| {
            agent_doc_queue::queue_continuation::live_continuation_head(&content)
        })
    };
    let dispatch_decision = classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
        recovery_queues_prompt_for_after_closeout,
        active_queue_head,
    });
    (recovery_decision, dispatch_decision)
}
