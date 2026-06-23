//! Cycle-state transition table backed by lazily's thread-safe state machine.
//!
//! The durable JSON sidecar remains the crash-recovery log. This module is the
//! pure transition authority that lets the session actor and compatibility
//! sidecar path share one phase graph.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};

use crate::cycle_state::CyclePhase;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_machine_accepts_normal_closeout_order() {
        let machine = CyclePhaseMachine::new(CyclePhase::PreflightStarted);

        assert!(machine.send(CycleEvent::ResponseCaptured));
        assert_eq!(machine.state(), CyclePhase::ResponseCaptured);
        assert!(machine.send(CycleEvent::WriteApplied));
        assert_eq!(machine.state(), CyclePhase::WriteApplied);
        assert!(machine.send(CycleEvent::Committed));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn phase_machine_rejects_lower_rank_and_terminal_regressions() {
        let machine = CyclePhaseMachine::new(CyclePhase::WriteApplied);

        assert!(!machine.send(CycleEvent::ResponseCaptured));
        assert_eq!(machine.state(), CyclePhase::WriteApplied);
        assert!(machine.send(CycleEvent::Committed));
        assert!(!machine.send(CycleEvent::WriteApplied));
        assert!(!machine.send(CycleEvent::Abandoned));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn duplicate_committed_event_is_stable_self_transition() {
        let machine = CyclePhaseMachine::new(CyclePhase::Committed);

        assert!(machine.send(CycleEvent::Committed));
        assert_eq!(machine.state(), CyclePhase::Committed);
    }

    #[test]
    fn abandoned_is_terminal() {
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
    fn recoverable_timeout_rewinds_only_open_cycles() {
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
}
