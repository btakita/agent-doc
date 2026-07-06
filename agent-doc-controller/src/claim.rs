//! Pure claim admission policy for pane/session ownership.

/// Outcome of the cross-session claim gate.
#[derive(Debug, PartialEq, Eq)]
pub enum CrossSessionDecision {
    /// Pane's tmux session matches the configured project session.
    Accept,
    /// Configured project session no longer exists on the tmux server.
    AcceptStale,
    /// Cross-session claim explicitly allowed via `--force`.
    AcceptForce,
    /// Cross-session claim rejected.
    Reject,
}

/// Stable marker prefix emitted before the human-readable cross-session claim
/// rejection.
pub const CROSS_SESSION_REJECT_MARKER: &str = "[claim] cross-session-reject";

/// Format the machine-readable cross-session reject marker line.
pub fn cross_session_reject_marker(pane_id: &str, pane_session: &str, configured: &str) -> String {
    format!(
        "{CROSS_SESSION_REJECT_MARKER} pane_id={pane_id} pane_session={pane_session} configured={configured}"
    )
}

pub fn cross_session_decision(
    pane_session: &str,
    configured: &str,
    configured_alive: bool,
    force: bool,
) -> CrossSessionDecision {
    cross_session_decision_with_lease(pane_session, configured, configured_alive, force, false)
}

/// Cross-session claim gate that also consults the document supervisor lease.
///
/// `fresh_foreign_lease` is `true` when a fresh lease is held by a live foreign
/// supervisor for this document. When set and the gate would otherwise
/// auto-force a stale-session reclaim, the decision becomes `Reject` unless the
/// operator passes an explicit `force` override.
pub fn cross_session_decision_with_lease(
    pane_session: &str,
    configured: &str,
    configured_alive: bool,
    force: bool,
    fresh_foreign_lease: bool,
) -> CrossSessionDecision {
    if pane_session == configured {
        return CrossSessionDecision::Accept;
    }
    if !configured_alive {
        if fresh_foreign_lease {
            if force {
                return CrossSessionDecision::AcceptForce;
            }
            return CrossSessionDecision::Reject;
        }
        return CrossSessionDecision::AcceptStale;
    }
    if force {
        return CrossSessionDecision::AcceptForce;
    }
    CrossSessionDecision::Reject
}

/// Cross-session claim gate that treats the operator's current tmux session as
/// authoritative for manual pane claiming. This lets `Claim for Tmux Pane`
/// follow the live `agent-doc` window without rewriting the configured project
/// `tmux_session`.
pub fn cross_session_decision_with_current(
    pane_session: &str,
    configured: &str,
    configured_alive: bool,
    current_session: Option<&str>,
    force: bool,
    fresh_foreign_lease: bool,
) -> CrossSessionDecision {
    if current_session
        .map(str::trim)
        .is_some_and(|current| !current.is_empty() && current == pane_session)
    {
        return CrossSessionDecision::Accept;
    }
    cross_session_decision_with_lease(
        pane_session,
        configured,
        configured_alive,
        force,
        fresh_foreign_lease,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_session_accept_when_pane_matches_configured() {
        let d = cross_session_decision("0", "0", true, false);
        assert_eq!(d, CrossSessionDecision::Accept);
    }

    #[test]
    fn cross_session_accept_stale_when_configured_dead() {
        let d = cross_session_decision("claude", "0", false, false);
        assert_eq!(d, CrossSessionDecision::AcceptStale);
    }

    #[test]
    fn cross_session_accept_stale_takes_precedence_over_force() {
        let d = cross_session_decision("claude", "0", false, true);
        assert_eq!(d, CrossSessionDecision::AcceptStale);
    }

    #[test]
    fn cross_session_reject_when_configured_alive_and_no_force() {
        let d = cross_session_decision("claude", "0", true, false);
        assert_eq!(d, CrossSessionDecision::Reject);
    }

    #[test]
    fn cross_session_accept_force_when_configured_alive_and_force() {
        let d = cross_session_decision("claude", "0", true, true);
        assert_eq!(d, CrossSessionDecision::AcceptForce);
    }

    #[test]
    fn cross_session_stale_without_foreign_lease_still_accept_stale() {
        let d = cross_session_decision_with_lease("claude", "0", false, false, false);
        assert_eq!(d, CrossSessionDecision::AcceptStale);
    }

    #[test]
    fn cross_session_stale_with_foreign_lease_rejects() {
        let d = cross_session_decision_with_lease("claude", "0", false, false, true);
        assert_eq!(d, CrossSessionDecision::Reject);
    }

    #[test]
    fn cross_session_stale_with_foreign_lease_and_force_accepts() {
        let d = cross_session_decision_with_lease("claude", "0", false, true, true);
        assert_eq!(d, CrossSessionDecision::AcceptForce);
    }

    #[test]
    fn cross_session_matching_session_ignores_foreign_lease() {
        let d = cross_session_decision_with_lease("0", "0", true, false, true);
        assert_eq!(d, CrossSessionDecision::Accept);
    }

    #[test]
    fn cross_session_live_configured_unaffected_by_lease_flag() {
        assert_eq!(
            cross_session_decision_with_lease("claude", "0", true, false, true),
            CrossSessionDecision::Reject
        );
        assert_eq!(
            cross_session_decision_with_lease("claude", "0", true, true, true),
            CrossSessionDecision::AcceptForce
        );
    }

    #[test]
    fn cross_session_decision_defaults_to_no_lease_guard() {
        assert_eq!(
            cross_session_decision("claude", "0", false, false),
            CrossSessionDecision::AcceptStale
        );
    }

    #[test]
    fn cross_session_accepts_current_session_over_live_configured_pin() {
        let d = cross_session_decision_with_current("2", "0", true, Some("2"), false, false);
        assert_eq!(d, CrossSessionDecision::Accept);
    }

    #[test]
    fn cross_session_blank_current_session_does_not_bypass_configured_pin() {
        let d = cross_session_decision_with_current("2", "0", true, Some("   "), false, false);
        assert_eq!(d, CrossSessionDecision::Reject);
    }

    #[test]
    fn cross_session_reject_marker_carries_stable_fields() {
        let line = cross_session_reject_marker("%43", "5", "0");
        assert_eq!(
            line,
            "[claim] cross-session-reject pane_id=%43 pane_session=5 configured=0"
        );
        assert!(line.starts_with(CROSS_SESSION_REJECT_MARKER));
    }
}
