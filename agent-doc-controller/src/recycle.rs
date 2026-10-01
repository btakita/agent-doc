//! Pure controller recycle policy.

use std::time::{Duration, Instant};

pub fn recycle_debounce_decision(
    wants_recycle_and_idle: bool,
    stale_since: Option<Instant>,
    now: Instant,
    grace: Duration,
) -> (bool, Option<Instant>) {
    match (wants_recycle_and_idle, stale_since) {
        (false, _) => (false, None),
        (true, None) => (false, Some(now)),
        (true, Some(since)) => (now.duration_since(since) >= grace, Some(since)),
    }
}

/// A project-controller image may begin a two-phase recycle while RPCs and a
/// durable harness dispatch remain open.
///
/// The harness child is owned by the route-owned supervisor, not by the project
/// controller. Dispatch/cycle state is durable in `state.db`, so treating the
/// whole harness turn as controller activity only keeps stale controller code
/// alive and delays retained-write recovery. The only launch exclusion is a
/// non-stable handoff. Promotion redirects new RPCs to the replacement, and the
/// predecessor exits only after its already-accepted RPCs drain.
pub fn controller_recycle_safe_to_handoff(handoff_stable: bool) -> bool {
    handoff_stable
}

/// Reason an install fan-out attaches to its `recycle` request.
pub const INSTALL_FANOUT_RECYCLE_REASON: &str = "install_fanout";

/// An install fan-out `recycle` is redundant when the controller is provably
/// already executing the installed binary — typically because it self-detected
/// the stale binary and restarted onto the new build between `binary-install`
/// and the fan-out. Launching a second handoff then buys nothing and holds every
/// RPC in `Preparing` for the whole successor wait (GH: fpe Stop hook overran its
/// 45s budget behind exactly that handoff). Unknown identities are not proof, so
/// they still recycle.
pub fn install_fanout_recycle_is_redundant(
    reason: Option<&str>,
    recorded: Option<&crate::status::ControllerBinaryIdentity>,
    current: Option<&crate::status::ControllerBinaryIdentity>,
) -> bool {
    reason == Some(INSTALL_FANOUT_RECYCLE_REASON)
        && crate::status::controller_binary_identity_matches(recorded, current)
}

/// Explicit force and protocol-skew recovery skip the normal recycle debounce.
/// Neither case may interrupt an RPC; promotion and predecessor drain own that
/// proof.
pub fn controller_recycle_is_urgent(recycle_forced: bool, protocol_skew_urgent: bool) -> bool {
    recycle_forced || protocol_skew_urgent
}

/// `#recycleidleonly`: a ROUTINE stale-binary recycle must wait for a real turn
/// boundary.
///
/// `execve` is supposed to preserve the live child and its tmux pane, but its
/// documented fallback is a clean exit + child restart, and that fallback tears
/// the pane down: observed live, pane `%3` vanished mid-turn and came back with
/// `history_size=5` against a 50000-line limit, so the operator lost the entire
/// visible session (`boundary=safe_intra_turn via=execve_preserve_child`).
/// `safe_intra_turn` is a truthful claim about DOCUMENT safety, not pane safety.
///
/// A pending queue head is NOT a licence to recycle mid-turn: `head_pending`
/// only bypasses the idle-grace *debounce* (an inter-queue-item recycle should
/// not wait out the grace window), and a genuine inter-queue-item boundary is
/// already a `turn_boundary`. Gating solely on `turn_boundary` is what closes
/// the gap with the `#wd40` / `#staleloop-recycle-restart` yield protocol.
///
/// Non-routine recycles (wedged supervisor, explicit admin, stale editor
/// delivery — i.e. `RecycleImmediate`) are never deferred here: the alternative
/// to recycling them mid-turn is staying wedged forever.
pub fn routine_stale_recycle_deferred_intra_turn(
    routine_stale_recycle: bool,
    turn_boundary: bool,
) -> bool {
    routine_stale_recycle && !turn_boundary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(modified_secs: u64) -> crate::status::ControllerBinaryIdentity {
        crate::status::ControllerBinaryIdentity {
            path: "/bin/agent-doc".into(),
            version: "0.35.428".into(),
            len: 10,
            modified_secs,
            modified_nanos: 0,
        }
    }

    /// A controller that already restarted onto the installed build declines the
    /// install fan-out's recycle instead of launching a redundant handoff.
    #[test]
    fn install_fanout_recycle_is_redundant_only_for_a_proven_current_binary() {
        let current = identity(2);
        let reason = Some(INSTALL_FANOUT_RECYCLE_REASON);
        assert!(install_fanout_recycle_is_redundant(
            reason,
            Some(&current),
            Some(&current)
        ));
        // Same version, different build: still a real recycle.
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            Some(&identity(1)),
            Some(&current)
        ));
        // Unknown identity is not proof.
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            None,
            Some(&current)
        ));
        assert!(!install_fanout_recycle_is_redundant(
            reason,
            Some(&current),
            None
        ));
        // An explicit operator recycle always recycles.
        assert!(!install_fanout_recycle_is_redundant(
            None,
            Some(&current),
            Some(&current)
        ));
        assert!(!install_fanout_recycle_is_redundant(
            Some("operator_request"),
            Some(&current),
            Some(&current)
        ));
    }

    #[test]
    fn debounce_requires_continuous_idle_grace() {
        let grace = Duration::from_secs(5);
        let t0 = Instant::now();

        assert_eq!(
            recycle_debounce_decision(false, Some(t0), t0, grace),
            (false, None)
        );

        let (do_recycle, since) = recycle_debounce_decision(true, None, t0, grace);
        assert!(!do_recycle);
        assert_eq!(since, Some(t0));

        let t_mid = t0 + Duration::from_secs(2);
        assert_eq!(
            recycle_debounce_decision(true, since, t_mid, grace),
            (false, Some(t0))
        );

        let t_late = t0 + Duration::from_secs(6);
        assert_eq!(
            recycle_debounce_decision(true, since, t_late, grace),
            (true, Some(t0))
        );
        assert_eq!(
            recycle_debounce_decision(false, since, t_late, grace),
            (false, None)
        );
    }

    #[test]
    fn routine_stale_recycle_waits_for_a_turn_boundary() {
        // At a turn boundary the routine recycle proceeds.
        assert!(!routine_stale_recycle_deferred_intra_turn(true, true));
        // Mid-turn it defers — this is the pane-destroying `boundary=safe_intra_turn`
        // case the operator hit (#eqmv / #recycleidleonly).
        assert!(routine_stale_recycle_deferred_intra_turn(true, false));
        // Non-routine (RecycleImmediate: wedge / admin / stale editor delivery)
        // is never deferred, boundary or not.
        assert!(!routine_stale_recycle_deferred_intra_turn(false, false));
        assert!(!routine_stale_recycle_deferred_intra_turn(false, true));
    }

    #[test]
    fn controller_recycle_is_safe_midturn_only_between_rpcs_and_outside_handoff() {
        // Active RPCs drain on the predecessor after promotion. A durable harness
        // dispatch may likewise remain open: it is supervisor-owned and survives.
        assert!(controller_recycle_safe_to_handoff(true));
        assert!(!controller_recycle_safe_to_handoff(false));
    }

    #[test]
    fn forced_or_protocol_skew_recycle_skips_the_debounce() {
        assert!(controller_recycle_is_urgent(true, false));
        assert!(controller_recycle_is_urgent(false, true));
        assert!(controller_recycle_is_urgent(true, true));
        assert!(!controller_recycle_is_urgent(false, false));
    }
}
