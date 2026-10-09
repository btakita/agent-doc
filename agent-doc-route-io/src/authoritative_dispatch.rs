//! Authoritative actor routed dispatch I/O.

use anyhow::Result;
use std::path::Path;

fn actor_transport_harness(recorded_harness: &str) -> HarnessConfig {
    HarnessConfig::from_agent_name(recorded_harness)
}
use std::time::Duration;

use agent_doc_controller::dispatch::{
    ActorDispatchState, AuthoritativeActorDispatchAction, AuthoritativeActorDispatchActionFacts,
    AuthoritativeActorDispatchIntent, CloseoutBlockDispatchDecision, DispatchOnlyBusyRefusalFacts,
    DispatchOnlyReopenDelivery, ReopenMode, RouteCloseoutBlockContext, RouteCloseoutDrainOutcome,
    RouteExplicitSendDecision, RouteOwnerTurnEvidence, RoutedDispatchStartProof, RoutedReopenFacts,
    RoutedReopenGuardReason, StartingTimeoutActorFacts, actor_blocked_by_starting_timeout,
    actor_dispatch_blocker_reason, busy_projection_repaired_by_ready_prompt,
    classify_authoritative_actor_dispatch_action, classify_route_explicit_send,
    decide_authoritative_reopen,
    dispatch_only_busy_refusal_message as controller_dispatch_only_busy_refusal_message,
    dispatch_only_busy_refusal_wait_secs, dispatch_only_busy_should_wait_for_ready,
    dispatch_only_focus_only_should_fail_closed, dispatch_only_should_probe_active_turn_cue,
    dispatch_only_starting_pane_recovery_timeout_for_binary, failclosed_wait_context,
    prompt_ready_barrier_failed_event, route_closeout_user_outcome_fields,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry::lookup_dispatch_registration;
use agent_doc_supervisor::route_runtime::authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible;
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryCommandInput, CloseoutRecoveryState, blocked_closeout_recovery_command,
    closeout_recovery_command, short_recovery_command_from_recommendation,
};
use agent_doc_turn::prompt_bearing_route::PromptBearingRouteContext;
use tmux_router::Tmux;

use crate::admission_projection::{RouteAdmissionEffects, require_routed_admission_projection};
use crate::authoritative_actor::{
    AuthoritativeActorDispatchTarget, PendingHarnessSwitch, RouteDispatchAuthorization,
    actor_dispatch_state, authoritative_actor_dispatch_recovery_hint,
    authorize_controller_dispatch, current_generation_ready_prompt_proven,
    load_authoritative_actor_binding, promote_starting_authoritative_actor_if_dispatch_ready,
    recover_starting_timeout_blocked_actor_if_dispatch_ready, route_dispatch_deduped_pane,
    wait_for_authoritative_actor_ready,
};
use crate::closeout_drain::{
    RouteCloseoutDrainEffects, apply_routed_dispatch_closeout_policy, classify_route_closeout_block,
};
use crate::dispatch::{RouteDispatchEffects, dispatch_via_supervisor_ipc};
use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_send_reopen,
};
use crate::dispatch_target::register_dispatch_target;
use crate::pane_resolution::{
    controller_dispatch_actor_state, recover_dispatch_only_authoritative_waiting_input,
    rescue_from_stash,
};
use crate::queue_dispatch::{
    RouteQueueEffects, activate_existing_route_queue_head,
    enqueue_exchange_slash_command_for_idle_drain, enqueue_route_dispatch_prompt,
    inactive_route_queue_head,
};
use crate::supervisor_runtime::query_supervisor_runtime;

#[derive(Clone, Copy)]
pub struct RouteAuthoritativeActorEffects {
    pub closeout_drain_effects: RouteCloseoutDrainEffects,
    pub queue_effects: RouteQueueEffects,
    pub route_dispatch_effects: RouteDispatchEffects,
    pub route_admission_effects: RouteAdmissionEffects,
    pub dispatch_only_route_effects: DispatchOnlyRouteEffects,
    pub wait_for_ready_override: fn() -> Option<Duration>,
}

/// Re-capture the pane after a short delay and report whether it *still* shows no
/// busy cue.
///
/// `#jbsteerinterrupt`: used to confirm a stale-busy projection before promoting
/// it to ready. A live turn re-renders its spinner every frame, so a second
/// cue-free capture distinguishes a genuinely idle pane from a torn or mid-redraw
/// frame. Fails closed — a capture error reports "not confirmed idle", leaving the
/// busy projection in place.
fn confirm_pane_still_idle(tmux: &Tmux, pane: &str, harness: &HarnessConfig) -> bool {
    std::thread::sleep(Duration::from_millis(250));
    match tmux.capture_pane(pane, Some(80)) {
        Ok(content) => !harness.has_busy_cue(&content),
        Err(_) => false,
    }
}

fn detect_active_queue_continuation(
    file: &Path,
    source: &str,
) -> Result<Option<agent_doc_queue::queue_continuation::QueueContinuation>> {
    let content =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)?;
    agent_doc_queue_io::queue_continuation::detect_for_content(file, &content)
}

/// Accept an explicit frontmatter harness change without dispatching into the
/// still-live old harness. The supervisor idle watch owns the safe-boundary
/// fresh spawn and its restart loop auto-triggers the document in the new
/// harness, so route only records/coalesces the handoff here.
fn accept_pending_harness_switch(
    tmux: &Tmux,
    file: &Path,
    actor: &AuthoritativeActorDispatchTarget,
    pending: &PendingHarnessSwitch,
) -> Result<String> {
    let dispatch_pane = actor.record.pane_id.clone();
    if !crate::invocation::preserve_route_layout()
        && let Err(err) = tmux.select_pane(&dispatch_pane)
    {
        eprintln!(
            "[route] warning: failed to focus pending harness handoff pane {}: {}",
            dispatch_pane, err
        );
    }
    let action = if pending.queue_paused {
        "accepted_pending_queue_resume"
    } else if pending.restart_in_flight {
        "coalesced_restart_in_flight"
    } else {
        "accepted_boundary_handoff"
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_harness_switch_handoff_accepted file={} pane={} generation={} old_harness={} new_harness={} actor_state={} queue_paused={} restart_in_flight={} action={} dispatch_old_harness=false auto_trigger=new_harness",
            file.display(),
            dispatch_pane,
            actor.record.generation,
            pending.previous_harness,
            pending.target_harness,
            actor.actor_state().as_str(),
            pending.queue_paused,
            pending.restart_in_flight,
            action,
        ),
    );
    let prerequisite = if pending.queue_paused {
        " Resume the agent-doc queue to release the handoff; no supervisor restart is required."
    } else if pending.restart_in_flight {
        " The fresh harness restart is already in flight and this request was coalesced."
    } else {
        " The supervisor will preserve any active turn, switch at the next safe boundary, and auto-trigger this document in the new harness."
    };
    eprintln!(
        "[route] accepted live harness handoff for {} on pane {} ({} -> {}).{} {}",
        file.display(),
        dispatch_pane,
        pending.previous_harness,
        pending.target_harness,
        prerequisite,
        agent_doc_flow::outcome::user_outcome_fields(
            agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
        ),
    );
    Ok(dispatch_pane)
}

/// What a route that met an undrainable open closeout reports
/// (`#claimedsteerwake`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutWaitRouteOutcome {
    /// The explicit send handed pending steering to the owning turn: a
    /// success, never "nothing was dispatched".
    SteeringDelivered(String),
    /// Nothing to send: the existing queue head waits behind the closeout.
    Deferred(String),
    /// GH #228 (`#routeliveturnverdict`): the open closeout is a live
    /// owner-scoped turn that is already doing the work. Benign and non-error
    /// (exit 0): no second trigger, no recovery, nothing for the operator to do.
    OwnerTurnBusy(String),
}

impl CloseoutWaitRouteOutcome {
    pub fn message(&self) -> &str {
        match self {
            Self::SteeringDelivered(message)
            | Self::Deferred(message)
            | Self::OwnerTurnBusy(message) => message,
        }
    }

    /// Hand this outcome to the invocation so the editor route reports it with
    /// the right exit status (only `Deferred` is a non-zero `EX_TEMPFAIL`).
    fn record(self, file: &Path, explicit_items: usize) {
        match self {
            Self::SteeringDelivered(message) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_explicit_send_delivered_to_owner_turn file={} items={explicit_items} (#claimedsteerwake)",
                        file.display()
                    ),
                );
                crate::invocation::record_route_steering_delivery(message);
            }
            Self::Deferred(message) => crate::invocation::record_route_deferral(message),
            Self::OwnerTurnBusy(message) => {
                crate::invocation::record_route_live_owner_turn(message)
            }
        }
    }
}

/// GH #228 (`#routeliveturnverdict`): pure outcome for a route that met an open
/// closeout session-check classified as a live owner-scoped turn (IN
/// PROGRESS). It reuses that verdict verbatim, never a re-derived
/// `open_empty_preflight` recovery, and never claims the queue head "remains
/// queued behind the closeout": the live turn owns the queue and is working it.
pub fn live_owner_turn_route_outcome(
    file: &Path,
    head: Option<&str>,
    owner_verdict: &str,
    explicit_items: usize,
) -> CloseoutWaitRouteOutcome {
    let fields = route_closeout_user_outcome_fields(None, RouteOwnerTurnEvidence::Live);
    if explicit_items > 0 {
        return closeout_wait_route_outcome(
            file,
            head.unwrap_or_default(),
            owner_verdict,
            &fields,
            explicit_items,
            RouteOwnerTurnEvidence::Live,
        );
    }
    let work = match head {
        Some(head) => format!(
            "The live turn owns the queue; head {head:?} is in its scope and is answered or drained at that turn's boundary."
        ),
        None => "Its response closes out at that turn's boundary.".to_string(),
    };
    CloseoutWaitRouteOutcome::OwnerTurnBusy(format!(
        "[route] a turn is already running on {}; no second trigger was sent and nothing needs recovery. {work} Do not cancel or recover it (owner: {owner_verdict}) owner_turn=live {fields}",
        file.display(),
    ))
}

/// Pure outcome for the `WaitForActiveQueueHead` closeout block.
pub fn closeout_wait_route_outcome(
    file: &Path,
    head: &str,
    blocker: &str,
    outcome_fields: &str,
    explicit_items: usize,
    owner_turn: RouteOwnerTurnEvidence,
) -> CloseoutWaitRouteOutcome {
    match classify_route_explicit_send(explicit_items, owner_turn) {
        RouteExplicitSendDecision::DeliverToLiveOwner => {
            CloseoutWaitRouteOutcome::SteeringDelivered(format!(
                "[route] explicit send for {}: delivered {explicit_items} pending steering item(s) to the owning turn (sent=explicit); its next tool call, its turn-boundary report, or the idle wake surfaces them. No second trigger was injected: the open closeout still owns the pane (blocker: {blocker}). steering_delivered={explicit_items} sent=explicit {outcome_fields}",
                file.display(),
            ))
        }
        RouteExplicitSendDecision::BlockOwnerAbsent => CloseoutWaitRouteOutcome::Deferred(format!(
            "[route] explicit send for {} is blocked: {explicit_items} pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: {blocker}). steering_delivered=0 sent=explicit {outcome_fields}",
            file.display(),
        )),
        RouteExplicitSendDecision::DeferNoExplicitItems => {
            CloseoutWaitRouteOutcome::Deferred(format!(
                "[route] active closeout for {} could not be drained before reroute; nothing was dispatched and existing queue head {:?} remains queued behind the closeout (blocker: {}) {}",
                file.display(),
                head,
                blocker,
                outcome_fields
            ))
        }
    }
}

fn open_empty_preflight_recovery_command(file: &Path) -> Option<String> {
    closeout_recovery_command(CloseoutRecoveryCommandInput {
        document: file.display().to_string(),
        state: CloseoutRecoveryState::OpenEmptyPreflight,
        open_cycle: None,
    })
    .and_then(|recommendation| short_recovery_command_from_recommendation(&recommendation))
}

#[allow(clippy::too_many_arguments)]
pub fn route_via_authoritative_actor(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    requested_harness: &HarnessConfig,
    baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    prompt_context: Option<&PromptBearingRouteContext>,
    dispatch_only: bool,
    plain_trigger: bool,
    actor: AuthoritativeActorDispatchTarget,
    effects: RouteAuthoritativeActorEffects,
) -> Result<String> {
    let mut actor = actor;
    let mut dispatch_pane = actor.record.pane_id.clone();
    let mut actor_state = actor.actor_state();
    let reopen_mode = if dispatch_only {
        ReopenMode::DispatchOnly
    } else {
        ReopenMode::Managed
    };
    let dispatch_intent = if plain_trigger {
        AuthoritativeActorDispatchIntent::PlainTrigger
    } else {
        AuthoritativeActorDispatchIntent::PromptAware
    };
    // A plain editor trigger is transport-only after route admission. It never
    // inherits prompt-bearing work derived before this authoritative boundary,
    // but it still drains any prior open closeout before transport begins.
    let prompt_context = if matches!(
        dispatch_intent,
        AuthoritativeActorDispatchIntent::PlainTrigger
    ) {
        None
    } else {
        prompt_context
    };
    let prompt_bearing_marker = prompt_context.map(|context| context.marker.as_str());
    if let Some(pending) = actor.pending_harness_switch.as_ref() {
        return accept_pending_harness_switch(tmux, file, &actor, pending);
    }
    let closeout_drain = agent_doc_tmux_io::with_current_pane_id_override(&dispatch_pane, || {
        apply_routed_dispatch_closeout_policy(
            file,
            reopen_mode,
            dispatch_intent,
            effects.closeout_drain_effects,
        )
    })?;
    match closeout_drain {
        RouteCloseoutDrainOutcome::NoOpenCycle => {}
        RouteCloseoutDrainOutcome::Recovered(outcome) => {
            eprintln!(
                "[route] drained open closeout for {} before reroute ({})",
                file.display(),
                outcome
            );
            if let Some(refreshed) = load_authoritative_actor_binding(
                tmux,
                file,
                session_id,
                file_path,
                requested_harness,
                false,
                false,
            )? {
                actor = refreshed;
                dispatch_pane = actor.record.pane_id.clone();
                actor_state = actor.actor_state();
                if let Some(pending) = actor.pending_harness_switch.as_ref() {
                    return accept_pending_harness_switch(tmux, file, &actor, pending);
                }
            }
        }
        RouteCloseoutDrainOutcome::Unlandable(report) => {
            let recovery_command =
                agent_doc_session_check_io::unlandable_capture::UnlandableCapture::recovery_command(
                    file,
                );
            anyhow::bail!(
                "route refused for {}: the open closeout can never drain, so nothing was dispatched. {} {}",
                file.display(),
                report,
                route_closeout_user_outcome_fields(
                    Some(&recovery_command),
                    RouteOwnerTurnEvidence::Absent,
                )
            );
        }
        RouteCloseoutDrainOutcome::Blocked { reason, context } => {
            // GH #228 (`#routeliveturnverdict`): session-check's live-turn
            // verdict is the one classification source. Keep it verbatim; the
            // recovery decision below is consulted only for queue-prompt and
            // head facts and never re-labels a live turn as a recovery.
            let live_owner_verdict = matches!(context, RouteCloseoutBlockContext::LiveOwnerTurn)
                .then(|| agent_doc_secret_redact::redact(&reason));
            let (decision, dispatch_decision) = classify_route_closeout_block(
                file,
                reason,
                prompt_context.is_some(),
                effects.closeout_drain_effects,
            );
            let owner_turn = if live_owner_verdict.is_some()
                || agent_doc_turn_status_io::turn_active_for_pane_for_file(file, &dispatch_pane)
            {
                RouteOwnerTurnEvidence::Live
            } else {
                RouteOwnerTurnEvidence::Absent
            };
            let blocked_recovery_command = if live_owner_verdict.is_some() {
                None
            } else {
                blocked_closeout_recovery_command(&decision).or_else(|| {
                    matches!(context, RouteCloseoutBlockContext::OpenEmptyPreflight)
                        .then(|| open_empty_preflight_recovery_command(file))
                        .flatten()
                })
            };
            // GH #228: without a prompt to queue, a live owner turn is a benign
            // yield, with or without a drainable head: never the
            // "remains queued behind the closeout" wait or a FailClosed bail.
            if let Some(verdict) = live_owner_verdict.as_deref()
                && !matches!(
                    dispatch_decision,
                    CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout
                )
            {
                let head = match &dispatch_decision {
                    CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head } => {
                        Some(head.as_str())
                    }
                    _ => None,
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_live_owner_turn file={} head={} owner_verdict={}",
                        file.display(),
                        agent_doc_secret_redact::redact(head.unwrap_or_default()),
                        verdict
                    ),
                );
                let explicit =
                    agent_doc_session_check_io::midturn_steering::explicit_send_pending_items(file)
                        .map(|items| items.len())
                        .unwrap_or(0);
                let outcome = live_owner_turn_route_outcome(file, head, verdict, explicit);
                eprintln!("{}", outcome.message());
                outcome.record(file, explicit);
                return Ok(dispatch_pane);
            }
            match dispatch_decision {
                CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout => {
                    let Some(context) = prompt_context else {
                        unreachable!("prompt-context decision requires a prompt context");
                    };
                    // #jb-run-preempt-autoloop-priority: manual reroute prompt preempts.
                    let queued = match enqueue_exchange_slash_command_for_idle_drain(
                        file,
                        context,
                        "open_closeout_blocked",
                        effects.queue_effects,
                    )? {
                        Some(queued) => queued,
                        None => enqueue_route_dispatch_prompt(
                            file,
                            &context.prompt_text,
                            "open_closeout_blocked",
                            true,
                            effects.queue_effects,
                        )?,
                    };
                    if let Some(verdict) = live_owner_verdict.as_deref() {
                        let message = format!(
                            "[route] a turn is already running on {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) for after that turn's boundary. Nothing needs recovery (owner: {}) owner_turn=live {}",
                            file.display(),
                            queued.prompt_text,
                            queued.appended,
                            queued.already_present,
                            queued.superseded,
                            verdict,
                            route_closeout_user_outcome_fields(None, owner_turn)
                        );
                        eprintln!("{message}");
                        crate::invocation::record_route_live_owner_turn(message);
                        return Ok(dispatch_pane);
                    }
                    let deferral = format!(
                        "[route] active closeout for {} could not be drained before reroute; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) {}",
                        file.display(),
                        queued.prompt_text,
                        queued.appended,
                        queued.already_present,
                        queued.superseded,
                        route_closeout_user_outcome_fields(
                            blocked_recovery_command.as_deref(),
                            owner_turn,
                        )
                    );
                    eprintln!("{deferral}");
                    crate::invocation::record_route_deferral(deferral);
                    return Ok(dispatch_pane);
                }
                CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head } => {
                    let blocker = decision.route_terminal_reason();
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_drain_closeout_wait_existing_queue file={} head={} blocker={}",
                            file.display(),
                            agent_doc_secret_redact::redact(&head),
                            agent_doc_secret_redact::redact(&blocker)
                        ),
                    );
                    let fields = route_closeout_user_outcome_fields(
                        blocked_recovery_command.as_deref(),
                        owner_turn,
                    );
                    // `#claimedsteerwake`: this route is the operator's
                    // explicit send. Pending steering goes to the owning turn
                    // instead of the route reporting "nothing was dispatched".
                    let explicit =
                        agent_doc_session_check_io::midturn_steering::explicit_send_pending_items(
                            file,
                        )
                        .map(|items| items.len())
                        .unwrap_or(0);
                    let outcome = closeout_wait_route_outcome(
                        file,
                        &head,
                        &agent_doc_secret_redact::redact(&blocker),
                        &fields,
                        explicit,
                        owner_turn,
                    );
                    eprintln!("{}", outcome.message());
                    outcome.record(file, explicit);
                    return Ok(dispatch_pane);
                }
                CloseoutBlockDispatchDecision::FailClosed => {
                    let reason = decision.route_terminal_reason();
                    anyhow::bail!(
                        "authoritative actor generation {} for {} owns pane {} but route could not drain the active closeout before dispatch; this route was NOT queued: {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        reason
                    );
                }
            }
        }
    }
    // The actor record is the generation-fenced identity of the process in the
    // owned pane. Desired/frontmatter harness selection is only a switch request;
    // it must never choose the probe syntax or prompt transport for that pane.
    let harness = actor_transport_harness(&actor.record.harness);
    let harness = &harness;
    if agent_doc_harness::normalize_harness_name(&requested_harness.binary)
        != agent_doc_harness::normalize_harness_name(&harness.binary)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_authoritative_actor_transport_authority file={} pane={} requested_harness={} actor_harness={} generation={} action=use_actor_record",
                file.display(),
                dispatch_pane,
                requested_harness.binary,
                harness.binary,
                actor.record.generation,
            ),
        );
    }
    if let Some(context) = prompt_context
        && let Some(queued) = enqueue_exchange_slash_command_for_idle_drain(
            file,
            context,
            "exchange_slash_command",
            effects.queue_effects,
        )?
    {
        eprintln!(
            "[route] unresolved exchange slash command for {} was queued as {:?} in active agent:queue (appended={}, already_present={}, superseded={}) for managed after-turn submission",
            file.display(),
            queued.prompt_text,
            queued.appended,
            queued.already_present,
            queued.superseded
        );
        return Ok(dispatch_pane);
    }
    if actor_state == agent_doc_controller::actor::ActorState::Starting
        && let Some(refreshed) = wait_for_authoritative_actor_ready(
            tmux,
            file,
            session_id,
            file_path,
            harness,
            &actor,
            (effects.wait_for_ready_override)(),
        )?
    {
        if refreshed.record.generation != actor.record.generation
            || refreshed.record.pane_id != actor.record.pane_id
            || refreshed.actor_state() != actor_state
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_refreshed_ready file={} old_pane={} new_pane={} harness={} old_generation={} new_generation={} old_state={} new_state={}",
                    file.display(),
                    actor.record.pane_id,
                    refreshed.record.pane_id,
                    harness.binary,
                    actor.record.generation,
                    refreshed.record.generation,
                    actor_state.as_str(),
                    refreshed.actor_state().as_str()
                ),
            );
        }
        actor = refreshed;
        dispatch_pane = actor.record.pane_id.clone();
        actor_state = actor.actor_state();
        if let Some(pending) = actor.pending_harness_switch.as_ref() {
            return accept_pending_harness_switch(tmux, file, &actor, pending);
        }
    }
    let has_existing_inactive_queue_fallback = if dispatch_only
        && actor_state == agent_doc_controller::actor::ActorState::Busy
        && prompt_context.is_none()
    {
        inactive_route_queue_head(file)?.is_some()
    } else {
        false
    };
    // #jb-run-agent-doc-busy-active-turn-stall: probe the live pane once for a
    // genuine active-turn busy cue (working spinner / `esc to interrupt`) before
    // direct dispatch. A stale Busy projection skips the slow ready-wait; a stale
    // Ready projection is downgraded to Busy so route cannot inject into a live
    // turn just because the durable actor record lagged behind the pane.
    let active_turn_busy_cue: Option<String> = if dispatch_only_should_probe_active_turn_cue(
        dispatch_only,
        controller_dispatch_actor_state(actor_state),
        prompt_context.is_some(),
        has_existing_inactive_queue_fallback,
    ) {
        tmux.capture_pane(&dispatch_pane, Some(80))
            .ok()
            .and_then(|content| harness.busy_proof_line(&content))
    } else {
        None
    };
    if actor_state == agent_doc_controller::actor::ActorState::Ready
        && let Some(cue) = active_turn_busy_cue.as_deref()
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_ready_actor_active_turn_blocked file={} pane={} harness={} generation={} cue={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                cue
            ),
        );
        eprintln!(
            "[route] authoritative actor for {} reported ready on pane {}, but the live pane is busy on an active {} turn ({}); treating the actor as busy before dispatch",
            file.display(),
            dispatch_pane,
            harness.binary,
            cue
        );
        actor_state = agent_doc_controller::actor::ActorState::Busy;
    }
    // `#runawayturnsurfaced`: this is the one place agent-doc has direct proof of
    // how long the live turn has been running, and it used to drop that proof on
    // the floor — it deferred dispatch and said nothing about the age. A turn that
    // never finishes therefore held the queue silently; observed at 1h51m on
    // `tasks/fpe.md`, ended only because the operator happened to look.
    //
    // Report only. agent-doc does not interrupt the turn: a long turn can be a
    // large refactor or a slow suite, and deciding whose work may run is not this
    // site's call. `0` in config disables the report.
    if let Some(cue) = active_turn_busy_cue.as_deref() {
        let threshold = agent_doc_project_config_io::load_project_for_doc(file)
            .agent_doc_runaway_turn_secs
            .unwrap_or(agent_doc_harness::DEFAULT_RUNAWAY_TURN_SECS);
        if threshold > 0
            && let Some(elapsed) = agent_doc_harness::elapsed_seconds_in_busy_cue(cue)
            && elapsed >= threshold
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_active_turn_runaway file={} pane={} harness={} generation={} elapsed_secs={} threshold_secs={} action=reported_only cue={:?}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    elapsed,
                    threshold,
                    cue
                ),
            );
            eprintln!(
                "[route] the {} turn on pane {} for {} has been running {} (over the {}s runaway threshold) and is holding further dispatch; agent-doc will not interrupt it — inspect the pane, or set `agent_doc_runaway_turn_secs` in .agent-doc/config.toml (0 disables this report)",
                harness.binary,
                dispatch_pane,
                file.display(),
                agent_doc_harness::format_turn_age(elapsed),
                threshold
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_busy_active_turn_skip_wait file={} pane={} harness={} generation={} cue={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                cue
            ),
        );
        eprintln!(
            "[route] authoritative actor for {} is busy on an active {} turn ({}); skipping the busy ready-wait and refusing immediately so the IDE shows the session-still-running notification without a {}s stall",
            file.display(),
            harness.binary,
            cue,
            dispatch_only_busy_refusal_wait_secs(
                (effects.wait_for_ready_override)(),
                dispatch_only_starting_pane_recovery_timeout_for_binary(
                    Some(harness.binary.as_str()),
                    cfg!(test),
                )
            )
        );
    }
    let mut waited_and_timed_out = false;
    if dispatch_only_busy_should_wait_for_ready(
        dispatch_only,
        controller_dispatch_actor_state(actor_state),
        prompt_context.is_some() || has_existing_inactive_queue_fallback,
        active_turn_busy_cue.is_some(),
    ) {
        if let Some(refreshed) = wait_for_authoritative_actor_ready(
            tmux,
            file,
            session_id,
            file_path,
            harness,
            &actor,
            (effects.wait_for_ready_override)(),
        )? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_busy_actor_refreshed_ready file={} old_pane={} new_pane={} harness={} old_generation={} new_generation={}",
                    file.display(),
                    actor.record.pane_id,
                    refreshed.record.pane_id,
                    harness.binary,
                    actor.record.generation,
                    refreshed.record.generation
                ),
            );
            actor = refreshed;
            dispatch_pane = actor.record.pane_id.clone();
            actor_state = actor.actor_state();
            if let Some(pending) = actor.pending_harness_switch.as_ref() {
                return accept_pending_harness_switch(tmux, file, &actor, pending);
            }
        } else {
            waited_and_timed_out = true;
        }
    }

    if actor_blocked_by_starting_timeout(StartingTimeoutActorFacts {
        actor_blocked: actor.record.state == agent_doc_controller::actor::ActorState::Blocked,
        last_transition_reason: &actor.record.last_transition.reason,
        prompt_ready: false,
    }) {
        if let Some(recovered) = recover_starting_timeout_blocked_actor_if_dispatch_ready(
            tmux, file, file_path, &actor, harness,
        ) {
            actor = recovered;
            dispatch_pane = actor.record.pane_id.clone();
            actor_state = actor.actor_state();
        } else {
            if !crate::invocation::preserve_route_layout()
                && let Err(e) = tmux.select_pane(&dispatch_pane)
            {
                eprintln!(
                    "[route] warning: failed to focus pane {}: {}",
                    dispatch_pane, e
                );
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_starting_timeout_durable_error file={} pane={} harness={} generation={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation
                ),
            );
            anyhow::bail!(
                "authoritative actor generation {} for {} owns pane {} but route will not bind a new dispatch target because this generation already timed out while starting. {}",
                actor.record.generation,
                file.display(),
                dispatch_pane,
                authoritative_actor_dispatch_recovery_hint(actor_state, file)
            );
        }
    }

    if lookup_dispatch_registration(file_path, session_id)?.as_deref()
        != Some(dispatch_pane.as_str())
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_actor_projection_reregistered file={} session={} pane={} generation={}",
                file.display(),
                session_id,
                dispatch_pane,
                actor.record.generation
            ),
        );
    }
    let rescued_from_stash = rescue_from_stash(
        tmux,
        &dispatch_pane,
        session_id,
        file_path,
        target_session,
        split_before,
    );
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;

    // After a real stash rescue the pane is now visible in the agent-doc window,
    // which can make the harness's dispatch-ready prompt observable for the first
    // time. The pre-rescue Starting wait (line ~2952) may have timed out while the
    // pane was still parked. Re-promote and, if still Starting, re-attempt the
    // ready wait once on the freshly-visible pane before bailing out.
    if rescued_from_stash && actor_state == agent_doc_controller::actor::ActorState::Starting {
        let runtime = query_supervisor_runtime(file, session_id);
        let (refreshed_record, refreshed_runtime) =
            promote_starting_authoritative_actor_if_dispatch_ready(
                tmux,
                file,
                file_path,
                actor.record.clone(),
                runtime,
                harness,
            );
        let mut refreshed = AuthoritativeActorDispatchTarget {
            record: refreshed_record,
            runtime: refreshed_runtime,
            pending_harness_switch: None,
        };
        if refreshed.actor_state() == agent_doc_controller::actor::ActorState::Ready {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_promoted_ready file={} pane={} generation={}",
                    file.display(),
                    dispatch_pane,
                    refreshed.record.generation
                ),
            );
            actor = refreshed;
            actor_state = actor.actor_state();
            dispatch_pane = actor.record.pane_id.clone();
            if let Some(pending) = actor.pending_harness_switch.as_ref() {
                return accept_pending_harness_switch(tmux, file, &actor, pending);
            }
        } else if let Some(after_wait) = wait_for_authoritative_actor_ready(
            tmux,
            file,
            session_id,
            file_path,
            harness,
            &refreshed,
            (effects.wait_for_ready_override)(),
        )? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_ready_after_wait file={} pane={} generation={}",
                    file.display(),
                    dispatch_pane,
                    after_wait.record.generation
                ),
            );
            actor = after_wait;
            actor_state = actor.actor_state();
            dispatch_pane = actor.record.pane_id.clone();
            if let Some(pending) = actor.pending_harness_switch.as_ref() {
                return accept_pending_harness_switch(tmux, file, &actor, pending);
            }
        } else {
            // Bind the unused refreshed target back so the diagnostic log captures
            // the post-rescue facts even when the wait still failed.
            refreshed.runtime = query_supervisor_runtime(file, session_id);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_authoritative_actor_post_rescue_still_starting file={} pane={} generation={} runtime_state={}",
                    file.display(),
                    dispatch_pane,
                    refreshed.record.generation,
                    refreshed.actor_state().as_str()
                ),
            );
        }
    }

    let prompt_ready = active_turn_busy_cue.is_none()
        && (actor_state == agent_doc_controller::actor::ActorState::Ready
            || current_generation_ready_prompt_proven(tmux, &actor, harness));

    // Direct pane evidence repairs a stale busy projection (#snrun). The actor
    // was projected Busy, but the live pane proves a dispatch-ready prompt in the
    // current generation — it is not actually mid-turn. Promote it to Ready so a
    // dispatch-only route dispatches to the proven-ready pane instead of queuing
    // the prompt into an active `agent:queue`. A Busy projection without a proven
    // ready prompt is left as-is and still fails closed (queues), per the
    // direct-evidence rule: idle direct evidence repairs stale busy; busy direct
    // evidence stays fail-closed.
    if busy_projection_repaired_by_ready_prompt(actor_dispatch_state(actor_state), prompt_ready) {
        eprintln!(
            "[route] authoritative actor for {} projected busy but the live pane proves a dispatch-ready prompt (generation {}); repairing stale busy projection to ready and dispatching instead of queuing",
            file.display(),
            actor.record.generation
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_authoritative_actor_busy_projection_repaired_by_ready_prompt file={} pane={} generation={} prior_state={}",
                file.display(),
                dispatch_pane,
                actor.record.generation,
                actor_state.as_str()
            ),
        );
        actor_state = agent_doc_controller::actor::ActorState::Ready;
    }

    // Timeout-idle recovery: the wait loop exhausted its budget without finding
    // a dispatch-ready prompt, but the live pane also shows no active busy cue.
    // The pane is idle by the absence-of-work test even though our prompt
    // detection patterns did not match the actual pane output. Promote the
    // stale Busy projection to Ready and dispatch. This handles Codex output
    // formats where the footer does not match `is_ignorable_output_line` or
    // `is_bottom_idle_chrome` patterns but the pane is clearly not mid-turn.
    if waited_and_timed_out
        && actor_dispatch_state(actor_state) == ActorDispatchState::Busy
        && !prompt_ready
        && let Ok(content) = tmux.capture_pane(&dispatch_pane, Some(80))
    {
        if !harness.has_busy_cue(&content) {
            eprintln!(
                "[route] timeout-idle recovery for {}: waited full timeout but pane has no busy cue; promoting stale busy projection to ready and dispatching",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_timeout_idle_recovery file={} pane={} harness={} generation={} actor_state={} busy_cue=false pane_tail={:?}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    content
                        .lines()
                        .rev()
                        .take(5)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
            actor_state = agent_doc_controller::actor::ActorState::Ready;
        } else {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_timeout_idle_recovery_blocked file={} pane={} harness={} generation={} actor_state={} busy_cue=true",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str()
                ),
            );
        }
    }

    // Eager busy-cue check for dispatch-only queue fallback: when
    // `dispatch_only_busy_should_wait_for_ready` skipped the wait because
    // a queue fallback existed (prompt_context or inactive queue head), the
    // timeout-idle recovery above never ran. The actor may be projected Busy
    // while the live pane is actually idle. Check the pane eagerly and promote
    // to Ready so the dispatch proceeds instead of queuing behind a stale
    // projection. This is the #opencode-jb-stall root cause: JB Run Agent Doc
    // sends a prompt, the wait is skipped, and the stale Busy projection queues
    // the prompt into agent:queue, which never drains because the
    // auto-loop requires the actor to become ready.
    if dispatch_only
        && actor_dispatch_state(actor_state) == ActorDispatchState::Busy
        && !waited_and_timed_out
        && prompt_context.is_some()
        && let Ok(content) = tmux.capture_pane(&dispatch_pane, Some(80))
        && !harness.has_busy_cue(&content)
        // #jbsteerinterrupt: never promote a busy projection to ready on a single
        // frame. Promotion here dispatches the trigger into the pane, and if the
        // turn is actually live that submission is what Claude Code renders as
        // "Interrupted" — the exact outcome `#realtime-steering-verbatim` forbids,
        // since the prompt is already in the document for the running turn to
        // consume as steering. A genuinely stale projection stays cue-free across
        // frames, while a live turn re-renders its spinner, so require a second
        // confirming capture. Cheap insurance against a torn or mid-redraw frame.
        && confirm_pane_still_idle(tmux, &dispatch_pane, harness)
    {
        eprintln!(
            "[route] eager busy-cue check for {}: actor projected busy but pane has no busy cue across two captures (queue fallback skipped the wait); promoting stale busy projection to ready and dispatching",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_eager_busy_cue_recovery file={} pane={} harness={} generation={} actor_state={} busy_cue=false pane_tail={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                actor_state.as_str(),
                content
                    .lines()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        );
        actor_state = agent_doc_controller::actor::ActorState::Ready;
    }

    let actor_dispatch_state = actor_dispatch_state(actor_state);
    let reopen_outcome = decide_authoritative_reopen(RoutedReopenFacts {
        actor_state: actor_dispatch_state,
        prompt_ready,
        has_prompt_bearing_work: prompt_bearing_marker.is_some(),
        mode: reopen_mode,
        degraded_authority: false,
        dispatch_eligible: supervisor_authoritative_actor_dispatch_target_eligible(&actor.runtime),
    });
    let action =
        classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
            mode: reopen_mode,
            actor_state: actor_dispatch_state,
            has_prompt_bearing_work: prompt_bearing_marker.is_some(),
            reopen_decision: reopen_outcome.decision,
            intent: dispatch_intent,
        });

    if !crate::invocation::preserve_route_layout()
        && actor_dispatch_blocker_reason(actor_dispatch_state).is_some()
        && let Err(e) = tmux.select_pane(&dispatch_pane)
    {
        eprintln!(
            "[route] warning: failed to focus pane {}: {}",
            dispatch_pane, e
        );
    }

    match action {
        AuthoritativeActorDispatchAction::FocusOnly => {
            // A plain dispatch-only reopen (IDE `Run Agent Doc`) against a busy
            // authoritative actor focuses the pane but never injects the trigger.
            // Returning Ok reports a routed run to the IDE even though nothing was
            // submitted, so the operator saw no feedback after a long wait
            // (`#jb-run-agent-doc-command-route-miss`). Fail closed with the same
            // busy-not-ready message the IDE classifies as a "session still
            // running" notification instead of silently succeeding. The pane was
            // already focused above (blocker states select the pane before this
            // match), so the operator still lands on the in-flight turn.
            if dispatch_only_focus_only_should_fail_closed(reopen_mode, actor_dispatch_state) {
                let reason = actor_dispatch_blocker_reason(actor_dispatch_state)
                    .unwrap_or("actor not ready");
                if let Some(queued) =
                    activate_existing_route_queue_head(file, reason, effects.queue_effects)?
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_busy_existing_queue_deferred file={} pane={} harness={} generation={} actor_state={} prompt={:?}",
                            file.display(),
                            dispatch_pane,
                            harness.binary,
                            actor.record.generation,
                            actor_state.as_str(),
                            queued.prompt_text
                        ),
                    );
                    eprintln!(
                        "[route] authoritative actor generation {} for {} is busy on pane {}; activated existing agent:queue head {:?} (already_present={}, activated={}) for idle drain instead of injecting a duplicate trigger {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        queued.prompt_text,
                        queued.already_present,
                        queued.activated,
                        agent_doc_flow::outcome::user_outcome_fields(
                            agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                        )
                    );
                    return Ok(dispatch_pane);
                }
                // #jb-busy-reopen-auto-drain-when-idle: there is no INACTIVE queue
                // head to activate, but the document may already have an ACTIVE
                // queue continuation (`queue: start` + ready `agent:queue`). When
                // it does, the running loop will continue this document on its own —
                // a bare dispatch-only reopen (IDE `Run Agent Doc`) has nothing to
                // add. Failing closed with the busy-not-ready error mis-reports a
                // self-driving session that IS making progress as a failure (the
                // operator clicks Run Agent Doc on an auto-looping doc, catches a
                // brief inter-iteration gap by eye, and gets an error even though the
                // loop is alive). Report deferred success so the IDE surfaces an
                // "auto-loop active, will continue" acknowledgment instead of an
                // error, mirroring the existing `*_busy_existing_queue_deferred` path.
                if let Some(continuation) =
                    detect_active_queue_continuation(file, "route_busy_focus_queue_continuation")?
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_dispatch_only_busy_active_auto_loop_deferred file={} pane={} harness={} generation={} actor_state={} head={:?}",
                            file.display(),
                            dispatch_pane,
                            harness.binary,
                            actor.record.generation,
                            actor_state.as_str(),
                            continuation.head_prompt
                        ),
                    );
                    eprintln!(
                        "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal {}",
                        actor.record.generation,
                        file.display(),
                        dispatch_pane,
                        continuation.head_prompt,
                        agent_doc_flow::outcome::user_outcome_fields(
                            agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                        )
                    );
                    return Ok(dispatch_pane);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched file={} pane={} harness={} generation={} actor_state={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        actor.record.generation,
                        actor_state.as_str()
                    ),
                );
                agent_doc_flow_io::log_flow_event(
                    file,
                    prompt_ready_barrier_failed_event(
                        RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady,
                    ),
                    agent_doc_ops_log_io::log_op,
                );
                let file_display = file.display().to_string();
                let recovery_hint = authoritative_actor_dispatch_recovery_hint(actor_state, file);
                let unblocker = if active_turn_busy_cue.is_some() {
                    "wait_for_owner_turn_to_finish"
                } else {
                    "wait_for_dispatch_ready_prompt"
                };
                let blocked_outcome =
                    agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(unblocker);
                anyhow::bail!(
                    "{}",
                    controller_dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
                        generation: actor.record.generation,
                        file_display: &file_display,
                        dispatch_pane: &dispatch_pane,
                        harness_binary: &harness.binary,
                        reason,
                        wait_secs: dispatch_only_busy_refusal_wait_secs(
                            (effects.wait_for_ready_override)(),
                            dispatch_only_starting_pane_recovery_timeout_for_binary(
                                Some(harness.binary.as_str()),
                                cfg!(test),
                            )
                        ),
                        recovery_hint: &recovery_hint,
                        active_turn_busy_cue: active_turn_busy_cue.as_deref(),
                        blocked_outcome_fields: &blocked_outcome,
                    })
                );
            }
            eprintln!(
                "[route] authoritative actor for {} remains in state {} on pane {} — focusing without injecting a duplicate reopen",
                file.display(),
                actor_state.as_str(),
                dispatch_pane
            );
            if let Some(queued) = activate_existing_route_queue_head(
                file,
                "focus_only_inactive_queue",
                effects.queue_effects,
            )? {
                eprintln!(
                    "[route] activated existing inactive agent:queue head {:?} for {} (already_present={}, activated={}) during focus-only reopen",
                    queued.prompt_text,
                    file.display(),
                    queued.already_present,
                    queued.activated
                );
            }
            Ok(dispatch_pane)
        }
        AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue => {
            let reason =
                actor_dispatch_blocker_reason(actor_dispatch_state).unwrap_or("actor not ready");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_authoritative_actor_busy_not_ready file={} pane={} harness={} generation={} actor_state={} flow_reason={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    reopen_outcome.reason
                ),
            );
            agent_doc_flow_io::log_flow_event(
                file,
                prompt_ready_barrier_failed_event(
                    RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady,
                ),
                agent_doc_ops_log_io::log_op,
            );
            if let Some(context) = prompt_context {
                // #jb-run-preempt-autoloop-priority: busy-actor Run Agent Doc preempts.
                let queued = enqueue_route_dispatch_prompt(
                    file,
                    &context.prompt_text,
                    reason,
                    true,
                    effects.queue_effects,
                )?;
                eprintln!(
                    "[route] authoritative actor generation {} for {} is busy on pane {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded,
                    agent_doc_flow::outcome::user_outcome_fields(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                Ok(dispatch_pane)
            } else if let Some(continuation) =
                detect_active_queue_continuation(file, "route_busy_queue_continuation")?
            {
                // #jb-busy-reopen-auto-drain-when-idle: a bare reopen (no prompt to
                // queue) against a busy actor whose document already has an active
                // queue continuation defers to that loop instead of erroring.
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_busy_active_auto_loop_deferred file={} pane={} harness={} generation={} actor_state={} head={:?}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        actor.record.generation,
                        actor_state.as_str(),
                        continuation.head_prompt
                    ),
                );
                eprintln!(
                    "[route] authoritative actor generation {} for {} is busy on pane {}, but its agent:queue loop is already active (next head {:?}); the running loop will continue this document — reporting deferred success instead of a busy refusal {}",
                    actor.record.generation,
                    file.display(),
                    dispatch_pane,
                    continuation.head_prompt,
                    agent_doc_flow::outcome::user_outcome_fields(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                Ok(dispatch_pane)
            } else {
                let file_display = file.display().to_string();
                let recovery_hint = authoritative_actor_dispatch_recovery_hint(actor_state, file);
                let unblocker = if active_turn_busy_cue.is_some() {
                    "wait_for_owner_turn_to_finish"
                } else {
                    "wait_for_dispatch_ready_prompt"
                };
                let blocked_outcome =
                    agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(unblocker);
                anyhow::bail!(
                    "{}",
                    controller_dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
                        generation: actor.record.generation,
                        file_display: &file_display,
                        dispatch_pane: &dispatch_pane,
                        harness_binary: &harness.binary,
                        reason,
                        wait_secs: dispatch_only_busy_refusal_wait_secs(
                            (effects.wait_for_ready_override)(),
                            dispatch_only_starting_pane_recovery_timeout_for_binary(
                                Some(harness.binary.as_str()),
                                cfg!(test),
                            )
                        ),
                        recovery_hint: &recovery_hint,
                        active_turn_busy_cue: active_turn_busy_cue.as_deref(),
                        blocked_outcome_fields: &blocked_outcome,
                    })
                )
            }
        }
        AuthoritativeActorDispatchAction::RecoverDispatchOnlyWaitingInput => {
            recover_dispatch_only_authoritative_waiting_input(
                tmux,
                file,
                session_id,
                file_path,
                target_session,
                split_before,
                harness,
                &dispatch_pane,
                actor.record.generation,
                effects.dispatch_only_route_effects,
            )
        }
        AuthoritativeActorDispatchAction::ManagedSupervisorQueue => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_actor_dispatch_optimistic_queue file={} pane={} harness={} generation={} actor_state={} flow_reason={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    reopen_outcome.reason
                ),
            );
            eprintln!(
                "[route] authoritative actor generation {} for {} still reports {} on pane {} — sending the bare {} reopen anyway so the supervisor can queue it",
                actor.record.generation,
                file.display(),
                actor_state.as_str(),
                dispatch_pane,
                harness.binary
            );
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "managed_reopen",
                &format!(
                    "submit=supervisor_ipc actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "managed_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            let dispatch_start = dispatch_via_supervisor_ipc(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                effects.route_dispatch_effects,
            )?;
            let admission_pane = require_routed_admission_projection(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                baseline,
                prompt_bearing_marker,
                true,
                dispatch_start,
                // `#jbroutasync`: the invocation guard carries the command
                // deadline from the controller boundary into every route branch.
                crate::invocation::wait_for_ready_override(),
                effects.route_admission_effects,
            )?;
            Ok(admission_pane.unwrap_or(dispatch_pane))
        }
        AuthoritativeActorDispatchAction::FailClosed => {
            let reason =
                actor_dispatch_blocker_reason(actor_dispatch_state).unwrap_or("actor not ready");
            let rescue_context = if rescued_from_stash {
                " (after a fresh stash rescue — re-promotion still did not observe a dispatch-ready prompt)"
            } else {
                ""
            };
            // #route-busy-vs-starting-wording: the default "(waited Ns for X
            // startup)" wording mis-reads a pane that is busy on an active harness
            // turn (e.g. a live Claude turn showing the working spinner / interrupt
            // hint) as a stuck cold start. Probe the live pane for a harness busy
            // cue and word the wait context as a busy active turn when present.
            // Best-effort: a capture failure falls back to the cold-start wording.
            let busy_cue = tmux
                .capture_pane(&dispatch_pane, Some(80))
                .ok()
                .and_then(|content| harness.dispatch_blocker_reason(&content));
            let wait_context = failclosed_wait_context(
                &harness.binary,
                busy_cue.as_deref(),
                dispatch_only_busy_refusal_wait_secs(
                    (effects.wait_for_ready_override)(),
                    dispatch_only_starting_pane_recovery_timeout_for_binary(
                        Some(harness.binary.as_str()),
                        cfg!(test),
                    ),
                ),
            );
            anyhow::bail!(
                "authoritative actor generation {} for {} owns pane {} but route will not inject a new trigger because {} ({}){}. {}",
                actor.record.generation,
                file.display(),
                dispatch_pane,
                reason,
                wait_context,
                rescue_context,
                authoritative_actor_dispatch_recovery_hint(actor_state, file)
            );
        }
        AuthoritativeActorDispatchAction::DispatchOnlyDirectPane => {
            let queue_prompt = if prompt_context.is_some() {
                prompt_context.map(|context| context.prompt_text.clone())
            } else {
                activate_existing_route_queue_head(
                    file,
                    "dispatch_only_inactive_queue",
                    effects.queue_effects,
                )?
                    .map(|queued| {
                        eprintln!(
                            "[route] activated existing inactive agent:queue head {:?} for {} (already_present={}, activated={}) before dispatch-only reopen",
                            queued.prompt_text,
                            file.display(),
                            queued.already_present,
                            queued.activated
                        );
                        queued.prompt_text
                    })
            };
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "dispatch_only_reopen",
                &format!(
                    "submit=direct_pane actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "dispatch_only_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            dispatch_only_send_reopen(
                tmux,
                file,
                session_id,
                &dispatch_pane,
                file_path,
                harness,
                DispatchOnlySendReopenOptions {
                    delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
                    queue_prompt_text: queue_prompt.as_deref(),
                    intent: dispatch_intent,
                    effects: effects.dispatch_only_route_effects,
                },
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_via_actor_direct_pane_submit file={} pane={} harness={} generation={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation
                ),
            );
            // `#preflightinbinary`: pane acceptance does not prove that a
            // long-lived harness process loaded newly installed admission
            // hooks. Require the same reactive admission projection as managed
            // dispatch. A missing projection restarts supported harnesses fresh
            // once so their startup hook snapshot is reloaded before retry.
            let admission_pane = require_routed_admission_projection(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                baseline,
                prompt_bearing_marker,
                true,
                RoutedDispatchStartProof::CommandAcceptedOnly,
                crate::invocation::wait_for_ready_override(),
                effects.route_admission_effects,
            )?;
            Ok(admission_pane.unwrap_or(dispatch_pane))
        }
        AuthoritativeActorDispatchAction::ManagedSupervisorIpc => {
            match authorize_controller_dispatch(
                file,
                session_id,
                file_path,
                &actor,
                "managed_reopen",
                &format!(
                    "submit=supervisor_ipc actor_state={} harness={}",
                    actor_state.as_str(),
                    harness.binary
                ),
            )? {
                RouteDispatchAuthorization::CoalescedDeduped { detail } => {
                    return Ok(route_dispatch_deduped_pane(
                        file,
                        "managed_reopen",
                        dispatch_pane.clone(),
                        &detail,
                    ));
                }
                RouteDispatchAuthorization::Authorized => {}
            }
            let dispatch_start = dispatch_via_supervisor_ipc(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                effects.route_dispatch_effects,
            )?;

            let admission_pane = require_routed_admission_projection(
                tmux,
                file,
                &dispatch_pane,
                session_id,
                file_path,
                harness,
                baseline,
                prompt_bearing_marker,
                true,
                dispatch_start,
                // `#jbroutasync`: the invocation guard carries the command
                // deadline from the controller boundary into every route branch.
                crate::invocation::wait_for_ready_override(),
                effects.route_admission_effects,
            )?;
            Ok(admission_pane.unwrap_or(dispatch_pane))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_record_selects_live_pane_transport_harness() {
        let requested = HarnessConfig::claude();
        let transport = actor_transport_harness("codex");

        assert_eq!(requested.binary, "claude");
        assert_eq!(transport.binary, "codex");
    }

    /// `#claimedsteerwake` live repro (agent-doc-bugs.md, 2026-10-04): Run
    /// Agent Doc while the coordinator's closeout was open was refused with
    /// "nothing was dispatched ... queued_behind_owner". With pending steering
    /// the explicit send is delivered to the owning turn and reported as such.
    #[test]
    fn run_agent_doc_during_an_open_closeout_delivers_pending_steering() {
        let file =
            Path::new("/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md");
        let head = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/126";
        let blocker = "closeout recovery replay_safe [open_empty_preflight]: agent-doc session cancel-turn tasks/agent-doc/agent-doc-bugs.md — first interrupt the owning harness run";
        let fields = agent_doc_controller::dispatch::route_closeout_user_outcome_fields(
            None,
            RouteOwnerTurnEvidence::Live,
        );
        assert!(
            fields.contains("ui_outcome=queued_behind_owner"),
            "{fields}"
        );

        let delivered = closeout_wait_route_outcome(
            file,
            head,
            blocker,
            &fields,
            2,
            RouteOwnerTurnEvidence::Live,
        );
        let CloseoutWaitRouteOutcome::SteeringDelivered(message) = &delivered else {
            panic!("an explicit send with pending steering is a delivery: {delivered:?}");
        };
        assert!(!message.contains("nothing was dispatched"), "{message}");
        assert!(!message.contains("could not be drained"), "{message}");
        assert!(
            message.contains(
                "delivered 2 pending steering item(s) to the owning turn (sent=explicit)"
            ),
            "{message}"
        );
        assert!(
            message.contains("steering_delivered=2 sent=explicit"),
            "{message}"
        );
        assert!(
            message.contains("No second trigger was injected"),
            "{message}"
        );

        // Nothing pending: the existing deferral is unchanged.
        let deferred = closeout_wait_route_outcome(
            file,
            head,
            blocker,
            &fields,
            0,
            RouteOwnerTurnEvidence::Live,
        );
        let CloseoutWaitRouteOutcome::Deferred(message) = &deferred else {
            panic!("{deferred:?}");
        };
        assert!(message.contains("nothing was dispatched"), "{message}");
    }

    #[test]
    fn explicit_send_without_a_live_owner_is_blocked() {
        let file = Path::new("/tmp/sample-session.md");
        let blocker = "closeout recovery replay_safe [open_empty_preflight]";
        let fields = agent_doc_controller::dispatch::route_closeout_user_outcome_fields(
            Some("agent-doc session cancel-turn /tmp/sample-session.md"),
            RouteOwnerTurnEvidence::Absent,
        );
        let outcome = closeout_wait_route_outcome(
            file,
            "do [#sample]",
            blocker,
            &fields,
            1,
            RouteOwnerTurnEvidence::Absent,
        );
        let CloseoutWaitRouteOutcome::Deferred(message) = outcome else {
            panic!("ownerless explicit send must be blocked");
        };
        assert!(
            message.contains("no live owning turn was proven"),
            "{message}"
        );
        assert!(message.contains("no second send is required"), "{message}");
        assert!(message.contains("ui_outcome_class=blocked"), "{message}");
        assert!(!message.contains("steering_delivered=1"), "{message}");
    }

    /// GH #228 live repro (2026-10-09): a route during a healthy live turn
    /// surfaced `replay_safe [open_empty_preflight]` with
    /// `recovery_command=agent-doc session cancel-turn` and claimed the head
    /// "remains queued behind the closeout", although session-check had just
    /// classified the same cycle IN PROGRESS and the turn went on to commit.
    #[test]
    fn route_during_a_live_owner_turn_reuses_the_in_progress_verdict() {
        let file = Path::new("/tmp/doc-a.md");
        let head = "Read <url>: implement <name>'s directive";
        let verdict = "[session-check] IN PROGRESS: cycle `cycle-1791582081192` remains `preflight_started` (preflight_started) while owning pane `%3` has a fresh active-turn lease";

        let outcome = live_owner_turn_route_outcome(file, Some(head), verdict, 0);
        let CloseoutWaitRouteOutcome::OwnerTurnBusy(message) = &outcome else {
            panic!("a live owner turn is a benign yield, not a deferral: {outcome:?}");
        };
        for forbidden in [
            "cancel-turn",
            "open_empty_preflight",
            "replay_safe",
            "remains queued behind the closeout",
            "nothing was dispatched",
            "blocked_with_exact_unblocker",
            "recovery_command=",
            "ui_outcome_class=blocked",
        ] {
            assert!(!message.contains(forbidden), "{forbidden}: {message}");
        }
        assert!(message.contains("a turn is already running"), "{message}");
        assert!(message.contains("cycle-1791582081192"), "{message}");
        assert!(message.contains("is in its scope"), "{message}");
        assert!(message.contains("ui_outcome_class=ok"), "{message}");
        assert!(message.contains("owner_turn=live"), "{message}");

        // No queue head (a plain trigger): still a benign yield, not FailClosed.
        let outcome = live_owner_turn_route_outcome(file, None, verdict, 0);
        assert!(
            matches!(outcome, CloseoutWaitRouteOutcome::OwnerTurnBusy(_)),
            "{outcome:?}"
        );
        assert!(!outcome.message().contains("cancel-turn"), "{outcome:?}");

        // Pending explicit steering still goes to the live owner (exit 0).
        let outcome = live_owner_turn_route_outcome(file, Some(head), verdict, 1);
        let CloseoutWaitRouteOutcome::SteeringDelivered(message) = &outcome else {
            panic!("{outcome:?}");
        };
        assert!(!message.contains("cancel-turn"), "{message}");
    }

    #[test]
    fn live_owner_turn_yield_is_recorded_as_benign_not_deferred() {
        let file = Path::new("/tmp/doc-a.md");
        let _ = crate::invocation::take_route_deferral();
        let _ = crate::invocation::take_route_live_owner_turn();
        live_owner_turn_route_outcome(file, Some("do [#a]"), "[session-check] IN PROGRESS: x", 0)
            .record(file, 0);
        assert!(
            crate::invocation::take_route_deferral().is_none(),
            "a live-turn yield must not be the exit-75 deferral"
        );
        assert!(crate::invocation::take_route_live_owner_turn().is_some());
    }

    #[test]
    fn open_empty_preflight_block_names_the_exact_recovery_command() {
        assert_eq!(
            open_empty_preflight_recovery_command(Path::new("/tmp/sample-session.md")),
            Some("agent-doc session cancel-turn /tmp/sample-session.md".to_string())
        );
    }
}
