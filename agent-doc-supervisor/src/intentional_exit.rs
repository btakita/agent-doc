//! `#gh133sigterm` (GH #133 follow-up): a deliberately terminated supervisor is
//! not a crashed one.
//!
//! The controller's dead-supervisor watchdog (`#supresilience` Part B) can only
//! see that a recorded `supervisor_pid` is gone. Without a termination handler
//! a `SIGTERM` and a panic/abort look identical from outside, so the watchdog
//! resurrected a supervisor the operator had just killed whenever its pane
//! still filled a visible column.
//!
//! The supervisor's `SIGTERM` handler therefore records a durable
//! [`IntentionalExitMarker`] naming the exact pid/generation that is exiting,
//! and the watchdog honours it ([`watchdog_intentional_exit_decision`]). The
//! marker is pid-scoped, so it can never suppress the restart of a *later*
//! generation, and the controller-initiated replacement path (`recycle` /
//! `session restart-supervisor` / `admin recycle --force`, which SIGTERMs the
//! old supervisor and then cold-starts the new one itself) clears it: a
//! replacement is controller intent to *continue* the document, not operator
//! intent to stop it.

use serde::{Deserialize, Serialize};

/// `document_runtime_state.state_kind` for the marker row.
pub const INTENTIONAL_EXIT_STATE_KIND: &str = "supervisor_intentional_exit";

/// Durable record that a specific supervisor process exited on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentionalExitMarker {
    /// The exiting supervisor process.
    pub supervisor_pid: u32,
    /// The actor generation that process served.
    pub generation: u64,
    /// The pane the exiting generation was bound to.
    pub pane_id: String,
    /// The supervisor session id.
    pub session_id: String,
    /// The signal that requested the exit (`SIGTERM`).
    pub signal: String,
    /// Wall-clock milliseconds when the handler recorded the exit.
    pub recorded_at_ms: u64,
}

/// What the crash watchdog should make of a dead supervisor given the marker
/// row (if any) for its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentionalExitDecision {
    /// No marker: the death is unexplained, so it is treated as a crash.
    NoMarker,
    /// A marker exists but names a different pid or generation (left behind by
    /// an earlier supervisor). It says nothing about this death.
    StaleMarker,
    /// The marker names exactly this dead pid and generation: the supervisor
    /// was deliberately terminated and must NOT be respawned.
    Intentional,
}

impl IntentionalExitDecision {
    /// Whether the watchdog may still treat the death as a crash.
    pub const fn allows_restart(self) -> bool {
        !matches!(self, Self::Intentional)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoMarker => "no_marker",
            Self::StaleMarker => "stale_marker",
            Self::Intentional => "intentional_exit",
        }
    }
}

/// Decide whether the dead supervisor `dead_pid` serving `generation` exited
/// deliberately. Only an exact pid + generation match is honoured, so a marker
/// can never block the crash recovery of a later generation.
pub fn watchdog_intentional_exit_decision(
    marker: Option<&IntentionalExitMarker>,
    dead_pid: u32,
    generation: u64,
) -> IntentionalExitDecision {
    match marker {
        None => IntentionalExitDecision::NoMarker,
        Some(marker) if marker.supervisor_pid == dead_pid && marker.generation == generation => {
            IntentionalExitDecision::Intentional
        }
        Some(_) => IntentionalExitDecision::StaleMarker,
    }
}

/// Parse a stored marker payload. A malformed payload is treated as absent:
/// it cannot prove intent, so the death stays a crash.
pub fn parse_intentional_exit_marker(payload_json: &str) -> Option<IntentionalExitMarker> {
    serde_json::from_str(payload_json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(pid: u32, generation: u64) -> IntentionalExitMarker {
        IntentionalExitMarker {
            supervisor_pid: pid,
            generation,
            pane_id: "%7".to_string(),
            session_id: "session".to_string(),
            signal: "SIGTERM".to_string(),
            recorded_at_ms: 1,
        }
    }

    /// Deliberate SIGTERM: the marker names the dead pid + generation, so the
    /// watchdog must not respawn.
    #[test]
    fn deliberate_sigterm_marker_blocks_watchdog_respawn() {
        let decision = watchdog_intentional_exit_decision(Some(&marker(42, 3)), 42, 3);
        assert_eq!(decision, IntentionalExitDecision::Intentional);
        assert!(!decision.allows_restart());
    }

    /// Crash: no handler ran, so no marker exists — the crash watchdog keeps
    /// its `#supresilience` behaviour.
    #[test]
    fn crash_without_marker_still_respawns() {
        let decision = watchdog_intentional_exit_decision(None, 42, 3);
        assert_eq!(decision, IntentionalExitDecision::NoMarker);
        assert!(decision.allows_restart());
    }

    /// A marker left by an earlier supervisor (a replaced generation, or a
    /// reused pid serving another generation) never suppresses a later crash.
    #[test]
    fn marker_for_another_pid_or_generation_is_stale() {
        for (pid, generation) in [(41, 3), (42, 2), (7, 9)] {
            let decision =
                watchdog_intentional_exit_decision(Some(&marker(pid, generation)), 42, 3);
            assert_eq!(decision, IntentionalExitDecision::StaleMarker);
            assert!(decision.allows_restart());
        }
    }

    #[test]
    fn marker_round_trips_and_malformed_payload_is_absent() {
        let original = marker(42, 3);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(parse_intentional_exit_marker(&json), Some(original));
        assert_eq!(parse_intentional_exit_marker("{not json"), None);
    }
}
