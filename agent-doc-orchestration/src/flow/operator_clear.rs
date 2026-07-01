use agent_doc_controller::operator_clear::{OperatorClearGuardOutcome, OperatorClearInputState};
use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

fn clear_guard_flow_outcome(outcome: OperatorClearGuardOutcome) -> FlowOutcome {
    match outcome {
        OperatorClearGuardOutcome::Completed => FlowOutcome::Completed,
        OperatorClearGuardOutcome::Blocked => FlowOutcome::Blocked,
        OperatorClearGuardOutcome::FailedClosed => FlowOutcome::FailedClosed,
    }
}

pub fn clear_guard_event(state: OperatorClearInputState) -> FlowEvent {
    let outcome = agent_doc_controller::operator_clear::clear_guard_outcome(state);
    FlowEvent::new(
        FlowName::OperatorClear,
        FlowStage::OperatorGuard,
        clear_guard_flow_outcome(outcome),
    )
    .with_reason(state.as_str())
}

pub fn log_clear_guard_event(file: &Path, state: OperatorClearInputState) {
    super::proof::log_flow_event(file, clear_guard_event(state));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_clear_blocks_without_destructive_confirmation() {
        let event = clear_guard_event(OperatorClearInputState::Busy);

        assert_eq!(event.flow, FlowName::OperatorClear);
        assert_eq!(event.stage, FlowStage::OperatorGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("busy"));
    }
}
