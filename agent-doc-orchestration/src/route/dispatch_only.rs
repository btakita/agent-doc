//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn dispatch_only_requires_ready_probe(
    status: Option<&crate::startup_miss::SessionLogStatus>,
    pane: &str,
    harness: &HarnessConfig,
) -> bool {
    let Some(status) = status else {
        return false;
    };
    if !status.latest_session_open()
        || status.latest_start_pane.as_deref() != Some(pane)
        || status.saw_committed_cycle_after_latest_run
    {
        return false;
    }

    status
        .latest_run_event
        .as_deref()
        .and_then(|event| event.split_whitespace().next())
        .is_some_and(|token| {
            token == format!("{}_start", harness.binary)
                || token == format!("{}_restart", harness.binary)
        })
}

pub(crate) fn dispatch_only_starting_pane_not_ready_error(
    harness: &HarnessConfig,
    pane: &str,
    file: &Path,
    detail: &str,
) -> String {
    format!(
        "dispatch-only {} reopen refused to inject into pane {} for {} because the latest run is still booting and never reached a dispatch-ready prompt ({detail}); wait for the pane to become ready and reroute again {}",
        harness.binary,
        pane,
        file.display(),
        blocked_with_unblocker_fields("wait_for_dispatch_ready_prompt")
    )
}

fn dispatch_only_starting_pane_actor_ready_gate(
    actor: &AuthoritativeActorDispatchTarget,
    pane: &str,
    prompt_ready: bool,
) -> bool {
    if actor.record.pane_id != pane {
        return false;
    }
    if actor.actor_state() != agent_doc_sqlite::state_store::ActorState::Ready {
        return false;
    }
    matches!(
        classify_authoritative_prompt_ready_barrier(AuthoritativePromptReadyBarrierFacts {
            ready_facts: &authoritative_actor_ready_facts_from_target(actor, prompt_ready),
            dispatch_eligible: authoritative_actor_dispatch_target_eligible(actor),
        }),
        PromptReadyBarrierDecision::Ready
    )
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
            crate::ops_log::log_op(
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
    if !dispatch_only_starting_pane_actor_ready_gate(&actor, dispatch_pane, prompt_ready) {
        return false;
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_dispatch_only_starting_pane_ready_via_actor_state file={} pane={} harness={} generation={} runtime_state={} transition={} prompt_ready={}",
            file.display(),
            dispatch_pane,
            harness.binary,
            actor.record.generation,
            runtime_actor_state_label(&actor.runtime),
            actor.record.last_transition.reason,
            prompt_ready
        ),
    );
    true
}

/// `#jbdisprecycle` — refused-because-supervisor-recycling error. Distinct from
/// the not-booted-yet error: the pane may already be at a prompt, but the
/// project supervisor is mid-`execve` hot-reload, so a trigger typed now would
/// be dropped before submit. Fail closed (don't type) and let the caller retry
/// once the recycle settles.
pub(crate) fn dispatch_only_recycle_inflight_error(
    harness: &HarnessConfig,
    pane: &str,
    file: &Path,
    reason: &str,
) -> String {
    format!(
        "dispatch-only {} reopen refused to inject into pane {} for {} because the project supervisor is mid-recycle (reason={reason}); a trigger typed across the hot-reload boundary would be dropped before submit. Retry once the supervisor settles onto the fresh binary {}",
        harness.binary,
        pane,
        file.display(),
        blocked_with_unblocker_fields("wait_for_supervisor_recycle_settle")
    )
}

/// `#jbdisprecycle` — bound on how long a dispatch waits for an in-flight
/// supervisor recycle to settle before failing closed. Slightly under the
/// recycle-inflight marker TTL so a genuinely-stuck recycle surfaces as a
/// retryable dispatch error rather than hanging the JB action indefinitely.
const RECYCLE_SETTLE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
/// Poll cadence while waiting for the recycle to settle.
const RECYCLE_SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatchOnlySendReopenOptions<'a> {
    pub(crate) delivery: DispatchOnlyReopenDelivery,
    pub(crate) queue_prompt_text: Option<&'a str>,
}

pub(crate) fn dispatch_only_send_reopen(
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
    if crate::recycle_inflight::recycle_inflight_pending(file_path) {
        let started = std::time::Instant::now();
        let reason = crate::recycle_inflight::read_recycle_inflight(file_path)
            .map(|m| m.reason)
            .unwrap_or_else(|| "unknown".to_string());
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_recycle_inflight_wait file={} pane={} harness={} reason={}",
                file.display(),
                pane,
                harness.binary,
                reason
            ),
        );
        while crate::recycle_inflight::recycle_inflight_pending(file_path) {
            if started.elapsed() >= RECYCLE_SETTLE_WAIT {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "route_dispatch_only_recycle_inflight_unsettled file={} pane={} harness={} reason={} waited_ms={}",
                        file.display(),
                        pane,
                        harness.binary,
                        reason,
                        started.elapsed().as_millis()
                    ),
                );
                anyhow::bail!(dispatch_only_recycle_inflight_error(
                    harness, pane, file, &reason
                ));
            }
            std::thread::sleep(RECYCLE_SETTLE_POLL);
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_recycle_inflight_settled file={} pane={} harness={} reason={} waited_ms={}",
                file.display(),
                pane,
                harness.binary,
                reason,
                started.elapsed().as_millis()
            ),
        );
    }

    let mut dispatch_pane = pane.to_string();
    let mut log_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    let mut recovery_attempts = 0usize;
    let requires_ready_probe =
        dispatch_only_requires_ready_probe(log_status.as_ref(), &dispatch_pane, harness);
    let mut pre_dispatch_route_guard =
        Some(crate::route_in_flight::begin_route_submit_with_reason(
            file,
            &dispatch_pane,
            &harness.binary,
            if requires_ready_probe {
                "dispatch_only_ready_probe"
            } else {
                "dispatch_only_pre_dispatch"
            },
        )?);
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
                dispatch_only_starting_pane_ready_timeout(harness),
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
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_dispatch_only_starting_pane_retry_same_pane file={} pane={} harness={} attempt={}",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                recovery_attempts
                            ),
                        );
                        log_status = crate::startup_miss::session_log_status(file, session_id)
                            .ok()
                            .flatten();
                        continue;
                    }
                    StartingPaneRecoveryTarget::DifferentPane(next_pane) => {
                        crate::ops_log::log_op(
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
                        log_status = crate::startup_miss::session_log_status(file, session_id)
                            .ok()
                            .flatten();
                        continue;
                    }
                }
            }

            let detail = ready_outcome.blocker_reason().unwrap_or("timed_out");
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_only_starting_pane_not_ready file={} pane={} harness={} outcome={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary,
                    detail
                ),
            );
            anyhow::bail!(dispatch_only_starting_pane_not_ready_error(
                harness,
                &dispatch_pane,
                file,
                detail
            ));
        }
    }

    if let Ok(content) = sessions::capture_pane(tmux, &dispatch_pane)
        && let Some(reason) = dispatch_only_blocker_reason(harness, &content)
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_blocked file={} pane={} harness={} reason={}",
                file.display(),
                dispatch_pane,
                harness.binary,
                reason
            ),
        );
        if let Some(source) = dispatch_active_turn_queue_source(harness, &reason)
            && let Some(prompt_text) = options.queue_prompt_text
        {
            // #jb-run-preempt-autoloop-priority: manual Run Agent Doc into a busy
            // active turn preempts pending auto items (head-insert).
            let queued = enqueue_route_dispatch_prompt(file, prompt_text, source, true)?;
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
                user_outcome_fields(crate::flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner)
            );
            // #claude-busy-status-during-active-turn: this queued path previously
            // returned Ok silently, so the operator saw nothing and the session
            // looked idle while a turn was in flight. Surface the turn-in-progress
            // + queued status on the pane (status-only; no hard block — the prompt
            // already auto-queued above and runs when the current turn finishes).
            emit_busy_route_queued_diagnostic(tmux, &dispatch_pane, file, harness);
            return Ok(dispatch_pane);
        }
        let recovery = dispatch_blocker_recovery_hint(harness, &reason, file);
        // #snrun: name the interactive shell substate distinctly from a generic
        // busy actor so the failure says which terminal state blocked dispatch.
        let guard_reason = dispatch_only_blocked_guard_reason(&reason);
        log_prompt_ready_barrier_failed(file, guard_reason);
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
    )?;
    let file_display = file.display().to_string();
    let proof_facts = DispatchOnlyProofOutcomeFacts {
        file_display: &file_display,
        pane: &dispatch_pane,
        harness_binary: &harness.binary,
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        )
        .as_secs(),
    };
    crate::ops_log::log_op(file, &dispatch_only_sent_log_message(proof_facts));
    eprintln!("{}", dispatch_only_sent_console_message(proof_facts));
    Ok(dispatch_pane)
}

pub(crate) fn dispatch_only_dispatch_start_proof_required(
    file: &Path,
    harness: &HarnessConfig,
) -> bool {
    if harness.binary == "codex" && codex_dispatch_start_tracking_enabled(file) {
        return true;
    }
    controller_dispatch_only_dispatch_start_proof_required(&harness.binary)
}

pub(crate) fn require_dispatch_only_dispatch_start_proof(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> Result<()> {
    let proof_required = dispatch_only_dispatch_start_proof_required(file, harness);
    let classification = classify_dispatch_start_proof(DispatchStartProofFacts {
        proof: dispatch_start,
        dispatch_start_proof_required: proof_required,
    });
    if classification.decision == DispatchStartProofDecision::Accepted {
        return Ok(());
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test))
            .as_secs();
    let file_display = file.display().to_string();
    let facts = DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: timeout,
    };
    log_dispatch_proof_failed(
        file,
        RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof,
    );
    if let Err(err) = crate::route_in_flight::mark_route_submit_blocked(
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
    crate::ops_log::log_op(file, &accepted_only_dispatch_start_log_message(facts));
    file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
        file,
        pane,
        harness,
        phase: "dispatch_only_dispatch_start_proof",
        issue: "accepted_without_dispatch_start_proof",
        result: "accepted_without_dispatch_start_proof",
        elapsed: routed_dispatch_start_timeout_for_binary(
            Some(harness.binary.as_str()),
            cfg!(test),
        ),
        proof: Some(dispatch_start),
        diagnostic_path: None,
    });
    anyhow::bail!(accepted_only_dispatch_start_refusal_message(facts));
}

#[cfg(test)]
fn dispatch_only_test_sent_log_message(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_only_reopen_existing_pane(
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
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_only_skip_capability_proof file={} pane={} harness={} reason=degraded_supervisor_unreachable",
                file.display(),
                dispatch_pane,
                harness.binary
            ),
        );
    }
    let log_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    if dispatch_only_requires_ready_probe(log_status.as_ref(), &dispatch_pane, harness) {
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
            },
        );
    }
    if harness.binary == "codex"
        && crate::startup_miss::load(file)
            .ok()
            .flatten()
            .is_some_and(|miss| miss.pane_id == dispatch_pane)
    {
        crate::ops_log::log_op(
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
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retry_dispatch_only_after_busy_pane(
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
                    false,
                );
            }
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart => {
                crate::ops_log::log_op(
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
                    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                    anyhow::bail!(format_busy_existing_pane_error(
                        file,
                        busy_pane,
                        harness,
                        provenance,
                        fallback_detail.as_deref(),
                        true
                    ));
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
                    false,
                );
            }
            BusyPaneInterruptRecoveryOutcome::Blocked { reason } => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                let detail = format!("bounded interrupt recovery still shows {reason}");
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some(detail.as_str()),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::TimedOut => {
                emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
                anyhow::bail!(format_busy_existing_pane_error(
                    file,
                    busy_pane,
                    harness,
                    provenance,
                    Some("bounded interrupt recovery never restored a dispatch-ready prompt"),
                    auto_fix_attempted || allow_auto_fix_retry
                ));
            }
            BusyPaneInterruptRecoveryOutcome::Skipped => {}
        }
    }
    emit_busy_route_diagnostic(tmux, busy_pane, file, harness);
    anyhow::bail!(format_busy_existing_pane_error(
        file,
        busy_pane,
        harness,
        provenance,
        fallback_detail.as_deref(),
        auto_fix_attempted || allow_auto_fix_retry
    ));
}

pub(crate) fn dispatch_only_blocker_reason(
    harness: &HarnessConfig,
    content: &str,
) -> Option<String> {
    if let Some(reason) = harness.dispatch_blocker_reason(content) {
        return Some(reason);
    }
    if harness.binary != "codex" {
        return None;
    }

    let normalized = agent_doc_turn_executor_tmux::prompt::strip_ansi(content).to_ascii_lowercase();
    if normalized.contains("reverse-i-search") {
        Some("interactive shell reverse-i-search".to_string())
    } else if normalized.contains("i-search")
        && normalized.contains("accept")
        && normalized.contains("cancel")
    {
        Some("interactive shell history search".to_string())
    } else {
        None
    }
}

pub(crate) fn dispatch_blocker_recovery_hint(
    harness: &HarnessConfig,
    reason: &str,
    file: &Path,
) -> String {
    if harness.binary == "codex" && reason == "codex hook review prompt" {
        return format!(
            "open `/hooks` in that Codex pane, approve or disable the pending hook change, wait for the idle composer, then rerun `agent-doc route --dispatch-only {}` or the editor Run Agent Doc action",
            file.display()
        );
    }

    "restore an idle prompt and retry".to_string()
}

pub(crate) fn dispatch_active_turn_queue_source(
    harness: &HarnessConfig,
    reason: &str,
) -> Option<&'static str> {
    match (harness.binary.as_str(), reason) {
        ("codex", "active codex turn") => Some("dispatch_only_codex_active_turn"),
        ("opencode", "opencode active turn") => Some("dispatch_only_opencode_active_turn"),
        // `#jb-run-agent-doc-busy-wait-deadlock`: a busy Claude pane (a session
        // not running `/loop`) is an active turn just like Codex/OpenCode. The
        // dispatch-only reopen path must queue the prompt to an active `agent:queue`
        // for the idle-queue watch to drain, not bail/refuse, so JB `Run Agent
        // Doc` on a mid-turn Claude actor enqueues instead of erroring.
        ("claude", "active claude turn") => Some("dispatch_only_claude_active_turn"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    #[test]
    fn dispatch_only_starting_pane_not_ready_error_matches_sampleportal_active_turn() {
        let file = std::path::Path::new("tasks/professional/sampleportal.md");
        let message = dispatch_only_starting_pane_not_ready_error(
            &HarnessConfig::codex(),
            "%42",
            file,
            "active codex turn",
        );

        assert!(message.contains("dispatch-only codex reopen refused"));
        assert!(message.contains("tasks/professional/sampleportal.md"));
        assert!(message.contains("latest run is still booting"));
        assert!(message.contains("never reached a dispatch-ready prompt"));
        assert!(message.contains("(active codex turn)"));
        assert!(message.contains("ui_outcome=blocked_with_exact_unblocker"));
        assert!(message.contains("unblocker=wait_for_dispatch_ready_prompt"));
    }
    #[test]
    fn dispatch_only_starting_pane_actor_ready_gate_requires_same_ready_prompt_proven_actor() {
        let mut record = test_actor_record("%42");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        record.generation = 9;
        record.last_transition.reason = "prompt_ready".to_string();
        record.last_transition.new_generation = 9;
        let ready_actor = AuthoritativeActorDispatchTarget {
            record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(agent_doc_sqlite::state_store::ActorState::Ready),
            },
        };

        assert!(
            dispatch_only_starting_pane_actor_ready_gate(&ready_actor, "%42", true),
            "a healthy Ready actor for the same pane with prompt proof should satisfy the startup gate"
        );
        assert!(
            !dispatch_only_starting_pane_actor_ready_gate(&ready_actor, "%99", true),
            "a Ready actor for a different pane must not satisfy this dispatch pane's gate"
        );
        assert!(
            !dispatch_only_starting_pane_actor_ready_gate(&ready_actor, "%42", false),
            "Ready state without prompt/current-generation proof must still fail closed"
        );

        let mut busy_actor = ready_actor.clone();
        busy_actor.runtime.actor_state = Some(agent_doc_sqlite::state_store::ActorState::Busy);
        assert!(
            !dispatch_only_starting_pane_actor_ready_gate(&busy_actor, "%42", true),
            "non-Ready runtime state must not bypass the startup probe"
        );

        let mut degraded_actor = ready_actor;
        degraded_actor.runtime = SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        };
        assert!(
            !dispatch_only_starting_pane_actor_ready_gate(&degraded_actor, "%42", true),
            "a persisted Ready record without healthy runtime authority must not bypass the startup probe"
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

    #[test]
    fn dispatch_only_codex_requires_start_proof_when_hooks_are_visible() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(dispatch_only_dispatch_start_proof_required(
            &doc,
            &HarnessConfig::codex()
        ));
        let err = require_dispatch_only_dispatch_start_proof(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
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

        assert!(!dispatch_only_dispatch_start_proof_required(
            &doc,
            &HarnessConfig::codex()
        ));
        require_dispatch_only_dispatch_start_proof(
            &doc,
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::CommandAcceptedOnly,
        )
        .expect(
            "Codex without hook tracking may accept text+Enter delivery for dispatch-only reroutes",
        );

        let message = dispatch_only_test_sent_log_message(
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
    fn dispatch_blocker_recovery_hint_names_codex_hook_review_action() {
        let doc = PathBuf::from("tasks/agent-doc/agent-doc-bugs2.md");
        let hint = dispatch_blocker_recovery_hint(
            &HarnessConfig::codex(),
            "codex hook review prompt",
            &doc,
        );

        assert!(
            hint.contains("open `/hooks`"),
            "hook-review blockers should tell the operator where to approve hooks: {hint}"
        );
        assert!(
            hint.contains("approve or disable the pending hook change"),
            "hook-review blockers should describe the approval gate: {hint}"
        );
        assert!(
            hint.contains("agent-doc route --dispatch-only tasks/agent-doc/agent-doc-bugs2.md"),
            "hook-review blockers should include a reroute recovery command: {hint}"
        );

        let generic = dispatch_blocker_recovery_hint(
            &HarnessConfig::codex(),
            "queued draft in composer",
            &doc,
        );
        assert_eq!(generic, "restore an idle prompt and retry");
    }
    #[test]
    fn dispatch_active_turn_blockers_are_queueable_for_prompt_bearing_reroutes() {
        assert_eq!(
            dispatch_active_turn_queue_source(&HarnessConfig::codex(), "active codex turn"),
            Some("dispatch_only_codex_active_turn")
        );
        assert_eq!(
            dispatch_active_turn_queue_source(&HarnessConfig::opencode(), "opencode active turn"),
            Some("dispatch_only_opencode_active_turn")
        );
        // #jb-run-agent-doc-busy-wait-deadlock: a busy Claude active turn is
        // queueable just like Codex/OpenCode, so the reopen path enqueues instead of
        // bailing.
        assert_eq!(
            dispatch_active_turn_queue_source(&HarnessConfig::claude(), "active claude turn"),
            Some("dispatch_only_claude_active_turn")
        );
        assert_eq!(
            dispatch_active_turn_queue_source(&HarnessConfig::codex(), "codex hook review prompt"),
            None,
            "hook review requires an explicit operator decision, not auto-queueing"
        );
        assert_eq!(
            dispatch_active_turn_queue_source(&HarnessConfig::codex(), "queued draft in composer"),
            None,
            "drafted prompt input must not be overwritten by route queueing"
        );
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
            require_dispatch_only_dispatch_start_proof(
                &doc,
                "%4",
                &harness,
                DispatchOnlyReopenDelivery::DirectPaneSubmit,
                RoutedDispatchStartProof::CommandAcceptedOnly,
            )
            .expect("accepted-only delivery remains an explicit success path for this harness");
        }
    }
    #[test]
    fn dispatch_only_tracked_timeout_fails_closed_even_when_accepted_only_is_allowed() {
        let err = require_dispatch_only_dispatch_start_proof(
            Path::new("/tmp/agent-doc-bugs2.md"),
            "%4",
            &HarnessConfig::codex(),
            DispatchOnlyReopenDelivery::DirectPaneSubmit,
            RoutedDispatchStartProof::DispatchStartUnproven,
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
    }
    #[test]
    fn dispatch_only_sent_log_marks_claude_accepted_only_scope() {
        let message = dispatch_only_test_sent_log_message(
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
        let message = dispatch_only_test_sent_log_message(
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
        let message = dispatch_only_test_sent_log_message(
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
        )
        .unwrap();
    }
    #[test]
    fn dispatch_only_sent_log_marks_codex_hook_proof_scope() {
        let message = dispatch_only_test_sent_log_message(
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn dispatch_only_send_reopen_direct_pane_submit_avoids_extra_enter_retries() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-no-enter-retries");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-dispatch-only-no-enter-retries.md");
        std::fs::write(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let trigger = format!("agent-doc {}", file_path);
        let script = write_mock_registered_agent_doc_with_stale_trigger(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} '{}'", script.display(), trigger),
        );
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            &format!("> {}", trigger),
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains(&format!("> {}", trigger)),
            "mock session should keep a stale visible trigger line in pane output: {content}"
        );
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(
            dir.path(),
            "route-test-dispatch-only-no-enter-retries",
            move |method| match method {
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    injects_for_ipc.lock().unwrap().push(bytes.clone());
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Restart { .. }
                | IpcMethod::Stop { .. }
                | IpcMethod::StopAgent { .. }
                | IpcMethod::ReplicaRegister { .. }
                | IpcMethod::ReplicaDeregister { .. }
                | IpcMethod::ReplicaUpdate { .. }
                | IpcMethod::ReplicaPull { .. }
                | IpcMethod::ReplicaAck { .. }
                | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        sessions::register(
            "route-test-dispatch-only-no-enter-retries",
            &pane,
            &file_path,
        )
        .unwrap();
        dispatch_only_send_reopen(
            &iso,
            &doc,
            "route-test-dispatch-only-no-enter-retries",
            &pane,
            &file_path,
            &HarnessConfig::codex(),
            DispatchOnlySendReopenOptions {
                delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
                queue_prompt_text: None,
            },
        )
        .expect("dispatch-only reopen should still send once when no explicit blocker is visible");
        assert!(
            injects.lock().unwrap().is_empty(),
            "dispatch-only direct pane submit should not fall back to supervisor inject"
        );
        let after = wait_for_pane_contains(
            &iso,
            &pane,
            &format!("GOT:{trigger}"),
            std::time::Duration::from_secs(3),
        );
        assert!(
            after.contains(&format!("GOT:{trigger}")),
            "dispatch-only reopen should submit the trigger through the live pane input path: {after}"
        );
        assert!(
            !after.contains("EXTRA:"),
            "dispatch-only reopen should not send an extra newline or second Enter: {after}"
        );
        ipc.stop();
    }
}
