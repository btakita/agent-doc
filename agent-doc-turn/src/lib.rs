//! Pure document turn lifecycle state machine.
//!
//! Realtime merge/apply verification is an input to this machine. Commits are
//! lifecycle decisions and are never performed by merge or realtime code.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

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
