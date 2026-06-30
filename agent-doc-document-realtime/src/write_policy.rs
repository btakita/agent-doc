//! Deterministic write/reconnect policy for realtime document mutations.
//!
//! The caller owns IO, editor IPC, git inspection, and flow logging. This
//! module owns only pure decisions about when a visible document mutation is
//! allowed to proceed.

use agent_doc_document::commit_normalization::{
    normalize_component_content_for_absorb, redact_component_contents_for_absorb,
};
use agent_doc_document::transient_markers::strip_boundary_markers;
use agent_doc_prompt_lines::text_line_looks_like_prompt_target;
use agent_doc_queue::queue_prompt_drift::queue_prompt_deletions_between;
use agent_doc_turn::closeout_signal::line_is_carry_forward_signal;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPersistMode {
    FinalContent,
    ContentOurs,
}

pub fn snapshot_persist_mode(
    baseline: Option<&str>,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_none() {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_norm = strip_boundary_markers(content_ours);
    let final_norm = strip_boundary_markers(final_content);
    if ours_norm == final_norm {
        return SnapshotPersistMode::FinalContent;
    }

    if agent_doc_turn::document_drift::detect_bypassed_response_write_between(
        &ours_norm,
        &final_norm,
    )
    .is_some()
    {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_prompt_norm = agent_doc_diff::strip_comments(&ours_norm);
    let final_prompt_norm = agent_doc_diff::strip_comments(&final_norm);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&ours_prompt_norm, &final_prompt_norm)
    else {
        return SnapshotPersistMode::FinalContent;
    };
    let has_prompt_bearing_user_drift = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .iter()
        .any(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        });

    if has_prompt_bearing_user_drift {
        SnapshotPersistMode::ContentOurs
    } else {
        SnapshotPersistMode::FinalContent
    }
}

pub fn snapshot_persist_mode_with_current(
    baseline: Option<&str>,
    base: &str,
    content_current: &str,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_some()
        && strip_boundary_markers(base) != strip_boundary_markers(content_current)
        && (has_prompt_bearing_user_drift(base, content_current)
            || non_exchange_drift_carries_directive(base, content_current))
    {
        return SnapshotPersistMode::ContentOurs;
    }

    snapshot_persist_mode(baseline, content_ours, final_content)
}

pub fn snapshot_content_to_persist<'a>(
    mode: SnapshotPersistMode,
    content_ours: &'a str,
    final_content: &'a str,
) -> &'a str {
    match mode {
        SnapshotPersistMode::FinalContent => final_content,
        SnapshotPersistMode::ContentOurs => content_ours,
    }
}

fn non_exchange_drift_carries_directive(base: &str, current: &str) -> bool {
    let base_norm = strip_boundary_markers(base);
    let current_norm = strip_boundary_markers(current);
    if base_norm == current_norm {
        return false;
    }
    if !outside_component_content_changed(&base_norm, &current_norm, "exchange") {
        return false;
    }
    added_nonblank_lines(&base_norm, &current_norm)
        .iter()
        .any(|line| line_is_carry_forward_signal(line))
}

fn outside_component_content_changed(left: &str, right: &str, component_name: &str) -> bool {
    let left_component = match agent_doc_element::element::parse(left) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };
    let right_component = match agent_doc_element::element::parse(right) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };

    let Some(left_component) = left_component else {
        return left != right;
    };
    let Some(right_component) = right_component else {
        return true;
    };

    left[..left_component.open_end] != right[..right_component.open_end]
        || left[left_component.close_start..] != right[right_component.close_start..]
}

fn has_prompt_bearing_user_drift(base: &str, current: &str) -> bool {
    !prompt_bearing_user_changes_between(base, current).is_empty()
}

pub fn prompt_bearing_user_changes_between(
    base: &str,
    current: &str,
) -> Vec<agent_doc_diff::PromptBearingChange> {
    let base_norm = strip_boundary_markers(base);
    let current_norm = strip_boundary_markers(current);
    let base_prompt_norm = agent_doc_diff::strip_comments(&base_norm);
    let current_prompt_norm = agent_doc_diff::strip_comments(&current_norm);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&base_prompt_norm, &current_prompt_norm)
    else {
        return Vec::new();
    };
    let mut changes: Vec<_> = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if line.starts_with("+++") {
            return false;
        }
        let trimmed = added.trim();
        trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed)
    }) {
        for line in diff_text.lines() {
            let Some(added) = line.strip_prefix('+') else {
                continue;
            };
            if line.starts_with("+++") {
                continue;
            }
            let trimmed = added.trim();
            if trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed) {
                let text = trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                if !changes.iter().any(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                        && change.text.trim() == text
                }) {
                    changes.push(agent_doc_diff::PromptBearingChange {
                        kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                        text,
                    });
                }
            }
        }
    }
    changes
}

fn prompt_bearing_change_owned_by_content_ours(
    change: &agent_doc_diff::PromptBearingChange,
    owned_changes: &[agent_doc_diff::PromptBearingChange],
) -> bool {
    let text = normalized_prompt_line(&change.text);
    owned_changes
        .iter()
        .any(|owned| owned.kind == change.kind && normalized_prompt_line(&owned.text) == text)
}

pub fn ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
    baseline: &str,
    snapshot_candidate: &str,
    content_ours: &str,
) -> bool {
    let baseline_norm = strip_boundary_markers(baseline);
    let candidate_norm = strip_boundary_markers(snapshot_candidate);
    let ours_norm = strip_boundary_markers(content_ours);
    if outside_component_content_changed(&baseline_norm, &candidate_norm, "exchange")
        && outside_component_content_changed(&ours_norm, &candidate_norm, "exchange")
    {
        return true;
    }

    let candidate_changes = prompt_bearing_user_changes_between(baseline, snapshot_candidate);
    if candidate_changes.is_empty() {
        return false;
    }
    let owned_changes = prompt_bearing_user_changes_between(baseline, content_ours);
    candidate_changes
        .iter()
        .any(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
}

fn added_nonblank_lines(baseline: &str, candidate: &str) -> Vec<String> {
    let base: HashSet<&str> = baseline
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    candidate
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !base.contains(line))
        .map(|line| line.to_string())
        .collect()
}

pub fn response_target_disjoint_from_user_edit(
    baseline: &str,
    content_ours: &str,
    candidate: &str,
    merge_contents: impl FnOnce(&str, &str, &str) -> Option<String>,
) -> bool {
    if strip_boundary_markers(candidate) == strip_boundary_markers(content_ours) {
        return false;
    }
    let user_added = added_nonblank_lines(baseline, candidate);
    if user_added.is_empty() {
        return false;
    }
    if !queue_prompt_deletions_between(baseline, candidate).is_empty() {
        return false;
    }

    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let ours_ex = exchange_component_text(content_ours);
    let response_ex_added: HashSet<String> = added_nonblank_lines(&baseline_ex, &ours_ex)
        .into_iter()
        .collect();
    let user_ex_added = added_nonblank_lines(&baseline_ex, &candidate_ex)
        .into_iter()
        .any(|line| !response_ex_added.contains(&line));
    if user_ex_added {
        return false;
    }

    let response_added_set: HashSet<String> = added_nonblank_lines(baseline, content_ours)
        .into_iter()
        .collect();
    let user_carries_directive = user_added
        .iter()
        .filter(|line| !response_added_set.contains(*line))
        .any(|line| line_is_carry_forward_signal(line));
    if user_carries_directive {
        return false;
    }

    let Some(merged) = merge_contents(baseline, content_ours, candidate) else {
        return false;
    };
    if merged.contains("<<<<<<<") || merged.contains(">>>>>>>") {
        return false;
    }
    let merged_lines: HashSet<&str> = merged
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let response_added = added_nonblank_lines(baseline, content_ours);
    response_added
        .iter()
        .all(|line| merged_lines.contains(line.as_str()))
        && user_added
            .iter()
            .all(|line| merged_lines.contains(line.as_str()))
}

pub fn exchange_component_text(doc: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return String::new();
    };
    components
        .iter()
        .find(|component| component.name == "exchange")
        .map(|component| component.content(doc).to_string())
        .unwrap_or_default()
}

pub fn dropped_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let content_ours_ex = exchange_component_text(content_ours);

    let candidate_changes = prompt_bearing_user_changes_between(&baseline_ex, &candidate_ex);
    if candidate_changes.is_empty() {
        return Vec::new();
    }
    let owned_changes = prompt_bearing_user_changes_between(&baseline_ex, &content_ours_ex);
    candidate_changes
        .into_iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
        .filter(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
        .map(|change| change.text.trim().to_string())
        .filter(|text| !text.is_empty() && !text.contains('\n'))
        .collect()
}

fn normalized_prompt_line(line: &str) -> String {
    line.trim()
        .strip_prefix('❯')
        .unwrap_or_else(|| line.trim())
        .trim()
        .to_string()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullContentScopeRejection {
    TemplateFrontmatter,
    AgentComponentMarkers,
}

impl FullContentScopeRejection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TemplateFrontmatter => "template_frontmatter",
            Self::AgentComponentMarkers => "agent_component_markers",
        }
    }
}

fn frontmatter_mode_is_explicit_template(mode: &str) -> bool {
    matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "template" | "stream"
    )
}

fn content_declares_template_frontmatter(content: &str) -> bool {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .is_some_and(|(fm, _)| {
            fm.format == Some(agent_doc_frontmatter::frontmatter::AgentDocFormat::Template)
                || fm
                    .mode
                    .as_deref()
                    .is_some_and(frontmatter_mode_is_explicit_template)
        })
}

fn content_has_agent_components(content: &str) -> bool {
    agent_doc_element::element::parse(content)
        .ok()
        .is_some_and(|components| !components.is_empty())
}

pub fn full_content_scope_rejection_reason(
    contents: &[Option<&str>],
) -> Option<FullContentScopeRejection> {
    for content in contents.iter().flatten() {
        if content_declares_template_frontmatter(content) {
            return Some(FullContentScopeRejection::TemplateFrontmatter);
        }
        if content_has_agent_components(content) {
            return Some(FullContentScopeRejection::AgentComponentMarkers);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferDelivery {
    FullContentEditorIpc,
    AckContentDiskWriteThrough,
    EditorRepairRedelivery,
}

impl WholeBufferDelivery {
    const fn requires_source_buffer_match(self) -> bool {
        matches!(
            self,
            Self::FullContentEditorIpc
                | Self::AckContentDiskWriteThrough
                | Self::EditorRepairRedelivery
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullContentEditorIpc => "full_content_editor_ipc",
            Self::AckContentDiskWriteThrough => "ack_content_disk_write_through",
            Self::EditorRepairRedelivery => "editor_repair_redelivery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferAuthority {
    OperatorTextAuthority,
    AckContentSidecar,
    FileRead,
    ContentOurs,
    None,
}

impl WholeBufferAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperatorTextAuthority => "operator_text_authority",
            Self::AckContentSidecar => "ack_content_sidecar",
            Self::FileRead => "file_read",
            Self::ContentOurs => "content_ours",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeBufferDeliveryAction {
    Apply,
    ObserveOnly,
    Reject,
}

impl WholeBufferDeliveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::ObserveOnly => "observe_only",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WholeBufferAuthorityFacts {
    pub delivery: WholeBufferDelivery,
    pub authority: WholeBufferAuthority,
    pub source_buffer_matches: bool,
    pub scope_rejection: Option<FullContentScopeRejection>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WholeBufferAuthorityDecision {
    pub action: WholeBufferDeliveryAction,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WholeBufferAuthorityRule {
    delivery: WholeBufferDelivery,
    authority: WholeBufferAuthority,
    action: WholeBufferDeliveryAction,
    reason: &'static str,
}

const WHOLE_BUFFER_AUTHORITY_TABLE: &[WholeBufferAuthorityRule] = &[
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::FullContentEditorIpc,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority_source_buffer",
    },
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::AckContentDiskWriteThrough,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority",
    },
    WholeBufferAuthorityRule {
        delivery: WholeBufferDelivery::EditorRepairRedelivery,
        authority: WholeBufferAuthority::OperatorTextAuthority,
        action: WholeBufferDeliveryAction::Apply,
        reason: "operator_text_authority_source_buffer",
    },
];

pub fn decide_whole_buffer_delivery(
    facts: WholeBufferAuthorityFacts,
) -> WholeBufferAuthorityDecision {
    if let Some(scope_rejection) = facts.scope_rejection {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: scope_rejection.as_str(),
        };
    }

    if facts.delivery.requires_source_buffer_match() && !facts.source_buffer_matches {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: "stale_source_buffer",
        };
    }

    if facts.delivery == WholeBufferDelivery::FullContentEditorIpc && !facts.enabled {
        return WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::ObserveOnly,
            reason: "disabled_by_default",
        };
    }

    WHOLE_BUFFER_AUTHORITY_TABLE
        .iter()
        .find(|rule| rule.delivery == facts.delivery && rule.authority == facts.authority)
        .map(|rule| WholeBufferAuthorityDecision {
            action: rule.action,
            reason: rule.reason,
        })
        .unwrap_or(WholeBufferAuthorityDecision {
            action: WholeBufferDeliveryAction::Reject,
            reason: "missing_operator_text_authority",
        })
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

pub fn exchange_change_is_complete_response_block_trim(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let blocks = exchange_response_block_ranges(snapshot);
    if blocks.is_empty() {
        return false;
    }

    let mut snapshot_pos = 0usize;
    let mut current_pos = 0usize;
    let mut removed = 0usize;
    for block in blocks {
        let prefix = &snapshot[snapshot_pos..block.start];
        if !current[current_pos..].starts_with(prefix) {
            return false;
        }
        current_pos += prefix.len();

        let block_text = &snapshot[block.clone()];
        if current[current_pos..].starts_with(block_text) {
            current_pos += block_text.len();
        } else {
            removed += 1;
        }
        snapshot_pos = block.end;
    }

    removed > 0 && current[current_pos..] == snapshot[snapshot_pos..]
}

pub fn exchange_change_is_safe_historical_reduction(snapshot: &str, current: &str) -> bool {
    exchange_change_is_complete_response_block_trim(snapshot, current)
        || exchange_change_is_compact_summary_replacement(snapshot, current)
}

pub fn exchange_change_is_compact_summary_replacement(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return false;
    }
    let current_trimmed = current.trim_start();
    if !current_trimmed.starts_with("### Session Summary") {
        return false;
    }
    if !current.contains("*Compacted. Content archived to `")
        && !current.contains("Compacted content:")
    {
        return false;
    }

    let snapshot_headings = exchange_response_heading_lines(snapshot);
    if snapshot_headings.is_empty() {
        return false;
    }
    let current_headings = exchange_response_heading_lines(current);
    if current_headings
        .iter()
        .any(|heading| !snapshot_headings.contains(heading))
    {
        return false;
    }

    current_headings.len() < snapshot_headings.len()
}

pub fn exchange_response_heading_lines(exchange: &str) -> Vec<String> {
    exchange
        .lines()
        .filter(|line| is_exchange_response_heading(line))
        .map(|line| line.trim().to_string())
        .collect()
}

pub fn exchange_response_block_ranges(exchange: &str) -> Vec<std::ops::Range<usize>> {
    #[derive(Clone, Copy)]
    struct Line<'a> {
        start: usize,
        end: usize,
        text: &'a str,
    }

    let mut lines = Vec::new();
    let mut offset = 0usize;
    for line in exchange.split_inclusive('\n') {
        let end = offset + line.len();
        lines.push(Line {
            start: offset,
            end,
            text: line,
        });
        offset = end;
    }
    if offset < exchange.len() {
        lines.push(Line {
            start: offset,
            end: exchange.len(),
            text: &exchange[offset..],
        });
    }

    let heading_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_exchange_response_heading(line.text).then_some(idx))
        .collect();
    let mut ranges = Vec::new();
    for (pos, &heading_idx) in heading_indices.iter().enumerate() {
        let mut end_idx = heading_indices.get(pos + 1).copied().unwrap_or(lines.len());
        for (idx, line) in lines.iter().enumerate().take(end_idx).skip(heading_idx + 1) {
            if is_exchange_boundary(line.text) {
                end_idx = idx;
                break;
            }
        }
        ranges.push(lines[heading_idx].start..lines[end_idx - 1].end);
    }
    ranges
}

fn is_exchange_response_heading(line: &str) -> bool {
    line.trim_start().starts_with("### Re:")
}

fn is_exchange_boundary(line: &str) -> bool {
    line.trim_start().starts_with("<!-- agent:boundary:")
}

/// `#exch-intermix`: realtime resolver for the `live_prompt_drift_after_preflight`
/// closeout wedge. After the IPC drift guard carries the agent response in the
/// snapshot candidate, the visible document may still be missing that response
/// while carrying newer operator-visible edits. Recovery must rebase only the
/// missing response block onto the current document; it must not adopt the
/// snapshot as a whole-document authority.
///
/// This returns true only when the current realtime document can preserve the
/// operator-visible state and accept the missing agent response as a delta. It
/// never authorizes wholesale snapshot adoption: queue/backlog/frontmatter and
/// other disjoint operator edits stay as they are in `file_content`, while only
/// the newest missing `### Re:` block from `snapshot` may be appended to
/// `agent:exchange`. Prompt-target edits inside the visible file still fail
/// closed because the resolver cannot prove where the response should land
/// relative to a newly typed prompt.
pub fn live_prompt_drift_auto_recovery_safe(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> bool {
    live_prompt_drift_recovery_target(snapshot, file_content, normalize_visible_recovery_compare)
        .is_some()
}

pub fn live_prompt_drift_recovery_target(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> Option<String> {
    // A newly typed prompt inside `agent:exchange` makes response placement
    // ambiguous. Queue/backlog prompt text is disjoint operator state and is
    // preserved by the merged target below.
    if exchange_has_disk_only_prompt_target(snapshot, file_content) {
        return None;
    }

    let response_block = latest_missing_snapshot_response_block(
        snapshot,
        file_content,
        normalize_visible_recovery_compare,
    )?;
    let components = agent_doc_element::element::parse(file_content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == AGENT_RESPONSE_COMPONENT)?;
    let mut exchange_body = exchange.content(file_content).to_string();
    agent_doc_template::response_materialization::push_materialization_segment(
        &mut exchange_body,
        &response_block,
    );
    let recovered = exchange.replace_content(file_content, &exchange_body);
    (normalize_visible_recovery_compare(&recovered)
        != normalize_visible_recovery_compare(file_content))
    .then_some(recovered)
}

fn exchange_has_disk_only_prompt_target(snapshot: &str, file_content: &str) -> bool {
    let (Ok(snapshot_components), Ok(file_components)) = (
        agent_doc_element::element::parse(snapshot),
        agent_doc_element::element::parse(file_content),
    ) else {
        return true;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return true;
    };
    let snapshot_counts = exchange_prompt_target_counts(snapshot_exchange.content(snapshot));
    let mut seen: HashMap<String, usize> = HashMap::new();
    for prompt in exchange_prompt_target_lines(file_exchange.content(file_content)) {
        let count = seen.entry(prompt.clone()).or_insert(0);
        *count += 1;
        if *count > snapshot_counts.get(&prompt).copied().unwrap_or(0) {
            return true;
        }
    }
    false
}

fn exchange_prompt_target_counts(exchange_body: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for prompt in exchange_prompt_target_lines(exchange_body) {
        *counts.entry(prompt).or_insert(0) += 1;
    }
    counts
}

fn exchange_prompt_target_lines(exchange_body: &str) -> Vec<String> {
    exchange_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('❯') || text_line_looks_like_prompt_target(trimmed) {
                Some(
                    trimmed
                        .strip_prefix('❯')
                        .unwrap_or(trimmed)
                        .trim()
                        .to_string(),
                )
            } else {
                None
            }
        })
        .collect()
}

fn latest_missing_snapshot_response_block(
    snapshot: &str,
    file_content: &str,
    normalize_visible_recovery_compare: impl Fn(&str) -> String + Copy,
) -> Option<String> {
    let (Ok(snapshot_components), Ok(file_components)) = (
        agent_doc_element::element::parse(snapshot),
        agent_doc_element::element::parse(file_content),
    ) else {
        return None;
    };
    let (Some(snapshot_exchange), Some(file_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        file_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return None;
    };
    let snapshot_body = snapshot_exchange.content(snapshot);
    let file_body = file_exchange.content(file_content);
    let file_norm = normalize_visible_recovery_compare(file_body);
    for range in exchange_response_block_ranges(snapshot_body)
        .into_iter()
        .rev()
    {
        let block = &snapshot_body[range];
        let block_norm = normalize_visible_recovery_compare(block);
        let block_trimmed = block_norm.trim();
        if block_trimmed.is_empty() {
            continue;
        }
        if !file_norm.contains(block_trimmed) {
            return Some(block.to_string());
        }
    }
    None
}

/// `#exch-intermix-falsedrop`: true when a recorded dropped prompt is still
/// present in the response candidate - as an active line, a
/// struck/consumed queue item (`~~...~~`), or echoed in a `### Re:` heading - so
/// response recovery loses nothing. The drift-time dropped-prompt record
/// compares the divergent IPC candidate against `content_ours` and therefore
/// false-positives on prompts that `content_ours` consumed or preserved; this
/// containment check reconciles those against the response candidate text.
/// Returns false only when the prompt text genuinely does not appear in the
/// candidate (real user-content loss -> fail closed). Strike markers are
/// stripped from both sides so a consumed item still matches its recorded prompt
/// text.
pub fn snapshot_contains_dropped_prompt(snapshot: &str, prompt: &str) -> bool {
    let stripped = prompt.replace("~~", "");
    let needle = stripped.trim();
    if needle.is_empty() {
        return true;
    }
    snapshot.replace("~~", "").contains(needle)
}

fn is_safe_out_of_band_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    if !file_content.starts_with(snapshot_content) {
        return false;
    }
    let suffix = file_content[snapshot_content.len()..].trim();
    !suffix.is_empty() && suffix.starts_with("### Re:")
}

fn is_safe_exchange_user_prompt_insert(snapshot_exchange: &str, file_exchange: &str) -> bool {
    let snap_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let file_lines: Vec<&str> = file_exchange.lines().collect();

    if snap_lines.len() >= file_lines.len() {
        return false;
    }

    let prefix_len = snap_lines
        .iter()
        .zip(file_lines.iter())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    let suffix_len = snap_lines
        .iter()
        .rev()
        .zip(file_lines.iter().rev())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    if suffix_len == 0 {
        return false;
    }

    let suffix_start_in_snap = snap_lines.len().saturating_sub(suffix_len);
    let suffix_has_response = snap_lines[suffix_start_in_snap..]
        .iter()
        .any(|line| line.trim().starts_with("### Re:"));

    if !suffix_has_response {
        return false;
    }

    let insert_start = prefix_len;
    let insert_end = file_lines.len().saturating_sub(suffix_len);

    if insert_start >= insert_end {
        return false;
    }

    let inserted_lines = &file_lines[insert_start..insert_end];

    for line in inserted_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
        {
            return false;
        }
    }

    true
}

fn flush_exchange_insert_block(block: &mut String) -> bool {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        block.clear();
        return true;
    }
    let ok = is_safe_historical_exchange_insert_block(trimmed);
    block.clear();
    ok
}

fn is_safe_historical_exchange_insert_block(block: &str) -> bool {
    let non_blank: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_blank.is_empty() {
        return true;
    }

    let Some(first_response_idx) = non_blank.iter().position(|line| {
        line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("##### Re:")
    }) else {
        return false;
    };
    if first_response_idx == 0 {
        return true;
    }

    non_blank[..first_response_idx]
        .iter()
        .all(|line| historical_exchange_prelude_looks_like_prompt_target(line))
}

fn historical_exchange_prelude_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !trimmed.starts_with("#### Re:")
        && !trimmed.starts_with("##### Re:")
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || historical_exchange_prelude_looks_like_imperative(trimmed))
}

fn historical_exchange_prelude_looks_like_imperative(line: &str) -> bool {
    let compact = line.trim_start_matches('>').trim().to_ascii_lowercase();
    compact == "go"
        || compact == "continue"
        || compact.starts_with("do #")
        || compact.starts_with("run ")
        || compact.starts_with("rerun ")
        || compact.starts_with("build ")
        || compact.starts_with("test ")
        || compact.starts_with("commit ")
        || compact.starts_with("push ")
        || compact.starts_with("fix ")
        || compact.starts_with("complete ")
}

fn is_safe_historical_exchange_growth(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut insert_block = String::new();
    let mut saw_insert = false;

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                if !flush_exchange_insert_block(&mut insert_block) {
                    return false;
                }
            }
            similar::ChangeTag::Delete => return false,
            similar::ChangeTag::Insert => {
                saw_insert = true;
                insert_block.push_str(change.value());
            }
        }
    }

    saw_insert && flush_exchange_insert_block(&mut insert_block)
}

pub fn is_safe_user_follow_up_exchange_growth(head_content: &str, current_content: &str) -> bool {
    if head_content == current_content || !current_content.starts_with(head_content) {
        return false;
    }

    let suffix = current_content[head_content.len()..].trim();
    !suffix.is_empty()
        && suffix != "## Assistant"
        && !suffix.starts_with("### Re:")
        && !suffix.starts_with("#### Re:")
}

fn is_safe_out_of_band_pending_mutation(snapshot_content: &str, file_content: &str) -> bool {
    let (snap_prelude, snap_items, snap_postlude) =
        agent_doc_element_backlog::backlog::parse_items(snapshot_content);
    let (file_prelude, file_items, file_postlude) =
        agent_doc_element_backlog::backlog::parse_items(file_content);

    if snap_prelude.trim() != file_prelude.trim() || snap_postlude.trim() != file_postlude.trim() {
        return false;
    }
    if file_items.is_empty() {
        return false;
    }

    let file_ids: HashSet<&str> = file_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.as_str())
        .collect();
    if file_ids.is_empty() {
        return false;
    }

    snap_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .all(|item| file_ids.contains(item.id.as_str()))
}

pub fn detect_reintroduced_reaped_pending_ids(
    doc: &str,
    reaped_ids: &HashSet<String>,
) -> Result<Vec<String>> {
    if reaped_ids.is_empty() {
        return Ok(Vec::new());
    }

    let components = agent_doc_element::element::parse(doc)?;
    let mut seen = HashSet::new();
    let mut reintroduced = Vec::new();
    for component in components
        .iter()
        .filter(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
    {
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(component.content(doc));
        for item in items {
            if !item.id.is_empty() && reaped_ids.contains(&item.id) && seen.insert(item.id.clone())
            {
                reintroduced.push(item.id);
            }
        }
    }

    reintroduced.sort();
    Ok(reintroduced)
}

fn strip_promptish_list_prefix(line: &str) -> &str {
    let mut trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }

    trimmed
}

fn starts_with_prompt_preset_reference(line: &str) -> bool {
    let trimmed = strip_promptish_list_prefix(line);
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let token_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if token_len == 0 {
        return false;
    }
    let remainder = &rest[token_len..];
    remainder.is_empty()
        || remainder
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
}

fn status_mutation_introduces_prompt_work(snapshot_content: &str, file_content: &str) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut added = String::new();

    for change in diff.iter_all_changes() {
        if change.tag() == similar::ChangeTag::Insert {
            added.push_str(change.value());
        }
    }

    if added.trim().is_empty() {
        return false;
    }

    if !agent_doc_diff::extract_prompt_preset_requests_from_text(&added).is_empty() {
        return true;
    }

    added.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && (text_line_looks_like_prompt_target(trimmed)
                || starts_with_prompt_preset_reference(trimmed))
    })
}

fn is_safe_out_of_band_status_mutation(snapshot_content: &str, file_content: &str) -> bool {
    snapshot_content.trim() != file_content.trim()
        && !status_mutation_introduces_prompt_work(snapshot_content, file_content)
}

pub fn is_empty_template_scaffold_snapshot(snapshot_doc: &str) -> bool {
    let body = agent_doc_frontmatter::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let Ok(components) = agent_doc_element::element::parse(body) else {
        return false;
    };

    let has_status = components.iter().any(|c| c.name == "status");
    let has_exchange = components.iter().any(|c| c.name == "exchange");
    let has_pending = components
        .iter()
        .any(|c| agent_doc_element::element::is_backlog_component(&c.name));
    if !(has_status && has_exchange && has_pending) {
        return false;
    }

    components.iter().all(|component| {
        (matches!(component.name.as_str(), "status" | "exchange" | "queue")
            || agent_doc_element::element::is_backlog_component(&component.name))
            && normalize_component_content_for_absorb(component.content(body)).is_empty()
    })
}

fn classify_safe_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
    allow_historical_exchange_growth: bool,
) -> Option<&'static str> {
    if snapshot_doc == file_doc {
        return None;
    }

    let snap_body = agent_doc_frontmatter::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let file_body = agent_doc_frontmatter::frontmatter::parse(file_doc)
        .map(|(_, body)| body)
        .unwrap_or(file_doc);

    if redact_component_contents_for_absorb(snap_body)?
        != redact_component_contents_for_absorb(file_body)?
    {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snap_body).ok()?;
    let file_components = agent_doc_element::element::parse(file_body).ok()?;
    if snap_components.len() != file_components.len() {
        return None;
    }

    let mut saw_exchange = false;
    let mut saw_pending = false;
    let mut saw_status = false;

    for (snap_comp, file_comp) in snap_components.iter().zip(file_components.iter()) {
        if snap_comp.name != file_comp.name {
            return None;
        }
        if !agent_doc_element::element::is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != file_comp.patch_mode()
        {
            return None;
        }

        let snap_content = normalize_component_content_for_absorb(snap_comp.content(snap_body));
        let file_content = normalize_component_content_for_absorb(file_comp.content(file_body));
        if snap_content == file_content {
            continue;
        }

        match snap_comp.name.as_str() {
            "exchange" => {
                let safe_exchange =
                    is_safe_out_of_band_exchange_growth(&snap_content, &file_content)
                        || (allow_historical_exchange_growth
                            && is_safe_historical_exchange_growth(&snap_content, &file_content))
                        || is_safe_exchange_user_prompt_insert(&snap_content, &file_content);
                if !safe_exchange {
                    return None;
                }
                saw_exchange = true;
            }
            name if agent_doc_element::element::is_backlog_component(name) => {
                if !is_safe_out_of_band_pending_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_pending = true;
            }
            "status" => {
                if !is_safe_out_of_band_status_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_status = true;
            }
            _ => return None,
        }
    }

    match (saw_status, saw_exchange, saw_pending) {
        (true, true, true) => Some("status+exchange+pending"),
        (true, true, false) => Some("status+exchange"),
        (true, false, true) => Some("status+pending"),
        (true, false, false) => Some("status"),
        (false, true, true) => Some("exchange+pending"),
        (false, true, false) => Some("exchange"),
        (false, false, true) => Some("pending"),
        (false, false, false) => None,
    }
}

pub fn classify_safe_out_of_band_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, false)
}

pub fn classify_committed_historical_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, true)
}

/// Length evidence for suspicious stale snapshot reset drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleSnapshotResetDrift {
    pub snapshot_len: usize,
    pub current_len: usize,
}

/// Minimum size delta before stale snapshot reset drift is considered dangerous.
const STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES: usize = 100;

/// Maximum current/snapshot size ratio for reset-drift detection.
const STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO: f64 = 0.90;

pub fn stale_snapshot_reset_drift(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<StaleSnapshotResetDrift> {
    let snapshot_clean = strip_boundary_markers(snapshot_doc);
    let current_clean = strip_boundary_markers(current_doc);
    let snapshot_len = snapshot_clean.len();
    let current_len = current_clean.len();

    if snapshot_len <= current_len + STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES {
        return None;
    }
    if current_len as f64 / snapshot_len as f64 >= STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO {
        return None;
    }
    if classify_safe_out_of_band_agent_doc_mutation(&snapshot_clean, &current_clean).is_some() {
        return None;
    }

    Some(StaleSnapshotResetDrift {
        snapshot_len,
        current_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_normalize(text: &str) -> String {
        text.to_string()
    }

    fn drift_baseline() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn drift_content_ours() -> String {
        concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "### Re: do #fix — opus-4-8\n\n",
            "Implemented the fix and verified it end to end. The response body is long\n",
            "enough to clear the stale-snapshot-reset-drift threshold so the wedge shape\n",
            "is genuinely detected by the recovery discriminator under test here.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
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
    fn full_content_scope_rejects_template_frontmatter() {
        let template_format = "---\nagent_doc_format: template\n---\nplain\n";
        let stream_mode = "---\nagent_doc_mode: stream\n---\nplain\n";

        assert_eq!(
            full_content_scope_rejection_reason(&[Some(template_format)]),
            Some(FullContentScopeRejection::TemplateFrontmatter)
        );
        assert_eq!(
            full_content_scope_rejection_reason(&[Some(stream_mode)]),
            Some(FullContentScopeRejection::TemplateFrontmatter)
        );
    }

    #[test]
    fn full_content_scope_rejects_agent_component_markers() {
        let target = "plain\n";
        let source = "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n";

        assert_eq!(
            full_content_scope_rejection_reason(&[Some(target), Some(source), None]),
            Some(FullContentScopeRejection::AgentComponentMarkers)
        );
    }

    #[test]
    fn full_content_scope_allows_plain_documents() {
        assert_eq!(
            full_content_scope_rejection_reason(&[Some("plain\n"), None, Some("other\n")]),
            None
        );
    }

    #[test]
    fn whole_buffer_table_observes_disabled_full_content() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::FullContentEditorIpc,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: false,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::ObserveOnly);
        assert_eq!(decision.reason, "disabled_by_default");
    }

    #[test]
    fn whole_buffer_table_rejects_stale_source_before_authority() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::FullContentEditorIpc,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: false,
            scope_rejection: None,
            enabled: false,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(decision.reason, "stale_source_buffer");
    }

    #[test]
    fn whole_buffer_table_allows_ack_write_through_only_with_operator_authority() {
        let allowed = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::AckContentDiskWriteThrough,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: true,
        });
        let blocked = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::AckContentDiskWriteThrough,
            authority: WholeBufferAuthority::AckContentSidecar,
            source_buffer_matches: true,
            scope_rejection: None,
            enabled: true,
        });

        assert_eq!(allowed.action, WholeBufferDeliveryAction::Apply);
        assert_eq!(blocked.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(blocked.reason, "missing_operator_text_authority");
    }

    #[test]
    fn whole_buffer_table_rejects_ack_write_through_stale_source() {
        let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
            delivery: WholeBufferDelivery::AckContentDiskWriteThrough,
            authority: WholeBufferAuthority::OperatorTextAuthority,
            source_buffer_matches: false,
            scope_rejection: None,
            enabled: true,
        });

        assert_eq!(decision.action, WholeBufferDeliveryAction::Reject);
        assert_eq!(decision.reason, "stale_source_buffer");
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

    #[test]
    fn exchange_change_is_safe_historical_reduction_accepts_response_block_trim() {
        let snapshot = concat!(
            "❯ do [#old]\n",
            "### Re: do [#old]\n\nOld response body.\n",
            "❯ do [#new]\n",
            "### Re: do [#new]\n\nNew response body.\n",
        );
        let current = concat!(
            "❯ do [#old]\n",
            "### Re: do [#old]\n\nOld response body.\n",
            "❯ do [#new]\n",
        );

        assert!(exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn exchange_change_is_safe_historical_reduction_accepts_compact_summary_replacement() {
        let snapshot = concat!(
            "### Re: archived 0 - gpt-5\n\nArchived response body.\n",
            "### Re: archived 1 - gpt-5\n\nArchived response body.\n",
        );
        let current = concat!(
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n",
            "Compacted content:\n",
            "- Archived 2 response topic(s): archived 0; archived 1\n",
        );

        assert!(exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn exchange_change_is_safe_historical_reduction_rejects_unproven_rewrite() {
        let snapshot = "### Re: archived 0 - gpt-5\n\nArchived response body.\n";
        let current =
            "### Session Summary\n\nOperator-authored replacement without compact archive proof.\n";

        assert!(!exchange_change_is_safe_historical_reduction(
            snapshot, current
        ));
    }

    #[test]
    fn stale_snapshot_reset_drift_detects_unsafe_large_shrink() {
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let drift = stale_snapshot_reset_drift(&snapshot, current)
            .expect("large unsafe shrink should be classified as stale reset drift");

        assert!(drift.snapshot_len > drift.current_len + 100);
    }

    #[test]
    fn stale_snapshot_reset_drift_ignores_small_delta() {
        assert_eq!(
            stale_snapshot_reset_drift(&"a".repeat(1000), &"b".repeat(940)),
            None
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_ignores_safe_status_mutation() {
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:status patch=replace -->\n{}<!-- /agent:status -->\n\n<!-- agent:exchange patch=append -->\n### Re: older\n\nold body\n<!-- /agent:exchange -->\n",
            "Verbose status line.\n".repeat(20)
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:status patch=replace -->\nDone.\n<!-- /agent:status -->\n\n<!-- agent:exchange patch=append -->\n### Re: older\n\nold body\n<!-- /agent:exchange -->\n";

        assert!(
            snapshot.len() > current.len() + 100,
            "fixture should be large enough to trip the length gate"
        );
        assert_eq!(stale_snapshot_reset_drift(&snapshot, current), None);
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_accepts_benign_wedge() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline();

        assert!(live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &fragmented,
            identity_normalize
        ));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_no_wedge() {
        let snapshot = drift_content_ours();

        assert!(!live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &snapshot,
            identity_normalize
        ));
    }

    #[test]
    fn live_prompt_drift_auto_recovery_safe_rejects_disk_only_exchange_prompt() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\n❯ do #brand-new-user-prompt-typed-after-preflight\n<!-- /agent:exchange -->",
        );

        assert!(!live_prompt_drift_auto_recovery_safe(
            &snapshot,
            &fragmented,
            identity_normalize
        ));
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_disk_only_queue_item() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-queue-item]\n<!-- /agent:queue -->",
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("queue edits should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- do [#user-added-queue-item]"));
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_partial_exchange_word() {
        let snapshot = drift_content_ours();
        let fragmented = drift_baseline().replace(
            "❯ do #fix\n<!-- /agent:exchange -->",
            "❯ do #fix\noperator-partial-wo\n<!-- /agent:exchange -->",
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("partial exchange text should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(
            target.contains("operator-partial-wo"),
            "operator-typed partial word must survive recovery:\n{target}"
        );
    }

    #[test]
    fn live_prompt_drift_recovery_target_preserves_operator_edited_backlog_text() {
        let snapshot = format!(
            "{}\n<!-- agent:backlog -->\n- original backlog wording\n<!-- /agent:backlog -->\n",
            drift_content_ours()
        );
        let fragmented = format!(
            "{}\n<!-- agent:backlog -->\n- edited backlog wording\n<!-- /agent:backlog -->\n",
            drift_baseline()
        );

        let target = live_prompt_drift_recovery_target(&snapshot, &fragmented, identity_normalize)
            .expect("backlog edits should be preserved while the response lands");

        assert!(target.contains("### Re: do #fix"));
        assert!(target.contains("- edited backlog wording"));
        assert!(!target.contains("- original backlog wording"));
    }

    #[test]
    fn snapshot_contains_dropped_prompt_matches_consumed_and_active() {
        let snapshot = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~do [#consumed]~~\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n",
        );

        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#consumed]"));
        assert!(snapshot_contains_dropped_prompt(snapshot, "do [#active]"));
        assert!(!snapshot_contains_dropped_prompt(snapshot, "do [#gone]"));
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_exchange_and_pending() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("exchange+pending")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_user_prompt_append() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_status_and_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("status+exchange")
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference_with_guidance()
     {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps for calibrating session benchmarks with expected scores\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn is_safe_historical_exchange_growth_allows_prompt_target_before_response() {
        let snapshot = "### Re: older\nold body\n";
        let head = "### Re: older\nold body\n\ndo #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` - codex\nCompleted.\n";

        assert!(is_safe_historical_exchange_insert_block(
            "do #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` - codex\nCompleted."
        ));
        assert!(is_safe_historical_exchange_growth(snapshot, head));
    }

    #[test]
    fn classify_safe_committed_historical_agent_doc_mutation_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            #### #next-steps\n\
            Follow up.\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_committed_historical_agent_doc_mutation(snapshot, file),
            Some("exchange")
        );
        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }

    #[test]
    fn safe_exchange_user_prompt_insert_basic() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\nUSER PROMPT\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_after_response() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response\nEXTRA TEXT";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_deletions() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file =
            "### Re: prev - model\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_rejects_agent_markers() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\n### Re: injected - model\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_no_boundary() {
        let snapshot = "### Re: new - model\nnew response";
        let file = "USER PROMPT\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_identical() {
        let snapshot = "### Re: prev - model\nprev response\n### Re: new - model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, snapshot));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_multiline_prompts() {
        let snapshot = "### Re: prev - model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        let file = "### Re: prev - model\nprev response\nline one\nline two\nline three\n<!-- agent:boundary:abc -->\n### Re: new - model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }

    #[test]
    fn safe_exchange_user_prompt_insert_classify_integration() {
        let snapshot_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev - model\nprev response\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new - model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        let file_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev - model\nprev response\n\
            USER PROMPT\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new - model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot_doc, file_doc),
            Some("exchange")
        );
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_captures_unowned_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &candidate, baseline);
        assert_eq!(dropped, vec!["go".to_string()]);
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_empty_when_content_ours_owns_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let with_go = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &with_go, &with_go);
        assert!(dropped.is_empty());
    }

    #[test]
    fn explicit_baseline_preserves_concurrent_user_edits_for_next_cycle() {
        let baseline = Some("baseline");
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::ContentOurs
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            content_ours
        );
    }

    #[test]
    fn explicit_baseline_forward_merges_concurrent_comment_tail_into_this_cycle() {
        let baseline = Some("baseline");
        let base = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let content_current = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";
        let content_ours = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let final_content = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";

        assert_eq!(
            snapshot_persist_mode_with_current(
                baseline,
                base,
                content_current,
                content_ours,
                final_content
            ),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode_with_current(
                    baseline,
                    base,
                    content_current,
                    content_ours,
                    final_content
                ),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn implicit_baseline_still_persists_final_merged_disk_state() {
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(None, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(None, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn explicit_baseline_keeps_final_content_when_delta_is_prior_streamed_agent_prefix() {
        let baseline = Some("baseline");
        let content_ours = "<!-- agent:exchange -->\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: orchestrate streaming — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn response_target_disjoint_from_user_edit_carries_queue_directives_forward() {
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#fix]\n",
            "<!-- /agent:queue -->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace(
            "- do [#fix]\n<!-- /agent:queue -->",
            "- do [#fix]\n- do [#user-added-directive]\n<!-- /agent:queue -->",
        );

        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_accepts_plain_outside_edit() {
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\nold parked note body\n-->\n",
        )
        .to_string();
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented and verified with a long-enough response body to matter.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace("old parked note body", "edited parked note body");

        assert!(response_target_disjoint_from_user_edit(
            &baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_blocks_unproven_queue_deletion() {
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#keep]\n",
            "<!-- /agent:queue -->\n\n",
            "<!--\nold parked note body\n-->\n",
        )
        .to_string();
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented and verified with a long-enough response body to matter.\n<!-- /agent:exchange -->",
        );
        let candidate = ours
            .replace("- do [#keep]\n", "")
            .replace("old parked note body", "edited parked note body");

        assert!(!response_target_disjoint_from_user_edit(
            &baseline,
            &ours,
            &candidate,
            |_, _, candidate| Some(candidate.to_string())
        ));
    }

    #[test]
    fn response_target_disjoint_from_user_edit_blocks_response_rewrite_and_new_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented the fix and verified it end to end.\n<!-- /agent:exchange -->",
        );
        let rewritten = ours.replace(
            "Implemented the fix and verified it end to end.",
            "User rewrote the committed response body inside the live buffer.",
        );
        let new_prompt = ours.replace(
            "<!-- /agent:exchange -->",
            "❯ a brand new prompt typed during closeout\n<!-- /agent:exchange -->",
        );

        for candidate in [rewritten, new_prompt, ours.clone()] {
            assert!(!response_target_disjoint_from_user_edit(
                baseline,
                &ours,
                &candidate,
                |_, _, candidate| Some(candidate.to_string())
            ));
        }
    }

    #[test]
    fn response_target_disjoint_from_user_edit_requires_clean_merge() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #fix\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\nold note\n-->\n",
        );
        let ours = baseline.replace(
            "<!-- /agent:exchange -->",
            "### Re: do #fix — opus-4-8\n\nImplemented.\n<!-- /agent:exchange -->",
        );
        let candidate = ours.replace("old note", "edited note");

        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, _| None
        ));
        assert!(!response_target_disjoint_from_user_edit(
            baseline,
            &ours,
            &candidate,
            |_, _, _| Some("<<<<<<< conflict\n>>>>>>>".to_string())
        ));
    }
}
