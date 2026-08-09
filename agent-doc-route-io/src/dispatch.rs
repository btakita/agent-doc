//! Route dispatch transport and dispatch-start proof I/O.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::direct_pane_dispatch::{
    CommandDispatchResult, CommandDispatchStatus, RouteSubmitObservationLogFacts,
    log_dispatch_inject, log_route_latency, log_route_submit_observation,
    poll_direct_pane_acceptance, preserve_route_pane_snapshot, print_route_pane_snapshot_hint,
    send_command_once_unchecked, send_command_unchecked,
    send_direct_pane_enter_resubmit_until_stable,
    try_late_direct_pane_enter_resubmit_after_unproven_dispatch,
};
use crate::dispatch_start::{
    RoutedDispatchStartTracker, build_routed_dispatch_start_tracker, wait_for_routed_dispatch_start,
};
use crate::supervisor_runtime::supervisor_socket_path;
use agent_doc_controller::dispatch::{
    DirectPaneDispatchStartProofFacts, DirectPaneSubmitPolicy,
    PASS_THROUGH_STRANDED_DRAFT_SETTLE, PassThroughStrandedDraftAction,
    PassThroughStrandedDraftFacts, PassThroughStrandedDraftLogFacts,
    PreDispatchStrandedDraftAction, PreDispatchStrandedDraftFacts, RouteSubmitObservation,
    RoutedDispatchStartProof, RoutedTriggerPayloadFacts, busy_dispatch_start_outcome,
    classify_pass_through_stranded_draft_action, classify_pre_dispatch_stranded_draft_action,
    direct_pane_should_await_dispatch_start_proof, direct_pane_submit_acceptance_budget,
    direct_pane_submit_outcome, dispatch_start_busy_probe_timeout,
    dispatch_start_early_resubmit_probe_timeout, pass_through_stranded_draft_log_line,
    pass_through_stranded_draft_max_enter_resubmits,
    pass_through_stranded_draft_required_clear_observations,
    pre_dispatch_stranded_draft_admission_timeout, route_trigger_visible_in_current_draft,
    routed_dispatch_start_timeout_for_binary, routed_trigger_payload_rejection,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry;
use agent_doc_supervisor::ipc_protocol::IpcMethod;
use agent_doc_turn::op_log::OpsLogEvent;
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

    // Supervisor IPC `ok` proves only that the bytes crossed the transport.
    // Observe the owned pane before reporting success: the trigger must either
    // be seen and consumed or produce an active-turn cue. If it remains as a
    // draft, recover with bounded Enter-only resubmits; never inject the prompt
    // text a second time. A stable empty composer without either observation is
    // ambiguous and therefore fails closed instead of enabling a second route.
    let acceptance = poll_direct_pane_acceptance(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "supervisor_ipc_acceptance",
    );
    let acceptance = send_direct_pane_enter_resubmit_until_stable(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "supervisor_ipc_resubmit_acceptance",
        acceptance,
    );
    if acceptance.trigger_visible {
        let diagnostic = acceptance
            .diagnostic_path
            .as_ref()
            .map(|path| format!(" snapshot_path={}", path.display()))
            .unwrap_or_default();
        anyhow::bail!(
            "authoritative actor transport accepted the routed trigger for {} in pane {}, but the trigger remained drafted after {}ms; refusing duplicate prompt injection{}",
            file.display(),
            pane,
            acceptance.elapsed.as_millis(),
            diagnostic,
        );
    }

    if !acceptance.end_to_end_submitted() {
        // A fast harness can consume the trigger before the first capture and
        // clear its busy cue before the stable-empty poll completes. Transport
        // acceptance plus the controller's durable open dispatch receipt is
        // enough to suppress a second prompt. For synchronous routes, the
        // stronger dispatch-start tracker below still gets its full chance to
        // prove the turn; async editor routes return accepted admission now.
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_actor_transport_accepted_composer_unobserved file={} pane={} harness={} elapsed_ms={} action=retain_dispatch_receipt_no_prompt_reinject",
                file.display(),
                pane,
                harness.binary,
                acceptance.elapsed.as_millis(),
            ),
        );
    }

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

pub fn send_command_once_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<String> {
    dispatch_registry::ensure_dispatch_target_matches_file(pane, file_path)?;
    send_command_once_unchecked(tmux, pane, file_path, harness)
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
            submit_policy: DirectPaneSubmitPolicy::ObserveHarnessAcceptance,
        },
    )
}

/// `#runfilesubmit`: verify the pass-through single submit actually crossed the
/// composer, and repair it when it did not.
///
/// `PassThroughSingleSubmit` exists so a plain `agent-doc <FILE>` trigger (the
/// JetBrains "Run Agent Doc" action) returns immediately instead of paying the
/// dispatch-start proof budget. It returned `TransportSubmittedOnly` without
/// ever looking at the pane, so a submit key absorbed by the harness TUI left
/// the trigger sitting unsent in the composer forever: route logged
/// `exit_code=0` / `pass_through_single_submit` while no cycle ever started.
///
/// `#runsubmitclaude`: the common case pays exactly one
/// `PASS_THROUGH_STRANDED_DRAFT_SETTLE` window before the first verdict.
/// `tmux send-keys` returns once the bytes reach the pty, so a capture taken
/// immediately after it shows the pane *before* the trigger arrived; reading
/// that empty composer as `Cleared` ended the repair in a millisecond and left
/// the operator's real strand unrepaired. 150ms is still two orders of
/// magnitude under the dispatch-start proof budget this path exists to skip.
/// A capture failure is never read as "stranded": unknown pane state must not
/// authorize pressing keys. This is a one-shot transport boundary inside a
/// single route call with no long-lived state relationship, so it stays a
/// direct command rather than a lifecycle-scoped `Effect`
/// (`#lazily-reactive-first`), matching the sibling
/// `send_direct_pane_enter_resubmit_until_stable` on the observed-acceptance
/// path.
fn repair_pass_through_stranded_draft(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    trigger: &str,
) {
    let start = Instant::now();
    let max_enters = pass_through_stranded_draft_max_enter_resubmits();
    let required_clear_observations = pass_through_stranded_draft_required_clear_observations();
    let mut enters_sent = 0usize;
    let mut clear_observations = 0usize;
    let mut settled = false;
    let mut capture_failed = false;
    loop {
        let (draft_visible, pane_busy) = match agent_doc_tmux_io::capture_pane(tmux, pane) {
            Ok(content) => (
                route_trigger_visible_in_current_draft(&content, trigger, |line| {
                    harness.is_prompt_line(line)
                }),
                harness.has_busy_cue(&content),
            ),
            Err(e) => {
                capture_failed = true;
                eprintln!(
                    "[route] warning: pass-through draft check could not capture pane {}: {}",
                    pane, e
                );
                (false, false)
            }
        };
        let action =
            classify_pass_through_stranded_draft_action(PassThroughStrandedDraftFacts {
                draft_visible,
                pane_busy,
                settled,
                enters_sent,
                max_enters,
                clear_observations,
                required_clear_observations,
            });
        match action {
            PassThroughStrandedDraftAction::SettleAndReobserve => {
                // Only a SETTLED idle-and-empty look counts toward the
                // confirmation; the pre-settle look observed nothing about this
                // submit at all (`#runsubmitclaude`).
                if settled && !draft_visible && !pane_busy {
                    clear_observations += 1;
                }
                settled = true;
                std::thread::sleep(PASS_THROUGH_STRANDED_DRAFT_SETTLE);
                continue;
            }
            PassThroughStrandedDraftAction::EnterResubmit => {
                enters_sent += 1;
                clear_observations = 0;
                send_pass_through_stranded_draft_enter(tmux, file, pane, harness);
                std::thread::sleep(PASS_THROUGH_STRANDED_DRAFT_SETTLE);
                continue;
            }
            _ => {}
        }
        agent_doc_ops_log_io::log_op(
            file,
            &pass_through_stranded_draft_log_line(PassThroughStrandedDraftLogFacts {
                file_display: &file.display().to_string(),
                pane,
                harness_binary: &harness.binary,
                action,
                enters_sent,
                elapsed_ms: start.elapsed().as_millis(),
                capture_failed,
            }),
        );
        if action == PassThroughStrandedDraftAction::ExhaustedStillStranded {
            eprintln!(
                "[route] warning: {} trigger is still unsent in pane {} for {} after {} submit-key retries",
                harness.binary,
                pane,
                file.display(),
                enters_sent
            );
        }
        return;
    }
}

fn send_pass_through_stranded_draft_enter(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
) {
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.pass_through_draft_resubmit",
        &format!("pane:{pane}"),
        "",
        Some(&harness.binary),
        "pass_through_stranded_draft_submit_key",
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
            "[route] warning: {} stranded-draft {} resubmit failed for pane {}: {}",
            harness.binary, submit_key, pane, e
        );
    }
}

/// (`#strandeddraftresubmit`) Observe the composer BEFORE injecting a trigger.
///
/// The "still unsubmitted" test is scoped to the CURRENT draft of a
/// cursor-anchored dispatch-ready prompt. This path runs against long-lived
/// panes whose scrollback holds every previously submitted trigger and whose
/// queued-input region echoes an accepted trigger verbatim, so a whole-capture
/// substring match (`pane_composer_has_pending_trigger`, sound only on the
/// brand-new fresh pane it was written for) would read consumed history as a
/// stranded draft (`#autotriggerscrollbackecho`).
fn observe_pre_dispatch_stranded_draft(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
    trigger: &str,
) -> PreDispatchStrandedDraftAction {
    let capture = agent_doc_tmux_io::capture_pane_with_ansi(tmux, pane).ok();
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(tmux, pane);
    classify_pre_dispatch_stranded_draft_action(PreDispatchStrandedDraftFacts {
        pane_captured: capture.is_some(),
        trigger_drafted: capture.as_deref().is_some_and(|content| {
            agent_doc_harness::ready_prompt_candidate_at_cursor(content, harness, cursor_y)
                .is_some()
                && route_trigger_visible_in_current_draft(content, trigger, |line| {
                    harness.is_prompt_line(line)
                })
        }),
        pane_busy: capture
            .as_deref()
            .is_some_and(|content| harness.has_busy_cue(content)),
    })
}

/// (`#strandeddraftresubmit`) Submit a trigger already stranded in the composer
/// instead of appending a second one to it.
///
/// Returns `Ok(Some(proof))` when the stranded draft was submitted and the
/// controller then projected turn admission — the routed request is running, so
/// the caller must NOT inject. Returns `Ok(None)` in every other case (nothing
/// stranded, unobservable pane, busy pane, submit-key failure, or no admission
/// within the bounded window) so the normal dispatch path runs exactly as
/// before. Falling through is deliberately safe: the post-dispatch repair still
/// owns a draft this call creates.
fn try_pre_dispatch_stranded_draft_submit(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<Option<RoutedDispatchStartProof>> {
    let trigger = harness.trigger_command(file_path);
    let action = observe_pre_dispatch_stranded_draft(tmux, pane, harness, &trigger);
    if action != PreDispatchStrandedDraftAction::ResubmitStrandedDraft {
        // Only log the states that diverted or could not be observed; a clean
        // fresh dispatch is the common case and stays silent.
        if action != PreDispatchStrandedDraftAction::DispatchFresh {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_pre_dispatch_draft_observation file={} pane={} harness={} action={}",
                    file.display(),
                    pane,
                    harness.binary,
                    action.as_str(),
                ),
            );
        }
        return Ok(None);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_pre_dispatch_stranded_draft_resubmit file={} pane={} harness={} action={} note=composer already held this trigger unsubmitted; submitting it instead of appending a second trigger",
            file.display(),
            pane,
            harness.binary,
            action.as_str(),
        ),
    );
    eprintln!(
        "[route] {} composer in pane {} still holds the {} trigger unsubmitted — submitting that draft instead of injecting a second one",
        harness.binary,
        pane,
        file.display()
    );
    let baseline = agent_doc_cycle_state_io::load(file).ok().flatten();
    send_pass_through_stranded_draft_enter(tmux, file, pane, harness);
    let timeout = pre_dispatch_stranded_draft_admission_timeout(cfg!(test));
    match crate::admission_projection::wait_for_start_projection(file, baseline.as_ref(), timeout) {
        Some(state) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_pre_dispatch_stranded_draft_admitted file={} pane={} harness={} cycle={} phase={} timeout_ms={}",
                    file.display(),
                    pane,
                    harness.binary,
                    state.cycle_id,
                    state.phase.as_str(),
                    timeout.as_millis(),
                ),
            );
            Ok(Some(RoutedDispatchStartProof::StrandedDraftSubmitted))
        }
        None => {
            // "Could not observe admission" must stay distinct from "the draft
            // was not submitted" (`#idlerevisionreactive`). Neither claims the
            // dispatch, so the normal path runs and owns the outcome.
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_pre_dispatch_stranded_draft_admission_unobserved file={} pane={} harness={} timeout_ms={} note=falling through to the normal dispatch path",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_millis(),
                ),
            );
            Ok(None)
        }
    }
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
    let _route_submit_guard = agent_doc_controller_io::project_controller::begin_route_submit(
        file,
        pane,
        &harness.binary,
    )?;
    // `#strandeddraftresubmit`: a trigger already stranded in the composer must
    // be submitted, never have a second trigger appended to it.
    if let Some(proof) =
        try_pre_dispatch_stranded_draft_submit(tmux, file, pane, file_path, harness)?
    {
        return Ok(proof);
    }
    if options.submit_policy == DirectPaneSubmitPolicy::PassThroughSingleSubmit {
        let submit_start = Instant::now();
        let trigger = send_command_once_checked(tmux, pane, file_path, harness)?;
        repair_pass_through_stranded_draft(tmux, file, pane, harness, &trigger);
        let elapsed = submit_start.elapsed();
        log_route_latency(
            file,
            "direct_pane_submit",
            elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            "pass_through_single_submit",
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_direct_pane_plain_trigger_pass_through file={} pane={} harness={} outcome=single_text_enter_submit elapsed_ms={}",
                file.display(),
                pane,
                harness.binary,
                elapsed.as_millis(),
            ),
        );
        return Ok(RoutedDispatchStartProof::TransportSubmittedOnly);
    }
    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
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
    let early_probe_timeout = dispatch_start_early_resubmit_probe_timeout(cfg!(test));
    if let Some(proof) =
        wait_for_routed_dispatch_start(tmux, file, &tracker, harness, early_probe_timeout)?
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
                "{} file={} pane={} harness={} proof={} timeout_secs={}",
                OpsLogEvent::RouteDispatchStartProven,
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
                || {
                    wait_for_routed_dispatch_start(
                        tmux,
                        file,
                        &tracker,
                        harness,
                        timeout.saturating_sub(proof_start.elapsed()),
                    )
                },
            )? {
                return Ok(proof);
            }
            if let Some(proof) = wait_for_routed_dispatch_start(
                tmux,
                file,
                &tracker,
                harness,
                timeout.saturating_sub(proof_start.elapsed()),
            )? {
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
                        "{} file={} pane={} harness={} proof={} timeout_secs={}",
                        OpsLogEvent::RouteDispatchStartProven,
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
                    "{} file={} pane={} harness={} timeout_secs={}",
                    OpsLogEvent::RouteDispatchStartUnprovenButAccepted,
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
    pub submit_policy: DirectPaneSubmitPolicy,
}
