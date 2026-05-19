use super::types::FlowOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperatorClearInputState {
    IdlePrompt,
    CleanExit,
    ActiveAgentDoc,
    ProtectedInput,
    Busy,
}

pub(crate) fn clear_guard_outcome(state: OperatorClearInputState) -> FlowOutcome {
    match state {
        OperatorClearInputState::IdlePrompt | OperatorClearInputState::CleanExit => {
            FlowOutcome::Completed
        }
        OperatorClearInputState::ActiveAgentDoc | OperatorClearInputState::ProtectedInput => {
            FlowOutcome::FailedClosed
        }
        OperatorClearInputState::Busy => FlowOutcome::Blocked,
    }
}
