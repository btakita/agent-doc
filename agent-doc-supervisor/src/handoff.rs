//! Pure controller-managed supervisor handoff state machine.
//!
//! This models a future two-process replacement path separately from the current
//! in-place `execve` hot reload. Until the controller commits lease promotion,
//! the old supervisor remains the only authoritative owner of child/pty/session
//! state.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerSupervisorHandoffState {
    /// No handoff is in progress.
    Idle,
    /// Controller accepted the request and should launch a private standby.
    LaunchingStandby,
    /// Standby process exists; controller is probing freshness/readiness.
    ProbingStandby,
    /// Standby is healthy but fenced. Wait for `prompt_visible && !turn_active`.
    AwaitTurnBoundary,
    /// Turn boundary reached; controller is compare-and-swap promoting the lease.
    PromotingLease,
    /// Lease promotion committed; the new supervisor may adopt child ownership.
    TransferringOwnership,
    /// Child ownership is transferred; stop the old supervisor generation.
    StoppingOld,
    /// New supervisor owns the lease and child; handoff is complete.
    Complete,
    /// Pre-promotion failure or abort: terminate the standby and keep old active.
    RollingBack,
    /// Failure after lease promotion. Do not silently roll back; require repair.
    BlockedPostPromotion,
}

impl ControllerSupervisorHandoffState {
    pub fn old_supervisor_authoritative(self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::LaunchingStandby
                | Self::ProbingStandby
                | Self::AwaitTurnBoundary
                | Self::PromotingLease
                | Self::RollingBack
        )
    }

    pub fn standby_may_touch_child(self) -> bool {
        matches!(
            self,
            Self::TransferringOwnership | Self::StoppingOld | Self::Complete
        )
    }

    pub fn lease_promoted(self) -> bool {
        matches!(
            self,
            Self::TransferringOwnership
                | Self::StoppingOld
                | Self::Complete
                | Self::BlockedPostPromotion
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerSupervisorHandoffEvent {
    RequestAccepted,
    StandbyStarted,
    StandbyStartFailed,
    StandbyHealthy,
    StandbyFailed,
    TurnBoundaryReached,
    PromotionCommitted,
    PromotionFailed,
    OwnershipTransferred,
    OldSupervisorStopped,
    RollbackComplete,
    AbortRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerSupervisorHandoffAction {
    None,
    LaunchStandby,
    ProbeStandby,
    WaitTurnBoundary,
    PromoteLease,
    TransferChildOwnership,
    StopOldSupervisor,
    CompleteHandoff,
    RollbackStandby,
    EscalateRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerSupervisorHandoffTransition {
    pub state: ControllerSupervisorHandoffState,
    pub action: ControllerSupervisorHandoffAction,
}

pub fn controller_supervisor_handoff_transition(
    state: ControllerSupervisorHandoffState,
    event: ControllerSupervisorHandoffEvent,
) -> ControllerSupervisorHandoffTransition {
    use ControllerSupervisorHandoffAction::*;
    use ControllerSupervisorHandoffEvent::*;
    use ControllerSupervisorHandoffState::*;

    let (state, action) = match (state, event) {
        (Idle, RequestAccepted) => (LaunchingStandby, LaunchStandby),

        (LaunchingStandby, StandbyStarted) => (ProbingStandby, ProbeStandby),
        (LaunchingStandby, StandbyStartFailed) => (RollingBack, RollbackStandby),

        (ProbingStandby, StandbyHealthy) => (AwaitTurnBoundary, WaitTurnBoundary),
        (ProbingStandby, StandbyFailed) => (RollingBack, RollbackStandby),

        (AwaitTurnBoundary, TurnBoundaryReached) => (PromotingLease, PromoteLease),
        (AwaitTurnBoundary, StandbyFailed) => (RollingBack, RollbackStandby),

        (PromotingLease, PromotionCommitted) => (TransferringOwnership, TransferChildOwnership),
        (PromotingLease, PromotionFailed | StandbyFailed) => (RollingBack, RollbackStandby),

        (TransferringOwnership, OwnershipTransferred) => (StoppingOld, StopOldSupervisor),
        (TransferringOwnership, StandbyFailed) => (BlockedPostPromotion, EscalateRepair),

        (StoppingOld, OldSupervisorStopped) => (Complete, CompleteHandoff),
        (StoppingOld, StandbyFailed) => (BlockedPostPromotion, EscalateRepair),

        (RollingBack, RollbackComplete) => (Idle, None),

        (Complete, _) => (Complete, None),
        (BlockedPostPromotion, _) => (BlockedPostPromotion, EscalateRepair),

        (Idle, AbortRequested) => (Idle, None),
        (
            LaunchingStandby | ProbingStandby | AwaitTurnBoundary | PromotingLease,
            AbortRequested,
        ) => (RollingBack, RollbackStandby),
        (TransferringOwnership | StoppingOld, AbortRequested) => {
            (BlockedPostPromotion, EscalateRepair)
        }

        (state, _) => (state, None),
    };

    ControllerSupervisorHandoffTransition { state, action }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_supervisor_handoff_happy_path_promotes_only_at_boundary() {
        use ControllerSupervisorHandoffAction::*;
        use ControllerSupervisorHandoffEvent::*;
        use ControllerSupervisorHandoffState::*;

        let mut state = Idle;
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = controller_supervisor_handoff_transition(state, RequestAccepted);
        assert_eq!(transition.action, LaunchStandby);
        state = transition.state;
        assert_eq!(state, LaunchingStandby);
        assert!(state.old_supervisor_authoritative());

        let transition = controller_supervisor_handoff_transition(state, StandbyStarted);
        assert_eq!(transition.action, ProbeStandby);
        state = transition.state;
        assert_eq!(state, ProbingStandby);
        assert!(state.old_supervisor_authoritative());

        let transition = controller_supervisor_handoff_transition(state, StandbyHealthy);
        assert_eq!(transition.action, WaitTurnBoundary);
        state = transition.state;
        assert_eq!(state, AwaitTurnBoundary);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());

        let transition = controller_supervisor_handoff_transition(state, TurnBoundaryReached);
        assert_eq!(transition.action, PromoteLease);
        state = transition.state;
        assert_eq!(state, PromotingLease);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = controller_supervisor_handoff_transition(state, PromotionCommitted);
        assert_eq!(transition.action, TransferChildOwnership);
        state = transition.state;
        assert_eq!(state, TransferringOwnership);
        assert!(!state.old_supervisor_authoritative());
        assert!(state.standby_may_touch_child());
        assert!(state.lease_promoted());

        let transition = controller_supervisor_handoff_transition(state, OwnershipTransferred);
        assert_eq!(transition.action, StopOldSupervisor);
        state = transition.state;
        assert_eq!(state, StoppingOld);

        let transition = controller_supervisor_handoff_transition(state, OldSupervisorStopped);
        assert_eq!(transition.action, CompleteHandoff);
        assert_eq!(transition.state, Complete);
        assert!(transition.state.standby_may_touch_child());
        assert!(transition.state.lease_promoted());
    }

    #[test]
    fn controller_supervisor_handoff_pre_promotion_failure_rolls_back_to_old_owner() {
        use ControllerSupervisorHandoffAction::*;
        use ControllerSupervisorHandoffEvent::*;
        use ControllerSupervisorHandoffState::*;

        let mut state = controller_supervisor_handoff_transition(Idle, RequestAccepted).state;
        state = controller_supervisor_handoff_transition(state, StandbyStarted).state;
        state = controller_supervisor_handoff_transition(state, StandbyHealthy).state;
        assert_eq!(state, AwaitTurnBoundary);

        let transition = controller_supervisor_handoff_transition(state, StandbyFailed);
        assert_eq!(transition.action, RollbackStandby);
        state = transition.state;
        assert_eq!(state, RollingBack);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = controller_supervisor_handoff_transition(state, RollbackComplete);
        assert_eq!(transition.action, None);
        assert_eq!(transition.state, Idle);
        assert!(transition.state.old_supervisor_authoritative());
    }

    #[test]
    fn controller_supervisor_handoff_post_promotion_failure_requires_repair() {
        use ControllerSupervisorHandoffAction::*;
        use ControllerSupervisorHandoffEvent::*;
        use ControllerSupervisorHandoffState::*;

        let mut state = Idle;
        for event in [
            RequestAccepted,
            StandbyStarted,
            StandbyHealthy,
            TurnBoundaryReached,
            PromotionCommitted,
        ] {
            state = controller_supervisor_handoff_transition(state, event).state;
        }
        assert_eq!(state, TransferringOwnership);
        assert!(state.lease_promoted());

        let transition = controller_supervisor_handoff_transition(state, StandbyFailed);
        assert_eq!(transition.action, EscalateRepair);
        assert_eq!(transition.state, BlockedPostPromotion);
        assert!(!transition.state.old_supervisor_authoritative());
        assert!(!transition.state.standby_may_touch_child());
        assert!(transition.state.lease_promoted());
    }
}
