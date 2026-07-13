use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tmux_router::Tmux;

pub use agent_doc_controller::dispatch::DirectPaneSubmitStatus as CommandDispatchStatus;
use agent_doc_controller::dispatch::{
    DeadHarnessShellDispatchFacts, DirectPaneAcceptancePollState,
    DirectPaneEnterResubmitAttemptFacts, DirectPaneExistingDraftSubmitFacts,
    DirectPaneResubmitProofFacts, DispatchInjectLogFacts, RouteLatencyFacts, RouteLatencyStatus,
    RouteSubmitObservation, RouteSubmitObservationFacts as ControllerRouteSubmitObservationFacts,
    RoutedDispatchStartProof, RoutedTriggerPayloadFacts,
    classify_dead_harness_shell_dispatch_block, direct_pane_acceptance_poll_status,
    direct_pane_can_continue_enter_resubmit, direct_pane_can_enter_existing_draft,
    direct_pane_fast_accept_on_processing, direct_pane_max_enter_resubmits,
    direct_pane_resubmit_proof_line, direct_pane_submit_acceptance_budget,
    direct_pane_submit_acceptance_timeout, direct_pane_submit_outcome, dispatch_inject_log_line,
    recent_lines_contain_trigger, route_latency_message, route_latency_status,
    route_submit_issue_message, route_submit_observation_message,
    route_trigger_visible_in_current_draft, routed_trigger_payload_rejection,
};
use agent_doc_controller_io::route_snapshot::RoutePaneSnapshot;
use agent_doc_harness::{HarnessConfig, protected_prompt_draft_preview};
use agent_doc_hash::short_content_hash;
use agent_doc_supervisor::lifecycle::recycle_interrupted_resubmit_should_wait;
use agent_doc_tmux::pane_current_command_is_bare_shell;

/// Poll cadence for the direct-pane submit-acceptance check.
pub const DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDispatchResult {
    pub status: CommandDispatchStatus,
    pub elapsed: Duration,
    pub diagnostic_path: Option<PathBuf>,
}

/// Outcome of one direct-pane submit-acceptance poll window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectPaneAcceptance {
    pub status: CommandDispatchStatus,
    pub elapsed: Duration,
    /// Whether the trigger text was still visible in the pane when the window
    /// closed (only meaningful when `status == TimedOut`).
    pub trigger_visible: bool,
    pub diagnostic_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub struct RouteSubmitObservationLogFacts<'a> {
    pub file: &'a Path,
    pub pane: &'a str,
    pub harness: &'a HarnessConfig,
    pub phase: &'a str,
    pub observation: RouteSubmitObservation,
    pub trigger_visible: Option<bool>,
    pub elapsed: Duration,
    pub capture_len: Option<usize>,
    pub capture_hash: Option<&'a str>,
    pub proof: Option<RoutedDispatchStartProof>,
}

/// `#rdypoll` (§D / img_52): process-global count of real trigger injections
/// into a harness composer for this `agent-doc route` invocation.
static DISPATCH_INJECT_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub fn editor_route_attempt_id() -> Option<String> {
    agent_doc_controller_io::route_snapshot::editor_route_attempt_id()
}

pub fn preserve_route_pane_snapshot(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    phase: &str,
    content: &str,
) -> RoutePaneSnapshot {
    let outcome = agent_doc_controller_io::route_snapshot::preserve_route_pane_snapshot(
        file,
        pane,
        &harness.binary,
        phase,
        content,
        agent_doc_ops_log_io::log_op,
    );
    if let Some(err) = outcome.warning.as_deref() {
        eprintln!(
            "[route] warning: failed to preserve pane snapshot for {} phase {}: {}",
            file.display(),
            phase,
            err
        );
    }
    outcome.snapshot
}

pub fn print_route_pane_snapshot_hint(
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    phase: &str,
    snapshot: &RoutePaneSnapshot,
) {
    let message = agent_doc_controller_io::route_snapshot::route_pane_snapshot_hint(
        file,
        pane,
        &harness.binary,
        phase,
        snapshot,
    );
    eprintln!("{message}");
}

pub fn log_route_submit_observation(facts: RouteSubmitObservationLogFacts<'_>) {
    let file_display = facts.file.display().to_string();
    let editor_attempt_id = editor_route_attempt_id();
    let controller_facts = ControllerRouteSubmitObservationFacts {
        file_display: &file_display,
        pane: facts.pane,
        harness_binary: &facts.harness.binary,
        phase: facts.phase,
        observation: facts.observation,
        trigger_visible: facts.trigger_visible,
        elapsed_ms: facts.elapsed.as_millis(),
        capture_len: facts.capture_len,
        capture_hash: facts.capture_hash,
        proof: facts.proof,
        editor_attempt_id: editor_attempt_id.as_deref(),
    };
    agent_doc_ops_log_io::log_op(
        facts.file,
        &route_submit_observation_message(controller_facts),
    );
    if let Some(issue) = route_submit_issue_message(controller_facts) {
        agent_doc_ops_log_io::log_op(facts.file, &issue);
    }
}

pub fn log_route_latency(
    file: &Path,
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    pane: &str,
    harness: &HarnessConfig,
    outcome: &str,
) {
    let editor_attempt_id = editor_route_attempt_id();
    let elapsed_ms = elapsed.as_millis();
    let budget_ms = budget.as_millis();
    let message = route_latency_message(RouteLatencyFacts {
        phase,
        elapsed_ms,
        budget_ms,
        pane,
        harness_binary: &harness.binary,
        outcome,
        editor_attempt_id: editor_attempt_id.as_deref(),
    });
    agent_doc_ops_log_io::log_op(file, &message);
    if route_latency_status(elapsed_ms, budget_ms) == RouteLatencyStatus::OverBudget {
        eprintln!(
            "[route] latency budget exceeded: phase {} took {}ms (budget {}ms, pane={}, harness={}, outcome={})",
            phase,
            elapsed.as_millis(),
            budget.as_millis(),
            pane,
            harness.binary,
            outcome
        );
    }
}

/// Record one real trigger injection and emit the `dispatch_inject attempt=N`
/// marker.
pub fn log_dispatch_inject(file: &Path, pane: &str, harness: &HarnessConfig, transport: &str) {
    let attempt = DISPATCH_INJECT_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let file_display = file.display().to_string();
    agent_doc_ops_log_io::log_op(
        file,
        &dispatch_inject_log_line(DispatchInjectLogFacts {
            file_display: &file_display,
            pane,
            harness_binary: &harness.binary,
            transport,
            attempt,
        }),
    );
}

/// Poll the pane capture until the trigger text is consumed or the acceptance
/// window expires. Pure detection: it never sends input.
pub fn poll_direct_pane_acceptance(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
) -> DirectPaneAcceptance {
    let start = std::time::Instant::now();
    let timeout = direct_pane_submit_acceptance_timeout();
    let poll_interval = DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL;
    let mut last_capture: Option<(bool, usize, String, String)> = None;
    let mut poll_state = DirectPaneAcceptancePollState::default();
    let mut capture_failed = false;
    while start.elapsed() < timeout {
        match agent_doc_tmux_io::capture_pane(tmux, pane) {
            Ok(content) => {
                let elapsed = start.elapsed();
                let cmd_still_in_input = recent_lines_contain_trigger(&content, trigger);
                let capture_hash = short_content_hash(&content);
                let capture_len = content.len();
                last_capture = Some((cmd_still_in_input, capture_len, capture_hash, content));

                if direct_pane_acceptance_poll_status(&mut poll_state, elapsed, cmd_still_in_input)
                    .is_some()
                {
                    let capture_hash = last_capture.as_ref().map(|(_, _, hash, _)| hash.as_str());
                    log_route_submit_observation(RouteSubmitObservationLogFacts {
                        file,
                        pane,
                        harness,
                        phase,
                        observation: RouteSubmitObservation::Accepted,
                        trigger_visible: Some(false),
                        elapsed,
                        capture_len: Some(capture_len),
                        capture_hash,
                        proof: None,
                    });
                    return DirectPaneAcceptance {
                        status: CommandDispatchStatus::Accepted,
                        elapsed,
                        trigger_visible: false,
                        diagnostic_path: None,
                    };
                }

                let pane_busy = last_capture
                    .as_ref()
                    .map(|(_, _, _, content)| harness.has_busy_cue(content))
                    .unwrap_or(false);
                if direct_pane_fast_accept_on_processing(
                    cmd_still_in_input,
                    poll_state.saw_trigger_visible(),
                    pane_busy,
                ) {
                    let capture_hash = last_capture.as_ref().map(|(_, _, hash, _)| hash.as_str());
                    log_route_submit_observation(RouteSubmitObservationLogFacts {
                        file,
                        pane,
                        harness,
                        phase,
                        observation: RouteSubmitObservation::Accepted,
                        trigger_visible: Some(false),
                        elapsed,
                        capture_len: Some(capture_len),
                        capture_hash,
                        proof: None,
                    });
                    return DirectPaneAcceptance {
                        status: CommandDispatchStatus::Accepted,
                        elapsed,
                        trigger_visible: false,
                        diagnostic_path: None,
                    };
                }
            }
            Err(_) => {
                capture_failed = true;
            }
        }
        std::thread::sleep(poll_interval);
    }
    let elapsed = start.elapsed();
    let trigger_visible = last_capture
        .as_ref()
        .map(|(visible, _, _, _)| *visible)
        .unwrap_or(false);
    let mut diagnostic_path = None;
    if let Some((visible, capture_len, capture_hash, content)) = last_capture.as_ref() {
        if *visible {
            diagnostic_path =
                preserve_route_pane_snapshot(file, pane, harness, phase, content).path;
        }
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase,
            observation: if *visible {
                RouteSubmitObservation::TriggerStillVisible
            } else {
                RouteSubmitObservation::Accepted
            },
            trigger_visible: Some(*visible),
            elapsed,
            capture_len: Some(*capture_len),
            capture_hash: Some(capture_hash.as_str()),
            proof: None,
        });
    } else if capture_failed {
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase,
            observation: RouteSubmitObservation::CaptureFailed,
            trigger_visible: None,
            elapsed,
            capture_len: None,
            capture_hash: None,
            proof: None,
        });
    }
    DirectPaneAcceptance {
        status: CommandDispatchStatus::TimedOut,
        elapsed,
        trigger_visible,
        diagnostic_path,
    }
}

pub fn send_direct_pane_enter_resubmit(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
    attempt: usize,
) -> DirectPaneAcceptance {
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.direct_pane_resubmit",
        &format!("pane:{pane}"),
        "",
        Some(&harness.binary),
        "routed_resubmit_submit_key",
        submit_key,
    );
    if let Err(e) = agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        tmux,
        pane,
        "",
        &harness.binary,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    ) {
        eprintln!(
            "[route] warning: {} resubmit {} failed for pane {}: {}",
            harness.binary, submit_key, pane, e
        );
    }
    let second = poll_direct_pane_acceptance(tmux, pane, file, harness, trigger, phase);
    let file_display = file.display().to_string();
    let editor_attempt_id = editor_route_attempt_id();
    agent_doc_ops_log_io::log_op(
        file,
        &direct_pane_resubmit_proof_line(DirectPaneResubmitProofFacts {
            file_display: &file_display,
            pane,
            harness_binary: &harness.binary,
            submit_key,
            status: second.status,
            elapsed_ms: second.elapsed.as_millis(),
            attempt,
            editor_attempt_id: editor_attempt_id.as_deref(),
        }),
    );
    second
}

pub fn send_direct_pane_enter_resubmit_until_stable(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
    initial: DirectPaneAcceptance,
) -> DirectPaneAcceptance {
    let mut status = initial.status;
    let mut trigger_visible = initial.trigger_visible;
    let mut elapsed = initial.elapsed;
    let mut diagnostic_path = initial.diagnostic_path;
    let mut attempts_sent = 0usize;
    let profile_allows_pending_draft_enter_resubmit =
        agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
            .pending_draft_enter_resubmit();
    let max_attempts = direct_pane_max_enter_resubmits();

    while direct_pane_can_continue_enter_resubmit(DirectPaneEnterResubmitAttemptFacts {
        profile_allows_pending_draft_enter_resubmit,
        status,
        trigger_visible,
        attempts_sent,
        max_attempts,
    }) {
        attempts_sent += 1;
        let retry = send_direct_pane_enter_resubmit(
            tmux,
            pane,
            file,
            harness,
            trigger,
            phase,
            attempts_sent,
        );
        elapsed += retry.elapsed;
        status = retry.status;
        trigger_visible = retry.trigger_visible;
        if retry.diagnostic_path.is_some() {
            diagnostic_path = retry.diagnostic_path;
        }
    }

    DirectPaneAcceptance {
        status,
        elapsed,
        trigger_visible,
        diagnostic_path,
    }
}

/// Returns the shell command name when route must fail closed instead of
/// dispatching into a dead harness shell.
pub fn dead_harness_shell_dispatch_block(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> Option<String> {
    let bare_shell_command = agent_doc_tmux_io::target_current_command(tmux, pane)
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .filter(|cmd| pane_current_command_is_bare_shell(cmd))?;
    let pane_shows_harness_prompt = agent_doc_tmux_io::capture_pane(tmux, pane)
        .ok()
        .and_then(|content| harness.last_prompt_candidate(&content))
        .map(|line| harness.is_dispatch_ready_prompt_line(&line))
        .unwrap_or(false);
    classify_dead_harness_shell_dispatch_block(DeadHarnessShellDispatchFacts {
        pane_shows_harness_prompt,
        bare_shell_command: Some(bare_shell_command),
    })
}

pub fn send_command_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    let file = Path::new(file_path);
    if let Some(shell) = dead_harness_shell_dispatch_block(tmux, pane, harness) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_into_dead_shell_blocked file={} pane={} harness={} pane_current_command={} reason=harness_exited_to_bare_shell",
                file.display(),
                pane,
                harness.binary,
                shell
            ),
        );
        anyhow::bail!(
            "route refusing to dispatch {} into pane {}: harness '{}' is not running (pane is a bare '{}' shell). The harness crashed/exited — claim/restart the harness before routing.",
            harness.trigger_command(file_path),
            pane,
            harness.binary,
            shell
        );
    }
    let trigger = harness.trigger_command(file_path);
    let payload = trigger.to_string();
    if let Some(rejection) = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
        harness_binary: &harness.binary,
        trigger: &trigger,
        payload: &payload,
    }) {
        anyhow::bail!("{rejection}");
    }
    let mut existing_draft_diagnostic_path = None;
    let mut protected_prompt_input = None;
    let existing_draft_visible = match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => {
            let visible = route_trigger_visible_in_current_draft(&content, &trigger, |line| {
                harness.is_prompt_line(line)
            });
            if visible {
                existing_draft_diagnostic_path = preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "direct_pane_existing_draft_visible",
                    &content,
                )
                .path;
            } else if let Some(reason) = harness.protected_prompt_input_reason(&content) {
                let diagnostic_path = preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "direct_pane_protected_prompt_input",
                    &content,
                )
                .path;
                let draft_preview = protected_prompt_draft_preview(harness, &content);
                protected_prompt_input = Some((reason, diagnostic_path, draft_preview));
            }
            visible
        }
        Err(e) => {
            eprintln!(
                "[route] warning: failed to capture pane {} before direct submit: {}",
                pane, e
            );
            false
        }
    };
    if let Some((reason, diagnostic_path, draft_preview)) = protected_prompt_input {
        let draft_preview_field = draft_preview
            .as_deref()
            .map(|preview| format!(" draft_preview={preview:?}"))
            .unwrap_or_default();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_direct_pane_blocked file={} pane={} harness={} protected_input={}{}",
                file.display(),
                pane,
                harness.binary,
                reason,
                draft_preview_field,
            ),
        );
        let diagnostic = diagnostic_path
            .as_ref()
            .map(|path| format!(" snapshot_path={}", path.display()))
            .unwrap_or_default();
        anyhow::bail!(
            "route refusing to dispatch {} into pane {} for {} because the composer contains protected prompt input ({}){}; clear or submit that draft, then rerun agent-doc route{}",
            harness.trigger_command(file_path),
            pane,
            file.display(),
            reason,
            draft_preview_field,
            diagnostic,
        );
    }
    if direct_pane_can_enter_existing_draft(DirectPaneExistingDraftSubmitFacts {
        profile_allows_pending_draft_enter_resubmit:
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
                .pending_draft_enter_resubmit(),
        trigger_visible: existing_draft_visible,
    }) {
        let first = send_direct_pane_enter_resubmit_until_stable(
            tmux,
            pane,
            file,
            harness,
            &trigger,
            "direct_pane_existing_draft_acceptance",
            DirectPaneAcceptance {
                status: CommandDispatchStatus::TimedOut,
                elapsed: Duration::ZERO,
                trigger_visible: true,
                diagnostic_path: existing_draft_diagnostic_path,
            },
        );
        return Ok(CommandDispatchResult {
            status: first.status,
            elapsed: first.elapsed,
            diagnostic_path: first.diagnostic_path,
        });
    }

    let trigger = send_command_once_unchecked(tmux, pane, file_path, harness)?;
    let mut acceptance = poll_direct_pane_acceptance(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "direct_pane_acceptance",
    );

    if acceptance.status != CommandDispatchStatus::Accepted
        && recycle_interrupted_resubmit_should_wait(
            true,
            agent_doc_controller_io::project_controller::supervisor_recycle_pending_for_file(
                std::path::Path::new(file_path),
            ),
        )
    {
        let settled =
            agent_doc_controller_io::project_controller::wait_for_supervisor_recycle_settle_for_file(
                std::path::Path::new(file_path),
            )
            .is_ok();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_submit_recycle_settle file={} pane={} harness={} settled={} action=submit_once_after_settle",
                file.display(),
                pane,
                harness.binary,
                settled
            ),
        );
        acceptance = poll_direct_pane_acceptance(
            tmux,
            pane,
            file,
            harness,
            &trigger,
            "direct_pane_post_recycle_acceptance",
        );
    }

    // The full trigger has crossed the tmux transport exactly once. If a fast
    // harness consumes it between captures, absence is ambiguous and the outer
    // dispatch-start proof owns the decision. Only an exact visible draft may
    // receive bounded bare-Enter recovery below.
    if acceptance.status == CommandDispatchStatus::Accepted {
        return Ok(CommandDispatchResult {
            status: acceptance.status,
            elapsed: acceptance.elapsed,
            diagnostic_path: acceptance.diagnostic_path,
        });
    }

    let second = send_direct_pane_enter_resubmit_until_stable(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "direct_pane_resubmit_acceptance",
        acceptance,
    );

    Ok(CommandDispatchResult {
        status: second.status,
        elapsed: second.elapsed,
        diagnostic_path: second.diagnostic_path,
    })
}

pub fn send_command_once_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<String> {
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let payload = trigger.to_string();
    if let Some(rejection) = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
        harness_binary: &harness.binary,
        trigger: &trigger,
        payload: &payload,
    }) {
        anyhow::bail!("{rejection}");
    }
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = agent_doc_tmux_io::show_message(tmux, pane, "2000", &flash_msg) {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let transform = agent_doc_tmux_commands::tmux_submit_transform_for_harness(&harness.binary);
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(
            Some(Path::new(file_path)),
            agent_doc_ops_log_io::log_op,
        ),
        "route.direct_pane_submit",
        &format!("pane:{pane}"),
        &payload,
        Some(&harness.binary),
        transform,
        submit_key,
    );
    log_dispatch_inject(Path::new(file_path), pane, harness, "direct_pane");
    agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        tmux,
        pane,
        &payload,
        &harness.binary,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    )?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent {} → pane {}", trigger, pane);
    Ok(trigger)
}

pub fn try_late_direct_pane_enter_resubmit_after_unproven_dispatch(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    timeout: Duration,
    mut wait_for_dispatch_start: impl FnMut() -> Result<Option<RoutedDispatchStartProof>>,
) -> Result<Option<RoutedDispatchStartProof>> {
    let trigger = harness.trigger_command(file_path);
    if let Some(shell) = agent_doc_tmux_io::target_current_command(tmux, pane)
        .map(|command| command.trim().to_string())
        .filter(|command| pane_current_command_is_bare_shell(command))
    {
        if let Ok(content) = agent_doc_tmux_io::capture_pane(tmux, pane) {
            preserve_route_pane_snapshot(
                file,
                pane,
                harness,
                "dispatch_start_unproven_late_resubmit_refused_bare_shell",
                &content,
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_submit_late_resubmit_refused file={} pane={} harness={} pane_current_command={} reason=harness_exited_to_bare_shell",
                file.display(),
                pane,
                harness.binary,
                shell,
            ),
        );
        return Ok(None);
    }
    let visible = match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => {
            let visible = route_trigger_visible_in_current_draft(&content, &trigger, |line| {
                harness.is_prompt_line(line)
            });
            let harness_surface_visible = late_resubmit_harness_surface_visible(&content, harness);
            if visible {
                preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "dispatch_start_unproven_late_draft_visible",
                    &content,
                );
            }
            if visible && !harness_surface_visible {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_submit_late_resubmit_refused file={} pane={} harness={} reason=harness_surface_not_visible",
                        file.display(),
                        pane,
                        harness.binary,
                    ),
                );
                false
            } else {
                visible
            }
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} before late direct-submit retry: {}",
                pane, err
            );
            false
        }
    };
    if !direct_pane_can_enter_existing_draft(DirectPaneExistingDraftSubmitFacts {
        profile_allows_pending_draft_enter_resubmit:
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
                .pending_draft_enter_resubmit(),
        trigger_visible: visible,
    }) {
        return Ok(None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_submit_late_resubmit file={} pane={} harness={} cause=dispatch_start_unproven_prompt_visible",
            file.display(),
            pane,
            harness.binary,
        ),
    );
    let retry = send_direct_pane_enter_resubmit(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "dispatch_start_unproven_late_draft_acceptance",
        1,
    );
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_dispatch_start()? {
        log_route_latency(
            file,
            "direct_pane_late_resubmit",
            retry.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(retry.status, Some(proof)),
        );
        log_route_latency(
            file,
            "dispatch_start_proof_after_late_resubmit",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase: "dispatch_start_proof_after_late_resubmit",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed: proof_start.elapsed(),
            capture_len: None,
            capture_hash: None,
            proof: Some(proof),
        });
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_start_late_resubmit_proven file={} pane={} harness={} dispatch_stage={} timeout_secs={} retry=late_enter",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(Some(proof));
    }

    log_route_latency(
        file,
        "direct_pane_late_resubmit",
        retry.elapsed,
        direct_pane_submit_acceptance_budget(),
        pane,
        harness,
        direct_pane_submit_outcome(retry.status, None),
    );
    log_route_latency(
        file,
        "dispatch_start_proof_after_late_resubmit",
        proof_start.elapsed(),
        timeout,
        pane,
        harness,
        "late_resubmit_unproven",
    );
    Ok(None)
}

fn late_resubmit_harness_surface_visible(content: &str, harness: &HarnessConfig) -> bool {
    let recent: Vec<&str> = content
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(12)
        .collect();
    harness.has_busy_cue(content)
        || harness.output_prompt_visible(content)
        || recent.iter().any(|line| harness.is_idle_status_line(line))
        || (harness.binary == "codex"
            && recent.iter().any(|line| {
                let line = line.trim();
                line.starts_with("gpt-") && line.contains('·')
            }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_pane_snapshot_preserves_redacted_terminal_capture() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("session.md");
        std::fs::write(&file, "session").unwrap();
        let content = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
OPENAI_API_KEY=sk-proj-aaaaaaaaaaaaaaaaaaaaaaaa
";

        let snapshot = preserve_route_pane_snapshot(
            &file,
            "%7",
            &HarnessConfig::codex(),
            "direct_pane_acceptance",
            content,
        );

        let path = snapshot.path.expect("snapshot path should be preserved");
        assert!(path.starts_with(tmp.path().join(".agent-doc/logs/route-submit")));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("OPENAI_API_KEY=[REDACTED]"),
            "snapshot should redact named API keys: {saved}"
        );
        assert!(
            !saved.contains("sk-proj-aaaaaaaa"),
            "raw token must not be preserved in snapshot: {saved}"
        );

        let ops = std::fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("route_pane_snapshot"), "{ops}");
        assert!(ops.contains("phase=direct_pane_acceptance"), "{ops}");
        assert!(ops.contains("capture_hash="), "{ops}");
        assert!(ops.contains("snapshot_path="), "{ops}");
    }

    #[test]
    fn late_resubmit_requires_current_codex_surface() {
        let harness = HarnessConfig::codex();
        let tui = "\
› agent-doc /tmp/session.md

  gpt-5.6-sol xhigh · ~/work · Context 42% used
";
        assert!(late_resubmit_harness_surface_visible(tui, &harness));

        let exited_shell = "\
To continue this session, run codex resume --last
brian@host repo% agent-doc /tmp/session.md
Error: bare agent-doc must run from a supported harness
brian@host repo%
";
        assert!(!late_resubmit_harness_surface_visible(
            exited_shell,
            &harness
        ));
    }
}
