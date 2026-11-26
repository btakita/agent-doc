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
    RouteCloseoutEntryFacts, RouteCloseoutEntryPlan, RouteExplicitSendDecision,
    RouteOwnerTurnEvidence, RoutedDispatchStartProof, RoutedReopenFacts, RoutedReopenGuardReason,
    StartingTimeoutActorFacts, actor_blocked_by_starting_timeout, actor_dispatch_blocker_reason,
    busy_projection_repaired_by_ready_prompt, classify_authoritative_actor_dispatch_action,
    classify_route_closeout_entry_plan, classify_route_explicit_send, decide_authoritative_reopen,
    dispatch_only_busy_refusal_message as controller_dispatch_only_busy_refusal_message,
    dispatch_only_busy_refusal_wait_secs, dispatch_only_busy_should_wait_for_ready,
    dispatch_only_focus_only_should_fail_closed, dispatch_only_should_probe_active_turn_cue,
    dispatch_only_starting_pane_recovery_timeout_for_binary, failclosed_wait_context,
    prompt_ready_barrier_failed_event, retained_busy_generation_deferral,
    route_closeout_user_outcome_fields,
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
    authorize_controller_dispatch, capture_active_turn_budget_abort_cue,
    current_generation_ready_prompt_proven, current_pane_ready_prompt_proven,
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
use crate::dispatch_target::bind_running_dispatch_target;
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

/// Re-capture the authoritative pane after the same torn-frame guard interval
/// and require the full empty-composer projection again. Closeout recovery
/// needs stronger evidence than stale-busy promotion: a draft or unknown idle
/// screen on the second frame must not authorize abandoning a cycle.
fn confirm_pane_still_dispatch_ready(
    tmux: &Tmux,
    actor: &AuthoritativeActorDispatchTarget,
    harness: &HarnessConfig,
) -> bool {
    std::thread::sleep(Duration::from_millis(250));
    current_pane_ready_prompt_proven(tmux, actor, harness)
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
    crate::route_focus::focus_route_pane(tmux, file, &dispatch_pane, "pending_harness_handoff");
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
    /// `#runsteer`: non-empty steering is durable for the turn already
    /// starting. Exit 0. This is not a tmux submit, so it is not
    /// `steering_delivered_to_owner`.
    SteeringEnqueued(String),
    /// `#runsteer`: the explicit send carried no steering while a turn was
    /// already starting. Exit 0. No trigger.
    RunSteerNoop(String),
    /// A `response_captured` or `write_applied` closeout is already owned by
    /// retained commit recovery. Exit 0. The route does not queue a second
    /// dispatch and does not resubmit finalize or write.
    RetainedRecoveryOwns(String),
}

impl CloseoutWaitRouteOutcome {
    pub fn message(&self) -> &str {
        match self {
            Self::SteeringDelivered(message)
            | Self::Deferred(message)
            | Self::OwnerTurnBusy(message)
            | Self::SteeringEnqueued(message)
            | Self::RunSteerNoop(message)
            | Self::RetainedRecoveryOwns(message) => message,
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
            Self::SteeringEnqueued(message) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_run_steer_enqueued file={} items={explicit_items} (#runsteer)",
                        file.display()
                    ),
                );
                crate::invocation::record_route_live_owner_turn(message);
            }
            Self::RetainedRecoveryOwns(message) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_retained_recovery_owns_closeout file={} items={explicit_items}",
                        file.display()
                    ),
                );
                crate::invocation::record_route_live_owner_turn(message);
            }
            Self::RunSteerNoop(message) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_run_steer_noop file={} items={explicit_items} (#runsteer)",
                        file.display()
                    ),
                );
                crate::invocation::record_route_live_owner_turn(message);
            }
        }
    }
}

/// Strengthen an `OpenEmptyPreflight` block when the exact authoritative pane
/// visibly proves that its harness turn is still running.
///
/// Session-check's typed `LiveOwnerTurn` verdict remains authoritative when it
/// is available. The pane cue is a one-way fallback for the admission race in
/// which the cycle is already open and the harness is visibly busy, but the
/// turn-active lease has not reached session-check yet. It may protect a live
/// turn; it never authorizes cancellation or recovery.
fn blocked_closeout_live_owner_verdict(
    context: RouteCloseoutBlockContext,
    reason: &str,
    pane: &str,
    harness: &str,
    generation: u64,
    active_turn_busy_cue: Option<&str>,
) -> Option<String> {
    if matches!(context, RouteCloseoutBlockContext::LiveOwnerTurn) {
        return Some(agent_doc_secret_redact::redact(reason));
    }
    if !matches!(context, RouteCloseoutBlockContext::OpenEmptyPreflight) {
        return None;
    }
    active_turn_busy_cue.map(|cue| {
        format!(
            "[route] ACTIVE OWNER: authoritative actor generation {generation} on pane {pane} visibly proves an active {harness} turn ({})",
            agent_doc_secret_redact::redact(cue)
        )
    })
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
            RouteCloseoutBlockContext::LiveOwnerTurn,
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

/// `#runsteer`: what Run Agent Doc does with steering.
///
/// Empty steering is a noop. Steering while a turn is running, including a
/// lease-backed turn that is still starting, is enqueued for that turn.
/// Steering while no turn is running is enqueued and that same route starts
/// the turn. A proven-idle pane does not by itself make `turn_running` false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSteerDecision {
    Noop,
    Enqueue,
    EnqueueAndStart,
}

pub fn classify_run_steer(steering_items: usize, turn_running: bool) -> RunSteerDecision {
    if steering_items == 0 {
        RunSteerDecision::Noop
    } else if turn_running {
        RunSteerDecision::Enqueue
    } else {
        RunSteerDecision::EnqueueAndStart
    }
}

fn run_steer_noop_outcome(
    file: &Path,
    pane: &str,
    owner_verdict: &str,
) -> CloseoutWaitRouteOutcome {
    let fields = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
    );
    CloseoutWaitRouteOutcome::RunSteerNoop(format!(
        "[route] Run Agent Doc for {} enqueued no steering, so it did nothing. No turn was started. owner_turn=noop pane={pane} (owner: {owner_verdict}) {fields}",
        file.display(),
    ))
}

fn run_steer_enqueued_updates_outcome(
    file: &Path,
    pane: &str,
    owner_verdict: &str,
    explicit_items: usize,
) -> CloseoutWaitRouteOutcome {
    let fields = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner,
    );
    let (update_noun, handles) = if explicit_items == 1 {
        ("update", "that turn handles it")
    } else {
        ("updates", "that turn handles them")
    };
    CloseoutWaitRouteOutcome::SteeringEnqueued(format!(
        "[route] Enqueued {explicit_items} {update_noun} as steering for the turn already starting on {}; {handles} and no second turn was started. steering_enqueued={explicit_items} owner_turn=starting pane={pane} (owner: {owner_verdict}) {fields}",
        file.display(),
    ))
}

/// A prompt queued behind a lease-backed starting turn is steering for that
/// turn. Exit 0. No second trigger.
pub fn run_steer_queued_prompt_outcome(
    file: &Path,
    owner_verdict: &str,
    prompt_text: &str,
    appended: bool,
    already_present: bool,
    superseded: bool,
) -> CloseoutWaitRouteOutcome {
    debug_assert_eq!(classify_run_steer(1, true), RunSteerDecision::Enqueue);
    let fields = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner,
    );
    CloseoutWaitRouteOutcome::SteeringEnqueued(format!(
        "[route] Queued pending dispatch {prompt_text:?} in active agent:queue as steering for the turn already starting on {} (appended={appended}, already_present={already_present}, superseded={superseded}). That turn handles it and no second turn was started. steering_enqueued=1 owner_turn=starting (owner: {owner_verdict}) {fields}",
        file.display(),
    ))
}

/// `#harnessquotaexhausted`: pure outcome for a route whose owning pane is
/// stopped on a harness usage or credit limit.
///
/// That pane reads as idle, so without this the route offers it a new turn, or
/// enqueues steering against a leftover lease (`#runsteer`). A retry cannot
/// succeed before the limit resets. Names the limit as the cause, injects
/// nothing, and never says to run it again.
pub fn quota_blocked_owner_route_outcome(
    file: &Path,
    pane: &str,
    limit: &agent_doc_harness::usage_limit::HarnessUsageLimit,
    explicit_items: usize,
    queued_prompt: Option<&str>,
) -> CloseoutWaitRouteOutcome {
    let fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
        "wait_for_usage_limit_reset_or_change_agent",
    );
    let pending = match (explicit_items, queued_prompt) {
        (_, Some(prompt)) => format!(" Queued {prompt:?} in active agent:queue; it stays there."),
        (0, None) => String::new(),
        (1, None) => " 1 update stays saved in the document.".to_string(),
        (n, None) => format!(" {n} updates stay saved in the document."),
    };
    CloseoutWaitRouteOutcome::Deferred(format!(
        "[route] {} did not start: {}.{pending} No trigger was sent to pane {pane}, so nothing is working on this request. {} steering_delivered=0 owner_turn=quota_blocked reason=harness_usage_limit {fields}",
        file.display(),
        limit.cause(),
        limit.operator_next_step(),
    ))
}

/// `#harnessquotaexhausted`: the printed refusal stopping the exact owning
/// pane, when it sits idle on one. A running turn keeps its normal handling.
/// A Grok composer border of `* limit left: 0%` is not a refusal
/// (`#routequotaoverride`). Capture failure is no evidence.
fn owner_pane_usage_limit(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> Option<agent_doc_harness::usage_limit::HarnessUsageLimit> {
    let capture = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane).ok()?;
    if harness.has_busy_cue(&capture) {
        return None;
    }
    agent_doc_harness::usage_limit::pane_usage_limit(&capture, harness)
}

/// `#runsteer`: the outcome for a route that met a lease-backed live-owner
/// verdict, given whether the same route saw the owning pane stably
/// dispatch-ready. A live lease means a turn is running, including one that
/// is still starting, so an idle pane enqueues steering or noops. It does not
/// start a second turn. A busy pane keeps [`live_owner_turn_route_outcome`].
pub fn lease_live_owner_route_outcome(
    file: &Path,
    pane: &str,
    head: Option<&str>,
    owner_verdict: &str,
    explicit_items: usize,
    owner_pane_dispatch_ready: bool,
) -> CloseoutWaitRouteOutcome {
    if owner_pane_dispatch_ready {
        // The lease is the liveness proof. The idle pane does not clear it.
        if matches!(
            classify_run_steer(explicit_items, true),
            RunSteerDecision::Noop
        ) {
            run_steer_noop_outcome(file, pane, owner_verdict)
        } else {
            run_steer_enqueued_updates_outcome(file, pane, owner_verdict, explicit_items)
        }
    } else {
        live_owner_turn_route_outcome(file, head, owner_verdict, explicit_items)
    }
}

/// Pure outcome for the `WaitForActiveQueueHead` closeout block.
pub fn closeout_wait_route_outcome(
    file: &Path,
    head: &str,
    blocker: &str,
    outcome_fields: &str,
    explicit_items: usize,
    owner_turn: RouteOwnerTurnEvidence,
    closeout_context: RouteCloseoutBlockContext,
) -> CloseoutWaitRouteOutcome {
    match classify_route_explicit_send(explicit_items, owner_turn) {
        RouteExplicitSendDecision::DeliverToLiveOwner => {
            let delivered_fields = agent_doc_flow::outcome::user_outcome_fields(
                agent_doc_flow::outcome::UserFacingOutcomeKind::SteeringDeliveredToOwner,
            );
            let update_noun = if explicit_items == 1 {
                "update"
            } else {
                "updates"
            };
            // Success. "No second trigger was injected" is the ambiguous and
            // blocked clause, where nothing is working yet. This path is
            // `ui_outcome_class=ok` and must not use it.
            CloseoutWaitRouteOutcome::SteeringDelivered(format!(
                "[route] Sent {explicit_items} {update_noun} to the agent already running for {}; no second turn was started. steering_delivered={explicit_items} sent=explicit {delivered_fields}",
                file.display(),
            ))
        }
        RouteExplicitSendDecision::BlockOwnerAbsent => {
            let automatic_recovery = match closeout_context {
                RouteCloseoutBlockContext::OpenEmptyPreflight => {
                    "The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence"
                }
                RouteCloseoutBlockContext::CapturedFinalizeRecovery => {
                    "The retained captured-finalize worker re-evaluates this durable response on controller state and delivery edges; this route already ran immediate repair and a bounded closeout wait"
                }
                RouteCloseoutBlockContext::LiveOwnerTurn | RouteCloseoutBlockContext::Other => {
                    "The closeout owner re-evaluates recovery when its durable state changes"
                }
            };
            CloseoutWaitRouteOutcome::Deferred(format!(
                "[route] explicit send for {} is blocked: {explicit_items} pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. {automatic_recovery}; no second send is required. Otherwise follow the named recovery (blocker: {blocker}). steering_delivered=0 sent=explicit {outcome_fields}",
                file.display(),
            ))
        }
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

/// Outcome when retained commit recovery already owns a captured or
/// `write_applied` closeout and the route has nothing new to enqueue.
///
/// Exit 0. A second finalize or write is not part of this route.
pub fn retained_recovery_route_outcome(
    file: &Path,
    blocker: &str,
    explicit_items: usize,
) -> CloseoutWaitRouteOutcome {
    let fields = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::NoDrainableWork,
    );
    let pending = match explicit_items {
        0 => String::new(),
        1 => " 1 pending steering item stays in the document.".to_string(),
        n => format!(" {n} pending steering items stay in the document."),
    };
    CloseoutWaitRouteOutcome::RetainedRecoveryOwns(format!(
        "[route] retained commit recovery owns the open closeout for {}. This route sent no second trigger and did not resubmit finalize or write.{pending} owner_turn=retained_recovery (blocker: {blocker}) {fields}",
        file.display(),
    ))
}

/// `#routesyncsteer`: an explicit send into any running route-owned TUI turn is
/// not delivered until that pane accepts the diff. Passive hook/poll delivery
/// remains available for document edits that were not explicitly sent.
#[derive(Debug, PartialEq, Eq)]
enum RunningTurnSteeringClaim {
    Count(usize),
    NotDelivered(String),
}

const ROUTE_SYNC_SUBMIT_ATTEMPTS: u32 = 3;
const ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT: usize = 3;
const ROUTE_SYNC_SUBMIT_SETTLE_POLLS_PER_ATTEMPT: usize = 12;
const ROUTE_SYNC_SUBMIT_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunningTurnSubmissionObservation {
    Delivered,
    Draft,
    Blocked(String),
    Unreadable,
}

/// The compact steering line is an `[agent-doc]` notice, so a busy composer
/// that still shows it projects as empty. Submit is proven only after that
/// text is gone.
fn composer_still_holds_injected_text(harness: &HarnessConfig, capture: &str) -> bool {
    let Some(candidate) = harness.last_prompt_candidate(capture) else {
        return false;
    };
    harness.is_prompt_line(&candidate) && !harness.is_dispatch_ready_prompt_line(&candidate)
}

fn running_turn_submission_observation(
    harness: &str,
    capture: &str,
) -> RunningTurnSubmissionObservation {
    let harness_cfg = HarnessConfig::from_agent_name(harness);
    let composer =
        agent_doc_harness::steering_delivery::running_turn_follow_up_composer(harness, capture);
    let injected = composer_still_holds_injected_text(&harness_cfg, capture);
    // `#groksteerdeliver`: send-now ends the busy cue, and the interrupt hint
    // can outlive the Enter that already cleared the composer. An empty box
    // is the delivery. A draft still fails closed.
    if harness_cfg.binary == "grok" && !injected {
        match &composer {
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {
                return RunningTurnSubmissionObservation::Delivered;
            }
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                reason,
            } if reason
                == agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason() =>
            {
                return RunningTurnSubmissionObservation::Delivered;
            }
            _ => {}
        }
    }
    if !harness_cfg.has_busy_cue(capture) {
        return RunningTurnSubmissionObservation::Unreadable;
    }
    match composer {
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {
            if injected {
                RunningTurnSubmissionObservation::Draft
            } else {
                RunningTurnSubmissionObservation::Delivered
            }
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft { .. } => {
            RunningTurnSubmissionObservation::Draft
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked { reason } => {
            RunningTurnSubmissionObservation::Blocked(reason)
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
            RunningTurnSubmissionObservation::Unreadable
        }
    }
}

/// Prove that the just-pasted steering left the composer while the same turn is
/// still busy. A large bracketed paste can absorb the first Enter; bounded
/// retries are safe because the pre-send gate proved the composer empty and a
/// retry is issued only while the injected draft is still visibly present.
fn confirm_running_turn_submission(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    pane: &str,
    harness: &str,
    poll_interval: Duration,
) -> Result<usize, String> {
    // `#implementlazilyowned`: each submit attempt is one Lazily-claimed attempt
    // of the steering delivery. A visible draft is `Retryable` (the composer
    // still holds the same injected text), Lazily owns whether another submit
    // is claimed, and an unreadable composer stops the drive. A retry is issued
    // only while the injected draft is still visibly present.
    let drive = agent_doc_effect_cas::drive_effect_epochs(
        &format!("route-sync-submit:{pane}"),
        "steering-draft",
        ROUTE_SYNC_SUBMIT_ATTEMPTS,
        &mut |attempt| {
            if attempt.attempt > 1 {
                agent_doc_tmux_io::send_key(tmux, pane, "Enter").map_err(|err| {
                    anyhow::anyhow!(
                        "tmux did not accept a follow-up submit retry ({})",
                        one_line_error(&err)
                    )
                })?;
            }
            let mut last_observation = RunningTurnSubmissionObservation::Unreadable;
            // A successful Codex follow-up can repaint through several frames
            // without its busy cue. Keep observing that ambiguous state without
            // claiming another Enter. A visible steering draft may authorize a
            // retry after the normal poll budget; unreadable frames never do.
            for poll in 0..ROUTE_SYNC_SUBMIT_SETTLE_POLLS_PER_ATTEMPT {
                std::thread::sleep(poll_interval);
                let capture =
                    agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane).map_err(|err| {
                        anyhow::anyhow!("the pane could not be read ({})", one_line_error(&err))
                    })?;
                last_observation = running_turn_submission_observation(harness, &capture);
                match &last_observation {
                    RunningTurnSubmissionObservation::Delivered => {
                        return Ok(agent_doc_effect_cas::EpochStep::Applied {
                            receipt: attempt.attempt as usize - 1,
                            authority: "composer-empty".to_string(),
                        });
                    }
                    RunningTurnSubmissionObservation::Blocked(reason) => {
                        return Err(anyhow::anyhow!("{reason}"));
                    }
                    RunningTurnSubmissionObservation::Draft
                        if poll + 1 >= ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT =>
                    {
                        return Ok(agent_doc_effect_cas::EpochStep::Retryable(
                            RunningTurnSubmissionObservation::Draft,
                        ));
                    }
                    RunningTurnSubmissionObservation::Draft
                    | RunningTurnSubmissionObservation::Unreadable => {}
                }
            }
            Ok(agent_doc_effect_cas::EpochStep::Retryable(last_observation))
        },
        &mut |_attempt, observation| {
            Ok(
                if matches!(observation, RunningTurnSubmissionObservation::Draft) {
                    agent_doc_effect_cas::Pace::Continue
                } else {
                    agent_doc_effect_cas::Pace::Stop
                },
            )
        },
    )
    .map_err(|err| err.to_string())?;
    match drive {
        agent_doc_effect_cas::EpochDrive::Applied(resubmits) => Ok(resubmits),
        agent_doc_effect_cas::EpochDrive::Exhausted {
            reason: RunningTurnSubmissionObservation::Draft,
            ..
        } => Err(format!(
            "the composer still holds the steering draft after {ROUTE_SYNC_SUBMIT_ATTEMPTS} submit attempts"
        )),
        agent_doc_effect_cas::EpochDrive::Exhausted { .. }
        | agent_doc_effect_cas::EpochDrive::Stopped { .. } => {
            Err("the submitted steering could not be proven in the busy composer".to_string())
        }
    }
}

fn route_sync_steering_not_delivered(file: &Path, reason: &str) -> String {
    format!(
        "[route] the latest diff since the last send was not handed to the agent already running for {}: {reason}. No second turn was started. steering_delivered=0 sent=explicit {}",
        file.display(),
        agent_doc_flow::outcome::user_outcome_fields(
            agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForOperatorProof,
        ),
    )
}

fn route_sync_operator_draft_reason(preview: &str) -> String {
    format!("the composer holds an unsent draft (redacted draft preview: {preview:?})")
}

fn one_line_error(err: &impl std::fmt::Display) -> String {
    err.to_string()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(240)
        .collect()
}

fn pending_explicit_send_count(file: &Path) -> usize {
    match agent_doc_session_check_io::midturn_steering::explicit_send_pending_items(file) {
        Ok(items) => items.len(),
        Err(err) => {
            // Only a count: nothing is advanced or recorded here, so the items
            // stay durable for the next consumer. Never swallow the failure.
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_explicit_send_items_unreadable file={} outcome=retained error={} (#routesyncsteer)",
                    file.display(),
                    one_line_error(&format!("{err:#}"))
                ),
            );
            eprintln!(
                "[route] warning: could not read the pending steering items for {}: {err:#}",
                file.display()
            );
            0
        }
    }
}

fn record_route_sync_baseline(
    file: &Path,
    prepared: &agent_doc_session_check_io::midturn_steering::RouteSyncSteering,
) {
    if let Err(err) =
        agent_doc_session_check_io::midturn_steering::record_route_sync_steering_delivered(
            file,
            &prepared.cycle_id,
            &prepared.content,
        )
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_sync_steering_baseline_record_failed file={} error={} (#routesyncsteer)",
                file.display(),
                one_line_error(&err)
            ),
        );
    }
}

fn normalized_composer_stash(text: &str) -> String {
    agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(text)
        .trim()
        .to_string()
}

fn same_unsent_draft(
    harness: &str,
    capture: &str,
    cursor_y: Option<usize>,
    stash: &str,
) -> Option<bool> {
    let text = agent_doc_harness::restorable_composer_draft_text(harness, capture, cursor_y)?;
    Some(normalized_composer_stash(&text) == stash)
}

fn read_follow_up_composer(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    pane: &str,
    harness: &str,
) -> Result<
    (
        String,
        Option<usize>,
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer,
    ),
    String,
> {
    let capture = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane)
        .map_err(|err| format!("the pane could not be read ({})", one_line_error(&err)))?;
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(tmux, pane);
    let composer = agent_doc_harness::steering_delivery::running_turn_follow_up_composer_at_cursor(
        harness, &capture, cursor_y,
    );
    Ok((capture, cursor_y, composer))
}

/// `C-u` once, then require an empty composer before any steering paste.
///
/// A no-op clear sends no further keys. A partial clear is put back with
/// another `C-u` and a paste that does not submit, and still fails closed so
/// the steering is not concatenated onto whatever remains.
fn move_unsent_draft_aside(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    pane: &str,
    harness: &str,
    stash: &str,
    preview: &str,
    poll_interval: Duration,
) -> Result<(), String> {
    agent_doc_tmux_io::send_key(tmux, pane, "C-u").map_err(|err| {
        format!(
            "tmux did not accept moving the unsent draft aside ({})",
            one_line_error(&err)
        )
    })?;
    let mut saw_same = false;
    for _ in 0..ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT {
        std::thread::sleep(poll_interval);
        let (capture, cursor_y, composer) = read_follow_up_composer(tmux, pane, harness)?;
        if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
            return Err(
                agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason().to_string(),
            );
        }
        match composer {
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {
                return Ok(());
            }
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                reason,
            } => return Err(reason),
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {}
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft { .. } => {
                match same_unsent_draft(harness, &capture, cursor_y, stash) {
                    Some(true) => saw_same = true,
                    Some(false) => {
                        return Err(put_changed_draft_back(
                            tmux,
                            pane,
                            harness,
                            stash,
                            preview,
                            poll_interval,
                        ));
                    }
                    None => {}
                }
            }
        }
    }
    if saw_same {
        return Err(format!(
            "the composer still holds an unsent draft and it could not be moved aside without submitting it (redacted draft preview: {preview:?})"
        ));
    }
    Err("the composer could not be read after moving the unsent draft aside".to_string())
}

fn put_changed_draft_back(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    pane: &str,
    harness: &str,
    stash: &str,
    preview: &str,
    poll_interval: Duration,
) -> String {
    let cleared =
        (|| {
            agent_doc_tmux_io::send_key(tmux, pane, "C-u").map_err(|err| one_line_error(&err))?;
            for _ in 0..ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT {
                std::thread::sleep(poll_interval);
                let (capture, _, composer) = read_follow_up_composer(tmux, pane, harness)?;
                if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
                    return Err(
                        agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason()
                            .to_string(),
                    );
                }
                match composer {
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {
                    return Ok(());
                }
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                    reason,
                } => return Err(reason),
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft { .. }
                | agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
                }
            }
            }
            Err("the composer did not become empty".to_string())
        })();
    match cleared {
        Ok(()) => {
            let _ = agent_doc_tmux_io::paste_bracketed_text(tmux, pane, stash);
            format!(
                "the unsent draft changed while it was moved aside, so the steering was not pasted and the draft was put back without submitting it (redacted draft preview: {preview:?})"
            )
        }
        Err(reason)
            if reason == agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason() =>
        {
            reason
        }
        Err(_) => format!(
            "the unsent draft changed while it was moved aside and could not be put back without submitting it (redacted draft preview: {preview:?})"
        ),
    }
}

fn log_draft_restore_failed(file: &Path, pane: &str, harness: &str, preview: &str, error: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_sync_steering_draft_restore_failed file={} pane={pane} harness={harness} draft_preview={preview:?} error={error} (#routesyncsteer)",
            file.display(),
        ),
    );
}

fn restore_unsent_draft(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane: &str,
    harness: &str,
    stash: &str,
    preview: &str,
    poll_interval: Duration,
) {
    if let Err(err) = agent_doc_tmux_io::paste_bracketed_text(tmux, pane, stash) {
        log_draft_restore_failed(file, pane, harness, preview, &one_line_error(&err));
        return;
    }
    for _ in 0..ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT {
        std::thread::sleep(poll_interval);
        let Ok((capture, cursor_y, composer)) = read_follow_up_composer(tmux, pane, harness) else {
            log_draft_restore_failed(file, pane, harness, preview, "the pane could not be read");
            return;
        };
        if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
            log_draft_restore_failed(
                file,
                pane,
                harness,
                preview,
                agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason(),
            );
            return;
        }
        if same_unsent_draft(harness, &capture, cursor_y, stash) == Some(true) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_sync_steering_draft_restored file={} pane={pane} harness={harness} draft_preview={preview:?} unsent=true (#routesyncsteer)",
                    file.display(),
                ),
            );
            return;
        }
        if let agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
            reason,
        } = composer
        {
            log_draft_restore_failed(file, pane, harness, preview, &reason);
            return;
        }
    }
    log_draft_restore_failed(
        file,
        pane,
        harness,
        preview,
        "the unsent draft was not back in the composer",
    );
}

/// After a proven-failed submit, put the stashed draft back without Enter.
/// A blocked or unreadable composer is not typed into.
fn restore_unsent_draft_after_rejected_submit(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane: &str,
    harness: &str,
    stash: &str,
    preview: &str,
    poll_interval: Duration,
) {
    let Ok((capture, _, composer)) = read_follow_up_composer(tmux, pane, harness) else {
        log_draft_restore_failed(file, pane, harness, preview, "the pane could not be read");
        return;
    };
    if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
        log_draft_restore_failed(
            file,
            pane,
            harness,
            preview,
            agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason(),
        );
        return;
    }
    let injected_text_remains =
        composer_still_holds_injected_text(&HarnessConfig::from_agent_name(harness), &capture);
    match composer {
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked { reason } => {
            log_draft_restore_failed(file, pane, harness, preview, &reason);
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
            log_draft_restore_failed(
                file,
                pane,
                harness,
                preview,
                "the composer could not be read",
            );
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty
            if !injected_text_remains =>
        {
            restore_unsent_draft(tmux, file, pane, harness, stash, preview, poll_interval);
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty
        | agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft { .. } => {
            if let Err(err) = agent_doc_tmux_io::send_key(tmux, pane, "C-u") {
                log_draft_restore_failed(file, pane, harness, preview, &one_line_error(&err));
                return;
            }
            for _ in 0..ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT {
                std::thread::sleep(poll_interval);
                let Ok((capture, _, composer)) = read_follow_up_composer(tmux, pane, harness)
                else {
                    log_draft_restore_failed(
                        file,
                        pane,
                        harness,
                        preview,
                        "the pane could not be read",
                    );
                    return;
                };
                if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
                    log_draft_restore_failed(
                        file,
                        pane,
                        harness,
                        preview,
                        agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason(),
                    );
                    return;
                }
                let injected_text_remains = composer_still_holds_injected_text(
                    &HarnessConfig::from_agent_name(harness),
                    &capture,
                );
                match composer {
                    agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty
                        if !injected_text_remains =>
                    {
                        restore_unsent_draft(
                            tmux,
                            file,
                            pane,
                            harness,
                            stash,
                            preview,
                            poll_interval,
                        );
                        return;
                    }
                    agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                        reason,
                    } => {
                        log_draft_restore_failed(file, pane, harness, preview, &reason);
                        return;
                    }
                    agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty
                    | agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft { .. }
                    | agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {}
                }
            }
            log_draft_restore_failed(
                file,
                pane,
                harness,
                preview,
                "the steering draft could not be moved aside before restoring the unsent draft",
            );
        }
    }
}

fn retain_unreadable_operator_draft(
    file: &Path,
    pane: &str,
    harness: &str,
    preview: &str,
) -> RunningTurnSteeringClaim {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_sync_steering_not_delivered file={} pane={pane} harness={harness} reason=operator_draft draft_preview={preview:?} outcome=retained (#routesyncsteer)",
            file.display(),
        ),
    );
    RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
        file,
        &route_sync_operator_draft_reason(preview),
    ))
}

#[allow(clippy::too_many_arguments)]
fn submit_prepared_running_turn_steering(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane: &str,
    harness: &str,
    harness_cfg: &HarnessConfig,
    prepared: &agent_doc_session_check_io::midturn_steering::RouteSyncSteering,
    poll_interval: Duration,
    send_now: bool,
) -> RunningTurnSteeringClaim {
    if let Err(err) = agent_doc_tmux_io::paste_bracketed_text_and_submit(
        tmux,
        pane,
        &prepared.instructions,
        Duration::from_millis(80),
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_sync_steering_not_delivered file={} pane={pane} harness={harness} error={} (#routesyncsteer)",
                file.display(),
                one_line_error(&err)
            ),
        );
        return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
            file,
            &format!(
                "tmux did not accept the follow-up ({})",
                one_line_error(&err)
            ),
        ));
    }
    let submit_retries = match confirm_running_turn_submission(tmux, pane, harness, poll_interval) {
        Ok(retries) => retries,
        Err(reason) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_sync_steering_not_delivered file={} pane={pane} harness={harness} reason={} (#routesyncsteer)",
                    file.display(),
                    reason
                ),
            );
            return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
                file, &reason,
            ));
        }
    };
    // `#groksteerdeliver`: plain Enter only queues while Grok is generating.
    // A second Enter on the emptied composer is send-now (Ctrl+Enter does not
    // survive tmux). The interrupt hint already ran the first Enter, so it
    // does not get this one. Pool refills never reach here.
    if send_now && let Err(err) = agent_doc_tmux_io::send_key(tmux, pane, "Enter") {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_sync_steering_not_delivered file={} pane={pane} harness={harness} error={} (#groksteerdeliver)",
                file.display(),
                one_line_error(&err)
            ),
        );
        return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
            file,
            &format!("tmux did not accept send-now ({})", one_line_error(&err)),
        ));
    }
    if let Err(err) = agent_doc_controller_io::live_turn_follow_up::mark_live_turn_follow_up_queued(
        file,
        &prepared.cycle_id,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_turn_follow_up_mark_failed file={} cycle={} error={} (#steercoalesce)",
                file.display(),
                prepared.cycle_id,
                one_line_error(&err)
            ),
        );
    }
    record_route_sync_baseline(file, prepared);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_sync_steering_delivered file={} pane={pane} harness={} items={} bytes={} submit_retries={} (#routesyncsteer)",
            file.display(),
            harness_cfg.binary,
            prepared.pending_items,
            prepared.instructions.len(),
            submit_retries
        ),
    );
    RunningTurnSteeringClaim::Count(prepared.pending_items.max(1))
}

/// Count, and for a busy route-owned composer submit, the explicit send.
///
/// `submit` is false when the caller has already proved the pane idle: typing
/// there would start a turn instead of steering the one that is running.
fn running_turn_steering_claim(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane: &str,
    harness: &str,
    submit: bool,
) -> RunningTurnSteeringClaim {
    running_turn_steering_claim_polled(
        tmux,
        file,
        pane,
        harness,
        submit,
        ROUTE_SYNC_SUBMIT_POLL_INTERVAL,
    )
}

fn running_turn_steering_claim_polled(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane: &str,
    harness: &str,
    submit: bool,
    poll_interval: Duration,
) -> RunningTurnSteeringClaim {
    if !submit
        || !agent_doc_harness::steering_delivery::explicit_running_turn_send_is_composer_diff(
            harness,
        )
    {
        return RunningTurnSteeringClaim::Count(pending_explicit_send_count(file));
    }
    let harness_cfg = HarnessConfig::from_agent_name(harness);
    let capture = match agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane) {
        Ok(capture) => capture,
        Err(err) => {
            return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
                file,
                &format!("the pane could not be read ({})", one_line_error(&err)),
            ));
        }
    };
    if !harness_cfg.has_busy_cue(&capture) {
        return RunningTurnSteeringClaim::Count(pending_explicit_send_count(file));
    }
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(tmux, pane);
    // A readable unrelated draft is moved aside and put back unsent. A draft
    // whose body cannot be read still fails closed, before any key.
    let stashed =
        match agent_doc_harness::steering_delivery::running_turn_follow_up_composer_at_cursor(
            harness, &capture, cursor_y,
        ) {
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft {
                preview,
            } => {
                // Draft wins over EnterCancels in the classifier. Grok's blocked
                // wait still must not see C-u or Enter.
                if agent_doc_harness::grok_enter_cancels_blocked_wait(&capture) {
                    return RunningTurnSteeringClaim::NotDelivered(
                        route_sync_steering_not_delivered(
                            file,
                            agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason(),
                        ),
                    );
                }
                let Some(draft) =
                    agent_doc_harness::restorable_composer_draft_text(harness, &capture, cursor_y)
                else {
                    return retain_unreadable_operator_draft(file, pane, harness, &preview);
                };
                let stash = normalized_composer_stash(&draft);
                if stash.is_empty() {
                    return retain_unreadable_operator_draft(file, pane, harness, &preview);
                }
                Some((stash, preview))
            }
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                reason,
            } => {
                // `#groksteerdeliver`: an empty composer on the interrupt hint
                // takes one Enter. Grok runs that message now. Any other block
                // still sends nothing. A draft returned above and is not
                // submitted. Pool refills do not use this path.
                if reason != agent_doc_harness::steering_delivery::grok_blocked_wait_enter_reason()
                {
                    return RunningTurnSteeringClaim::NotDelivered(
                        route_sync_steering_not_delivered(file, &reason),
                    );
                }
                None
            }
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
                return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
                    file,
                    "the composer could not be read",
                ));
            }
            agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => None,
        };
    let prepared =
        match agent_doc_session_check_io::midturn_steering::prepare_route_sync_steering(file) {
            Ok(prepared) => prepared,
            Err(err) => {
                return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
                    file,
                    &format!(
                        "the diff since the last send could not be read ({})",
                        one_line_error(&err)
                    ),
                ));
            }
        };
    let Some(prepared) = prepared else {
        return RunningTurnSteeringClaim::Count(0);
    };
    // A pool refill or an earlier steering paste already queued this turn's
    // one reference. Folding the diff into that payload is the delivery, and
    // the composer — including an unsent draft — is left untouched.
    if !prepared.needs_queue_paste {
        record_route_sync_baseline(file, &prepared);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_sync_steering_coalesced file={} pane={pane} harness={} items={} bytes={} cycle={} (#steercoalesce)",
                file.display(),
                harness_cfg.binary,
                prepared.pending_items,
                prepared.instructions.len(),
                prepared.cycle_id,
            ),
        );
        return RunningTurnSteeringClaim::Count(prepared.pending_items.max(1));
    }
    if let Some((stash, preview)) = stashed.as_ref() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_sync_steering_draft_stashed file={} pane={pane} harness={harness} draft_preview={preview:?} outcome=stashed (#routesyncsteer)",
                file.display(),
            ),
        );
        if let Err(reason) =
            move_unsent_draft_aside(tmux, pane, harness, stash, preview, poll_interval)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_sync_steering_not_delivered file={} pane={pane} harness={harness} reason={} (#routesyncsteer)",
                    file.display(),
                    reason
                ),
            );
            return RunningTurnSteeringClaim::NotDelivered(route_sync_steering_not_delivered(
                file, &reason,
            ));
        }
    }
    let send_now = harness_cfg.binary == "grok"
        && !agent_doc_harness::grok_enter_cancels_blocked_wait(&capture);
    match submit_prepared_running_turn_steering(
        tmux,
        file,
        pane,
        harness,
        &harness_cfg,
        &prepared,
        poll_interval,
        send_now,
    ) {
        RunningTurnSteeringClaim::Count(count) => {
            if let Some((stash, preview)) = stashed.as_ref() {
                restore_unsent_draft(tmux, file, pane, harness, stash, preview, poll_interval);
            }
            RunningTurnSteeringClaim::Count(count)
        }
        RunningTurnSteeringClaim::NotDelivered(message) => {
            if let Some((stash, preview)) = stashed.as_ref() {
                restore_unsent_draft_after_rejected_submit(
                    tmux,
                    file,
                    pane,
                    harness,
                    stash,
                    preview,
                    poll_interval,
                );
            }
            RunningTurnSteeringClaim::NotDelivered(message)
        }
    }
}

/// `#steercoalesce`: the controller accepted this Run Agent Doc as steering
/// for the turn already running. Deliver the pending diff once. A coalesced
/// reopen does not paste again once that delivery is proven. It still submits
/// when the pending request never reached the composer (`#groksteerdeliver`):
/// skipping that call reported `steering_delivered=0` for a fold the live
/// turn cannot see.
pub fn finish_operator_live_turn_steering(
    tmux: &Tmux,
    file: &Path,
    dispatch_pane: &str,
    harness: &HarnessConfig,
    generation: u64,
    coalesced: bool,
) -> Result<String> {
    let Some(delivered) = claimed_running_turn_steering(
        tmux,
        file,
        dispatch_pane,
        &harness.binary,
        true,
        "route_live_turn_steering_target_not_visible",
    ) else {
        return Ok(dispatch_pane.to_string());
    };
    let fields = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::SteeringDeliveredToOwner,
    );
    let verb = if coalesced {
        "coalesced into the steering already pending for"
    } else {
        "accepted as steering for"
    };
    let message = format!(
        "[route] Run Agent Doc for {} was {verb} the turn already running; no second trigger was sent. steering_delivered={delivered} sent=explicit {fields}",
        file.display(),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_operator_live_turn_steering file={} pane={dispatch_pane} harness={} generation={generation} coalesced={coalesced} steering_delivered={delivered} (#steercoalesce)",
            file.display(),
            harness.binary,
        ),
    );
    eprintln!("{message}");
    crate::invocation::record_route_steering_delivery(message);
    crate::invocation::report_pane_if_not_visible(
        tmux,
        file,
        dispatch_pane,
        "route_live_turn_steering_target_not_visible",
    );
    Ok(dispatch_pane.to_string())
}

/// `None` when the diff was not handed to the running turn. The caller records
/// the deferral and must not also report `steering_delivered_to_owner`.
fn claimed_running_turn_steering(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &str,
    submit: bool,
    visibility_reason: &str,
) -> Option<usize> {
    match running_turn_steering_claim(tmux, file, pane, harness, submit) {
        RunningTurnSteeringClaim::Count(count) => Some(count),
        RunningTurnSteeringClaim::NotDelivered(message) => {
            eprintln!("{message}");
            crate::invocation::record_route_deferral(message);
            crate::invocation::report_pane_if_not_visible(tmux, file, pane, visibility_reason);
            None
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
    // GH #319: the plain-trigger busy pass-through is operator
    // steering. A controller-originated (automated) route must defer instead.
    let dispatch_origin = crate::invocation::dispatch_origin();
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
    let closeout_harness = actor_transport_harness(&actor.record.harness);

    // `#harnessquotaexhausted`: before any closeout, lease, or readiness
    // reasoning. A pane out of usage reads as idle, so each of those would
    // conclude something false about it. The harness-switch handoff above runs
    // first because it is the remedy this refusal names.
    if let Some(limit) = owner_pane_usage_limit(tmux, &actor.record.pane_id, &closeout_harness) {
        let queued = match prompt_context {
            Some(context) => Some(enqueue_route_dispatch_prompt(
                file,
                &context.prompt_text,
                "harness_usage_limit",
                true,
                effects.queue_effects,
            )?),
            None => None,
        };
        let explicit = pending_explicit_send_count(file);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_owner_harness_usage_limit file={} pane={} harness={} limit={:?} source={:?} explicit_items={explicit} queued={} (#harnessquotaexhausted)",
                file.display(),
                actor.record.pane_id,
                actor.record.harness,
                limit.limit,
                limit.source,
                queued.is_some(),
            ),
        );
        let outcome = quota_blocked_owner_route_outcome(
            file,
            &actor.record.pane_id,
            &limit,
            explicit,
            queued.as_ref().map(|queued| queued.prompt_text.as_str()),
        );
        eprintln!("{}", outcome.message());
        outcome.record(file, explicit);
        return Ok(actor.record.pane_id.clone());
    }

    // `#persistingsubmitfast`: Run Agent Doc is an explicit send. When the
    // matching actor is busy in an already-open cycle, inspect the live pane
    // before starting closeout recovery. A real harness busy cue proves that
    // the current turn can accept the document diff as a follow-up, so waiting
    // up to 30 seconds for the same turn's closeout would only delay delivery.
    // No cue means no bypass: idle, torn, protected, automated, and ambiguous
    // shapes keep the existing drain/recovery boundary.
    if classify_route_closeout_entry_plan(RouteCloseoutEntryFacts {
        mode: reopen_mode,
        intent: dispatch_intent,
        origin: dispatch_origin,
        actor_state: actor_dispatch_state(actor_state),
        open_cycle: baseline.is_some_and(|state| state.is_open()),
    }) == RouteCloseoutEntryPlan::ProbeLiveTurnForExplicitSend
        && let Ok(content) = tmux.capture_pane(&dispatch_pane, Some(80))
        && let Some(cue) = closeout_harness.busy_proof_line(&content)
    {
        let owner_verdict = format!(
            "[route] ACTIVE OWNER: authoritative actor generation {} on pane {} visibly proves an active {} turn ({})",
            actor.record.generation,
            dispatch_pane,
            closeout_harness.binary,
            agent_doc_secret_redact::redact(&cue),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_explicit_send_before_closeout_drain file={} pane={} harness={} generation={} phase={} action=deliver_live_turn_steering (#persistingsubmitfast)",
                file.display(),
                dispatch_pane,
                closeout_harness.binary,
                actor.record.generation,
                baseline
                    .map(|state| state.phase.as_str())
                    .unwrap_or("unknown"),
            ),
        );
        let Some(explicit) = claimed_running_turn_steering(
            tmux,
            file,
            &dispatch_pane,
            &actor.record.harness,
            true,
            "route_predrain_live_owner_target_not_visible",
        ) else {
            return Ok(dispatch_pane);
        };
        let outcome = live_owner_turn_route_outcome(file, None, &owner_verdict, explicit);
        eprintln!("{}", outcome.message());
        outcome.record(file, explicit);
        crate::invocation::report_pane_if_not_visible(
            tmux,
            file,
            &dispatch_pane,
            "route_predrain_live_owner_target_not_visible",
        );
        return Ok(dispatch_pane);
    }

    // `#runsteer`: what the drain's two-capture pane probe saw, if it ran.
    // A proven-idle pane under a live lease is a turn that is still starting.
    let stable_dispatch_ready_observed: std::cell::Cell<Option<bool>> = std::cell::Cell::new(None);
    let closeout_drain = agent_doc_tmux_io::with_current_pane_id_override(&dispatch_pane, || {
        apply_routed_dispatch_closeout_policy(
            file,
            reopen_mode,
            dispatch_intent,
            effects.closeout_drain_effects,
            || {
                // Grok's passive lifecycle cannot always publish a turn-end
                // receipt after a manual interrupt + `/clear`. Require current
                // structural readiness across two captures before offering
                // pane state as closeout recovery authority.
                let ready = current_pane_ready_prompt_proven(tmux, &actor, &closeout_harness)
                    && confirm_pane_still_dispatch_ready(tmux, &actor, &closeout_harness);
                stable_dispatch_ready_observed.set(Some(ready));
                ready
            },
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
            // verdict is the primary classification source. Keep it verbatim;
            // the recovery decision below is consulted only for queue-prompt
            // and head facts and never re-labels a live turn as a recovery.
            // A freshly admitted turn can open its preflight before its
            // turn-active lease becomes visible to session-check. The exact
            // authoritative pane's harness-specific busy cue closes that race:
            // protect the running turn and deliver explicit steering to it,
            // never surface `session cancel-turn` for the same live work.
            let active_owner_busy_cue = if matches!(
                context,
                RouteCloseoutBlockContext::OpenEmptyPreflight
            ) {
                match tmux.capture_pane(&dispatch_pane, Some(80)) {
                    Ok(content) => closeout_harness.busy_proof_line(&content),
                    Err(error) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_closeout_busy_owner_probe_failed file={} pane={} harness={} generation={} error={}",
                                file.display(),
                                dispatch_pane,
                                closeout_harness.binary,
                                actor.record.generation,
                                agent_doc_secret_redact::redact(&error.to_string()),
                            ),
                        );
                        None
                    }
                }
            } else {
                None
            };
            let live_owner_verdict = blocked_closeout_live_owner_verdict(
                context,
                &reason,
                &dispatch_pane,
                &closeout_harness.binary,
                actor.record.generation,
                active_owner_busy_cue.as_deref(),
            );
            if active_owner_busy_cue.is_some()
                && !matches!(context, RouteCloseoutBlockContext::LiveOwnerTurn)
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_drain_closeout_busy_owner_strengthened file={} pane={} harness={} generation={} original_context={} action=protect_live_turn",
                        file.display(),
                        dispatch_pane,
                        closeout_harness.binary,
                        actor.record.generation,
                        crate::closeout_drain::route_closeout_block_context_label(context),
                    ),
                );
            }
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
            // `#runsteer`: a session-check live-owner verdict means a turn is
            // running, including one that is still starting. An idle composer
            // enqueues steering or noops; it does not start a second turn or
            // advise cancel-turn. A busy frame the drain already saw is
            // reused; an idle one is re-proven now, because the drain's
            // projection wait (up to 30s) sits between that capture and this
            // verdict. The admission-race fallback already holds a positive
            // busy frame, so this proof cannot downgrade it to idle.
            let observe_owner_pane_idle = || {
                stable_dispatch_ready_observed.get() != Some(false)
                    && agent_doc_tmux_io::with_current_pane_id_override(&dispatch_pane, || {
                        current_pane_ready_prompt_proven(tmux, &actor, &closeout_harness)
                            && confirm_pane_still_dispatch_ready(tmux, &actor, &closeout_harness)
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
                let pane_idle = observe_owner_pane_idle();
                let Some(explicit) = claimed_running_turn_steering(
                    tmux,
                    file,
                    &dispatch_pane,
                    &actor.record.harness,
                    !pane_idle,
                    "route_live_owner_target_not_visible",
                ) else {
                    return Ok(dispatch_pane);
                };
                if pane_idle {
                    let decision = if explicit == 0 { "noop" } else { "enqueue" };
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "route_run_steer file={} pane={} harness={} decision={decision} explicit_items={explicit} lease=live pane=dispatch_ready turn_running=true (#runsteer)",
                            file.display(),
                            dispatch_pane,
                            actor.record.harness,
                        ),
                    );
                }
                let outcome = lease_live_owner_route_outcome(
                    file,
                    &dispatch_pane,
                    head,
                    verdict,
                    explicit,
                    pane_idle,
                );
                eprintln!("{}", outcome.message());
                outcome.record(file, explicit);
                // A live owner can be doing the right work in an off-screen
                // pane. The explicit-send result still succeeds, but the
                // editor must not mistake transport delivery for visibility.
                crate::invocation::report_pane_if_not_visible(
                    tmux,
                    file,
                    &dispatch_pane,
                    "route_live_owner_target_not_visible",
                );
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
                    if let Some(verdict) = live_owner_verdict.as_deref()
                        && observe_owner_pane_idle()
                    {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_run_steer file={} pane={} harness={} decision=enqueue steering_items=1 explicit_items=0 queued=true lease=live pane=dispatch_ready turn_running=true (#runsteer)",
                                file.display(),
                                dispatch_pane,
                                actor.record.harness,
                            ),
                        );
                        let outcome = run_steer_queued_prompt_outcome(
                            file,
                            verdict,
                            &queued.prompt_text,
                            queued.appended,
                            queued.already_present,
                            queued.superseded,
                        );
                        eprintln!("{}", outcome.message());
                        outcome.record(file, 1);
                        crate::invocation::report_pane_if_not_visible(
                            tmux,
                            file,
                            &dispatch_pane,
                            "route_live_owner_target_not_visible",
                        );
                        return Ok(dispatch_pane);
                    }
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
                        crate::invocation::report_pane_if_not_visible(
                            tmux,
                            file,
                            &dispatch_pane,
                            "route_live_owner_target_not_visible",
                        );
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
                    let Some(explicit) = claimed_running_turn_steering(
                        tmux,
                        file,
                        &dispatch_pane,
                        &actor.record.harness,
                        matches!(owner_turn, RouteOwnerTurnEvidence::Live),
                        "route_closeout_wait_target_not_visible",
                    ) else {
                        return Ok(dispatch_pane);
                    };
                    let outcome = closeout_wait_route_outcome(
                        file,
                        &head,
                        &agent_doc_secret_redact::redact(&blocker),
                        &fields,
                        explicit,
                        owner_turn,
                        context,
                    );
                    eprintln!("{}", outcome.message());
                    outcome.record(file, explicit);
                    crate::invocation::report_pane_if_not_visible(
                        tmux,
                        file,
                        &dispatch_pane,
                        "route_closeout_wait_target_not_visible",
                    );
                    return Ok(dispatch_pane);
                }
                CloseoutBlockDispatchDecision::FailClosed => {
                    let reason = decision.route_terminal_reason();
                    if agent_doc_controller::dispatch::retained_recovery_owns_failed_closeout_block(
                        context,
                        &CloseoutBlockDispatchDecision::FailClosed,
                    ) {
                        let Some(explicit) = claimed_running_turn_steering(
                            tmux,
                            file,
                            &dispatch_pane,
                            &actor.record.harness,
                            matches!(owner_turn, RouteOwnerTurnEvidence::Live),
                            "route_retained_recovery_target_not_visible",
                        ) else {
                            return Ok(dispatch_pane);
                        };
                        let outcome = retained_recovery_route_outcome(file, &reason, explicit);
                        eprintln!("{}", outcome.message());
                        outcome.record(file, explicit);
                        crate::invocation::report_pane_if_not_visible(
                            tmux,
                            file,
                            &dispatch_pane,
                            "route_retained_recovery_target_not_visible",
                        );
                        return Ok(dispatch_pane);
                    }
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
                "route_dispatch_only_busy_active_turn_skip_wait file={} pane={} harness={} generation={} origin={} cue={:?}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                dispatch_origin.as_str(),
                cue
            ),
        );
        eprintln!(
            "[route] authoritative actor for {} is busy on an active {} turn ({}); skipping the busy ready-wait (origin {}) instead of a {}s stall",
            file.display(),
            harness.binary,
            cue,
            dispatch_origin.as_str(),
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
            crate::route_focus::focus_route_pane(
                tmux,
                file,
                &dispatch_pane,
                "starting_timeout_blocked",
            );
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
    let rescued_from_stash = if crate::invocation::preserve_route_layout() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_authoritative_background_stash_rescue_skipped file={} pane={} focus_effect=none pane_effect=none",
                file.display(),
                dispatch_pane,
            ),
        );
        false
    } else {
        rescue_from_stash(
            tmux,
            &dispatch_pane,
            session_id,
            file_path,
            target_session,
            split_before,
        )
    };
    // `#predispatchreadyoverwrite`: the actor owns this pane and is running, so
    // the bind refreshes the registry without re-projecting the record as
    // `Ready`. This route's admission reads `actor_state` (supervisor runtime
    // over the record) above, and the controller's `authorize_dispatch` below
    // reads the durable record: a `Ready` written here would wave a live `Busy`
    // turn through the `admin queue drain` gate (`actor_busy_draining`).
    bind_running_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;

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

    if waited_and_timed_out && actor_dispatch_state(actor_state) == ActorDispatchState::Busy {
        // The wait has a transport budget. Ending it says nothing about
        // whether the current turn can still produce a response, so the turn
        // and its pending prompt stay. The report is the same busy refusal
        // Idea classifies as still-running. The old unclassified sentence was
        // a persistent Run Agent Doc error after the budget (observed: a live
        // Grok turn, `has_busy_cue=true`, exit 75 shown as a failure).
        let live_turn_cue = capture_active_turn_budget_abort_cue(tmux, &dispatch_pane, harness);
        let file_display = file.display().to_string();
        let recovery_hint = authoritative_actor_dispatch_recovery_hint(actor_state, file);
        let unblocker = if live_turn_cue.is_some() {
            "wait_for_owner_turn_to_finish"
        } else {
            "wait_for_dispatch_ready_prompt"
        };
        let blocked_outcome =
            agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(unblocker);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_retained_busy_generation file={} pane={} harness={} generation={} cue={:?} unblocker={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                actor.record.generation,
                live_turn_cue.as_deref().unwrap_or("none"),
                unblocker,
            ),
        );
        crate::invocation::record_route_deferral(retained_busy_generation_deferral(
            DispatchOnlyBusyRefusalFacts {
                generation: actor.record.generation,
                file_display: &file_display,
                dispatch_pane: &dispatch_pane,
                harness_binary: &harness.binary,
                reason: actor_dispatch_blocker_reason(actor_dispatch_state(actor_state))
                    .unwrap_or("the authoritative actor is busy"),
                wait_secs: dispatch_only_busy_refusal_wait_secs(
                    (effects.wait_for_ready_override)(),
                    dispatch_only_starting_pane_recovery_timeout_for_binary(
                        Some(harness.binary.as_str()),
                        cfg!(test),
                    ),
                ),
                recovery_hint: &recovery_hint,
                active_turn_busy_cue: live_turn_cue.as_deref(),
                blocked_outcome_fields: &blocked_outcome,
            },
        ));
        return Ok(dispatch_pane);
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
            origin: dispatch_origin,
        });

    if action != AuthoritativeActorDispatchAction::DeferAutomatedBehindLiveTurn
        && actor_dispatch_blocker_reason(actor_dispatch_state).is_some()
    {
        crate::route_focus::focus_route_pane(tmux, file, &dispatch_pane, "dispatch_blocked");
    }

    match action {
        AuthoritativeActorDispatchAction::DeferAutomatedBehindLiveTurn => {
            // GH #319: the controller orphan drain read the live
            // active-turn cue, then typed its plain trigger into the running
            // turn anyway because the plain-trigger busy pass-through is
            // origin-blind. An automated origin has no operator steering
            // intent: submit nothing, leave focus/layout alone, and report a
            // deferral so the drain retries after its durable backoff.
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_automated_dispatch_deferred_live_turn file={} pane={} harness={} generation={} actor_state={} origin={} cue={:?} action=retry_after_backoff focus_effect=preserved pane_effect=none",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    actor.record.generation,
                    actor_state.as_str(),
                    dispatch_origin.as_str(),
                    active_turn_busy_cue.as_deref().unwrap_or("none"),
                ),
            );
            crate::invocation::record_route_deferral(format!(
                "[route] automated dispatch for {} deferred: authoritative actor generation {} on pane {} is {} ({}); no trigger was typed into the live turn, the caller retries after its backoff",
                file.display(),
                actor.record.generation,
                dispatch_pane,
                actor_state.as_str(),
                active_turn_busy_cue.as_deref().unwrap_or("no live cue"),
            ));
            Ok(dispatch_pane)
        }
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
                RouteDispatchAuthorization::LiveTurnSteering { coalesced } => {
                    return finish_operator_live_turn_steering(
                        tmux,
                        file,
                        &dispatch_pane,
                        harness,
                        actor.record.generation,
                        coalesced,
                    );
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
                RouteDispatchAuthorization::LiveTurnSteering { coalesced } => {
                    return finish_operator_live_turn_steering(
                        tmux,
                        file,
                        &dispatch_pane,
                        harness,
                        actor.record.generation,
                        coalesced,
                    );
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
                RouteDispatchAuthorization::LiveTurnSteering { coalesced } => {
                    return finish_operator_live_turn_steering(
                        tmux,
                        file,
                        &dispatch_pane,
                        harness,
                        actor.record.generation,
                        coalesced,
                    );
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

    #[derive(Default)]
    struct ScriptedTmux {
        outputs: std::sync::Mutex<std::collections::VecDeque<String>>,
    }

    impl ScriptedTmux {
        fn new(outputs: impl IntoIterator<Item = &'static str>) -> Self {
            Self {
                outputs: std::sync::Mutex::new(outputs.into_iter().map(str::to_string).collect()),
            }
        }
    }

    impl agent_doc_tmux_io::TmuxCommandRunner for ScriptedTmux {
        fn run(
            &self,
            _command: &agent_doc_tmux_commands::TmuxCommand,
        ) -> std::result::Result<String, agent_doc_tmux_io::TmuxIoError> {
            Ok(self
                .outputs
                .lock()
                .expect("script lock")
                .pop_front()
                .expect("scripted tmux output"))
        }
    }

    #[test]
    fn multiline_steering_retries_enter_until_the_busy_composer_is_empty() {
        let draft = "• Working (1m 34s • esc to interrupt)\n\n› first page of steering\n  second page\ntab to queue message\ngpt-5.4 high · Context 54% used";
        let delivered =
            "• Waiting for background terminal (18m 36s • esc to interrupt)\n\n›\n29% context left";
        let tmux = ScriptedTmux::new([draft, draft, draft, "", delivered]);

        assert_eq!(
            confirm_running_turn_submission(&tmux, "%1", "codex", Duration::ZERO),
            Ok(1)
        );
        assert!(tmux.outputs.lock().expect("script lock").is_empty());
    }

    #[test]
    fn unreadable_post_submit_capture_never_authorizes_an_enter_retry() {
        let tmux = ScriptedTmux::new(std::iter::repeat_n(
            "redrawing",
            ROUTE_SYNC_SUBMIT_SETTLE_POLLS_PER_ATTEMPT,
        ));

        let error = confirm_running_turn_submission(&tmux, "%1", "claude", Duration::ZERO)
            .expect_err("unreadable composer fails closed");
        assert!(error.contains("could not be proven"), "{error}");
        assert!(tmux.outputs.lock().expect("script lock").is_empty());
    }

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
            RouteCloseoutBlockContext::LiveOwnerTurn,
        );
        let CloseoutWaitRouteOutcome::SteeringDelivered(message) = &delivered else {
            panic!("an explicit send with pending steering is a delivery: {delivered:?}");
        };
        assert!(!message.contains("nothing was dispatched"), "{message}");
        assert!(!message.contains("could not be drained"), "{message}");
        assert!(
            message.contains("Sent 2 updates to the agent already running"),
            "{message}"
        );
        assert!(message.contains("no second turn was started"), "{message}");
        assert!(
            !message.contains("No second trigger was injected"),
            "{message}"
        );
        assert!(
            message.contains("steering_delivered=2 sent=explicit"),
            "{message}"
        );
        assert!(
            message.contains("ui_outcome=steering_delivered_to_owner"),
            "{message}"
        );
        assert!(!message.contains("blocker:"), "{message}");
        assert!(!message.contains("session-check"), "{message}");

        let refused = route_sync_steering_not_delivered(file, "the composer could not be read");
        assert!(refused.contains("steering_delivered=0"), "{refused}");
        assert!(
            refused.contains("was not handed to the agent already running"),
            "{refused}"
        );
        assert!(
            !refused.contains("steering_delivered_to_owner"),
            "{refused}"
        );
        assert!(!refused.contains("ui_outcome_class=ok"), "{refused}");
        assert!(
            refused.contains("ui_outcome=deferred_for_operator_proof"),
            "{refused}"
        );

        // A readable unrelated draft is not this deferral. The behavioral
        // tests below hand the settled diff to the running turn and leave
        // that draft unsent. This wording stays for a draft that cannot be read.
        let unreadable_draft = route_sync_steering_not_delivered(
            file,
            &route_sync_operator_draft_reason("<unavailable>"),
        );
        assert!(
            unreadable_draft.contains("redacted draft preview"),
            "{unreadable_draft}"
        );
        assert!(
            unreadable_draft.contains("steering_delivered=0"),
            "{unreadable_draft}"
        );
        assert!(
            unreadable_draft.contains("ui_outcome=deferred_for_operator_proof"),
            "{unreadable_draft}"
        );

        // Nothing pending: the existing deferral is unchanged.
        let deferred = closeout_wait_route_outcome(
            file,
            head,
            blocker,
            &fields,
            0,
            RouteOwnerTurnEvidence::Live,
            RouteCloseoutBlockContext::LiveOwnerTurn,
        );
        let CloseoutWaitRouteOutcome::Deferred(message) = &deferred else {
            panic!("{deferred:?}");
        };
        assert!(message.contains("nothing was dispatched"), "{message}");
    }

    struct DraftComposer {
        draft: std::sync::Mutex<String>,
        submitted: std::sync::Mutex<Vec<String>>,
        commands: std::sync::Mutex<Vec<Vec<String>>>,
        loaded: std::sync::Mutex<String>,
        post_submit_unreadable_captures: std::sync::Mutex<usize>,
        clear_noop: bool,
        enter_noop: bool,
        cursor_y: String,
        render: fn(&str) -> String,
    }

    impl DraftComposer {
        fn claude(draft: &str) -> Self {
            Self::new(draft, "2", claude_busy_frame)
        }

        fn new(draft: &str, cursor_y: &str, render: fn(&str) -> String) -> Self {
            Self {
                draft: std::sync::Mutex::new(draft.to_string()),
                submitted: std::sync::Mutex::new(Vec::new()),
                commands: std::sync::Mutex::new(Vec::new()),
                loaded: std::sync::Mutex::new(String::new()),
                post_submit_unreadable_captures: std::sync::Mutex::new(0),
                clear_noop: false,
                enter_noop: false,
                cursor_y: cursor_y.to_string(),
                render,
            }
        }

        fn draft(&self) -> String {
            self.draft.lock().expect("draft").clone()
        }

        fn submitted(&self) -> Vec<String> {
            self.submitted.lock().expect("submitted").clone()
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.lock().expect("commands").clone()
        }

        fn mutating_keys(&self) -> Vec<String> {
            self.commands()
                .into_iter()
                .filter_map(|args| match args.first().map(String::as_str) {
                    Some("send-keys") => args.last().cloned(),
                    Some("load-buffer" | "paste-buffer") => Some(args[0].clone()),
                    _ => None,
                })
                .collect()
        }
    }

    impl agent_doc_tmux_io::TmuxCommandRunner for DraftComposer {
        fn run(
            &self,
            command: &agent_doc_tmux_commands::TmuxCommand,
        ) -> std::result::Result<String, agent_doc_tmux_io::TmuxIoError> {
            let args = command.args();
            self.commands.lock().expect("commands").push(args.to_vec());
            let verb = args.first().map(String::as_str);
            match verb {
                Some("capture-pane") => {
                    let mut unreadable = self
                        .post_submit_unreadable_captures
                        .lock()
                        .expect("post-submit unreadable captures");
                    if !self.submitted.lock().expect("submitted").is_empty() && *unreadable > 0 {
                        *unreadable -= 1;
                        return Ok("redrawing".to_string());
                    }
                    let draft = self.draft();
                    Ok((self.render)(&draft))
                }
                Some("display-message") => Ok(format!("{}\n", self.cursor_y)),
                Some("send-keys") => {
                    match args.last().map(String::as_str) {
                        Some("C-u") if !self.clear_noop => {
                            *self.draft.lock().expect("draft") = String::new();
                        }
                        Some("Enter") if !self.enter_noop => {
                            let mut draft = self.draft.lock().expect("draft");
                            if !draft.is_empty() {
                                self.submitted
                                    .lock()
                                    .expect("submitted")
                                    .push(draft.clone());
                                draft.clear();
                            }
                        }
                        _ => {}
                    }
                    Ok(String::new())
                }
                Some("load-buffer") => {
                    let path = args.last().expect("paste path");
                    *self.loaded.lock().expect("loaded") =
                        std::fs::read_to_string(path).expect("paste file");
                    Ok(String::new())
                }
                Some("paste-buffer") => {
                    let text = self.loaded.lock().expect("loaded").clone();
                    let mut draft = self.draft.lock().expect("draft");
                    if draft.is_empty() {
                        *draft = text;
                    } else {
                        draft.push_str(&text);
                    }
                    Ok(String::new())
                }
                _ => Ok(String::new()),
            }
        }
    }

    fn claude_busy_frame(draft: &str) -> String {
        let composer = if draft.is_empty() {
            "❯".to_string()
        } else {
            format!("❯ {draft}")
        };
        format!(
            "✶ Generating… (3s · esc to interrupt)\n\
             ────────────────────────────────────────\n\
             {composer}\n\
             ────────────────────────────────────────\n\
               ⏵⏵ bypass permissions on (shift+tab to cycle)\n\
               Opus 4.8 ctx:40% ~/work/repo main user@host\n"
        )
    }

    fn grok_blocked_frame(draft: &str) -> String {
        format!(
            "Waiting for response… 1.0s [stop]\n\
             1 command still running · send a message to interrupt\n\
             ╭────────────────────────╮\n\
             │ ❯ {draft} │\n\
             ╰──── Grok 4.7 (high) ───╯\n"
        )
    }

    fn grok_live_frame(draft: &str) -> String {
        format!(
            "Waiting for response… 1.0s [stop]\n\
             ╭────────────────────────╮\n\
             │ ❯ {draft} │\n\
             ╰──── Grok 4.7 (high) ───╯\n\
             Ctrl+c:cancel │ Ctrl+x:shortcuts\n"
        )
    }

    fn grok_idle_frame(draft: &str) -> String {
        format!(
            "╭────────────────────────╮\n\
             │ ❯ {draft} │\n\
             ╰──── Grok 4.7 (high) ───╯\n\
             Shift+Tab:mode  │  Ctrl+x:shortcuts\n"
        )
    }

    fn grok_unreadable_frame(_draft: &str) -> String {
        "Waiting for response… 1.0s [stop]\nno composer box\n".to_string()
    }

    fn settled_explicit_send() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        agent_doc_session_check_io::midturn_steering::seed_for_cycle(
            &file,
            "cycle-1",
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        agent_doc_session_check_io::midturn_steering::record_explicit_send(&file).unwrap();
        let edited = baseline.replace("- current task\n", "- current task\n- ship the diff\n");
        std::fs::write(&file, edited).unwrap();
        (dir, file)
    }

    fn claim_polled(
        runner: &DraftComposer,
        file: &Path,
        harness: &str,
    ) -> RunningTurnSteeringClaim {
        running_turn_steering_claim_polled(runner, file, "%1", harness, true, Duration::ZERO)
    }

    /// A settled explicit send is pasted into the running turn even while the
    /// composer holds a newer unsent draft. The draft is restored, unsent.
    #[test]
    fn explicit_send_delivers_settled_diff_and_restores_the_unsent_draft() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::claude("keep this draft");
        let claim = claim_polled(&runner, &file, "claude");
        assert!(
            matches!(claim, RunningTurnSteeringClaim::Count(n) if n >= 1),
            "{claim:?}"
        );
        let submitted = runner.submitted();
        assert_eq!(submitted.len(), 1, "{submitted:?}");
        assert!(submitted[0].contains("agent-doc steering"), "{submitted:?}");
        assert!(!submitted[0].contains("keep this draft"), "{submitted:?}");
        assert_eq!(runner.draft(), "keep this draft");
        let keys = runner.mutating_keys();
        let clear_at = keys.iter().position(|key| key == "C-u").expect("C-u");
        let enter_at = keys.iter().position(|key| key == "Enter").expect("Enter");
        assert!(clear_at < enter_at, "{keys:?}");
        assert_eq!(
            keys.iter().filter(|key| key.as_str() == "Enter").count(),
            1,
            "{keys:?}"
        );
        let pastes: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter_map(|(index, key)| (key == "paste-buffer").then_some(index))
            .collect();
        assert_eq!(pastes.len(), 2, "{keys:?}");
        assert!(pastes[0] < enter_at && enter_at < pastes[1], "{keys:?}");

        let before = runner.commands().len();
        let again = claim_polled(&runner, &file, "claude");
        assert_eq!(again, RunningTurnSteeringClaim::Count(0));
        assert_eq!(runner.draft(), "keep this draft");
        assert_eq!(runner.submitted().len(), 1);
        assert!(
            runner.commands()[before..].iter().all(|args| !matches!(
                args.first().map(String::as_str),
                Some("send-keys" | "load-buffer" | "paste-buffer")
            )),
            "{:?}",
            &runner.commands()[before..]
        );
    }

    /// Codex can briefly repaint without its busy cue after accepting a
    /// follow-up. That ambiguity is observed through, never submitted again,
    /// and the explicit route remains a delivery rather than an operator-proof
    /// deferral once the busy empty composer is visible again.
    #[test]
    fn explicit_send_settles_after_transient_unreadable_post_submit_frames() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::claude("");
        *runner
            .post_submit_unreadable_captures
            .lock()
            .expect("post-submit unreadable captures") = ROUTE_SYNC_SUBMIT_POLLS_PER_ATTEMPT;

        let claim = claim_polled(&runner, &file, "claude");
        let RunningTurnSteeringClaim::Count(delivered) = claim else {
            panic!("transient repaint must settle as delivered: {claim:?}");
        };
        assert!(delivered >= 1);
        assert_eq!(runner.submitted().len(), 1, "one follow-up was delivered");
        assert_eq!(
            runner
                .mutating_keys()
                .iter()
                .filter(|key| key.as_str() == "Enter")
                .count(),
            1,
            "an unreadable repaint never authorizes a duplicate submit"
        );

        let fields = agent_doc_controller::dispatch::route_closeout_user_outcome_fields(
            None,
            RouteOwnerTurnEvidence::Live,
        );
        let outcome = closeout_wait_route_outcome(
            &file,
            "current task",
            "the owner turn is still live",
            &fields,
            delivered,
            RouteOwnerTurnEvidence::Live,
            RouteCloseoutBlockContext::LiveOwnerTurn,
        );
        let CloseoutWaitRouteOutcome::SteeringDelivered(message) = outcome else {
            panic!("explicit steering must have a delivered route outcome: {outcome:?}");
        };
        assert!(message.contains("no second turn was started"), "{message}");
        assert!(
            message.contains(&format!("steering_delivered={delivered} sent=explicit")),
            "{message}"
        );
        assert!(
            message.contains("ui_outcome=steering_delivered_to_owner"),
            "{message}"
        );
        assert!(
            !message.contains("deferred_for_operator_proof"),
            "{message}"
        );
    }

    #[test]
    fn queued_follow_up_leaves_the_unsent_draft_untouched() {
        let (_dir, file) = settled_explicit_send();
        let prepared =
            agent_doc_session_check_io::midturn_steering::prepare_route_sync_steering(&file)
                .unwrap()
                .expect("a settled diff");
        assert!(prepared.needs_queue_paste);
        agent_doc_controller_io::live_turn_follow_up::mark_live_turn_follow_up_queued(
            &file,
            &prepared.cycle_id,
        )
        .unwrap();
        let runner = DraftComposer::claude("keep this draft");
        let claim = claim_polled(&runner, &file, "claude");
        assert!(
            matches!(claim, RunningTurnSteeringClaim::Count(n) if n >= 1),
            "{claim:?}"
        );
        assert!(runner.submitted().is_empty());
        assert_eq!(runner.draft(), "keep this draft");
        assert!(
            runner.mutating_keys().is_empty(),
            "{:?}",
            runner.mutating_keys()
        );
    }

    #[test]
    fn draft_that_cannot_be_moved_aside_is_not_concatenated_or_submitted() {
        let (_dir, file) = settled_explicit_send();
        let mut runner = DraftComposer::claude("keep this draft");
        runner.clear_noop = true;
        let claim = claim_polled(&runner, &file, "claude");
        let RunningTurnSteeringClaim::NotDelivered(message) = claim else {
            panic!("a draft C-u cannot move stays unsent: {claim:?}");
        };
        assert!(message.contains("could not be moved aside"), "{message}");
        assert!(message.contains("steering_delivered=0"), "{message}");
        assert!(
            message.contains("ui_outcome=deferred_for_operator_proof"),
            "{message}"
        );
        assert!(
            !message.contains("steering_delivered_to_owner"),
            "{message}"
        );
        assert_eq!(runner.draft(), "keep this draft");
        assert!(runner.submitted().is_empty());
        assert!(
            !runner
                .mutating_keys()
                .iter()
                .any(|key| key == "paste-buffer" || key == "Enter" || key == "load-buffer"),
            "{:?}",
            runner.mutating_keys()
        );
    }

    #[test]
    fn rejected_submit_restores_the_unsent_draft_without_submitting_it() {
        let (_dir, file) = settled_explicit_send();
        let mut runner = DraftComposer::claude("keep this draft");
        runner.enter_noop = true;
        let claim = claim_polled(&runner, &file, "claude");
        let RunningTurnSteeringClaim::NotDelivered(message) = claim else {
            panic!("an unproven submit is not a delivery: {claim:?}");
        };
        assert!(message.contains("steering draft"), "{message}");
        assert!(message.contains("steering_delivered=0"), "{message}");
        assert_eq!(runner.draft(), "keep this draft");
        assert!(
            runner.submitted().is_empty(),
            "the operator draft was not submitted: {:?}",
            runner.submitted()
        );
        assert!(!runner.draft().contains("agent-doc steering"));
    }

    #[test]
    fn grok_blocked_wait_with_a_draft_sends_no_enter() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::new("keep this draft", "3", grok_blocked_frame);
        let claim = claim_polled(&runner, &file, "grok");
        let RunningTurnSteeringClaim::NotDelivered(message) = claim else {
            panic!("blocked-wait Enter is not a delivery: {claim:?}");
        };
        assert!(message.contains("#blockedwaitentercancels"), "{message}");
        assert!(
            message.contains("ui_outcome=deferred_for_operator_proof"),
            "{message}"
        );
        assert!(message.contains("steering_delivered=0"), "{message}");
        assert_eq!(runner.draft(), "keep this draft");
        assert!(
            runner.mutating_keys().is_empty(),
            "{:?}",
            runner.mutating_keys()
        );
    }

    /// `#groksteerdeliver`: the interrupt hint with an empty composer is the
    /// explicit send. One Enter runs the message. A second claim does not.
    #[test]
    fn grok_blocked_wait_with_an_empty_composer_submits_once() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::new("", "3", grok_blocked_frame);
        let claim = claim_polled(&runner, &file, "grok");
        let RunningTurnSteeringClaim::Count(delivered) = claim else {
            panic!("an empty blocked-wait composer submits the explicit send: {claim:?}");
        };
        assert!(delivered >= 1, "{delivered}");
        assert_eq!(runner.submitted().len(), 1, "{:?}", runner.submitted());
        assert!(
            runner.submitted()[0].contains("agent-doc steering"),
            "{:?}",
            runner.submitted()
        );
        assert_eq!(
            runner
                .mutating_keys()
                .iter()
                .filter(|key| key.as_str() == "Enter")
                .count(),
            1,
            "{:?}",
            runner.mutating_keys()
        );

        let again = claim_polled(&runner, &file, "grok");
        assert_eq!(
            again,
            RunningTurnSteeringClaim::Count(0),
            "the delivered diff is not sent again"
        );
        assert_eq!(
            runner.submitted().len(),
            1,
            "a coalesced send does not paste again"
        );
        assert_eq!(
            runner
                .mutating_keys()
                .iter()
                .filter(|key| key.as_str() == "Enter")
                .count(),
            1,
            "a coalesced send does not press Enter again: {:?}",
            runner.mutating_keys()
        );
    }

    /// `#groksteerdeliver`: a live Grok turn that is not on the interrupt hint
    /// queues a plain Enter. The second Enter is send-now, once.
    #[test]
    fn grok_explicit_send_on_a_live_turn_sends_now_once() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::new("", "3", grok_live_frame);
        let claim = claim_polled(&runner, &file, "grok");
        let RunningTurnSteeringClaim::Count(delivered) = claim else {
            panic!("a live empty composer sends the explicit follow-up now: {claim:?}");
        };
        assert!(delivered >= 1, "{delivered}");
        assert_eq!(runner.submitted().len(), 1, "{:?}", runner.submitted());
        assert_eq!(
            runner
                .mutating_keys()
                .iter()
                .filter(|key| key.as_str() == "Enter")
                .count(),
            2,
            "{:?}",
            runner.mutating_keys()
        );

        let before = runner.mutating_keys().len();
        let again = claim_polled(&runner, &file, "grok");
        assert_eq!(
            again,
            RunningTurnSteeringClaim::Count(0),
            "the delivered diff is not sent again"
        );
        assert_eq!(runner.submitted().len(), 1);
        assert_eq!(
            runner.mutating_keys().len(),
            before,
            "the queued reference is not submitted again: {:?}",
            runner.mutating_keys()
        );
    }

    #[test]
    fn grok_send_now_that_drops_the_busy_cue_still_counts_as_delivered() {
        let idle = grok_idle_frame("");
        assert!(
            matches!(
                running_turn_submission_observation("grok", &idle),
                RunningTurnSubmissionObservation::Delivered
            ),
            "an empty composer after send-now is the delivery"
        );
        let blocked = grok_blocked_frame("");
        assert!(
            matches!(
                running_turn_submission_observation("grok", &blocked),
                RunningTurnSubmissionObservation::Delivered
            ),
            "the interrupt hint can outlive the Enter that cleared the composer"
        );
        let draft = grok_blocked_frame("keep this draft");
        assert!(
            matches!(
                running_turn_submission_observation("grok", &draft),
                RunningTurnSubmissionObservation::Draft
            ),
            "a draft under the interrupt hint is not a delivery"
        );
    }

    #[test]
    fn unreadable_running_composer_still_fails_closed() {
        let (_dir, file) = settled_explicit_send();
        let runner = DraftComposer::new("", "0", grok_unreadable_frame);
        let claim = claim_polled(&runner, &file, "grok");
        let RunningTurnSteeringClaim::NotDelivered(message) = claim else {
            panic!("an unreadable composer is not a delivery: {claim:?}");
        };
        assert!(
            message.contains("the composer could not be read"),
            "{message}"
        );
        assert!(
            message.contains("ui_outcome=deferred_for_operator_proof"),
            "{message}"
        );
        assert!(
            runner.mutating_keys().is_empty(),
            "{:?}",
            runner.mutating_keys()
        );
    }

    #[test]
    fn quota_blocked_owner_names_the_limit_and_never_advises_a_retry() {
        let file = Path::new("/tmp/sample-session.md");
        let gauge = "│ ❯   │\n╰── Weekly limit left: 0% · Grok 4.7 (high) · always-approve ─╯\n";
        assert!(
            agent_doc_harness::usage_limit::pane_usage_limit(gauge, &HarnessConfig::grok())
                .is_none(),
            "a 0% gauge is the harness's credit decision, not a block"
        );
        let limit = agent_doc_harness::usage_limit::pane_usage_limit(
            "You hit your weekly limit\n│ ❯   │\n╰── Weekly limit left: 0% · Grok 4.7 (high) · always-approve ─╯\n",
            &HarnessConfig::grok(),
        )
        .expect("printed refusal");
        let outcome = quota_blocked_owner_route_outcome(file, "%10", &limit, 2, None);
        let CloseoutWaitRouteOutcome::Deferred(message) = &outcome else {
            panic!("{outcome:?}");
        };
        for needle in [
            "Grok Build is out of usage",
            "You hit your weekly limit",
            "2 updates stay saved in the document",
            "No trigger was sent to pane %10",
            "Wait for the limit to reset",
            "`agent:` in its frontmatter",
            "owner_turn=quota_blocked",
            "steering_delivered=0",
            "ui_outcome_class=blocked",
        ] {
            assert!(message.contains(needle), "missing {needle:?}: {message}");
        }
        for forbidden in [
            "owner_turn=ambiguous",
            "again in a few seconds",
            "cancel-turn",
            "could not tell",
        ] {
            assert!(!message.contains(forbidden), "{forbidden:?}: {message}");
        }
    }

    #[test]
    fn quota_blocked_owner_says_where_the_prompt_went() {
        let file = Path::new("/tmp/sample-session.md");
        let limit = agent_doc_harness::usage_limit::HarnessUsageLimit::reported("out of credits");
        let outcome =
            quota_blocked_owner_route_outcome(file, "%10", &limit, 0, Some("do [#sample]"));
        assert!(
            outcome
                .message()
                .contains("Queued \"do [#sample]\" in active agent:queue"),
            "{}",
            outcome.message()
        );
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
            RouteCloseoutBlockContext::OpenEmptyPreflight,
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

    #[test]
    fn write_applied_route_yields_to_retained_commit_recovery() {
        let file = Path::new("/tmp/sdk-dev.md");
        let blocker = "closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: resume durable checkpoint cycle=cycle-1794810168413 phase=write_applied capture_id=cycle-1794810168413; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /tmp/sdk-dev.md";
        let outcome = retained_recovery_route_outcome(file, blocker, 0);
        let CloseoutWaitRouteOutcome::RetainedRecoveryOwns(message) = &outcome else {
            panic!("write_applied with nothing to enqueue yields: {outcome:?}");
        };
        for needle in [
            "retained commit recovery owns the open closeout",
            "did not resubmit finalize or write",
            "owner_turn=retained_recovery",
            "cycle-1794810168413",
            "ui_outcome=no_drainable_work",
            "ui_outcome_class=ok",
        ] {
            assert!(message.contains(needle), "missing {needle:?}: {message}");
        }
        for forbidden in [
            "this route was NOT queued",
            "cancel-turn",
            "ui_outcome_class=blocked",
            "recovery_command=",
        ] {
            assert!(!message.contains(forbidden), "{forbidden:?}: {message}");
        }

        let _ = crate::invocation::take_route_deferral();
        let _ = crate::invocation::take_route_live_owner_turn();
        outcome.record(file, 0);
        assert!(crate::invocation::take_route_deferral().is_none());
        assert!(crate::invocation::take_route_live_owner_turn().is_some());
    }

    #[test]
    fn ownerless_write_applied_send_names_captured_finalize_recovery() {
        let file = Path::new("/tmp/sample-session.md");
        let blocker = "closeout recovery blocked [open_cycle]: retained commit recovery owns the terminal boundary";
        let fields = agent_doc_controller::dispatch::route_closeout_user_outcome_fields(
            Some("agent-doc admin reregister-editor /tmp/sample-session.md"),
            RouteOwnerTurnEvidence::Absent,
        );
        let outcome = closeout_wait_route_outcome(
            file,
            "do [#sample]",
            blocker,
            &fields,
            1,
            RouteOwnerTurnEvidence::Absent,
            RouteCloseoutBlockContext::CapturedFinalizeRecovery,
        );
        let CloseoutWaitRouteOutcome::Deferred(message) = outcome else {
            panic!("ownerless explicit send must be blocked");
        };
        assert!(
            message.contains("retained captured-finalize worker"),
            "{message}"
        );
        assert!(message.contains("state and delivery edges"), "{message}");
        assert!(
            message.contains("already ran immediate repair"),
            "{message}"
        );
        assert!(!message.contains("stale-empty-preflight"), "{message}");
    }

    /// `#runsteer`: Run Agent Doc is steering. Empty steering is a noop. A
    /// live lease means a turn is running, so an idle pane enqueues instead
    /// of deferring. It does not claim a tmux submit and does not start a
    /// second turn. A busy pane keeps the live-owner outcomes.
    #[test]
    fn classify_run_steer_noops_enqueues_or_starts() {
        assert_eq!(classify_run_steer(0, true), RunSteerDecision::Noop);
        assert_eq!(classify_run_steer(0, false), RunSteerDecision::Noop);
        assert_eq!(classify_run_steer(1, true), RunSteerDecision::Enqueue);
        assert_eq!(classify_run_steer(2, true), RunSteerDecision::Enqueue);
        assert_eq!(
            classify_run_steer(1, false),
            RunSteerDecision::EnqueueAndStart
        );
        assert_eq!(
            classify_run_steer(3, false),
            RunSteerDecision::EnqueueAndStart
        );
    }

    #[test]
    fn idle_owner_pane_with_a_live_lease_enqueues_steering_or_noops() {
        let file = Path::new("/tmp/game-sessions.md");
        let verdict = "[session-check] IN PROGRESS: cycle `cycle-1793163655242` remains `preflight_started` (preflight_started) while owning pane `%16` has a fresh active-turn lease";
        assert!(matches!(
            lease_live_owner_route_outcome(file, "%16", None, verdict, 1, false),
            CloseoutWaitRouteOutcome::SteeringDelivered(_)
        ));
        assert!(matches!(
            lease_live_owner_route_outcome(file, "%16", None, verdict, 0, false),
            CloseoutWaitRouteOutcome::OwnerTurnBusy(_)
        ));

        let noop =
            lease_live_owner_route_outcome(file, "%16", Some("commitpush"), verdict, 0, true);
        let CloseoutWaitRouteOutcome::RunSteerNoop(message) = &noop else {
            panic!("empty steering while a turn is starting is a noop: {noop:?}");
        };
        assert!(
            message.contains("enqueued no steering, so it did nothing"),
            "{message}"
        );
        assert!(message.contains("No turn was started"), "{message}");
        assert!(message.contains("owner_turn=noop"), "{message}");
        assert!(message.contains("pane=%16"), "{message}");
        assert!(
            message.contains("ui_outcome=no_drainable_work"),
            "{message}"
        );
        assert!(message.contains("ui_outcome_class=ok"), "{message}");
        for forbidden in [
            "could not tell",
            "cancel-turn",
            "Run Agent Doc again",
            "saved in the document",
            "nothing is working",
            "No second trigger was injected",
            "owner_turn=ambiguous",
            "deferred_for_operator_proof",
            "steering_delivered_to_owner",
            "steering_enqueued=",
            "already running",
        ] {
            assert!(!message.contains(forbidden), "{forbidden}: {message}");
        }

        for explicit in [1, 2] {
            let outcome = lease_live_owner_route_outcome(
                file,
                "%16",
                Some("commitpush"),
                verdict,
                explicit,
                true,
            );
            let CloseoutWaitRouteOutcome::SteeringEnqueued(message) = &outcome else {
                panic!("pending steering is enqueued for the starting turn: {outcome:?}");
            };
            let noun = if explicit == 1 { "update" } else { "updates" };
            assert!(
                message.contains(&format!(
                    "Enqueued {explicit} {noun} as steering for the turn already starting"
                )),
                "{message}"
            );
            assert!(message.contains("no second turn was started"), "{message}");
            assert!(
                message.contains(&format!("steering_enqueued={explicit}")),
                "{message}"
            );
            assert!(message.contains("owner_turn=starting"), "{message}");
            assert!(
                message.contains("ui_outcome=queued_behind_owner"),
                "{message}"
            );
            assert!(message.contains("ui_outcome_class=ok"), "{message}");
            assert!(message.contains("cycle-1793163655242"), "{message}");
            for forbidden in [
                "could not tell",
                "cancel-turn",
                "Run Agent Doc again",
                "saved in the document",
                "nothing is working",
                "No second trigger was injected",
                "owner_turn=ambiguous",
                "deferred_for_operator_proof",
                "steering_delivered",
                "already running",
                "Sent ",
            ] {
                assert!(!message.contains(forbidden), "{forbidden}: {message}");
            }
        }

        let queued =
            run_steer_queued_prompt_outcome(file, verdict, "do [#sample]", true, false, false);
        let CloseoutWaitRouteOutcome::SteeringEnqueued(message) = &queued else {
            panic!("{queued:?}");
        };
        assert!(
            message.contains("Queued pending dispatch \"do [#sample]\" in active agent:queue"),
            "{message}"
        );
        assert!(
            message.contains("as steering for the turn already starting"),
            "{message}"
        );
        assert!(message.contains("steering_enqueued=1"), "{message}");
        assert!(
            message.contains("ui_outcome=queued_behind_owner"),
            "{message}"
        );
        assert!(message.contains("ui_outcome_class=ok"), "{message}");
        for forbidden in [
            "could not tell",
            "cancel-turn",
            "nothing is working",
            "No second trigger was injected",
            "owner_turn=ambiguous",
            "deferred_for_operator_proof",
            "steering_delivered",
        ] {
            assert!(!message.contains(forbidden), "{forbidden}: {message}");
        }
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
        assert!(
            message.contains("Sent 1 update to the agent already running"),
            "{message}"
        );
        assert!(message.contains("no second turn was started"), "{message}");
        assert!(
            !message.contains("No second trigger was injected"),
            "{message}"
        );
    }

    /// A route can observe the empty preflight before the admission hook's
    /// turn-active lease reaches session-check. The authoritative pane's
    /// harness-specific busy cue is positive liveness proof and must protect
    /// that turn from the generic `open_empty_preflight` cancellation remedy.
    #[test]
    fn busy_authoritative_pane_strengthens_open_empty_preflight_to_live_owner() {
        let blocker = "closeout recovery replay_safe [open_empty_preflight]: agent-doc session cancel-turn /tmp/sample-session.md";
        let verdict = blocked_closeout_live_owner_verdict(
            RouteCloseoutBlockContext::OpenEmptyPreflight,
            blocker,
            "%2",
            "codex",
            200,
            Some("esc to interrupt"),
        )
        .expect("an exact busy owner pane proves the turn is live");

        assert!(verdict.contains("ACTIVE OWNER"), "{verdict}");
        assert!(verdict.contains("generation 200"), "{verdict}");
        assert!(verdict.contains("pane %2"), "{verdict}");
        assert!(verdict.contains("active codex turn"), "{verdict}");
        assert!(!verdict.contains("cancel-turn"), "{verdict}");
        assert!(!verdict.contains("open_empty_preflight"), "{verdict}");

        let outcome =
            live_owner_turn_route_outcome(Path::new("/tmp/sample-session.md"), None, &verdict, 0);
        let CloseoutWaitRouteOutcome::OwnerTurnBusy(message) = outcome else {
            panic!("busy owner proof must be a benign live-turn yield");
        };
        assert!(message.contains("a turn is already running"), "{message}");
        assert!(message.contains("nothing needs recovery"), "{message}");
        assert!(message.contains("owner_turn=live"), "{message}");
        assert!(!message.contains("cancel-turn"), "{message}");
        assert!(!message.contains("open_empty_preflight"), "{message}");

        assert!(
            blocked_closeout_live_owner_verdict(
                RouteCloseoutBlockContext::OpenEmptyPreflight,
                blocker,
                "%2",
                "codex",
                200,
                None,
            )
            .is_none(),
            "an empty-preflight shape alone is not owner liveness proof"
        );
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
