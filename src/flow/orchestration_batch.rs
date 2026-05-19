use super::types::FlowOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchChildResult {
    pub(crate) label: String,
    pub(crate) outcome: FlowOutcome,
    pub(crate) proof: Option<String>,
}

pub(crate) fn batch_should_continue(
    source_changed_after_child: bool,
    child: &BatchChildResult,
) -> bool {
    !source_changed_after_child && child.outcome == FlowOutcome::Completed
}
