//! Route pane-resolution helper I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_send_reopen,
};
use crate::dispatch_recovery::{
    resolve_fresh_dispatch_target_after_ready_wait, wait_for_starting_pane_recovery_target,
};
use crate::dispatch_target::register_dispatch_target;
use crate::pane_provenance::pane_route_provenance;
use crate::supervisor_runtime::restart_via_supervisor_with_mode;
use agent_doc_controller::dispatch::{
    DispatchActorState, DispatchOnlyReopenDelivery, DispatchRuntimeHealth, StartupMissRouteFacts,
    is_stash_window_name,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::route_runtime::SupervisorHealth;
use agent_doc_supervisor::startup_miss::StartingPaneRecoveryTarget;
use tmux_router::Tmux;

pub fn dispatch_runtime_health(health: SupervisorHealth) -> DispatchRuntimeHealth {
    match health {
        SupervisorHealth::Healthy => DispatchRuntimeHealth::Healthy,
        SupervisorHealth::Restartable => DispatchRuntimeHealth::Restartable,
        SupervisorHealth::Halted { restart_count } => {
            DispatchRuntimeHealth::Halted { restart_count }
        }
        SupervisorHealth::Unreachable => DispatchRuntimeHealth::Unreachable,
        SupervisorHealth::NoSocket => DispatchRuntimeHealth::NoSocket,
    }
}

pub fn startup_miss_route_facts(
    miss: &agent_doc_supervisor::startup_miss::StartupMiss,
    registered_pane: &str,
    pane_alive: bool,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> StartupMissRouteFacts {
    StartupMissRouteFacts {
        miss_timestamp: miss.timestamp,
        registered_pane_is_live_owner: live_owner == Some(registered_pane),
        pane_alive,
        supervisor_health: dispatch_runtime_health(supervisor_health),
        latest_start_matches_registered_pane: log_status
            .and_then(|status| status.latest_start_pane.as_deref())
            == Some(registered_pane),
        latest_session_open: log_status
            .is_some_and(agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_open),
        latest_session_closed: log_status.is_some_and(
            agent_doc_supervisor::startup_miss::SessionLogStatus::latest_session_closed,
        ),
        latest_start_timestamp: log_status.and_then(|status| status.latest_start_timestamp),
        latest_open_run_timestamp: log_status
            .and_then(agent_doc_supervisor::startup_miss::latest_open_run_timestamp),
    }
}

pub fn startup_miss_route_provenance(
    tmux: &Tmux,
    pane_id: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&agent_doc_supervisor::startup_miss::SessionLogStatus>,
) -> String {
    let log_detail = match log_status {
        Some(status) => format!(
            "session_log={} {} last_event={}",
            agent_doc_supervisor::startup_miss::latest_log_outcome(status),
            agent_doc_supervisor::startup_miss::latest_log_anchor(status),
            agent_doc_supervisor::startup_miss::latest_log_last_event(status)
        ),
        None => "session_log=missing".to_string(),
    };
    format!(
        "{} live_owner={} supervisor_health={:?} {}",
        pane_route_provenance(tmux, pane_id),
        live_owner.unwrap_or("none"),
        supervisor_health,
        log_detail
    )
}

pub fn fail_if_recent_session_loss_window(file: &Path, session_id: &str) -> Result<()> {
    let Some(window) =
        agent_doc_supervisor_io::startup_miss::recent_session_loss_window(file, session_id)?
    else {
        return Ok(());
    };

    let first = agent_doc_supervisor::startup_miss::format_timestamp(window.first_timestamp);
    let last = agent_doc_supervisor::startup_miss::format_timestamp(window.last_timestamp);
    let latest_reason = window.latest_reason.as_deref().unwrap_or("unknown");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_repeated_session_loss_fail_closed file={} session={} count={} first={} last={} latest_reason={}",
            file.display(),
            session_id,
            window.count,
            first,
            last,
            latest_reason
        ),
    );
    anyhow::bail!(
        "refusing to auto-start {} after {} unexpected pane-loss events since {} (latest reason={} at {}). Route will not keep spawning replacements over a repeated crash window; inspect the last dead-pane/session-loss diagnostics, then run `agent-doc start {}` manually to recover",
        file.display(),
        window.count,
        first,
        latest_reason,
        last,
        file.display()
    );
}

/// Returns true if the pane is running an agent process for the given harness.
/// Returns true on query failure so route does not skip panes it cannot inspect.
pub fn is_agent_process(tmux: &Tmux, pane_id: &str, harness: &HarnessConfig) -> bool {
    agent_doc_tmux_io::target_current_command(tmux, pane_id)
        .map(|cmd| harness.is_agent_process_name(&cmd))
        .unwrap_or(true)
}

pub fn cleanup_failed_route_panes(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    created_panes: &[String],
) {
    for p in created_panes {
        if !tmux.pane_alive(p) {
            continue;
        }
        if failed_route_pane_has_startup_miss(file, p) {
            eprintln!(
                "[route] reaping startup-miss pane {} after failed fresh route for {}",
                p,
                file.display()
            );
            let _ = agent_doc_tmux_io::kill_pane(tmux, p);
            continue;
        }
        if should_preserve_failed_route_pane(tmux, file, p, session_id) {
            eprintln!(
                "[route] preserving newly-created pane {} after failed route because it is still the live registered owner for {}",
                p,
                file.display()
            );
            continue;
        }
        eprintln!(
            "[route] cleaning up orphaned pane {} (created during failed route)",
            p
        );
        let _ = agent_doc_tmux_io::kill_pane(tmux, p);
    }
}

pub fn failed_route_pane_has_startup_miss(file: &Path, pane_id: &str) -> bool {
    agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        .ok()
        .flatten()
        .is_some_and(|miss| {
            miss.pane_id == pane_id
                && matches!(
                    miss.origin,
                    agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart
                )
        })
}

pub fn failed_route_registry_root(file: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(|parent| parent.to_path_buf()))
}

pub fn should_preserve_failed_route_pane(
    tmux: &Tmux,
    file: &Path,
    pane_id: &str,
    session_id: &str,
) -> bool {
    let Some(root) = failed_route_registry_root(file) else {
        return false;
    };
    agent_doc_session_registry_io::load_in(&root)
        .ok()
        .and_then(|registry| {
            registry
                .values()
                .find(|entry| entry.session_id == session_id)
                .map(|entry| entry.pane.as_str() == pane_id)
        })
        .unwrap_or(false)
        && tmux.pane_alive(pane_id)
}

#[allow(clippy::too_many_arguments)]
pub fn recover_dispatch_only_authoritative_waiting_input(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
    harness: &HarnessConfig,
    pane: &str,
    generation: u64,
    effects: DispatchOnlyRouteEffects,
) -> Result<String> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_dispatch_only_waiting_input_restart file={} pane={} harness={} generation={}",
            file.display(),
            pane,
            harness.binary,
            generation
        ),
    );
    eprintln!(
        "[route] authoritative actor generation {} for {} is waiting for supervisor restart input on pane {} — restarting fresh once before the dispatch-only reroute",
        generation,
        file.display(),
        pane
    );
    let initial_status =
        agent_doc_supervisor_io::startup_miss::session_log_status(file, session_id)
            .ok()
            .flatten();

    if !restart_via_supervisor_with_mode(file, session_id, "fresh") {
        anyhow::bail!(
            "authoritative actor generation {} for {} owns pane {} but route could not restart the waiting supervisor fresh. Run `agent-doc start {}` manually to recover",
            generation,
            file.display(),
            pane,
            file.display()
        );
    }

    let dispatch_pane = match wait_for_starting_pane_recovery_target(
        tmux,
        file,
        session_id,
        pane,
        file_path,
        harness,
        initial_status.as_ref(),
    ) {
        Some(StartingPaneRecoveryTarget::DifferentPane(recovered)) => recovered,
        Some(StartingPaneRecoveryTarget::SamePane) | None => {
            resolve_fresh_dispatch_target_after_ready_wait(tmux, session_id, pane, file_path, None)?
        }
    };

    rescue_from_stash(
        tmux,
        &dispatch_pane,
        session_id,
        file_path,
        target_session,
        split_before,
    );
    register_dispatch_target(tmux, session_id, &dispatch_pane, file_path)?;
    dispatch_only_send_reopen(
        tmux,
        file,
        session_id,
        &dispatch_pane,
        file_path,
        harness,
        DispatchOnlySendReopenOptions {
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            queue_prompt_text: None,
            effects,
        },
    )
}

/// Rescue a pane from a stash window back to the agent-doc window.
/// Only rescues if the pane is in the target session -- never swaps across sessions.
///
/// Returns `true` when the pane was actually moved out of a stash window so callers
/// can re-evaluate state that depends on pane location (e.g. Starting->Ready
/// promotion after the rescue makes the pane visible). Returns `false` when the
/// rescue was a no-op (pane not in stash, or session guard tripped).
pub fn rescue_from_stash(
    tmux: &Tmux,
    pane_id: &str,
    session_id: &str,
    file_path: &str,
    target_session: &str,
    split_before: bool,
) -> bool {
    let pane_session = agent_doc_tmux_io::target_session_name(tmux, pane_id).unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return false;
    }

    let pane_win_name = agent_doc_tmux_io::target_window_name(tmux, pane_id).unwrap_or_default();

    if is_stash_window_name(&pane_win_name) {
        tracing::debug!(pane_id, window = %pane_win_name, target_session, "route: rescuing pane from stash");
        eprintln!(
            "[route] Pane {} is in stash window '{}', rescuing to agent-doc window",
            pane_id, pane_win_name
        );
        let agent_doc_window = format!("{}:agent-doc", target_session);
        let target_panes = tmux
            .list_window_panes(&agent_doc_window)
            .unwrap_or_default();
        let target = if split_before {
            target_panes.first()
        } else {
            target_panes.last()
        };
        let mut moved = false;
        if let Some(target) = target {
            let join_flag = if split_before { "-dbh" } else { "-dh" };
            match agent_doc_tmux_io::join_pane_guarded(
                tmux,
                pane_id,
                target,
                target_session,
                join_flag,
            ) {
                Ok(()) => {
                    eprintln!("[route] Rescued pane {} via join-pane", pane_id);
                    moved = true;
                }
                Err(e) => eprintln!("[route] join-pane rescue failed for {} ({})", pane_id, e),
            }
        }
        if let Err(e) = register_dispatch_target(tmux, session_id, pane_id, file_path) {
            eprintln!("[route] warning: re-register failed: {}", e);
        }
        return moved;
    }
    false
}

pub fn controller_dispatch_actor_state(
    actor_state: agent_doc_sqlite::state_store::ActorState,
) -> DispatchActorState {
    match actor_state {
        agent_doc_sqlite::state_store::ActorState::Ready => DispatchActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => DispatchActorState::Busy,
        _ => DispatchActorState::Other,
    }
}
