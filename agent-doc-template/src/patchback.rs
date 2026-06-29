//! Pure template patchback classification policy.

use std::fmt;

use agent_doc_element::element;

use crate::template::{PatchBlock, parse_patches};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatchbackShape {
    ValidPatch,
    PlainResponse,
    MalformedPatch,
    TranscriptDump,
    MixedOutput,
    /// Raw template form: stdin carries literal `<!-- agent:NAME -->` /
    /// `<!-- /agent:NAME -->` component blocks instead of supported
    /// `<!-- patch:* -->` patch blocks. Committing these as plain text escapes
    /// the markers into the live exchange (`#closeout-repair-churn`), so this
    /// shape must fail closed before commit.
    EscapedComponentMarkers,
}

impl PatchbackShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidPatch => "valid_patch",
            Self::PlainResponse => "plain_response",
            Self::MalformedPatch => "malformed_patch",
            Self::TranscriptDump => "transcript_dump",
            Self::MixedOutput => "mixed_output",
            Self::EscapedComponentMarkers => "escaped_component_markers",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchbackShapeFacts {
    pub marker_count: usize,
    pub patch_count: usize,
    pub exchange_patch_count: usize,
    pub unmatched_len: usize,
    pub has_transcript_markers: bool,
    /// Count of fully-formed `<!-- agent:NAME -->` ... `<!-- /agent:NAME -->`
    /// component blocks parsed from the response stdin (outside code fences).
    /// Non-zero with no patch markers means the caller piped the raw template
    /// form instead of `<!-- patch:* -->` blocks.
    pub raw_component_block_count: usize,
}

pub fn classify_patchback_shape(facts: PatchbackShapeFacts) -> PatchbackShape {
    if facts.marker_count > 0 && facts.patch_count == 0 {
        return PatchbackShape::MalformedPatch;
    }
    if facts.patch_count > 0 && facts.unmatched_len > 0 {
        return PatchbackShape::MixedOutput;
    }
    if facts.patch_count > 0 {
        return PatchbackShape::ValidPatch;
    }
    // No supported patch blocks. A response that instead carries literal
    // component blocks is the raw template form and must not be committed as
    // plain exchange text (`#closeout-repair-churn`).
    if facts.raw_component_block_count > 0 {
        return PatchbackShape::EscapedComponentMarkers;
    }
    if facts.has_transcript_markers {
        return PatchbackShape::TranscriptDump;
    }
    PatchbackShape::PlainResponse
}

/// Count fully-formed `<!-- agent:NAME -->` component blocks in a response.
///
/// Reuses the validated component parser so boundary markers
/// (`<!-- agent:boundary:HASH -->`, which have no close marker) and markers
/// inside code fences are excluded. A parse error (mismatched/invalid markers)
/// is treated as "no clean component blocks" because the malformed/plain paths
/// handle that shape separately.
pub fn raw_component_block_count(response: &str) -> usize {
    element::parse(response)
        .map(|components| components.len())
        .unwrap_or(0)
}

pub fn patchback_marker_count_outside_code(response: &str) -> usize {
    let code_ranges = element::find_code_ranges(response);
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

pub fn patchback_shape_facts(
    response: &str,
    patches: &[PatchBlock],
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
        raw_component_block_count: raw_component_block_count(response),
    }
}

#[derive(Debug, Clone)]
pub struct TemplatePatchbackPlan {
    pub patches: Vec<PatchBlock>,
    pub unmatched: String,
    pub shape: PatchbackShape,
    pub marker_count: usize,
    pub exchange_patch_count: usize,
    pub raw_component_block_count: usize,
}

impl TemplatePatchbackPlan {
    pub fn has_malformed_orphan_markers(&self) -> bool {
        self.shape == PatchbackShape::MalformedPatch && !self.unmatched.trim().is_empty()
    }

    pub fn has_escaped_component_markers(&self) -> bool {
        self.shape == PatchbackShape::EscapedComponentMarkers
    }
}

pub fn parse_template_patchback_plan(response: &str) -> anyhow::Result<TemplatePatchbackPlan> {
    let (patches, unmatched) = parse_patches(response)?;
    let facts = patchback_shape_facts(response, &patches, &unmatched);
    let shape = classify_patchback_shape(facts.clone());
    Ok(TemplatePatchbackPlan {
        patches,
        unmatched,
        shape,
        marker_count: facts.marker_count,
        exchange_patch_count: facts.exchange_patch_count,
        raw_component_block_count: facts.raw_component_block_count,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratePatchbackRejectReason {
    MissingExchangePatch,
    RawUnmatchedContent,
    FullDocumentComponentMarkers,
    TranscriptPromptLines,
    TranscriptHeadings,
    MultipleAssistantResponses,
}

impl OrchestratePatchbackRejectReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingExchangePatch => "missing_exchange_patch",
            Self::RawUnmatchedContent => "raw_unmatched_content",
            Self::FullDocumentComponentMarkers => "full_document_component_markers",
            Self::TranscriptPromptLines => "transcript_prompt_lines",
            Self::TranscriptHeadings => "transcript_headings",
            Self::MultipleAssistantResponses => "multiple_assistant_responses",
        }
    }

    pub const fn message(self) -> &'static str {
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

impl fmt::Display for OrchestratePatchbackRejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for OrchestratePatchbackRejectReason {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratePatchbackDecision {
    AcceptExplicitPatch,
    AcceptPlainResponse,
    Reject(OrchestratePatchbackRejectReason),
}

impl OrchestratePatchbackDecision {
    pub fn is_accepted(self) -> bool {
        !matches!(self, Self::Reject(_))
    }
}

pub fn classify_orchestrate_patchback(
    patches: &[PatchBlock],
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

pub fn classify_orchestrate_plain_response(unmatched: &str) -> OrchestratePatchbackDecision {
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

pub fn enforce_orchestrate_patchback_contract(
    origin: Option<&str>,
    patches: &[PatchBlock],
    unmatched: &str,
) -> Result<(), OrchestratePatchbackRejectReason> {
    if origin != Some("orchestrate") {
        return Ok(());
    }
    match classify_orchestrate_patchback(patches, unmatched) {
        OrchestratePatchbackDecision::AcceptExplicitPatch
        | OrchestratePatchbackDecision::AcceptPlainResponse => Ok(()),
        OrchestratePatchbackDecision::Reject(reason) => Err(reason),
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
            raw_component_block_count: 0,
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
            raw_component_block_count: 0,
        });

        assert_eq!(shape, PatchbackShape::MixedOutput);
    }

    #[test]
    fn raw_component_blocks_without_patches_are_escaped_component_markers() {
        let shape = classify_patchback_shape(PatchbackShapeFacts {
            marker_count: 0,
            patch_count: 0,
            exchange_patch_count: 0,
            unmatched_len: 64,
            has_transcript_markers: false,
            raw_component_block_count: 1,
        });

        assert_eq!(shape, PatchbackShape::EscapedComponentMarkers);
    }

    #[test]
    fn valid_patches_take_precedence_over_incidental_component_blocks() {
        let shape = classify_patchback_shape(PatchbackShapeFacts {
            marker_count: 1,
            patch_count: 1,
            exchange_patch_count: 1,
            unmatched_len: 0,
            has_transcript_markers: false,
            raw_component_block_count: 1,
        });

        assert_eq!(shape, PatchbackShape::ValidPatch);
    }

    #[test]
    fn raw_component_block_count_ignores_plain_response_and_boundary_markers() {
        assert_eq!(
            raw_component_block_count("### Re: topic - gpt-5\n\nImplemented and verified.\n"),
            0
        );
        assert_eq!(
            raw_component_block_count("<!-- agent:boundary:abc123 -->\nsome text\n"),
            0
        );
        assert_eq!(
            raw_component_block_count(
                "<!-- agent:exchange -->\n### Re: topic\nbody\n<!-- /agent:exchange -->\n"
            ),
            1
        );
    }

    #[test]
    fn parse_template_patchback_plan_accepts_plain_response_without_component_markers() {
        let plain = "### Re: closeout - gpt-5\n\nImplemented and verified.\n";
        let plan = parse_template_patchback_plan(plain).unwrap();
        assert_eq!(plan.shape, PatchbackShape::PlainResponse);
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
        let patches = vec![PatchBlock::new("status", "updated")];
        let decision = classify_orchestrate_patchback(&patches, "");

        assert_eq!(
            decision,
            OrchestratePatchbackDecision::Reject(
                OrchestratePatchbackRejectReason::MissingExchangePatch
            )
        );
    }
}
