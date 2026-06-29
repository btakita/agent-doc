use super::types::{
    DocumentMutationKind, FlowEvent, FlowName, FlowOutcome, FlowStage, PatchbackShape,
};
use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_element::element;

use agent_doc_template as template;

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
/// is treated as "no clean component blocks" — that shape is handled by the
/// existing malformed/plain paths rather than this raw-template guard.
pub fn raw_component_block_count(response: &str) -> usize {
    element::parse(response)
        .map(|components| components.len())
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct TemplatePatchbackPlan {
    pub patches: Vec<template::PatchBlock>,
    pub unmatched: String,
    pub shape: PatchbackShape,
    pub marker_count: usize,
    pub exchange_patch_count: usize,
}

impl TemplatePatchbackPlan {
    pub fn has_malformed_orphan_markers(&self) -> bool {
        self.shape == PatchbackShape::MalformedPatch && !self.unmatched.trim().is_empty()
    }

    pub fn has_escaped_component_markers(&self) -> bool {
        self.shape == PatchbackShape::EscapedComponentMarkers
    }
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
        raw_component_block_count: raw_component_block_count(response),
    }
}

pub fn parse_template_patchback(
    file: &Path,
    response: &str,
    source: &str,
) -> Result<TemplatePatchbackPlan> {
    let (patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;
    let facts = patchback_shape_facts(response, &patches, &unmatched);
    let shape = classify_patchback_shape(facts.clone());
    let parse_outcome = if (shape == PatchbackShape::MalformedPatch && facts.unmatched_len > 0)
        || shape == PatchbackShape::EscapedComponentMarkers
    {
        FlowOutcome::FailedClosed
    } else {
        FlowOutcome::Completed
    };

    if facts.marker_count > 0 || facts.raw_component_block_count > 0 {
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
    if plan.has_escaped_component_markers() {
        crate::ops_log::log_op(
            file,
            &format!(
                "template_patchback_escaped_component_rejected file={} source={} response_hash={} component_blocks={} reason=raw_component_markers_without_patch_blocks",
                file.display(),
                source,
                crate::ops_log::content_hash(response),
                facts.raw_component_block_count
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleWriteTypingFacts {
    pub idle_reached: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleWriteDecision {
    Apply,
    DeferActiveTyping,
}

impl VisibleWriteDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::DeferActiveTyping => "defer_active_typing",
        }
    }

    pub const fn outcome(self) -> FlowOutcome {
        match self {
            Self::Apply => FlowOutcome::Completed,
            Self::DeferActiveTyping => FlowOutcome::Blocked,
        }
    }
}

pub fn decide_visible_write_after_typing(facts: VisibleWriteTypingFacts) -> VisibleWriteDecision {
    let _timeout_ms = facts.timeout_ms;
    if facts.idle_reached {
        VisibleWriteDecision::Apply
    } else {
        VisibleWriteDecision::DeferActiveTyping
    }
}

pub fn visible_write_guard_event(decision: VisibleWriteDecision, source: &str) -> FlowEvent {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullContentSourceProof {
    pub expected_content_hash: String,
    pub expected_content_len: usize,
}

impl FullContentSourceProof {
    pub fn from_content(content: &str) -> Self {
        Self {
            expected_content_hash: crate::ops_log::content_hash(content),
            expected_content_len: content.len(),
        }
    }

    pub fn matches_current(&self, current_content: &str) -> bool {
        current_content.len() == self.expected_content_len
            && crate::ops_log::content_hash(current_content) == self.expected_content_hash
    }
}

pub fn full_content_source_proof(before_content: Option<&str>) -> Option<FullContentSourceProof> {
    before_content.map(FullContentSourceProof::from_content)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullContentVisibleReplacementDecision {
    Apply,
    RejectStaleSourceBuffer,
}

impl FullContentVisibleReplacementDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::RejectStaleSourceBuffer => "reject_stale_source_buffer",
        }
    }

    pub const fn outcome(self) -> FlowOutcome {
        match self {
            Self::Apply => FlowOutcome::Completed,
            Self::RejectStaleSourceBuffer => FlowOutcome::Blocked,
        }
    }
}

pub fn decide_full_content_visible_replacement(
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

pub fn full_content_visible_replacement_event(
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

/// Decision for reconciling a JetBrains editor buffer against disk when the
/// plugin (re)connects its IPC listener (`#yzer` / `#evmhplugin`, the plugin
/// half of `#evmh`).
///
/// While the plugin is disconnected (supervisor down, plugin/cdylib reload), the
/// binary may commit control-plane content to disk/HEAD. On reconnect the editor
/// buffer is then stale vs HEAD. If the plugin later pushes that stale buffer
/// back via `save_document`, it reverts the binary's committed writes — the
/// `#postcommit-ipc-worktree-corruption` direction. The fix: on reconnect,
/// re-read disk/HEAD into the buffer when (and only when) we can PROVE the buffer
/// is stale committed content. The editor wins only for genuine user edits
/// (per `#editorbufwin` P1), so an unprovable divergence keeps the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectBufferDecision {
    /// Buffer already matches disk — nothing to do.
    InSync,
    /// Buffer equals a prior commit of the file and disk is clean HEAD — the
    /// binary advanced disk while the plugin was disconnected. HEAD wins:
    /// re-read disk into the buffer.
    RereadDisk,
    /// Buffer diverges from disk but is not a known prior commit — treat as
    /// genuine local user edits. Editor wins: keep the buffer.
    KeepBuffer,
}

impl ReconnectBufferDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InSync => "in_sync",
            Self::RereadDisk => "reread_disk",
            Self::KeepBuffer => "keep_buffer",
        }
    }
}

/// Decide how to reconcile an editor buffer with disk on plugin IPC reconnect.
///
/// - `buffer_matches_disk`: editor buffer content == on-disk content.
/// - `disk_is_committed_head`: on-disk content == the file's blob at `HEAD`
///   (the working tree is clean for this file — disk is authoritative committed
///   content the binary wrote, not a half-applied write).
/// - `buffer_matches_prior_commit`: editor buffer content == the file's blob at
///   some recent prior commit (definitive proof the buffer is stale committed
///   content, not unsynced user edits).
///
/// Re-read disk only when the buffer is PROVABLY stale (equals a prior commit)
/// and disk is clean HEAD; otherwise keep the buffer so genuine user edits are
/// never clobbered.
pub fn decide_reconnect_buffer(
    buffer_matches_disk: bool,
    disk_is_committed_head: bool,
    buffer_matches_prior_commit: bool,
) -> ReconnectBufferDecision {
    if buffer_matches_disk {
        return ReconnectBufferDecision::InSync;
    }
    if disk_is_committed_head && buffer_matches_prior_commit {
        return ReconnectBufferDecision::RereadDisk;
    }
    ReconnectBufferDecision::KeepBuffer
}

/// Decision for a finalize/converge write when the editor-IPC socket may be
/// absent, connectable, or backed by a live editor endpoint (`#kcb5`).
///
/// After the 08b in-process-supervisor cutover the controller hosts the
/// editor-IPC socket even when no JB plugin is attached, so a pure-CLI session
/// can have a *connectable* socket with no editor. Under the realtime cutover, a
/// connectable socket that fails to prove delivery is still not permission to
/// write behind a live editor path. If no editor endpoint owns the document, the
/// current file is the detached realtime replica and may be updated through the
/// guarded `DetachedDisk` path.
///
/// Safety invariant: unproven delivery to a live editor always fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorlessDiskFallbackDecision {
    /// A live editor endpoint is (or may be) present and delivery is unproven —
    /// never clobber the buffer; retry the editor/CRDT path.
    FailClosed,
    /// No editor endpoint owns the document and the editor path is absent or has
    /// failed its delivery proof. The current file is the detached realtime
    /// replica, so a guarded direct disk write is allowed.
    DetachedDisk,
    /// Explicit operator force — route to the controller-host disk write.
    ForceDiskNoEditor,
    /// A live editor endpoint is present and reachable — converge through it.
    ConvergeViaEditor,
}

impl EditorlessDiskFallbackDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FailClosed => "fail_closed",
            Self::DetachedDisk => "detached_disk",
            Self::ForceDiskNoEditor => "force_disk_no_editor",
            Self::ConvergeViaEditor => "converge_via_editor",
        }
    }
}

/// Decide how to handle a finalize/converge write against a (possibly editor-less)
/// editor-IPC socket (`#kcb5`).
///
/// - `socket_connectable`: the editor-IPC socket accepts a connection (true even
///   when only the controller — not a JB plugin — is behind it).
/// - `editor_endpoint_proven`: a live JB plugin editor consumer is registered for
///   this document (distinct from the controller merely hosting the socket).
/// - `consecutive_no_ack`: consecutive `no_ack` / `send_failed` / `error` acks
///   with no proven delivery this run.
/// - `threshold`: how many consecutive failures prove "no delivery."
/// - `force_disk_requested`: the operator passed `--force-disk`.
///
/// Routes to disk when no editor endpoint owns the document and the editor path
/// is absent or has proven no delivery; explicit `--force-disk` remains a
/// separate operator override.
pub fn decide_editorless_disk_fallback(
    socket_connectable: bool,
    editor_endpoint_proven: bool,
    consecutive_no_ack: usize,
    threshold: usize,
    force_disk_requested: bool,
) -> EditorlessDiskFallbackDecision {
    if force_disk_requested {
        return EditorlessDiskFallbackDecision::ForceDiskNoEditor;
    }
    if editor_endpoint_proven {
        // A real editor buffer is in play: protect it. Unproven delivery means
        // retry, never a disk clobber (preserves #editorbufwin / the FCC guard).
        return if consecutive_no_ack >= threshold && threshold > 0 {
            EditorlessDiskFallbackDecision::FailClosed
        } else {
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        };
    }
    // No editor endpoint proven. With no socket there is no editor transport to
    // converge through; after repeated no-ACKs a controller-hosted socket has
    // likewise proven no delivery. In both cases the current file is the
    // detached realtime replica.
    if !socket_connectable || (threshold > 0 && consecutive_no_ack >= threshold) {
        return EditorlessDiskFallbackDecision::DetachedDisk;
    }
    EditorlessDiskFallbackDecision::FailClosed
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
    fn editorless_detached_disk_after_no_delivery_or_no_listener() {
        // #kcb5 cutover: connectable controller socket, no editor, every send
        // no_acks. Once delivery is proven absent, the current file is the
        // detached realtime replica.
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 3, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        // No listener at all also uses detached disk authority.
        assert_eq!(
            decide_editorless_disk_fallback(false, false, 0, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        // Explicit --force-disk overrides everything.
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, true),
            EditorlessDiskFallbackDecision::ForceDiskNoEditor
        );
    }

    #[test]
    fn editorless_fail_closed_protects_live_editor_buffer() {
        // A PROVEN live editor with failing delivery must NOT disk-clobber.
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 5, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
        // No editor proven yet but below threshold → conservative, don't clobber.
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 1, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
    }

    #[test]
    fn editorless_converges_via_healthy_editor() {
        // A live editor with healthy delivery converges normally.
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, false),
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        );
    }

    #[test]
    fn reconnect_buffer_in_sync_when_buffer_matches_disk() {
        // Even if the proof inputs look stale, an in-sync buffer is a no-op.
        assert_eq!(
            decide_reconnect_buffer(true, true, true),
            ReconnectBufferDecision::InSync
        );
        assert_eq!(
            decide_reconnect_buffer(true, false, false),
            ReconnectBufferDecision::InSync
        );
    }

    #[test]
    fn reconnect_buffer_rereads_provably_stale_committed_buffer() {
        // buffer != disk, disk is clean HEAD, buffer equals a prior commit
        // → the binary advanced disk while disconnected; HEAD wins.
        assert_eq!(
            decide_reconnect_buffer(false, true, true),
            ReconnectBufferDecision::RereadDisk
        );
    }

    #[test]
    fn reconnect_buffer_keeps_unproven_divergent_buffer() {
        // buffer diverges but is NOT a known prior commit → genuine user edits;
        // editor wins, never clobber.
        assert_eq!(
            decide_reconnect_buffer(false, true, false),
            ReconnectBufferDecision::KeepBuffer
        );
        // disk is not clean HEAD (uncommitted working-tree change) → don't reread
        // even if the buffer happens to match a prior commit.
        assert_eq!(
            decide_reconnect_buffer(false, false, true),
            ReconnectBufferDecision::KeepBuffer
        );
    }

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
        // A response that has real patch blocks must never be misread as the
        // raw-template form even if a component block also appears.
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
            raw_component_block_count("### Re: topic — gpt-5\n\nImplemented and verified.\n"),
            0
        );
        // A bare boundary marker has no close marker, so it is not a block.
        assert_eq!(
            raw_component_block_count("<!-- agent:boundary:abc123 -->\nsome text\n"),
            0
        );
        // A full component block is counted.
        assert_eq!(
            raw_component_block_count(
                "<!-- agent:exchange -->\n### Re: topic\nbody\n<!-- /agent:exchange -->\n"
            ),
            1
        );
    }

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
