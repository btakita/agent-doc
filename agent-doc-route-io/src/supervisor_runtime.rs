//! Route-facing supervisor runtime IPC.

use agent_doc_supervisor::ipc_protocol::IpcMethod;
use agent_doc_supervisor::route_runtime::{RouteActorState, SupervisorHealth, SupervisorRuntime};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorRestartRequestOutcome {
    Accepted,
    Rejected(String),
    Unavailable(String),
}

fn supervisor_runtime_from_state_data(data: &serde_json::Value) -> SupervisorRuntime {
    let running = data
        .get("running")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let restart_count = data
        .get("restart_count")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let actor_state = data
        .get("actor_state")
        .and_then(|v| v.as_str())
        .and_then(RouteActorState::parse);
    let current_harness = data
        .get("current_harness")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(agent_doc_harness::normalize_harness_name);
    let health = if running && state == "healthy" {
        SupervisorHealth::Healthy
    } else if state == "halted" {
        SupervisorHealth::Halted { restart_count }
    } else {
        SupervisorHealth::Restartable
    };
    SupervisorRuntime {
        health,
        actor_state,
        current_harness,
    }
}

pub fn supervisor_socket_path(file: &Path, session_id: &str) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    Some(agent_doc_supervisor_io::ipc::socket_path(
        &project_root,
        session_id,
    ))
}

pub fn query_supervisor_runtime(file: &Path, session_id: &str) -> SupervisorRuntime {
    let Some(sock) = supervisor_socket_path(file, session_id) else {
        return SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            current_harness: None,
        };
    };
    if !sock.exists() {
        return SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            current_harness: None,
        };
    }
    match agent_doc_supervisor_io::ipc::send_command(&sock, &IpcMethod::State) {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                supervisor_runtime_from_state_data(data)
            } else {
                SupervisorRuntime {
                    health: SupervisorHealth::Restartable,
                    actor_state: None,
                    current_harness: None,
                }
            }
        }
        Ok(_) | Err(_) => SupervisorRuntime {
            health: SupervisorHealth::Unreachable,
            actor_state: None,
            current_harness: None,
        },
    }
}

pub fn query_supervisor_health(file: &Path, session_id: &str) -> SupervisorHealth {
    query_supervisor_runtime(file, session_id).health
}

pub fn request_restart_via_supervisor_with_mode(
    file: &Path,
    session_id: &str,
    mode: &str,
) -> SupervisorRestartRequestOutcome {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(err) => {
            return SupervisorRestartRequestOutcome::Unavailable(format!(
                "canonicalize {}: {err}",
                file.display()
            ));
        }
    };
    let project_root = match agent_doc_project_root_io::project_root_containing(&canonical) {
        Some(r) => r,
        None => {
            return SupervisorRestartRequestOutcome::Unavailable(format!(
                "no project root contains {}",
                canonical.display()
            ));
        }
    };
    let sock = agent_doc_supervisor_io::ipc::socket_path(&project_root, session_id);
    let method = IpcMethod::Restart {
        mode: mode.to_string(),
    };
    match agent_doc_supervisor_io::ipc::send_command(&sock, &method) {
        Ok(resp) if resp.ok => SupervisorRestartRequestOutcome::Accepted,
        Ok(resp) => SupervisorRestartRequestOutcome::Rejected(
            resp.error
                .unwrap_or_else(|| "supervisor rejected restart without a reason".to_string()),
        ),
        Err(err) => SupervisorRestartRequestOutcome::Unavailable(format!("{err:#}")),
    }
}

pub fn restart_via_supervisor_with_mode(file: &Path, session_id: &str, mode: &str) -> bool {
    matches!(
        request_restart_via_supervisor_with_mode(file, session_id, mode),
        SupervisorRestartRequestOutcome::Accepted
    )
}

pub fn restart_via_supervisor(file: &Path, session_id: &str) -> bool {
    restart_via_supervisor_with_mode(file, session_id, "continue")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_projection_reads_and_normalizes_live_harness_identity() {
        let runtime = supervisor_runtime_from_state_data(&serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 2,
            "current_harness": "claude",
        }));

        assert_eq!(runtime.health, SupervisorHealth::Healthy);
        assert_eq!(runtime.actor_state, Some(RouteActorState::Ready));
        assert_eq!(runtime.current_harness.as_deref(), Some("claude-code"));
    }
}
