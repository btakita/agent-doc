//! Route closeout drain and closeout-block dispatch I/O.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    CloseoutBlockDispatchDecision, CloseoutBlockDispatchFacts, CloseoutDrainProjection,
    CloseoutProjectionChange, RouteCloseoutDrainOutcome, classify_closeout_block_dispatch,
    project_closeout_drain,
};
use agent_doc_controller_io::project_controller::CloseoutCycleWaitOutcome;
use agent_doc_session_check_io::SessionCheckStatus;
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryCycleInput, CloseoutRecoveryDecision, CloseoutRecoveryDecisionInput,
};

pub type DecideRouteCloseoutRecoveryFn =
    for<'a> fn(&Path, CloseoutRecoveryDecisionInput<'a>) -> CloseoutRecoveryDecision;
pub type AwaitCloseoutProjectionFn = fn(&Path, &str, Duration) -> Result<CloseoutCycleWaitOutcome>;

#[derive(Clone, Copy)]
pub struct RouteCloseoutDrainEffects {
    pub force_disk_route_writes: fn() -> bool,
    pub run_pending_maintenance: fn(&Path, bool) -> Result<()>,
    pub cancel_empty_preflight: fn(&Path) -> Result<bool>,
    pub repair_closeout: fn(&Path) -> Result<String>,
    pub inspect_session: fn(&Path) -> Result<SessionCheckStatus>,
    pub await_closeout_projection: AwaitCloseoutProjectionFn,
    pub decide_closeout_recovery: DecideRouteCloseoutRecoveryFn,
}

enum CloseoutRecoveryAttempt {
    Recovered(String),
    Blocked(String),
}

fn project_closeout_recovery_effects(
    file: &Path,
    effects: RouteCloseoutDrainEffects,
) -> Result<CloseoutRecoveryAttempt> {
    let block_reason = match (effects.repair_closeout)(file) {
        Ok(label) => match (effects.inspect_session)(file)? {
            SessionCheckStatus::Ok(_) => return Ok(CloseoutRecoveryAttempt::Recovered(label)),
            SessionCheckStatus::Interrupted(reason) => reason,
        },
        Err(error) => error.to_string(),
    };
    Ok(CloseoutRecoveryAttempt::Blocked(block_reason))
}

pub fn drain_open_closeout_before_routed_dispatch(
    file: &Path,
    effects: RouteCloseoutDrainEffects,
) -> Result<RouteCloseoutDrainOutcome> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
    }

    let cycle = CloseoutRecoveryCycleInput {
        phase: state.phase,
        has_capture: state.capture_id.is_some(),
        has_response_hash: state.response_sha256.is_some(),
        had_pending_mutations: state.had_pending_mutations,
    };
    if cycle.is_empty_preflight()
        && state.tracked_work_maintenance_required_at_preflight != Some(true)
        && (effects.cancel_empty_preflight)(file)?
    {
        let label = "empty_preflight_cancelled".to_string();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_drain_empty_preflight_cancelled file={} cycle_id={}",
                file.display(),
                state.cycle_id,
            ),
        );
        return Ok(RouteCloseoutDrainOutcome::Recovered(label));
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

    // Reap completed tracked items once before recovery. Subsequent progress is
    // driven by the controller's document projection, not a sleep/re-read loop.
    if let Err(error) = (effects.run_pending_maintenance)(file, (effects.force_disk_route_writes)())
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_drain_pending_maintenance_warning file={} error={}",
                file.display(),
                agent_doc_secret_redact::redact(&error.to_string())
            ),
        );
    }

    let first_reason = match project_closeout_recovery_effects(file, effects)? {
        CloseoutRecoveryAttempt::Recovered(label) => {
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
        CloseoutRecoveryAttempt::Blocked(reason) => reason,
    };

    let change = match (effects.await_closeout_projection)(
        file,
        &state.cycle_id,
        Duration::from_secs(30),
    )? {
        CloseoutCycleWaitOutcome::Terminal => CloseoutProjectionChange::Terminal,
        CloseoutCycleWaitOutcome::Superseded => CloseoutProjectionChange::Superseded,
        CloseoutCycleWaitOutcome::OwnerReleased => CloseoutProjectionChange::OwnerReleased,
        CloseoutCycleWaitOutcome::TimedOut => CloseoutProjectionChange::TimedOut,
    };
    let last_reason = match project_closeout_drain(change) {
        CloseoutDrainProjection::DispatchReady => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_drain_closeout_projection_ready file={} cycle_id={}",
                    file.display(),
                    state.cycle_id
                ),
            );
            return Ok(RouteCloseoutDrainOutcome::NoOpenCycle);
        }
        CloseoutDrainProjection::RecoverAfterOwnerRelease => {
            match project_closeout_recovery_effects(file, effects)? {
                CloseoutRecoveryAttempt::Recovered(label) => {
                    return Ok(RouteCloseoutDrainOutcome::Recovered(label));
                }
                CloseoutRecoveryAttempt::Blocked(reason) => reason,
            }
        }
        CloseoutDrainProjection::AwaitingTerminal => first_reason,
    };

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
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "route_closeout_block_active_queue_head",
        )
        .ok()
        .and_then(|content| agent_doc_queue::queue_continuation::live_continuation_head(&content))
    };
    let dispatch_decision = classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
        recovery_queues_prompt_for_after_closeout,
        active_queue_head,
    });
    (recovery_decision, dispatch_decision)
}
