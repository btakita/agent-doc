use agent_doc_flow::types::{DocumentMutationKind, FlowEvent, FlowName, FlowOutcome, FlowStage};
use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_document_realtime::write_policy::{
    FullContentVisibleReplacementDecision, VisibleWriteDecision,
};
use agent_doc_template::patchback::{PatchbackShape, TemplatePatchbackPlan};

pub fn parse_template_patchback(
    file: &Path,
    response: &str,
    source: &str,
) -> Result<TemplatePatchbackPlan> {
    let plan = agent_doc_template::patchback::parse_template_patchback_plan(response)
        .context("failed to parse patch blocks from response")?;
    let parse_outcome = if (plan.shape == PatchbackShape::MalformedPatch
        && !plan.unmatched.trim().is_empty())
        || plan.shape == PatchbackShape::EscapedComponentMarkers
    {
        FlowOutcome::FailedClosed
    } else {
        FlowOutcome::Completed
    };

    if plan.marker_count > 0 || plan.raw_component_block_count > 0 {
        log_patchback_parse_event(file, plan.shape, parse_outcome);
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_parse_shape file={} source={} response_hash={} markers={} patches={} exchange_patches={} unmatched_len={}",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.marker_count,
                plan.patches.len(),
                plan.exchange_patch_count,
                plan.unmatched.trim().len()
            ),
        );
    }

    if plan.has_malformed_orphan_markers() {
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_malformed_rejected file={} source={} response_hash={} markers={} unmatched_len={} reason=patch_markers_without_closed_blocks",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.marker_count,
                plan.unmatched.trim().len()
            ),
        );
        anyhow::bail!(
            "malformed template patchback: found patch/replace markers but no closed patch blocks parsed; refusing to append unmatched content"
        );
    }
    if plan.has_escaped_component_markers() {
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_escaped_component_rejected file={} source={} response_hash={} component_blocks={} reason=raw_component_markers_without_patch_blocks",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.raw_component_block_count
            ),
        );
        anyhow::bail!(
            "escaped template patchback: response carries raw `<!-- agent:NAME -->` component blocks instead of supported `<!-- patch:* -->` patch blocks; refusing to commit them as literal exchange text. Wrap the response in `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` blocks, or rerun `agent-doc write --commit {}` to absorb an already-visible `### Re:` response.",
            file.display()
        );
    }

    Ok(plan)
}

pub fn mutation_kind_for_patch(component: &str) -> DocumentMutationKind {
    match component {
        "exchange" => DocumentMutationKind::ExchangePatch,
        "icebox" => DocumentMutationKind::IceboxPatch,
        "backlog" | "pending" => DocumentMutationKind::BacklogOp,
        _ => DocumentMutationKind::FullContent,
    }
}

pub fn patchback_parse_event(shape: PatchbackShape, outcome: FlowOutcome) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PatchbackParse,
        outcome,
    )
    .with_reason(shape.as_str())
}

pub fn log_patchback_parse_event(file: &Path, shape: PatchbackShape, outcome: FlowOutcome) {
    super::proof::log_flow_event(file, patchback_parse_event(shape, outcome));
}

pub fn visible_write_guard_event(decision: VisibleWriteDecision, source: &str) -> FlowEvent {
    let outcome = match decision {
        VisibleWriteDecision::Apply => FlowOutcome::Completed,
        VisibleWriteDecision::DeferActiveTyping => FlowOutcome::Blocked,
    };
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        outcome,
    )
    .with_reason(format!(
        "visible_write_typing_{}:{source}",
        decision.as_str()
    ))
}

pub fn visible_write_current_changed_event(source: &str) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        FlowOutcome::Blocked,
    )
    .with_reason(format!("visible_write_current_changed:{source}"))
}

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

pub fn log_template_structure_guard_event(
    file: &Path,
    reason: TemplateStructureGuardReason,
    outcome: FlowOutcome,
) {
    super::proof::log_flow_event(file, template_structure_guard_event(reason, outcome));
}

pub fn full_content_visible_replacement_event(
    decision: FullContentVisibleReplacementDecision,
    source: &str,
) -> FlowEvent {
    let outcome = match decision {
        FullContentVisibleReplacementDecision::Apply => FlowOutcome::Completed,
        FullContentVisibleReplacementDecision::RejectStaleSourceBuffer => FlowOutcome::Blocked,
    };
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        outcome,
    )
    .with_reason(format!(
        "full_content_source_buffer_{}:{source}",
        decision.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_template_patchback_rejects_raw_component_form_before_commit() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let raw_template_form = concat!(
            "<!-- agent:status -->\n",
            "Work complete.\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: closeout — gpt-5\n\n",
            "Implemented and verified.\n",
            "<!-- /agent:exchange -->\n",
        );
        let err = parse_template_patchback(&file, raw_template_form, "test").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escaped template patchback"),
            "expected escaped-component rejection, got: {msg}"
        );
        assert!(
            msg.contains("<!-- patch:exchange -->"),
            "diagnostic must point to the supported patch form, got: {msg}"
        );
    }

    #[test]
    fn parse_template_patchback_accepts_plain_response_without_component_markers() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let plain = "### Re: closeout — gpt-5\n\nImplemented and verified.\n";
        let plan = parse_template_patchback(&file, plain, "test").unwrap();
        assert_eq!(plan.shape, PatchbackShape::PlainResponse);
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

    #[test]
    fn parse_template_patchback_rejects_orphan_patch_markers() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let err =
            parse_template_patchback(&file, "<!-- patch:exchange -->\n### Re: broken\n", "test")
                .unwrap_err();

        assert!(err.to_string().contains("malformed template patchback"));
    }

    #[test]
    fn visible_write_guard_defers_when_typing_never_settles() {
        let decision = VisibleWriteDecision::DeferActiveTyping;
        let event = visible_write_guard_event(decision, "socket_ipc");

        assert_eq!(decision, VisibleWriteDecision::DeferActiveTyping);
        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("visible_write_typing_defer_active_typing:socket_ipc")
        );
    }

    #[test]
    fn visible_write_guard_allows_idle_writes() {
        let decision = VisibleWriteDecision::Apply;
        let event = visible_write_guard_event(decision, "socket_ipc");

        assert_eq!(decision, VisibleWriteDecision::Apply);
        assert_eq!(event.outcome, FlowOutcome::Completed);
    }

    #[test]
    fn full_content_visible_replacement_blocks_stale_source_buffer() {
        let decision = FullContentVisibleReplacementDecision::RejectStaleSourceBuffer;
        let event = full_content_visible_replacement_event(decision, "compact_exchange");

        assert_eq!(
            decision,
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        );
        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("full_content_source_buffer_reject_stale_source_buffer:compact_exchange")
        );
    }

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
