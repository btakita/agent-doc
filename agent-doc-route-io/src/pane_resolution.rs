//! Route pane-resolution helper I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::pane_provenance::pane_route_provenance;
use agent_doc_controller::dispatch::{
    DispatchActorState, DispatchRuntimeHealth, StartupMissRouteFacts,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::route_runtime::SupervisorHealth;
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

pub fn controller_dispatch_actor_state(
    actor_state: agent_doc_sqlite::state_store::ActorState,
) -> DispatchActorState {
    match actor_state {
        agent_doc_sqlite::state_store::ActorState::Ready => DispatchActorState::Ready,
        agent_doc_sqlite::state_store::ActorState::Busy => DispatchActorState::Busy,
        _ => DispatchActorState::Other,
    }
}
