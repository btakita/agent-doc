//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Outcome of one direct-pane submit-acceptance poll window.
pub(crate) struct DirectPaneAcceptance {
    status: CommandDispatchStatus,
    elapsed: Duration,
    /// Whether the trigger text was still visible in the pane when the window
    /// closed (only meaningful when `status == TimedOut`).
    trigger_visible: bool,
}

/// Poll the pane capture until the trigger text is consumed or the acceptance
/// window expires, logging the resulting submit observation. Pure detection —
/// it never sends input — so callers can re-run it after a re-submit attempt.
pub(crate) fn poll_direct_pane_acceptance(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
) -> DirectPaneAcceptance {
    let start = std::time::Instant::now();
    let timeout = direct_pane_submit_acceptance_timeout();
    // `#run-agent-doc-latency`: capture-then-sleep, not sleep-then-capture. A pane
    // that consumes the trigger quickly is detected on the first capture (~capture
    // overhead) instead of paying a full poll interval before the first check, and
    // a tighter poll shortens the acceptance floor for slower panes.
    let poll_interval = DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL;
    let mut last_capture: Option<(bool, usize, String)> = None;
    let mut capture_failed = false;
    while start.elapsed() < timeout {
        match sessions::capture_pane(tmux, pane) {
            Ok(content) => {
                let cmd_still_in_input = recent_lines_contain_trigger(&content, trigger);
                let capture_hash = short_content_hash(&content);
                let capture_len = content.len();
                last_capture = Some((cmd_still_in_input, capture_len, capture_hash));

                if !cmd_still_in_input {
                    let elapsed = start.elapsed();
                    let capture_hash = last_capture.as_ref().map(|(_, _, hash)| hash.as_str());
                    log_route_submit_observation(RouteSubmitObservationFacts {
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
        .map(|(visible, _, _)| *visible)
        .unwrap_or(false);
    if let Some((visible, capture_len, capture_hash)) = last_capture.as_ref() {
        log_route_submit_observation(RouteSubmitObservationFacts {
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
    }
}

/// `#jbcodexsubmit`: decide whether a timed-out direct-pane submit warrants a
/// one-shot bare `Enter` re-submit. The Codex TUI composer can leave the routed
/// prompt drafted when the trigger text and its trailing carriage return arrive
/// as one `send-keys` payload — the operator then has to press Enter manually.
/// Scope strictly to Codex (claude/opencode submit behavior is untouched), only
/// when the first attempt timed out with the trigger still visible.
pub(crate) fn direct_pane_needs_codex_resubmit(
    harness_binary: &str,
    status: CommandDispatchStatus,
    trigger_visible: bool,
) -> bool {
    harness_binary == "codex" && status == CommandDispatchStatus::TimedOut && trigger_visible
}

pub(crate) fn send_command_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    let trigger = send_command_once_unchecked(tmux, pane, file_path, harness)?;
    let file = Path::new(file_path);
    let first = poll_direct_pane_acceptance(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "direct_pane_acceptance",
    );
    if first.status == CommandDispatchStatus::Accepted {
        return Ok(CommandDispatchResult {
            status: first.status,
            elapsed: first.elapsed,
        });
    }

    // Try exactly one re-submit so we never loop on a genuinely stuck pane.
    if direct_pane_needs_codex_resubmit(&harness.binary, first.status, first.trigger_visible) {
        crate::input_diag::log_text_submit(
            Some(file),
            "route.direct_pane_resubmit",
            &format!("pane:{pane}"),
            "",
            Some(&harness.binary),
            "routed_resubmit_enter_key",
            "Enter",
        );
        if let Err(e) = crate::sessions::send_key(tmux, pane, "Enter") {
            eprintln!(
                "[route] warning: codex resubmit Enter failed for pane {}: {}",
                pane, e
            );
        }
        let second = poll_direct_pane_acceptance(
            tmux,
            pane,
            file,
            harness,
            &trigger,
            "direct_pane_resubmit_acceptance",
        );
        let result = if second.status == CommandDispatchStatus::Accepted {
            "accepted"
        } else {
            "still_visible"
        };
        crate::ops_log::log_op(
            file,
            &format!(
                "route_submit_resubmit file={} pane={} harness={} action=enter_key result={} elapsed_ms={}",
                file.display(),
                pane,
                harness.binary,
                result,
                second.elapsed.as_millis()
            ),
        );
        return Ok(CommandDispatchResult {
            status: second.status,
            elapsed: first.elapsed + second.elapsed,
        });
    }

    Ok(CommandDispatchResult {
        status: first.status,
        elapsed: first.elapsed,
    })
}

pub(crate) fn send_command_once_unchecked(
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
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    crate::input_diag::log_text_submit(
        Some(Path::new(file_path)),
        "route.direct_pane_submit",
        &format!("pane:{pane}"),
        &payload,
        Some(&harness.binary),
        if harness.binary == "opencode" {
            "routed_trigger_kitty_return"
        } else {
            "routed_trigger_cr"
        },
        if harness.binary == "opencode" {
            "KittyReturn"
        } else {
            "Enter"
        },
    );
    crate::sessions::send_submitted_text_for_harness(tmux, pane, &payload, &harness.binary)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent {} → pane {}", trigger, pane);
    Ok(trigger)
}

pub(crate) fn dispatch_via_supervisor_ipc_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: SupervisorIpcDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
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
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let method = IpcMethod::Inject {
        bytes: routed_trigger_submit_payload(&payload),
    };
    crate::input_diag::log_text_submit(
        Some(file),
        "route.supervisor_ipc",
        &format!("socket:{}:pane:{pane}", sock.display()),
        &payload,
        Some(&harness.binary),
        "supervisor_ipc_inject",
        "Inject",
    );
    let submit_start = Instant::now();
    let response = crate::supervisor::ipc::send_command(&sock, &method).with_context(|| {
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

    let timeout = routed_dispatch_start_timeout(harness);
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
        crate::ops_log::log_op(
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
    crate::ops_log::log_op(
        file,
        &format!(
            "route_actor_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            timeout.as_secs()
        ),
    );
    log_route_submit_observation(RouteSubmitObservationFacts {
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
    if options.print_unproven_progress {
        eprintln!(
            "[route] authoritative actor accepted the {} reopen for {} in pane {}, but no routed submission proof appeared after {}s",
            harness.binary,
            file.display(),
            pane,
            timeout.as_secs()
        );
    }
    Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SupervisorIpcDispatchOptions {
    pub(crate) await_start_proof: bool,
    pub(crate) print_unproven_progress: bool,
}

pub(crate) fn dispatch_via_supervisor_ipc(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc_with_mode(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        SupervisorIpcDispatchOptions {
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

pub(crate) fn authoritative_actor_dispatch_recovery_hint(
    state: crate::session_actor::ActorState,
    file: &Path,
) -> String {
    actor_recovery_hint(actor_dispatch_state(state), &file.display().to_string())
}

#[cfg(test)]
pub(crate) fn authoritative_actor_dispatch_can_queue_optimistically(
    state: crate::session_actor::ActorState,
) -> bool {
    crate::flow::routed_reopen::actor_can_queue_optimistically(actor_dispatch_state(state))
}

pub(crate) fn canonical_dispatch_file(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = crate::git::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub(crate) fn canonical_registered_file(entry: &sessions::SessionEntry) -> std::path::PathBuf {
    let path = std::path::Path::new(&entry.file);
    let resolved = if path.is_absolute() || entry.cwd.is_empty() {
        path.to_path_buf()
    } else {
        std::path::Path::new(&entry.cwd).join(path)
    };
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub(crate) fn registry_base_dir_for_dispatch(file_path: &str) -> std::path::PathBuf {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    crate::snapshot::find_project_root(&requested)
        .or_else(|| requested.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

pub(crate) fn lookup_dispatch_registration(file_path: &str, session_id: &str) -> Result<Option<String>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::lookup_in(&base_dir, session_id)
}

pub(crate) fn load_dispatch_registry(file_path: &str) -> Result<sessions::SessionRegistry> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::load_in(&base_dir)
}

pub(crate) fn deregister_dispatch_registration(file_path: &str, session_id: &str) -> Result<bool> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let registry_path = sessions::registry_path_in(&base_dir);
    let _lock = sessions::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load_in(&base_dir)?;
    let removed_key = registry.iter().find_map(|(key, entry)| {
        ((entry.session_id == session_id) || (entry.session_id.is_empty() && key == session_id))
            .then(|| key.clone())
    });
    let removed = removed_key.and_then(|key| registry.remove(&key)).is_some();
    if removed {
        sessions::save_in(&base_dir, &registry)?;
    }
    Ok(removed)
}

pub(crate) fn register_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<()> {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let requested_str = requested.to_string_lossy().to_string();
    let base_dir = registry_base_dir_for_dispatch(&requested_str);
    ensure_dispatch_target_can_bind_file(tmux, &base_dir, pane_id, &requested_str)?;
    let window = sessions::pane_window(pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    sessions::register_full_with_cwd_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
    )
}

pub(crate) fn ensure_dispatch_target_can_bind_file(
    tmux: &Tmux,
    base_dir: &Path,
    pane: &str,
    file_path: &str,
) -> Result<()> {
    let registry = sessions::load_in(base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch registration from {}",
            base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        let registered = canonical_registered_file(entry);
        let registered_is_live_owner = !entry.session_id.is_empty()
            && crate::sync::find_normal_path_owner_pane(tmux, &registered, &entry.session_id)
                .as_deref()
                == Some(pane);
        if !registered_is_live_owner {
            return Ok(());
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            registered.display(),
            requested.display()
        );
    }

    Ok(())
}

pub(crate) fn pane_registration_matches_file(
    registry: &sessions::SessionRegistry,
    pane: &str,
    file_path: &str,
) -> bool {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| canonical_registered_file(entry) == requested)
        .unwrap_or(false)
}

pub(crate) fn ensure_dispatch_target_matches_file(pane: &str, file_path: &str) -> Result<()> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    anyhow::bail!(
        "route dispatch target {} is not registered for {}; refusing unbound dispatch",
        pane,
        requested.display()
    );
}

pub(crate) fn resolve_fresh_dispatch_target_after_ready_wait(
    tmux: &Tmux,
    session_id: &str,
    pane: &str,
    file_path: &str,
    _startup_miss_handoff_blocked_pane: Option<&str>,
) -> Result<String> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before fresh-dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(pane.to_string());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let handoff_target = registry
        .values()
        .find(|entry| {
            entry.session_id == session_id
                && !entry.pane.is_empty()
                && entry.pane != pane
                && canonical_registered_file(entry) == requested
        })
        .map(|entry| entry.pane.clone());
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        if let Some(handoff_pane) = handoff_target {
            eprintln!(
                "[route] fresh restart re-bound {} away from pane {} and onto authoritative pane {} before retry",
                file_path, pane, handoff_pane
            );
            return Ok(handoff_pane);
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    // A fresh route already created `pane` deliberately. If some concurrent
    // sync/layout path rebinds the same document session back to another pane
    // during the ready wait, keep the fresh pane authoritative instead of
    // handing dispatch back to the older pane and making the new pane disposable.
    register_dispatch_target(tmux, session_id, pane, file_path)?;
    Ok(pane.to_string())
}

pub(crate) fn send_command_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    ensure_dispatch_target_matches_file(pane, file_path)?;
    send_command_unchecked(tmux, pane, file_path, harness)
}

pub(crate) fn dispatch_existing_managed_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc(tmux, file, pane, session_id, file_path, harness)
}

pub(crate) fn dispatch_routed_reopen(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_routed_reopen_with_mode(tmux, file, pane, file_path, harness, true)
}

pub(crate) fn dispatch_routed_reopen_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    print_unproven_progress: bool,
) -> Result<RoutedDispatchStartProof> {
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

    let timeout = routed_dispatch_start_timeout(harness);
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
        crate::ops_log::log_op(
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
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "unproven_but_accepted",
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            log_route_submit_observation(RouteSubmitObservationFacts {
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
            if print_unproven_progress {
                eprintln!(
                    "[route] bare {} reopen for {} was accepted in pane {}, but no routed submission proof appeared after {}s",
                    harness.binary,
                    file.display(),
                    pane,
                    timeout.as_secs()
                );
            }
            Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
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

pub(crate) fn routed_trigger_payload(trigger: &str) -> String {
    trigger.to_string()
}

pub(crate) fn apply_plain_trigger_override(harness: &mut HarnessConfig) {
    harness.trigger_command_template = "agent-doc {file}".to_string();
}

pub(crate) fn routed_trigger_submit_payload(payload: &str) -> String {
    crate::supervisor::ipc::normalize_submit_text(payload)
}

pub(crate) fn validate_routed_trigger_payload(
    harness: &HarnessConfig,
    trigger: &str,
    payload: &str,
) -> Result<()> {
    if harness.binary == "codex"
        && (payload != trigger || payload.contains('\n') || payload.contains('\r'))
    {
        anyhow::bail!(
            "internal route bug: Codex reroute payload must stay the bare `agent-doc <FILE>` reopen; refusing to inject {:?}",
            payload
        );
    }
    Ok(())
}
