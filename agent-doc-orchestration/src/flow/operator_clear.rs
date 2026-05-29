use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorClearInputState {
    IdlePrompt,
    CleanExit,
    NoLivePane,
    ProtectedInput,
    Busy,
}

impl OperatorClearInputState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdlePrompt => "idle_prompt",
            Self::CleanExit => "clean_exit",
            Self::NoLivePane => "no_live_pane",
            Self::ProtectedInput => "protected_input",
            Self::Busy => "busy",
        }
    }
}

pub fn clear_guard_outcome(state: OperatorClearInputState) -> FlowOutcome {
    match state {
        OperatorClearInputState::IdlePrompt
        | OperatorClearInputState::CleanExit
        | OperatorClearInputState::NoLivePane => FlowOutcome::Completed,
        OperatorClearInputState::ProtectedInput => FlowOutcome::FailedClosed,
        OperatorClearInputState::Busy => FlowOutcome::Blocked,
    }
}

pub fn clear_guard_event(state: OperatorClearInputState) -> FlowEvent {
    FlowEvent::new(
        FlowName::OperatorClear,
        FlowStage::OperatorGuard,
        clear_guard_outcome(state),
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
