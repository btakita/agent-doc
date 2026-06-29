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

#[allow(clippy::too_many_arguments)]
pub fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    turn_boundary: bool,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    reexec_failed: bool,
    cycle_open: bool,
) -> SupervisorRecycleAction {
    if !turn_boundary {
        return SupervisorRecycleAction::None;
    }

    let cycle_wedged_on_stale_binary = stale && (explicit_admin || write_wedged || reexec_failed);
    if cycle_open && !cycle_wedged_on_stale_binary {
        return SupervisorRecycleAction::DeferCycleOpen;
    }

    if stale {
        if reexec_failed {
            return SupervisorRecycleAction::EscalateKillRelaunch;
        }
        if explicit_admin || write_wedged {
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
            supervisor_recycle_action(false, true, true, true, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, false, false),
            Detect
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, false, false, false),
            RecycleDebounced
        );
    }

    #[test]
    fn recycle_defers_while_cycle_open_unless_stale_cycle_is_wedged() {
        use SupervisorRecycleAction::*;

        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, true),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, true, false, false, true),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false, true),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, true, false, true),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, true, true),
            EscalateKillRelaunch
        );
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
