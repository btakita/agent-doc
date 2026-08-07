//! Dispatch-only route reopen I/O.

mod proof;
pub use proof::{
    DispatchOnlyBugReportFacts, dispatch_only_dispatch_start_proof_required,
    dispatch_only_sent_console_message_for, dispatch_only_sent_log_message_for,
    require_dispatch_only_dispatch_start_proof, wait_for_dispatch_only_recycle_inflight_settle,
};

use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

use crate::authoritative_actor::{
    ManagedCapabilityProofStatus, authoritative_actor_ready_facts_from_target,
    current_generation_ready_prompt_proven, load_authoritative_actor_binding,
    managed_capability_proof_status,
};
use crate::busy_pane::{
    BusyPaneInterruptRecoveryOutcome, ExistingPaneDispatchReadiness,
    attempt_busy_existing_pane_auto_fix, attempt_busy_existing_pane_interrupt_recovery,
    ensure_existing_pane_ready_for_dispatch,
};
use crate::dispatch::{
    DirectPaneDispatchOptions, RouteDispatchBugReportFacts, RouteDispatchEffects,
    SupervisorIpcDispatchOptions, dispatch_routed_reopen_with_mode,
    dispatch_via_supervisor_ipc_with_mode,
};
use crate::dispatch_recovery::{
    StartingPaneRecoveryWaitOptions, wait_for_starting_pane_recovery_target,
};
use crate::dispatch_target::register_dispatch_target;
use crate::launch_contract::reapply_codex_launch_contract_before_reuse;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::startup_ready::{pane_composer_draft, wait_for_agent_ready_outcome};
use crate::supervisor_runtime::restart_via_supervisor_with_mode;
use agent_doc_controller::dispatch::{
    AuthoritativeActorDispatchIntent, BusyPaneAutoFixOutcome, DirectPaneSubmitPolicy,
    DispatchOnlyBlockerRecoveryHintFacts, DispatchOnlyReadyProbeResolutionFacts,
    DispatchOnlyReopenDelivery, DispatchOnlyRouteCycleStamp,
    DispatchOnlyStartingPaneActorReadyFacts, DispatchOnlyStartingPaneDraftMessageFacts,
    DispatchOnlyStartingPaneNotReadyMessageFacts, RoutedReopenGuardReason, StartingPaneBlocker,
    classify_direct_pane_submit_policy, dispatch_only_blocked_guard_reason,
    dispatch_only_blocker_recovery_hint, dispatch_only_effective_ready_probe_required,
    dispatch_only_route_superseded_by_new_cycle, dispatch_only_should_print_unproven_progress,
    dispatch_only_starting_pane_actor_settled, dispatch_only_starting_pane_draft_message,
    dispatch_only_starting_pane_not_ready_message, prompt_ready_barrier_failed_event,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::route_runtime::authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible;
use agent_doc_supervisor::startup_miss::StartingPaneRecoveryTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOnlyQueuedPromptOutcome {
    pub prompt_text: String,
    pub appended: bool,
    pub already_present: bool,
    pub superseded: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DispatchOnlyRouteEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub enqueue_route_dispatch_prompt:
        fn(&Path, &str, &str, bool) -> Result<DispatchOnlyQueuedPromptOutcome>,
    pub emit_busy_route_queued_diagnostic: fn(&Tmux, &str, &Path, &HarnessConfig),
    pub emit_busy_route_diagnostic: fn(&Tmux, &str, &Path, &HarnessConfig),
    pub dispatch_only_starting_pane_ready_timeout: fn(&HarnessConfig) -> Duration,
    pub file_route_dispatch_bug_report: for<'a> fn(RouteDispatchBugReportFacts<'a>),
}

fn remaining_ready_wait(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

fn dispatch_only_starting_pane_ready_via_authoritative_actor(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    dispatch_pane: &str,
    harness: &HarnessConfig,
) -> bool {
    let actor = match load_authoritative_actor_binding(
        tmux, file, session_id, file_path, harness, false, false,
    ) {
        Ok(Some(actor)) => actor,
        Ok(None) => return false,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_starting_pane_actor_probe_failed file={} pane={} harness={} error={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    agent_doc_secret_redact::redact(&err.to_string())
                ),
            );
            return false;
        }
    };
    if !actor.prompt_dispatch_allowed() {
        return false;
    }
    let prompt_ready = current_generation_ready_prompt_proven(tmux, &actor, harness);
    // Escapes preserved: `dispatch_only_blocker_reason` reaches
    // `protected_prompt_input_reason`, which uses the dim/faint styling of the
    // composer body to tell a ghost hint from real unsent input.
    let recognized_blocker = agent_doc_tmux_io::capture_pane_with_ansi(tmux, dispatch_pane)
        .ok()
        .and_then(|content| agent_doc_harness::dispatch_only_blocker_reason(harness, &content));
    let ready_facts = authoritative_actor_ready_facts_from_target(&actor, prompt_ready);
    if !dispatch_only_starting_pane_actor_settled(
        DispatchOnlyStartingPaneActorReadyFacts {
            requested_pane: dispatch_pane,
            ready_facts: &ready_facts,
            dispatch_eligible: supervisor_authoritative_actor_dispatch_target_eligible(
                &actor.runtime,
            ),
        },
        recognized_blocker.is_some(),
    ) {
        return false;
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_starting_pane_settled_via_actor_state file={} pane={} harness={} generation={} runtime_state={} transition={} prompt_ready={} recognized_blocker={}",
            file.display(),
            dispatch_pane,
            harness.binary,
            actor.record.generation,
            actor.runtime.actor_state_label(),
            actor.record.last_transition.reason,
            prompt_ready,
            recognized_blocker.as_deref().unwrap_or("none"),
        ),
    );
    true
}

/// `#jbdisprecycle` — refused-because-supervisor-recycling error. Distinct from
/// the not-booted-yet error: the pane may already be at a prompt, but the
/// project supervisor is mid-`execve` hot-reload, so a trigger typed now would
/// be dropped before submit. Fail closed (don't type) and let the caller retry
/// once the recycle settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchOnlyBlockerAction {
    SubmitPlainTrigger,
    QueuePrompt(&'static str),
    Refuse,
}

fn classify_dispatch_only_blocker(
    intent: AuthoritativeActorDispatchIntent,
    harness_binary: &str,
    blocker_reason: &str,
    has_queue_prompt: bool,
) -> DispatchOnlyBlockerAction {
    if intent == AuthoritativeActorDispatchIntent::PlainTrigger
        && agent_doc_queue::route_dispatch::dispatch_active_turn_accepts_plain_trigger(
            harness_binary,
            blocker_reason,
        )
    {
        return DispatchOnlyBlockerAction::SubmitPlainTrigger;
    }
    if has_queue_prompt
        && let Some(source) = agent_doc_queue::route_dispatch::dispatch_active_turn_queue_source(
            harness_binary,
            blocker_reason,
        )
    {
        return DispatchOnlyBlockerAction::QueuePrompt(source);
    }
    DispatchOnlyBlockerAction::Refuse
}

#[derive(Debug, Clone, Copy)]
pub struct DispatchOnlySendReopenOptions<'a> {
    pub delivery: DispatchOnlyReopenDelivery,
    pub queue_prompt_text: Option<&'a str>,
    pub intent: AuthoritativeActorDispatchIntent,
    pub effects: DispatchOnlyRouteEffects,
}

pub fn dispatch_only_send_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: DispatchOnlySendReopenOptions<'_>,
) -> Result<String> {
    let delivery = options.delivery;

    // `#jbdisprecycle`: gate BEFORE any pane input. If the project supervisor is
    // mid-`execve` recycle (lib-install auto-recycle / operator restart), a
    // trigger typed now is dropped before submit — the live no-submit repro.
    // Wait a bounded window for the fresh supervisor to settle (it clears the
    // marker on watch-loop start), then proceed with the normal ready probe.
    // Fail closed (never type) if it never settles, so the caller retries
    // instead of stacking an unsubmitted trigger.
    wait_for_dispatch_only_recycle_inflight_settle(file, file_path, pane, &harness.binary)?;

    let route_start_closeout = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    let mut dispatch_pane = pane.to_string();
    let mut log_status =
        agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
            .ok()
            .flatten();
    let mut recovery_attempts = 0usize;
    let historical_probe_required =
        agent_doc_supervisor::startup_miss::dispatch_only_requires_ready_probe(
            log_status.as_ref(),
            &dispatch_pane,
            &harness.binary,
        );
    let authoritative_actor_settled = historical_probe_required
        && dispatch_only_starting_pane_ready_via_authoritative_actor(
            tmux,
            file,
            session_id,
            file_path,
            &dispatch_pane,
            harness,
        );
    let requires_ready_probe =
        dispatch_only_effective_ready_probe_required(DispatchOnlyReadyProbeResolutionFacts {
            historical_probe_required,
            authoritative_actor_settled,
        });
    if historical_probe_required && !requires_ready_probe {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_historical_ready_probe_superseded file={} pane={} harness={} source=authoritative_actor",
                file.display(),
                dispatch_pane,
                harness.binary,
            ),
        );
    }
    let mut pre_dispatch_route_guard = Some(
        agent_doc_controller_io::project_controller::begin_route_submit_with_reason(
            file,
            &dispatch_pane,
            &harness.binary,
            if requires_ready_probe {
                agent_doc_state_backbone::ROUTE_DISPATCH_ONLY_READY_PROBE_REASON
            } else {
                "dispatch_only_pre_dispatch"
            },
        )?,
    );
    if requires_ready_probe {
        // `--wait-for-ready` is one request budget, not a fresh allowance for
        // every same-pane/handoff recovery attempt. Resetting it here made the
        // async controller publish a useful terminal refusal after the editor
        // had already stopped polling (#jbroutasync-starting).
        let ready_timeout = (options.effects.dispatch_only_starting_pane_ready_timeout)(harness);
        let ready_deadline = Instant::now() + ready_timeout;
        loop {
            // A visible operator draft is a terminal blocker, not a readiness
            // state that can improve with time. Refuse before spending the
            // editor's route budget so the async command plane can publish the
            // exact unblocker while its client is still polling.
            // Capture WITH escapes: the draft/ghost discriminator is the composer
            // body's *styling* (dim/faint SGR 2 = placeholder, normal intensity =
            // real keystrokes), so a plain `capture-pane -p` throws away the only
            // signal that tells them apart and reports every autosuggest hint as an
            // operator draft. Prompt parsing strips ANSI per line itself
            // (`last_prompt_candidate`), so the candidate is unchanged either way.
            if let Some(draft_preview) =
                agent_doc_tmux_io::capture_pane_with_ansi(tmux, &dispatch_pane)
                    .ok()
                    .and_then(|content| {
                        pane_composer_draft(tmux, &dispatch_pane, &content, harness)
                    })
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_starting_pane_not_ready file={} pane={} harness={} outcome=operator_draft composer_draft=true",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                    ),
                );
                let outcome_fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
                    StartingPaneBlocker::OperatorDraft.unblocker(),
                );
                anyhow::bail!(dispatch_only_starting_pane_draft_message(
                    DispatchOnlyStartingPaneDraftMessageFacts {
                        harness_binary: &harness.binary,
                        pane: &dispatch_pane,
                        file_display: &file.display().to_string(),
                        draft_preview: &draft_preview,
                        outcome_fields: &outcome_fields,
                    },
                ));
            }
            if dispatch_only_starting_pane_ready_via_authoritative_actor(
                tmux,
                file,
                session_id,
                file_path,
                &dispatch_pane,
                harness,
            ) {
                break;
            }

            let remaining = remaining_ready_wait(ready_deadline, Instant::now());
            let ready_outcome = if remaining.is_zero() {
                crate::startup_ready::AgentReadyWaitOutcome::TimedOut
            } else {
                wait_for_agent_ready_outcome(tmux, &dispatch_pane, remaining, harness)
            };
            if ready_outcome.is_ready() {
                break;
            }
            if dispatch_only_starting_pane_ready_via_authoritative_actor(
                tmux,
                file,
                session_id,
                file_path,
                &dispatch_pane,
                harness,
            ) {
                break;
            }

            let recovery_remaining = remaining_ready_wait(ready_deadline, Instant::now());
            if recovery_attempts < 2
                && !recovery_remaining.is_zero()
                && let Some(target) = wait_for_starting_pane_recovery_target(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    StartingPaneRecoveryWaitOptions {
                        initial_status: log_status.as_ref(),
                        max_wait: Some(recovery_remaining),
                    },
                )
            {
                recovery_attempts += 1;
                match target {
                    StartingPaneRecoveryTarget::SamePane => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_starting_pane_retry_same_pane file={} pane={} harness={} attempt={}",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                recovery_attempts
                            ),
                        );
                        log_status = agent_doc_supervisor_io::startup_miss::session_log_status(
                            file, session_id,
                        )
                        .ok()
                        .flatten();
                        continue;
                    }
                    StartingPaneRecoveryTarget::DifferentPane(next_pane) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_starting_pane_handoff file={} old_pane={} new_pane={} harness={} attempt={}",
                                file.display(),
                                dispatch_pane,
                                next_pane,
                                harness.binary,
                                recovery_attempts
                            ),
                        );
                        dispatch_pane = next_pane;
                        log_status = agent_doc_supervisor_io::startup_miss::session_log_status(
                            file, session_id,
                        )
                        .ok()
                        .flatten();
                        continue;
                    }
                }
            }

            let detail = ready_outcome.blocker_reason().unwrap_or("timed_out");
            // An operator draft parked in the composer can never satisfy the
            // dispatch-ready predicate, so "wait for the pane to become ready"
            // would be an unsatisfiable instruction. Report the draft and the
            // real unblocker instead (#panedraftunblocker).
            // Escapes preserved for the same reason as the pre-ready check above:
            // dim/faint styling is what separates a ghost hint from real input.
            let draft = agent_doc_tmux_io::capture_pane_with_ansi(tmux, &dispatch_pane)
                .ok()
                .and_then(|content| pane_composer_draft(tmux, &dispatch_pane, &content, harness));
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_starting_pane_not_ready file={} pane={} harness={} outcome={} composer_draft={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    detail,
                    draft.is_some()
                ),
            );
            let file_display = file.display().to_string();
            let blocker = StartingPaneBlocker::from_composer_draft(draft.as_deref());
            let outcome_fields =
                agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(blocker.unblocker());
            match (blocker, draft.as_deref()) {
                (StartingPaneBlocker::OperatorDraft, Some(draft_preview)) => {
                    anyhow::bail!(dispatch_only_starting_pane_draft_message(
                        DispatchOnlyStartingPaneDraftMessageFacts {
                            harness_binary: &harness.binary,
                            pane: &dispatch_pane,
                            file_display: &file_display,
                            draft_preview,
                            outcome_fields: &outcome_fields,
                        },
                    ));
                }
                _ => anyhow::bail!(dispatch_only_starting_pane_not_ready_message(
                    DispatchOnlyStartingPaneNotReadyMessageFacts {
                        harness_binary: &harness.binary,
                        pane: &dispatch_pane,
                        file_display: &file_display,
                        detail,
                        outcome_fields: &outcome_fields,
                    },
                )),
            }
        }
    }

    let current_closeout = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    let route_start_stamp = DispatchOnlyRouteCycleStamp {
        cycle_id: route_start_closeout
            .as_ref()
            .and_then(|projection| projection.cycle_id.as_deref()),
        phase: route_start_closeout
            .as_ref()
            .and_then(|projection| projection.phase),
    };
    let current_stamp = DispatchOnlyRouteCycleStamp {
        cycle_id: current_closeout
            .as_ref()
            .and_then(|projection| projection.cycle_id.as_deref()),
        phase: current_closeout
            .as_ref()
            .and_then(|projection| projection.phase),
    };
    if dispatch_only_route_superseded_by_new_cycle(route_start_stamp, current_stamp) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_superseded_by_new_cycle file={} pane={} harness={} baseline_cycle={} current_cycle={} current_phase={} outcome=no_pane_input",
                file.display(),
                dispatch_pane,
                harness.binary,
                route_start_stamp.cycle_id.unwrap_or("none"),
                current_stamp.cycle_id.unwrap_or("none"),
                current_stamp
                    .phase
                    .map(agent_doc_turn::CyclePhase::as_str)
                    .unwrap_or("none"),
            ),
        );
        return Ok(dispatch_pane);
    }

    // Escapes preserved for the same reason as the recognized-blocker probe above.
    if let Ok(content) = agent_doc_tmux_io::capture_pane_with_ansi(tmux, &dispatch_pane)
        && let Some(reason) = agent_doc_harness::dispatch_only_blocker_reason(harness, &content)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_blocked file={} pane={} harness={} reason={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                reason
            ),
        );
        match classify_dispatch_only_blocker(
            options.intent,
            &harness.binary,
            &reason,
            options.queue_prompt_text.is_some(),
        ) {
            DispatchOnlyBlockerAction::SubmitPlainTrigger => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_plain_trigger_active_turn file={} pane={} harness={} blocker={} outcome=submit_bare_trigger",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        reason,
                    ),
                );
            }
            DispatchOnlyBlockerAction::QueuePrompt(source) => {
                let prompt_text = options
                    .queue_prompt_text
                    .expect("classified queue action requires prompt text");
                // #jb-run-preempt-autoloop-priority: manual Run Agent Doc into a busy
                // active turn preempts pending auto items (head-insert).
                let queued = (options.effects.enqueue_route_dispatch_prompt)(
                    file,
                    prompt_text,
                    source,
                    true,
                )?;
                eprintln!(
                    "[route] dispatch-only {} reopen for {} found {} on pane {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger {}",
                    harness.binary,
                    file.display(),
                    reason,
                    dispatch_pane,
                    queued.prompt_text,
                    queued.appended,
                    queued.already_present,
                    queued.superseded,
                    agent_doc_flow::outcome::user_outcome_fields(
                        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner
                    )
                );
                // #claude-busy-status-during-active-turn: this queued path previously
                // returned Ok silently, so the operator saw nothing and the session
                // looked idle while a turn was in flight. Surface the turn-in-progress
                // + queued status on the pane (status-only; no hard block — the prompt
                // already auto-queued above and runs when the current turn finishes).
                (options.effects.emit_busy_route_queued_diagnostic)(
                    tmux,
                    &dispatch_pane,
                    file,
                    harness,
                );
                return Ok(dispatch_pane);
            }
            DispatchOnlyBlockerAction::Refuse => {
                let file_display = file.display().to_string();
                let recovery =
                    dispatch_only_blocker_recovery_hint(DispatchOnlyBlockerRecoveryHintFacts {
                        harness_binary: &harness.binary,
                        reason: &reason,
                        file_display: &file_display,
                    });
                // #snrun: name the interactive shell substate distinctly from a generic
                // busy actor so the failure says which terminal state blocked dispatch.
                let guard_reason = dispatch_only_blocked_guard_reason(&reason);
                agent_doc_flow_io::log_flow_event(
                    file,
                    prompt_ready_barrier_failed_event(guard_reason),
                    agent_doc_ops_log_io::log_op,
                );
                if guard_reason == RoutedReopenGuardReason::BlockedInInteractiveSubstate {
                    anyhow::bail!(
                        "dispatch-only {} reopen refused to inject into pane {} for {} because the pane is blocked in an interactive terminal substate ({}), not a dispatch-ready composer; {}",
                        harness.binary,
                        dispatch_pane,
                        file.display(),
                        reason,
                        recovery
                    );
                }
                anyhow::bail!(
                    "dispatch-only {} reopen refused to inject into pane {} for {} because the pane still shows {}; {}",
                    harness.binary,
                    dispatch_pane,
                    file.display(),
                    reason,
                    recovery
                );
            }
        }
    }

    drop(pre_dispatch_route_guard.take());
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    let direct_pane_submit_policy = classify_direct_pane_submit_policy(options.intent);
    let await_start_proof = direct_pane_submit_policy
        == DirectPaneSubmitPolicy::ObserveHarnessAcceptance
        && dispatch_only_dispatch_start_proof_required(file, harness);
    let dispatch_start = match delivery {
        DispatchOnlyReopenDelivery::SupervisorIpcOnce => dispatch_via_supervisor_ipc_with_mode(
            tmux,
            file,
            &dispatch_pane,
            session_id,
            file_path,
            harness,
            SupervisorIpcDispatchOptions {
                effects: options.effects.route_dispatch_effects,
                await_start_proof,
                print_unproven_progress: dispatch_only_should_print_unproven_progress(),
            },
        )?,
        DispatchOnlyReopenDelivery::DirectPaneSubmit => dispatch_routed_reopen_with_mode(
            tmux,
            file,
            &dispatch_pane,
            file_path,
            harness,
            DirectPaneDispatchOptions {
                effects: options.effects.route_dispatch_effects,
                await_start_proof,
                print_unproven_progress: dispatch_only_should_print_unproven_progress(),
                submit_policy: direct_pane_submit_policy,
            },
        )?,
    };
    if direct_pane_submit_policy == DirectPaneSubmitPolicy::ObserveHarnessAcceptance {
        require_dispatch_only_dispatch_start_proof(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
            |DispatchOnlyBugReportFacts { elapsed, proof }| {
                (options.effects.file_route_dispatch_bug_report)(RouteDispatchBugReportFacts {
                    file,
                    pane: &dispatch_pane,
                    harness,
                    phase: "dispatch_only_dispatch_start_proof",
                    issue: "accepted_without_dispatch_start_proof",
                    result: "accepted_without_dispatch_start_proof",
                    elapsed,
                    proof: Some(proof),
                    diagnostic_path: None,
                });
            },
        )?;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &dispatch_only_sent_log_message_for(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
        ),
    );
    eprintln!(
        "{}",
        dispatch_only_sent_console_message_for(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start
        )
    );
    Ok(dispatch_pane)
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_only_reopen_existing_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    prompt_bearing_marker: Option<&str>,
    queue_prompt_text: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    pane_id: &str,
    delivery: DispatchOnlyReopenDelivery,
    effects: DispatchOnlyRouteEffects,
    skip_capability_proof: bool,
) -> Result<String> {
    let dispatch_pane = reapply_codex_launch_contract_before_reuse(
        tmux, file, pane_id, session_id, file_path, harness, false, false,
    )?;
    if !skip_capability_proof {
        // `#capproofbg`: a still-`Pending` proof no longer blocks the dispatch-only
        // reopen. Read status without polling for it to settle; a pending proof lets
        // the reopen dispatch proceed while the proof runs in the background, and a
        // later FAILURE is surfaced asynchronously by the supervisor. Only an
        // already-failed/missing proof disables the reopen.
        match managed_capability_proof_status(file, session_id, harness)? {
            ManagedCapabilityProofStatus::NotRequired
            | ManagedCapabilityProofStatus::Proven
            | ManagedCapabilityProofStatus::Pending => {}
            ManagedCapabilityProofStatus::Failed => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is disabled because managed capability proof failed",
                harness.binary,
                file.display(),
                dispatch_pane
            ),
            ManagedCapabilityProofStatus::Missing => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is disabled because this network/SSH/write-root session has no current capability proof",
                harness.binary,
                file.display(),
                dispatch_pane
            ),
        }
    } else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_skip_capability_proof file={} pane={} harness={} reason=degraded_supervisor_unreachable",
                file.display(),
                dispatch_pane,
                harness.binary
            ),
        );
    }
    let log_status = agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    if agent_doc_supervisor::startup_miss::dispatch_only_requires_ready_probe(
        log_status.as_ref(),
        &dispatch_pane,
        &harness.binary,
    ) {
        return dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
                intent: AuthoritativeActorDispatchIntent::PromptAware,
                effects,
            },
        );
    }
    if harness.binary == "codex"
        && agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
            .ok()
            .flatten()
            .is_some_and(|miss| miss.pane_id == dispatch_pane)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_startup_miss_bypass file={} pane={} harness={}",
                file.display(),
                dispatch_pane,
                harness.binary
            ),
        );
        return dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
                intent: AuthoritativeActorDispatchIntent::PromptAware,
                effects,
            },
        );
    }
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    match ensure_existing_pane_ready_for_dispatch(
        tmux,
        file,
        &dispatch_pane,
        harness,
        prompt_bearing_marker,
    )? {
        ExistingPaneDispatchReadiness::Ready => dispatch_only_send_reopen(
            tmux,
            file,
            session_id,
            &dispatch_pane,
            file_path,
            harness,
            DispatchOnlySendReopenOptions {
                delivery,
                queue_prompt_text,
                intent: AuthoritativeActorDispatchIntent::PromptAware,
                effects,
            },
        ),
        ExistingPaneDispatchReadiness::BusyAlreadyRunning => Ok(dispatch_pane),
        ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
            provenance,
            blocker_reason,
        } => retry_dispatch_only_after_busy_pane(
            tmux,
            file,
            pane,
            col_args,
            session_id,
            file_path,
            target_session,
            harness,
            created_panes,
            prompt_bearing_marker,
            queue_prompt_text,
            allow_auto_fix_retry,
            allow_busy_interrupt_retry,
            auto_fix_attempted,
            &dispatch_pane,
            &provenance,
            blocker_reason.as_deref(),
            delivery,
            effects,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn retry_dispatch_only_after_busy_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
    created_panes: &mut Vec<String>,
    prompt_bearing_marker: Option<&str>,
    queue_prompt_text: Option<&str>,
    allow_auto_fix_retry: bool,
    allow_busy_interrupt_retry: bool,
    auto_fix_attempted: bool,
    busy_pane: &str,
    provenance: &str,
    blocker_reason: Option<&str>,
    delivery: DispatchOnlyReopenDelivery,
    effects: DispatchOnlyRouteEffects,
) -> Result<String> {
    if let Some(reason) = blocker_reason
        && let Some(source) = agent_doc_queue::route_dispatch::dispatch_active_turn_queue_source(
            &harness.binary,
            reason,
        )
        && let Some(prompt_text) = queue_prompt_text
    {
        // A queueable blocker (an active turn or operator-owned modal) is
        // neither an idle composer nor a broken actor. Preserve the prompt
        // durably and let the normal queue drain resume when it clears; never
        // run document repair or send Escape/Enter into the pane.
        let queued = (effects.enqueue_route_dispatch_prompt)(file, prompt_text, source, true)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_only_blocker_queued file={} pane={} harness={} reason={} source={} appended={} already_present={} superseded={}",
                file.display(),
                busy_pane,
                harness.binary,
                reason,
                source,
                queued.appended,
                queued.already_present,
                queued.superseded,
            ),
        );
        (effects.emit_busy_route_queued_diagnostic)(tmux, busy_pane, file, harness);
        return Ok(busy_pane.to_string());
    }
    let fallback_detail = blocker_reason.map(|reason| format!("still shows {reason}"));
    if allow_auto_fix_retry {
        match attempt_busy_existing_pane_auto_fix(tmux, file, session_id, busy_pane, file_path)? {
            BusyPaneAutoFixOutcome::RetryRoute => {
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    effects,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart => {
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    effects,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_retry_after_fresh_restart file={} pane={} harness={}",
                        file.display(),
                        busy_pane,
                        harness.binary
                    ),
                );
                eprintln!(
                    "[route] dispatch-only {} reopen for {} found busy authoritative pane {} after the scoped recovery path — restarting the live session fresh once before retrying",
                    harness.binary,
                    file.display(),
                    busy_pane
                );
                if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
                    (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                    anyhow::bail!(
                        agent_doc_controller::dispatch::format_busy_existing_pane_error(
                            file.display(),
                            busy_pane,
                            &harness.binary,
                            provenance,
                            fallback_detail.as_deref(),
                            true
                        )
                    );
                }
                wait_for_busy_restart_handoff(tmux, file, file_path, session_id, busy_pane);
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    allow_busy_interrupt_retry,
                    true,
                    busy_pane,
                    delivery,
                    effects,
                    false,
                );
            }
            BusyPaneAutoFixOutcome::FailClosed => {}
        }
    }
    if allow_busy_interrupt_retry {
        match attempt_busy_existing_pane_interrupt_recovery(
            tmux,
            file,
            busy_pane,
            harness,
            blocker_reason,
        )? {
            BusyPaneInterruptRecoveryOutcome::Recovered => {
                return dispatch_only_reopen_existing_pane(
                    tmux,
                    file,
                    pane,
                    col_args,
                    session_id,
                    file_path,
                    target_session,
                    harness,
                    created_panes,
                    prompt_bearing_marker,
                    queue_prompt_text,
                    false,
                    false,
                    true,
                    busy_pane,
                    delivery,
                    effects,
                    false,
                );
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                let detail = format!("bounded interrupt recovery still shows {reason}");
                anyhow::bail!(
                    agent_doc_controller::dispatch::format_busy_existing_pane_error(
                        file.display(),
                        busy_pane,
                        &harness.binary,
                        provenance,
                        Some(detail.as_str()),
                        auto_fix_attempted || allow_auto_fix_retry
                    )
                );
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
                anyhow::bail!(
                    agent_doc_controller::dispatch::format_busy_existing_pane_error(
                        file.display(),
                        busy_pane,
                        &harness.binary,
                        provenance,
                        Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                        auto_fix_attempted || allow_auto_fix_retry
                    )
                );
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    (effects.emit_busy_route_diagnostic)(tmux, busy_pane, file, harness);
    anyhow::bail!(
        agent_doc_controller::dispatch::format_busy_existing_pane_error(
            file.display(),
            busy_pane,
            &harness.binary,
            provenance,
            fallback_detail.as_deref(),
            auto_fix_attempted || allow_auto_fix_retry
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Operator-reported 2026-07-25: Run Agent Doc refused to dispatch into a pane
    /// whose composer held only Claude's dim autosuggest hint
    /// ("❯\u{a0}Please compact the exchange"), reporting
    /// `unblocker=submit_or_clear_pane_draft` — an instruction the operator cannot
    /// satisfy, because there is nothing to submit or clear.
    ///
    /// The discriminator (`prompt_candidate_is_dim_placeholder`) was already correct;
    /// it was being fed a **plain** `capture-pane -p`, which strips the SGR-2 styling
    /// that is the ONLY signal separating a ghost hint from real unsent input. This
    /// pins the plumbing: every pane capture whose content reaches a dim-sensitive
    /// predicate must preserve escapes. A behavioural test cannot cover it here (the
    /// predicates need a live tmux pane and cursor), and the harness-level unit tests
    /// pass either way precisely because they are handed raw content directly — which
    /// is exactly how this regression slipped through.
    #[test]
    fn composer_draft_and_blocker_probes_capture_with_escapes() {
        let full = include_str!("dispatch_only.rs");
        // Scan production code only — otherwise this guard matches its own needle.
        let source = full
            .split_once("\n#[cfg(test)]")
            .map(|(production, _)| production)
            .expect("dispatch_only.rs must keep its tests behind #[cfg(test)]");
        for (probe, context) in [
            ("pane_composer_draft(tmux, &dispatch_pane", "composer draft"),
            (
                "agent_doc_harness::dispatch_only_blocker_reason(harness,",
                "blocker reason",
            ),
        ] {
            assert!(
                source.contains(probe),
                "{context} probe moved; update this guard"
            );
        }
        assert!(
            !source.contains("agent_doc_tmux_io::capture_pane(tmux,"),
            "a dim-sensitive probe is reading a plain capture again — the SGR-2 \
             styling that distinguishes an autosuggest ghost from real unsent \
             operator input is stripped by `capture-pane -p`, so every hint would be \
             reported as a draft with an unsatisfiable unblocker"
        );
    }

    #[test]
    fn starting_pane_recovery_consumes_one_total_ready_budget() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(15);

        assert_eq!(
            remaining_ready_wait(deadline, start + Duration::from_secs(10)),
            Duration::from_secs(5)
        );
        assert_eq!(
            remaining_ready_wait(deadline, start + Duration::from_secs(15)),
            Duration::ZERO
        );
        assert_eq!(
            remaining_ready_wait(deadline, start + Duration::from_secs(20)),
            Duration::ZERO
        );
    }

    #[test]
    fn blocker_policy_submits_only_plain_triggers_to_actual_active_turns() {
        assert_eq!(
            classify_dispatch_only_blocker(
                AuthoritativeActorDispatchIntent::PlainTrigger,
                "codex",
                "active codex turn",
                false,
            ),
            DispatchOnlyBlockerAction::SubmitPlainTrigger,
        );
        assert_eq!(
            classify_dispatch_only_blocker(
                AuthoritativeActorDispatchIntent::PlainTrigger,
                "claude",
                "claude artifact picker open",
                false,
            ),
            DispatchOnlyBlockerAction::Refuse,
        );
    }

    #[test]
    fn blocker_policy_preserves_prompt_aware_queue_or_refuse_behavior() {
        assert_eq!(
            classify_dispatch_only_blocker(
                AuthoritativeActorDispatchIntent::PromptAware,
                "codex",
                "active codex turn",
                true,
            ),
            DispatchOnlyBlockerAction::QueuePrompt("dispatch_only_codex_active_turn"),
        );
        assert_eq!(
            classify_dispatch_only_blocker(
                AuthoritativeActorDispatchIntent::PromptAware,
                "codex",
                "active codex turn",
                false,
            ),
            DispatchOnlyBlockerAction::Refuse,
        );
    }

    #[test]
    fn dispatch_only_progress_policy_is_harness_neutral() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            dispatch_only_should_print_unproven_progress(),
            "dispatch-only reroutes report accepted-delivery progress the same way for all harnesses"
        );
        assert!(
            dispatch_only_should_print_unproven_progress(),
            "Codex hook visibility does not change the dispatch-only progress policy"
        );
    }
}
