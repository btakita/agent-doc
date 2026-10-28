//! Supervisor process effect boundary.
//!
//! This crate describes process/socket effects for supervisors. Concrete
//! process execution can live here as adapters are extracted from
//! `agent-doc-orchestration`.

use agent_doc_supervisor::SupervisorBinding;
use serde::{Deserialize, Serialize};

pub mod detached_child;
pub mod in_process;
pub mod io_handoff;
pub mod io_threads;
pub mod memory_guard;
pub mod output_state;
pub mod pty;
pub mod reexec_bridge;
pub mod resize;
pub mod route_owned_completion;
pub mod screen;
pub mod session_liveness;
pub mod shared_writer;
/// GH #380 / `#headlessharnesspath`: the resolver lives in `agent-doc-harness` so
/// the headless backends in `agent-doc-agent-io` share it; re-exported here so
/// `supervisor_process::spawn_program` keeps resolving for the pty spawn.
pub use agent_doc_harness::spawn_program;
pub mod start_command;

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
pub const REEXEC_REDISPATCH_IF_CHILD_DIED_ENV: &str = "AGENT_DOC_REEXEC_REDISPATCH_IF_CHILD_DIED";
pub const REEXEC_CAPABILITY_PROOF_CONTRACT_ENV: &str = "AGENT_DOC_REEXEC_CAPABILITY_PROOF_CONTRACT";
/// Set on a same-child hot-reexec when the outgoing supervisor had promoted its
/// route-owned start purpose from `layout-provision` to `dispatch`.
///
/// `#reexecpurposelost`: the reexec replays the ORIGINAL argv, which still says
/// `--route-owned-start-purpose layout-provision`. The promotion lived only in the
/// completion thread's memory, so the fresh image treated a pane carrying a live
/// conversation as an unused layout placeholder and reaped it at the next commit.
pub const REEXEC_ROUTE_OWNED_DISPATCH_PROMOTED_ENV: &str =
    "AGENT_DOC_REEXEC_ROUTE_OWNED_DISPATCH_PROMOTED";
/// Harness binary of the preserved child, so the replacement image's
/// `#reexecptybridge` applies the same output filter policy before start
/// admission resolves the harness. Absent from an older outgoing image: the
/// bridge then uses the default policy.
pub const REEXEC_HARNESS_ENV: &str = "AGENT_DOC_REEXEC_HARNESS";
/// PID of the outgoing supervisor image. `execve` preserves it, so the
/// replacement proves it is that image (not a subprocess that inherited the
/// handoff environment) before its `#reexecptybridge` touches the master fd.
pub const REEXEC_SUPERVISOR_PID_ENV: &str = "AGENT_DOC_REEXEC_SUPERVISOR_PID";
/// The outgoing idle watch's queue auto-clear baseline (`#autoclearbaseline`),
/// encoded by `QueueAutoClearGenerationBaseline::encode_reexec_handoff`. The
/// adopted child keeps its context across `execve`, so the replacement image
/// continues this baseline instead of recording a fresh one. Absent from an
/// older outgoing image (or an oversized one): the replacement records a
/// fresh baseline, as before.
pub const REEXEC_QUEUE_AUTO_CLEAR_BASELINE_ENV: &str = "AGENT_DOC_REEXEC_QUEUE_AUTO_CLEAR_BASELINE";
pub const ROUTE_BIN_ENV: &str = "AGENT_DOC_ROUTE_BIN";

/// Start purpose the supervisor runs under after (re)entry.
///
/// Only an adopted, still-living child inherits the outgoing image's promotion —
/// the promotion is a fact about THAT child (it carried a real turn). A fresh
/// spawn, or an adopt whose child died, gets a new child and the declared purpose.
pub fn reentry_route_owned_start_purpose(
    declared: agent_doc_supervisor::route_owned::RouteOwnedStartPurpose,
    adopted_child_survived: bool,
    promoted_env: Option<&str>,
) -> agent_doc_supervisor::route_owned::RouteOwnedStartPurpose {
    if adopted_child_survived && promoted_env.is_some_and(|value| value.trim() == "1") {
        agent_doc_supervisor::route_owned::RouteOwnedStartPurpose::Dispatch
    } else {
        declared
    }
}

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

    if let Some(current_exe) = current_exe {
        let current_exe = current_exe.to_string_lossy();
        if !current_exe.ends_with(" (deleted)") {
            return current_exe.to_string();
        }
    }

    "agent-doc".to_string()
}

/// State handed from a stale supervisor to its freshly-execed replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReexecState {
    pub child_pid: u32,
    pub master_fd: i32,
    /// Whether a missing child may re-submit the interrupted document turn.
    /// Open-cycle live-child handoffs set this false: their safety contract is
    /// adoption only, never replay of an in-flight turn.
    pub redispatch_if_child_died: bool,
}

impl ReexecState {
    pub fn parse(pid: &str, fd: &str, redispatch_if_child_died: Option<&str>) -> Option<Self> {
        let child_pid = pid.trim().parse::<u32>().ok()?;
        let master_fd = fd.trim().parse::<i32>().ok()?;
        if child_pid == 0 || master_fd < 0 {
            return None;
        }
        Some(Self {
            child_pid,
            master_fd,
            // Missing means an older outgoing image, whose established
            // behavior was to recover a dead child by redispatching once.
            redispatch_if_child_died: redispatch_if_child_died != Some("0"),
        })
    }

    pub fn from_env() -> Option<Self> {
        Self::parse(
            &std::env::var(REEXEC_CHILD_PID_ENV).ok()?,
            &std::env::var(REEXEC_MASTER_FD_ENV).ok()?,
            std::env::var(REEXEC_REDISPATCH_IF_CHILD_DIED_ENV)
                .ok()
                .as_deref(),
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

    pub fn to_env(self) -> [(String, String); 3] {
        [
            (REEXEC_CHILD_PID_ENV.to_string(), self.child_pid.to_string()),
            (REEXEC_MASTER_FD_ENV.to_string(), self.master_fd.to_string()),
            (
                REEXEC_REDISPATCH_IF_CHILD_DIED_ENV.to_string(),
                if self.redispatch_if_child_died {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reentry_keeps_a_promoted_dispatch_purpose_only_for_the_adopted_child() {
        use agent_doc_supervisor::route_owned::RouteOwnedStartPurpose as P;
        // `#reexecpurposelost`: the lazily pane was promoted, self-recycled, and
        // came back as layout-provision — then reaped as a stashed orphan.
        assert_eq!(
            reentry_route_owned_start_purpose(P::LayoutProvision, true, Some("1")),
            P::Dispatch
        );
        assert_eq!(
            reentry_route_owned_start_purpose(P::LayoutProvision, true, None),
            P::LayoutProvision,
            "no promotion was handed over"
        );
        assert_eq!(
            reentry_route_owned_start_purpose(P::LayoutProvision, false, Some("1")),
            P::LayoutProvision,
            "a new child never interacted; the promotion belonged to the dead one"
        );
        assert_eq!(
            reentry_route_owned_start_purpose(P::LayoutProvision, true, Some("0")),
            P::LayoutProvision
        );
        assert_eq!(
            reentry_route_owned_start_purpose(P::Dispatch, false, None),
            P::Dispatch
        );
    }

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
            ReexecState::parse("4242", "7", None),
            Some(ReexecState {
                child_pid: 4242,
                master_fd: 7,
                redispatch_if_child_died: true,
            })
        );
        assert_eq!(
            ReexecState::parse(" 9 ", " 12 ", Some("0")),
            Some(ReexecState {
                child_pid: 9,
                master_fd: 12,
                redispatch_if_child_died: false,
            })
        );
    }

    #[test]
    fn reexec_state_rejects_invalid_handoff() {
        assert_eq!(ReexecState::parse("0", "7", None), None);
        assert_eq!(ReexecState::parse("4242", "-1", None), None);
        assert_eq!(ReexecState::parse("x", "7", None), None);
        assert_eq!(ReexecState::parse("4242", "fd", None), None);
    }

    #[test]
    fn reexec_state_env_round_trips() {
        let state = ReexecState {
            child_pid: 4242,
            master_fd: 7,
            redispatch_if_child_died: false,
        };
        let env = state.to_env();
        assert_eq!(env[0].0, REEXEC_CHILD_PID_ENV);
        assert_eq!(env[0].1, "4242");
        assert_eq!(env[1].0, REEXEC_MASTER_FD_ENV);
        assert_eq!(env[1].1, "7");
        assert_eq!(env[2].0, REEXEC_REDISPATCH_IF_CHILD_DIED_ENV);
        assert_eq!(env[2].1, "0");
        assert_eq!(
            ReexecState::parse(&env[0].1, &env[1].1, Some(&env[2].1)),
            Some(state)
        );
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

    #[test]
    fn agent_doc_start_bin_ignores_deleted_current_exe() {
        assert_eq!(
            resolve_agent_doc_start_bin(
                None,
                Some("/home/brian/.cargo/bin/agent-doc (deleted)".into())
            ),
            "agent-doc"
        );
    }

    #[test]
    fn agent_doc_start_bin_keeps_override_when_current_exe_is_deleted() {
        assert_eq!(
            resolve_agent_doc_start_bin(
                Some("/home/brian/.cargo/bin/agent-doc".to_string()),
                Some("/home/brian/.cargo/bin/agent-doc (deleted)".into())
            ),
            "/home/brian/.cargo/bin/agent-doc"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reexec_state_child_survived_distinguishes_live_from_dead_pid() {
        let live = ReexecState {
            child_pid: std::process::id(),
            master_fd: 7,
            redispatch_if_child_died: false,
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
            redispatch_if_child_died: false,
        };
        assert!(!dead.child_survived());
    }
}
