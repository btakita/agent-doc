//! Monotonic per-intent document write pipeline.
//!
//! One machine instance describes one immutable write intent (identified by the
//! capture/delivery id in the durable state store). Transport loss is therefore
//! a retry condition, never a reason to recapture the response or replace the
//! editor authority. Every durable observer can dead-reckon the only legal next
//! proof from the current phase.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentWritePhase {
    IntentCaptured,
    CanonicalApplied,
    ReplicaAccepted,
    ReplicaVisible,
    DiskProjected,
    Committed,
}

impl DocumentWritePhase {
    pub const fn rank(self) -> u8 {
        self as u8
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Committed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentWriteEvent {
    IntentCaptured,
    CanonicalApplied,
    ReplicaAccepted,
    ReplicaVisible,
    DiskProjected,
    Committed,
    RetryRequested,
    EndpointLost,
    EndpointRestored,
}

impl DocumentWriteEvent {
    const fn target_phase(self) -> Option<DocumentWritePhase> {
        match self {
            Self::IntentCaptured => Some(DocumentWritePhase::IntentCaptured),
            Self::CanonicalApplied => Some(DocumentWritePhase::CanonicalApplied),
            Self::ReplicaAccepted => Some(DocumentWritePhase::ReplicaAccepted),
            Self::ReplicaVisible => Some(DocumentWritePhase::ReplicaVisible),
            Self::DiskProjected => Some(DocumentWritePhase::DiskProjected),
            Self::Committed => Some(DocumentWritePhase::Committed),
            Self::RetryRequested | Self::EndpointLost | Self::EndpointRestored => None,
        }
    }
}

pub struct DocumentWritePipeline {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<DocumentWritePhase, DocumentWriteEvent>,
}

impl DocumentWritePipeline {
    pub fn new(initial: DocumentWritePhase) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_document_write);
        Self { ctx, machine }
    }

    pub fn send(&self, event: DocumentWriteEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> DocumentWritePhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: DocumentWritePhase,
        event: DocumentWriteEvent,
    ) -> Option<DocumentWritePhase> {
        let machine = Self::new(initial);
        machine.send(event).then(|| machine.state())
    }
}

/// Transition one immutable write intent without ever moving its proof frontier
/// backward. Replayed facts at or behind the frontier are idempotent; a fact
/// that skips a required proof is rejected. Endpoint churn and retries are
/// self-loops because they do not change document truth.
pub fn transition_document_write(
    current: &DocumentWritePhase,
    event: &DocumentWriteEvent,
) -> Option<DocumentWritePhase> {
    let Some(target) = event.target_phase() else {
        return Some(*current);
    };
    if target.rank() <= current.rank() {
        return Some(*current);
    }
    (target.rank() == current.rank() + 1).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASES: [DocumentWritePhase; 6] = [
        DocumentWritePhase::IntentCaptured,
        DocumentWritePhase::CanonicalApplied,
        DocumentWritePhase::ReplicaAccepted,
        DocumentWritePhase::ReplicaVisible,
        DocumentWritePhase::DiskProjected,
        DocumentWritePhase::Committed,
    ];
    const EVENTS: [DocumentWriteEvent; 9] = [
        DocumentWriteEvent::IntentCaptured,
        DocumentWriteEvent::CanonicalApplied,
        DocumentWriteEvent::ReplicaAccepted,
        DocumentWriteEvent::ReplicaVisible,
        DocumentWriteEvent::DiskProjected,
        DocumentWriteEvent::Committed,
        DocumentWriteEvent::RetryRequested,
        DocumentWriteEvent::EndpointLost,
        DocumentWriteEvent::EndpointRestored,
    ];

    #[test]
    fn complete_transition_relation_is_monotonic_and_cannot_skip_proofs() {
        for phase in PHASES {
            for event in EVENTS {
                if let Some(next) = DocumentWritePipeline::transition(phase, event) {
                    assert!(
                        next.rank() >= phase.rank(),
                        "{phase:?} + {event:?} regressed"
                    );
                    assert!(
                        next.rank() <= phase.rank() + 1,
                        "{phase:?} + {event:?} skipped"
                    );
                }
            }
        }
    }

    #[test]
    fn endpoint_crash_restart_and_duplicate_delivery_preserve_progress() {
        let machine = DocumentWritePipeline::new(DocumentWritePhase::IntentCaptured);
        for event in [
            DocumentWriteEvent::CanonicalApplied,
            DocumentWriteEvent::EndpointLost,
            DocumentWriteEvent::RetryRequested,
            DocumentWriteEvent::EndpointRestored,
            DocumentWriteEvent::CanonicalApplied,
            DocumentWriteEvent::ReplicaAccepted,
            DocumentWriteEvent::ReplicaVisible,
            DocumentWriteEvent::ReplicaVisible,
            DocumentWriteEvent::DiskProjected,
            DocumentWriteEvent::Committed,
        ] {
            assert!(machine.send(event));
        }
        assert_eq!(machine.state(), DocumentWritePhase::Committed);
    }

    #[test]
    fn simulation_never_regresses_under_arbitrary_churn() {
        // Deterministic pseudo-random simulation: each run injects endpoint
        // churn, duplicate facts, stale facts, and premature future facts.
        let mut seed = 0x5eed_cafe_u64;
        for _ in 0..10_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let initial = PHASES[(seed as usize) % PHASES.len()];
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let event = EVENTS[(seed as usize) % EVENTS.len()];
            if let Some(next) = DocumentWritePipeline::transition(initial, event) {
                assert!(next >= initial);
            }
        }
    }
}
