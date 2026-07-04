//! Route dispatch transport and dispatch-start proof I/O.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::direct_pane_dispatch::{
    CommandDispatchResult, CommandDispatchStatus, RouteSubmitObservationLogFacts,
    log_dispatch_inject, log_route_latency, log_route_submit_observation,
    preserve_route_pane_snapshot, print_route_pane_snapshot_hint, send_command_unchecked,
    try_late_direct_pane_enter_resubmit_after_unproven_dispatch,
};
use crate::dispatch_start::{
    RoutedDispatchStartTracker, build_routed_dispatch_start_tracker, wait_for_routed_dispatch_start,
};
use crate::supervisor_runtime::supervisor_socket_path;
use agent_doc_controller::dispatch::{
    DirectPaneDispatchStartProofFacts, RouteSubmitObservation, RoutedDispatchStartProof,
    RoutedTriggerPayloadFacts, busy_dispatch_start_outcome,
    direct_pane_should_await_dispatch_start_proof, direct_pane_submit_acceptance_budget,
    direct_pane_submit_outcome, dispatch_start_busy_probe_timeout,
    routed_dispatch_start_timeout_for_binary, routed_trigger_payload_rejection,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry;
use agent_doc_supervisor::ipc_protocol::IpcMethod;
use tmux_router::Tmux;

#[derive(Debug, Clone, Copy)]
pub struct RouteDispatchEffects {
    pub file_route_dispatch_bug_report: for<'a> fn(RouteDispatchBugReportFacts<'a>),
    pub emit_busy_route_queued_diagnostic: for<'a> fn(BusyRouteQueuedDiagnosticFacts<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct RouteDispatchBugReportFacts<'a> {
    pub file: &'a Path,
    pub pane: &'a str,
    pub harness: &'a HarnessConfig,
    pub phase: &'a str,
    pub issue: &'a str,
    pub result: &'a str,
    pub elapsed: Duration,
    pub proof: Option<RoutedDispatchStartProof>,
    pub diagnostic_path: Option<&'a Path>,
}

#[derive(Clone, Copy)]
pub struct BusyRouteQueuedDiagnosticFacts<'a> {
    pub tmux: &'a Tmux,
    pub pane: &'a str,
    pub file: &'a Path,
    pub harness: &'a HarnessConfig,
}

/// If the target pane is mid-turn, the routed trigger is queued behind that
/// active turn and harness dispatch-start proof cannot arrive within the proof
/// budget. Detect that up front and short-circuit to a queued outcome instead
/// of blocking on the full `wait_for_routed_dispatch_start` budget (twice, via
/// the late-resubmit retry) and then filing a false
/// `accepted_without_dispatch_start_proof` bug (#kjw0 / #jbrunautobug).
///
/// Returns `Ok(Some(proof))` when the pane is busy — either because our own
/// routed prompt already produced dispatch-start proof (busy because it
/// started) or because the trigger is queued behind a prior active turn
/// (`AcceptedQueuedBehindActiveTurn`). Returns `Ok(None)` when the pane is not
/// mid-turn, so the caller proceeds to the normal proof wait. On pane-capture
/// failure it returns `Ok(None)` (never falsely claims queued).
fn short_circuit_dispatch_start_when_pane_busy(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    tracker: &RoutedDispatchStartTracker,
    effects: RouteDispatchEffects,
) -> Result<Option<RoutedDispatchStartProof>> {
    let content = match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => content,
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} for busy dispatch-start check: {}",
                pane, err
            );
            return Ok(None);
        }
    };
    let Some(busy_line) = harness.busy_proof_line(&content) else {
        return Ok(None);
    };
    // The pane is mid-turn. Give the harness one short chance to prove the
    // active turn IS our routed prompt (so a real proof is not discarded),
    // then short-circuit to the queued outcome.
    let probe_proof = wait_for_routed_dispatch_start(
        tmux,
        file,
        tracker,
        harness,
        dispatch_start_busy_probe_timeout(cfg!(test)),
    )?;
    let outcome = busy_dispatch_start_outcome(true, probe_proof);
    if outcome == Some(RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn) {
        (effects.emit_busy_route_queued_diagnostic)(BusyRouteQueuedDiagnosticFacts {
            tmux,
            pane,
            file,
            harness,
        });
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_start_queued_behind_active_turn file={} pane={} harness={} busy_proof={:?}",
                file.display(),
                pane,
                harness.binary,
                busy_line.trim(),
            ),
        );
    }
    Ok(outcome)
}

pub fn dispatch_via_supervisor_ipc_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: SupervisorIpcDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
    let effects = options.effects;
    let Some(sock) = supervisor_socket_path(file, session_id) else {
        anyhow::bail!(
            "authoritative actor for {} has no supervisor socket; run `agent-doc start {}` to recover",
            file.display(),
            file.display()
        );
    };
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

    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let _route_submit_guard = agent_doc_controller_io::project_controller::begin_route_submit(
        file,
        pane,
        &harness.binary,
    )?;
    let method = IpcMethod::Inject {
        bytes: agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(&payload)
            .to_string(),
    };
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.supervisor_ipc",
        &format!("socket:{}:pane:{pane}", sock.display()),
        &payload,
        Some(&harness.binary),
        "supervisor_ipc_inject",
        "Inject",
    );
    log_dispatch_inject(file, pane, harness, "supervisor_ipc");
    let submit_start = Instant::now();
    let response =
        agent_doc_supervisor_io::ipc::send_command(&sock, &method).with_context(|| {
            format!(
                "failed to dispatch authoritative actor trigger for {} via supervisor IPC",
                file.display()
            )
        })?;
    log_route_latency(
        file,
        "supervisor_ipc_submit",
        submit_start.elapsed(),
        Duration::from_millis(500),
        pane,
        harness,
        if response.ok { "accepted" } else { "rejected" },
    );
    if !response.ok {
        let message = response
            .error
            .unwrap_or_else(|| "unknown supervisor error".to_string());
        anyhow::bail!(
            "authoritative actor for {} rejected routed trigger in pane {}: {}",
            file.display(),
            pane,
            message
        );
    }

    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!(
        "[route] Dispatched {} via supervisor IPC → pane {}",
        trigger, pane
    );

    if !options.await_start_proof {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    }

    let Some(tracker) = tracker else {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };

    // Busy short-circuit (#kjw0 / #jbrunautobug): if the pane is mid-turn the
    // routed trigger is queued behind that active turn and dispatch-start proof
    // cannot arrive within budget — resolve to a queued outcome instead of
    // burning the full proof budget and filing a false unproven bug.
    if let Some(proof) =
        short_circuit_dispatch_start_when_pane_busy(tmux, file, pane, harness, &tracker, effects)?
    {
        return Ok(proof);
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "dispatch_start_proof",
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
            phase: "supervisor_dispatch_start_proof",
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
                "route_actor_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "dispatch_start_proof",
        proof_start.elapsed(),
        timeout,
        pane,
        harness,
        "unproven_but_accepted",
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_actor_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            timeout.as_secs()
        ),
    );
    let diagnostic_path = match agent_doc_tmux_io::capture_pane(tmux, pane) {
        Ok(content) => {
            let snapshot = preserve_route_pane_snapshot(
                file,
                pane,
                harness,
                "supervisor_dispatch_start_unproven",
                &content,
            );
            print_route_pane_snapshot_hint(
                file,
                pane,
                harness,
                "supervisor_dispatch_start_unproven",
                &snapshot,
            );
            snapshot.path
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} after unproven supervisor dispatch: {}",
                pane, err
            );
            None
        }
    };
    log_route_submit_observation(RouteSubmitObservationLogFacts {
        file,
        pane,
        harness,
        phase: "supervisor_dispatch_start_proof",
        observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
        trigger_visible: None,
        elapsed: proof_start.elapsed(),
        capture_len: None,
        capture_hash: None,
        proof: None,
    });
    (effects.file_route_dispatch_bug_report)(RouteDispatchBugReportFacts {
        file,
        pane,
        harness,
        phase: "supervisor_dispatch_start_proof",
        issue: "accepted_without_dispatch_start_proof",
        result: RouteSubmitObservation::AcceptedWithoutDispatchProof.label(),
        elapsed: proof_start.elapsed(),
        proof: None,
        diagnostic_path: diagnostic_path.as_deref(),
    });
    if options.print_unproven_progress {
        eprintln!(
            "[route] authoritative actor accepted the {} reopen for {} in pane {}, but no routed submission proof appeared after {}s",
            harness.binary,
            file.display(),
            pane,
            timeout.as_secs()
        );
    }
    Ok(RoutedDispatchStartProof::DispatchStartUnproven)
}

#[derive(Debug, Clone, Copy)]
pub struct SupervisorIpcDispatchOptions {
    pub effects: RouteDispatchEffects,
    pub await_start_proof: bool,
    pub print_unproven_progress: bool,
}

pub fn dispatch_via_supervisor_ipc(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    effects: RouteDispatchEffects,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc_with_mode(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        SupervisorIpcDispatchOptions {
            effects,
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

pub fn send_command_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    dispatch_registry::ensure_dispatch_target_matches_file(pane, file_path)?;
    send_command_unchecked(tmux, pane, file_path, harness)
}

pub fn dispatch_existing_managed_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    effects: RouteDispatchEffects,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc(tmux, file, pane, session_id, file_path, harness, effects)
}

pub fn dispatch_routed_reopen(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    effects: RouteDispatchEffects,
) -> Result<RoutedDispatchStartProof> {
    dispatch_routed_reopen_with_mode(
        tmux,
        file,
        pane,
        file_path,
        harness,
        DirectPaneDispatchOptions {
            effects,
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

pub fn dispatch_routed_reopen_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: DirectPaneDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
    let effects = options.effects;
    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let _route_submit_guard = agent_doc_controller_io::project_controller::begin_route_submit(
        file,
        pane,
        &harness.binary,
    )?;
    let submit_result = send_command_checked(tmux, pane, file_path, harness)?;
    let Some(tracker) = tracker else {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, None),
        );
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };
    if !direct_pane_should_await_dispatch_start_proof(DirectPaneDispatchStartProofFacts {
        await_start_proof: options.await_start_proof,
        submit_status: submit_result.status,
    }) {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, None),
        );
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    }

    // Busy short-circuit (#kjw0 / #jbrunautobug): if the pane is mid-turn the
    // routed trigger is queued behind that active turn and dispatch-start proof
    // cannot arrive within budget — resolve to a queued outcome instead of
    // burning the full proof budget (twice, via late-resubmit) and filing a
    // false accepted_without_dispatch_start_proof bug.
    if let Some(proof) =
        short_circuit_dispatch_start_when_pane_busy(tmux, file, pane, harness, &tracker, effects)?
    {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, Some(proof)),
        );
        return Ok(proof);
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, Some(proof)),
        );
        log_route_latency(
            file,
            "dispatch_start_proof",
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
            phase: "dispatch_start_proof",
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
                "route_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "direct_pane_submit",
        submit_result.elapsed,
        direct_pane_submit_acceptance_budget(),
        pane,
        harness,
        direct_pane_submit_outcome(submit_result.status, None),
    );
    match submit_result.status {
        CommandDispatchStatus::Accepted => {
            if let Some(proof) = try_late_direct_pane_enter_resubmit_after_unproven_dispatch(
                tmux,
                file,
                pane,
                file_path,
                harness,
                timeout,
                || wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout),
            )? {
                return Ok(proof);
            }
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "unproven_but_accepted",
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            let diagnostic_path = match agent_doc_tmux_io::capture_pane(tmux, pane) {
                Ok(content) => {
                    let snapshot = preserve_route_pane_snapshot(
                        file,
                        pane,
                        harness,
                        "direct_pane_dispatch_start_unproven",
                        &content,
                    );
                    print_route_pane_snapshot_hint(
                        file,
                        pane,
                        harness,
                        "direct_pane_dispatch_start_unproven",
                        &snapshot,
                    );
                    snapshot.path
                }
                Err(err) => {
                    eprintln!(
                        "[route] warning: failed to capture pane {} after unproven direct dispatch: {}",
                        pane, err
                    );
                    None
                }
            };
            log_route_submit_observation(RouteSubmitObservationLogFacts {
                file,
                pane,
                harness,
                phase: "dispatch_start_proof",
                observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
                trigger_visible: None,
                elapsed: proof_start.elapsed(),
                capture_len: None,
                capture_hash: None,
                proof: None,
            });
            (effects.file_route_dispatch_bug_report)(RouteDispatchBugReportFacts {
                file,
                pane,
                harness,
                phase: "dispatch_start_proof",
                issue: "accepted_without_dispatch_start_proof",
                result: RouteSubmitObservation::AcceptedWithoutDispatchProof.label(),
                elapsed: proof_start.elapsed(),
                proof: None,
                diagnostic_path: diagnostic_path.as_deref(),
            });
            if options.print_unproven_progress {
                eprintln!(
                    "[route] bare {} reopen for {} was accepted in pane {}, but no routed submission proof appeared after {}s",
                    harness.binary,
                    file.display(),
                    pane,
                    timeout.as_secs()
                );
            }
            Ok(RoutedDispatchStartProof::DispatchStartUnproven)
        }
        CommandDispatchStatus::TimedOut => {
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "submit_timed_out_without_proof",
            );
            (effects.file_route_dispatch_bug_report)(RouteDispatchBugReportFacts {
                file,
                pane,
                harness,
                phase: "direct_pane_submit_final",
                issue: "prompt_not_submitted",
                result: "submit_timed_out_without_proof",
                elapsed: proof_start.elapsed(),
                proof: None,
                diagnostic_path: submit_result.diagnostic_path.as_deref(),
            });
            anyhow::bail!(
                "routed {} trigger for {} left the bare reopen drafted in pane {} and still showed no routed submission proof after waiting {}s",
                harness.binary,
                file.display(),
                pane,
                timeout.as_secs()
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DirectPaneDispatchOptions {
    pub effects: RouteDispatchEffects,
    pub await_start_proof: bool,
    pub print_unproven_progress: bool,
}
