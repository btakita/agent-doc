use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCycleStep {
    pub stage: FlowStage,
    pub outcome: FlowOutcome,
    pub reason: Option<String>,
}

impl SessionCycleStep {
    pub fn completed(stage: FlowStage) -> Self {
        Self {
            stage,
            outcome: FlowOutcome::Completed,
            reason: None,
        }
    }
}

pub fn session_cycle_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: impl Into<Option<String>>,
) -> FlowEvent {
    let mut event = FlowEvent::new(FlowName::SessionCycle, stage, outcome);
    if let Some(reason) = reason.into() {
        event = event.with_reason(reason);
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cycle_event_adapts_flow_name_stage_outcome_and_reason() {
        let event = session_cycle_event(
            FlowStage::Preflight,
            FlowOutcome::Blocked,
            Some("waiting".to_string()),
        );

        assert_eq!(event.flow, FlowName::SessionCycle);
        assert_eq!(event.stage, FlowStage::Preflight);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(event.reason.as_deref(), Some("waiting"));
    }
}
