use super::types::{FlowOutcome, FlowStage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionCycleStep {
    pub(crate) stage: FlowStage,
    pub(crate) outcome: FlowOutcome,
    pub(crate) reason: Option<String>,
}

impl SessionCycleStep {
    pub(crate) fn completed(stage: FlowStage) -> Self {
        Self {
            stage,
            outcome: FlowOutcome::Completed,
            reason: None,
        }
    }
}
