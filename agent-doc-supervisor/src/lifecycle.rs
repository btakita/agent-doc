//! Pure supervisor process lifecycle decisions.
//!
//! This module owns restart/recycle/install policy. It does not spawn, kill,
//! exec, probe files, inspect panes, or mutate documents.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRecycleAction {
    None,
    Detect,
    RecycleImmediate,
    RecycleDebounced,
    EscalateKillRelaunch,
    DeferCycleOpen,
}

pub const MAX_REEXEC_ESCALATIONS: u32 = 2;

pub fn reexec_escalation_within_bound(attempts: u32, max: u32) -> bool {
    attempts < max
}

/// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`) — classify the
/// `write_wedged` evidence input consumed by [`supervisor_recycle_action`].
///
/// The editor-IPC write path is wedged when a nominally-active listener refuses
/// at least `threshold` consecutive writes without proving delivery. Failures
/// against an inactive listener are missing-listener blocks, not active-listener
/// wedges.
pub fn write_wedged_from_ipc_failures(
    consecutive_failures: u64,
    listener_nominally_active: bool,
    threshold: u64,
) -> bool {
    listener_nominally_active && consecutive_failures >= threshold
}

pub const MAX_CYCLE_OPEN_DEFER_TICKS: u32 = 40;

pub fn cycle_open_defer_escalates(consecutive_defers: u32, max: u32) -> bool {
    consecutive_defers >= max
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootResumeAction {
    None,
    AdoptSurvivingChild,
    RedispatchInterruptedTurn,
}

pub fn boot_resume_action(
    is_recycle_boot: bool,
    cycle_open: bool,
    child_survived: bool,
    already_consumed: bool,
) -> BootResumeAction {
    if !is_recycle_boot || !cycle_open {
        return BootResumeAction::None;
    }
    if child_survived {
        return BootResumeAction::AdoptSurvivingChild;
    }
    if already_consumed {
        return BootResumeAction::None;
    }
    BootResumeAction::RedispatchInterruptedTurn
}

/// A `start_session` failure during supervisor recycle is a transient controller
/// teardown race while retry budget remains.
pub fn start_session_retryable_during_recycle(
    recycle_pending: bool,
    attempts_used: usize,
    max_attempts: usize,
) -> bool {
    recycle_pending && attempts_used < max_attempts
}

/// After the trigger has already been injected, an in-flight recycle means the
/// submit may have been dropped across `execve`; wait before the single resubmit.
pub fn recycle_interrupted_resubmit_should_wait(
    trigger_already_injected: bool,
    recycle_pending: bool,
) -> bool {
    trigger_already_injected && recycle_pending
}

#[allow(clippy::too_many_arguments)]
pub fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    turn_boundary: bool,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    editor_delivery_stale: bool,
    reexec_failed: bool,
    cycle_open: bool,
) -> SupervisorRecycleAction {
    // `#midturn-wedge-recycle`: a proven editor-IPC write wedge means the active
    // turn/cycle can never reach its own boundary — closeout is blocked on a
    // convergence receipt that will not arrive, so `turn_boundary` never becomes
    // true and the `cycle_open` defer below would wait forever. That is the exact
    // deadlock that previously forced the operator to run `admin recycle` by hand
    // mid-turn. A wedge therefore escalates immediately, bypassing the boundary and
    // cycle-open gates. The idle-watch caller pre-gates `write_wedged` on the first
    // available SAFE intra-turn checkpoint (no IPC connection in flight), so the
    // `execve` cannot sever an active patch apply, and latches a `recycle_attempted`
    // flag on the dewedge marker before the execve so this is once-per-episode and
    // cannot recycle-loop. The in-flight response is capture-backed and is recovered
    // via redispatch/replay on the fresh supervisor.
    // A typed stale editor-delivery request has the same liveness shape as a
    // proven write wedge: the open closeout cycle cannot reach its boundary
    // until the replica worker is refreshed. The caller only raises this at a
    // capture-backed safe checkpoint, so deferring on `cycle_open` would be a
    // circular wait while an immediate in-place recycle preserves the response.
    if write_wedged || editor_delivery_stale {
        return if reexec_failed {
            SupervisorRecycleAction::EscalateKillRelaunch
        } else {
            SupervisorRecycleAction::RecycleImmediate
        };
    }

    if !turn_boundary {
        return SupervisorRecycleAction::None;
    }

    if cycle_open {
        return SupervisorRecycleAction::DeferCycleOpen;
    }

    if stale {
        if reexec_failed {
            return SupervisorRecycleAction::EscalateKillRelaunch;
        }
        if explicit_admin {
            return SupervisorRecycleAction::RecycleImmediate;
        }
        if !auto_recycle {
            return SupervisorRecycleAction::Detect;
        }
        return if head_pending {
            SupervisorRecycleAction::RecycleImmediate
        } else {
            SupervisorRecycleAction::RecycleDebounced
        };
    }

    if explicit_admin {
        return SupervisorRecycleAction::RecycleImmediate;
    }
    SupervisorRecycleAction::None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorInstallAction {
    None,
    Detect,
    Install,
}

pub fn supervisor_install_action(
    source_newer: bool,
    auto_install: bool,
    turn_boundary: bool,
) -> SupervisorInstallAction {
    if !source_newer || !turn_boundary {
        return SupervisorInstallAction::None;
    }
    if !auto_install {
        return SupervisorInstallAction::Detect;
    }
    SupervisorInstallAction::Install
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRestartAction {
    None,
    AwaitDrain,
    ReexecInPlace,
    RelaunchChild,
}

pub fn supervisor_restart_action(
    restart_requested: bool,
    reexec_intent: bool,
    turn_boundary: bool,
) -> SupervisorRestartAction {
    if !restart_requested {
        return SupervisorRestartAction::None;
    }
    if !turn_boundary {
        return SupervisorRestartAction::AwaitDrain;
    }
    if reexec_intent {
        SupervisorRestartAction::ReexecInPlace
    } else {
        SupervisorRestartAction::RelaunchChild
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycle_action_policy() {
        use SupervisorRecycleAction::*;

        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, false, false, false),
            Detect
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, false, false, false, false),
            RecycleDebounced
        );
    }

    #[test]
    fn recycle_defers_all_recycle_arms_while_cycle_open() {
        use SupervisorRecycleAction::*;

        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false, true),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, true, false, false, false, true),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false, false, true),
            DeferCycleOpen
        );
        // `#midturn-wedge-recycle`: a proven write wedge no longer defers on
        // cycle_open — the wedged cycle is precisely the one that never closes, so
        // deferring would deadlock. Covered by `wedge_recycles_immediately_mid_turn`.
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, true, true),
            DeferCycleOpen
        );

        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, true, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, true, false),
            EscalateKillRelaunch
        );
    }

    #[test]
    fn wedge_recycles_immediately_mid_turn() {
        use SupervisorRecycleAction::*;

        // Args: (stale, auto, turn_boundary, head_pending, admin, write_wedged,
        // editor_delivery_stale, reexec_failed, cycle_open)
        // The whole point of `#midturn-wedge-recycle`: a proven wedge recycles even
        // when NOT at a turn boundary — the wedged turn can never reach one.
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, false, true, false, false, false),
            RecycleImmediate
        );
        // ...and even mid-cycle: the open cycle is the one that is wedged.
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, false, true, false, false, true),
            RecycleImmediate
        );
        // Fires on a fresh (non-stale) binary — this session's exact case, where the
        // supervisor was `fresh` yet the editor-IPC write never converged.
        assert_eq!(
            supervisor_recycle_action(false, true, false, false, false, true, false, false, true),
            RecycleImmediate
        );
        // A wedge whose in-place execve already failed escalates to kill+relaunch
        // instead of re-execing a binary that cannot converge.
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, false, true, false, true, true),
            EscalateKillRelaunch
        );
        // No wedge, mid-turn: unchanged — nothing recycles off a boundary.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, true, false, false, false, false),
            None
        );
    }

    #[test]
    fn stale_editor_delivery_recycles_capture_backed_open_cycle() {
        use SupervisorRecycleAction::*;

        assert_eq!(
            supervisor_recycle_action(
                /* stale binary */ false, /* auto */ false,
                /* turn boundary */ false, /* head pending */ false,
                /* admin */ false, /* write wedged */ false,
                /* editor delivery stale */ true, /* reexec failed */ false,
                /* cycle open */ true,
            ),
            RecycleImmediate,
            "the stale replica refresh must not wait for the cycle it unblocks",
        );
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, false, false, true, true, true),
            EscalateKillRelaunch,
        );
    }

    #[test]
    fn write_wedged_classifier_trips_only_against_active_listener_at_threshold() {
        let threshold = 3;

        assert!(write_wedged_from_ipc_failures(threshold, true, threshold));
        assert!(write_wedged_from_ipc_failures(
            threshold + 1,
            true,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(
            threshold - 1,
            true,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(
            threshold + 5,
            false,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(0, true, threshold));
    }

    #[test]
    fn cycle_open_defer_escalates_after_threshold_only() {
        assert!(!cycle_open_defer_escalates(0, MAX_CYCLE_OPEN_DEFER_TICKS));
        assert!(!cycle_open_defer_escalates(
            MAX_CYCLE_OPEN_DEFER_TICKS - 1,
            MAX_CYCLE_OPEN_DEFER_TICKS
        ));
        assert!(cycle_open_defer_escalates(
            MAX_CYCLE_OPEN_DEFER_TICKS,
            MAX_CYCLE_OPEN_DEFER_TICKS
        ));
        assert!(!cycle_open_defer_escalates(2, 3));
        assert!(cycle_open_defer_escalates(3, 3));
    }

    #[test]
    fn boot_resume_redispatches_only_when_cycle_open_and_child_died() {
        use BootResumeAction::*;

        assert_eq!(boot_resume_action(false, true, false, false), None);
        assert_eq!(boot_resume_action(true, false, false, false), None);
        assert_eq!(
            boot_resume_action(true, true, true, false),
            AdoptSurvivingChild
        );
        assert_eq!(
            boot_resume_action(true, true, false, false),
            RedispatchInterruptedTurn
        );
        assert_eq!(boot_resume_action(true, true, false, true), None);
    }

    #[test]
    fn start_session_retry_only_while_recycling_and_budget_remains() {
        assert!(start_session_retryable_during_recycle(true, 0, 2));
        assert!(start_session_retryable_during_recycle(true, 1, 2));
        assert!(!start_session_retryable_during_recycle(true, 2, 2));
        assert!(!start_session_retryable_during_recycle(false, 0, 2));
    }

    #[test]
    fn recycle_interrupted_resubmit_waits_only_when_injected_and_recycling() {
        assert!(recycle_interrupted_resubmit_should_wait(true, true));
        assert!(!recycle_interrupted_resubmit_should_wait(false, true));
        assert!(!recycle_interrupted_resubmit_should_wait(true, false));
    }

    #[test]
    fn reexec_escalation_is_bounded() {
        assert!(reexec_escalation_within_bound(0, MAX_REEXEC_ESCALATIONS));
        assert!(reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS - 1,
            MAX_REEXEC_ESCALATIONS
        ));
        assert!(!reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS,
            MAX_REEXEC_ESCALATIONS
        ));
    }

    #[test]
    fn install_action_policy() {
        use SupervisorInstallAction::*;

        assert_eq!(supervisor_install_action(false, true, true), None);
        assert_eq!(supervisor_install_action(true, true, false), None);
        assert_eq!(supervisor_install_action(true, false, true), Detect);
        assert_eq!(supervisor_install_action(true, true, true), Install);
    }

    #[test]
    fn restart_action_drain_and_supersede_policy() {
        use SupervisorRestartAction::*;

        assert_eq!(supervisor_restart_action(false, true, true), None);
        assert_eq!(supervisor_restart_action(true, true, false), AwaitDrain);
        assert_eq!(supervisor_restart_action(true, true, true), ReexecInPlace);
        assert_eq!(supervisor_restart_action(true, false, true), RelaunchChild);
    }
}
