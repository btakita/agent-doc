//! Route closeout drain and closeout-block dispatch I/O.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    AuthoritativeActorDispatchIntent, CloseoutBlockDispatchDecision, CloseoutBlockDispatchFacts,
    CloseoutDrainProjection, CloseoutProjectionChange, ReopenMode, RouteCloseoutBlockContext,
    RouteCloseoutDrainOutcome, RouteCloseoutDrainPolicy, classify_closeout_block_dispatch,
    classify_route_closeout_drain_policy, project_closeout_drain,
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
    /// `#duplicatepreflightunblock`: reclaim an empty preflight once the
    /// controller projection has PROVEN the owning actor released the cycle.
    /// `cancel_empty_preflight` above carries no such proof, so it refuses by
    /// construction while a first response may still be generating — which is
    /// why a duplicate invocation had no bounded exit before this.
    pub cancel_empty_preflight_after_owner_release: fn(&Path) -> Result<bool>,
    pub repair_closeout: fn(&Path) -> Result<String>,
    pub inspect_session: fn(&Path) -> Result<SessionCheckStatus>,
    pub await_closeout_projection: AwaitCloseoutProjectionFn,
    pub decide_closeout_recovery: DecideRouteCloseoutRecoveryFn,
    /// GH 91: operator report for the open cycle's capture when its bytes can
    /// never land (GH 90's deterministic refusal), else `None`.
    pub unlandable_capture_report: fn(&Path) -> Result<Option<String>>,
}

enum CloseoutRecoveryAttempt {
    Recovered(String),
    Blocked(CloseoutRecoveryBlock),
}

/// Why a recovery attempt left the closeout open.
struct CloseoutRecoveryBlock {
    reason: String,
    /// GH #228: session-check's own `#sessioncheckliveturn` verdict (IN
    /// PROGRESS). Carried typed so the route never re-derives the blocker from
    /// the cycle shape and recommends interrupting a healthy turn.
    live_owner_turn: bool,
}

fn project_closeout_recovery_effects(
    file: &Path,
    effects: RouteCloseoutDrainEffects,
) -> Result<CloseoutRecoveryAttempt> {
    let block = match (effects.repair_closeout)(file) {
        Ok(label) => {
            let status = (effects.inspect_session)(file)?;
            let live_owner_turn = status.is_live_owner_turn_in_progress();
            match status {
                // `#steerinterruptexit`: the closeout recovered; pending steering is
                // the routed dispatch's own input, not a block.
                SessionCheckStatus::Ok(_) | SessionCheckStatus::SteeringPending(_) => {
                    return Ok(CloseoutRecoveryAttempt::Recovered(label));
                }
                SessionCheckStatus::Interrupted(reason) => CloseoutRecoveryBlock {
                    reason,
                    live_owner_turn,
                },
            }
        }
        // The repair itself runs the same session-check and refuses with its
        // verdict, so the live-turn classification is read from that verdict
        // on this branch too.
        Err(error) => {
            let reason = error.to_string();
            CloseoutRecoveryBlock {
                live_owner_turn: agent_doc_session_check_io::is_live_owner_turn_verdict(&reason),
                reason,
            }
        }
    };
    Ok(CloseoutRecoveryAttempt::Blocked(block))
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

    // GH 91: a deterministic refusal is not a transient wait. Every retry of
    // an unlandable capture fails identically, so neither repair, the
    // projection wait, nor retained-write recovery can clear it — and repair
    // must never replay those bytes. Fail closed before any recovery with the
    // structural reason and the one command that changes the outcome.
    if let Some(report) = (effects.unlandable_capture_report)(file)? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_drain_closeout_unlandable file={} cycle_id={} blocker={}",
                file.display(),
                state.cycle_id,
                agent_doc_secret_redact::redact(&report)
            ),
        );
        return Ok(RouteCloseoutDrainOutcome::Unlandable(report));
    }

    let first_block = match project_closeout_recovery_effects(file, effects)? {
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
        CloseoutRecoveryAttempt::Blocked(block) => block,
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
    let last_block = match project_closeout_drain(change) {
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
                // `#duplicatepreflightunblock`: ordinary recovery cannot close
                // an empty preflight — it owns no response to replay and no
                // drift to commit — so before this the drain reported `Blocked`
                // and every later duplicate invocation repeated the same 30s
                // await, indefinitely, until an operator ran `session
                // cancel-turn` by hand. The projection has now proven the owner
                // RELEASED the cycle, which is the same fact run cancellation
                // proves, so the reclaim is authorized and bounded.
                CloseoutRecoveryAttempt::Blocked(block) => {
                    if cycle.is_empty_preflight()
                        && state.tracked_work_maintenance_required_at_preflight != Some(true)
                        && (effects.cancel_empty_preflight_after_owner_release)(file)?
                    {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_dispatch_drain_empty_preflight_cancelled_after_owner_release file={} cycle_id={}",
                                file.display(),
                                state.cycle_id,
                            ),
                        );
                        return Ok(RouteCloseoutDrainOutcome::Recovered(
                            "empty_preflight_cancelled_after_owner_release".to_string(),
                        ));
                    }
                    block
                }
            }
        }
        CloseoutDrainProjection::AwaitingTerminal => first_block,
    };
    let last_reason = last_block.reason;
    // GH #228: reuse session-check's verdict. An open `preflight_started`
    // cycle whose owner holds a fresh active-turn lease is a busy owner, even
    // though its shape is also an empty preflight: the cancel/repair live-turn
    // guards have just refused to touch it for the same reason.
    let context = if last_block.live_owner_turn
        && matches!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted)
    {
        RouteCloseoutBlockContext::LiveOwnerTurn
    } else if cycle.is_empty_preflight() {
        RouteCloseoutBlockContext::OpenEmptyPreflight
    } else {
        RouteCloseoutBlockContext::Other
    };

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_drain_closeout_blocked file={} cycle_id={} context={} blocker={}",
            file.display(),
            state.cycle_id,
            route_closeout_block_context_label(context),
            agent_doc_secret_redact::redact(&last_reason)
        ),
    );
    Ok(RouteCloseoutDrainOutcome::Blocked {
        reason: last_reason,
        context,
    })
}

pub fn route_closeout_block_context_label(context: RouteCloseoutBlockContext) -> &'static str {
    match context {
        RouteCloseoutBlockContext::OpenEmptyPreflight => "open_empty_preflight",
        RouteCloseoutBlockContext::LiveOwnerTurn => "live_owner_turn",
        RouteCloseoutBlockContext::Other => "other",
    }
}

pub fn apply_routed_dispatch_closeout_policy(
    file: &Path,
    mode: ReopenMode,
    intent: AuthoritativeActorDispatchIntent,
    effects: RouteCloseoutDrainEffects,
) -> Result<RouteCloseoutDrainOutcome> {
    match classify_route_closeout_drain_policy(mode, intent) {
        RouteCloseoutDrainPolicy::DrainBeforeDispatch => {
            drain_open_closeout_before_routed_dispatch(file, effects)
        }
    }
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
        // `#planhead`/`#qchurn`: drainability-filtered, not the raw head. The
        // unfiltered `live_continuation_head` returns the first head regardless of
        // whether any drainer will act on it, so an `[operator-verify]` head — one
        // preflight has already deferred and that `session-check` reports as
        // needing no continuation — made a blocked closeout dispatch anyway and
        // burned a guaranteed no-op cycle. Supervisor scope is the right scope
        // here: the supervisor clear-and-continue path does drain
        // `[focused-cycle]`/`[clean-session]` heads, and defers only
        // `[operator-verify]`/noise.
        .and_then(|content| {
            agent_doc_queue::queue_continuation::live_drainable_continuation_head(
                &content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            )
        })
    };
    let dispatch_decision = classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
        recovery_queues_prompt_for_after_closeout,
        active_queue_head,
    });
    (recovery_decision, dispatch_decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_effects() -> RouteCloseoutDrainEffects {
        RouteCloseoutDrainEffects {
            force_disk_route_writes: || false,
            run_pending_maintenance: |_, _| Ok(()),
            cancel_empty_preflight: |_| Ok(false),
            cancel_empty_preflight_after_owner_release: |_| Ok(false),
            repair_closeout: |_| Ok(String::new()),
            inspect_session: |_| Ok(SessionCheckStatus::Ok("stub".to_string())),
            await_closeout_projection: |_, _, _| Ok(CloseoutCycleWaitOutcome::Terminal),
            // Anything but `QueuePromptForAfterCloseout`, so the queue-head branch
            // under test is the one that runs.
            decide_closeout_recovery: |_, _| CloseoutRecoveryDecision::AlreadyCommitted,
            unlandable_capture_report: |_| Ok(None),
        }
    }

    fn operator_verify_only_queue_doc(dir: &Path) -> std::path::PathBuf {
        let doc = dir.join("session.md");
        std::fs::write(
            &doc,
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "queue: go\n",
                "queue_active: true\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — opus-5\n\nDone.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n",
                "- do [#opv]\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog queue=append -->\n",
                "- [ ] [#opv] [operator-verify] needs a human eyeball, do not auto-drain\n",
                "<!-- /agent:backlog -->\n",
            ),
        )
        .unwrap();
        doc
    }

    #[test]
    fn closeout_block_does_not_wait_on_an_operator_verify_only_queue_head() {
        // `#planhead`/`#qchurn`: this used to read the raw
        // `live_continuation_head`, so an `[operator-verify]` head — deferred by
        // preflight and reported by `session-check` as needing no continuation —
        // came back as an active queue head and the blocked closeout dispatched
        // for it anyway, burning a guaranteed no-op cycle. No drainer acts on
        // that head, so the decision must not be `WaitForActiveQueueHead`.
        let dir = tempfile::tempdir().unwrap();
        let doc = operator_verify_only_queue_doc(dir.path());

        let (_recovery, dispatch) = classify_route_closeout_block(
            &doc,
            "blocked for test".to_string(),
            false,
            stub_effects(),
        );

        assert!(
            !matches!(
                dispatch,
                CloseoutBlockDispatchDecision::WaitForActiveQueueHead { .. }
            ),
            "an operator-verify-only queue must not be treated as a drainable active head: {dispatch:?}"
        );
    }
}
