//! Pure queue prompt consumption policy.
//!
//! This module owns entry-level consume planning helpers. Callers still own
//! file IO, snapshot persistence, IPC transport, and editor convergence.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use agent_doc_document::queue_projection::{
    IN_PROGRESS_MARKER, strip_in_progress_marker, strip_priority_markers,
};
use agent_doc_document::write_normalization::strip_boundary_for_dedup;
use agent_doc_element::element;
use agent_doc_markdown_ast::mutations::{MutationNodePatch, MutationNodePatchOp};
use anyhow::{Context, Result};

use crate::{
    document_queue::{self, QueueEntry},
    queue_response::{
        normalize_done_id, normalize_queue_prompt_text, queue_prompt_done_id,
        queue_prompt_text_is_free_text, queue_prompt_text_matches,
    },
};

/// The deterministic, visible explanation appended to a struck free-text queue
/// head (`#qstrikenote`). It is fixed text and lives outside the `~~...~~`
/// wrapper so the original head text stays struck and readable.
pub const STRUCK_FREE_TEXT_NOTE: &str = "answered this cycle (#ftstrike)";

/// Pure projection of the queue mutation implied by a captured response and an
/// authoritative markdown frontier.
///
/// The controller keeps the response and markdown as independent reactive
/// inputs.  This value is therefore a derived target, not a request to perform
/// queue maintenance.  I/O adapters may apply `target_content` through the
/// document authority and then publish the resulting authority frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsweredFreeTextStrikeProjection {
    pub node_keys: Vec<String>,
    pub target_content: String,
}

pub struct QueueConsumptionPlan {
    pub consumed_text: String,
    pub consumed_texts: Vec<String>,
    pub node_ops: Vec<IpcNodeOp>,
    pub remaining: usize,
    pub drained: bool,
    pub auto: bool,
    pub new_document: String,
    pub new_snapshot: String,
    pub save_snapshot: bool,
}

pub fn reconcile_postcommit_queue_strikes_to_head(working: &str, head: &str) -> Option<String> {
    let working_components = element::parse(working).ok()?;
    let head_components = element::parse(head).ok()?;
    let working_queue = working_components
        .iter()
        .find(|component| component.name == "queue")?;
    let head_queue = head_components
        .iter()
        .find(|component| component.name == "queue")?;
    let working_body = working_queue.content(working);
    let head_body = head_queue.content(head);
    let working_entries = document_queue::parse(working_body).ok()?;
    let head_entries = document_queue::parse(head_body).ok()?;

    let prompt_key = |text: &str| text.trim().to_string();
    let mut head_active_counts: HashMap<String, usize> = HashMap::new();
    let mut head_completed_counts: HashMap<String, usize> = HashMap::new();
    for entry in &head_entries {
        match entry {
            QueueEntry::Prompt(prompt) => {
                *head_active_counts
                    .entry(prompt_key(&prompt.text))
                    .or_insert(0) += 1;
            }
            QueueEntry::Completed(prompt) => {
                *head_completed_counts
                    .entry(prompt_key(&prompt.text))
                    .or_insert(0) += 1;
            }
            _ => {}
        }
    }
    if head_completed_counts.is_empty() {
        return None;
    }

    let mut seen_working_active: HashMap<String, usize> = HashMap::new();
    let mut restored = false;
    let reconciled_entries: Vec<QueueEntry> = working_entries
        .into_iter()
        .map(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                let key = prompt_key(&prompt.text);
                let seen = seen_working_active.entry(key.clone()).or_insert(0);
                *seen += 1;
                let allowed_active = head_active_counts.get(&key).copied().unwrap_or(0);
                let head_completed = head_completed_counts.get(&key).copied().unwrap_or(0);
                if *seen > allowed_active && head_completed > 0 {
                    restored = true;
                    QueueEntry::Completed(prompt)
                } else {
                    QueueEntry::Prompt(prompt)
                }
            }
            other => other,
        })
        .collect();
    if !restored {
        return None;
    }

    let new_body = document_queue::render(&reconciled_entries);
    if new_body == working_body {
        return None;
    }
    Some(working_queue.replace_content(working, &new_body))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextQueueHeadSelection {
    pub node_key: String,
    pub head_text: String,
    pub stop_fence_at_head: bool,
}

/// The structural-op vocabulary for queue node mutations. Each variant maps to a
/// [`QueueCrdt`](agent_doc_merge::queue_seqcrdt::QueueCrdt) effect (Phase A,
/// `agent-doc-merge::queue_seqcrdt`):
/// - [`NodeOpKind::Consume`] / [`NodeOpKind::MarkDone`] → `QueueCrdt::mark_done`
///   (the node stays, rendered `~~text~~` — completed).
/// - [`NodeOpKind::Strike`] → `QueueCrdt::strike` (tombstone — the node is
///   removed from the live view).
///
/// Phase B of `plan-crdt-structural-ops`: this is the IPC vocabulary the editor
/// structural-op handler (Phase C) applies and the consume sites (Phase D) emit
/// instead of a flock-serialized whole-buffer rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOpKind {
    /// Strike-through/keep (Prompt → Completed). The historical consume op.
    Consume,
    /// Mark completed (same CRDT effect as `Consume`; distinct intent for audit).
    MarkDone,
    /// Tombstone/remove.
    Strike,
}

impl NodeOpKind {
    /// `true` for ops that tombstone the node in the [`QueueCrdt`].
    ///
    /// [`QueueCrdt`]: agent_doc_merge::queue_seqcrdt::QueueCrdt
    pub fn is_tombstone(self) -> bool {
        matches!(self, NodeOpKind::Strike)
    }
}

/// The canonical op label for a consume (strike-through/keep) structural op.
pub const OP_CONSUME: &str = "consume";
/// The canonical op label for a mark-done (completed) structural op.
pub const OP_MARK_DONE: &str = "mark_done";
/// The canonical op label for a tombstone (remove) structural op.
pub const OP_STRIKE: &str = "strike";
/// The canonical op label for a content replace structural op (carries `content`).
/// Used when a node's mutation is more than a bare strike-through — e.g. the
/// `#qstrikenote` annotation appended to a struck free-text head — so the
/// per-node target text is sent as a drift-resilient `MutationNodePatchOp::Replace`.
pub const OP_REPLACE: &str = "replace";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcNodeOp {
    pub component: String,
    pub node_id: String,
    pub op: String,
    /// Replacement text for `OP_REPLACE` ops (the fully-mutated node raw, e.g. an
    /// annotated struck line). `None` for strike/consume/mark_done/strike ops.
    pub content: Option<String>,
}

impl IpcNodeOp {
    /// A consume op (strike-through/keep → `QueueCrdt::mark_done`).
    pub fn consume(component: &str, node_id: String) -> Self {
        Self::new(component, node_id, OP_CONSUME)
    }

    /// A mark-done op (completed → `QueueCrdt::mark_done`).
    pub fn mark_done(component: &str, node_id: String) -> Self {
        Self::new(component, node_id, OP_MARK_DONE)
    }

    /// A strike op (tombstone/remove → `QueueCrdt::strike`).
    pub fn strike(component: &str, node_id: String) -> Self {
        Self::new(component, node_id, OP_STRIKE)
    }

    /// A replace op: swap the node's raw text for `content` (→
    /// `MutationNodePatchOp::Replace`). Use this when the per-node mutation
    /// carries more than a bare strike-through (e.g. an annotation). The node
    /// stays live with the new text; in `QueueCrdt` terms this is a mark_done
    /// effect (the node is not tombstoned).
    pub fn replace(component: &str, node_id: String, content: String) -> Self {
        Self {
            component: component.to_string(),
            node_id,
            op: OP_REPLACE.to_string(),
            content: Some(content),
        }
    }

    fn new(component: &str, node_id: String, op: &str) -> Self {
        Self {
            component: component.to_string(),
            node_id,
            op: op.to_string(),
            content: None,
        }
    }

    /// The durable node key this op targets (the `node_id` is the queue
    /// `node_key` from `agent_doc_markdown_ast::mutations::item_nodes`).
    pub fn node_key(&self) -> &str {
        &self.node_id
    }

    /// Classify this op's effect on the [`QueueCrdt`].
    ///
    /// [`QueueCrdt`]: agent_doc_merge::queue_seqcrdt::QueueCrdt
    pub fn kind(&self) -> NodeOpKind {
        match self.op.as_str() {
            OP_MARK_DONE | OP_REPLACE => NodeOpKind::MarkDone,
            OP_STRIKE => NodeOpKind::Strike,
            _ => NodeOpKind::Consume,
        }
    }

    /// `true` if this op tombstones (removes) the node in the [`QueueCrdt`].
    ///
    /// [`QueueCrdt`]: agent_doc_merge::queue_seqcrdt::QueueCrdt
    pub fn is_tombstone(&self) -> bool {
        self.kind().is_tombstone()
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "component": self.component,
            "node_id": self.node_id,
            "op": self.op,
        });
        if let Some(content) = &self.content {
            v["content"] = serde_json::Value::String(content.clone());
        }
        v
    }
}

pub fn queue_consume_node_ops(node_keys: &[String]) -> Vec<IpcNodeOp> {
    node_keys
        .iter()
        .cloned()
        .map(|node_key| IpcNodeOp::consume("queue", node_key))
        .collect()
}

/// Build mark-done structural ops for the given queue node keys.
pub fn queue_mark_done_node_ops(node_keys: &[String]) -> Vec<IpcNodeOp> {
    node_keys
        .iter()
        .cloned()
        .map(|node_key| IpcNodeOp::mark_done("queue", node_key))
        .collect()
}

/// Build strike (tombstone) structural ops for the given queue node keys.
pub fn queue_strike_node_ops(node_keys: &[String]) -> Vec<IpcNodeOp> {
    node_keys
        .iter()
        .cloned()
        .map(|node_key| IpcNodeOp::strike("queue", node_key))
        .collect()
}

/// Structural ops for site 2 (`strike_answered_free_text_queue_heads`): consume
/// (strike-through) the answered free-text queue heads.
pub fn free_text_strike_node_ops(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
) -> Result<Vec<IpcNodeOp>> {
    let keys = answered_free_text_head_node_keys(content, response_body, baseline)?;
    Ok(queue_consume_node_ops(&keys))
}

/// Structural ops for the node-addressable part of site 3
/// (`prune_noise_queue_heads`): consume (strike-through) the bulleted noise heads
/// and orphan id-backed heads. The multiline/freeform excise is byte-range
/// removal of non-item content and is NOT node-addressable, so it is not
/// represented here; callers keep that excise in the full-text path until a
/// non-item structural op exists.
pub fn noise_prune_node_ops(content: &str) -> Result<Vec<IpcNodeOp>> {
    let mut keys = noise_queue_head_node_keys(content)?;
    for key in orphan_id_queue_head_node_keys(content)? {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    Ok(queue_consume_node_ops(&keys))
}

/// Structural ops for sites 4 and 5 (`strike_orphan_id_backed_queue_head` +
/// `acknowledge_open_id_backed_queue_head`): consume (strike-through) the
/// id-backed queue head(s) resolving to `target_id`.
pub fn id_backed_strike_node_ops(content: &str, target_id: &str) -> Result<Vec<IpcNodeOp>> {
    let keys = id_backed_head_node_keys(content, target_id)?;
    Ok(queue_consume_node_ops(&keys))
}

/// Structural ops for site 6 (`mark_completed_queue_prompts_for_done_ids`):
/// mark-done the queue entries whose directive id is in `done_ids`.
pub fn mark_done_node_ops_for_done_ids(
    content: &str,
    done_ids: &[String],
    consumed_texts: &[String],
) -> Vec<IpcNodeOp> {
    let keys = queue_prompt_node_keys_for_done_ids(content, done_ids, consumed_texts).keys;
    queue_mark_done_node_ops(&keys)
}

/// Map a batch of structural ops ([`IpcNodeOp`], Phase B vocabulary) to the
/// drift-resilient node-patch wire format ([`MutationNodePatch`]) the editor
/// already applies via `agent_doc_apply_node_patches`.
///
/// Effect mapping (Phase C, `#crdtstructops`):
/// - `consume` / `mark_done` (strike-through/keep) → [`MutationNodePatchOp::Strike`].
/// - `strike` (tombstone/remove) → [`MutationNodePatchOp::Remove`].
///
/// Reusing the existing node-patch machinery means the editor needs no new
/// apply primitive — an `ApplyStructuralOp` intent just carries these as its
/// `node_patches` without the whole-buffer canonical replace.
pub fn ipc_node_ops_to_node_patches(ops: &[IpcNodeOp]) -> Vec<MutationNodePatch> {
    ops.iter()
        .map(|op| {
            let is_replace = op.op == OP_REPLACE;
            let (patch_op, content) = if is_replace {
                (
                    MutationNodePatchOp::Replace,
                    op.content.clone().unwrap_or_default(),
                )
            } else if op.is_tombstone() {
                (MutationNodePatchOp::Remove, String::new())
            } else {
                (MutationNodePatchOp::Strike, String::new())
            };
            MutationNodePatch {
                component: op.component.clone(),
                node_key: op.node_id.clone(),
                op: patch_op,
                content: if is_replace { Some(content) } else { None },
                expected_content: None,
                before: None,
                after: None,
                order: Vec::new(),
            }
        })
        .collect()
}

/// Apply a batch of structural ops to document `content`, returning the mutated
/// text. This is the pure detached-writer projection of the CRDT structural ops:
/// it maps each [`IpcNodeOp`] to a [`MutationNodePatch`] and applies them via the
/// existing `apply_node_patches` machinery, which is byte-identical to the
/// per-site `consume_queue_nodes_by_key` strike-through for node-addressable
/// ops (`strike_node_patch` calls the same `consume_node`). The editor-attached
/// path instead sends an `ApplyStructuralOp` intent (Phase C); this helper is
/// the detached single-writer fallback (Phase E).
pub fn apply_structural_ops_to_content(content: &str, ops: &[IpcNodeOp]) -> Result<String> {
    let patches = ipc_node_ops_to_node_patches(ops);
    agent_doc_markdown_ast::mutations::apply_node_patches(content, &patches)
        .map_err(|err| anyhow::anyhow!("structural op apply failed: {err}"))
}

/// Derive node-keyed `OP_REPLACE` structural ops from the per-node raw-text diff
/// between `before` and `after`. For each node whose raw changed (e.g. a free-text
/// head that was struck `~~text~~` AND annotated `~~text~~ — note`), emit a
/// [`IpcNodeOp::replace`] carrying the new raw. This is the robust bridge from a
/// site's full-text computation to node-only structural ops: it reuses the actual
/// computed target text, so it never diverges from the site's `new_document` for
/// the mutated nodes, without reimplementing the strike/annotate logic.
///
/// Nodes unchanged between `before` and `after` produce no op; the caller falls
/// back to the full-text write when the result is empty (e.g. a mutation the node
/// model can't express, like a non-item excise).
pub fn node_replace_ops_from_diff(
    before: &str,
    after: &str,
    component: &str,
) -> Result<Vec<IpcNodeOp>> {
    // Compare full node spans (which include the `- ` bullet) so a Replace op —
    // which rewrites `item.start_byte..end_byte` — preserves the bullet. Using
    // `item.raw` (bullet-stripped) here would drop the bullet on apply.
    let before_spans: HashMap<String, String> =
        agent_doc_markdown_ast::mutations::item_nodes(before, component)
            .map_err(|err| anyhow::anyhow!("node diff: failed to parse before: {err}"))?
            .into_iter()
            .map(|n| {
                (
                    n.node_key,
                    before[n.item.start_byte..n.item.end_byte].to_string(),
                )
            })
            .collect();
    let after_nodes = agent_doc_markdown_ast::mutations::item_nodes(after, component)
        .map_err(|err| anyhow::anyhow!("node diff: failed to parse after: {err}"))?;
    let mut ops = Vec::new();
    for n in after_nodes {
        let after_span = &after[n.item.start_byte..n.item.end_byte];
        if let Some(before_span) = before_spans.get(&n.node_key)
            && after_span != before_span
        {
            ops.push(IpcNodeOp::replace(
                component,
                n.node_key,
                after_span.to_string(),
            ));
        }
    }
    Ok(ops)
}
pub fn first_n_queue_prompt_texts(entries: &[QueueEntry], count: usize) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(strip_in_progress_marker(&prompt.text)),
            _ => None,
        })
        .take(count)
        .collect()
}

pub fn next_queue_head_selection(content: &str) -> Result<Option<NextQueueHeadSelection>> {
    let components = element::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        document_queue::parse(body).context("queue consume: failed to parse next queue head")?;
    let stop_fence_at_head = document_queue::has_stop_fence_at_head(&entries);
    let Some(head_text) = first_n_queue_prompt_texts(&entries, 1).into_iter().next() else {
        return Ok(None);
    };
    let Some(node_key) = queue_prompt_node_keys_for_count(content, 1)?
        .keys
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    Ok(Some(NextQueueHeadSelection {
        node_key,
        head_text,
        stop_fence_at_head,
    }))
}

pub fn queue_consume_count_for_done_ids(entries: &[QueueEntry], done_ids: &[String]) -> usize {
    if done_ids.is_empty() {
        return 0;
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let mut count = 0usize;
    for entry in entries {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let Some(id) = queue_prompt_done_id(&prompt.text) else {
            break;
        };
        if done_ids.contains(&id) {
            count += 1;
            continue;
        }
        break;
    }
    count
}

pub fn queue_prompt_texts_match_for_consumption(left: &str, right: &str) -> bool {
    strip_priority_markers(left) == strip_priority_markers(right)
}

/// Resolve whether this cycle's committed response should consume (strike) the
/// active queue head. The `file` argument is retained for the orchestration
/// call shape; this crate makes the decision from the supplied document content.
pub fn queue_consumption_allowed_for_response(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    completion_ids: &[String],
) -> Result<bool> {
    if should_consume_queue_prompt_for_write(file, baseline, current_content, completion_ids)? {
        return Ok(true);
    }
    if crate::queue_heads::queue_head_has_explicit_completion_signal(
        current_content,
        completion_ids,
    )? {
        return Ok(true);
    }
    let has_response = !response_body.trim().is_empty();
    if has_response && response_targets_synthetic_queue_head_id(current_content, response_body)? {
        return Ok(true);
    }
    if has_response
        && crate::queue_response::queue_head_is_free_text_prompt(current_content)?
        && let Some(head_text) = crate::queue_heads::active_queue_head_text(current_content)?
    {
        return Ok(crate::queue_response::free_text_head_answered_by_response(
            response_body,
            &head_text,
        ) && !cycle_answered_foreign_exchange_prompt(
            baseline,
            current_content,
            &head_text,
        ));
    }
    Ok(false)
}

pub fn should_consume_queue_prompt_for_write(
    _file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    completion_ids: &[String],
) -> Result<bool> {
    // An explicit closeout signal naming the queue head authorizes consumption
    // regardless of any pending mutations bundled into the same diff
    // (#pending-add-suppresses-queue-consume).
    if crate::queue_heads::queue_head_matches_done_ids(current_content, completion_ids)? {
        return Ok(true);
    }
    let Some(base) = baseline else {
        return Ok(false);
    };
    let base_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let diff_text = agent_doc_diff::unified_diff_from_contents(&base_norm, &current_norm);
    should_consume_queue_prompt_for_diff_content(_file, current_content, diff_text.as_deref())
}

pub fn should_consume_queue_prompt_for_diff_content(
    _file: &Path,
    content: &str,
    diff_text: Option<&str>,
) -> Result<bool> {
    let Some(queue_head) = crate::queue_heads::active_queue_head_text(content)? else {
        return Ok(true);
    };
    let Some(diff_text) = diff_text else {
        return Ok(false);
    };
    let prompt_changes: Vec<_> = agent_doc_diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if agent_doc_diff::detect_queue_trigger(diff_text) {
        return Ok(true);
    }
    if prompt_changes
        .iter()
        .any(|change| crate::queue_response::queue_prompt_text_matches(&change.text, &queue_head))
    {
        return Ok(true);
    }
    Ok(false)
}

/// Return the id of the pre-commit queue head when this turn targeted that
/// exact head through the prompt diff or response heading.
pub fn queue_targeted_completion_id_for_current_head(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    pending_done: &[String],
) -> Result<Option<String>> {
    if crate::queue_response::queue_head_is_free_text_prompt(current_content)? {
        return Ok(None);
    }
    let Some(queue_head) = crate::queue_heads::active_queue_head_text(current_content)? else {
        return Ok(None);
    };
    let Some(head_id) = crate::queue_response::queue_prompt_done_id(&queue_head) else {
        return Ok(None);
    };
    if !response_body.trim().is_empty()
        && crate::queue_response::response_explicitly_targets_queue_head(response_body, &queue_head)
    {
        return Ok(Some(head_id));
    }
    if should_consume_queue_prompt_for_write(file, baseline, current_content, pending_done)? {
        return Ok(Some(head_id));
    }
    Ok(None)
}

pub fn queue_diff_completion_id_for_current_head(
    file: &Path,
    current_content: &str,
    diff_text: &str,
) -> Result<Option<String>> {
    if crate::queue_response::queue_head_is_free_text_prompt(current_content)? {
        return Ok(None);
    }
    let Some(queue_head) = crate::queue_heads::active_queue_head_text(current_content)? else {
        return Ok(None);
    };
    let Some(head_id) = crate::queue_response::queue_prompt_done_id(&queue_head) else {
        return Ok(None);
    };
    if should_consume_queue_prompt_for_diff_content(file, current_content, Some(diff_text))? {
        return Ok(Some(head_id));
    }
    Ok(None)
}

fn response_targets_synthetic_queue_head_id(content: &str, response: &str) -> Result<bool> {
    let Some(queue_head) = crate::queue_heads::active_queue_head_text(content)? else {
        return Ok(false);
    };
    if crate::queue_response::queue_head_is_bare_do_directive(&queue_head) {
        return Ok(false);
    }
    let Some(head_id) = crate::queue_response::queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    // #zwn5: an operator-pinned bare id head that resolves to exactly its own id
    // and names a tracked backlog/review item is an id-backed directive, not a
    // synthetic/preset prompt. Leave it pinned for an explicit closeout flag.
    let normalized_head = crate::queue_response::normalize_queue_prompt_text(&queue_head);
    if crate::queue_directive::topic_resolves_to_exact_id(&normalized_head, &head_id)
        && crate::queue_response::head_id_names_tracked_directive_item(content, &head_id)
    {
        return Ok(false);
    }
    Ok(response
        .lines()
        .filter_map(crate::queue_response::response_heading_topic)
        .any(|topic| crate::queue_directive::topic_resolves_to_exact_id(topic, &head_id)))
}

/// Node keys of every non-struck free-text queue head that this cycle answered,
/// at any position in the queue.
pub fn answered_free_text_head_node_keys(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
) -> Result<Vec<String>> {
    if response_body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
        anyhow::anyhow!("free-text strike: failed to derive queue node keys: {err}")
    })?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !crate::queue_response::queue_prompt_text_is_free_text(content, text)
        {
            continue;
        }
        // `#bugautostruck`: the in-progress marker proves only which queue head
        // was selected for this cycle. It is not evidence that the response
        // addressed that head. Require the same exact quoted-prompt proof for
        // marked and unmarked free-text heads.
        if !crate::queue_response::free_text_head_answered_by_response(response_body, text) {
            continue;
        }
        // `#ftstrikedefer`: the quoted echo is the responding agent's assertion
        // that it answered the head, not proof. When the response says next to
        // that quote that the head is still outstanding, believe the sentence
        // over the echo and leave it queued.
        if crate::queue_response::response_defers_free_text_head(response_body, text) {
            continue;
        }
        if let Some(baseline) = baseline
            && !crate::queue_response::free_text_head_present_in_baseline(baseline, text)
        {
            continue;
        }
        keys.push(node.node_key);
    }
    Ok(keys)
}

/// True when this cycle's diff introduced a prompt-bearing exchange change (a
/// new or edited user prompt) that does NOT match the active queue head.
///
/// This is pure queue policy for free-text queue consumption: a cycle that
/// answered unrelated exchange work should keep the free-text head queued, while
/// a cycle that only added a response for the head may drain it.
pub fn cycle_answered_foreign_exchange_prompt(
    baseline: Option<&str>,
    current_content: &str,
    queue_head: &str,
) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let base_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = agent_doc_diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&base_norm, &current_norm)
    else {
        return false;
    };

    // Prefix-normalization can make an already-answered baseline prompt appear
    // as an added `+❯ ...` diff line. Skip prompt text that already existed in
    // the baseline in bare or prefixed form.
    let baseline_prompt_texts: HashSet<String> = base_norm
        .lines()
        .map(|line| normalize_queue_prompt_text(line.trim().trim_start_matches('❯').trim()))
        .filter(|text| !text.is_empty())
        .collect();

    diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if added.starts_with("++") {
            return false;
        }
        let Some(prompt) = added.trim().strip_prefix('❯') else {
            return false;
        };
        let prompt = prompt.trim();
        if prompt.is_empty() || queue_prompt_text_matches(prompt, queue_head) {
            return false;
        }
        !baseline_prompt_texts.contains(&normalize_queue_prompt_text(prompt))
    })
}

pub fn mark_first_matching_prompts_completed_by_texts(
    entries: &[QueueEntry],
    target_texts: &[String],
) -> Option<Vec<QueueEntry>> {
    let mut remaining_targets = target_texts.to_vec();
    let mut marked = Vec::with_capacity(target_texts.len());
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(pos) = remaining_targets
                .iter()
                .position(|target| queue_prompt_texts_match_for_consumption(&prompt.text, target))
        {
            let mut completed = prompt.clone();
            completed.text = strip_in_progress_marker(&completed.text);
            marked.push(remaining_targets.remove(pos));
            result.push(QueueEntry::Completed(completed));
            continue;
        }
        result.push(entry.clone());
    }
    if marked.len() == target_texts.len() {
        Some(result)
    } else {
        None
    }
}

pub fn mark_entries_completed_by_done_ids(
    entries: &[QueueEntry],
    done_ids: &[String],
) -> (Vec<QueueEntry>, Vec<String>) {
    if done_ids.is_empty() {
        return (entries.to_vec(), Vec::new());
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let mut marked_texts = Vec::new();
    let entries = entries
        .iter()
        .map(|entry| match entry {
            QueueEntry::Prompt(prompt)
                if queue_prompt_done_id(&prompt.text).is_some_and(|id| done_ids.contains(&id)) =>
            {
                let mut completed = prompt.clone();
                completed.text = strip_in_progress_marker(&completed.text);
                marked_texts.push(completed.text.clone());
                QueueEntry::Completed(completed)
            }
            _ => entry.clone(),
        })
        .collect();
    (entries, marked_texts)
}

pub fn normalized_done_id_bag(texts: &[String]) -> Vec<String> {
    let mut ids = texts
        .iter()
        .filter_map(|text| queue_prompt_done_id(text))
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// Node keys selected for queue consumption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePromptNodeKeys {
    pub keys: Vec<String>,
    pub ast_backed: bool,
}

pub fn queue_prompt_node_keys_for_texts(
    content: &str,
    target_texts: &[String],
    preferred_node_keys: &[String],
) -> Result<Option<QueuePromptNodeKeys>> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("queue consume: failed to derive queue node keys: {err}"))?;
    let mut selected_indices = HashSet::new();
    let mut keys = Vec::with_capacity(target_texts.len());
    for (target_index, target_text) in target_texts.iter().enumerate() {
        let preferred = preferred_node_keys.get(target_index);
        let preferred_index = preferred.and_then(|preferred_key| {
            nodes.iter().enumerate().position(|(node_index, node)| {
                !selected_indices.contains(&node_index)
                    && !node.item.struck
                    && node.node_key == *preferred_key
                    && queue_prompt_texts_match_for_consumption(&node.item.text, target_text)
            })
        });
        let fallback_index = || {
            nodes.iter().enumerate().position(|(node_index, node)| {
                !selected_indices.contains(&node_index)
                    && !node.item.struck
                    && queue_prompt_texts_match_for_consumption(&node.item.text, target_text)
            })
        };
        let Some(node_index) = preferred_index.or_else(fallback_index) else {
            return Ok(None);
        };
        selected_indices.insert(node_index);
        keys.push(nodes[node_index].node_key.clone());
    }
    Ok(Some(QueuePromptNodeKeys {
        keys,
        ast_backed: true,
    }))
}

pub fn queue_prompt_node_keys_for_count(
    content: &str,
    count: usize,
) -> Result<QueuePromptNodeKeys> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("queue consume: failed to derive queue node keys: {err}"))?;
    // `#qconsumenostrike`: never assume a second enumerator segments the queue
    // identically. `item_nodes` now includes canonical multiline prompt
    // surfaces, but this text correspondence remains the fail-closed proof for
    // any future or malformed shape. A count-only match could otherwise target
    // the next enumerable node and strike unrun work.
    let unstruck = nodes
        .into_iter()
        .filter(|node| !node.item.struck)
        .take(count)
        .collect::<Vec<_>>();
    let parsed_heads = element::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "queue")
                .map(|component| component.content(content).to_string())
        })
        .and_then(|body| crate::document_queue::parse(&body).ok())
        .map(|entries| {
            crate::document_queue::prompts(&entries)
                .into_iter()
                .map(|prompt| prompt.text.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let corresponds = unstruck.len() >= count
        && unstruck
            .iter()
            .enumerate()
            .take(count)
            .all(|(index, node)| {
                parsed_heads.get(index).is_some_and(|head| {
                    queue_prompt_texts_match_for_consumption(&node.item.text, head)
                })
            });
    if corresponds {
        return Ok(QueuePromptNodeKeys {
            keys: unstruck
                .into_iter()
                .map(|node| node.node_key)
                .collect::<Vec<_>>(),
            ast_backed: true,
        });
    }

    let components = element::parse(content)?;
    let queue_component = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| anyhow::anyhow!("queue consume: document has no agent:queue component"))?;
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        document_queue::parse(body).context("queue consume: failed to parse document queue")?;
    let prompt_texts = first_n_queue_prompt_texts(&entries, count);
    if prompt_texts.len() < count {
        anyhow::bail!(
            "queue consume: document has {} prompt(s) but planned to consume {}",
            prompt_texts.len(),
            count
        );
    }

    let keys = prompt_texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let hash = agent_doc_hash::content_hash(text);
            let short_hash = &hash[..hash.len().min(12)];
            format!("queue:entry:{index}:{short_hash}")
        })
        .collect::<Vec<_>>();

    Ok(QueuePromptNodeKeys {
        keys,
        ast_backed: false,
    })
}

pub fn queue_prompt_node_keys_for_done_ids(
    content: &str,
    done_ids: &[String],
    consumed_texts: &[String],
) -> QueuePromptNodeKeys {
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();

    if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes(content, "queue") {
        let keys = nodes
            .into_iter()
            .filter(|node| !node.item.struck)
            .filter(|node| {
                queue_prompt_done_id(&node.item.text).is_some_and(|id| done_ids.contains(&id))
            })
            .map(|node| node.node_key)
            .collect::<Vec<_>>();
        if keys.len() == consumed_texts.len() {
            return QueuePromptNodeKeys {
                keys,
                ast_backed: true,
            };
        }
    }

    let keys = consumed_texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let hash = agent_doc_hash::content_hash(text);
            let short_hash = &hash[..hash.len().min(12)];
            format!("queue:done:{index}:{short_hash}")
        })
        .collect::<Vec<_>>();
    QueuePromptNodeKeys {
        keys,
        ast_backed: false,
    }
}

pub fn consume_queue_nodes_by_key(content: &str, node_keys: &[String]) -> Result<String> {
    let borrowed = node_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let consumed = agent_doc_markdown_ast::mutations::consume_nodes(content, "queue", &borrowed)
        .map_err(|err| {
            anyhow::anyhow!("queue consume: failed to apply node-keyed consume: {err}")
        })?;
    Ok(strip_in_progress_marker_from_struck_queue_items(&consumed))
}

/// Derive the exact answered-free-text queue target for one authoritative
/// document frontier.
///
/// `baseline` fences the projection to heads that existed when the turn began,
/// so a newly typed operator head cannot be consumed by an older response.
/// Response-materialization proof remains the caller's responsibility because
/// it depends on the closeout projection rather than queue syntax.
pub fn project_answered_free_text_strike(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    if response_body.trim().is_empty() {
        return Ok(None);
    }
    agent_doc_frontmatter::frontmatter::parse(content)?;
    let node_keys = answered_free_text_head_node_keys(content, response_body, baseline)?;
    if node_keys.is_empty() {
        return Ok(None);
    }
    let struck = consume_queue_nodes_by_key(content, &node_keys)?;
    if struck == content {
        return Ok(None);
    }
    let annotated = annotate_newly_struck_free_text_heads(content, &struck)?;
    let remaining = crate::queue_heads::active_queue_heads(&annotated).len();
    let target_content =
        agent_doc_frontmatter::frontmatter::merge_queue_state(&annotated, remaining > 0)?;
    Ok(Some(AnsweredFreeTextStrikeProjection {
        node_keys,
        target_content,
    }))
}

/// Project completion of every live queue prompt matching `done_ids` into the
/// supplied document bytes without performing IO.
///
/// Closeout composition uses this before a tracked-work reap is published so
/// the queue strike and backlog removal share one editor-authoritative target.
pub fn mark_queue_prompts_completed_by_done_ids_in_content(
    content: &str,
    done_ids: &[String],
) -> Result<(String, usize)> {
    if done_ids.is_empty() {
        return Ok((content.to_string(), 0));
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let components = element::parse(content)?;
    if !components.iter().any(|component| component.name == "queue") {
        return Ok((content.to_string(), 0));
    }
    let keys = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("queue done projection: failed to parse queue: {err}"))?
        .into_iter()
        .filter(|node| !node.item.struck)
        .filter(|node| {
            queue_prompt_done_id(&node.item.text).is_some_and(|id| done_ids.contains(&id))
        })
        .map(|node| node.node_key)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Ok((content.to_string(), 0));
    }
    let count = keys.len();
    Ok((consume_queue_nodes_by_key(content, &keys)?, count))
}

/// Verify that every matching live queue prompt in `before` is completed in
/// `after`. IDs that were not queued impose no queue transition requirement.
pub fn done_ids_have_atomic_queue_completion(
    before: &str,
    after: &str,
    done_ids: &[String],
) -> Result<bool> {
    if done_ids.is_empty() {
        return Ok(true);
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let states = |content: &str| -> Result<(HashSet<String>, HashSet<String>)> {
        let components = element::parse(content)?;
        let Some(queue) = components
            .iter()
            .find(|component| component.name == "queue")
        else {
            return Ok((HashSet::new(), HashSet::new()));
        };
        let mut live = HashSet::new();
        let mut completed = HashSet::new();
        for entry in document_queue::parse(queue.content(content))? {
            match entry {
                QueueEntry::Prompt(prompt) => {
                    if let Some(id) = queue_prompt_done_id(&prompt.text)
                        && done_ids.contains(&id)
                    {
                        live.insert(id);
                    }
                }
                QueueEntry::Completed(prompt) => {
                    if let Some(id) = queue_prompt_done_id(&prompt.text)
                        && done_ids.contains(&id)
                    {
                        completed.insert(id);
                    }
                }
                _ => {}
            }
        }
        Ok((live, completed))
    };
    let (before_live, _) = states(before)?;
    if before_live.is_empty() {
        return Ok(true);
    }
    let (after_live, after_completed) = states(after)?;
    Ok(before_live
        .iter()
        .all(|id| !after_live.contains(id) && after_completed.contains(id)))
}

fn strip_in_progress_marker_from_struck_queue_items(content: &str) -> String {
    let Ok(components) = element::parse(content) else {
        return content.to_string();
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return content.to_string();
    };
    let body = queue.content(content);
    let needle_with_space = format!("~~{} ", IN_PROGRESS_MARKER);
    let needle_bare = format!("~~{}", IN_PROGRESS_MARKER);
    let updated_body = body
        .replace(&needle_with_space, "~~")
        .replace(&needle_bare, "~~");
    if updated_body == body {
        content.to_string()
    } else {
        queue.replace_content(content, &updated_body)
    }
}

/// Node keys of every active (non-struck) queue head that is non-drainable
/// **noise** (`#goqstall2` / `#qcontam`): pasted console output, an agent-response
/// fragment, or another structural/log artifact. `preset_supplies_directive` is
/// taken from the queue's `preset` attribute so classification matches
/// `queue_continuation::queue_stale_noise_lines` exactly. Id-backed directive heads
/// (`do [#id]`) and genuinely drainable free-text/prose heads are excluded, so
/// pruning never desyncs tracked or runnable work.
pub fn noise_queue_head_node_keys(content: &str) -> Result<Vec<String>> {
    let preset_supplies_directive = element::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .iter()
                .find(|c| c.name == "queue")
                .map(|c| c.attrs.contains_key("preset"))
        })
        .unwrap_or(false);
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("noise prune: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() {
            continue;
        }
        if crate::queue_continuation::is_noise_queue_head(text, preset_supplies_directive) {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
}

/// Node keys of every live (non-struck) **orphan id-backed** queue head: an
/// id-backed directive (`do [#id]` / `[#id]` / `#id`) whose id names NO open
/// `agent:backlog` item (#orphanqhead bulk prune / #qchurn). Such a head has no
/// drain path — `queue consume` rejects id-backed heads ("reap via --done") and
/// `--done <id>` is a no-op — yet it is excluded from `drainable_head_count` and,
/// when it sits at the queue head, BLOCKS the leading-run `queue consume` from
/// reaching answered free-text heads behind it, so the go-mode loop churns. Bulk
/// pruning it (alongside noise) clears that wedge without the operator naming each
/// id via the targeted `queue consume --id <id>` escape hatch.
///
/// Gated on an `agent:backlog` component being PRESENT: a free-form id-head queue
/// (no backlog) treats the id-heads AS the work, so membership is not required and
/// nothing is pruned — mirroring `head_is_drainable`'s `open_backlog_ids` gate so
/// the prune set and the drainable set agree on what an "orphan" is.
pub fn orphan_id_queue_head_node_keys(content: &str) -> Result<Vec<String>> {
    let has_backlog = element::parse(content)
        .map(|comps| comps.iter().any(|c| element::is_backlog_component(&c.name)))
        .unwrap_or(false);
    if !has_backlog {
        return Ok(Vec::new());
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("orphan prune: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        // Only id-backed heads are candidates; a free-text report that merely
        // contains a stray `#token` must never be force-struck here.
        if text.is_empty() || queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        let Some(id) = queue_prompt_done_id(text) else {
            continue;
        };
        // An id naming OPEN backlog work (including a deferred `[operator-verify]` /
        // `[focused-cycle]` item, which is still an open `[ ]`/`[/]` entry) has a
        // real drain path — preserve it. Only a truly absent id is an orphan.
        if !head_id_names_open_backlog_item(content, &id) {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
}

/// Clear every non-drainable queue head from `content`, returning the rewritten
/// document and the number struck. Two non-drainable classes are cleared: **noise**
/// (pasted console output / agent fragments / structural artifacts) and **orphan
/// id-backed heads** (`#orphanqhead`: a `do [#id]` / `[#id]` head whose id names no
/// open `agent:backlog` item). Multiline fenced noise blocks are excised by byte
/// range (`queue::parse_spans`); bulleted single-line noise AND orphan id heads are
/// struck by durable node key (`item_nodes`). Multiline removal runs first so the
/// node-key pass sees stable post-excision offsets. (#qnoise-multiline-strike)
pub fn strike_all_noise_queue_heads(content: &str) -> Result<(String, usize)> {
    let comps = element::parse(content)?;
    let Some(queue) = comps.iter().find(|c| c.name == "queue") else {
        return Ok((content.to_string(), 0));
    };
    let preset_supplies_directive = queue.attrs.contains_key("preset");
    let body_start = queue.open_end;
    let body = &content[body_start..queue.close_start];

    // 1. Multiline noise Prompt blocks AND pasted-evidence `Freeform` lines, by exact
    //    byte range (#qnoise-multiline-strike). A multiline `---`/~~~-fenced Prompt is
    //    excised only when its text is noise (multi-line console dump, nested ``` fence,
    //    agent-marker, or bold-report) — a single-line `do [#id]` directive that merely
    //    happens to be `---`-wrapped stays drainable and is preserved. A bare ```` ``` ````
    //    console paste (the most common operator flood) is not a recognized queue fence,
    //    so it lands as a run of `Freeform` lines instead; `is_noise_freeform_line`
    //    excises those while preserving `---`/`~~~` separators and `re [#id]` references.
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (entry, range) in document_queue::parse_spans(body)? {
        let is_noise = match &entry {
            QueueEntry::Prompt(prompt) => {
                prompt.multiline
                    && crate::queue_continuation::is_noise_queue_head(
                        &prompt.text,
                        preset_supplies_directive,
                    )
            }
            QueueEntry::Freeform(line) => document_queue::is_noise_freeform_line(line),
            _ => false,
        };
        if is_noise {
            ranges.push((body_start + range.start)..(body_start + range.end));
        }
    }
    let multiline_struck = ranges.len();
    let mut working = content.to_string();
    ranges.sort_by_key(|r| r.start);
    // Excise back-to-front so earlier offsets stay valid.
    for range in ranges.into_iter().rev() {
        working.replace_range(range, "");
    }

    // 2. Bulleted single-line noise heads AND orphan id-backed heads (#orphanqhead),
    //    struck via durable node keys. Orphan id-heads are non-drainable like noise
    //    but `is_noise_queue_head` keeps them (they carry an `#id`), so they are
    //    collected separately and merged into one strike set. Dedup so a head that
    //    somehow matches both passes is not double-counted.
    let mut keys = noise_queue_head_node_keys(&working)?;
    for key in orphan_id_queue_head_node_keys(&working)? {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let bulleted_struck = keys.len();
    if !keys.is_empty() {
        working = consume_queue_nodes_by_key(&working, &keys)?;
    }

    Ok((working, multiline_struck + bulleted_struck))
}

/// Node keys of every live (non-struck) id-backed queue head whose directive id
/// resolves to exactly `target_id`. Free-text prompts — even ones that merely
/// *contain* a `#token` — are excluded so the orphan escape hatch can never strike
/// a free-text operator report by accident.
pub fn id_backed_head_node_keys(content: &str, target_id: &str) -> Result<Vec<String>> {
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
        .map_err(|err| anyhow::anyhow!("orphan strike: failed to derive queue node keys: {err}"))?;
    let mut keys = Vec::new();
    for node in nodes {
        if node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || queue_prompt_text_is_free_text(content, text) {
            continue;
        }
        if queue_prompt_done_id(text).as_deref() == Some(target_id) {
            keys.push(node.node_key);
        }
    }
    Ok(keys)
}

/// True when `target_id` names a NON-done item in any `agent:backlog` component —
/// live work that should drain through the normal `--done` lifecycle rather than
/// the orphan escape hatch.
pub fn head_id_names_open_backlog_item(content: &str, target_id: &str) -> bool {
    let Ok(comps) = element::parse(content) else {
        return false;
    };
    comps
        .iter()
        .filter(|c| element::is_backlog_component(&c.name))
        .any(|comp| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(comp.content(content));
            items.iter().any(|item| {
                !item.is_done() && !item.id.is_empty() && item.id.eq_ignore_ascii_case(target_id)
            })
        })
}

/// Given a single queue line, append the deterministic auto-struck explanation
/// when the line is a struck free-text head that is not already annotated.
pub fn annotate_struck_free_text_line(line: &str) -> String {
    let (core, newline) = match line.strip_suffix('\n') {
        Some(rest) => (rest, "\n"),
        None => (line, ""),
    };
    let trimmed_end = core.trim_end();
    let trailing_ws = &core[trimmed_end.len()..];
    if trimmed_end.contains(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR) {
        return line.to_string();
    }
    if !trimmed_end.ends_with("~~") {
        return line.to_string();
    }
    let content = strip_list_bullet_prefix(trimmed_end);
    let Some(inner) = content
        .strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
    else {
        return line.to_string();
    };
    if inner.trim().is_empty() {
        return line.to_string();
    }
    format!(
        "{trimmed_end}{}{STRUCK_FREE_TEXT_NOTE}{trailing_ws}{newline}",
        agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
    )
}

/// Strip a leading markdown list bullet from a line's content.
fn strip_list_bullet_prefix(line: &str) -> &str {
    let t = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    if let Some(dot) = t.find(". ")
        && t[..dot].chars().all(|c| c.is_ascii_digit())
        && !t[..dot].is_empty()
    {
        return t[dot + 2..].trim_start();
    }
    t
}

/// Apply the auto-struck annotation to every free-text queue head that became
/// struck between `before` and `after`.
pub fn annotate_newly_struck_free_text_heads(before: &str, after: &str) -> anyhow::Result<String> {
    let struck_before: HashSet<String> =
        agent_doc_markdown_ast::mutations::item_nodes(before, "queue")
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|n| n.item.struck)
                    .map(|n| n.node_key)
                    .collect()
            })
            .unwrap_or_default();

    let nodes = match agent_doc_markdown_ast::mutations::item_nodes(after, "queue") {
        Ok(nodes) => nodes,
        Err(_) => return Ok(after.to_string()),
    };

    let mut edits: Vec<(usize, usize)> = Vec::new();
    for node in &nodes {
        if !node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !queue_prompt_text_is_free_text(after, text) {
            continue;
        }
        if struck_before.contains(&node.node_key) {
            continue;
        }
        let line = &after[node.item.start_byte..node.item.end_byte];
        if line.contains(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR) {
            continue;
        }
        edits.push((node.item.start_byte, node.item.end_byte));
    }

    if edits.is_empty() {
        return Ok(after.to_string());
    }
    edits.sort_by_key(|(start, _)| *start);
    let mut out = after.to_string();
    for (start, end) in edits.into_iter().rev() {
        let annotated = annotate_struck_free_text_line(&out[start..end]);
        out.replace_range(start..end, &annotated);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;
    use agent_doc_document::queue_projection::IN_PROGRESS_MARKER;

    fn entries(body: &str) -> Vec<QueueEntry> {
        document_queue::parse(body).unwrap()
    }

    fn doc_with_queue_and_exchange(queue_body: &str, response: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    fn queue_doc(body: &str) -> String {
        format!("<!-- agent:queue -->\n{body}<!-- /agent:queue -->\n")
    }

    #[test]
    fn pure_done_projection_strikes_matching_queue_prompts_only() {
        let content = queue_doc("- do [#alpha]\n- do [#beta]\n");
        let (updated, count) =
            mark_queue_prompts_completed_by_done_ids_in_content(&content, &["alpha".to_string()])
                .unwrap();

        assert_eq!(count, 1);
        assert!(updated.contains("- ~~do [#alpha]~~\n"));
        assert!(updated.contains("- do [#beta]\n"));
        assert!(
            done_ids_have_atomic_queue_completion(&content, &updated, &["alpha".to_string()],)
                .unwrap()
        );
        assert!(
            !done_ids_have_atomic_queue_completion(&content, &content, &["alpha".to_string()],)
                .unwrap()
        );
    }

    #[test]
    fn ipc_node_op_vocabulary_classifies_crdt_effect() {
        // consume + mark_done → not tombstone (QueueCrdt::mark_done).
        let consume = IpcNodeOp::consume("queue", "queue:0:a:0".into());
        assert_eq!(consume.kind(), NodeOpKind::Consume);
        assert!(!consume.is_tombstone());
        assert_eq!(consume.op, OP_CONSUME);

        let mark_done = IpcNodeOp::mark_done("queue", "queue:0:b:0".into());
        assert_eq!(mark_done.kind(), NodeOpKind::MarkDone);
        assert!(!mark_done.is_tombstone());
        assert_eq!(mark_done.op, OP_MARK_DONE);

        // strike → tombstone (QueueCrdt::strike).
        let strike = IpcNodeOp::strike("queue", "queue:0:c:0".into());
        assert_eq!(strike.kind(), NodeOpKind::Strike);
        assert!(strike.is_tombstone());
        assert_eq!(strike.op, OP_STRIKE);
    }

    #[test]
    fn ipc_node_op_node_key_accessor_and_json_round_trip() {
        let op = IpcNodeOp::mark_done("queue", "queue:0:alpha:0".into());
        assert_eq!(op.node_key(), "queue:0:alpha:0");
        let json = op.to_json();
        assert_eq!(json["op"], "mark_done");
        assert_eq!(json["component"], "queue");
        assert_eq!(json["node_id"], "queue:0:alpha:0");
    }

    #[test]
    fn node_op_helpers_build_correct_op_labels() {
        let keys = vec!["queue:0:a:0".to_string(), "queue:0:b:0".to_string()];
        let consume = queue_consume_node_ops(&keys);
        assert!(consume.iter().all(|op| op.op == OP_CONSUME));
        let mark_done = queue_mark_done_node_ops(&keys);
        assert!(mark_done.iter().all(|op| op.op == OP_MARK_DONE));
        let strike = queue_strike_node_ops(&keys);
        assert!(
            strike
                .iter()
                .all(|op| op.op == OP_STRIKE && op.is_tombstone())
        );
    }

    #[test]
    fn ipc_node_ops_map_to_node_patch_wire_format() {
        // consume + mark_done (strike-through/keep) -> MutationNodePatchOp::Strike.
        // strike (tombstone/remove) -> MutationNodePatchOp::Remove.
        let ops = vec![
            IpcNodeOp::consume("queue", "queue:0:a:0".into()),
            IpcNodeOp::mark_done("queue", "queue:0:b:0".into()),
            IpcNodeOp::strike("queue", "queue:0:c:0".into()),
        ];
        let patches = ipc_node_ops_to_node_patches(&ops);
        assert_eq!(patches.len(), 3);
        assert_eq!(patches[0].node_key, "queue:0:a:0");
        assert_eq!(patches[0].op, MutationNodePatchOp::Strike);
        assert_eq!(patches[1].op, MutationNodePatchOp::Strike);
        assert_eq!(patches[2].op, MutationNodePatchOp::Remove);
        assert!(
            patches
                .iter()
                .all(|p| p.content.is_none() && p.order.is_empty())
        );
    }

    #[test]
    fn apply_structural_ops_to_content_strikes_targeted_nodes() {
        // The detached single-writer projection: structural ops applied to content
        // produce the same strike-through as the per-site consume path, leaving
        // non-targeted nodes untouched.
        let content = queue_doc("- do [#alpha]\n- do [#beta]\n");
        let keys = id_backed_head_node_keys(&content, "beta").unwrap();
        let ops = queue_consume_node_ops(&keys);
        let struck = apply_structural_ops_to_content(&content, &ops).unwrap();
        assert!(
            struck.contains("~~do [#beta]~~"),
            "targeted node is struck-through:\n{struck}"
        );
        assert!(
            struck.contains("- do [#alpha]\n"),
            "non-targeted node is untouched:\n{struck}"
        );
    }

    #[test]
    fn apply_structural_ops_tombstone_removes_the_node() {
        let content = queue_doc("- do [#alpha]\n- do [#beta]\n");
        let keys = id_backed_head_node_keys(&content, "beta").unwrap();
        // A strike (tombstone) op removes the node entirely.
        let ops = queue_strike_node_ops(&keys);
        let removed = apply_structural_ops_to_content(&content, &ops).unwrap();
        assert!(!removed.contains("beta"), "tombstoned node is gone");
        assert!(removed.contains("- do [#alpha]\n"), "sibling survives");
    }

    #[test]
    fn replace_op_carries_content_and_maps_to_node_patch_replace() {
        let op = IpcNodeOp::replace("queue", "queue:0:a:0".into(), "~~struck~~ — note".into());
        assert_eq!(
            op.kind(),
            NodeOpKind::MarkDone,
            "replace is a mark_done effect"
        );
        assert!(!op.is_tombstone());
        assert_eq!(op.content.as_deref(), Some("~~struck~~ — note"));
        let patches = ipc_node_ops_to_node_patches(&[op]);
        assert_eq!(patches[0].op, MutationNodePatchOp::Replace);
        assert_eq!(patches[0].content.as_deref(), Some("~~struck~~ — note"));
    }

    #[test]
    fn node_replace_ops_from_diff_derives_per_node_replace() {
        // before: bare heads; after: one head struck+annotated. The diff yields a
        // single Replace op for the mutated node, and applying it reproduces after.
        let before = queue_doc("- do [#alpha]\n- a free-text report\n");
        let after = queue_doc("- do [#alpha]\n- ~~a free-text report~~\n");
        let ops = node_replace_ops_from_diff(&before, &after, "queue").unwrap();
        assert_eq!(ops.len(), 1, "only the mutated node yields an op");
        assert_eq!(ops[0].op, OP_REPLACE);
        // Applying the derived op to `before` reproduces `after` for the queue body.
        let applied = apply_structural_ops_to_content(&before, &ops).unwrap();
        assert_eq!(applied, after, "derived Replace op reproduces the target");
    }

    #[test]
    fn free_text_strike_node_ops_never_tombstone() {
        let content = queue_doc("- a plain free-text report\n- do [#alpha]\n");
        let ops = free_text_strike_node_ops(
            &content,
            "### Re: report\n\nAnswered the plain free-text report.\n",
            None,
        )
        .unwrap();
        // Whatever the free-text heuristic targets, the ops must be mark_done
        // (strike-through/keep), never tombstones.
        assert!(
            ops.iter().all(|op| !op.is_tombstone()),
            "free-text strike is mark_done, not remove"
        );
        assert!(ops.iter().all(|op| op.node_key().starts_with("queue:")));
    }

    #[test]
    fn id_backed_strike_node_ops_target_the_named_id() {
        let content = queue_doc("- do [#alpha]\n- do [#beta]\n");
        let ops = id_backed_strike_node_ops(&content, "beta").unwrap();
        assert_eq!(ops.len(), 1, "only the beta id-backed head is targeted");
        assert_eq!(ops[0].op, OP_CONSUME);
        assert!(!ops[0].is_tombstone());
    }

    #[test]
    fn noise_prune_node_ops_are_mark_done_not_remove() {
        let content = queue_doc("- $ ./run-tests.sh\n- do [#alpha]\n");
        let ops = noise_prune_node_ops(&content).unwrap();
        // The bulleted/orphan strike is mark_done (strike-through/keep); the
        // non-node-addressable multiline excise is intentionally NOT here.
        assert!(
            ops.iter().all(|op| !op.is_tombstone()),
            "bulleted noise strike is mark_done, not remove"
        );
        assert!(ops.iter().all(|op| op.node_key().starts_with("queue:")));
    }

    #[test]
    fn mark_done_node_ops_for_done_ids_target_completed_entries() {
        let content = queue_doc("- do [#alpha]\n- do [#beta]\n");
        let ops = mark_done_node_ops_for_done_ids(
            &content,
            &["alpha".to_string()],
            &["do [#alpha]".to_string()],
        );
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].op, OP_MARK_DONE);
        assert!(!ops[0].is_tombstone());
    }

    #[test]
    fn reconcile_postcommit_queue_strikes_restores_answered_pinned_and_free_text_heads() {
        let head = doc_with_queue_and_exchange(
            "- ~~:pushpin: do [#pzjy]~~\n- ~~plain queued report~~\n",
            "### Re: topic\n\nAnswered.",
        );
        let working = doc_with_queue_and_exchange(
            "- :pushpin: do [#pzjy]\n- plain queued report\n- do [#new]\n",
            "### Re: topic\n\nAnswered.",
        );

        let reconciled =
            reconcile_postcommit_queue_strikes_to_head(&working, &head).expect("queue repair");

        assert!(
            reconciled.contains("- ~~:pushpin: do [#pzjy]~~\n"),
            "pinned completed prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- ~~plain queued report~~\n"),
            "answered free-text prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- do [#new]\n"),
            "unrelated queue additions must remain live:\n{reconciled}"
        );
    }

    #[test]
    fn reconcile_postcommit_queue_strikes_does_not_unstrike_editor_completed_head() {
        let head =
            doc_with_queue_and_exchange("- a free-text head\n", "### Re: topic\n\nAnswered.");
        let working =
            doc_with_queue_and_exchange("- ~~a free-text head~~\n", "### Re: topic\n\nAnswered.");

        assert!(
            reconcile_postcommit_queue_strikes_to_head(&working, &head).is_none(),
            "editor-owned queue strike must remain editor-wins"
        );
    }

    #[test]
    fn first_n_queue_prompt_texts_skips_non_prompts_and_strips_in_progress_marker() {
        let body = format!(
            "--- stop\n- {} do [#head]\n- do [#tail]\n",
            IN_PROGRESS_MARKER,
        );
        let entries = entries(&body);

        assert_eq!(
            first_n_queue_prompt_texts(&entries, 1),
            vec!["do [#head]".to_string()]
        );
    }

    #[test]
    fn queue_consume_count_for_done_ids_counts_leading_matching_id_prompts_only() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- do [#tail]\n",
            "- do [#other]\n",
        ));

        assert_eq!(
            queue_consume_count_for_done_ids(&entries, &["tail".to_string(), "head".to_string()]),
            2
        );
        assert_eq!(
            queue_consume_count_for_done_ids(&entries, &["tail".to_string()]),
            0
        );
    }

    #[test]
    fn next_queue_head_selection_returns_none_without_queue_component() {
        let selection = next_queue_head_selection("plain document\n").unwrap();

        assert_eq!(selection, None);
    }

    #[test]
    fn next_queue_head_selection_returns_none_without_prompt() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "--- stop\n",
            "<!-- /agent:queue -->\n",
        );

        let selection = next_queue_head_selection(content).unwrap();

        assert_eq!(selection, None);
    }

    #[test]
    fn next_queue_head_selection_reports_stop_fence_at_head() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "--- stop\n",
            "- do [#blocked]\n",
            "<!-- /agent:queue -->\n",
        );

        let selection = next_queue_head_selection(content)
            .unwrap()
            .expect("selection");

        assert_eq!(selection.head_text, "do [#blocked]");
        assert!(selection.stop_fence_at_head);
        assert!(!selection.node_key.is_empty());
    }

    #[test]
    fn foreign_exchange_prompt_detection_ignores_prefix_flip_on_baseline_prompt() {
        let head = "Evaluate axocoatl thing";
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        let prefix_flip = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "### Re: axocoatl\n",
            "plan written.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );

        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), prefix_flip, head),
            "a prompt-prefix flip on an already-answered baseline prompt is not new foreign work"
        );
    }

    #[test]
    fn foreign_exchange_prompt_detection_distinguishes_drain_and_new_prompt() {
        let head = "lazily-rs plan-update";
        let baseline = "\
---
agent_doc_format: template
queue_active: true
---

<!-- agent:exchange -->
### Re: older
Old.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- lazily-rs plan-update
<!-- /agent:queue -->
";
        let drain = baseline.replace(
            "<!-- agent:boundary:x -->",
            "### Re: updated the plan\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), &drain, head),
            "a drain cycle with only a new response is not foreign work"
        );

        let foreign = baseline.replace(
            "<!-- agent:boundary:x -->",
            "❯ Fix the JB cache conflict instead\n### Re: fix jb\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), &foreign, head),
            "a genuinely new unrelated exchange prompt is foreign work"
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_marks_matching_live_prompts_only() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- do [#opportunistic]\n",
            "- ~~do [#already]~~\n",
            "- do [#tail]\n",
        ));

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);

        assert_eq!(marked, vec!["do [#opportunistic]".to_string()]);
        assert_eq!(
            document_queue::render(&updated),
            concat!(
                "- do [#head]\n",
                "- ~~do [#opportunistic]~~\n",
                "- ~~do [#already]~~\n",
                "- do [#tail]\n",
            )
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_marks_all_sibling_lines_for_one_resolved_id() {
        let entries = entries(concat!(
            "- [#8667]\n",
            "- do [#liveone]\n",
            "- do [#8667] follow-up\n",
        ));

        let (updated, marked) = mark_entries_completed_by_done_ids(&entries, &["8667".into()]);

        assert_eq!(
            marked,
            vec!["[#8667]".to_string(), "do [#8667] follow-up".to_string()],
            "every live queue prompt referencing the resolved id should be marked"
        );
        assert_eq!(
            document_queue::render(&updated),
            concat!(
                "- ~~[#8667]~~\n",
                "- do [#liveone]\n",
                "- ~~do [#8667] follow-up~~\n",
            )
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_marks_review_gated_items() {
        let entries = entries("- do [#gatedphase]\n- do [#stillopen]\n");

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["gatedphase".into()]);

        assert_eq!(marked, vec!["do [#gatedphase]".to_string()]);
        assert_eq!(
            document_queue::render(&updated),
            "- ~~do [#gatedphase]~~\n- do [#stillopen]\n"
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_ignores_already_completed_prompt() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- ~~do [#opportunistic]~~\n",
            "- do [#tail]\n",
        ));

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);
        assert!(marked.is_empty());
        assert_eq!(updated, entries);
    }

    #[test]
    fn mark_first_matching_prompts_completed_by_texts_matches_priority_marker_identity() {
        let entries = entries("- :pushpin: deploy\n- do [#tail]\n");

        let updated =
            mark_first_matching_prompts_completed_by_texts(&entries, &["deploy".to_string()])
                .unwrap();

        assert_eq!(
            document_queue::render(&updated),
            "- ~~:pushpin: deploy~~\n- do [#tail]\n"
        );
    }

    #[test]
    fn normalized_done_id_bag_sorts_ids_from_prompt_texts() {
        assert_eq!(
            normalized_done_id_bag(&["do [#b]".to_string(), "do #a".to_string()]),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn consume_queue_nodes_by_key_strips_in_progress_marker_before_strike_text() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- 🚧 do [#head]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        let mut keys = queue_prompt_node_keys_for_count(content, 1).unwrap().keys;
        let key = keys.remove(0);

        let updated = consume_queue_nodes_by_key(content, &[key]).unwrap();

        assert!(updated.contains("- ~~do [#head]~~\n"), "{updated}");
        assert!(!updated.contains("~~🚧"), "{updated}");
        assert!(updated.contains("- do [#tail]\n"), "{updated}");
    }

    #[test]
    fn id_backed_head_node_keys_targets_only_id_backed_heads() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- do [#orphangone]\n",
            "- Please keep [#orphangone] visible in the report\n",
            "- do [#other]\n",
            "<!-- /agent:queue -->\n",
        );

        let keys = id_backed_head_node_keys(content, "orphangone").unwrap();

        assert_eq!(keys.len(), 1, "only the exact id directive is targetable");
        let updated = consume_queue_nodes_by_key(content, &keys).unwrap();
        assert!(updated.contains("- ~~do [#orphangone]~~\n"), "{updated}");
        assert!(
            updated.contains("- Please keep [#orphangone] visible in the report\n"),
            "{updated}"
        );
        assert!(updated.contains("- do [#other]\n"), "{updated}");
    }

    #[test]
    fn strike_all_noise_queue_heads_strikes_noise_and_orphan_id_heads_only() {
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- [route] target tmux session: 0\n",
            "- :pushpin: [#orphan]\n",
            "- :pushpin: do [#focused]\n",
            "- fix the tokenizer now\n",
            "- do [#keepme]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] real open work\n",
            "- [ ] [#focused] [focused-cycle] dedicated cycle work\n",
            "<!-- /agent:backlog -->\n",
        );

        let (planned, struck) = strike_all_noise_queue_heads(content).unwrap();

        assert_eq!(struck, 2, "{planned}");
        assert!(
            planned.contains("~~[route] target tmux session: 0~~"),
            "{planned}"
        );
        assert!(
            planned.contains("~~:pushpin: [#orphan]~~") || planned.contains("~~[#orphan]~~"),
            "{planned}"
        );
        assert!(planned.contains("- :pushpin: do [#focused]\n"), "{planned}");
        assert!(planned.contains("- fix the tokenizer now\n"), "{planned}");
        assert!(planned.contains("- do [#keepme]\n"), "{planned}");
    }

    #[test]
    fn strike_all_noise_queue_heads_excises_multiline_log_blocks() {
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" go -->\n",
            "- [#keepme]\n",
            "---\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: dispatch blocked: only the gated #5eq8 remains.\n",
            "```\n",
            "---\n",
            "---\n",
            ":pushpin: JB `Run Agent Doc` report that should remain.\n",
            "```\n",
            "Error: diagnostic evidence\n",
            "```\n",
            "---\n",
            "<!-- /agent:queue -->\n",
        );

        let (planned, struck) = strike_all_noise_queue_heads(content).unwrap();

        assert_eq!(struck, 1, "{planned}");
        assert!(planned.contains("- [#keepme]\n"), "{planned}");
        assert!(
            !planned.contains("[route] target tmux session: 0"),
            "{planned}"
        );
        assert!(!planned.contains("#5eq8"), "{planned}");
        assert!(
            planned.contains("JB `Run Agent Doc` report that should remain."),
            "{planned}"
        );
        assert!(planned.contains("Error: diagnostic evidence"), "{planned}");
    }

    #[test]
    fn annotate_struck_free_text_line_is_idempotent_and_targeted() {
        assert_eq!(
            annotate_struck_free_text_line("- ~~foo~~"),
            "- ~~foo~~ — auto-struck: answered this cycle (#ftstrike)"
        );
        let once = annotate_struck_free_text_line("- ~~foo~~");
        assert_eq!(annotate_struck_free_text_line(&once), once);
        assert_eq!(annotate_struck_free_text_line("- foo"), "- foo");
        assert_eq!(
            annotate_struck_free_text_line("~~bar baz~~"),
            "~~bar baz~~ — auto-struck: answered this cycle (#ftstrike)"
        );
        assert_eq!(
            annotate_struck_free_text_line("- ~~foo~~\n"),
            "- ~~foo~~ — auto-struck: answered this cycle (#ftstrike)\n"
        );
        assert_eq!(annotate_struck_free_text_line("- ~~~~"), "- ~~~~");
    }

    #[test]
    fn annotated_struck_line_still_parses_as_struck_node() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- ~~answered free-text head~~ — auto-struck: answered this cycle (#ftstrike)\n",
            "<!-- /agent:queue -->\n",
        );
        let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].item.struck, "annotated head must parse as struck");
        assert_eq!(
            nodes[0].item.text.trim(),
            "answered free-text head",
            "the inner text must exclude both the wrapper and the note"
        );
    }

    #[test]
    fn annotate_newly_struck_free_text_heads_only_marks_new_queue_strikes() {
        let before = concat!(
            "<!-- agent:queue -->\n",
            "- open report\n",
            "- ~~old report~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:exchange -->\n",
            "unchanged\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:queue -->\n",
            "- ~~open report~~\n",
            "- ~~old report~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:exchange -->\n",
            "unchanged\n",
            "<!-- /agent:exchange -->\n",
        );

        let annotated = annotate_newly_struck_free_text_heads(before, after).unwrap();

        assert!(
            annotated.contains("- ~~open report~~ — auto-struck: answered this cycle (#ftstrike)")
        );
        assert!(annotated.contains("- ~~old report~~\n"));
        assert!(annotated.contains("<!-- agent:exchange -->\nunchanged\n<!-- /agent:exchange -->"));
        assert_eq!(
            annotate_newly_struck_free_text_heads(after, &annotated).unwrap(),
            annotated
        );
    }

    #[test]
    fn answered_free_text_strike_is_a_pure_authority_projection() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-projection\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: staging deploy — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> staging deploy\n\n",
            "Deployed and verified.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- staging deploy\n",
            "- do [#production-deploy]\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: staging deploy — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> staging deploy\n\n",
            "Deployed and verified.\n",
        );

        let projected = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("answered free-text head should project a target");

        assert_eq!(projected.node_keys.len(), 1);
        assert!(
            projected
                .target_content
                .contains("- ~~staging deploy~~ — auto-struck: answered this cycle (#ftstrike)")
        );
        assert!(
            projected
                .target_content
                .contains("- do [#production-deploy]")
        );
        assert!(
            project_answered_free_text_strike(&projected.target_content, response, Some(document))
                .unwrap()
                .is_none(),
            "the projection must disappear once its target is authoritative"
        );
    }

    #[test]
    fn answered_free_text_projection_heals_torn_inactive_queue_flag() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-projection\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: completed work — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> completed work\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- completed work\n",
            "- remaining work\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: completed work — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> completed work\n\n",
            "Done.\n",
        );

        let projected = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("captured response should heal and advance the torn queue projection");

        assert!(projected.target_content.contains("queue: start"));
        assert!(
            projected
                .target_content
                .contains("- ~~completed work~~ — auto-struck: answered this cycle (#ftstrike)")
        );
        assert!(projected.target_content.contains("- remaining work"));
    }
}
