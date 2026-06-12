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
        "dispatch-only {} reopen refused to inject into pane {} for {} because the latest run is still booting and never reached a dispatch-ready prompt ({detail}); wait for the pane to become ready and reroute again",
        harness.binary,
        pane,
        file.display()
    )
}

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
    let mut dispatch_pane = pane.to_string();
    let mut log_status = crate::startup_miss::session_log_status(file, session_id)
        .ok()
        .flatten();
    let mut recovery_attempts = 0usize;
    let requires_ready_probe =
        dispatch_only_requires_ready_probe(log_status.as_ref(), &dispatch_pane, harness);
    if requires_ready_probe {
        loop {
            let ready_outcome = wait_for_agent_ready_outcome(
                tmux,
                &dispatch_pane,
                dispatch_only_starting_pane_ready_timeout(harness),
                harness,
            );
            if ready_outcome.is_ready() {
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
                "[route] dispatch-only {} reopen for {} found {} on pane {}; queued pending dispatch {:?} in active agent:queue (appended={}, already_present={}, superseded={}) instead of injecting a duplicate trigger",
                harness.binary,
                file.display(),
                reason,
                dispatch_pane,
                queued.prompt_text,
                queued.appended,
                queued.already_present,
                queued.superseded
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
                await_start_proof: true,
                print_unproven_progress: should_print_dispatch_only_unproven_progress(
                    file, harness,
                ),
            },
        )?,
        DispatchOnlyReopenDelivery::DirectPaneSubmit => dispatch_routed_reopen_with_mode(
            tmux,
            file,
            &dispatch_pane,
            file_path,
            harness,
            should_print_dispatch_only_unproven_progress(file, harness),
        )?,
    };
    require_dispatch_only_dispatch_start_proof(
        file,
        &dispatch_pane,
        harness,
        delivery,
        dispatch_start,
    )?;
    crate::ops_log::log_op(
        file,
        &route_dispatch_only_sent_log_message(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
        ),
    );
    eprintln!(
        "{}",
        route_dispatch_only_sent_console_message(
            file,
            &dispatch_pane,
            harness,
            delivery,
            dispatch_start,
        )
    );
    Ok(dispatch_pane)
}

pub(crate) fn should_print_dispatch_only_unproven_progress(file: &Path, harness: &HarnessConfig) -> bool {
    flow_should_print_dispatch_only_unproven_progress(DispatchOnlyProofPolicyFacts {
        harness_binary: harness.binary.as_str(),
        codex_dispatch_start_tracking_enabled: codex_dispatch_start_tracking_enabled(file),
    })
}

pub(crate) fn dispatch_only_dispatch_start_proof_required(file: &Path, harness: &HarnessConfig) -> bool {
    flow_dispatch_only_dispatch_start_proof_required(DispatchOnlyProofPolicyFacts {
        harness_binary: harness.binary.as_str(),
        codex_dispatch_start_tracking_enabled: codex_dispatch_start_tracking_enabled(file),
    })
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

    let timeout = routed_dispatch_start_timeout(harness).as_secs();
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
    crate::ops_log::log_op(file, &accepted_only_dispatch_start_log_message(facts));
    anyhow::bail!(accepted_only_dispatch_start_refusal_message(facts));
}

pub(crate) fn route_dispatch_only_sent_log_message(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_log_message(DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout(harness).as_secs(),
    })
}

pub(crate) fn route_dispatch_only_sent_console_message(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    delivery: DispatchOnlyReopenDelivery,
    dispatch_start: RoutedDispatchStartProof,
) -> String {
    let file_display = file.display().to_string();
    dispatch_only_sent_console_message(DispatchOnlyProofOutcomeFacts {
        file_display: file_display.as_str(),
        pane,
        harness_binary: harness.binary.as_str(),
        delivery,
        dispatch_start,
        timeout_secs: routed_dispatch_start_timeout(harness).as_secs(),
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
        match wait_for_managed_capability_proof(
            file,
            session_id,
            harness,
            fresh_route_start_ack_timeout(),
        )? {
            ManagedCapabilityProofStatus::NotRequired | ManagedCapabilityProofStatus::Proven => {}
            ManagedCapabilityProofStatus::Pending => anyhow::bail!(
                "dispatch-only {} reopen for {} on pane {} is gated because managed capability proof is still pending after waiting {}s",
                harness.binary,
                file.display(),
                dispatch_pane,
                fresh_route_start_ack_timeout().as_secs()
            ),
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

pub(crate) fn dispatch_only_blocker_reason(harness: &HarnessConfig, content: &str) -> Option<String> {
    if let Some(reason) = harness.dispatch_blocker_reason(content) {
        return Some(reason);
    }
    if harness.binary != "codex" {
        return None;
    }

    let normalized = crate::prompt::strip_ansi(content).to_ascii_lowercase();
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

pub(crate) fn dispatch_blocker_recovery_hint(harness: &HarnessConfig, reason: &str, file: &Path) -> String {
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
