use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateStructureGuardReason {
    DuplicateScaffoldDropped,
    DuplicatePromptResidue,
    MixedDuplicateScaffoldTail,
    DuplicateCloseTailMoved,
}

impl TemplateStructureGuardReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateScaffoldDropped => "duplicate_scaffold_dropped",
            Self::DuplicatePromptResidue => "duplicate_prompt_residue",
            Self::MixedDuplicateScaffoldTail => "mixed_duplicate_scaffold_tail",
            Self::DuplicateCloseTailMoved => "duplicate_close_tail_moved",
        }
    }
}

pub fn template_structure_guard_event(
    reason: TemplateStructureGuardReason,
    outcome: FlowOutcome,
) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::DocumentMutation,
        outcome,
    )
    .with_reason(reason.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_structure_guard_event_carries_mixed_duplicate_scaffold_reason() {
        let event = template_structure_guard_event(
            TemplateStructureGuardReason::MixedDuplicateScaffoldTail,
            FlowOutcome::FailedClosed,
        );

        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::DocumentMutation);
        assert_eq!(event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            event.reason.as_deref(),
            Some("mixed_duplicate_scaffold_tail")
        );
    }

    #[test]
    fn template_structure_guard_event_carries_duplicate_prompt_residue_reason() {
        let event = template_structure_guard_event(
            TemplateStructureGuardReason::DuplicatePromptResidue,
            FlowOutcome::FailedClosed,
        );

        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::DocumentMutation);
        assert_eq!(event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(event.reason.as_deref(), Some("duplicate_prompt_residue"));
    }
}
