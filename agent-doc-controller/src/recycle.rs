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

/// Why an `execve_preserve_child` hot-reload must not run right now.
///
/// `#reexecdeadchild`: the in-place reexec hands the CURRENT harness child to
/// the replacement image, which adopts it and resumes reaping it. That handoff
/// is only sound while the child is alive and nothing has already decided to
/// replace it. Observed live on `tasks/sdk.md` (2026-10-03T02:27:41Z): an
/// operator "Clear Session Context" (`session_clear delivery=supervisor_restart_fresh`)
/// SIGTERMed the child, the host loop reaped it (`exit_code=143`), and in the
/// window before the host loop stopped the idle watch an install-fanout recycle
/// fired `supervisor_binary_stale_self_recycled ... child_pid=899775` with the
/// already-reaped PID. The new image adopted a PID it could never wait on
/// (`try_wait failed: No child processes`), synthesized exit 1, lost the fresh
/// restart the clear asked for, and the operator saw the harness "crash" twice
/// (`claude exited with code 1. Restarting in 2s...`) before a fresh relaunch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReexecPreserveChildRefusal {
    /// No child PID is published (between generations, or never spawned).
    NoLiveChild,
    /// A stop or a child-replacing restart is pending: the current host loop
    /// owns that kill + relaunch, and an exec would silently drop the request.
    ChildReplacementPending,
    /// The published child already exited (zombie) or was already reaped.
    ChildExited,
}

impl ReexecPreserveChildRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoLiveChild => "no_live_child",
            Self::ChildReplacementPending => "child_replacement_pending",
            Self::ChildExited => "child_exited",
        }
    }
}

impl std::fmt::Display for ReexecPreserveChildRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Observed facts the `execve_preserve_child` handoff depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReexecPreserveChildFacts {
    pub child_pid_published: bool,
    pub child_exited: bool,
    pub stop_requested: bool,
    pub restart_requested: bool,
    /// The pending restart is itself served by the in-place reexec
    /// (`restart_reexec`), so it is not a child replacement.
    pub restart_served_by_reexec: bool,
}

/// `#reexecdeadchild`: decide whether an in-place reexec may preserve the
/// current child. `None` means the handoff is sound.
pub fn reexec_preserve_child_refusal(
    facts: ReexecPreserveChildFacts,
) -> Option<ReexecPreserveChildRefusal> {
    if !facts.child_pid_published {
        return Some(ReexecPreserveChildRefusal::NoLiveChild);
    }
    if facts.child_exited {
        return Some(ReexecPreserveChildRefusal::ChildExited);
    }
    if facts.stop_requested || (facts.restart_requested && !facts.restart_served_by_reexec) {
        return Some(ReexecPreserveChildRefusal::ChildReplacementPending);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_child() -> ReexecPreserveChildFacts {
        ReexecPreserveChildFacts {
            child_pid_published: true,
            ..Default::default()
        }
    }

    /// `#reexecdeadchild` regression: the sdk.md clear crash. A clear's fresh
    /// restart SIGTERMed and reaped the child; the recycle must refuse to hand
    /// the dead PID across `execve`.
    #[test]
    fn reexec_refuses_to_preserve_an_exited_or_replaced_child() {
        assert_eq!(reexec_preserve_child_refusal(live_child()), None);
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts::default()),
            Some(ReexecPreserveChildRefusal::NoLiveChild)
        );
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                child_exited: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildExited)
        );
        // The operator clear: `restart mode=fresh` is pending, child not yet dead.
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                restart_requested: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildReplacementPending)
        );
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                stop_requested: true,
                ..live_child()
            }),
            Some(ReexecPreserveChildRefusal::ChildReplacementPending)
        );
        // A restart routed to the in-place reexec keeps the child by design.
        assert_eq!(
            reexec_preserve_child_refusal(ReexecPreserveChildFacts {
                restart_requested: true,
                restart_served_by_reexec: true,
                ..live_child()
            }),
            None
        );
    }

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
