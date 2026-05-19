use super::types::{
    DocumentMutationKind, FlowEvent, FlowName, FlowOutcome, FlowStage, PatchbackShape,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatchbackShapeFacts {
    pub(crate) marker_count: usize,
    pub(crate) patch_count: usize,
    pub(crate) exchange_patch_count: usize,
    pub(crate) unmatched_len: usize,
    pub(crate) has_transcript_markers: bool,
}

pub(crate) fn classify_patchback_shape(facts: PatchbackShapeFacts) -> PatchbackShape {
    if facts.marker_count > 0 && facts.patch_count == 0 {
        return PatchbackShape::MalformedPatch;
    }
    if facts.patch_count > 0 && facts.unmatched_len > 0 {
        return PatchbackShape::MixedOutput;
    }
    if facts.patch_count > 0 {
        return PatchbackShape::ValidPatch;
    }
    if facts.has_transcript_markers {
        return PatchbackShape::TranscriptDump;
    }
    PatchbackShape::PlainResponse
}

pub(crate) fn mutation_kind_for_patch(component: &str) -> DocumentMutationKind {
    match component {
        "exchange" => DocumentMutationKind::ExchangePatch,
        "icebox" => DocumentMutationKind::IceboxPatch,
        "backlog" | "pending" => DocumentMutationKind::BacklogOp,
        _ => DocumentMutationKind::FullContent,
    }
}

pub(crate) fn patchback_parse_event(shape: PatchbackShape, outcome: FlowOutcome) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PatchbackParse,
        outcome,
    )
    .with_reason(shape.as_str())
}

pub(crate) fn log_patchback_parse_event(file: &Path, shape: PatchbackShape, outcome: FlowOutcome) {
    super::proof::log_flow_event(file, patchback_parse_event(shape, outcome));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_markers_without_closed_blocks_are_malformed() {
        let shape = classify_patchback_shape(PatchbackShapeFacts {
            marker_count: 2,
            patch_count: 0,
            exchange_patch_count: 0,
            unmatched_len: 42,
            has_transcript_markers: false,
        });

        assert_eq!(shape, PatchbackShape::MalformedPatch);
    }

    #[test]
    fn patch_plus_unmatched_text_is_mixed_output() {
        let shape = classify_patchback_shape(PatchbackShapeFacts {
            marker_count: 1,
            patch_count: 1,
            exchange_patch_count: 1,
            unmatched_len: 7,
            has_transcript_markers: false,
        });

        assert_eq!(shape, PatchbackShape::MixedOutput);
    }

    #[test]
    fn patchback_parse_event_carries_shape_reason() {
        let event =
            patchback_parse_event(PatchbackShape::MalformedPatch, FlowOutcome::FailedClosed);

        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PatchbackParse);
        assert_eq!(event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(event.reason.as_deref(), Some("malformed_patch"));
    }
}
