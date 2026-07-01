//! Supervisor process effect boundary.
//!
//! This crate describes process/socket effects for supervisors. Concrete
//! process execution can live here as adapters are extracted from
//! `agent-doc-orchestration`.

use agent_doc_supervisor::SupervisorBinding;
use serde::{Deserialize, Serialize};

pub mod resize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SupervisorProcessCommand {
    Start { pane_id: String },
    RestartSamePane { binding: SupervisorBinding },
    Kill { supervisor_instance_id: String },
    UnlinkSocket { path: String },
}

impl SupervisorProcessCommand {
    pub fn preserves_pane(&self, pane_id: &str) -> bool {
        match self {
            SupervisorProcessCommand::RestartSamePane { binding } => binding.pane_id == pane_id,
            _ => false,
        }
    }
}

pub const REEXEC_CHILD_PID_ENV: &str = "AGENT_DOC_REEXEC_CHILD_PID";
pub const REEXEC_MASTER_FD_ENV: &str = "AGENT_DOC_REEXEC_MASTER_FD";
pub const ROUTE_BIN_ENV: &str = "AGENT_DOC_ROUTE_BIN";

pub fn agent_doc_start_bin() -> String {
    resolve_agent_doc_start_bin(
        std::env::var(ROUTE_BIN_ENV).ok(),
        std::env::current_exe().ok(),
    )
}

fn resolve_agent_doc_start_bin(
    route_bin_override: Option<String>,
    current_exe: Option<std::path::PathBuf>,
) -> String {
    if let Some(override_bin) = route_bin_override
        && !override_bin.trim().is_empty()
    {
        return override_bin;
    }

    current_exe
        .unwrap_or_else(|| "agent-doc".into())
        .to_string_lossy()
        .to_string()
}

/// State handed from a stale supervisor to its freshly-execed replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReexecState {
    pub child_pid: u32,
    pub master_fd: i32,
}

impl ReexecState {
    pub fn parse(pid: &str, fd: &str) -> Option<Self> {
        let child_pid = pid.trim().parse::<u32>().ok()?;
        let master_fd = fd.trim().parse::<i32>().ok()?;
        if child_pid == 0 || master_fd < 0 {
            return None;
        }
        Some(Self {
            child_pid,
            master_fd,
        })
    }

    pub fn from_env() -> Option<Self> {
        Self::parse(
            &std::env::var(REEXEC_CHILD_PID_ENV).ok()?,
            &std::env::var(REEXEC_MASTER_FD_ENV).ok()?,
        )
    }

    #[cfg(unix)]
    pub fn child_survived(self) -> bool {
        let ret = unsafe { libc::kill(self.child_pid as libc::pid_t, 0) };
        if ret == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(not(unix))]
    pub fn child_survived(self) -> bool {
        true
    }

    pub fn to_env(self) -> [(String, String); 2] {
        [
            (REEXEC_CHILD_PID_ENV.to_string(), self.child_pid.to_string()),
            (REEXEC_MASTER_FD_ENV.to_string(), self.master_fd.to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_command_preserves_old_pane_identity() {
        let command = SupervisorProcessCommand::RestartSamePane {
            binding: SupervisorBinding {
                pane_id: "%34".to_string(),
                generation: 7,
                supervisor_instance_id: Some("old".to_string()),
            },
        };

        assert!(command.preserves_pane("%34"));
        assert!(!command.preserves_pane("%35"));
    }

    #[test]
    fn reexec_state_parses_valid_handoff() {
        assert_eq!(
            ReexecState::parse("4242", "7"),
            Some(ReexecState {
                child_pid: 4242,
                master_fd: 7,
            })
        );
        assert_eq!(
            ReexecState::parse(" 9 ", " 12 "),
            Some(ReexecState {
                child_pid: 9,
                master_fd: 12,
            })
        );
    }

    #[test]
    fn reexec_state_rejects_invalid_handoff() {
        assert_eq!(ReexecState::parse("0", "7"), None);
        assert_eq!(ReexecState::parse("4242", "-1"), None);
        assert_eq!(ReexecState::parse("x", "7"), None);
        assert_eq!(ReexecState::parse("4242", "fd"), None);
    }

    #[test]
    fn reexec_state_env_round_trips() {
        let state = ReexecState {
            child_pid: 4242,
            master_fd: 7,
        };
        let env = state.to_env();
        assert_eq!(env[0].0, REEXEC_CHILD_PID_ENV);
        assert_eq!(env[0].1, "4242");
        assert_eq!(env[1].0, REEXEC_MASTER_FD_ENV);
        assert_eq!(env[1].1, "7");
        assert_eq!(ReexecState::parse(&env[0].1, &env[1].1), Some(state));
    }

    #[test]
    fn agent_doc_start_bin_respects_nonblank_route_override() {
        assert_eq!(
            resolve_agent_doc_start_bin(
                Some("/tmp/custom-agent-doc".to_string()),
                Some("/usr/bin/agent-doc".into())
            ),
            "/tmp/custom-agent-doc"
        );
    }

    #[test]
    fn agent_doc_start_bin_ignores_blank_route_override() {
        assert_eq!(
            resolve_agent_doc_start_bin(
                Some(" \t ".to_string()),
                Some("/usr/bin/agent-doc".into())
            ),
            "/usr/bin/agent-doc"
        );
    }

    #[test]
    fn agent_doc_start_bin_falls_back_to_agent_doc_when_current_exe_is_unavailable() {
        assert_eq!(resolve_agent_doc_start_bin(None, None), "agent-doc");
    }

    #[cfg(unix)]
    #[test]
    fn reexec_state_child_survived_distinguishes_live_from_dead_pid() {
        let live = ReexecState {
            child_pid: std::process::id(),
            master_fd: 7,
        };
        assert!(live.child_survived());

        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn a short-lived child");
        let dead_pid = child.id();
        child.wait().expect("reap the child");
        let dead = ReexecState {
            child_pid: dead_pid,
            master_fd: 7,
        };
        assert!(!dead.child_survived());
    }
}
