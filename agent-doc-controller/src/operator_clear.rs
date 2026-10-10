//! Pure operator-clear admission policy.

use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorClearInputState {
    IdlePrompt,
    CleanExit,
    NoLivePane,
    ProtectedInput,
    Busy,
    /// GH #232: the supervisor's live-turn evidence
    /// (`harness_turn_live`: the harness-authored turn-active lease for the
    /// owned pane, or an observed running dispatch receipt) says a turn is
    /// executing. A rendered composer between tool calls is NOT idleness, so
    /// this outranks an idle-looking prompt and the busy-cue fallback.
    LiveTurn,
}

impl OperatorClearInputState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdlePrompt => "idle_prompt",
            Self::CleanExit => "clean_exit",
            Self::NoLivePane => "no_live_pane",
            Self::ProtectedInput => "protected_input",
            Self::Busy => "busy",
            Self::LiveTurn => "harness_turn_live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorClearGuardOutcome {
    Completed,
    Blocked,
    FailedClosed,
}

impl OperatorClearGuardOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::FailedClosed => "failed_closed",
        }
    }
}

pub const fn clear_guard_outcome(state: OperatorClearInputState) -> OperatorClearGuardOutcome {
    match state {
        OperatorClearInputState::IdlePrompt
        | OperatorClearInputState::CleanExit
        | OperatorClearInputState::NoLivePane => OperatorClearGuardOutcome::Completed,
        OperatorClearInputState::ProtectedInput => OperatorClearGuardOutcome::FailedClosed,
        OperatorClearInputState::Busy | OperatorClearInputState::LiveTurn => {
            OperatorClearGuardOutcome::Blocked
        }
    }
}

pub const fn clear_guard_flow_outcome(outcome: OperatorClearGuardOutcome) -> FlowOutcome {
    match outcome {
        OperatorClearGuardOutcome::Completed => FlowOutcome::Completed,
        OperatorClearGuardOutcome::Blocked => FlowOutcome::Blocked,
        OperatorClearGuardOutcome::FailedClosed => FlowOutcome::FailedClosed,
    }
}

pub fn clear_guard_event(state: OperatorClearInputState) -> FlowEvent {
    FlowEvent::new(
        FlowName::OperatorClear,
        FlowStage::OperatorGuard,
        clear_guard_flow_outcome(clear_guard_outcome(state)),
    )
    .with_reason(state.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_clear_input_state_labels_are_stable() {
        assert_eq!(OperatorClearInputState::IdlePrompt.as_str(), "idle_prompt");
        assert_eq!(OperatorClearInputState::CleanExit.as_str(), "clean_exit");
        assert_eq!(OperatorClearInputState::NoLivePane.as_str(), "no_live_pane");
        assert_eq!(
            OperatorClearInputState::ProtectedInput.as_str(),
            "protected_input"
        );
        assert_eq!(OperatorClearInputState::Busy.as_str(), "busy");
        assert_eq!(
            OperatorClearInputState::LiveTurn.as_str(),
            "harness_turn_live"
        );
    }

    #[test]
    fn operator_clear_guard_outcome_labels_are_stable() {
        assert_eq!(OperatorClearGuardOutcome::Completed.as_str(), "completed");
        assert_eq!(OperatorClearGuardOutcome::Blocked.as_str(), "blocked");
        assert_eq!(
            OperatorClearGuardOutcome::FailedClosed.as_str(),
            "failed_closed"
        );
    }

    #[test]
    fn operator_clear_guard_outcome_matches_input_state() {
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::IdlePrompt),
            OperatorClearGuardOutcome::Completed
        );
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::CleanExit),
            OperatorClearGuardOutcome::Completed
        );
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::NoLivePane),
            OperatorClearGuardOutcome::Completed
        );
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::ProtectedInput),
            OperatorClearGuardOutcome::FailedClosed
        );
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::Busy),
            OperatorClearGuardOutcome::Blocked
        );
        assert_eq!(
            clear_guard_outcome(OperatorClearInputState::LiveTurn),
            OperatorClearGuardOutcome::Blocked
        );
    }

    /// GH #232: a live turn must never complete the guard as `idle_prompt`.
    #[test]
    fn operator_clear_guard_event_blocks_a_live_turn() {
        let event = clear_guard_event(OperatorClearInputState::LiveTurn);

        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("harness_turn_live"));
    }

    #[test]
    fn operator_clear_guard_event_uses_policy_outcome_and_input_reason() {
        let event = clear_guard_event(OperatorClearInputState::Busy);

        assert_eq!(event.flow, FlowName::OperatorClear);
        assert_eq!(event.stage, FlowStage::OperatorGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("busy"));
    }
}
