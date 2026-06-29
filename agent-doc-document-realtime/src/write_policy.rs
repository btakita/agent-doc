//! Deterministic write/reconnect policy for realtime document mutations.
//!
//! The caller owns IO, editor IPC, git inspection, and flow logging. This
//! module owns only pure decisions about when a visible document mutation is
//! allowed to proceed.

use sha2::{Digest, Sha256};
use std::collections::HashSet;

fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
}

pub fn decide_visible_write_after_typing(facts: VisibleWriteTypingFacts) -> VisibleWriteDecision {
    let _timeout_ms = facts.timeout_ms;
    if facts.idle_reached {
        VisibleWriteDecision::Apply
    } else {
        VisibleWriteDecision::DeferActiveTyping
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullContentSourceProof {
    pub expected_content_hash: String,
    pub expected_content_len: usize,
}

impl FullContentSourceProof {
    pub fn from_content(content: &str) -> Self {
        Self {
            expected_content_hash: content_hash(content),
            expected_content_len: content.len(),
        }
    }

    pub fn matches_current(&self, current_content: &str) -> bool {
        current_content.len() == self.expected_content_len
            && content_hash(current_content) == self.expected_content_hash
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

/// Decision for reconciling an editor buffer against disk when the plugin
/// reconnects its IPC listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectBufferDecision {
    /// Buffer already matches disk; nothing to do.
    InSync,
    /// Buffer equals a prior commit of the file and disk is clean HEAD; re-read
    /// disk into the buffer.
    RereadDisk,
    /// Buffer diverges from disk but is not a known prior commit; keep it.
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

/// Re-read disk only when the buffer is provably stale committed content.
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

/// Decision for a finalize/converge write when an editor-IPC socket may be
/// absent, controller-hosted, or backed by a live editor endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorlessDiskFallbackDecision {
    /// A live editor endpoint is present or possible and delivery is unproven.
    FailClosed,
    /// No editor endpoint owns the document; a guarded direct disk write is
    /// allowed.
    DetachedDisk,
    /// Explicit operator force routes to controller-hosted disk write.
    ForceDiskNoEditor,
    /// A live editor endpoint is present and reachable; converge through it.
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
        return if consecutive_no_ack >= threshold && threshold > 0 {
            EditorlessDiskFallbackDecision::FailClosed
        } else {
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        };
    }
    if !socket_connectable || (threshold > 0 && consecutive_no_ack >= threshold) {
        return EditorlessDiskFallbackDecision::DetachedDisk;
    }
    EditorlessDiskFallbackDecision::FailClosed
}

const AGENT_RESPONSE_COMPONENT: &str = "exchange";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckMismatchRecovery {
    RevertUntrustedAckToCurrent,
    ReplayMissingAgentResponseToTarget,
}

fn blank_components_named(doc: &str, names: &[&str]) -> Option<String> {
    let comps = agent_doc_element::element::parse(doc).ok()?;
    let mut spans: Vec<(usize, usize)> = comps
        .iter()
        .filter(|c| names.contains(&c.name.as_str()))
        .map(|c| (c.open_end, c.close_start))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, "");
        }
    }
    Some(out)
}

fn missing_agent_response_block<'a>(target_body: &'a str, recovered_body: &str) -> Option<&'a str> {
    if target_body.len() <= recovered_body.len() {
        return None;
    }
    let missing = if let Some(missing) = target_body.strip_prefix(recovered_body) {
        missing
    } else if let Some(missing) = target_body.strip_suffix(recovered_body) {
        missing
    } else {
        let start = target_body.find(recovered_body)?;
        let end = start + recovered_body.len();
        let before = &target_body[..start];
        let after = &target_body[end..];
        if before.trim().is_empty() {
            after
        } else if after.trim().is_empty() {
            before
        } else {
            return None;
        }
    };
    let trimmed = missing.trim_start();
    if trimmed.starts_with("### Re:") || trimmed.contains("\n### Re:") {
        Some(missing)
    } else {
        None
    }
}

fn stale_queue_prompt_exchange_artifact(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('>') || trimmed == "❯ >"
}

pub fn classify_ack_mismatch_recovery(
    target: &str,
    recovered: &str,
    normalize_transient_markers: impl Fn(&str) -> String,
) -> Option<AckMismatchRecovery> {
    let (Some(target_without_exchange), Some(recovered_without_exchange)) = (
        blank_components_named(target, &[AGENT_RESPONSE_COMPONENT]),
        blank_components_named(recovered, &[AGENT_RESPONSE_COMPONENT]),
    ) else {
        return None;
    };
    if normalize_transient_markers(&target_without_exchange)
        != normalize_transient_markers(&recovered_without_exchange)
    {
        return None;
    }

    let (Ok(target_comps), Ok(recovered_comps)) = (
        agent_doc_element::element::parse(target),
        agent_doc_element::element::parse(recovered),
    ) else {
        return None;
    };
    let target_exchange = target_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let recovered_exchange = recovered_comps
        .iter()
        .find(|c| c.name == AGENT_RESPONSE_COMPONENT);
    let (Some(target_exchange), Some(recovered_exchange)) = (target_exchange, recovered_exchange)
    else {
        return None;
    };
    let target_body = normalize_transient_markers(target_exchange.content(target));
    let recovered_body = normalize_transient_markers(recovered_exchange.content(recovered));
    if target_body == recovered_body {
        return None;
    }
    if recovered_body.len() < target_body.len()
        && missing_agent_response_block(&target_body, &recovered_body).is_some()
    {
        return Some(AckMismatchRecovery::ReplayMissingAgentResponseToTarget);
    }
    let target_lines: HashSet<&str> = target_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let recovered_lines: HashSet<&str> = recovered_body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !target_lines
        .iter()
        .all(|line| recovered_lines.contains(line))
    {
        return None;
    }
    let recovered_only: Vec<&str> = recovered_lines.difference(&target_lines).copied().collect();
    if !recovered_only.is_empty()
        && recovered_only
            .iter()
            .all(|line| stale_queue_prompt_exchange_artifact(line))
        && recovered_only
            .iter()
            .any(|line| line.trim().starts_with("> **Queue prompt:**"))
    {
        return Some(AckMismatchRecovery::RevertUntrustedAckToCurrent);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_normalize(text: &str) -> String {
        text.to_string()
    }

    fn doc_with_exchange(exchange: &str, queue: &str) -> String {
        format!(
            "# Plan\n\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn editorless_detached_disk_after_no_delivery_or_no_listener() {
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 3, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        assert_eq!(
            decide_editorless_disk_fallback(false, false, 0, 3, false),
            EditorlessDiskFallbackDecision::DetachedDisk
        );
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, true),
            EditorlessDiskFallbackDecision::ForceDiskNoEditor
        );
    }

    #[test]
    fn editorless_fail_closed_protects_live_editor_buffer() {
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 5, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
        assert_eq!(
            decide_editorless_disk_fallback(true, false, 1, 3, false),
            EditorlessDiskFallbackDecision::FailClosed
        );
    }

    #[test]
    fn editorless_converges_via_healthy_editor() {
        assert_eq!(
            decide_editorless_disk_fallback(true, true, 0, 3, false),
            EditorlessDiskFallbackDecision::ConvergeViaEditor
        );
    }

    #[test]
    fn reconnect_buffer_in_sync_when_buffer_matches_disk() {
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
        assert_eq!(
            decide_reconnect_buffer(false, true, true),
            ReconnectBufferDecision::RereadDisk
        );
    }

    #[test]
    fn reconnect_buffer_keeps_unproven_divergent_buffer() {
        assert_eq!(
            decide_reconnect_buffer(false, true, false),
            ReconnectBufferDecision::KeepBuffer
        );
        assert_eq!(
            decide_reconnect_buffer(false, false, true),
            ReconnectBufferDecision::KeepBuffer
        );
    }

    #[test]
    fn visible_write_guard_defers_when_typing_never_settles() {
        let decision = decide_visible_write_after_typing(VisibleWriteTypingFacts {
            idle_reached: false,
            timeout_ms: 5_000,
        });

        assert_eq!(decision, VisibleWriteDecision::DeferActiveTyping);
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

        assert_eq!(
            decision,
            FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        );
    }

    #[test]
    fn ack_mismatch_classifies_stale_queue_prompt_artifact_as_revert() {
        let exchange = "### Re: do [#head]\n\nAnswered from the agent.\n";
        let target = doc_with_exchange(exchange, "- do [#head]\n");
        let recovered = doc_with_exchange(
            &format!("{exchange}> **Queue prompt:** stale leftover from failed queue consume\n"),
            "- do [#head]\n",
        );

        assert_eq!(
            classify_ack_mismatch_recovery(&target, &recovered, identity_normalize),
            Some(AckMismatchRecovery::RevertUntrustedAckToCurrent)
        );
    }

    #[test]
    fn ack_mismatch_classifies_missing_agent_response_as_target_replay() {
        let target = doc_with_exchange(
            "❯ do [#head]\n\n### Re: do [#head]\n\nAnswered from the agent.\n",
            "- do [#head]\n",
        );
        let recovered = doc_with_exchange("❯ do [#head]\n", "- do [#head]\n");

        assert_eq!(
            classify_ack_mismatch_recovery(&target, &recovered, identity_normalize),
            Some(AckMismatchRecovery::ReplayMissingAgentResponseToTarget)
        );
    }

    #[test]
    fn ack_mismatch_rejects_user_prompt_drift() {
        let exchange = "### Re: do [#head]\n\nAnswered from the agent.\n";
        let target = doc_with_exchange(exchange, "- do [#head]\n");
        let recovered =
            doc_with_exchange(&format!("{exchange}❯ do [#followup]\n"), "- do [#head]\n");

        assert_eq!(
            classify_ack_mismatch_recovery(&target, &recovered, identity_normalize),
            None
        );
    }
}
