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
//!
//! `#runfrontendcrashed`: the replacement path must not simply *clear* the
//! marker before it cold-starts the new supervisor. The cold start is typed into
//! the pane's shell and needs seconds to register a new lease, and during that
//! window the lease still names the dead pid. With the marker gone the watchdog
//! read the replacement's own kill as a crash, issued a SECOND replacement, and
//! that one force-killed the booting cold-started supervisor (IPC socket not up
//! yet ⇒ "dead"), leaving the operator at a bare shell. The replacement path
//! therefore rewrites the marker as a time-bounded
//! [`REPLACEMENT_COLD_START_SIGNAL`] marker: the watchdog stands down for
//! [`REPLACEMENT_COLD_START_GRACE_MS`] (the cold start owns recovery), and once
//! the grace elapses an unregistered cold start is treated as a crash again, so a
//! failed cold start is still recovered.

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

/// `IntentionalExitMarker::signal` written by the controller replacement path
/// once it has stopped the old supervisor and is cold-starting its successor.
pub const REPLACEMENT_COLD_START_SIGNAL: &str = "controller_replacement_cold_start";

/// How long a [`REPLACEMENT_COLD_START_SIGNAL`] marker keeps the watchdog off
/// the replaced pid. Covers a cold start's shell launch, admission, and the
/// route's 120 s harness-ready budget with headroom.
pub const REPLACEMENT_COLD_START_GRACE_MS: u64 = 180_000;

/// Rewrite (or synthesize) the marker for a supervisor the controller
/// replacement path just stopped, so the watchdog leaves its cold start alone
/// for [`REPLACEMENT_COLD_START_GRACE_MS`]. `prior` is the SIGTERM handler's own
/// marker when it managed to record one; otherwise `killed_pid` (the pid the
/// replacement signalled) identifies the stopped supervisor.
pub fn replacement_cold_start_marker(
    prior: Option<&IntentionalExitMarker>,
    killed_pid: Option<u32>,
    generation: u64,
    pane_id: &str,
    session_id: &str,
    now_ms: u64,
) -> Option<IntentionalExitMarker> {
    let supervisor_pid = killed_pid.or_else(|| {
        prior
            .filter(|marker| marker.generation == generation)
            .map(|marker| marker.supervisor_pid)
    })?;
    Some(IntentionalExitMarker {
        supervisor_pid,
        generation,
        pane_id: pane_id.to_string(),
        session_id: session_id.to_string(),
        signal: REPLACEMENT_COLD_START_SIGNAL.to_string(),
        recorded_at_ms: now_ms,
    })
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
    /// `#runfrontendcrashed`: the controller replaced this pid and its cold
    /// start is still inside the grace window. The cold start owns recovery; a
    /// second replacement would kill the booting successor.
    ReplacementColdStartPending,
    /// The controller replaced this pid but the cold start never registered a
    /// successor within the grace window: recover it as a crash.
    ReplacementColdStartExpired,
}

impl IntentionalExitDecision {
    /// Whether the watchdog may still treat the death as a crash.
    pub const fn allows_restart(self) -> bool {
        !matches!(self, Self::Intentional | Self::ReplacementColdStartPending)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoMarker => "no_marker",
            Self::StaleMarker => "stale_marker",
            Self::Intentional => "intentional_exit",
            Self::ReplacementColdStartPending => "replacement_cold_start_pending",
            Self::ReplacementColdStartExpired => "replacement_cold_start_expired",
        }
    }
}

/// Decide whether the dead supervisor `dead_pid` serving `generation` exited
/// deliberately. Only an exact pid + generation match is honoured, so a marker
/// can never block the crash recovery of a later generation. `now_ms` ages a
/// [`REPLACEMENT_COLD_START_SIGNAL`] marker against
/// [`REPLACEMENT_COLD_START_GRACE_MS`].
pub fn watchdog_intentional_exit_decision(
    marker: Option<&IntentionalExitMarker>,
    dead_pid: u32,
    generation: u64,
    now_ms: u64,
) -> IntentionalExitDecision {
    match marker {
        None => IntentionalExitDecision::NoMarker,
        Some(marker) if marker.supervisor_pid == dead_pid && marker.generation == generation => {
            if marker.signal == REPLACEMENT_COLD_START_SIGNAL {
                if now_ms.saturating_sub(marker.recorded_at_ms) < REPLACEMENT_COLD_START_GRACE_MS {
                    IntentionalExitDecision::ReplacementColdStartPending
                } else {
                    IntentionalExitDecision::ReplacementColdStartExpired
                }
            } else {
                IntentionalExitDecision::Intentional
            }
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
        let decision = watchdog_intentional_exit_decision(Some(&marker(42, 3)), 42, 3, 1);
        assert_eq!(decision, IntentionalExitDecision::Intentional);
        assert!(!decision.allows_restart());
    }

    /// Crash: no handler ran, so no marker exists — the crash watchdog keeps
    /// its `#supresilience` behaviour.
    #[test]
    fn crash_without_marker_still_respawns() {
        let decision = watchdog_intentional_exit_decision(None, 42, 3, 1);
        assert_eq!(decision, IntentionalExitDecision::NoMarker);
        assert!(decision.allows_restart());
    }

    /// A marker left by an earlier supervisor (a replaced generation, or a
    /// reused pid serving another generation) never suppresses a later crash.
    #[test]
    fn marker_for_another_pid_or_generation_is_stale() {
        for (pid, generation) in [(41, 3), (42, 2), (7, 9)] {
            let decision =
                watchdog_intentional_exit_decision(Some(&marker(pid, generation)), 42, 3, 1);
            assert_eq!(decision, IntentionalExitDecision::StaleMarker);
            assert!(decision.allows_restart());
        }
    }

    /// `#runfrontendcrashed`: the replacement path's own kill must keep the
    /// watchdog off the replaced pid while the cold start boots (otherwise the
    /// watchdog's second replacement kills the booting successor), but an
    /// unregistered cold start is recovered once the grace elapses.
    #[test]
    fn replacement_cold_start_marker_defers_watchdog_only_within_grace() {
        let sigterm = marker(42, 3);
        let pending =
            replacement_cold_start_marker(Some(&sigterm), Some(42), 3, "%7", "session", 10_000)
                .expect("marker for the killed pid");
        assert_eq!(pending.signal, REPLACEMENT_COLD_START_SIGNAL);
        assert_eq!(pending.supervisor_pid, 42);

        // One second later (the incident: watchdog tick at +1 s).
        let decision = watchdog_intentional_exit_decision(Some(&pending), 42, 3, 11_000);
        assert_eq!(
            decision,
            IntentionalExitDecision::ReplacementColdStartPending
        );
        assert!(!decision.allows_restart());

        // Grace elapsed with no successor lease: recover as a crash.
        let expired = watchdog_intentional_exit_decision(
            Some(&pending),
            42,
            3,
            10_000 + REPLACEMENT_COLD_START_GRACE_MS,
        );
        assert_eq!(
            expired,
            IntentionalExitDecision::ReplacementColdStartExpired
        );
        assert!(expired.allows_restart());

        // Never shields a later generation.
        assert!(watchdog_intentional_exit_decision(Some(&pending), 99, 4, 11_000).allows_restart());
    }

    #[test]
    fn replacement_cold_start_marker_falls_back_to_handler_marker_pid() {
        let sigterm = marker(42, 3);
        let from_prior =
            replacement_cold_start_marker(Some(&sigterm), None, 3, "%7", "s", 5).unwrap();
        assert_eq!(from_prior.supervisor_pid, 42);
        // A prior marker for another generation names some other process.
        assert!(replacement_cold_start_marker(Some(&sigterm), None, 4, "%7", "s", 5).is_none());
        assert!(replacement_cold_start_marker(None, None, 3, "%7", "s", 5).is_none());
    }

    #[test]
    fn marker_round_trips_and_malformed_payload_is_absent() {
        let original = marker(42, 3);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(parse_intentional_exit_marker(&json), Some(original));
        assert_eq!(parse_intentional_exit_marker("{not json"), None);
    }
}
