//! Pure document turn lifecycle state machine.
//!
//! Realtime merge/apply verification is an input to this machine. Commits are
//! lifecycle decisions and are never performed by merge or realtime code.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod drain_stall;
pub mod heuristics;
pub mod op_log;
pub mod turn_scope;
pub mod wait_machine;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePhase {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleEvent {
    StartPreflight,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
    RecoverablePreflightTimeout,
    Bookkeeping(CycleBookkeepingEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleBookkeepingEvent {
    ActiveQueueHeads,
    TurnCheckpoint,
    PendingMutations,
    PendingDoneIds,
    PendingKeptOpenIds,
    ReapedPendingIds,
    ExpectDoneOrGateIds,
    PendingGatedIds,
    PendingAddedIds,
    BacklogCaptureRequirement,
    BacklogTargetRequirements,
    RequiredExplicitBacklogItemCount,
    RequiredPlanReferenceCount,
    OpenCycleProgress,
    IpcSnapshotAdoptionBlocked,
    DroppedExchangePrompts,
    DroppedQueuePrompts,
    SemanticMergeAcks,
}

pub struct CyclePhaseMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<CyclePhase, CycleEvent>,
}

impl CyclePhaseMachine {
    pub fn new(initial: CyclePhase) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_phase);
        Self { ctx, machine }
    }

    pub fn send(&self, event: CycleEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> CyclePhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: CyclePhase, event: CycleEvent) -> Option<CyclePhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_phase(current: &CyclePhase, event: &CycleEvent) -> Option<CyclePhase> {
    match event {
        CycleEvent::StartPreflight => Some(CyclePhase::PreflightStarted),
        CycleEvent::ResponseCaptured => match current {
            CyclePhase::PreflightStarted | CyclePhase::ResponseCaptured => {
                Some(CyclePhase::ResponseCaptured)
            }
            CyclePhase::WriteApplied | CyclePhase::Committed | CyclePhase::Abandoned => None,
        },
        CycleEvent::WriteApplied => match current {
            CyclePhase::PreflightStarted
            | CyclePhase::ResponseCaptured
            | CyclePhase::WriteApplied => Some(CyclePhase::WriteApplied),
            CyclePhase::Committed | CyclePhase::Abandoned => None,
        },
        CycleEvent::Committed => match current {
            CyclePhase::PreflightStarted
            | CyclePhase::ResponseCaptured
            | CyclePhase::WriteApplied
            | CyclePhase::Committed => Some(CyclePhase::Committed),
            CyclePhase::Abandoned => None,
        },
        CycleEvent::Abandoned => match current {
            CyclePhase::PreflightStarted
            | CyclePhase::ResponseCaptured
            | CyclePhase::WriteApplied => Some(CyclePhase::Abandoned),
            CyclePhase::Committed | CyclePhase::Abandoned => None,
        },
        CycleEvent::RecoverablePreflightTimeout => match current {
            CyclePhase::PreflightStarted
            | CyclePhase::ResponseCaptured
            | CyclePhase::WriteApplied => Some(CyclePhase::PreflightStarted),
            CyclePhase::Committed | CyclePhase::Abandoned => None,
        },
        CycleEvent::Bookkeeping(_) => match current {
            CyclePhase::PreflightStarted
            | CyclePhase::ResponseCaptured
            | CyclePhase::WriteApplied => Some(*current),
            CyclePhase::Committed | CyclePhase::Abandoned => None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Idle,
    Admitted,
    PreflightOpened,
    PromptDispatched,
    AgentRunning,
    ResponseCaptured,
    RealtimeApplyPending,
    RealtimeApplyVerified,
    CommitPending,
    Committed,
    NoCommitComplete,
    InterruptedBlocked,
    Abandoned,
}

impl TurnState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TurnState::Committed | TurnState::NoCommitComplete | TurnState::Abandoned
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEvent {
    AdmitPrompt,
    OpenPreflight,
    NoWork,
    ParseBlocked,
    DispatchAccepted,
    ProbeCancelled,
    BackendAccepted,
    DispatchFailed,
    FinalResponseCaptured,
    CancelBeforeCapture,
    HarnessInterrupted,
    OperationSetBuilt,
    RealtimeVerified,
    RealtimeBlocked,
    CommitPolicySelected,
    NoCommitPolicySelected,
    CommitComplete,
    CommitBlocked,
    RetryCloseout,
    SafeAbandon,
    NewPrompt,
}

pub fn transition_turn(current: &TurnState, event: &TurnEvent) -> Option<TurnState> {
    use TurnEvent::*;
    use TurnState::*;

    match (*current, *event) {
        (Idle, AdmitPrompt) => Some(Admitted),
        (Admitted, OpenPreflight) => Some(PreflightOpened),
        (Admitted | PreflightOpened, NoWork | ProbeCancelled) => Some(Abandoned),
        (PreflightOpened, ParseBlocked) => Some(InterruptedBlocked),
        (PreflightOpened, DispatchAccepted) => Some(PromptDispatched),
        (PromptDispatched, BackendAccepted) => Some(AgentRunning),
        (PromptDispatched, DispatchFailed) => Some(InterruptedBlocked),
        (AgentRunning, FinalResponseCaptured) => Some(ResponseCaptured),
        (AgentRunning, CancelBeforeCapture) => Some(Abandoned),
        (AgentRunning, HarnessInterrupted) => Some(InterruptedBlocked),
        (ResponseCaptured, OperationSetBuilt) => Some(RealtimeApplyPending),
        (RealtimeApplyPending, RealtimeVerified) => Some(RealtimeApplyVerified),
        (RealtimeApplyPending, RealtimeBlocked) => Some(InterruptedBlocked),
        (RealtimeApplyVerified, CommitPolicySelected) => Some(CommitPending),
        (RealtimeApplyVerified, NoCommitPolicySelected) => Some(NoCommitComplete),
        (CommitPending, CommitComplete) => Some(Committed),
        (CommitPending, CommitBlocked) => Some(InterruptedBlocked),
        (InterruptedBlocked, RetryCloseout) => Some(ResponseCaptured),
        (InterruptedBlocked, SafeAbandon) => Some(Abandoned),
        (Committed | NoCommitComplete | Abandoned, NewPrompt) => Some(Idle),
        _ => None,
    }
}

pub struct TurnLifecycleMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<TurnState, TurnEvent>,
}

impl TurnLifecycleMachine {
    pub fn new(initial: TurnState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_turn);
        Self { ctx, machine }
    }

    pub fn send(&self, event: TurnEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> TurnState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: TurnState, event: TurnEvent) -> Option<TurnState> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_phase_machine_accepts_normal_closeout_order() {
        let machine = CyclePhaseMachine::new(CyclePhase::PreflightStarted);

        assert!(machine.send(CycleEvent::ResponseCaptured));
        assert_eq!(machine.state(), CyclePhase::ResponseCaptured);
        assert!(machine.send(CycleEvent::WriteApplied));
        assert_eq!(machine.state(), CyclePhase::WriteApplied);
        assert!(machine.send(CycleEvent::Committed));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn cycle_phase_machine_rejects_lower_rank_and_terminal_regressions() {
        let machine = CyclePhaseMachine::new(CyclePhase::WriteApplied);

        assert!(!machine.send(CycleEvent::ResponseCaptured));
        assert_eq!(machine.state(), CyclePhase::WriteApplied);
        assert!(machine.send(CycleEvent::Committed));
        assert!(!machine.send(CycleEvent::WriteApplied));
        assert!(!machine.send(CycleEvent::Abandoned));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn cycle_duplicate_committed_event_is_stable_self_transition() {
        let machine = CyclePhaseMachine::new(CyclePhase::Committed);

        assert!(machine.send(CycleEvent::Committed));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn cycle_abandoned_is_terminal() {
        let machine = CyclePhaseMachine::new(CyclePhase::ResponseCaptured);

        assert!(machine.send(CycleEvent::Abandoned));
        assert_eq!(machine.state(), CyclePhase::Abandoned);
        assert!(!machine.send(CycleEvent::Committed));
        assert!(!machine.send(CycleEvent::Bookkeeping(
            CycleBookkeepingEvent::PendingDoneIds,
        )));
        assert_eq!(machine.state(), CyclePhase::Abandoned);
    }

    #[test]
    fn cycle_recoverable_timeout_rewinds_only_open_cycles() {
        assert_eq!(
            CyclePhaseMachine::transition(
                CyclePhase::WriteApplied,
                CycleEvent::RecoverablePreflightTimeout,
            ),
            Some(CyclePhase::PreflightStarted)
        );
        assert_eq!(
            CyclePhaseMachine::transition(
                CyclePhase::Committed,
                CycleEvent::RecoverablePreflightTimeout,
            ),
            None
        );
    }

    #[test]
    fn normal_commit_path_crosses_verified_handoff() {
        let machine = TurnLifecycleMachine::new(TurnState::Idle);
        for event in [
            TurnEvent::AdmitPrompt,
            TurnEvent::OpenPreflight,
            TurnEvent::DispatchAccepted,
            TurnEvent::BackendAccepted,
            TurnEvent::FinalResponseCaptured,
            TurnEvent::OperationSetBuilt,
            TurnEvent::RealtimeVerified,
            TurnEvent::CommitPolicySelected,
            TurnEvent::CommitComplete,
        ] {
            assert!(machine.send(event), "{event:?}");
        }
        assert_eq!(machine.state(), TurnState::Committed);
    }

    #[test]
    fn commit_cannot_skip_realtime_verification() {
        assert_eq!(
            TurnLifecycleMachine::transition(
                TurnState::ResponseCaptured,
                TurnEvent::CommitPolicySelected
            ),
            None
        );
        assert_eq!(
            TurnLifecycleMachine::transition(
                TurnState::RealtimeApplyPending,
                TurnEvent::CommitPolicySelected
            ),
            None
        );
    }

    #[test]
    fn no_commit_is_explicit_after_verified_handoff() {
        assert_eq!(
            TurnLifecycleMachine::transition(
                TurnState::RealtimeApplyVerified,
                TurnEvent::NoCommitPolicySelected
            ),
            Some(TurnState::NoCommitComplete)
        );
    }

    #[test]
    fn parse_blocked_interrupts_before_dispatch() {
        assert_eq!(
            TurnLifecycleMachine::transition(TurnState::PreflightOpened, TurnEvent::ParseBlocked),
            Some(TurnState::InterruptedBlocked)
        );
    }
}
