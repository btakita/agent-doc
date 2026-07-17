//! Extracted from `write.rs` (large-module split). See parent module for context.

use crate::pane_provenance::pane_route_provenance;
use crate::startup_ready::{AgentReadyWaitOutcome, wait_for_agent_ready_outcome};
use crate::supervisor_runtime::{query_supervisor_health, restart_via_supervisor};
use agent_doc_controller::dispatch::busy_existing_pane_auto_fix_outcome as controller_busy_existing_pane_auto_fix_outcome;
use agent_doc_controller::dispatch::{
    BusyPaneAutoFixFacts, BusyPaneAutoFixOutcome, existing_pane_ready_timeout,
    fresh_route_start_ack_timeout, is_codex_shell_search_blocker,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_session_registry_io::dispatch_registry::lookup_dispatch_registration;
use agent_doc_supervisor::route_runtime::SupervisorHealth;
#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tmux_router::Tmux;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingPaneDispatchReadiness {
    Ready,
    BusyAlreadyRunning,
    BusyNeedsAutoFix {
        provenance: String,
        blocker_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusyPaneInterruptRecoveryOutcome {
    Recovered,
    Blocked { reason: String },
    TimedOut,
    Skipped,
}

fn pending_prompt_waits_behind_queueable_blocker(
    harness_binary: &str,
    prompt_bearing_marker: Option<&str>,
    blocker_reason: Option<&str>,
) -> Option<&'static str> {
    prompt_bearing_marker?;
    agent_doc_queue::route_dispatch::dispatch_active_turn_queue_source(
        harness_binary,
        blocker_reason?,
    )
}

#[cfg(test)]
pub(crate) fn maybe_run_test_busy_auto_fix_hook(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
) -> Result<bool> {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
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
    agent_doc_tmux_io::respawn_pane(tmux, pane, command)?;
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
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
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
    agent_doc_tmux_io::respawn_pane(tmux, pane, command)?;
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

pub fn attempt_busy_existing_pane_auto_fix(
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
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_busy_existing_pane_auto_fix_started file={} pane={}",
            file_path, pane
        ),
    );
    let test_hook_changed = maybe_run_test_busy_auto_fix_hook(tmux, file, pane)?;
    let fix_outcome = agent_doc_sync_io::resync::apply_targeted_fix_for_route(tmux, file)?;
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
    let outcome = controller_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
        test_hook_changed,
        fix_made_changes: fix_outcome.made_changes(),
        supervisor_health,
        restarted_supervisor: restarted,
    });
    agent_doc_ops_log_io::log_op(
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

/// Whether the busy-pane reroute may send `C-g`. Fast-path on the authoritative
/// `blocker_reason` from the readiness wait; otherwise re-classify a fresh
/// capture. The wait loop can report a timeout (`blocker_reason == None`) even
/// while the pane is genuinely in reverse-i-search (its 2-poll blocker streak
/// may not have latched), so we re-scan with
/// [`agent_doc_harness::dispatch_only_blocker_reason`], which matches the whole
/// capture rather than only the last few lines —
/// critical here because the shell-search line sits above trailing blank pane
/// rows, out of the window `HarnessConfig::dispatch_blocker_reason` inspects.
pub fn codex_pane_in_shell_search_state(
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
    let Ok(captured) = agent_doc_tmux_io::capture_pane(tmux, pane) else {
        return false;
    };
    is_codex_shell_search_blocker(
        agent_doc_harness::dispatch_only_blocker_reason(harness, &captured).as_deref(),
    )
}

pub fn attempt_busy_existing_pane_interrupt_recovery(
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

    agent_doc_ops_log_io::log_op(
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
            agent_doc_ops_log_io::log_op(
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
        agent_doc_ops_log_io::log_op(
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
    agent_doc_ops_log_io::log_op(
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

pub fn attempt_opencode_busy_interrupt_recovery(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    harness: &HarnessConfig,
    blocker_reason: Option<&str>,
) -> Result<BusyPaneInterruptRecoveryOutcome> {
    agent_doc_ops_log_io::log_op(
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
    agent_doc_ops_log_io::log_op(
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

pub fn ensure_existing_pane_ready_for_dispatch(
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
    if let Some(source) = pending_prompt_waits_behind_queueable_blocker(
        &harness.binary,
        prompt_bearing_marker,
        blocker_reason.as_deref(),
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_existing_pane_prompt_queued_behind_blocker file={} pane={} harness={} blocker={} source={} {}",
                file.display(),
                pane,
                harness.binary,
                blocker_reason.as_deref().unwrap_or("timeout"),
                source,
                provenance
            ),
        );
        eprintln!(
            "[route] registered pane {} for {} has an operator-owned {} blocker — the pending document prompt remains queued and will run when the live {} session returns to its idle composer",
            pane,
            file.display(),
            blocker_reason.as_deref().unwrap_or("busy"),
            harness.binary,
        );
        return Ok(ExistingPaneDispatchReadiness::BusyAlreadyRunning);
    }
    if prompt_bearing_marker.is_none() {
        agent_doc_ops_log_io::log_op(
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
    agent_doc_ops_log_io::log_op(
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

#[cfg(test)]
mod tests {
    use super::pending_prompt_waits_behind_queueable_blocker;

    #[test]
    fn pending_claude_prompt_waits_behind_artifact_picker_without_auto_fix() {
        assert_eq!(
            pending_prompt_waits_behind_queueable_blocker(
                "claude",
                Some("operator-prompt-marker"),
                Some("claude artifact picker open"),
            ),
            Some("dispatch_only_claude_artifact_picker")
        );
    }

    #[test]
    fn artifact_picker_without_pending_prompt_does_not_schedule_duplicate_work() {
        assert_eq!(
            pending_prompt_waits_behind_queueable_blocker(
                "claude",
                None,
                Some("claude artifact picker open"),
            ),
            None
        );
    }

    #[test]
    fn unknown_blocker_still_uses_fail_closed_recovery() {
        assert_eq!(
            pending_prompt_waits_behind_queueable_blocker(
                "claude",
                Some("operator-prompt-marker"),
                Some("unknown claude modal"),
            ),
            None
        );
    }
}
