use super::types::{
    DocumentMutationKind, FlowEvent, FlowName, FlowOutcome, FlowStage, PatchbackShape,
};
use anyhow::{Context, Result};
use std::path::Path;

use crate::{component, template};

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

#[derive(Debug, Clone)]
pub(crate) struct TemplatePatchbackPlan {
    pub(crate) patches: Vec<template::PatchBlock>,
    pub(crate) unmatched: String,
    pub(crate) shape: PatchbackShape,
    pub(crate) marker_count: usize,
    pub(crate) exchange_patch_count: usize,
}

impl TemplatePatchbackPlan {
    pub(crate) fn has_malformed_orphan_markers(&self) -> bool {
        self.shape == PatchbackShape::MalformedPatch && !self.unmatched.trim().is_empty()
    }
}

pub(crate) fn patchback_marker_count_outside_code(response: &str) -> usize {
    let code_ranges = component::find_code_ranges(response);
    let markers = [
        "<!-- patch:",
        "<!-- /patch:",
        "<!-- replace:",
        "<!-- /replace:",
    ];
    let mut count = 0usize;
    for marker in markers {
        let mut search_from = 0usize;
        while let Some(rel) = response[search_from..].find(marker) {
            let pos = search_from + rel;
            if !code_ranges
                .iter()
                .any(|&(start, end)| pos >= start && pos < end)
            {
                count += 1;
            }
            search_from = pos + marker.len();
        }
    }
    count
}

fn patchback_shape_facts(
    response: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> PatchbackShapeFacts {
    PatchbackShapeFacts {
        marker_count: patchback_marker_count_outside_code(response),
        patch_count: patches.len(),
        exchange_patch_count: patches
            .iter()
            .filter(|patch| patch.name == "exchange")
            .count(),
        unmatched_len: unmatched.trim().len(),
        has_transcript_markers: response.contains("## User") || response.contains("## Assistant"),
    }
}

pub(crate) fn parse_template_patchback(
    file: &Path,
    response: &str,
    source: &str,
) -> Result<TemplatePatchbackPlan> {
    let (patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;
    let facts = patchback_shape_facts(response, &patches, &unmatched);
    let shape = classify_patchback_shape(facts.clone());
    let parse_outcome = if shape == PatchbackShape::MalformedPatch && facts.unmatched_len > 0 {
        FlowOutcome::FailedClosed
    } else {
        FlowOutcome::Completed
    };

    if facts.marker_count > 0 {
        log_patchback_parse_event(file, shape, parse_outcome);
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_parse_shape file={} source={} response_hash={} markers={} patches={} exchange_patches={} unmatched_len={}",
                file.display(),
                source,
                crate::ops_log::content_hash(response),
                facts.marker_count,
                facts.patch_count,
                facts.exchange_patch_count,
                facts.unmatched_len
            ),
        );
    }

    let plan = TemplatePatchbackPlan {
        patches,
        unmatched,
        shape,
        marker_count: facts.marker_count,
        exchange_patch_count: facts.exchange_patch_count,
    };
    if plan.has_malformed_orphan_markers() {
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_malformed_rejected file={} source={} response_hash={} markers={} unmatched_len={} reason=patch_markers_without_closed_blocks",
                file.display(),
                source,
                crate::ops_log::content_hash(response),
                plan.marker_count,
                plan.unmatched.trim().len()
            ),
        );
        anyhow::bail!(
            "malformed template patchback: found patch/replace markers but no closed patch blocks parsed; refusing to append unmatched content"
        );
    }

    Ok(plan)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleWriteTypingFacts {
    pub(crate) idle_reached: bool,
    pub(crate) timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibleWriteDecision {
    Apply,
    DeferActiveTyping,
}

impl VisibleWriteDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::DeferActiveTyping => "defer_active_typing",
        }
    }

    pub(crate) const fn outcome(self) -> FlowOutcome {
        match self {
            Self::Apply => FlowOutcome::Completed,
            Self::DeferActiveTyping => FlowOutcome::Blocked,
        }
    }
}

pub(crate) fn decide_visible_write_after_typing(
    facts: VisibleWriteTypingFacts,
) -> VisibleWriteDecision {
    let _timeout_ms = facts.timeout_ms;
    if facts.idle_reached {
        VisibleWriteDecision::Apply
    } else {
        VisibleWriteDecision::DeferActiveTyping
    }
}

pub(crate) fn visible_write_guard_event(decision: VisibleWriteDecision, source: &str) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        decision.outcome(),
    )
    .with_reason(format!(
        "visible_write_typing_{}:{source}",
        decision.as_str()
    ))
}

pub(crate) fn visible_write_current_changed_event(source: &str) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        FlowOutcome::Blocked,
    )
    .with_reason(format!("visible_write_current_changed:{source}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FullContentSourceProof {
    pub(crate) expected_content_hash: String,
    pub(crate) expected_content_len: usize,
}

impl FullContentSourceProof {
    pub(crate) fn from_content(content: &str) -> Self {
        Self {
            expected_content_hash: crate::ops_log::content_hash(content),
            expected_content_len: content.len(),
        }
    }

    pub(crate) fn matches_current(&self, current_content: &str) -> bool {
        current_content.len() == self.expected_content_len
            && crate::ops_log::content_hash(current_content) == self.expected_content_hash
    }
}

pub(crate) fn full_content_source_proof(
    before_content: Option<&str>,
) -> Option<FullContentSourceProof> {
    before_content.map(FullContentSourceProof::from_content)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullContentVisibleReplacementDecision {
    Apply,
    RejectStaleSourceBuffer,
}

impl FullContentVisibleReplacementDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::RejectStaleSourceBuffer => "reject_stale_source_buffer",
        }
    }

    pub(crate) const fn outcome(self) -> FlowOutcome {
        match self {
            Self::Apply => FlowOutcome::Completed,
            Self::RejectStaleSourceBuffer => FlowOutcome::Blocked,
        }
    }
}

pub(crate) fn decide_full_content_visible_replacement(
    current_content: &str,
    proof: Option<&FullContentSourceProof>,
) -> FullContentVisibleReplacementDecision {
    match proof {
        Some(proof) if !proof.matches_current(current_content) => {
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        }
        _ => FullContentVisibleReplacementDecision::Apply,
    }
}

pub(crate) fn full_content_visible_replacement_event(
    decision: FullContentVisibleReplacementDecision,
    source: &str,
) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PreWriteGuard,
        decision.outcome(),
    )
    .with_reason(format!(
        "full_content_source_buffer_{}:{source}",
        decision.as_str()
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrchestratePatchbackRejectReason {
    MissingExchangePatch,
    RawUnmatchedContent,
    FullDocumentComponentMarkers,
    TranscriptPromptLines,
    TranscriptHeadings,
    MultipleAssistantResponses,
}

impl OrchestratePatchbackRejectReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::MissingExchangePatch => "missing_exchange_patch",
            Self::RawUnmatchedContent => "raw_unmatched_content",
            Self::FullDocumentComponentMarkers => "full_document_component_markers",
            Self::TranscriptPromptLines => "transcript_prompt_lines",
            Self::TranscriptHeadings => "transcript_headings",
            Self::MultipleAssistantResponses => "multiple_assistant_responses",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::MissingExchangePatch => {
                "orchestrate template-mode responses must include a <!-- patch:exchange --> block"
            }
            Self::RawUnmatchedContent => {
                "orchestrate template-mode responses must not include raw unmatched content outside patch blocks"
            }
            Self::FullDocumentComponentMarkers => {
                "orchestrate template-mode plain responses must not include full document component markers"
            }
            Self::TranscriptPromptLines => {
                "orchestrate template-mode plain responses must not include transcript prompt lines"
            }
            Self::TranscriptHeadings => {
                "orchestrate template-mode plain responses must not include transcript headings"
            }
            Self::MultipleAssistantResponses => {
                "orchestrate template-mode plain responses must contain only one assistant response"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrchestratePatchbackDecision {
    AcceptExplicitPatch,
    AcceptPlainResponse,
    Reject(OrchestratePatchbackRejectReason),
}

impl OrchestratePatchbackDecision {
    pub(crate) fn is_accepted(self) -> bool {
        !matches!(self, Self::Reject(_))
    }
}

pub(crate) fn classify_orchestrate_patchback(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> OrchestratePatchbackDecision {
    if patches.is_empty() {
        return classify_orchestrate_plain_response(unmatched);
    }
    if !patches.iter().any(|patch| patch.name == "exchange") {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::MissingExchangePatch,
        );
    }
    if !unmatched.trim().is_empty() {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::RawUnmatchedContent,
        );
    }
    OrchestratePatchbackDecision::AcceptExplicitPatch
}

pub(crate) fn classify_orchestrate_plain_response(unmatched: &str) -> OrchestratePatchbackDecision {
    let trimmed = unmatched.trim();
    if trimmed.is_empty() {
        return OrchestratePatchbackDecision::AcceptPlainResponse;
    }

    if trimmed.contains("<!-- agent:")
        || trimmed.contains("<!-- /agent:")
        || trimmed.contains("&lt;!-- agent:")
        || trimmed.contains("&lt;!-- /agent:")
    {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::FullDocumentComponentMarkers,
        );
    }

    if trimmed
        .lines()
        .any(|line| line.trim_start().starts_with('❯'))
    {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::TranscriptPromptLines,
        );
    }

    if trimmed.lines().any(|line| {
        let line = line.trim();
        line == "## User"
            || line.starts_with("## User ")
            || line == "## Assistant"
            || line.starts_with("## Assistant ")
    }) {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::TranscriptHeadings,
        );
    }

    let response_headings = trimmed
        .lines()
        .filter(|line| line.trim_start().starts_with("### Re:"))
        .count();
    if response_headings > 1 {
        return OrchestratePatchbackDecision::Reject(
            OrchestratePatchbackRejectReason::MultipleAssistantResponses,
        );
    }

    OrchestratePatchbackDecision::AcceptPlainResponse
}

pub(crate) fn enforce_orchestrate_patchback_contract(
    origin: Option<&str>,
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    if origin != Some("orchestrate") {
        return Ok(());
    }
    match classify_orchestrate_patchback(patches, unmatched) {
        OrchestratePatchbackDecision::AcceptExplicitPatch
        | OrchestratePatchbackDecision::AcceptPlainResponse => Ok(()),
        OrchestratePatchbackDecision::Reject(reason) => anyhow::bail!(reason.message()),
    }
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
        let decision = decide_visible_write_after_typing(VisibleWriteTypingFacts {
            idle_reached: false,
            timeout_ms: 5_000,
        });
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
        let decision = decide_visible_write_after_typing(VisibleWriteTypingFacts {
            idle_reached: true,
            timeout_ms: 5_000,
        });

        assert_eq!(decision, VisibleWriteDecision::Apply);
    }

    #[test]
    fn full_content_source_proof_matches_original_buffer_only() {
        let proof = FullContentSourceProof::from_content("before");
        let utf8_proof = FullContentSourceProof::from_content("before ❯");

        assert!(proof.matches_current("before"));
        assert!(!proof.matches_current("before\nlive prompt"));
        assert!(!proof.matches_current("beforE"));
        assert_eq!(utf8_proof.expected_content_len, "before ❯".len());
        assert!(utf8_proof.expected_content_len > "before ❯".chars().count());
    }

    #[test]
    fn full_content_visible_replacement_blocks_stale_source_buffer() {
        let proof = FullContentSourceProof::from_content("before");
        let decision = decide_full_content_visible_replacement("before\nlive prompt", Some(&proof));
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
    fn orchestrate_plain_response_rejects_transcript_prompt_lines() {
        let decision = classify_orchestrate_plain_response("### Re: done\n\nDone.\n❯ do #next\n");

        assert_eq!(
            decision,
            OrchestratePatchbackDecision::Reject(
                OrchestratePatchbackRejectReason::TranscriptPromptLines
            )
        );
    }

    #[test]
    fn orchestrate_patchback_requires_exchange_patch() {
        let patches = vec![template::PatchBlock::new("status", "updated")];
        let decision = classify_orchestrate_patchback(&patches, "");

        assert_eq!(
            decision,
            OrchestratePatchbackDecision::Reject(
                OrchestratePatchbackRejectReason::MissingExchangePatch
            )
        );
    }
}
