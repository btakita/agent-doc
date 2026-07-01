//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::is_codex_shell_search_blocker;

#[cfg(test)]
pub(crate) fn maybe_run_test_busy_auto_fix_hook(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
) -> Result<bool> {
    let Some(project_root) = agent_doc_fs::find_project_root(file)
        .or_else(|| file.parent().map(|parent| parent.to_path_buf()))
    else {
        return Ok(false);
    };
    let hook_path = project_root.join(".agent-doc/route-busy-auto-fix.txt");
    if !hook_path.exists() {
        return Ok(false);
    }
    let command = std::fs::read_to_string(&hook_path)
        .with_context(|| format!("failed to read {}", hook_path.display()))?;
    let command = command.trim();
    if command.is_empty() {
        return Ok(false);
    }
    tmux.raw_cmd(&["respawn-pane", "-k", "-t", pane, command])?;
    Ok(true)
}

#[cfg(not(test))]
pub(crate) fn maybe_run_test_busy_auto_fix_hook(
    _tmux: &Tmux,
    _file: &Path,
    _pane: &str,
) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
pub(crate) fn maybe_run_test_busy_interrupt_hook(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
) -> Result<bool> {
    let Some(project_root) = agent_doc_fs::find_project_root(file)
        .or_else(|| file.parent().map(|parent| parent.to_path_buf()))
    else {
        return Ok(false);
    };
    let hook_path = project_root.join(".agent-doc/route-busy-interrupt.txt");
    if !hook_path.exists() {
        return Ok(false);
    }
    let command = std::fs::read_to_string(&hook_path)
        .with_context(|| format!("failed to read {}", hook_path.display()))?;
    let command = command.trim();
    if command.is_empty() {
        return Ok(false);
    }
    tmux.raw_cmd(&["respawn-pane", "-k", "-t", pane, command])?;
    Ok(true)
}

#[cfg(not(test))]
pub(crate) fn maybe_run_test_busy_interrupt_hook(
    _tmux: &Tmux,
    _file: &Path,
    _pane: &str,
) -> Result<bool> {
    Ok(false)
}

pub(crate) fn attempt_busy_existing_pane_auto_fix(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
) -> Result<BusyPaneAutoFixOutcome> {
    eprintln!(
        "[route] registered pane {} for {} is busy with pending document drift — applying scoped `agent-doc fix {}` once before failing closed",
        pane,
        file_path,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_auto_fix_started file={} pane={}",
            file_path, pane
        ),
    );
    let test_hook_changed = maybe_run_test_busy_auto_fix_hook(tmux, file, pane)?;
    let fix_outcome = resync::apply_targeted_fix_for_route(tmux, file)?;
    let post_fix_binding = lookup_dispatch_registration(file_path, session_id)?;
    let pane_still_authoritative = post_fix_binding.as_deref() == Some(pane);
    let supervisor_health = Some(query_supervisor_health(file, session_id));
    let mut restarted = false;
    if !test_hook_changed
        && !fix_outcome.made_changes()
        && matches!(supervisor_health, Some(SupervisorHealth::Restartable))
    {
        restarted = restart_via_supervisor(file, session_id);
        if restarted {
            eprintln!(
                "[route] scoped fix left pane {} authoritative for {} — restarted the supervisor once before retrying route",
                pane, file_path
            );
        }
    } else if !test_hook_changed
        && !pane_still_authoritative
        && post_fix_binding.is_none()
        && fix_outcome.fixed_issues > 0
        && fix_outcome.pruned_dead_entries == 0
        && !fix_outcome.reregistered_owner
        && fix_outcome.killed_redundant_stash_panes == 0
        && matches!(supervisor_health, Some(SupervisorHealth::Restartable))
    {
        restarted = restart_via_supervisor(file, session_id);
        if restarted {
            eprintln!(
                "[route] scoped fix deregistered stale pane {} for {}, but the supervisor is still restartable — restarting once to wait for a clean handoff before retrying route",
                pane, file_path
            );
        }
    }
    let outcome = busy_existing_pane_auto_fix_outcome(
        test_hook_changed,
        fix_outcome.made_changes(),
        supervisor_health,
        restarted,
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_auto_fix_finished file={} pane={} pruned_dead_entries={} reregistered_owner={} killed_redundant_stash_panes={} fixed_issues={} restarted_supervisor={} outcome={:?}",
            file_path,
            pane,
            fix_outcome.pruned_dead_entries,
            fix_outcome.reregistered_owner,
            fix_outcome.killed_redundant_stash_panes,
            fix_outcome.fixed_issues,
            restarted,
            outcome
        ),
    );
    Ok(outcome)
}

pub(crate) fn busy_existing_pane_auto_fix_outcome(
    test_hook_changed: bool,
    fix_made_changes: bool,
    supervisor_health: Option<SupervisorHealth>,
    restarted_supervisor: bool,
) -> BusyPaneAutoFixOutcome {
    controller_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
        test_hook_changed,
        fix_made_changes,
        supervisor_healthy: matches!(supervisor_health, Some(SupervisorHealth::Healthy)),
        restarted_supervisor,
    })
}

/// Whether the busy-pane reroute may send `C-g`. Fast-path on the authoritative
/// `blocker_reason` from the readiness wait; otherwise re-classify a fresh
/// capture. The wait loop can report a timeout (`blocker_reason == None`) even
/// while the pane is genuinely in reverse-i-search (its 2-poll blocker streak
/// may not have latched), so we re-scan with [`dispatch_only_blocker_reason`],
/// which matches the whole capture rather than only the last few lines —
/// critical here because the shell-search line sits above trailing blank pane
/// rows, out of the window `HarnessConfig::dispatch_blocker_reason` inspects.
pub(crate) fn codex_pane_in_shell_search_state(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> bool {
    if harness.binary != "codex" {
        return false;
    }
    if is_codex_shell_search_blocker(blocker_reason) {
        return true;
    }
    let Ok(captured) = crate::sessions::capture_pane(tmux, pane) else {
        return false;
    };
    is_codex_shell_search_blocker(dispatch_only_blocker_reason(harness, &captured).as_deref())
}

pub(crate) fn attempt_busy_existing_pane_interrupt_recovery(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> Result<BusyPaneInterruptRecoveryOutcome> {
    if blocker_reason == Some("active permission prompt") {
        return Ok(BusyPaneInterruptRecoveryOutcome::Skipped);
    }

    if harness.binary == "opencode" {
        return attempt_opencode_busy_interrupt_recovery(tmux, file, pane, harness, blocker_reason);
    }

    if harness.binary != "codex" {
        return Ok(BusyPaneInterruptRecoveryOutcome::Skipped);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_interrupt_started file={} pane={} harness={} blocker={}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.unwrap_or("timeout")
        ),
    );
    eprintln!(
        "[route] live {} pane {} for {} is still busy after the scoped recovery path — sending one interrupt sequence before the final reroute attempt",
        harness.binary,
        pane,
        file.display()
    );

    // #codex-route-busy-ctrl-g-opens-editor: `C-g` only aborts a shell
    // reverse-i-search / history-search. In the normal Codex composer (or an
    // active turn) `C-g` opens the external editor ($EDITOR/nvim) instead of
    // interrupting — the same root cause as
    // `#codex-interrupt-clear-ctrl-g-opens-editor` in
    // `send_operator_interrupt_sequence`. Only send `C-g` when the live capture
    // proves a shell-search state; otherwise fall straight through to the
    // Escape + C-c interrupt below so a busy active turn is never sent into the
    // editor.
    if codex_pane_in_shell_search_state(tmux, pane, harness, blocker_reason) {
        let _ = tmux.send_keys_raw(pane, "C-g");
        std::thread::sleep(Duration::from_millis(100));
        let ctrl_g_probe =
            wait_for_agent_ready_outcome(tmux, pane, Duration::from_secs(2), harness);
        if ctrl_g_probe.is_ready() {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_busy_existing_pane_interrupt_finished file={} pane={} harness={} recovered=true outcome=ready stage=ctrl_g_probe",
                    file.display(),
                    pane,
                    harness.binary,
                ),
            );
            return Ok(BusyPaneInterruptRecoveryOutcome::Recovered);
        }
    } else {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_busy_existing_pane_interrupt_skipped_ctrl_g file={} pane={} harness={} reason=not_shell_search",
                file.display(),
                pane,
                harness.binary,
            ),
        );
    }

    let _ = tmux.send_keys_raw(pane, "Escape");
    std::thread::sleep(Duration::from_millis(100));
    let _ = tmux.send_keys_raw(pane, "C-c");
    std::thread::sleep(Duration::from_millis(100));
    let _ = maybe_run_test_busy_interrupt_hook(tmux, file, pane)?;

    let ready = wait_for_agent_ready_outcome(
        tmux,
        pane,
        fresh_route_start_ack_timeout(cfg!(test)),
        harness,
    );
    let recovered = ready.is_ready();
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_interrupt_finished file={} pane={} harness={} recovered={} outcome={} stage=escape_ctrl_c",
            file.display(),
            pane,
            harness.binary,
            recovered,
            match ready {
                AgentReadyWaitOutcome::Ready => "ready",
                AgentReadyWaitOutcome::Blocked { .. } => "blocked",
                AgentReadyWaitOutcome::TimedOut => "timed_out",
            }
        ),
    );
    Ok(match ready {
        AgentReadyWaitOutcome::Ready => BusyPaneInterruptRecoveryOutcome::Recovered,
        AgentReadyWaitOutcome::Blocked { reason } => {
            BusyPaneInterruptRecoveryOutcome::Blocked { reason }
        }
        AgentReadyWaitOutcome::TimedOut => BusyPaneInterruptRecoveryOutcome::TimedOut,
    })
}

pub(crate) fn attempt_opencode_busy_interrupt_recovery(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> Result<BusyPaneInterruptRecoveryOutcome> {
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_opencode_interrupt_started file={} pane={} harness={} blocker={}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.unwrap_or("timeout")
        ),
    );
    eprintln!(
        "[route] live {} pane {} for {} is still busy after the scoped recovery path — sending Escape to interrupt before the final reroute attempt",
        harness.binary,
        pane,
        file.display()
    );

    let _ = tmux.send_keys_raw(pane, "Escape");
    std::thread::sleep(Duration::from_millis(200));
    let mut ready = wait_for_agent_ready_outcome(
        tmux,
        pane,
        fresh_route_start_ack_timeout(cfg!(test)),
        harness,
    );
    if !ready.is_ready() {
        let _ = tmux.send_keys_raw(pane, "Escape");
        std::thread::sleep(Duration::from_millis(100));
        ready = wait_for_agent_ready_outcome(
            tmux,
            pane,
            fresh_route_start_ack_timeout(cfg!(test)),
            harness,
        );
    }
    let recovered = ready.is_ready();
    crate::ops_log::log_op(
        file,
        &format!(
            "route_busy_existing_pane_opencode_interrupt_finished file={} pane={} harness={} recovered={} outcome={}",
            file.display(),
            pane,
            harness.binary,
            recovered,
            match ready {
                AgentReadyWaitOutcome::Ready => "ready",
                AgentReadyWaitOutcome::Blocked { .. } => "blocked",
                AgentReadyWaitOutcome::TimedOut => "timed_out",
            }
        ),
    );
    Ok(match ready {
        AgentReadyWaitOutcome::Ready => BusyPaneInterruptRecoveryOutcome::Recovered,
        AgentReadyWaitOutcome::Blocked { reason } => {
            BusyPaneInterruptRecoveryOutcome::Blocked { reason }
        }
        AgentReadyWaitOutcome::TimedOut => BusyPaneInterruptRecoveryOutcome::TimedOut,
    })
}

pub(crate) fn ensure_existing_pane_ready_for_dispatch(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    prompt_bearing_marker: Option<&str>,
) -> Result<ExistingPaneDispatchReadiness> {
    let ready_outcome =
        wait_for_agent_ready_outcome(tmux, pane, existing_pane_ready_timeout(cfg!(test)), harness);
    if ready_outcome.is_ready() {
        return Ok(ExistingPaneDispatchReadiness::Ready);
    }

    let provenance = pane_route_provenance(tmux, pane);
    let blocker_reason = ready_outcome.blocker_reason().map(str::to_string);
    if prompt_bearing_marker.is_none() {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_existing_pane_already_running file={} pane={} harness={} {}",
                file.display(),
                pane,
                harness.binary,
                provenance
            ),
        );
        if let Err(e) = tmux.select_pane(pane) {
            eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
        }
        eprintln!(
            "[route] registered pane {} for {} is busy but has no pending prompt-bearing drift — focusing the live {} session instead of injecting a duplicate reopen",
            pane,
            file.display(),
            harness.binary
        );
        return Ok(ExistingPaneDispatchReadiness::BusyAlreadyRunning);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_existing_pane_not_idle file={} pane={} harness={} blocker={} {}",
            file.display(),
            pane,
            harness.binary,
            blocker_reason.as_deref().unwrap_or("timeout"),
            provenance
        ),
    );
    Ok(ExistingPaneDispatchReadiness::BusyNeedsAutoFix {
        provenance,
        blocker_reason,
    })
}
