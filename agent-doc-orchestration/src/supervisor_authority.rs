//! # Module: supervisor_authority
//!
//! ## Spec (`#pcpc5e1` / `#dav9` — 08b supervisor-host cutover gate ladder)
//! Drives the `specs/08b-single-process-control-plane.md` migration for the
//! **harness-child host**: moving the PTY/stdin/restart/heartbeat hosting from
//! the out-of-process `agent-doc start` supervisor (its own foreground process
//! in a tmux pane, see [`crate::start`] + `supervisor::ipc`) onto the in-process
//! supervisor adapter ([`crate::supervisor::in_process::InProcessSupervisor`],
//! built + SimWorld-tested in `#pcpc2`) hosted as a controller-managed task.
//!
//! This is the supervisor-side companion to [`crate::write_authority`]: same
//! rollback-flag ladder discipline, applied to *who hosts the child* rather than
//! *who serializes document writes*. Together they retire the supervisor
//! self-race / exit-75 family (`#ipc-crdt-response-drift` / R6): once the host
//! and the write serializer live in one process, a route-owned supervisor write
//! and an agent finalize write can no longer interleave between `flock`
//! acquisitions across two processes.
//!
//! Per 08b §"Migration gates", the cutover follows a gated ladder, each rung with
//! a rollback flag that returns the previous host authority:
//!
//! - **`off`** (default) — exactly today's behavior: the out-of-process
//!   `agent-doc start` supervisor hosts the child in its own tmux-pane process.
//!   Shipped users are unchanged until an operator opts in. Rollback target for
//!   every later rung.
//! - **`shadow`** (`#pcpc5e1` first rung) — the out-of-process supervisor still
//!   hosts the real child (unchanged authority); the start path additionally
//!   *reports* that the in-process adapter is the configured cutover target to
//!   `ops.log`, so an operator can confirm the wiring before flipping authority.
//!   Observe-only: zero behavior change to child hosting.
//! - **`in-process`** (`#pcpc5e1` authority rung) — `agent-doc start` hosts the
//!   harness child through [`InProcessSupervisor`] driven by the controller task
//!   loop instead of the inline out-of-process supervisor. The external
//!   Unix-socket IPC boundary for CLI/editor callers is preserved either way.
//!   Landing this rung is the hosting-swap follow-on (gated under `#pcpc5`).
//!
//! ## Agentic Contracts
//! - [`current_mode`] reads `AGENT_DOC_SUPERVISOR` on every call (cheap, no
//!   caching) so an operator can advance or roll back the gate without
//!   restarting a long-lived process. An unrecognized value logs one warning and
//!   falls back to `Off` (fail-safe to current behavior — a host decision must
//!   never fail closed and strand a session).
//!
//! ## Evals
//! - `mode_parses_known_values_and_defaults_off`
//! - `mode_unknown_value_falls_back_off`
//! - `mode_hosts_in_process_only_at_authority_rung`

/// Environment variable that selects the active supervisor-host migration gate.
pub const ENV_VAR: &str = "AGENT_DOC_SUPERVISOR";

/// The 08b supervisor-host migration gate (see module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorMode {
    /// Out-of-process `agent-doc start` supervisor hosts the child (today's
    /// behavior). Default + rollback target.
    Off,
    /// Observe-only: the out-of-process supervisor still hosts the real child;
    /// the start path reports the configured in-process cutover target.
    Shadow,
    /// `agent-doc start` hosts the child through the in-process adapter driven by
    /// the controller task loop (cutover authority rung).
    InProcess,
}

impl SupervisorMode {
    /// Parse a mode token. Accepts the canonical spellings plus a few
    /// punctuation/case variants. Returns `None` for unknown values so the caller
    /// can warn and fail safe.
    pub fn parse(s: &str) -> Option<SupervisorMode> {
        match s.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "" | "off" | "out-of-process" | "oop" => Some(SupervisorMode::Off),
            "shadow" => Some(SupervisorMode::Shadow),
            "in-process" | "inprocess" => Some(SupervisorMode::InProcess),
            _ => None,
        }
    }

    /// Stable lowercase token for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            SupervisorMode::Off => "off",
            SupervisorMode::Shadow => "shadow",
            SupervisorMode::InProcess => "in-process",
        }
    }

    /// Whether this rung hosts the harness child through the in-process adapter
    /// (only the `in-process` authority rung; `off`/`shadow` keep the
    /// out-of-process supervisor as the real host).
    pub fn hosts_in_process(self) -> bool {
        matches!(self, SupervisorMode::InProcess)
    }
}

/// Resolve the active mode from the environment on each call. An unrecognized
/// value warns once to stderr and falls back to `Off` (never fail-closed on a
/// host decision).
pub fn current_mode() -> SupervisorMode {
    match std::env::var(ENV_VAR) {
        Ok(raw) => SupervisorMode::parse(&raw).unwrap_or_else(|| {
            eprintln!("[supervisor-authority] WARNING: unrecognized {ENV_VAR}={raw:?}; falling back to off");
            SupervisorMode::Off
        }),
        Err(_) => SupervisorMode::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parses_known_values_and_defaults_off() {
        assert_eq!(SupervisorMode::parse(""), Some(SupervisorMode::Off));
        assert_eq!(SupervisorMode::parse("off"), Some(SupervisorMode::Off));
        assert_eq!(
            SupervisorMode::parse("out_of_process"),
            Some(SupervisorMode::Off)
        );
        assert_eq!(SupervisorMode::parse("SHADOW"), Some(SupervisorMode::Shadow));
        assert_eq!(
            SupervisorMode::parse(" In-Process "),
            Some(SupervisorMode::InProcess)
        );
        assert_eq!(
            SupervisorMode::parse("inprocess"),
            Some(SupervisorMode::InProcess)
        );
        assert_eq!(SupervisorMode::Off.as_str(), "off");
        assert_eq!(SupervisorMode::InProcess.as_str(), "in-process");
    }

    #[test]
    fn mode_unknown_value_falls_back_off() {
        assert_eq!(SupervisorMode::parse("banana"), None);
    }

    #[test]
    fn mode_hosts_in_process_only_at_authority_rung() {
        assert!(!SupervisorMode::Off.hosts_in_process());
        assert!(!SupervisorMode::Shadow.hosts_in_process());
        assert!(SupervisorMode::InProcess.hosts_in_process());
    }
}
