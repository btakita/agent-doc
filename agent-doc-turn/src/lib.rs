//! Pure document turn lifecycle state machine.
//!
//! Realtime merge/apply verification is an input to this machine. Commits are
//! lifecycle decisions and are never performed by merge or realtime code.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod authority_recovery;
pub mod closeout_guard;
pub mod closeout_recovery;
pub mod closeout_signal;
pub mod codex_stop_continuation;
pub mod coined_ids;
pub mod cp_projection;
pub mod cycle_ack;
pub mod cycle_policy;
pub mod document_drift;
pub mod drain_stall;
pub mod exchange_tail;
pub mod heuristics;
pub mod no_change;
pub mod op_log;
pub mod owner_pane_recursion;
pub mod repair;
pub mod response_replay;
pub mod response_text;
pub mod turn_scope;
pub mod turn_status;
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

impl CyclePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreflightStarted => "preflight_started",
            Self::ResponseCaptured => "response_captured",
            Self::WriteApplied => "write_applied",
            Self::Committed => "committed",
            Self::Abandoned => "abandoned",
        }
    }

    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Committed | Self::Abandoned)
    }

    const fn progress_rank(self) -> Option<u8> {
        match self {
            Self::PreflightStarted => Some(0),
            Self::ResponseCaptured => Some(1),
            Self::WriteApplied => Some(2),
            Self::Committed | Self::Abandoned => None,
        }
    }
}

/// Monotone resolution for two durable views of the same closeout cycle.
///
/// The state backbone and the local cycle projection are replicas of one
/// lifecycle, not competing authorities. A lagging replica may fill missing
/// metadata, but it must never move the lifecycle backward. Conflicting
/// terminal facts remain explicit instead of being resolved by last-read wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutPhaseResolution {
    KeepCurrent,
    AdoptProjected,
    ConflictingTerminalFacts,
}

pub const fn resolve_closeout_phase(
    current: CyclePhase,
    projected: CyclePhase,
) -> CloseoutPhaseResolution {
    use CloseoutPhaseResolution::{AdoptProjected, ConflictingTerminalFacts, KeepCurrent};

    if current as u8 == projected as u8 {
        return KeepCurrent;
    }
    match (current, projected) {
        (
            CyclePhase::Committed | CyclePhase::Abandoned,
            CyclePhase::Committed | CyclePhase::Abandoned,
        ) => ConflictingTerminalFacts,
        (CyclePhase::Committed | CyclePhase::Abandoned, _) => KeepCurrent,
        (_, CyclePhase::Committed | CyclePhase::Abandoned) => AdoptProjected,
        _ => match (current.progress_rank(), projected.progress_rank()) {
            (Some(current), Some(projected)) if projected > current => AdoptProjected,
            _ => KeepCurrent,
        },
    }
}

/// Maximum age for treating an open cycle as actively in flight for reactive
/// watch suppression.
pub const WATCH_CYCLE_IN_FLIGHT_MAX_SECS: u64 = 600;

pub fn cycle_phase_freshly_in_flight(
    phase: CyclePhase,
    updated_at_secs: u64,
    now_secs: u64,
) -> bool {
    phase.is_open() && now_secs.saturating_sub(updated_at_secs) < WATCH_CYCLE_IN_FLIGHT_MAX_SECS
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
    DocumentCellMergeAcks,
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
    fn cycle_phase_labels_are_owned_by_turn_lifecycle() {
        assert_eq!(CyclePhase::PreflightStarted.as_str(), "preflight_started");
        assert_eq!(CyclePhase::ResponseCaptured.as_str(), "response_captured");
        assert_eq!(CyclePhase::WriteApplied.as_str(), "write_applied");
        assert_eq!(CyclePhase::Committed.as_str(), "committed");
        assert_eq!(CyclePhase::Abandoned.as_str(), "abandoned");
    }

    #[test]
    fn cycle_phase_open_status_is_owned_by_turn_lifecycle() {
        assert!(CyclePhase::PreflightStarted.is_open());
        assert!(CyclePhase::ResponseCaptured.is_open());
        assert!(CyclePhase::WriteApplied.is_open());
        assert!(!CyclePhase::Committed.is_open());
        assert!(!CyclePhase::Abandoned.is_open());
    }

    #[test]
    fn cycle_phase_freshly_in_flight_requires_open_fresh_cycle() {
        let now = 1_000;

        assert!(cycle_phase_freshly_in_flight(
            CyclePhase::PreflightStarted,
            now,
            now
        ));
        assert!(cycle_phase_freshly_in_flight(
            CyclePhase::ResponseCaptured,
            now - 60,
            now
        ));
        assert!(cycle_phase_freshly_in_flight(
            CyclePhase::WriteApplied,
            now - WATCH_CYCLE_IN_FLIGHT_MAX_SECS + 1,
            now
        ));

        assert!(!cycle_phase_freshly_in_flight(
            CyclePhase::PreflightStarted,
            now - WATCH_CYCLE_IN_FLIGHT_MAX_SECS,
            now
        ));
        assert!(!cycle_phase_freshly_in_flight(
            CyclePhase::Committed,
            now,
            now
        ));
        assert!(!cycle_phase_freshly_in_flight(
            CyclePhase::Abandoned,
            now,
            now
        ));
    }

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
    fn closeout_phase_resolution_is_monotone_across_replica_lag() {
        use super::CloseoutPhaseResolution::{
            AdoptProjected, ConflictingTerminalFacts, KeepCurrent,
        };

        assert_eq!(
            resolve_closeout_phase(CyclePhase::ResponseCaptured, CyclePhase::WriteApplied),
            AdoptProjected
        );
        assert_eq!(
            resolve_closeout_phase(CyclePhase::WriteApplied, CyclePhase::ResponseCaptured),
            KeepCurrent
        );
        assert_eq!(
            resolve_closeout_phase(CyclePhase::Committed, CyclePhase::WriteApplied),
            KeepCurrent
        );
        assert_eq!(
            resolve_closeout_phase(CyclePhase::Committed, CyclePhase::Abandoned),
            ConflictingTerminalFacts
        );
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
