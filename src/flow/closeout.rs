use super::types::{CloseoutState, FlowOutcome};

pub(crate) fn closeout_state_from_cycle_phase(phase: &str) -> Option<CloseoutState> {
    match phase {
        "preflight_started" => Some(CloseoutState::PreflightStarted),
        "response_captured" => Some(CloseoutState::ResponseCaptured),
        "write_applied" => Some(CloseoutState::WriteApplied),
        "committed" => Some(CloseoutState::Committed),
        "abandoned" => Some(CloseoutState::Abandoned),
        _ => None,
    }
}

pub(crate) fn terminal_guard_outcome(state: CloseoutState) -> FlowOutcome {
    match state {
        CloseoutState::Committed => FlowOutcome::Completed,
        CloseoutState::Abandoned => FlowOutcome::FailedClosed,
        CloseoutState::PreflightStarted
        | CloseoutState::ResponseCaptured
        | CloseoutState::WriteApplied => FlowOutcome::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_is_terminal_completed() {
        assert_eq!(
            terminal_guard_outcome(CloseoutState::Committed),
            FlowOutcome::Completed
        );
    }
}
