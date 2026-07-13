//! Dispatch-only route reopen I/O.

mod proof;
pub use proof::{
    DispatchOnlyBugReportFacts, dispatch_only_dispatch_start_proof_required,
    dispatch_only_sent_console_message_for, dispatch_only_sent_log_message_for,
    require_dispatch_only_dispatch_start_proof, wait_for_dispatch_only_recycle_inflight_settle,
};

use anyhow::Result;
use std::path::Path;
use std::time::Duration;
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
use crate::dispatch_recovery::wait_for_starting_pane_recovery_target;
use crate::dispatch_target::register_dispatch_target;
use crate::launch_contract::reapply_codex_launch_contract_before_reuse;
use crate::restart_handoff::wait_for_busy_restart_handoff;
use crate::startup_ready::wait_for_agent_ready_outcome;
use crate::supervisor_runtime::restart_via_supervisor_with_mode;
use agent_doc_controller::dispatch::{
    BusyPaneAutoFixOutcome, DispatchOnlyBlockerRecoveryHintFacts, DispatchOnlyReopenDelivery,
    DispatchOnlyStartingPaneActorReadyFacts, DispatchOnlyStartingPaneNotReadyMessageFacts,
    RoutedReopenGuardReason, dispatch_only_blocked_guard_reason,
    dispatch_only_blocker_recovery_hint, dispatch_only_should_print_unproven_progress,
    dispatch_only_starting_pane_actor_settled, dispatch_only_starting_pane_not_ready_message,
    prompt_ready_barrier_failed_event,
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
    let prompt_ready = current_generation_ready_prompt_proven(tmux, &actor, harness);
    let recognized_blocker = agent_doc_tmux_io::capture_pane(tmux, dispatch_pane)
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
#[derive(Debug, Clone, Copy)]
pub struct DispatchOnlySendReopenOptions<'a> {
    pub delivery: DispatchOnlyReopenDelivery,
    pub queue_prompt_text: Option<&'a str>,
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

    let mut dispatch_pane = pane.to_string();
    let mut log_status =
        agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
            .ok()
            .flatten();
    let mut recovery_attempts = 0usize;
    let requires_ready_probe =
        agent_doc_supervisor::startup_miss::dispatch_only_requires_ready_probe(
            log_status.as_ref(),
            &dispatch_pane,
            &harness.binary,
        );
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
        loop {
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

            let ready_outcome = wait_for_agent_ready_outcome(
                tmux,
                &dispatch_pane,
                (options.effects.dispatch_only_starting_pane_ready_timeout)(harness),
                harness,
            );
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

            if recovery_attempts < 2
                && let Some(target) = wait_for_starting_pane_recovery_target(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    log_status.as_ref(),
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
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_only_starting_pane_not_ready file={} pane={} harness={} outcome={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    detail
                ),
            );
            let file_display = file.display().to_string();
            let outcome_fields = agent_doc_flow::outcome::blocked_with_exact_unblocker_fields(
                "wait_for_dispatch_ready_prompt",
            );
            anyhow::bail!(dispatch_only_starting_pane_not_ready_message(
                DispatchOnlyStartingPaneNotReadyMessageFacts {
                    harness_binary: &harness.binary,
                    pane: &dispatch_pane,
                    file_display: &file_display,
                    detail,
                    outcome_fields: &outcome_fields,
                },
            ));
        }
    }

    if let Ok(content) = agent_doc_tmux_io::capture_pane(tmux, &dispatch_pane)
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
        if let Some(source) = agent_doc_queue::route_dispatch::dispatch_active_turn_queue_source(
            &harness.binary,
            &reason,
        ) && let Some(prompt_text) = options.queue_prompt_text
        {
            // #jb-run-preempt-autoloop-priority: manual Run Agent Doc into a busy
            // active turn preempts pending auto items (head-insert).
            let queued =
                (options.effects.enqueue_route_dispatch_prompt)(file, prompt_text, source, true)?;
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
        let file_display = file.display().to_string();
        let recovery = dispatch_only_blocker_recovery_hint(DispatchOnlyBlockerRecoveryHintFacts {
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

    drop(pre_dispatch_route_guard.take());
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
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
                await_start_proof: dispatch_only_dispatch_start_proof_required(file, harness),
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
                await_start_proof: dispatch_only_dispatch_start_proof_required(file, harness),
                print_unproven_progress: dispatch_only_should_print_unproven_progress(),
            },
        )?,
    };
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
