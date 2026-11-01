//! Pure queue prompt consumption policy.
//!
//! This module owns entry-level consume planning helpers. Callers still own
//! file IO, snapshot persistence, IPC transport, and editor convergence.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use agent_doc_document::queue_projection::{
    LEGACY_OWNER_ANNOTATION_OPEN, OWNER_ANNOTATION_OPEN, lifecycle_markers,
    strip_in_progress_marker, strip_owner_annotation_prefix, strip_priority_markers,
};
use agent_doc_document::write_normalization::strip_boundary_for_dedup;
use agent_doc_element::element;
use agent_doc_element_queue::struck_work_text;
use agent_doc_markdown_ast::mutations::{MutationError, MutationNodePatch, MutationNodePatchOp};
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

/// Searchable backlink from a completed managed-list prompt to the stable
/// exchange response cell that answered it.
pub const RESPONSE_ID_NOTE_PREFIX: &str = "response: ";

/// Compatibility timestamp for pure callers that have not yet supplied the
/// cycle's durable addressed-at fact. Runtime adapters use the explicit `_at`
/// APIs; this keeps legacy projections deterministic instead of reading time.
const FALLBACK_RESPONSE_TIMESTAMP: &str = "1970-01-01T00:00:00.000Z";

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

/// Retire the transient binary completion cells whose queue items this
/// closeout proves integrated. The attempt keys are returned so I/O adapters
/// can fence retained editor delivery on both the queue strike and cell
/// retirement reaching the authoritative document.
pub fn retire_consumed_completion_feedback(
    content: &str,
    consumed_heads: &[String],
) -> (String, HashSet<String>) {
    let mut retired_keys = HashSet::new();
    let retired = agent_doc_document::subagent_completion::strip_completion_cells_matching(
        content,
        |evidence| {
            // The cell names the row's own work, not every row that shares its
            // tracked id (`#queueworkevidence`).
            let matches = consumed_heads.iter().any(|head| {
                crate::queue_claim::completion_evidence_matches_row(&evidence.item, head)
            });
            if matches {
                retired_keys.insert(evidence.key.clone());
            }
            matches
        },
    );
    (retired, retired_keys)
}

/// The finished work a strike successor closes (`#strikeretires`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StruckRowRetirement {
    /// `after` without the completion cells of the rows it struck.
    pub content: String,
    /// The struck rows' work texts (markers stripped), in document order.
    pub struck_heads: Vec<String>,
    /// Keys of the completion cells retired from `after`.
    pub retired_keys: HashSet<String>,
}

fn queue_rows(content: &str) -> Vec<(bool, String)> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    components
        .iter()
        .filter(|component| component.name == "queue")
        .filter_map(|component| document_queue::parse(component.content(content)).ok())
        .flatten()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some((false, prompt.text)),
            QueueEntry::Completed(prompt) => Some((true, prompt.text)),
            _ => None,
        })
        .map(|(struck, text)| {
            if struck {
                (true, struck_row_work_text(&text))
            } else {
                (false, row_work_text(&text))
            }
        })
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

fn row_work_text(text: &str) -> String {
    strip_in_progress_marker(&strip_priority_markers(text.trim()))
        .trim()
        .to_string()
}

/// The work text of a struck (completed) queue row: markers removed, and a
/// strike that bakes its note inside the strikethrough
/// (`~~<work> — auto-struck: <reason>~~`, `#qftbklgstrike`) still struck
/// `<work>`; the note is never part of the work identity.
pub fn struck_row_work_text(text: &str) -> String {
    row_work_text(struck_text_without_annotation(text))
}

/// A struck row's work text without a trailing auto-struck note
/// ([`STRUCK_ANNOTATION_SEPARATOR`]). The outside-the-wrapper note shape is
/// already dropped by the queue parser; this covers the note baked inside
/// the wrapper.
///
/// [`STRUCK_ANNOTATION_SEPARATOR`]: agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
fn struck_text_without_annotation(text: &str) -> &str {
    match text.split_once(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR) {
        Some((work, _)) if !work.trim().is_empty() => work,
        _ => text,
    }
}

fn live_identities(rows: &[(bool, String)]) -> HashSet<agent_doc_element_queue::QueueItemIdentity> {
    rows.iter()
        .filter(|(struck, _)| !struck)
        .map(|(_, text)| crate::queue_claim::claim_identity(text))
        .collect()
}

/// Identities of every live row in every queue component of `content`.
pub fn live_work_identities(content: &str) -> HashSet<agent_doc_element_queue::QueueItemIdentity> {
    live_identities(&queue_rows(content))
}

/// Work rows that the successor `after` strikes relative to `before`
/// (`#strikeretires`): a row whose identity is live in `before`, has no live
/// row left in `after`, and is struck in `after`. A row the successor
/// deleted is not here (removal is cancellation, `#queue-removal-cancels`),
/// and a work identity that still has a live row anywhere (a sibling queue's
/// mirror copy, a duplicate) keeps its evidence for that row. Identity is the
/// marker-invariant claim identity, across every queue component.
pub fn heads_struck_by_successor(before: &str, after: &str) -> Vec<String> {
    let before_live = live_work_identities(before);
    if before_live.is_empty() {
        return Vec::new();
    }
    let after_rows = queue_rows(after);
    let after_live = live_identities(&after_rows);
    let mut seen = HashSet::new();
    after_rows
        .into_iter()
        .filter(|(struck, _)| *struck)
        .filter(|(_, text)| {
            let identity = crate::queue_claim::claim_identity(text);
            before_live.contains(&identity)
                && !after_live.contains(&identity)
                && seen.insert(identity)
        })
        .map(|(_, text)| text)
        .collect()
}

/// Retire the completion cells of every row the successor `after` strikes
/// relative to `before`, in that same successor (`#strikeretires`).
///
/// Every binary strike path (closeout echo, preflight residue/backlog/done
/// strike, the orphan `queue consume --id` strike, the controller's
/// answered-head projection) closes finished work. Its transient completion
/// cell names the row's own work text (`completion_evidence_matches_row`), so
/// a cell that outlives the strike would project `🔄` onto a later row with
/// that text. A longer line that only shares the tracked id is not that row.
/// Only structurally attested cells are removed
/// ([`retire_consumed_completion_feedback`]); operator text and heading-shaped
/// prose are never touched.
pub fn retire_struck_rows_completion_feedback(before: &str, after: &str) -> StruckRowRetirement {
    let struck_heads = heads_struck_by_successor(before, after);
    if struck_heads.is_empty() {
        return StruckRowRetirement {
            content: after.to_string(),
            ..StruckRowRetirement::default()
        };
    }
    let (content, retired_keys) = retire_consumed_completion_feedback(after, &struck_heads);
    StruckRowRetirement {
        content,
        struck_heads,
        retired_keys,
    }
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

/// The queue head a response closeout judged answered, captured from the SAME
/// document the consumption decision read (`#qconsumeapprovedhead`, gh#240).
///
/// A free-text closeout decides "consume" against its pre-write document but
/// the consume re-reads the live document. Between the two, another closeout
/// driver (a captured-finalize resume's repair pass, an `#ftstrike` effect) can
/// strike the answered head, so "the leading live head" at consume time is the
/// NEXT operator prompt, which no response answered. Draining it also cleared
/// `queue: go`. The consume therefore binds to this identity, never to
/// "whatever is the head now".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovedQueueHead {
    /// The decision read no live head, so a free-text consume has nothing it
    /// is entitled to take.
    NoHead,
    /// The decision approved exactly this head.
    Head {
        /// Head text with the in-progress and priority markers removed.
        text: String,
        /// Strike-stable node identity (`{item id}:{occurrence}`, without the
        /// component index), when the queue is AST-addressable. Distinguishes
        /// two heads with identical text.
        node_identity: Option<String>,
    },
}

/// Capture the head a consume decision is about to approve, from the exact
/// content that decision read.
pub fn approved_queue_head_for_consume(content: &str) -> Result<ApprovedQueueHead> {
    let Some(head) = crate::queue_heads::active_queue_head_text(content)? else {
        return Ok(ApprovedQueueHead::NoHead);
    };
    let node_identity = queue_prompt_node_keys_for_count(content, 1)
        .ok()
        .filter(|keys| keys.ast_backed)
        .and_then(|keys| keys.keys.into_iter().next())
        .map(|key| queue_node_identity_without_component_index(&key));
    Ok(ApprovedQueueHead::Head {
        text: approved_head_text(&head),
        node_identity,
    })
}

fn approved_head_text(text: &str) -> String {
    strip_priority_markers(&strip_in_progress_marker(text))
        .trim()
        .to_string()
}

/// `queue:{component index}:{item id}:{occurrence}` -> `{item id}:{occurrence}`.
/// The component index is positional across ALL components, so an unrelated
/// component inserted above the queue between decision and consume must not
/// read as a different head.
pub fn queue_node_identity_without_component_index(node_key: &str) -> String {
    let mut parts = node_key.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_component), Some(index), Some(rest)) if index.parse::<usize>().is_ok() => {
            rest.to_string()
        }
        _ => node_key.to_string(),
    }
}

/// Whether the head a consume plan is about to take is the head the closeout
/// approved. Text must match after marker normalization; when both sides carry
/// a node identity it must match too.
pub fn approved_queue_head_matches(
    approved: &ApprovedQueueHead,
    consumed_text: &str,
    consumed_node_key: Option<&str>,
) -> bool {
    let ApprovedQueueHead::Head {
        text,
        node_identity,
    } = approved
    else {
        return false;
    };
    if *text != approved_head_text(consumed_text) {
        return false;
    }
    match (node_identity.as_deref(), consumed_node_key) {
        (Some(approved), Some(consumed)) => {
            approved == queue_node_identity_without_component_index(consumed)
        }
        _ => true,
    }
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
    document_queue::prompts(entries)
        .into_iter()
        .map(|prompt| strip_in_progress_marker(&prompt.text))
        .take(count)
        .collect()
}

pub fn next_queue_head_selection(content: &str) -> Result<Option<NextQueueHeadSelection>> {
    let components = element::parse(content)?;
    let Some(queue_component) = crate::queue_set::selected_component(content, &components)? else {
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
    let has_response = !response_body.trim().is_empty();
    let active_head = if has_response {
        crate::queue_heads::active_queue_head_text(current_content)?
    } else {
        None
    };
    // `#operatoractionstrike`: `🙋 [#id]` is the document projection of a
    // still-open operator todo. It is an orthogonal lifecycle gate, so no answer
    // echo, tracked completion id, or other closeout signal may consume the row
    // while the annotation remains. Completing the todo clears this projection
    // through `set_prompt_operator_actions`, after which ordinary completion
    // evidence is authoritative again.
    // `#answeredrequeststrike`: only an OPEN request holds the row (a visible
    // `🙋` or a still-open linked todo); an answered request whose todo closed
    // is answer provenance, not a hold.
    if active_head.as_ref().is_some_and(|head| {
        agent_doc_document::queue_projection::operator_action_holds_row(
            head,
            &agent_doc_element::todo::open_todo_ids(current_content),
        )
    }) {
        return Ok(false);
    }
    let head_is_free_text = active_head.is_some()
        && crate::queue_response::queue_head_is_free_text_prompt(current_content)?;
    let active_free_text_head = active_head.clone().filter(|_| head_is_free_text);
    // `#deferstrike-closeout`: deferral is a veto over every completion signal.
    // In particular, the explicit queue-prompt echo below is also the documented
    // way to quote deferred work, so it cannot authorize consumption first.
    if active_head.as_ref().is_some_and(|head_text| {
        crate::queue_response::response_defers_queue_head(current_content, response_body, head_text)
    }) {
        if head_is_free_text {
            return Ok(false);
        }
        // `#annotateddeferstrike`: an id-backed head (`do [#id]: note`) whose
        // echo is followed by a deferral is kept against every IMPLICIT signal
        // (prompt diff, `### Re: #id` heading). Only an explicit closeout flag
        // naming the head (`--done`/`--pending-gate`/`--review-resolve`)
        // resolves it, because that flag resolves the backlog item itself.
        return crate::queue_heads::queue_head_has_explicit_completion_signal(
            current_content,
            completion_ids,
        );
    }
    if should_consume_queue_prompt_for_write(file, baseline, current_content, completion_ids)? {
        return Ok(true);
    }
    if crate::queue_heads::queue_head_has_explicit_completion_signal(
        current_content,
        completion_ids,
    )? {
        return Ok(true);
    }
    if has_response && response_targets_synthetic_queue_head_id(current_content, response_body)? {
        return Ok(true);
    }
    if let Some(head_text) = active_free_text_head {
        let answered = crate::queue_response::queue_head_answered_by_response(
            current_content,
            response_body,
            &head_text,
        );
        return Ok(answered
            && !cycle_answered_foreign_exchange_prompt(baseline, current_content, &head_text));
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
    // `#annotateddeferstrike`: a response that echoes the head and then defers
    // it did not complete it, so neither its `### Re:` heading nor the prompt
    // diff may turn into a completion id. An explicit `--done` naming the head
    // still does.
    if !response_body.trim().is_empty()
        && crate::queue_response::response_defers_queue_head(
            current_content,
            response_body,
            &queue_head,
        )
    {
        return Ok(
            crate::queue_heads::queue_head_matches_done_ids(current_content, pending_done)?
                .then_some(head_id),
        );
    }
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

/// Node keys of the non-struck free-text queue heads that this cycle answered,
/// at any position in the queue.
///
/// `#queuefreetextdup`: the queue is a scheduling FIFO, so an identical line
/// repeated later is an intentional "run it again". One answered quote strikes
/// only the FIRST unstruck occurrence of that text; later repeats stay live for
/// their own cycle. Striking every copy (`nodes=2` on 2026-09-30) silently
/// dropped the operator's second `release + publish`.
pub fn answered_free_text_head_node_keys(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
) -> Result<Vec<String>> {
    answered_free_text_head_node_keys_with_claims(content, response_body, baseline, &[])
}

/// [`answered_free_text_head_node_keys`] judging each head in `claimed_heads`
/// (live heads an active worker claim holds) by the claimed-head proof
/// (`#ftstrikeclaimedmention`, see
/// [`crate::queue_response::queue_head_answered_by_response_with_claim`]): only
/// an exact agent-authored `> **Queue prompt:**` echo strikes a claimed head.
/// A status response that merely mentions claimed heads, or quotes their
/// shared preset body, strikes none of them.
pub fn answered_free_text_head_node_keys_with_claims(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
) -> Result<Vec<String>> {
    answered_free_text_head_node_keys_with_authority(
        content,
        response_body,
        baseline,
        claimed_heads,
        &[],
    )
}

/// `queue:<n>:` node-key prefixes of every `loop` queue in `content`.
///
/// `#ftstrikeloopindex`: `<n>` must be counted in the SAME component list the
/// node keys come from. [`agent_doc_markdown_ast::mutations::item_nodes_all`]
/// numbers components over [`agent_doc_markdown_ast::overlay::components`],
/// which counts an `agent:boundary:<id>` marker as a component, while
/// [`element::parse`] does not. Indexing with `element::parse` shifted every
/// component after the exchange boundary by one, so in a document holding a
/// run queue followed by a loop queue the loop's prefix named the RUN queue:
/// GH #361's loop exclusion then skipped every ordinary row, and no echoed
/// head struck (agent-doc-bugs.md cycle-1793042953867, 2026-10-26).
fn loop_queue_node_prefixes(content: &str) -> HashSet<String> {
    agent_doc_markdown_ast::overlay::components(content)
        .into_iter()
        .enumerate()
        .filter(|(_, component)| {
            component.name == "queue"
                && crate::queue_schedule::queue_retains_struck_heads(&element::parse_attrs(
                    &component.attrs,
                ))
        })
        .map(|(component_index, _)| format!("queue:{component_index}:"))
        .collect()
}

/// [`answered_free_text_head_node_keys_with_claims`] plus the free-text heads
/// selected by this cycle's preflight. Selection is durable binary authority,
/// unlike the cosmetic `🚧` projection in `content`, so an exactly echoed and
/// answered selected head may close even when queue projection introduced it
/// after the response baseline was captured.
pub fn answered_free_text_head_node_keys_with_authority(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
) -> Result<Vec<String>> {
    Ok(answered_free_text_strike_decision(
        content,
        response_body,
        baseline,
        claimed_heads,
        selected_heads,
        &[],
    )?
    .node_keys)
}

/// Which answered free-text heads strike, and which answered heads the
/// subagents-dispatch veto kept queued (`#subagentinlineveto`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnsweredFreeTextStrikeDecision {
    /// Node keys of the heads that strike.
    pub node_keys: Vec<String>,
    /// Texts of heads the response answered inline although the queue's
    /// `subagents` attribute owns them for dispatch and no worker claim attempt
    /// exists. They stay queued and the caller reports them loudly.
    pub inline_vetoed_heads: Vec<String>,
}

/// Whether one mutation node is context for a deeper queue row.
///
/// The markdown overlay exposes every nested list row as its own mutation
/// node.  A parent row is therefore recognizable from the indentation of the
/// following rows in the same component: the first list row before the tree
/// returns to this depth makes this row context-only.  Keep this check beside
/// the reactive strike projection so an answer can close leaves without ever
/// consuming their parent context.
fn mutation_node_is_group_parent(
    source: &str,
    nodes: &[agent_doc_markdown_ast::mutations::MutationItemNode],
    index: usize,
) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let indent = |node: &agent_doc_markdown_ast::mutations::MutationItemNode| {
        (node.item.surface == agent_doc_markdown_ast::overlay::ItemSurface::ListItem).then(|| {
            source[node.item.start_byte..]
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .count()
        })
    };
    let Some(own_indent) = indent(node) else {
        return false;
    };
    for later in nodes.iter().skip(index + 1) {
        // Item indexes restart at each component occurrence.
        if later.index <= node.index {
            break;
        }
        match indent(later) {
            Some(later_indent) if later_indent > own_indent => return true,
            Some(_) => return false,
            None => continue,
        }
    }
    false
}

/// [`answered_free_text_head_node_keys_with_authority`] plus the
/// `#subagentinlineveto` rule.
///
/// `dispatch_owned_heads` are live heads a queue-level `subagents` / `fan-out`
/// attribute owns for dispatch (the cycle's `queue_subagent_dispatch` items
/// and capacity-held heads) that no worker claim attempt, reported-release
/// receipt, or completion cell backs. Such a head is never closed by an inline
/// answer: the coordinator must claim it and hand it to a subagent. An echo of
/// one is reported in [`AnsweredFreeTextStrikeDecision::inline_vetoed_heads`]
/// instead of striking, so the row is offered for dispatch again next cycle.
/// A head in `claimed_heads` is backed by a worker attempt and keeps the
/// ordinary `#claimstrike` behaviour.
pub fn answered_free_text_strike_decision(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
    dispatch_owned_heads: &[String],
) -> Result<AnsweredFreeTextStrikeDecision> {
    let mut decision = AnsweredFreeTextStrikeDecision::default();
    let dispatch_owned =
        crate::queue_claim::ClaimedQueueItems::none().with_heads(dispatch_owned_heads);
    let claimed = crate::queue_claim::ClaimedQueueItems::none().with_heads(claimed_heads);
    let selected = crate::queue_claim::ClaimedQueueItems::none().with_heads(selected_heads);
    // `#claimstrike` (live, 2026-10-05): a worker claim does NOT hold an
    // answered head back. A claim says who owns the work; the response's
    // `> **Queue prompt:**` echo says that work is finished, and the
    // `#ftstrikedefer` deferral detection below is what keeps a quoted head
    // that is still outstanding (dispatched, `**Deferred:**`) queued. Skipping
    // claimed heads here (`#deferstrike`) diverged from the session-check
    // residue guard, which judges the same echo without claims: the cycle
    // committed with the head unstruck, then session-check INTERRUPTED on
    // completed residue (`#qheadresidue`). The struck head's claim is released
    // by the caller (finalize) or pruned as closed at closeout reconciliation.
    if response_body.trim().is_empty() {
        return Ok(decision);
    }
    let nodes = match agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue") {
        Ok(nodes) => nodes,
        // `#ftstrike-noqueue`: queue is an optional document component. An
        // answered response in a document without one has no queue lifecycle
        // state to project; absence is therefore an empty projection, not a
        // failed mutation. Preserve every other AST/mutation error so malformed
        // queue state remains visible to the operator.
        Err(MutationError::ComponentNotFound(component)) if component == "queue" => {
            return Ok(decision);
        }
        Err(err) => {
            return Err(anyhow::anyhow!(
                "free-text strike: failed to derive queue node keys: {err}"
            ));
        }
    };
    // GH #361: `#ftstrike` is a document-wide catch-up projection for ordinary
    // queues. A loop queue has its own pass-scoped consume/restore lifecycle;
    // striking one of its rows because another queue echoed identical text can
    // leave that standing program struck until a later due tick. Exclude every
    // loop component here. Its selected head is still struck by normal queue
    // consumption and restored from the pass roster when that pass completes.
    let current_loop_queue_node_prefixes = loop_queue_node_prefixes(content);
    let is_loop_queue_node = |node_key: &str| {
        current_loop_queue_node_prefixes
            .iter()
            .any(|prefix| node_key.starts_with(prefix))
    };
    // `#ftstrikeonce` (live, agent-doc-bugs.md 2026-09-30): queue consume and
    // this answered-head projection both act on the same answer. When consume
    // had already struck the answered copy this cycle, the "first unstruck
    // occurrence" here was the operator's NEXT repeat, so one answer struck two
    // lines and silently dropped a queued rerun. A text with more struck copies
    // now than in the baseline has already had its answer applied.
    let struck_counts = |doc: &str| -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        let loop_prefixes = loop_queue_node_prefixes(doc);
        if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(doc, "queue") {
            for node in nodes.into_iter().filter(|node| {
                node.item.struck
                    && !loop_prefixes
                        .iter()
                        .any(|prefix| node.node_key.starts_with(prefix))
            }) {
                *counts
                    .entry(normalize_queue_prompt_text(node.item.text.trim()))
                    .or_insert(0) += 1;
            }
        }
        counts
    };
    let already_applied: HashSet<String> = match baseline {
        Some(baseline) => {
            let before = struck_counts(baseline);
            struck_counts(content)
                .into_iter()
                .filter(|(text, now)| *now > before.get(text).copied().unwrap_or(0))
                .map(|(text, _)| text)
                .collect()
        }
        None => HashSet::new(),
    };
    let mut struck_texts = already_applied;
    let open_todo_ids = agent_doc_element::todo::open_todo_ids(content);
    for (node_index, node) in nodes.iter().enumerate() {
        if node.item.struck || is_loop_queue_node(&node.node_key) {
            continue;
        }
        // A row with descendants is their context, not executable work.  Its
        // prose may appear in a submitted nest vector, but that must never turn
        // the parent into an answered queue head.
        if mutation_node_is_group_parent(content, &nodes, node_index) {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !crate::queue_response::queue_prompt_text_is_free_text(content, text)
        {
            continue;
        }
        // `#operatoractionstrike`: the captured response can present a blocking
        // operator request and quote the queue head in the same cycle. The quote
        // is answer evidence for ordinary free text, but an open `🙋 [#id]`
        // annotation is stronger evidence that the row is not terminal yet.
        // This check belongs in the reactive projection itself so every adapter
        // observes the same invariant; todo completion removes the annotation
        // and invalidates/recomputes this derived target normally.
        // `#answeredrequeststrike`: an answered request whose todo closed no
        // longer holds the row, matching the `🙋` projection.
        if agent_doc_document::queue_projection::operator_action_holds_row(text, &open_todo_ids) {
            continue;
        }
        if struck_texts.contains(&normalize_queue_prompt_text(text)) {
            continue;
        }
        // `#bugautostruck`: the in-progress marker proves only which queue head
        // was selected for this cycle. It is not evidence that the response
        // addressed that head. Require exact quoted-prompt proof for ordinary
        // free text, or resolved-expansion proof for a prompt-preset head.
        let has_explicit_answer_evidence =
            crate::queue_response::free_text_head_has_explicit_answer_evidence(response_body, text)
                || crate::queue_response::response_explicitly_echoes_nest_vector(
                    content,
                    response_body,
                    text,
                );
        // `#adresponsechildlive`: this projection scans every queue row. Do
        // not let a later row close merely because closeout prose, a `### Re:`
        // heading, or an unlabeled quote names it. Claimed work remains on the
        // stricter exact-echo contract; unclaimed registered presets may still
        // use their resolved-expansion proof.
        let has_conservative_strike_evidence = if claimed.claims(text) {
            has_explicit_answer_evidence
        } else {
            crate::queue_response::free_text_head_has_conservative_strike_evidence(
                content,
                response_body,
                text,
            )
        };
        if !has_conservative_strike_evidence {
            continue;
        }
        if !crate::queue_response::queue_head_answered_by_response_with_claim(
            content,
            response_body,
            text,
            claimed.claims(text),
        ) {
            continue;
        }
        // `#ftstrikedefer`: the quoted echo is the responding agent's assertion
        // that it answered the head, not proof. When the response says next to
        // that quote that the head is still outstanding, believe the sentence
        // over the echo and leave it queued. A nest-vector quote is that leaf's
        // echo (`#nestvector`).
        if crate::queue_response::response_defers_queue_head(content, response_body, text) {
            continue;
        }
        // Formal post-baseline closeout rule:
        //
        //   strike(q) => echo(q) && !deferred(q)
        //                && (baseline(q) || exact_worker_authority(q)
        //                    || preflight_selection_authority(q))
        //
        // `claimed` is the caller-supplied set of heads backed by current
        // claim-attempt authority (a live claim or an exact-attempt release
        // receipt). `selected` is the cycle-state projection recorded by
        // preflight. A cosmetic lifecycle prefix is never authority: accepting
        // raw `🚧` here allowed a hand-edited/stale marker to consume new
        // operator work, while rejecting binary-recorded selection made an
        // exactly answered projected head survive its own atomic closeout.
        if let Some(baseline) = baseline
            && !crate::queue_response::free_text_head_present_in_baseline(baseline, text)
            && !((claimed.claims(text) || selected.claims(text)) && has_explicit_answer_evidence)
        {
            continue;
        }
        // `#subagentinlineveto`: an answered head the `subagents` attribute
        // owns, with no worker attempt behind it, was run inline. The answer
        // is real but the lifecycle is not: keep the row queued for dispatch
        // and report it, never close it as if a subagent had run it.
        if dispatch_owned.claims(text) && !claimed.claims(text) {
            decision.inline_vetoed_heads.push(
                agent_doc_document::queue_projection::strip_priority_markers(text)
                    .trim()
                    .to_string(),
            );
            continue;
        }
        struck_texts.insert(normalize_queue_prompt_text(text));
        decision.node_keys.push(node.node_key.clone());
    }
    Ok(decision)
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
    let before_shape = document_queue::queue_tree_shape(entries);
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
            completed.text = struck_work_text(&completed.text).to_string();
            marked.push(remaining_targets.remove(pos));
            result.push(QueueEntry::Completed(completed));
            continue;
        }
        result.push(entry.clone());
    }
    if marked.len() == target_texts.len() {
        debug_assert_eq!(
            document_queue::queue_tree_shape(&result),
            before_shape,
            "targeted queue strike changed prompt parent/sibling placement"
        );
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
                completed.text = struck_work_text(&completed.text).to_string();
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
    let components = element::parse(content)?;
    let selected_occurrence = crate::queue_set::QueueSet::parse(content, &components)?
        .selected()
        .map(|block| block.occurrence);
    if selected_occurrence != Some(0) {
        return Ok(None);
    }
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
    let components = element::parse(content)?;
    let queue_set = crate::queue_set::QueueSet::parse(content, &components)?;
    let selected = queue_set
        .selected()
        .ok_or_else(|| anyhow::anyhow!("queue consume: document has no runnable agent:queue"))?;
    let nodes = if selected.occurrence == 0 {
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue consume: failed to derive queue node keys: {err}")
        })?
    } else {
        Vec::new()
    };
    // `#qconsumenostrike`: never assume a second enumerator segments the queue
    // identically. `item_nodes` now includes canonical multiline prompt
    // surfaces, but this text correspondence remains the fail-closed proof for
    // any future or malformed shape. A count-only match could otherwise target
    // the next enumerable node and strike unrun work.
    let parsed_heads = crate::document_queue::parse(selected.component.content(content))
        .ok()
        .map(|entries| {
            crate::document_queue::prompts(&entries)
                .into_iter()
                .map(|prompt| prompt.text.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if parsed_heads.len() >= count
        && let Some(matched) = queue_prompt_node_keys_for_texts(
            content,
            &parsed_heads[..count],
            &nodes
                .iter()
                .filter(|node| !node.item.struck)
                .map(|node| node.node_key.clone())
                .collect::<Vec<_>>(),
        )?
    {
        return Ok(matched);
    }

    let queue_component = selected.component;
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

    let selected_is_first = element::parse(content)
        .ok()
        .and_then(|components| {
            crate::queue_set::QueueSet::parse(content, &components)
                .ok()
                .and_then(|queues| queues.selected().map(|block| block.occurrence == 0))
        })
        .unwrap_or(false);
    if selected_is_first
        && let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
    {
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
    let consumed = agent_doc_markdown_ast::mutations::consume_nodes_all(
        content, "queue", &borrowed,
    )
    .map_err(|err| anyhow::anyhow!("queue consume: failed to apply node-keyed consume: {err}"))?;
    Ok(strip_in_progress_marker_from_struck_queue_items(&consumed))
}

/// Remove exact queue items by the durable node keys selected from one snapshot.
pub fn remove_queue_nodes_by_key(content: &str, node_keys: &[String]) -> Result<String> {
    let borrowed = node_keys.iter().map(String::as_str).collect::<Vec<_>>();
    agent_doc_markdown_ast::mutations::remove_nodes(content, "queue", &borrowed)
        .map_err(|err| anyhow::anyhow!("queue reap: failed to remove node-keyed items: {err}"))
}

/// Consume the selected queue prompts through the queue parser's exact source
/// spans when a prompt has no Markdown-AST node key.
///
/// This is the safe fallback for narrowly recovered prompt surfaces such as an
/// operator-authored prose report containing a fenced diagnostic and a trailing
/// request. [`document_queue::parse_spans`] is the segmentation authority for
/// those heads, so replacing only the matched ranges preserves every byte of
/// later queue work. Returning `None` means the requested texts could not be
/// proven against this exact document frontier.
pub fn consume_queue_prompts_by_exact_spans(
    content: &str,
    target_texts: &[String],
) -> Result<Option<String>> {
    if target_texts.is_empty() {
        return Ok(Some(content.to_string()));
    }

    let components = element::parse(content)?;
    let queue = crate::queue_set::selected_component(content, &components)?
        .ok_or_else(|| anyhow::anyhow!("queue consume: document has no runnable agent:queue"))?;
    let body = &content[queue.open_end..queue.close_start];
    let mut target_index = 0usize;
    let mut replacements = Vec::with_capacity(target_texts.len());

    for (entry, range) in document_queue::parse_spans(body)? {
        if target_index == target_texts.len() {
            break;
        }
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if !queue_prompt_texts_match_for_consumption(&prompt.text, &target_texts[target_index]) {
            continue;
        }

        let mut completed = prompt;
        completed.text = struck_work_text(&completed.text).to_string();
        replacements.push((
            (queue.open_end + range.start)..(queue.open_end + range.end),
            document_queue::render(&[QueueEntry::Completed(completed)]),
        ));
        target_index += 1;
    }

    if target_index != target_texts.len() {
        return Ok(None);
    }

    let mut rewritten = content.to_string();
    for (range, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(range, &replacement);
    }
    Ok(Some(rewritten))
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
    project_answered_free_text_strike_with_claims(content, response_body, baseline, &[])
}

/// [`project_answered_free_text_strike`] with the claimed-head proof for every
/// head in `claimed_heads` (`#ftstrikeclaimedmention`).
pub fn project_answered_free_text_strike_with_claims(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    project_answered_free_text_strike_with_claims_at(
        content,
        response_body,
        baseline,
        claimed_heads,
        FALLBACK_RESPONSE_TIMESTAMP,
    )
}

pub fn project_answered_free_text_strike_with_claims_at(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    addressed_at: &str,
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    project_answered_free_text_strike_with_authority_at(
        content,
        response_body,
        baseline,
        claimed_heads,
        &[],
        addressed_at,
    )
}

/// [`project_answered_free_text_strike_with_claims`] plus durable preflight
/// selection authority for post-baseline free-text heads.
pub fn project_answered_free_text_strike_with_authority(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    project_answered_free_text_strike_with_authority_at(
        content,
        response_body,
        baseline,
        claimed_heads,
        selected_heads,
        FALLBACK_RESPONSE_TIMESTAMP,
    )
}

pub fn project_answered_free_text_strike_with_authority_at(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
    addressed_at: &str,
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    project_answered_free_text_strike_with_dispatch_at(
        content,
        response_body,
        baseline,
        claimed_heads,
        selected_heads,
        &[],
        addressed_at,
    )
    .map(|(projection, _)| projection)
}

/// [`project_answered_free_text_strike_with_authority`] plus the
/// `#subagentinlineveto` rule (see [`answered_free_text_strike_decision`]).
/// Returns the projection and the answered heads the veto kept queued.
pub fn project_answered_free_text_strike_with_dispatch(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
    dispatch_owned_heads: &[String],
) -> Result<(Option<AnsweredFreeTextStrikeProjection>, Vec<String>)> {
    project_answered_free_text_strike_with_dispatch_at(
        content,
        response_body,
        baseline,
        claimed_heads,
        selected_heads,
        dispatch_owned_heads,
        FALLBACK_RESPONSE_TIMESTAMP,
    )
}

pub fn project_answered_free_text_strike_with_dispatch_at(
    content: &str,
    response_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
    selected_heads: &[String],
    dispatch_owned_heads: &[String],
    addressed_at: &str,
) -> Result<(Option<AnsweredFreeTextStrikeProjection>, Vec<String>)> {
    if response_body.trim().is_empty() {
        return Ok((None, Vec::new()));
    }
    agent_doc_frontmatter::frontmatter::parse(content)?;
    let decision = answered_free_text_strike_decision(
        content,
        response_body,
        baseline,
        claimed_heads,
        selected_heads,
        dispatch_owned_heads,
    )?;
    Ok((
        project_free_text_strike_of_node_keys(
            content,
            Some(response_body),
            decision.node_keys,
            addressed_at,
        )?,
        decision.inline_vetoed_heads,
    ))
}

/// `#qresiduewedge`: the answered-head strike for a response the replay guard
/// refuses because the document already holds an answer under its `### Re:`
/// heading (a committed-heading replay, typically a second answer the Codex
/// Stop hook captured for a head whose first answer was committed without an
/// echo).
///
/// The replay is never written, so its body cannot be the materialization
/// proof [`project_answered_free_text_strike_with_claims`] callers require.
/// Two proofs replace it, and a head strikes only with both:
///
/// - the replay itself answers the head, by the same predicate the finalize
///   strike and the session-check `#qheadresidue` guard use (echo, claimed-head
///   proof, `#deferstrike`, and the `baseline` in-flight-edit gate), and
/// - the document's `agent:exchange` already holds an answer titled with the
///   head's own text (`### Re: <head>`, attribution ignored).
///
/// A head echoed only by the replay, or an answer titled with some other
/// topic, never strikes: the never-written replay is not an answer.
pub fn project_committed_heading_replay_free_text_strike_with_claims(
    content: &str,
    replay_body: &str,
    baseline: Option<&str>,
    claimed_heads: &[String],
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    if replay_body.trim().is_empty() {
        return Ok(None);
    }
    agent_doc_frontmatter::frontmatter::parse(content)?;
    let titled = exchange_answer_topics(content);
    if titled.is_empty() {
        return Ok(None);
    }
    let answered: HashSet<String> = answered_free_text_head_node_keys_with_claims(
        content,
        replay_body,
        baseline,
        claimed_heads,
    )?
    .into_iter()
    .collect();
    if answered.is_empty() {
        return Ok(None);
    }
    let node_keys = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
        .map_err(|err| {
            anyhow::anyhow!("committed-heading replay strike: failed to derive queue nodes: {err}")
        })?
        .into_iter()
        .filter(|node| answered.contains(&node.node_key))
        .filter(|node| {
            head_answer_topic(&node.item.text).is_some_and(|topic| titled.contains(&topic))
        })
        .map(|node| node.node_key)
        .collect();
    // The replay body was never materialized. Its content-derived id therefore
    // cannot be used as the backlink for the already-committed response.
    project_free_text_strike_of_node_keys(content, None, node_keys, "")
}

/// Normalized topics of every `### Re:` answer heading in `agent:exchange`:
/// the text after `### Re:` up to the attribution's last spaced em dash
/// (`### Re: topic — model · time`), whitespace-collapsed and lowercased.
fn exchange_answer_topics(content: &str) -> HashSet<String> {
    let Ok(components) = element::parse(content) else {
        return HashSet::new();
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return HashSet::new();
    };
    exchange
        .content(content)
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("### Re:")?;
            let rest = rest.trim_end().trim_end_matches("(HEAD)");
            let topic = rest.rfind(" — ").map_or(rest, |at| &rest[..at]);
            let topic = normalize_answer_topic(topic);
            (!topic.is_empty()).then_some(topic)
        })
        .collect()
}

/// The answer topic a queue head is titled by: its first line without
/// priority / in-progress markers, normalized like [`exchange_answer_topics`].
fn head_answer_topic(head: &str) -> Option<String> {
    let head = strip_in_progress_marker(&strip_priority_markers(head));
    let first_line = head.lines().map(str::trim).find(|line| !line.is_empty())?;
    let topic = normalize_answer_topic(first_line);
    (!topic.is_empty()).then_some(topic)
}

fn normalize_answer_topic(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Strike `node_keys` (free-text queue heads already proven answered), annotate
/// them, and retire their transient completion cells; when no live head remains
/// and the body is losslessly discardable, clear it and drop the queue
/// activation, as one projection.
/// A `loop` queue's body is never cleared: its struck rows are the
/// scheduler's completion evidence and the loop restore removes them
/// (GH #327).
fn project_free_text_strike_of_node_keys(
    content: &str,
    response_body: Option<&str>,
    node_keys: Vec<String>,
    addressed_at: &str,
) -> Result<Option<AnsweredFreeTextStrikeProjection>> {
    if node_keys.is_empty() {
        return Ok(None);
    }
    let node_key_set = node_keys.iter().collect::<HashSet<_>>();
    let consumed_heads = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")?
        .into_iter()
        .filter(|node| node_key_set.contains(&node.node_key))
        .map(|node| node.item.text)
        .collect::<Vec<_>>();
    let struck = consume_queue_nodes_by_key(content, &node_keys)?;
    if struck == content {
        return Ok(None);
    }
    let annotated = annotate_newly_struck_free_text_heads(content, &struck)?;
    let annotated = match response_body {
        Some(response_body) => annotate_newly_struck_queue_items_with_response_id_at(
            content,
            &annotated,
            response_body,
            addressed_at,
        )?,
        None => annotated,
    };
    // `#queueworkevidence`: the answered-head strike is a terminal closeout
    // boundary just like queue consume. Retire its completion cell in the same
    // derived target so stale evidence cannot attach to a later same-text row.
    let annotated = retire_consumed_completion_feedback(&annotated, &consumed_heads).0;
    let remaining = crate::queue_heads::active_queue_heads(&annotated).len();
    let mut target_content = annotated;
    if remaining == 0 {
        let components = element::parse(&target_content)?;
        if let Some(queue) = components
            .iter()
            .find(|component| component.name == "queue")
        {
            let entries = document_queue::parse(queue.content(&target_content))?;
            if !crate::queue_schedule::queue_retains_struck_heads(&queue.attrs)
                && crate::queue_convergence::queue_body_clear_is_lossless(&entries)
            {
                target_content = queue.replace_content(&target_content, "");
                let components = element::parse(&target_content)?;
                if let Some(queue) = components
                    .iter()
                    .find(|component| component.name == "queue")
                {
                    let raw = &target_content[queue.open_start..queue.open_end];
                    let new_tag = document_queue::strip_auto_from_tag(
                        &document_queue::strip_control_from_tag(raw),
                    );
                    if new_tag != raw {
                        let mut rebuilt = String::with_capacity(target_content.len());
                        rebuilt.push_str(&target_content[..queue.open_start]);
                        rebuilt.push_str(&new_tag);
                        rebuilt.push_str(&target_content[queue.open_end..]);
                        target_content = rebuilt;
                    }
                }
            }
        }
    }
    target_content =
        agent_doc_frontmatter::frontmatter::merge_queue_state(&target_content, remaining > 0)?;
    Ok(Some(AnsweredFreeTextStrikeProjection {
        node_keys,
        target_content,
    }))
}

/// `#closeoutstrandedmsg`: whether `current` is `committed` with exactly the
/// answered queue nodes `node_keys` struck, each in EITHER binary-authored
/// shape — the plain `~~text~~` of the `--done` completion projection or the
/// `#ftstrike`-annotated strike — and every other byte unchanged.
///
/// One closeout can strike the same answered head through two projections:
/// `#subagents do [#preflightdeadline]` landed plain via `--done` while the
/// free-text note beside it landed annotated, both after the commit. The
/// all-annotated [`project_answered_free_text_strike`] target then never
/// matched the live document, so the binary's own strikes read as operator
/// drift. Only the annotation suffix is normalized; which nodes are struck, and
/// everything outside them, must match exactly.
pub fn matches_answered_strike_in_any_projection_shape(
    committed: &str,
    current: &str,
    node_keys: &[String],
) -> Result<bool> {
    if node_keys.is_empty() {
        return Ok(false);
    }
    let plain = consume_queue_nodes_by_key(committed, node_keys)?;
    if plain == committed {
        return Ok(false);
    }
    Ok(strip_struck_free_text_annotations(&plain)? == strip_struck_free_text_annotations(current)?)
}

/// Remove the fixed `#ftstrike` annotation ([`annotate_struck_free_text_line`])
/// from struck `agent:queue` lines, leaving the `~~text~~` strike itself.
fn strip_struck_free_text_annotations(content: &str) -> Result<String> {
    let components = element::parse(content)?;
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    let suffix = format!(
        "~~{}{STRUCK_FREE_TEXT_NOTE}",
        agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
    );
    let queue_body = strip_response_projection_children(queue.content(content));
    let body = queue_body
        .split_inclusive('\n')
        .filter_map(|line| {
            let (core, newline) = line
                .strip_suffix('\n')
                .map_or((line, ""), |core| (core, "\n"));
            if is_response_id_lookup_child(core) {
                return None;
            }
            let response_separator = format!("; {RESPONSE_ID_NOTE_PREFIX}");
            let core = core
                .trim_end()
                .rsplit_once(&response_separator)
                .filter(|(_, response_id)| !response_id.trim().is_empty())
                .map_or_else(|| core.trim_end(), |(head, _)| head.trim_end());
            match core.strip_suffix(suffix.as_str()) {
                Some(head) => Some(format!("{head}~~{newline}")),
                None => Some(line.to_string()),
            }
        })
        .collect::<String>();
    let queue_normalized = queue.replace_content(content, &body);
    Ok(queue_normalized
        .split_inclusive('\n')
        .filter(|line| !is_response_id_lookup_child(line.trim_end_matches(['\n', '\r'])))
        .collect())
}

fn strip_response_projection_children(body: &str) -> String {
    let mut ranges = agent_doc_markdown_ast::overlay::response_details_child_ranges(body);
    ranges.extend(agent_doc_markdown_ast::overlay::response_row_child_ranges(
        body,
    ));
    ranges.sort_unstable();
    let mut without_projection = body.to_string();
    for (start, end) in ranges.into_iter().rev() {
        without_projection.replace_range(start..end, "");
    }
    without_projection
}

fn is_response_id_lookup_child(line: &str) -> bool {
    let marker = line
        .trim()
        .strip_prefix("- ")
        .unwrap_or_else(|| line.trim());
    agent_doc_markdown_ast::exchange_tree::is_response_id_lookup_marker(marker)
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

/// Live (unstruck) `agent:queue` prompts in `content` that name one of
/// `done_ids`, in queue order.
///
/// `#donequeuestrike`: closeout uses this as a fail-closed witness after it
/// projects the queue strike for completed tracked work. A completed id that is
/// still a live queue prompt is re-served by the next preflight and by the
/// go-mode drain, so the strike silently not applying must fail the turn rather
/// than commit a document whose backlog and queue disagree.
pub fn live_queue_prompt_done_ids(content: &str, done_ids: &[String]) -> Result<Vec<String>> {
    if done_ids.is_empty() {
        return Ok(Vec::new());
    }
    let wanted = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let components = element::parse(content)?;
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(Vec::new());
    };
    let mut live = Vec::new();
    for entry in document_queue::parse(queue.content(content))? {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(id) = queue_prompt_done_id(&prompt.text)
            && wanted.contains(&id)
            && !live.contains(&id)
        {
            live.push(id);
        }
    }
    Ok(live)
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
    fn strip_struck_owner_annotations(mut body: String) -> String {
        let mut search_from = 0;
        while let Some((offset, open)) = [OWNER_ANNOTATION_OPEN, LEGACY_OWNER_ANNOTATION_OPEN]
            .into_iter()
            .filter_map(|open| body[search_from..].find(open).map(|offset| (offset, open)))
            .min_by_key(|(offset, _)| *offset)
        {
            let start = search_from + offset;
            let terminal_prefix =
                body[..start].ends_with("~~") || body[..start].ends_with("~~~done\n");
            let Some((_, rest)) = strip_owner_annotation_prefix(&body[start..]) else {
                search_from = start + open.len();
                continue;
            };
            let close = body.len() - rest.len();
            if !terminal_prefix {
                search_from = close;
                continue;
            }
            let end = close + usize::from(body[close..].starts_with(' '));
            body.replace_range(start..end, "");
            search_from = start;
        }
        body
    }

    let Ok(components) = element::parse(content) else {
        return content.to_string();
    };
    let mut edits = Vec::new();
    for queue in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = queue.content(content);
        let mut updated_body = body.to_string();
        for marker in lifecycle_markers() {
            let needle_with_space = format!("~~{marker} ");
            let needle_bare = format!("~~{marker}");
            // `#multilinequeuehead`: a multi-line head struck into a `~~~done`
            // fence keeps its lifecycle marker on the first body line, not after
            // `~~`.
            let fence_with_space = format!("~~~done\n{marker} ");
            let fence_bare = format!("~~~done\n{marker}");
            updated_body = updated_body
                .replace(&fence_with_space, "~~~done\n")
                .replace(&fence_bare, "~~~done\n")
                .replace(&needle_with_space, "~~")
                .replace(&needle_bare, "~~");
        }
        updated_body = strip_struck_owner_annotations(updated_body);
        if updated_body != body {
            edits.push((queue.open_end, queue.close_start, updated_body));
        }
    }
    let mut updated = content.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        updated.replace_range(start..end, &replacement);
    }
    updated
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
                .map(|c| crate::prompt_component_attrs::component_preset(&c.attrs).is_some())
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
    let preset_supplies_directive =
        crate::prompt_component_attrs::component_preset(&queue.attrs).is_some();
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
    // `#multilinequeuehead`: a multi-line item struck as a `~~~done` fence is
    // already explicitly completed by its fence. Appending the note to the
    // closing `~~~` line turned it into an info-string line that no longer
    // closes the fence, so the fence swallowed `<!-- /agent:queue -->` and every
    // projection over the result failed with `unclosed component`. Only a
    // single-line `~~text~~` strike carries the note.
    if core.contains('\n') || trimmed_end.trim_start().starts_with("~~~") {
        return line.to_string();
    }
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
        agent_doc_markdown_ast::mutations::item_nodes_all(before, "queue")
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|n| n.item.struck)
                    .map(|n| n.node_key)
                    .collect()
            })
            .unwrap_or_default();

    let nodes = match agent_doc_markdown_ast::mutations::item_nodes_all(after, "queue") {
        Ok(nodes) => nodes,
        Err(_) => return Ok(after.to_string()),
    };

    let mut edits: Vec<(usize, usize)> = Vec::new();
    for node in &nodes {
        if !node.item.struck {
            continue;
        }
        // `#multilinequeuehead`: a `~~~done` fence is its own completion
        // marker; see [`annotate_struck_free_text_line`].
        if node.item.surface == agent_doc_markdown_ast::overlay::ItemSurface::MultilinePrompt {
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

/// Append this turn's stable response-cell id to every managed-list item newly
/// struck between `before` and `after`.
///
/// Comparing node identity across the lifecycle transition covers every strike
/// path without relabeling an older, identical prompt. The note remains outside
/// the strike wrapper so searching the response id finds both the response and
/// the addressed prompt.
struct QueueAddressedResponseCell {
    body: String,
    heading_count: usize,
}

fn queue_addressed_response_cells(response: &str) -> Vec<QueueAddressedResponseCell> {
    #[derive(Default)]
    struct Cell {
        text: String,
        heading_count: usize,
        has_prompt_echo: bool,
    }

    let mut cells = Vec::new();
    let mut current = Cell::default();
    for line in response.split_inclusive('\n') {
        let trimmed = line.trim();
        let quoted = line.trim_start().strip_prefix('>').map(str::trim_start);
        let starts_prompt_echo = quoted.is_some_and(|quoted| {
            quoted.starts_with("**Queue prompt:**") || quoted.starts_with("**Queue prompts:**")
        });
        let starts_heading = trimmed.starts_with("### ");
        // Support both canonical heading→echo responses and retained payloads
        // that carry echo→heading pairs. Once a cell has both anchors, either
        // the next echo or the next H3 starts a new independently addressable
        // response. Extra H3 sections without another echo remain part of the
        // same intentional multi-section cell.
        if ((starts_prompt_echo && current.heading_count > 0 && current.has_prompt_echo)
            || (starts_heading && current.heading_count > 0 && current.has_prompt_echo))
            && !current.text.trim().is_empty()
        {
            let completed = std::mem::take(&mut current);
            cells.push(QueueAddressedResponseCell {
                body: completed.text,
                heading_count: completed.heading_count,
            });
        }
        current.text.push_str(line);
        current.heading_count += usize::from(starts_heading);
        current.has_prompt_echo |= starts_prompt_echo;
    }
    if !current.text.trim().is_empty() {
        cells.push(QueueAddressedResponseCell {
            body: current.text,
            heading_count: current.heading_count,
        });
    }
    cells
}

/// Attach response lookup children to rows resolved by this closeout and to
/// live rows carrying a blocking operator request presented by this response.
/// The latter must remain unstruck while the operator decides, so request
/// projection is keyed by its durable request id or exact `queue_item` lineage.
pub fn annotate_newly_struck_queue_items_with_response_id(
    before: &str,
    after: &str,
    response_body: &str,
) -> anyhow::Result<String> {
    annotate_newly_struck_queue_items_with_response_id_at(
        before,
        after,
        response_body,
        FALLBACK_RESPONSE_TIMESTAMP,
    )
}

pub fn annotate_newly_struck_queue_items_with_response_id_at(
    before: &str,
    after: &str,
    response_body: &str,
    addressed_at: &str,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        !addressed_at.trim().is_empty(),
        "queue response projection requires a stable addressed-at timestamp"
    );
    #[derive(Clone)]
    struct ResponseCandidate {
        body: String,
        policy: agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy,
    }

    let normalized_after =
        agent_doc_markdown_ast::overlay::normalize_generated_response_lifecycle_markers(after);
    let after = normalized_after.as_str();
    let response_body = response_body_without_exchange_transport(response_body);
    let response_nodes = agent_doc_markdown_ast::exchange_tree::parse_exchange_nodes(response_body)
        .into_iter()
        .filter(|node| {
            matches!(
                node.kind,
                agent_doc_markdown_ast::exchange_tree::ExchangeNodeKind::Response { .. }
            )
        })
        .collect::<Vec<_>>();
    let mut response_node_cursor = 0usize;
    let response_candidates = queue_addressed_response_cells(response_body)
        .into_iter()
        .filter_map(|cell| {
            if cell.heading_count == 0
                || response_node_cursor + cell.heading_count > response_nodes.len()
            {
                return None;
            }
            let nodes =
                &response_nodes[response_node_cursor..response_node_cursor + cell.heading_count];
            response_node_cursor += cell.heading_count;
            let policy =
                agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response_nodes(
                    nodes,
                );
            (!policy.response_id().is_empty()).then_some(ResponseCandidate {
                body: cell.body,
                policy,
            })
        })
        .collect::<Vec<_>>();
    let explicitly_addressed_sections = response_candidates
        .iter()
        .filter(|candidate| {
            candidate.body.lines().any(|line| {
                let quoted = line.trim_start().strip_prefix('>').map(str::trim_start);
                quoted.is_some_and(|quoted| {
                    quoted.starts_with("**Queue prompt:**")
                        || quoted.starts_with("**Queue prompts:**")
                })
            })
        })
        .count();
    let composite_candidate = ResponseCandidate {
        body: response_body.to_string(),
        policy: agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
            response_body,
        )),
    };
    if composite_candidate.policy.response_id().is_empty() {
        return Ok(after.to_string());
    }

    let before_nodes = agent_doc_markdown_ast::mutations::item_nodes_all(before, "queue")
        .unwrap_or_default()
        .into_iter()
        .filter(|node| !node.item.struck)
        .collect::<Vec<_>>();

    let struck_before: HashSet<String> =
        agent_doc_markdown_ast::mutations::item_nodes_all(before, "queue")
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|node| node.item.struck)
                    .map(|node| node.node_key)
                    .collect()
            })
            .unwrap_or_default();
    let nodes = match agent_doc_markdown_ast::mutations::item_nodes_all(after, "queue") {
        Ok(nodes) => nodes,
        Err(_) => return Ok(after.to_string()),
    };

    let newly_struck = nodes
        .iter()
        .filter(|node| {
            node.item.struck
                && !struck_before.contains(&node.node_key)
                && node.item.surface
                    != agent_doc_markdown_ast::overlay::ItemSurface::MultilinePrompt
        })
        .collect::<Vec<_>>();
    // `#operatorrequestresponsesubtree`: a blocking worker handback deliberately
    // keeps its originating row live while the operator chooses. Waiting for a
    // strike therefore makes the request summary (and, critically, its choices)
    // invisible until after it can no longer help. Project the same inert
    // response child as soon as the durable request packet and its queue action
    // coexist. The queue-item fallback covers an answer that clears `🙋` in the
    // same closeout before this backlink pass runs.
    let operator_request_packets =
        crate::operator_request_packet::blocking_operator_request_packets(response_body);
    let operator_request_nodes = nodes
        .iter()
        .filter(|node| {
            if node.item.struck
                || node.item.surface
                    == agent_doc_markdown_ast::overlay::ItemSurface::MultilinePrompt
            {
                return false;
            }
            let action_ids =
                agent_doc_document::queue_projection::operator_action_ids(&node.item.text);
            operator_request_packets.iter().any(|packet| {
                action_ids
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&packet.id))
                    || packet.queue_item.as_deref().is_some_and(|queue_item| {
                        agent_doc_document::queue_projection::operator_request_item_matches_row(
                            &node.item.text,
                            queue_item,
                        )
                    })
            })
        })
        .collect::<Vec<_>>();
    if newly_struck.is_empty() && operator_request_nodes.is_empty() {
        return Ok(after.to_string());
    }

    // A closeout may carry several independently authored `### Re:` sections.
    // Bind each newly completed prompt to the one section that explicitly
    // targets or echoes it. The former composite-cell projection gave every
    // prompt the aggregate turn's id and summary, which made unrelated queue
    // decisions indistinguishable. Preserve intentional one-section batch
    // closeouts (for example several explicit `--done` ids), but a closeout
    // that supplies distinct sections must map those sections one-to-one.
    let mut candidates_by_node_key = Vec::<(String, ResponseCandidate, bool)>::new();
    let mut claimed_sections = HashSet::<usize>::new();
    for node in &newly_struck {
        // Queue rows can move between parents while an editor cut and a
        // retained closeout converge. The response echo then names the row's
        // admitted ancestry while `after` exposes its new ancestry. Correlate
        // the two cuts by the durable q-id when present, otherwise by the
        // normalized work-text identity. If duplicate text existed in the old
        // cut, the candidate set below still has to collapse to exactly one
        // response section; ambiguity remains a refusal.
        let stable_identity =
            agent_doc_element_queue::queue_prompt_stable_identity(node.item.work_text());
        let prior_spellings = before_nodes
            .iter()
            .filter(|prior| {
                agent_doc_element_queue::queue_prompt_stable_identity(prior.item.work_text())
                    == stable_identity
            })
            .map(|prior| prior.item.text.as_str())
            .collect::<Vec<_>>();
        let matching = response_candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                crate::queue_response::queue_head_answered_by_response(
                    after,
                    &candidate.body,
                    &node.item.text,
                ) || crate::queue_response::response_explicitly_targets_queue_head(
                    &candidate.body,
                    &node.item.text,
                ) || prior_spellings.iter().any(|prior| {
                    crate::queue_response::queue_head_answered_by_response(
                        before,
                        &candidate.body,
                        prior,
                    ) || crate::queue_response::response_explicitly_targets_queue_head(
                        &candidate.body,
                        prior,
                    )
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        let candidate = match matching.as_slice() {
            [index] if response_candidates.len() <= 1 || claimed_sections.insert(*index) => {
                response_candidates[*index].clone()
            }
            [] if response_candidates.len() <= 1 || explicitly_addressed_sections <= 1 => {
                composite_candidate.clone()
            }
            [] => anyhow::bail!(
                "newly completed queue prompt has no unique response section: {}",
                node.item.text.trim()
            ),
            [index] => anyhow::bail!(
                "response section {} answers more than one newly completed queue prompt",
                index + 1
            ),
            _ => anyhow::bail!(
                "newly completed queue prompt matches multiple response sections: {}",
                node.item.text.trim()
            ),
        };
        candidates_by_node_key.push((node.node_key.clone(), candidate, true));
    }

    for node in operator_request_nodes {
        let action_ids = agent_doc_document::queue_projection::operator_action_ids(&node.item.text);
        let matching_packet_ids = operator_request_packets
            .iter()
            .filter(|packet| {
                action_ids
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&packet.id))
                    || packet.queue_item.as_deref().is_some_and(|queue_item| {
                        agent_doc_document::queue_projection::operator_request_item_matches_row(
                            &node.item.text,
                            queue_item,
                        )
                    })
            })
            .map(|packet| packet.id.as_str())
            .collect::<Vec<_>>();
        let mut matching = response_candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                matching_packet_ids
                    .iter()
                    .any(|id| candidate.body.contains(&format!("[#{id}]")))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        matching.sort_unstable();
        matching.dedup();
        let candidate = match matching.as_slice() {
            [index] => response_candidates[*index].clone(),
            [] if response_candidates.len() <= 1 => composite_candidate.clone(),
            [] => anyhow::bail!(
                "blocking operator request has no unique response section: {}",
                node.item.text.trim()
            ),
            _ => anyhow::bail!(
                "blocking operator request matches multiple response sections: {}",
                node.item.text.trim()
            ),
        };
        if !candidates_by_node_key
            .iter()
            .any(|(node_key, _, _)| node_key == &node.node_key)
        {
            // Live request rows get the child subtree only. The inline
            // `; response: [#r-…]` completion note belongs to terminal rows.
            candidates_by_node_key.push((node.node_key.clone(), candidate, false));
        }
    }

    let mut after = after.to_string();
    let mut projection_by_node_key = HashMap::<String, (String, String, bool)>::new();
    for (node_key, candidate, annotate_inline_id) in candidates_by_node_key {
        let response_id = response_lookup_id_for_document(&after, &candidate.policy)?;
        if response_id.is_empty() {
            continue;
        }
        after = annotate_exchange_response_with_lookup_id(&after, &candidate.policy, &response_id)?;
        projection_by_node_key.insert(node_key, (candidate.body, response_id, annotate_inline_id));
    }

    let nodes = match agent_doc_markdown_ast::mutations::item_nodes_all(&after, "queue") {
        Ok(nodes) => nodes,
        Err(_) => return Ok(after),
    };
    let queue_response_ranges = element::parse(&after)?
        .into_iter()
        .filter(|component| component.name == "queue")
        .map(|component| {
            (
                component.open_end,
                component.close_start,
                QueueResponseMode::from_attr(component.attrs.get("response").map(String::as_str)),
            )
        })
        .collect::<Vec<_>>();

    let mut edits = Vec::new();
    // A blocking request is one decision even when an aggregate response is
    // projected below several completed queue rows. Render its full choices
    // once, then retain an inert request reference below later parents.
    let mut rendered_choice_packet_ids = HashSet::<String>::new();
    for node in &nodes {
        let Some((response_body, response_id, annotate_inline_id)) =
            projection_by_node_key.get(&node.node_key)
        else {
            continue;
        };
        let raw = &after[node.item.start_byte..node.item.end_byte];
        let indent = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let descendant_end = descendant_region_end(&after, node.item.end_byte, indent);
        let owned_scope_end = descendant_end.max(node.item.end_byte);
        let owned_scope = &after[node.item.start_byte..owned_scope_end];
        let child_indent = " ".repeat(indent + 2);
        let response_mode = queue_response_ranges
            .iter()
            .find(|&&(start, end, _)| node.item.start_byte >= start && node.item.start_byte < end)
            .map_or(QueueResponseMode::Summary, |&(_, _, mode)| mode);
        let lookup_children = response_lookup_children(
            before,
            &node.item.text,
            response_body,
            response_id,
            response_mode,
            addressed_at,
            &mut rendered_choice_packet_ids,
        );
        let missing_children = lookup_children
            .iter()
            .filter(|child| {
                !owned_scope.contains(&render_response_lookup_child(child, &child_indent))
            })
            .cloned()
            .collect::<Vec<_>>();
        let needs_inline_id = *annotate_inline_id
            && !raw.contains(&format!("{RESPONSE_ID_NOTE_PREFIX}{response_id}"));
        if needs_inline_id || !missing_children.is_empty() {
            edits.push((
                node.item.start_byte,
                node.item.end_byte,
                indent,
                needs_inline_id,
                response_id.clone(),
                missing_children,
            ));
        }
    }

    let mut out = after;
    for (start, end, indent, needs_inline_id, response_id, children) in edits.into_iter().rev() {
        let mut replacement = if needs_inline_id {
            annotate_struck_line_with_response_id(&out[start..end], &response_id)
        } else {
            out[start..end].to_string()
        };
        if !children.is_empty() {
            if !replacement.ends_with('\n') {
                replacement.push('\n');
            }
            let child_indent = " ".repeat(indent + 2);
            for child in children {
                replacement.push_str(&render_response_lookup_child(&child, &child_indent));
            }
        }
        out.replace_range(start..end, &replacement);
    }
    Ok(out)
}

fn annotate_exchange_response_with_lookup_id(
    document: &str,
    response_policy: &agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy,
    response_id: &str,
) -> anyhow::Result<String> {
    let components = element::parse(document)?;
    let mut edits = Vec::new();
    for component in components {
        if component.name != "exchange" {
            continue;
        }
        let inner = &document[component.open_end..component.close_start];
        let annotated = agent_doc_markdown_ast::exchange_tree::annotate_response_with_lookup_id(
            inner,
            response_policy,
            response_id,
        );
        if annotated != inner {
            edits.push((component.open_end, component.close_start, annotated));
        }
    }
    let mut out = document.to_string();
    for (start, end, annotated) in edits.into_iter().rev() {
        out.replace_range(start..end, &annotated);
    }
    Ok(out)
}

/// Resolve one compact public response id for the exact response cell in a
/// document. The internal ordered node identity never crosses this boundary.
pub fn response_lookup_id_for_document(
    document: &str,
    response_policy: &agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy,
) -> anyhow::Result<String> {
    for component in element::parse(document)? {
        if component.name != "exchange" {
            continue;
        }
        let inner = &document[component.open_end..component.close_start];
        if agent_doc_markdown_ast::exchange_tree::response_cell_materialized(inner, response_policy)
        {
            return Ok(agent_doc_markdown_ast::exchange_tree::response_lookup_id(
                inner,
                response_policy,
            ));
        }
    }
    Ok(response_policy.response_id().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueResponseMode {
    Summary,
    Inline,
    Lookup,
}

impl QueueResponseMode {
    fn from_attr(value: Option<&str>) -> Self {
        match value {
            Some("inline") => Self::Inline,
            Some("lookup") => Self::Lookup,
            Some("summary") | None | Some(_) => Self::Summary,
        }
    }
}

fn response_lookup_children(
    source_document: &str,
    queue_item: &str,
    response_body: &str,
    response_id: &str,
    mode: QueueResponseMode,
    addressed_at: &str,
    rendered_choice_packet_ids: &mut HashSet<String>,
) -> Vec<String> {
    // `patch:exchange` is the response transport envelope. The exchange
    // materializer consumes it, but this queue projection receives the original
    // captured payload so it must cross the same visibility boundary explicitly.
    // Strip only one complete outer pair; literal examples inside an ordinary
    // response remain operator-visible.
    let response_body = response_body_without_exchange_transport(response_body);
    let mut children = Vec::new();
    match mode {
        QueueResponseMode::Summary => children.push(
            response_summary_row_child(
                source_document,
                queue_item,
                response_body,
                response_id,
                addressed_at,
                rendered_choice_packet_ids,
            )
            .expect("response lookup ids are canonical response ids"),
        ),
        QueueResponseMode::Inline => {
            let marker =
                agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id)
                    .expect("response lookup ids are canonical response ids");
            let details = response_details_child(response_body, response_id)
                .expect("response lookup ids are canonical response ids");
            children.push(format!(
                "{marker}\n- **Timestamp:** {}\n{}",
                addressed_at.trim(),
                render_nested_response_content(&details)
            ));
        }
        QueueResponseMode::Lookup => {
            let marker =
                agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id)
                    .expect("response lookup ids are canonical response ids");
            children.push(format!(
                "{marker}\n- **Timestamp:** {}",
                addressed_at.trim()
            ));
        }
    }
    children
}

fn render_nested_response_content(content: &str) -> String {
    let mut lines = content.lines();
    let mut rendered = format!("- {}", lines.next().unwrap_or_default());
    for line in lines {
        rendered.push_str("\n  ");
        rendered.push_str(line);
    }
    rendered
}

fn response_body_without_exchange_transport(response: &str) -> &str {
    const OPEN: &str = "<!-- patch:exchange -->";
    const CLOSE: &str = "<!-- /patch:exchange -->";

    let trimmed = response.trim();
    let Some(after_open) = trimmed.strip_prefix(OPEN) else {
        return response;
    };
    let Some(after_open) = after_open
        .strip_prefix("\r\n")
        .or_else(|| after_open.strip_prefix('\n'))
    else {
        return response;
    };
    let Some(before_close) = after_open.strip_suffix(CLOSE) else {
        return response;
    };
    let Some(body) = before_close
        .strip_suffix("\r\n")
        .or_else(|| before_close.strip_suffix('\n'))
    else {
        return response;
    };
    body
}

fn response_details_child(response_body: &str, response_id: &str) -> Option<String> {
    let attribute_id = response_id.strip_prefix("[#")?.strip_suffix(']')?;
    let hash = attribute_id.strip_prefix("r-")?;
    if hash.len() < 12
        || !hash
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
    {
        return None;
    }
    let body = balance_nested_details_for_response(response_body.trim_end());
    Some(format!(
        "<details data-agent-doc-response-id=\"{attribute_id}\">\n<summary>Response</summary>\n\n{body}\n</details>"
    ))
}

const RESPONSE_SUMMARY_SOFT_TARGET_CHARS: usize = 280;
const ACTIONABLE_RESPONSE_LEAD_LABELS: &[&str] = &[
    "**outcome:**",
    "**current state:**",
    "**blocked:**",
    "**blocker:**",
    "**next action:**",
];

fn response_summary_row_child(
    source_document: &str,
    queue_item: &str,
    response_body: &str,
    response_id: &str,
    addressed_at: &str,
    rendered_choice_packet_ids: &mut HashSet<String>,
) -> Option<String> {
    if !agent_doc_markdown_ast::exchange_tree::is_compact_response_id(response_id) {
        return None;
    }
    let mut summary = response_summary_lead(response_body);
    let next_actions = response_next_actions(response_body);
    let packets = crate::operator_request_packet::blocking_operator_request_packets(response_body);
    let mut choice_projections = Vec::new();
    for packet in packets {
        if !summary.contains(&packet.request) {
            if !summary.is_empty() {
                summary.push(' ');
            }
            summary.push_str(&packet.request);
        }
        let canonical = rendered_choice_packet_ids.insert(packet.id.clone());
        choice_projections.push((packet.id, packet.choice_rows, canonical));
    }
    let decision_projection =
        structured_choice_projection(source_document, queue_item, response_body);
    let decision_choices = decision_projection
        .as_ref()
        .map(|projection| projection.choices.clone());
    let marker = agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id)?;
    let mut child = format!(
        "{marker}\n- **Timestamp:** {}\n- **Summary:** {summary}",
        addressed_at.trim()
    );
    if let Some(projection) = decision_projection {
        if let Some(context) = projection.context {
            child.push_str("\n- **Context:** ");
            child.push_str(&context);
        }
        if let Some(question) = projection.question {
            child.push_str("\n- **Question:** ");
            child.push_str(&question);
        }
        child.push_str("\n- **Selected:** ");
        child.push_str(&projection.selected);
    }
    if choice_projections.is_empty() {
        if let Some(choices) = decision_choices
            && !choices.is_empty()
        {
            child.push_str("\n- **Choices:**");
            for choice in choices {
                child.push_str("\n  - ");
                child.push_str(&choice);
            }
        }
    } else {
        for (index, (packet_id, packet_choices, canonical)) in
            choice_projections.into_iter().enumerate()
        {
            child.push_str("\n- **Choices:**");
            if canonical {
                child.push_str(&format!("\n  - **Request**: [#{packet_id}]"));
                let choices = if index == 0 {
                    decision_choices.clone().unwrap_or(packet_choices)
                } else {
                    packet_choices
                };
                for choice in choices {
                    child.push_str("\n  - ");
                    child.push_str(&choice);
                }
            } else {
                child.push_str(&format!("\n  - **Reference**: [#{packet_id}]"));
            }
        }
    }
    if !next_actions.is_empty() {
        child.push_str("\n- **Next actions:**");
        for action in next_actions {
            child.push_str("\n  - **Action:** ");
            child.push_str(&action);
        }
    }
    Some(child)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredChoiceProjection {
    context: Option<String>,
    question: Option<String>,
    selected: String,
    choices: Vec<String>,
}

fn structured_choice_projection(
    source_document: &str,
    queue_item: &str,
    response_body: &str,
) -> Option<StructuredChoiceProjection> {
    let decision = crate::operator_request_packet::structured_choice_decisions(response_body)
        .into_iter()
        .next()
        .or_else(|| {
            crate::operator_request_packet::structured_choice_decisions(source_document)
                .into_iter()
                .rev()
                .find(|decision| {
                    decision
                        .request_id
                        .as_ref()
                        .is_some_and(|request_id| queue_item.contains(&format!("[#{request_id}]")))
                })
        })?;
    let packet = decision.request_id.as_deref().and_then(|request_id| {
        crate::operator_request_packet::blocking_operator_request_packets(source_document)
            .into_iter()
            .chain(crate::operator_request_packet::blocking_operator_request_packets(response_body))
            .find(|packet| packet.id == request_id)
    });
    let selected_label = decision
        .selected_label
        .as_deref()
        .map(normalized_choice_label);
    let mut choices = packet
        .as_ref()
        .map(|packet| packet.choice_rows.clone())
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| {
            decision
                .choice_labels
                .iter()
                .map(|label| format!("**{label}**: Offered choice."))
                .collect()
        });
    if let Some(selected_label) = selected_label {
        for choice in &mut choices {
            let label = choice
                .strip_prefix("**")
                .and_then(|choice| choice.split_once("**:"))
                .map(|(label, _)| normalized_choice_label(label));
            if label.as_deref() == Some(selected_label.as_str()) {
                choice.push_str(" — **Selected**");
            }
        }
    }
    Some(StructuredChoiceProjection {
        context: decision
            .context
            .or_else(|| packet.as_ref().and_then(|packet| packet.context.clone())),
        question: packet
            .as_ref()
            .map(|packet| packet.request.clone())
            .or(decision.question),
        selected: decision.selected,
        choices,
    })
}

fn normalized_choice_label(label: &str) -> String {
    label
        .trim()
        .trim_end_matches('.')
        .trim_end_matches("(recommended)")
        .trim_end_matches("(Recommended)")
        .trim()
        .to_ascii_lowercase()
}

fn response_next_actions(response_body: &str) -> Vec<String> {
    let lines = response_body.lines().collect::<Vec<_>>();
    let mut actions = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(action) = trimmed.strip_prefix("**Next action:**") {
            let action = action.split_whitespace().collect::<Vec<_>>().join(" ");
            if !action.is_empty() && !actions.contains(&action) {
                actions.push(action);
            }
            index += 1;
            continue;
        }
        if trimmed == "**Next actions:**" {
            index += 1;
            while let Some(line) = lines.get(index) {
                let trimmed = line.trim();
                let Some(action) = trimmed.strip_prefix("- ") else {
                    break;
                };
                let action = action.split_whitespace().collect::<Vec<_>>().join(" ");
                if !action.is_empty() && !actions.contains(&action) {
                    actions.push(action);
                }
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    actions
}

fn response_summary_lead(response_body: &str) -> String {
    let mut prose = response_body.to_string();
    for packet in crate::operator_request_packet::blocking_operator_request_blocks(response_body) {
        prose = prose.replacen(packet, "", 1);
    }

    let mut paragraphs = Vec::<String>::new();
    let mut paragraph = Vec::<&str>::new();
    let flush = |paragraph: &mut Vec<&str>, paragraphs: &mut Vec<String>| {
        if !paragraph.is_empty() {
            paragraphs.push(
                paragraph
                    .iter()
                    .flat_map(|line| line.split_whitespace())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            paragraph.clear();
        }
    };
    for line in prose.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut paragraphs);
            continue;
        }
        if response_summary_structural_line(trimmed) {
            flush(&mut paragraph, &mut paragraphs);
            continue;
        }
        paragraph.push(trimmed);
    }
    flush(&mut paragraph, &mut paragraphs);

    let (lead, actionable_contract) = paragraphs
        .iter()
        .find(|paragraph| actionable_response_lead(paragraph))
        .map(|paragraph| (paragraph.as_str(), true))
        .or_else(|| {
            paragraphs
                .first()
                .map(|paragraph| (paragraph.as_str(), false))
        })
        .unwrap_or(("Response.", false));
    soft_bound_response_summary(lead, actionable_contract)
}

fn response_summary_structural_line(line: &str) -> bool {
    line.starts_with('#')
        || line.starts_with('>')
        || line.starts_with("<!--")
        || line.starts_with("```")
        || line.starts_with("~~~")
        || line.starts_with("<details")
        || line == "</details>"
        || line.starts_with("%%")
        || line.starts_with("- ")
        || line.starts_with("* ")
        || line.starts_with("+ ")
        || line.split_once(". ").is_some_and(|(number, _)| {
            !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
        })
}

fn actionable_response_lead(paragraph: &str) -> bool {
    let lower = paragraph.to_ascii_lowercase();
    ACTIONABLE_RESPONSE_LEAD_LABELS
        .iter()
        .any(|label| lower.starts_with(label))
}

fn soft_bound_response_summary(lead: &str, actionable_contract: bool) -> String {
    if actionable_contract || lead.chars().count() <= RESPONSE_SUMMARY_SOFT_TARGET_CHARS {
        return lead.to_string();
    }
    let chars = lead.chars().collect::<Vec<_>>();
    let Some(sentence_end) = chars
        .iter()
        .enumerate()
        .skip(RESPONSE_SUMMARY_SOFT_TARGET_CHARS.saturating_sub(1))
        .find_map(|(index, character)| {
            matches!(character, '.' | '?' | '!')
                .then_some(index)
                .filter(|index| chars.get(index + 1).is_none_or(|next| next.is_whitespace()))
        })
    else {
        return lead.to_string();
    };
    if sentence_end + 1 == chars.len() {
        return lead.to_string();
    }
    let mut bounded = chars[..=sentence_end].iter().collect::<String>();
    bounded.push('…');
    bounded
}

fn balance_nested_details_for_response(response_body: &str) -> String {
    #[derive(Clone, Copy)]
    enum Kind {
        Open,
        Close,
    }

    let code_ranges = element::find_code_ranges(response_body);
    let bytes = response_body.as_bytes();
    let mut tokens = Vec::<(usize, Kind, bool)>::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(relative) = bytes[cursor..].iter().position(|byte| *byte == b'<') else {
            break;
        };
        let start = cursor + relative;
        cursor = start + 1;
        if code_ranges
            .iter()
            .any(|&(range_start, range_end)| start >= range_start && start < range_end)
        {
            continue;
        }
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'>') else {
            break;
        };
        let end = start + relative_end + 1;
        let tag = response_body[start..end].to_ascii_lowercase();
        let kind = if tag.starts_with("<details")
            && tag
                .as_bytes()
                .get("<details".len())
                .is_some_and(|byte| *byte == b'>' || byte.is_ascii_whitespace())
        {
            Some(Kind::Open)
        } else if tag.starts_with("</details") && tag["</details".len()..].trim() == ">" {
            Some(Kind::Close)
        } else {
            None
        };
        if let Some(kind) = kind {
            let line_start = response_body[..start].rfind('\n').map_or(0, |at| at + 1);
            let line_end = response_body[end..]
                .find('\n')
                .map_or(response_body.len(), |at| end + at);
            let standalone =
                response_body[line_start..line_end].trim() == &response_body[start..end];
            tokens.push((start, kind, standalone));
        }
        cursor = end;
    }

    let mut stack = Vec::new();
    let mut matched = vec![false; tokens.len()];
    for (index, (_, kind, standalone)) in tokens.iter().enumerate() {
        if !standalone {
            continue;
        }
        match kind {
            Kind::Open => stack.push(index),
            Kind::Close => {
                if let Some(open) = stack.pop() {
                    matched[open] = true;
                    matched[index] = true;
                }
            }
        }
    }

    let mut balanced = response_body.to_string();
    for (index, (start, _, _)) in tokens.into_iter().enumerate().rev() {
        if !matched[index] {
            balanced.replace_range(start..start + 1, "&lt;");
        }
    }
    balanced
}

fn render_response_lookup_child(child: &str, child_indent: &str) -> String {
    let mut lines = child.split('\n');
    let mut rendered = format!("{child_indent}- {}\n", lines.next().unwrap_or_default());
    let continuation_indent = format!("{child_indent}  ");
    for line in lines {
        rendered.push_str(&continuation_indent);
        rendered.push_str(line);
        rendered.push('\n');
    }
    rendered
}

fn descendant_region_end(document: &str, item_end: usize, parent_indent: usize) -> usize {
    let mut cursor = item_end;
    for line in document[item_end..].split_inclusive('\n') {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - trimmed.len();
        if (trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.starts_with("<!-- /agent:"))
            && indent <= parent_indent
        {
            return cursor;
        }
        cursor += line.len();
    }
    document.len()
}

fn annotate_struck_line_with_response_id(line: &str, response_id: &str) -> String {
    let (core, newline) = match line.strip_suffix('\n') {
        Some(rest) => (rest, "\n"),
        None => (line, ""),
    };
    let trimmed_end = core.trim_end();
    let trailing_ws = &core[trimmed_end.len()..];
    if core.contains('\n')
        || trimmed_end.trim_start().starts_with("~~~")
        || !trimmed_end.contains("~~")
        || trimmed_end.contains(&format!("{RESPONSE_ID_NOTE_PREFIX}{response_id}"))
    {
        return line.to_string();
    }
    let separator = agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR;
    if trimmed_end.contains(separator) {
        format!("{trimmed_end}; {RESPONSE_ID_NOTE_PREFIX}{response_id}{trailing_ws}{newline}")
    } else if trimmed_end.ends_with("~~") {
        format!(
            "{trimmed_end}{separator}{RESPONSE_ID_NOTE_PREFIX}{response_id}{trailing_ws}{newline}"
        )
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;
    use agent_doc_document::queue_projection::IN_PROGRESS_MARKER;
    use std::path::Path;

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

    fn inline_response_queue_doc(body: &str) -> String {
        format!("<!-- agent:queue response=\"inline\" -->\n{body}<!-- /agent:queue -->\n")
    }

    fn active_queue_doc(body: &str) -> String {
        format!("---\nqueue: go\n---\n\n<!-- agent:queue go -->\n{body}<!-- /agent:queue -->\n")
    }

    #[test]
    fn newly_struck_managed_list_item_defaults_to_actionable_response_summary() {
        let before = queue_doc("- do [#fresh]\n- ~~do [#old]~~\n");
        let after = queue_doc("- ~~do [#fresh]~~\n- ~~do [#old]~~\n");
        let response = concat!(
            "### Re: fresh — test\n\n",
            "Completed `/work/tasks/result.md`.\n",
            "%% response published locally; coordinator install remains %%\n",
        );
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();

        assert!(
            annotated.contains(&format!(
                "- ~~do [#fresh]~~ — auto-struck: response: {response_id}\n"
            )),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!(
                "  - **Response {response_id}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - **Summary:** Completed `/work/tasks/result.md`.\n"
            )),
            "{annotated}"
        );
        assert!(agent_doc_markdown_ast::exchange_tree::is_compact_response_id(&response_id));
        assert!(!annotated.contains("response-cell:r:"), "{annotated}");
        assert!(!annotated.contains("  - %% /work/tasks/result.md %%\n"));
        assert!(
            !annotated
                .contains("  - %% response published locally; coordinator install remains %%\n")
        );
        assert!(annotated.contains("- ~~do [#old]~~\n"), "{annotated}");
        assert!(!annotated.contains("**Choices:**"), "{annotated}");
        assert!(!annotated.contains("**Next actions:**"), "{annotated}");
        assert_eq!(
            annotate_newly_struck_queue_items_with_response_id(&before, &annotated, response)
                .unwrap(),
            annotated,
            "replaying the projection must not duplicate the response id"
        );
    }

    #[test]
    fn aggregate_closeout_projects_each_queue_item_from_its_own_response_section() {
        let response = concat!(
            "### Re: alpha — test\n\n",
            "> **Queue prompt:** Investigate alpha [#q-alpha]\n\n",
            "Alpha is fixed and verified.\n\n",
            "### Re: beta — test\n\n",
            "> **Queue prompt:** Investigate beta [#q-beta]\n\n",
            "Beta needs the operator to choose a deployment target.\n",
            "**Operator input needed**\n",
            "- Request: Choose the deployment target.\n",
            "- Why: The target changes the publication path.\n",
            "- Choices:\n",
            "  - **staging**: Publish only to staging.\n",
            "  - **production**: Publish to production.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- Investigate alpha [#q-alpha]\n- Investigate beta [#q-beta]\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ~~Investigate alpha [#q-alpha]~~\n- ~~Investigate beta [#q-beta]~~\n",
            response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let sections = agent_doc_markdown_ast::exchange_tree::parse_exchange_nodes(response)
            .into_iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    agent_doc_markdown_ast::exchange_tree::ExchangeNodeKind::Response { .. }
                )
            })
            .collect::<Vec<_>>();
        let ids = sections
            .iter()
            .map(|node| {
                let policy = agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response_nodes(
                    std::slice::from_ref(node),
                );
                response_lookup_id_for_document(&annotated, &policy).unwrap()
            })
            .collect::<Vec<_>>();

        assert_ne!(ids[0], ids[1], "each response section needs its own id");
        assert!(
            annotated.contains(&format!(
                "- ~~Investigate alpha [#q-alpha]~~ — auto-struck: response: {}\n  - **Response {}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - **Summary:** Alpha is fixed and verified.\n",
                ids[0], ids[0],
            )),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!(
                "- ~~Investigate beta [#q-beta]~~ — auto-struck: response: {}\n  - **Response {}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - **Summary:** Beta needs the operator to choose a deployment target.",
                ids[1], ids[1],
            )),
            "{annotated}"
        );
        assert_eq!(
            annotated
                .matches(&format!("**Response {}**", ids[0]))
                .count(),
            2,
            "the exchange lookup and queue parent share one public id"
        );
        assert_eq!(
            annotated
                .matches(&format!("**Response {}**", ids[1]))
                .count(),
            2,
            "the exchange lookup and queue parent share one public id"
        );
        assert!(!annotated.contains(&format!(
            "Investigate alpha [#q-alpha]~~ — auto-struck: response: {}",
            ids[1]
        )));

        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&annotated)).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "generated response and choice descendants must stay inert:\n{entries:#?}\n{annotated}"
        );
    }

    #[test]
    fn reparented_prompt_uses_its_prior_stable_identity_and_never_the_aggregate_response() {
        const PER_ITEM: &str = "The same response was duplicated for several queue items, meaning the same decisions were requested. I don't know which one to answer. Does an answer in one apply to all? The response and prompts should be unique to the queue item it is responding to. Not an aggregate turn response spanning multiple subagents.";
        let todo = "agent-doc should proactively maintain the agent:todo items and keep them in sync. Complete any agent:todo items that are done or moot. [#q-6ec397004779]";
        let decision = "When I Compact Exchange, I lose the context for decisions re: a todo item. [#q-cc23f2613f0b]";
        let legacy = "This item is complete in the installed implementation.";
        let before = doc_with_queue_and_exchange(
            &format!(
                concat!(
                    "- agent:todo items out of sync\n",
                    "  - {todo}\n",
                    "- {decision}\n",
                    "- agent-doc issues in sdk-dev.md\n",
                    "  - {per_item}\n",
                    "- legacy one\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy two\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy three\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy four\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                ),
                todo = todo,
                decision = decision,
                per_item = PER_ITEM,
                legacy = legacy,
            ),
            "",
        );
        let response = format!(
            concat!(
                "> **Queue prompt:** agent:todo items out of sync: {todo}\n\n",
                "### Re: proactive TODO synchronization\n\n",
                "TODO synchronization is installed and verified.\n\n",
                "> **Queue prompt:** {decision}\n\n",
                "### Re: durable decision links\n\n",
                "Decision links remain actionable.\n\n",
                "> **Queue prompt:** agent-doc issues in sdk-dev.md: {per_item}\n\n",
                "### Re: unique per-item response projection\n\n",
                "Per-item response projection is installed and verified.\n",
            ),
            todo = todo,
            decision = decision,
            per_item = PER_ITEM,
        );
        let after = doc_with_queue_and_exchange(
            &format!(
                concat!(
                    "- agent:todo items out of sync\n",
                    "  - ~~{todo}~~\n",
                    "  - ~~{per_item}~~\n",
                    "- {decision}\n",
                    "- legacy one\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy two\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy three\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                    "- legacy four\n",
                    "  - 📥 **Response [#r-3bb63500aa30]:** {legacy}\n",
                ),
                todo = todo,
                decision = decision,
                per_item = PER_ITEM,
                legacy = legacy,
            ),
            &response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let sections = agent_doc_markdown_ast::exchange_tree::parse_exchange_nodes(&response)
            .into_iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    agent_doc_markdown_ast::exchange_tree::ExchangeNodeKind::Response { .. }
                )
            })
            .collect::<Vec<_>>();
        let ids = sections
            .iter()
            .map(|node| {
                response_lookup_id_for_document(
                    &annotated,
                    &agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response_nodes(
                        std::slice::from_ref(node),
                    ),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let aggregate =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                &response,
            ))
            .response_id()
            .to_string();

        assert_ne!(ids[0], ids[2]);
        assert!(
            annotated.contains(&format!("~~{todo}~~ — auto-struck: response: {}", ids[0])),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!(
                "~~{PER_ITEM}~~ — auto-struck: response: {}",
                ids[2]
            )),
            "the old sdk-dev lineage must bind the reparented row to its own section: {annotated}"
        );
        assert!(!annotated.contains(&format!(
            "~~{PER_ITEM}~~ — auto-struck: response: {aggregate}"
        )));
        assert!(annotated.contains(&format!("**Response {}**", ids[2])));
        assert!(annotated.contains(&format!(
            "**Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n      - **Summary:** Per-item response projection is installed and verified."
        )));
        assert!(annotated.contains(&format!("- {decision}\n")));
        assert!(!annotated.contains("📥 **Response [#r-3bb63500aa30]"));
        assert_eq!(
            annotated.matches("**Response [#r-3bb63500aa30]:**").count(),
            4,
            "legacy generated summaries remain visible but lose executable lifecycle state"
        );
    }

    #[test]
    fn one_newly_struck_prompt_never_falls_back_to_a_multi_section_aggregate() {
        let before = queue_doc("- unmatched prompt\n");
        let after = queue_doc("- ~~unmatched prompt~~\n");
        let response = concat!(
            "### Re: alpha\n\n> **Queue prompt:** alpha prompt\n\nAlpha done.\n\n",
            "### Re: beta\n\n> **Queue prompt:** beta prompt\n\nBeta done.\n",
        );
        let error = annotate_newly_struck_queue_items_with_response_id(&before, &after, response)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("has no unique response section: unmatched prompt"),
            "{error:#}"
        );
    }

    #[test]
    fn completed_row_response_child_strips_exchange_transport_wrapper() {
        let head = "do [#subagent-response-exchange-gap]";
        let before = inline_response_queue_doc(&format!("- {head}\n"));
        let after = inline_response_queue_doc(&format!("- ~~{head}~~\n"));
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: subagent response exchange gap — test\n\n",
            "The response body stays readable and lossless.\n",
            "> **Queue prompt:** copied evidence must stay inert.\n",
            "### Re: copied nested heading\n",
            "- [ ] copied task-like row\n",
            "A literal %% pair remains balanced after child escaping.\n",
            "<!-- /patch:exchange -->\n",
        );
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response_body_without_exchange_transport(response),
            ))
            .response_id()
            .to_string();

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();

        assert!(
            annotated
                .contains("- ~~do [#subagent-response-exchange-gap]~~ — auto-struck: response: "),
            "{annotated}"
        );
        let attribute_id = response_id.trim_start_matches("[#").trim_end_matches(']');
        assert!(
            annotated.contains(&format!(
                "  - **Response {response_id}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - <details data-agent-doc-response-id=\"{attribute_id}\">\n      <summary>Response</summary>\n      \n      ### Re: subagent response exchange gap — test\n      \n      The response body stays readable and lossless.\n      > **Queue prompt:** copied evidence must stay inert.\n      ### Re: copied nested heading\n      - [ ] copied task-like row\n      A literal %% pair remains balanced after child escaping.\n      </details>\n"
            )),
            "the exact struck-parent child projection must remain balanced and nested:\n{annotated}"
        );
        assert!(
            !annotated.contains("<!-- patch:exchange -->"),
            "{annotated}"
        );
        assert!(
            !annotated.contains("<!-- /patch:exchange -->"),
            "{annotated}"
        );
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&annotated)).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "copied response scaffolding must never become another prompt:\n{annotated}"
        );
    }

    #[test]
    fn inline_response_mode_is_scoped_to_its_queue_component() {
        let before = concat!(
            "<!-- agent:queue id=terse response=\"lookup\" -->\n",
            "- do [#terse]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=expanded response=\"inline\" -->\n",
            "- do [#expanded]\n",
            "<!-- /agent:queue -->\n",
        );
        let after = before
            .replace("- do [#terse]", "- ~~do [#terse]~~")
            .replace("- do [#expanded]", "- ~~do [#expanded]~~");
        let response = concat!(
            "### Re: terse — test\n\n",
            "> **Queue prompt:** do [#terse]\n\n",
            "Completed the terse row.\n\n",
            "### Re: expanded — test\n\n",
            "> **Queue prompt:** do [#expanded]\n\n",
            "Completed the expanded row.\n",
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(before, &after, response).unwrap();
        let (terse, expanded) = annotated
            .split_once("<!-- agent:queue id=expanded response=\"inline\" -->")
            .unwrap();
        assert!(!terse.contains("data-agent-doc-response-id"), "{annotated}");
        assert!(
            expanded.contains("data-agent-doc-response-id"),
            "{annotated}"
        );
        assert_eq!(annotated.matches("data-agent-doc-response-id").count(), 1);
        assert_eq!(annotated.matches("- **Response [#r-").count(), 2);
    }

    #[test]
    fn explicit_summary_matches_the_default_projection() {
        let response = "### Re: summary — test\n\n**Outcome:** Completed the requested repair.\n";
        let default = annotate_newly_struck_queue_items_with_response_id(
            &queue_doc("- do [#summary]\n"),
            &queue_doc("- ~~do [#summary]~~\n"),
            response,
        )
        .unwrap();
        let explicit = annotate_newly_struck_queue_items_with_response_id(
            "<!-- agent:queue response=\"summary\" -->\n- do [#summary]\n<!-- /agent:queue -->\n",
            "<!-- agent:queue response=\"summary\" -->\n- ~~do [#summary]~~\n<!-- /agent:queue -->\n",
            response,
        )
        .unwrap();
        let default_child = default
            .split_once("  - **Response")
            .map(|(_, child)| child)
            .unwrap();
        let explicit_child = explicit
            .split_once("  - **Response")
            .map(|(_, child)| child)
            .unwrap();
        assert_eq!(default_child, explicit_child);
    }

    #[test]
    fn summary_keeps_actionable_lead_and_all_operator_request_choices_beyond_soft_target() {
        let long_state = format!(
            "**Current state:** {} The release remains blocked until the operator selects a target. Next action: choose one option below.",
            "The implementation and focused verification are complete. ".repeat(8)
        );
        let response = format!(
            "### Re: release target — test\n\n{long_state}\n\n\
[#subagent-operator-request-release-target] subagent operator request\n\
- *queue_item:*\n  - publish release\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n  - Which release target should be used?\n\
- *context:*\n  - The build is ready.\n\
- *choices:*\n  - **staging**: Publish to staging.\n  - **production**: Publish to production.\n"
        );
        let annotated = annotate_newly_struck_queue_items_with_response_id(
            &queue_doc("- do [#release]\n"),
            &queue_doc("- ~~do [#release]~~\n"),
            &response,
        )
        .unwrap();

        assert!(long_state.chars().count() > RESPONSE_SUMMARY_SOFT_TARGET_CHARS);
        let normalized_long_state = long_state.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(annotated.contains(&normalized_long_state), "{annotated}");
        assert!(
            annotated.contains("Which release target should be used?"),
            "{annotated}"
        );
        assert!(
            annotated.contains(concat!(
                "    - **Choices:**\n",
                "      - **Request**: [#subagent-operator-request-release-target]\n",
                "      - **staging**: Publish to staging."
            )),
            "{annotated}"
        );
        assert!(
            annotated.contains("- **production**: Publish to production."),
            "{annotated}"
        );
    }

    #[test]
    fn blocking_operator_request_projects_choice_subtree_before_queue_row_is_struck() {
        let queue_item = "🚧 [#subagent:publisher#1] publish release [#release]";
        let request_id = "subagent-operator-request-release-target";
        let response = format!(
            "### Re: release target — test\n\n\
**Current state:** The build is ready, but publication needs an operator choice.\n\n\
[#{request_id}] subagent operator request\n\
- *queue_item:*\n  - {queue_item}\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n  - Which release target should be used?\n\
- *choices:*\n  - **staging**: Publish to staging.\n  - **production**: Publish to production.\n"
        );
        let before = queue_doc(&format!("- {queue_item}\n"));
        let after =
            doc_with_queue_and_exchange(&format!("- 🙋 [#{request_id}] {queue_item}\n"), &response);

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue_body = queue.content(&annotated);

        assert!(
            queue_body.contains(&format!("- 🙋 [#{request_id}] {queue_item}")),
            "the request row must remain live:\n{queue_body}"
        );
        assert!(
            queue_body.contains("Which release target should be used?"),
            "{queue_body}"
        );
        assert!(
            queue_body.contains(&format!(
                "    - **Choices:**\n      - **Request**: [#{request_id}]\n      - **staging**: Publish to staging."
            )),
            "{queue_body}"
        );
        assert!(
            queue_body.contains("- **production**: Publish to production."),
            "{queue_body}"
        );
        assert!(
            !queue_body.contains("; response: [#r-"),
            "live rows must not receive terminal inline annotations:\n{queue_body}"
        );
        let entries = document_queue::parse(queue_body).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "the operator-gated origin and generated response subtree must stay inert:\n{queue_body}"
        );
        assert_eq!(
            annotate_newly_struck_queue_items_with_response_id(&before, &annotated, &response)
                .unwrap(),
            annotated,
            "replaying the durable request evidence must not duplicate its subtree"
        );
    }

    #[test]
    fn early_operator_answer_does_not_suppress_choice_subtree_projection() {
        let queue_item = "🚧 [#subagent:publisher#1] publish release [#release]";
        let response = format!(
            "### Re: release target — test\n\n\
[#subagent-operator-request-release-target] subagent operator request\n\
- *queue_item:*\n  - {queue_item}\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n  - Which release target should be used?\n\
- *choices:*\n  - **staging**: Publish to staging.\n  - **production**: Publish to production.\n"
        );
        let before = queue_doc(&format!("- {queue_item}\n"));
        // The operator answered quickly enough that todo synchronization already
        // cleared `🙋 [#subagent-operator-request-…]`. The packet's exact
        // queue_item remains durable evidence for the same originating row.
        let after = doc_with_queue_and_exchange(&format!("- {queue_item}\n"), &response);

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue_body = queue.content(&annotated);

        assert!(
            queue_body.contains("Which release target should be used?"),
            "{queue_body}"
        );
        assert!(
            queue_body.contains("- **production**: Publish to production."),
            "{queue_body}"
        );
        assert!(
            !queue_body.contains('🙋'),
            "projection must not reopen an already answered choice:\n{queue_body}"
        );
    }

    #[test]
    fn terminal_answer_projects_selected_choice_with_timestamped_parent() {
        let packet = concat!(
            "[#subagent-operator-request-terminal-design] subagent operator request\n",
            "- *queue_item:* do [#terminal]\n",
            "- *kind:* question\n",
            "- *blocking:* true\n",
            "- *request:* What should happen to the unfinished row?\n",
            "- *context:* Unexpected hand-backs currently lose the worker's decision context.\n",
            "- *choices:*\n",
            "  - **redispatch-same-row (recommended)**: Keep the row and redispatch its claim.\n",
            "  - **repeat-row-per-preset**: Add a second row for the next attempt.\n",
            "  - **hold-only**: Keep the row blocked for manual action.\n",
        );
        let before =
            format!("<!-- agent:queue -->\n- do [#terminal]\n<!-- /agent:queue -->\n\n{packet}");
        let after = before.replace("- do [#terminal]", "- ~~do [#terminal]~~");
        let response = concat!(
            "### Re: terminal choice — test\n\n",
            "**Decision record** for `[#subagent-operator-request-terminal-design]`:\n",
            "- **Context:** The worker returned without a terminal token.\n",
            "- **Question:** what should agent-doc do?\n",
            "- **Choices presented:** redispatch-same-row (Recommended), repeat-row-per-preset, hold-only.\n",
            "- **Selected:** **redispatch-same-row**. Preserve the exact row and claim evidence.\n",
        );

        let annotated = annotate_newly_struck_queue_items_with_response_id_at(
            &before,
            &after,
            response,
            "2026-11-01T12:34:56-04:00",
        )
        .unwrap();
        assert!(
            annotated.contains("**Timestamp:** 2026-11-01T12:34:56-04:00"),
            "{annotated}"
        );
        assert!(
            annotated.contains("**Context:** The worker returned without a terminal token."),
            "{annotated}"
        );
        assert!(
            annotated.contains("**Question:** What should happen to the unfinished row?"),
            "{annotated}"
        );
        assert!(
            annotated.contains(
                "**Selected:** **redispatch-same-row**. Preserve the exact row and claim evidence."
            ),
            "{annotated}"
        );
        assert!(
            annotated.contains(concat!(
                "**redispatch-same-row (recommended)**: Keep the row and redispatch its claim. — **Selected**\n",
                "      - **repeat-row-per-preset**: Add a second row for the next attempt.\n",
                "      - **hold-only**: Keep the row blocked for manual action."
            )),
            "{annotated}"
        );
    }

    #[test]
    fn shared_operator_request_projects_one_choice_tree_and_stable_references() {
        let response = concat!(
            "### Re: aggregate repair — test\n\n",
            "> **Queue prompt:** repair alpha [#q-alpha]\n\n",
            "Alpha is repaired.\n\n",
            "> **Queue prompt:** repair beta [#q-beta]\n\n",
            "Beta is repaired.\n\n",
            "> **Queue prompt:** repair gamma [#q-gamma]\n\n",
            "Gamma is repaired.\n\n",
            "[#subagent-operator-request-shared-release] subagent operator request\n",
            "- *queue_item:*\n  - publish the aggregate repair\n",
            "- *kind:* permission\n",
            "- *blocking:* true\n",
            "- *request:*\n  - Choose whether to publish the aggregate repair.\n",
            "- *choices:*\n",
            "  - **publish**: Publish the validated repair.\n",
            "  - **keep local**: Keep the repair local.\n",
        );
        let before = queue_doc(concat!(
            "- repair alpha [#q-alpha]\n",
            "- repair beta [#q-beta]\n",
            "- repair gamma [#q-gamma]\n",
        ));
        let after = queue_doc(concat!(
            "- ~~repair alpha [#q-alpha]~~\n",
            "- ~~repair beta [#q-beta]~~\n",
            "- ~~repair gamma [#q-gamma]~~\n",
        ));

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();

        assert_eq!(
            annotated
                .matches("**Request**: [#subagent-operator-request-shared-release]")
                .count(),
            1,
            "one request id must own the one canonical choice tree:\n{annotated}"
        );
        assert_eq!(
            annotated
                .matches("**Reference**: [#subagent-operator-request-shared-release]")
                .count(),
            1,
            "the second composite parent must link to the canonical request:\n{annotated}"
        );
        assert_eq!(
            annotated
                .matches("**publish**: Publish the validated repair.")
                .count(),
            1
        );
        assert_eq!(
            annotated
                .matches("**keep local**: Keep the repair local.")
                .count(),
            1
        );

        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&annotated)).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "canonical choice metadata and references must remain inert:\n{entries:#?}\n{annotated}"
        );
        assert_eq!(
            annotate_newly_struck_queue_items_with_response_id(&before, &annotated, response)
                .unwrap(),
            annotated,
            "recovery replay must preserve the same canonical owner and references"
        );
    }

    #[test]
    fn fallback_summary_shortens_only_after_a_complete_sentence_beyond_soft_target() {
        let first_sentence = format!("{}.", "A".repeat(RESPONSE_SUMMARY_SOFT_TARGET_CHARS + 7));
        let response = format!(
            "### Re: historical response\n\n{first_sentence} This later sentence is omitted.\n"
        );

        assert_eq!(
            response_summary_lead(&response),
            format!("{first_sentence}…")
        );
    }

    #[test]
    fn summary_projects_structured_next_actions_only_when_present() {
        let response = concat!(
            "### Re: follow-up — test\n\n",
            "**Current state:** The repair is complete; one operator check remains.\n\n",
            "**Next actions:**\n",
            "- Open the preview.\n",
            "- Confirm the rendered choices.\n",
        );
        let annotated = annotate_newly_struck_queue_items_with_response_id(
            &queue_doc("- do [#follow-up]\n"),
            &queue_doc("- ~~do [#follow-up]~~\n"),
            response,
        )
        .unwrap();

        assert!(
            annotated.contains(concat!(
                "    - **Next actions:**\n",
                "      - **Action:** Open the preview.\n",
                "      - **Action:** Confirm the rendered choices.\n",
            )),
            "{annotated}"
        );
    }

    #[test]
    fn fallback_summary_without_a_safe_sentence_boundary_is_retained_whole() {
        let paragraph = "A".repeat(RESPONSE_SUMMARY_SOFT_TARGET_CHARS + 40);
        let response = format!("### Re: historical response\n\n{paragraph}\n");

        assert_eq!(response_summary_lead(&response), paragraph);
    }

    #[test]
    fn lookup_mode_emits_only_the_response_id_child() {
        let before =
            "<!-- agent:queue response=\"lookup\" -->\n- do [#lookup]\n<!-- /agent:queue -->\n";
        let after =
            "<!-- agent:queue response=\"lookup\" -->\n- ~~do [#lookup]~~\n<!-- /agent:queue -->\n";
        let response = "### Re: lookup — test\n\n**Outcome:** Completed `tasks/result.md`.\n";
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(before, after, response).unwrap();
        assert!(!annotated.contains("data-agent-doc-response-id"));
        assert!(!annotated.contains("%% tasks/result.md %%"));
        assert_eq!(annotated.matches("- **Response [#r-").count(), 1);
    }

    #[test]
    fn response_projection_is_timestamped_nested_and_keeps_operator_descendants_promptable() {
        const ADDRESSED_AT: &str = "2026-11-01T14:43:00.000Z";
        let response = "### Re: timestamped response — test\n\nCompleted the requested repair.\n";
        let before = queue_doc("- do [#timestamped]\n");
        let after = queue_doc("- ~~do [#timestamped]~~\n");
        let annotated = annotate_newly_struck_queue_items_with_response_id_at(
            &before,
            &after,
            response,
            ADDRESSED_AT,
        )
        .unwrap();
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();
        let generated = format!(
            "  - **Response {response_id}**\n    - **Timestamp:** {ADDRESSED_AT}\n    - **Summary:** Completed the requested repair.\n"
        );

        assert!(annotated.contains(&generated), "{annotated}");
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&annotated)).unwrap();
        assert!(document_queue::prompts(&entries).is_empty(), "{entries:#?}");

        let operator_prompt = "    - Choose whether to publish this response.\n";
        let with_operator_descendant = annotated.replacen(
            &format!("    - **Timestamp:** {ADDRESSED_AT}\n"),
            &format!("    - **Timestamp:** {ADDRESSED_AT}\n{operator_prompt}"),
            1,
        );
        let queue = element::parse(&with_operator_descendant)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&with_operator_descendant)).unwrap();
        assert!(
            document_queue::prompts(&entries)
                .iter()
                .any(|prompt| prompt.text == "Choose whether to publish this response."),
            "operator-authored descendants must remain promptable: {entries:#?}\n{with_operator_descendant}"
        );
    }

    #[test]
    fn response_child_preserves_literal_exchange_wrapper_documentation() {
        let response = concat!(
            "### Re: transport documentation — test\n\n",
            "The literals `<!-- patch:exchange -->` and ",
            "`<!-- /patch:exchange -->` are documentation.\n",
        );

        assert_eq!(response_body_without_exchange_transport(response), response);
        let child = response_details_child(
            response_body_without_exchange_transport(response),
            "[#r-0123456789ab]",
        )
        .unwrap();
        assert!(child.contains("`<!-- patch:exchange -->`"), "{child}");
        assert!(child.contains("`<!-- /patch:exchange -->`"), "{child}");
    }

    #[test]
    fn response_details_balances_nested_blocks_and_escapes_unmatched_tags() {
        let response = concat!(
            "### Re: nested details — test\n\n",
            "<details>\n<summary>Nested</summary>\n\nNested body.\n</details>\n\n",
            "An unmatched raw close follows: </details>\n",
            "Inline code stays literal: `</details>`.\n",
            "<details>\n",
        );

        let child = response_details_child(response, "[#r-0123456789ab]").unwrap();
        assert!(
            child.contains("<details>\n<summary>Nested</summary>\n\nNested body.\n</details>"),
            "{child}"
        );
        assert!(
            child.contains("An unmatched raw close follows: &lt;/details>"),
            "{child}"
        );
        assert!(child.contains("`</details>`"), "{child}");
        assert!(child.contains("&lt;details>\n</details>"), "{child}");

        let mixed = response_details_child(
            "Inline <details> then a standalone closer.\n</details>\nStill inert.",
            "[#r-0123456789ab]",
        )
        .unwrap();
        assert!(mixed.contains("Inline &lt;details>"), "{mixed}");
        assert!(mixed.contains("&lt;/details>\nStill inert."), "{mixed}");
    }

    #[test]
    fn nested_queue_lookup_children_follow_the_completed_item_indent() {
        let before = inline_response_queue_doc("- group\n  - do [#nested]\n- later\n");
        let after = inline_response_queue_doc("- group\n  - ~~do [#nested]~~\n- later\n");
        let response = "### Re: nested\n\nCompleted `tasks/nested.md`.\n";
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();

        assert!(
            annotated.contains(&format!(
                "  - ~~do [#nested]~~ — auto-struck: response: {response_id}\n    - **Response {response_id}**\n      - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n      - <details data-agent-doc-response-id=\"{}\">\n        <summary>Response</summary>\n        \n        ### Re: nested\n        \n        Completed `tasks/nested.md`.\n        </details>\n",
                response_id.trim_start_matches("[#").trim_end_matches(']'),
            )),
            "{annotated}"
        );
        assert!(annotated.contains("- later\n"), "{annotated}");
    }

    #[test]
    fn multi_section_cell_uses_one_public_id_for_exchange_and_queue_lookup() {
        let response = concat!(
            "### Re: nested\n\nCompleted the implementation.\n\n",
            "### Verification\n\nAll focused tests pass.\n",
        );
        let before = doc_with_queue_and_exchange("- do [#nested]\n", "");
        let after = doc_with_queue_and_exchange("- ~~do [#nested]~~\n", response);
        let policy = agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(
            Some(response),
        );
        assert!(policy.cell_id().starts_with("response-cell:r:"));

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let response_id = policy.response_id();
        let components = element::parse(&annotated).unwrap();
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&annotated);
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap()
            .content(&annotated);
        let lookup_marker =
            agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id).unwrap();

        assert_eq!(exchange.matches(&lookup_marker).count(), 1, "{exchange}");
        assert!(
            queue.contains(&format!("**Response {response_id}**")),
            "{queue}"
        );
        assert!(
            queue.contains(&format!(
                "**Response {response_id}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n"
            )),
            "{queue}"
        );
        assert!(
            queue.contains(&format!("response: {response_id}")),
            "{queue}"
        );
        assert!(exchange.contains(response_id) && queue.contains(response_id));
        assert!(!annotated.contains(policy.cell_id()), "{annotated}");
    }

    #[test]
    fn answered_free_text_projection_appends_its_response_id() {
        let document = active_queue_doc(
            "- explain the searchable response backlink behavior\n- do [#later]\n",
        );
        let response = concat!(
            "### Re: backlinks — test\n\n",
            "> **Queue prompt:**\n>\n",
            "> explain the searchable response backlink behavior\n\n",
            "The completed prompt now links to this response.\n",
        );
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();

        let projected = project_answered_free_text_strike(&document, response, Some(&document))
            .unwrap()
            .expect("prompt is answered");

        assert!(
            projected
                .target_content
                .contains(&format!("response: {response_id}")),
            "{}",
            projected.target_content
        );
        assert!(
            projected.target_content.contains("- do [#later]\n"),
            "{}",
            projected.target_content
        );
    }

    fn first_live_node_key(content: &str) -> String {
        queue_prompt_node_keys_for_count(content, 1)
            .unwrap()
            .keys
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn approved_head_rejects_the_next_head_after_the_answered_head_is_struck() {
        // #qconsumeapprovedhead (gh#240).
        let decision = active_queue_doc("- #upgrade\n- [#rv1]: go\n");
        let approved = approved_queue_head_for_consume(&decision).unwrap();
        let struck = active_queue_doc(
            "- ~~#upgrade~~ — auto-struck: answered this cycle (#ftstrike)\n- [#rv1]: go\n",
        );
        let next_key = first_live_node_key(&struck);
        assert!(!approved_queue_head_matches(
            &approved,
            "[#rv1]: go",
            Some(&next_key)
        ));
        assert!(!approved_queue_head_matches(&approved, "[#rv1]: go", None));
        let own_key = first_live_node_key(&decision);
        assert!(approved_queue_head_matches(
            &approved,
            "#upgrade",
            Some(&own_key)
        ));
    }

    #[test]
    fn approved_head_identity_separates_duplicate_text_heads() {
        let decision = active_queue_doc("- go\n- go\n");
        let approved = approved_queue_head_for_consume(&decision).unwrap();
        let struck = active_queue_doc("- ~~go~~\n- go\n");
        let second = first_live_node_key(&struck);
        assert!(!approved_queue_head_matches(&approved, "go", Some(&second)));
        assert!(approved_queue_head_matches(
            &approved,
            "go",
            Some(&first_live_node_key(&decision))
        ));
    }

    #[test]
    fn approved_head_ignores_markers_and_component_index_drift() {
        let decision = active_queue_doc(&format!("- :pushpin: {IN_PROGRESS_MARKER} ship it\n"));
        let approved = approved_queue_head_for_consume(&decision).unwrap();
        let key = first_live_node_key(&decision);
        let drifted = key.replacen(":0:", ":7:", 1);
        assert_ne!(
            key, drifted,
            "fixture must actually move the component index"
        );
        assert!(approved_queue_head_matches(
            &approved,
            "ship it",
            Some(&drifted)
        ));
    }

    #[test]
    fn no_approved_head_matches_nothing() {
        let paused =
            "---\nqueue: stop\n---\n\n<!-- agent:queue -->\n- later\n<!-- /agent:queue -->\n";
        let approved = approved_queue_head_for_consume(paused).unwrap();
        assert_eq!(approved, ApprovedQueueHead::NoHead);
        assert!(!approved_queue_head_matches(&approved, "later", None));
    }

    #[test]
    fn operator_action_gate_blocks_closeout_consumption_until_annotation_clears() {
        let free_text = "publish the release after operator confirmation";
        let blocked_free_text = format!(
            "🙋 [#subagent-operator-request-publish-confirmation] 🚧 ⟦subagent:publisher#1⟧ {free_text}"
        );
        let blocked_document = active_queue_doc(&format!("- {blocked_free_text}\n"));
        let response = format!(
            "### Re: publication\n\n> **Queue prompt:** {free_text}\n\nThe release is ready.\n"
        );
        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&blocked_document),
                &blocked_document,
                &response,
                &[],
            )
            .unwrap(),
            "an exact answer echo cannot consume work that still needs operator action"
        );

        let blocked_tracked =
            active_queue_doc("- 🙋 [#subagent-operator-request-deploy-approval] do [#deploy]\n");
        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&blocked_tracked),
                &blocked_tracked,
                "### Re: deploy\n\nDeployment prepared.\n",
                &["deploy".to_string()],
            )
            .unwrap(),
            "even an explicit tracked completion signal waits for the open operator action"
        );

        let cleared_free_text = agent_doc_document::queue_projection::apply_operator_action_ids(
            &blocked_free_text,
            &[],
        );
        let cleared_document = active_queue_doc(&format!("- {cleared_free_text}\n"));
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&cleared_document),
                &cleared_document,
                &response,
                &[],
            )
            .unwrap(),
            "clearing the completed operator action restores ordinary answer-based consumption"
        );
    }

    #[test]
    fn resolved_preset_head_completion_uses_expansion_without_literal_quote() {
        let content = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#actionable-review': Add actionable review items into backlog + queue\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #actionable-review\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: actionable review\n\n",
            "Add actionable review items into backlog + queue. Added three items.\n",
        );

        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                response,
                &[],
            )
            .unwrap(),
            "resolved preset expansion should complete without a literal queue-prompt quote"
        );
    }

    fn upgrade_preset_document() -> &'static str {
        concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc. Are the current issues fixed? Check for new issues.\n",
            "---\n\n",
            "<!-- agent:queue subagents preset=\"#auth\" priority go -->\n",
            "- 🚧 #upgrade\n",
            "<!-- /agent:queue -->\n",
        )
    }

    #[test]
    fn preset_exact_echo_admission_and_consumption_use_the_same_evidence() {
        let content = upgrade_preset_document();
        let response = concat!(
            "### Re: agent-doc upgrade report — gpt-6.1-sol\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "Upgraded agent-doc, checked the current issues, and filed the new defect.\n",
        );

        assert!(
            crate::queue_closeout_guard::selected_free_text_heads_missing_response_evidence(
                Some(content),
                content,
                response,
            )
            .unwrap()
            .is_empty(),
            "pre-write admission accepts the exact preset-head echo"
        );
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                response,
                &[],
            )
            .unwrap(),
            "the same exact echo must authorize closeout consumption"
        );
    }

    /// `#reviewloopterminal`: posting a review is a phase result, not terminal
    /// proof for a goal-like review preset when that review reports actionable
    /// findings. The exact queue echo must remain a deferral in both the live
    /// closeout and the committed-exchange residue pass. A later fresh clean
    /// result for the same unchanged head can close the row.
    #[test]
    fn goal_like_review_preset_waits_for_address_and_a_fresh_clean_review() {
        const HEAD: &str = "https://github.com/example/sample-sdk/pull/190: #deep-review";
        let content = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#deep-review': '/goal Keep one coordinator-owned review, fix, and fresh-review loop live until clean.'\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- https://github.com/example/sample-sdk/pull/190: #deep-review\n",
            "<!-- /agent:queue -->\n",
        );
        let actionable_review = concat!(
            "### Re: independent review cycle 1\n\n",
            "> **Queue prompt:** https://github.com/example/sample-sdk/pull/190: #deep-review\n\n",
            "Posted one review against exact unchanged head `abc123`.\n\n",
            "The review is not clean. It identifies one must-fix P2 in reconnect bookkeeping.\n\n",
            "The review effect is published. Its actionable finding is the next fixer dependency; ",
            "the pull request cannot be review-clean until that fix lands and a fresh exact-head review passes.\n",
        );

        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                actionable_review,
                &[],
            )
            .unwrap(),
            "an actionable review must keep the goal-like preset row live"
        );
        assert!(crate::queue_response::latest_queue_head_echo_is_deferral(
            content,
            actionable_review,
            HEAD,
        ));

        let clean_review = concat!(
            "### Re: independent review cycle 2\n\n",
            "> **Queue prompt:** https://github.com/example/sample-sdk/pull/190: #deep-review\n\n",
            "A fresh independent review covered exact unchanged head `def456`. ",
            "It is review-clean with zero must-fix and zero optional findings, and the final remote-head re-query is unchanged.\n",
        );
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                clean_review,
                &[],
            )
            .unwrap(),
            "fresh review-clean evidence on the unchanged head is terminal"
        );
        let accumulated = format!(
            "{actionable_review}\n{clean_review}\n### Re: another pull request\n\n\
             Its review is not review-clean and one P1 survives.\n"
        );
        assert!(
            !crate::queue_response::latest_queue_head_echo_is_deferral(content, &accumulated, HEAD,),
            "the latest clean echo supersedes the earlier actionable review"
        );
    }

    #[test]
    fn preset_incidental_status_words_do_not_create_a_deferral_veto() {
        let content = upgrade_preset_document();
        let response = concat!(
            "### Re: #upgrade — gpt-6.1-sol\n\n",
            "> **Queue prompt:** #upgrade\n\n",
            "Upgraded agent-doc, checked the current issues, and filed the new defect.\n\n",
            "| Issue | Status |\n",
            "| --- | --- |\n",
            "| 164 | `upgrade` recycles controller |\n",
            "| 172 | `**Deferred:**` leading head consumed |\n",
        );

        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                response,
                &[],
            )
            .unwrap(),
            "an unrelated status table must not defer the explicitly echoed head"
        );
        let projected = project_answered_free_text_strike(content, response, Some(content))
            .unwrap()
            .expect("the answered preset must produce a strike projection");
        assert!(
            projected.target_content.contains(
                "<!-- agent:queue subagents preset=\"#auth\" priority -->\n<!-- /agent:queue -->"
            ),
            "the sole consumed head must leave an empty queue projection:\n{}",
            projected.target_content
        );
    }

    /// `#qresiduewedge`: a committed-heading replay strikes a head only when
    /// the replay answers it AND the exchange already holds an answer titled
    /// with that head. Attribution and case differences do not matter; a head
    /// only the never-written replay answers, or an answer under another
    /// title, never strikes.
    #[test]
    fn committed_heading_replay_strikes_only_heads_the_exchange_answers_by_title() {
        let titled =
            "See https://example.com/pull/48/changes#r1 https://example.com/pull/48/changes#r2";
        let untitled = "Audit the archived Confluence pages for drifted status values";
        let content = format!(
            "---\nqueue_active: true\n---\n\n\
             <!-- agent:exchange patch=append -->\n\
             ### Re: {} — gpt-5 · 2026-10-15\nReviewed. Verdict: request changes.\n\
             <!-- /agent:exchange -->\n\n\
             <!-- agent:queue -->\n- 🚧 {titled}\n- {untitled}\n<!-- /agent:queue -->\n",
            titled.to_uppercase().replace("HTTPS", "https"),
        );
        let replay = format!(
            "<!-- patch:exchange -->\n### Re: {titled} — gpt-5\n\n\
             > **Queue prompt:**\n>\n> {titled}\n\n\
             > **Queue prompt:**\n>\n> {untitled}\n\n\
             Refreshed.\n<!-- /patch:exchange -->\n"
        );

        let projected = project_committed_heading_replay_free_text_strike_with_claims(
            &content,
            &replay,
            Some(&content),
            &[],
        )
        .unwrap()
        .expect("the head the exchange answers by title must strike");
        assert!(
            projected.target_content.contains(&format!("~~{titled}")),
            "{}",
            projected.target_content
        );
        assert!(
            projected
                .target_content
                .contains(&format!("- {untitled}\n")),
            "a head only the replay echoes must stay live:\n{}",
            projected.target_content
        );
        assert_eq!(projected.node_keys.len(), 1);

        // Without the replay's own answer evidence nothing strikes, even
        // though the exchange is titled with the head.
        let unanswering = format!(
            "<!-- patch:exchange -->\n### Re: {titled} — gpt-5\n\nRefreshed.\n<!-- /patch:exchange -->\n"
        );
        assert!(
            project_committed_heading_replay_free_text_strike_with_claims(
                &content,
                &unanswering,
                Some(&content),
                &[],
            )
            .unwrap()
            .is_none()
        );

        // A head the operator typed after the baseline is never struck. (A
        // `🚧` head is exempt from that gate by design, so use a plain one.)
        let plain = content.replace("- 🚧 ", "- ");
        let baseline = plain.replace(&format!("- {titled}\n"), "");
        assert!(
            project_committed_heading_replay_free_text_strike_with_claims(
                &plain,
                &replay,
                Some(&baseline),
                &[],
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn ordinary_free_text_head_still_requires_exact_queue_prompt_evidence() {
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Add actionable review items into backlog + queue\n",
            "<!-- /agent:queue -->\n",
        );
        let unquoted_response =
            "### Re: actionable review\n\nAdded actionable review items into backlog + queue.\n";

        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                unquoted_response,
                &[],
            )
            .unwrap(),
            "ordinary free text must not inherit prompt-preset expansion semantics"
        );

        let quoted_response = concat!(
            "### Re: actionable review\n\n",
            "> **Queue prompt:** Add actionable review items into backlog + queue\n\n",
            "Added the items.\n",
        );
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(content),
                content,
                quoted_response,
                &[],
            )
            .unwrap(),
            "ordinary free text should retain exact quoted-prompt completion"
        );
    }

    /// `#closeoutstrandedmsg`: the observed late shape — the id head struck
    /// plain by `--done`, the free-text note struck with `#ftstrike` — is the
    /// binary's own strike of the answered nodes; anything else is not.
    #[test]
    fn answered_strike_accepts_plain_and_annotated_mix_only() {
        let response = "### Re: x\n\n> **Queue prompt:** #subagents do [#preflightdeadline]\n\n> **Queue prompt:** pick up steering\n\nDone.\n";
        let committed = doc_with_queue_and_exchange(
            "- #subagents do [#preflightdeadline]\n- pick up steering\n- later work",
            response,
        )
        .replacen("queue_active: true", "queue: go", 1);
        let projection = project_answered_free_text_strike(&committed, response, None)
            .unwrap()
            .expect("response answers both heads");
        assert_eq!(projection.node_keys.len(), 2);
        let note = format!(
            "{}{STRUCK_FREE_TEXT_NOTE}",
            agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
        );
        let mixed = committed
            .replace(
                "- #subagents do [#preflightdeadline]\n",
                "- ~~#subagents do [#preflightdeadline]~~\n",
            )
            .replace(
                "- pick up steering\n",
                &format!("- ~~pick up steering~~{note}\n"),
            );
        assert_ne!(mixed, projection.target_content);
        assert!(
            matches_answered_strike_in_any_projection_shape(
                &committed,
                &mixed,
                &projection.node_keys
            )
            .unwrap()
        );
        assert!(
            matches_answered_strike_in_any_projection_shape(
                &committed,
                &projection.target_content,
                &projection.node_keys
            )
            .unwrap()
        );

        let extra_strike = mixed.replace("- later work", "- ~~later work~~");
        assert!(
            !matches_answered_strike_in_any_projection_shape(
                &committed,
                &extra_strike,
                &projection.node_keys
            )
            .unwrap(),
            "a strike of an unanswered head is operator intent"
        );
        let new_prompt = mixed.replace("- later work", "- later work\n- new operator prompt");
        assert!(
            !matches_answered_strike_in_any_projection_shape(
                &committed,
                &new_prompt,
                &projection.node_keys
            )
            .unwrap(),
            "a new queue prompt is operator intent"
        );
        let half = committed.replace(
            "- pick up steering\n",
            &format!("- ~~pick up steering~~{note}\n"),
        );
        assert!(
            !matches_answered_strike_in_any_projection_shape(
                &committed,
                &half,
                &projection.node_keys
            )
            .unwrap(),
            "every answered node must be struck"
        );
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

    /// `#deferstrike` live shape (agent-doc-bugs.md 2026-10-04): the
    /// coordinator claimed `release + publish`, quoted it, and deferred it in
    /// prose. `#ftstrike` struck it as answered anyway.
    const DEFERSTRIKE_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#ghbrew]\n",
        "- release + publish\n",
        "<!-- /agent:queue -->\n",
    );
    const DEFERSTRIKE_DEFERRAL: &str = concat!(
        "### Re: dispatch — opus\n\n",
        "> **Queue prompt:** release + publish\n\n",
        "**Deferred, not done:** this release has not happened yet. It is claimed by me ",
        "(`coordinator:integration`) and waits for the six in-flight branches. ",
        "Keep this head open until then.\n",
    );
    const DEFERSTRIKE_ANSWER: &str = concat!(
        "### Re: release — opus\n\n",
        "> **Queue prompt:** release + publish\n\n",
        "Released v0.35.453: tagged, the GitHub release and PyPI published, issues closed.\n",
    );

    /// `#deferquotedmarker` live shape (agent-doc-bugs.md, 2026-10-28): a
    /// reported row whose text itself contains a queue-marker emoji is quoted
    /// exactly by an answer that explains the strike rule (a `**Deferred:**`
    /// mention in code) and later says other rows are "waiting on" choices.
    /// Neither is a deferral of this head, so the exact echo strikes it.
    #[test]
    fn deferquotedmarker_exact_echo_of_marker_emoji_row_strikes_despite_quoted_rule() {
        let work = "I'm still seeing run items in a ☑️ state in sdk-demo.md + game-sessions.md. What are the rules for agent-doc to strike those run items as done?";
        let row = format!("☑️ [#subagent:reportedstrike#1] {work}");
        let document = active_queue_doc(&format!("- {row}\n- do [#later]\n"));
        let answer = |tail: &str| {
            format!(
                "### Re: rules — opus\n\n> **Queue prompt:** {work}\n\n\
                 - **The rule:** a ☑️ row is struck only by a closeout that quotes it, with no `**Deferred:**` line beside it.\n\
                 - **Fixed:** the strike paths.\n\
                 {tail}"
            )
        };
        let response = answer("- The three 🙋☑️ rows are correctly waiting on your choices.\n");
        assert!(
            !crate::queue_response::response_defers_free_text_head(&response, &row),
            "a quoted marker and a later bullet about other rows are not a deferral"
        );
        let projected = project_answered_free_text_strike_with_claims(
            &document,
            &response,
            Some(&document),
            std::slice::from_ref(&row),
        )
        .unwrap()
        .expect("the exact echo strikes the marker-emoji row");
        assert_eq!(projected.node_keys.len(), 1);

        // Controls: a real deferral still holds the row.
        let structured_later = answer("- **Deferred:** the doc update waits for review.\n");
        assert!(crate::queue_response::response_defers_free_text_head(
            &structured_later,
            &row
        ));
        let natural_lead = format!(
            "> **Queue prompt:** {work}\n\nDispatched to a subagent; it waits for the integration batch.\n"
        );
        assert!(crate::queue_response::response_defers_free_text_head(
            &natural_lead,
            &row
        ));
        let natural_first_bullet = format!(
            "> **Queue prompt:** {work}\n\n- Still waiting on the integration batch.\n- Other notes.\n"
        );
        assert!(crate::queue_response::response_defers_free_text_head(
            &natural_first_bullet,
            &row
        ));
    }

    #[test]
    fn deferstrike_quoted_head_with_concrete_deferral_is_not_struck() {
        let keys = answered_free_text_head_node_keys(
            DEFERSTRIKE_DOC,
            DEFERSTRIKE_DEFERRAL,
            Some(DEFERSTRIKE_DOC),
        )
        .unwrap();
        assert!(
            keys.is_empty(),
            "a quoted, deferred head stays queued: {keys:?}"
        );
        let structured = concat!(
            "> **Queue prompt:** release + publish\n\n",
            "**Deferred:** the integration batch has not landed.\n",
        );
        assert!(
            answered_free_text_head_node_keys(DEFERSTRIKE_DOC, structured, Some(DEFERSTRIKE_DOC))
                .unwrap()
                .is_empty(),
            "the structured `**Deferred:**` lead keeps the head queued"
        );
        let dispatched = concat!(
            "> **Queue prompt:** release + publish\n\n",
            "Dispatched to a subagent in its own worktree.\n",
        );
        assert!(
            answered_free_text_head_node_keys(DEFERSTRIKE_DOC, dispatched, Some(DEFERSTRIKE_DOC))
                .unwrap()
                .is_empty(),
            "a head only dispatched this cycle is not answered"
        );
    }

    /// `#claimstrike`: the strike selection judges the response alone. A
    /// quoted head with no deferral is answered whether or not a worker claim
    /// holds it (the claim is released by finalize); the in-progress marker
    /// does not change that.
    #[test]
    fn deferstrike_quoted_and_answered_head_is_struck() {
        let keys = answered_free_text_head_node_keys(
            DEFERSTRIKE_DOC,
            DEFERSTRIKE_ANSWER,
            Some(DEFERSTRIKE_DOC),
        )
        .unwrap();
        assert_eq!(keys.len(), 1, "the answered head is struck: {keys:?}");
        let projected = project_answered_free_text_strike(
            DEFERSTRIKE_DOC,
            DEFERSTRIKE_ANSWER,
            Some(DEFERSTRIKE_DOC),
        )
        .unwrap()
        .expect("answered head projects a strike");
        assert!(projected.target_content.contains("~~release + publish~~"));
        let marked = DEFERSTRIKE_DOC.replace("- release + publish", "- 🚧 release + publish");
        assert_eq!(
            answered_free_text_head_node_keys(&marked, DEFERSTRIKE_ANSWER, Some(&marked))
                .unwrap()
                .len(),
            1,
            "the in-progress marker does not hide an answered head"
        );
    }

    /// `#nestedechostrike` (live, agent-doc-bugs.md 2026-10-27): a free-text
    /// parent with nested sub-prompts is context for independent leaves. A
    /// full-tree closeout echo answers the executable leaves;
    /// the parent remains context and is never itself consumed.
    const NESTED_ECHO_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:run subagents go -->\n",
        "- ☑️ [#subagent:pmsync#1] Sync PR and Jira state into a document element.\n",
        "  - So this is the first subtask.\n",
        "  - This is the second subtask.\n",
        "- later free-text work stays queued\n",
        "<!-- /agent:run -->\n",
    );
    const NESTED_ECHO_FIRST_LEAF: &str = "So this is the first subtask.";
    const NESTED_ECHO_ANSWER: &str = concat!(
        "### Re: PM sync — opus\n\n",
        "> **Queue prompt:** Sync PR and Jira state into a document element.\n",
        ">   - So this is the first subtask.\n",
        ">   - This is the second subtask.\n\n",
        "Part 1 shipped in 1959c54f63; Part 2 is a design doc.\n",
    );

    #[test]
    fn nestedechostrike_verbatim_nested_echo_strikes_only_leaves() {
        for claimed in [Vec::new(), vec![NESTED_ECHO_FIRST_LEAF.to_string()]] {
            let keys = answered_free_text_head_node_keys_with_claims(
                NESTED_ECHO_DOC,
                NESTED_ECHO_ANSWER,
                Some(NESTED_ECHO_DOC),
                &claimed,
            )
            .unwrap();
            assert_eq!(keys.len(), 2, "claimed={claimed:?}: {keys:?}");
            let projected = project_answered_free_text_strike_with_claims(
                NESTED_ECHO_DOC,
                NESTED_ECHO_ANSWER,
                Some(NESTED_ECHO_DOC),
                &claimed,
            )
            .unwrap()
            .expect("the nested echo projects a strike");
            assert!(
                projected.target_content.contains(
                    "- ☑️ [#subagent:pmsync#1] Sync PR and Jira state into a document element.\n"
                ),
                "{}",
                projected.target_content
            );
            assert!(
                projected
                    .target_content
                    .contains("  - ~~So this is the first subtask.~~ — auto-struck:"),
                "{}",
                projected.target_content
            );
            assert!(
                projected
                    .target_content
                    .contains("- later free-text work stays queued\n"),
                "{}",
                projected.target_content
            );
        }
    }

    /// Each child is independently executable. A child-only echo closes that
    /// leaf, a partial tree closes only the echoed leaf, and a response that
    /// defers the tree keeps every leaf queued.
    #[test]
    fn nestedechostrike_partial_or_deferred_nested_echo_does_not_strike() {
        let claimed = vec![NESTED_ECHO_FIRST_LEAF.to_string()];
        let keys = |response: &str| {
            answered_free_text_head_node_keys_with_claims(
                NESTED_ECHO_DOC,
                response,
                Some(NESTED_ECHO_DOC),
                &claimed,
            )
            .unwrap()
        };
        let child_only = concat!(
            "### Re: subtask — opus\n\n",
            "> **Queue prompt:** So this is the first subtask.\n\n",
            "Done.\n",
        );
        assert_eq!(keys(child_only).len(), 1, "a child echo closes that leaf");
        let missing_child = concat!(
            "### Re: PM sync — opus\n\n",
            "> **Queue prompt:** Sync PR and Jira state into a document element.\n",
            ">   - So this is the first subtask.\n\n",
            "Done.\n",
        );
        assert_eq!(
            keys(missing_child).len(),
            1,
            "a partial tree closes only the echoed leaf"
        );
        let deferred = concat!(
            "### Re: PM sync — opus\n\n",
            "> **Queue prompt:** Sync PR and Jira state into a document element.\n",
            ">   - So this is the first subtask.\n",
            ">   - This is the second subtask.\n\n",
            "**Deferred:** dispatched to a subagent.\n",
        );
        assert!(
            keys(deferred).is_empty(),
            "a deferred nested echo stays queued"
        );
    }

    /// Live shape (agent-doc-bugs.md 2026-10-28): `release + publish` names the
    /// subagent work it must contain. Those `Include #subagent:` children are
    /// executable leaves beneath a context-only release parent. The leaf that
    /// names an open subagent remains queued while an unrelated include leaf
    /// may close.
    const RELEASE_INCLUDE_OPEN_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:run subagents go -->\n",
        "- 🚧 [#subagent:nestedlineage#1] When submitting a nested run item, include the parent lineage.\n",
        "- release + publish\n",
        "  - Include #subagent:nestvector + #subagent:nestedproject\n",
        "  - Include #subagent:nestedlineage\n",
        "- later free-text work stays queued\n",
        "<!-- /agent:run -->\n",
    );
    const RELEASE_INCLUDE_HEAD: &str = concat!(
        "release + publish\n",
        "  - Include #subagent:nestvector + #subagent:nestedproject\n",
        "  - Include #subagent:nestedlineage",
    );
    const RELEASE_INCLUDE_ECHO: &str = concat!(
        "### Re: release — opus\n\n",
        "> **Queue prompt:** release + publish\n",
        ">   - Include #subagent:nestvector + #subagent:nestedproject\n",
        ">   - Include #subagent:nestedlineage\n\n",
        "No release was published.\n",
    );

    #[test]
    fn nestedechostrike_release_include_open_subagent_echo_leaves_the_row_queued() {
        assert!(
            crate::queue_response::release_include_context_is_open(
                RELEASE_INCLUDE_OPEN_DOC,
                RELEASE_INCLUDE_HEAD,
            ),
            "nestedlineage is an open include"
        );
        for claimed in [Vec::new(), vec![RELEASE_INCLUDE_HEAD.to_string()]] {
            let keys = answered_free_text_head_node_keys_with_claims(
                RELEASE_INCLUDE_OPEN_DOC,
                RELEASE_INCLUDE_ECHO,
                Some(RELEASE_INCLUDE_OPEN_DOC),
                &claimed,
            )
            .unwrap();
            assert_eq!(
                keys.len(),
                1,
                "claimed={claimed:?}: only the unrelated include leaf closes: {keys:?}"
            );
            let projected = project_answered_free_text_strike_with_claims(
                RELEASE_INCLUDE_OPEN_DOC,
                RELEASE_INCLUDE_ECHO,
                Some(RELEASE_INCLUDE_OPEN_DOC),
                &claimed,
            )
            .unwrap();
            let projected = projected.expect("the unrelated include leaf closes");
            assert!(
                projected.target_content.contains("- release + publish\n"),
                "the parent remains context: {}",
                projected.target_content
            );
            assert!(
                projected
                    .target_content
                    .contains("  - Include #subagent:nestedlineage\n"),
                "the open include leaf remains queued: {}",
                projected.target_content
            );
        }
        assert!(
            !crate::queue_heads::free_text_queue_head_is_completed_residue(
                RELEASE_INCLUDE_OPEN_DOC,
                RELEASE_INCLUDE_ECHO,
                RELEASE_INCLUDE_HEAD,
            ),
            "an open include is not completed residue"
        );
    }

    /// Once every named include is done, the same verbatim echo strikes both
    /// executable include leaves while leaving their parent as context.
    #[test]
    fn nestedechostrike_release_include_echo_strikes_when_every_include_is_done() {
        let done = RELEASE_INCLUDE_OPEN_DOC.replace(
            "- 🚧 [#subagent:nestedlineage#1] When submitting a nested run item, include the parent lineage.\n",
            "- ~~[#subagent:nestedlineage#1] When submitting a nested run item, include the parent lineage.~~\n",
        );
        assert!(
            !crate::queue_response::release_include_context_is_open(&done, RELEASE_INCLUDE_HEAD),
            "a struck include row is done"
        );
        for claimed in [Vec::new(), vec![RELEASE_INCLUDE_HEAD.to_string()]] {
            let keys = answered_free_text_head_node_keys_with_claims(
                &done,
                RELEASE_INCLUDE_ECHO,
                Some(&done),
                &claimed,
            )
            .unwrap();
            assert_eq!(keys.len(), 2, "claimed={claimed:?}: {keys:?}");
            let projected = project_answered_free_text_strike_with_claims(
                &done,
                RELEASE_INCLUDE_ECHO,
                Some(&done),
                &claimed,
            )
            .unwrap()
            .expect("a done include lets the verbatim echo strike");
            assert!(
                projected.target_content.contains("- release + publish\n"),
                "{}",
                projected.target_content
            );
            assert!(
                projected
                    .target_content
                    .contains("- later free-text work stays queued\n"),
                "{}",
                projected.target_content
            );
        }
    }

    /// `#subagentinlineveto` (live, haiven-dev sdk-demo.md 2026-10-28T03:22Z):
    /// preflight listed `https://.../pull/74/changes` + its nested sub-prompt
    /// under `queue_subagent_dispatch` for `<!-- agent:run priority subagents
    /// go -->`; the coordinator fixed it inline, echoed it as `> **Queue
    /// prompt:**`, and the answered-free-text projection struck it as done, so
    /// the operator's `subagents` attribute was silently ignored. A
    /// dispatch-owned head without a worker attempt is kept queued and named
    /// as vetoed; a claimed one keeps `#claimstrike`.
    const INLINE_VETO_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:run priority subagents go -->\n",
        "- 📥 https://github.com/example/docs/pull/74/changes\n",
        "  - The ADR links are malformed.\n",
        "- 📥 later free-text work stays queued\n",
        "<!-- /agent:run -->\n",
    );
    const INLINE_VETO_HEAD: &str = "The ADR links are malformed.";
    const INLINE_VETO_ANSWER: &str = concat!(
        "### Re: PR 74 links — opus\n\n",
        "> **Queue prompt:** https://github.com/example/docs/pull/74/changes\n",
        ">   - The ADR links are malformed.\n\n",
        "Fixed and pushed: every link is inline now.\n",
    );

    #[test]
    fn subagentinlineveto_unclaimed_dispatch_head_answered_inline_stays_queued() {
        let dispatch = vec![INLINE_VETO_HEAD.to_string()];
        let decision = answered_free_text_strike_decision(
            INLINE_VETO_DOC,
            INLINE_VETO_ANSWER,
            Some(INLINE_VETO_DOC),
            &[],
            &[],
            &dispatch,
        )
        .unwrap();
        assert!(
            decision.node_keys.is_empty(),
            "an inline answer must not close a dispatched leaf: {decision:?}"
        );
        assert_eq!(decision.inline_vetoed_heads, dispatch);
        let (projection, vetoed) = project_answered_free_text_strike_with_dispatch(
            INLINE_VETO_DOC,
            INLINE_VETO_ANSWER,
            Some(INLINE_VETO_DOC),
            &[],
            &[],
            &dispatch,
        )
        .unwrap();
        assert!(projection.is_none(), "{projection:?}");
        assert_eq!(vetoed, dispatch);
        // Without the dispatch ownership fact the same echo strikes (the
        // pre-fix behaviour every other caller keeps).
        assert_eq!(
            answered_free_text_head_node_keys_with_authority(
                INLINE_VETO_DOC,
                INLINE_VETO_ANSWER,
                Some(INLINE_VETO_DOC),
                &[],
                &[],
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn subagentinlineveto_claimed_dispatch_head_still_strikes_on_exact_echo() {
        let dispatch = vec![INLINE_VETO_HEAD.to_string()];
        let (projection, vetoed) = project_answered_free_text_strike_with_dispatch(
            INLINE_VETO_DOC,
            INLINE_VETO_ANSWER,
            Some(INLINE_VETO_DOC),
            &dispatch,
            &[],
            &dispatch,
        )
        .unwrap();
        let projection = projection.expect("a worker-backed head strikes on its echo");
        assert!(vetoed.is_empty(), "{vetoed:?}");
        assert!(
            projection
                .target_content
                .contains("  - ~~The ADR links are malformed.~~ — auto-struck:"),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("later free-text work stays queued"),
            "{}",
            projection.target_content
        );
    }

    /// `#ftstrikeclaimedmention` + `#claimstrike`: a claimed head refuses every
    /// weak proof, yet its exact agent-authored `> **Queue prompt:**` echo
    /// still strikes it, unless the response defers it beside that echo.
    #[test]
    fn claimed_head_strikes_only_on_exact_undeferred_echo() {
        let claimed = vec!["release + publish".to_string()];
        let keys = |response: &str| {
            answered_free_text_head_node_keys_with_claims(
                DEFERSTRIKE_DOC,
                response,
                Some(DEFERSTRIKE_DOC),
                &claimed,
            )
            .unwrap()
        };
        assert!(
            keys(DEFERSTRIKE_DEFERRAL).is_empty(),
            "quoted and deferred: stays queued"
        );
        let mention =
            "### Re: status — opus\n\n`release + publish` is claimed and waits on PR 222.\n";
        assert!(keys(mention).is_empty(), "mentioned only: stays queued");
        assert_eq!(
            keys(DEFERSTRIKE_ANSWER).len(),
            1,
            "an exact echo answers a claimed head (#claimstrike)"
        );
        let projected = project_answered_free_text_strike_with_claims(
            DEFERSTRIKE_DOC,
            DEFERSTRIKE_ANSWER,
            Some(DEFERSTRIKE_DOC),
            &claimed,
        )
        .unwrap()
        .expect("the exact echo projects a strike");
        assert!(projected.target_content.contains("~~release + publish~~"));
    }

    /// `#ftstrikeclaimedmention` live shape (agent-doc-bugs.md 2026-10-09
    /// ~19:27): four claimed `#gh-fix <url>` heads (two coordinator-held, two
    /// with live subagents). A status response that only MENTIONED them and
    /// quoted the preset body ("fix then close") struck all four.
    const CLAIMED_MENTION_DOC: &str = concat!(
        "---\nqueue_active: true\nprompt_presets:\n  '#gh-fix': fix then close\n---\n\n",
        "<!-- agent:queue go -->\n",
        "- #gh-fix https://github.com/btakita/agent-doc/issues/218\n",
        "- #gh-fix https://github.com/btakita/agent-doc/issues/221\n",
        "- #gh-fix https://github.com/btakita/agent-doc/issues/227\n",
        "- #gh-fix https://github.com/btakita/agent-doc/issues/228\n",
        "- release + publish\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn ftstrikeclaimedmention_status_mention_never_strikes_claimed_heads() {
        let claimed: Vec<String> = [218, 221, 227, 228]
            .iter()
            .map(|n| format!("#gh-fix https://github.com/btakita/agent-doc/issues/{n}"))
            .collect();
        // Mentions with URLs, a distinctive heading and the preset body: none
        // of them is an exact echo, so no claimed head strikes.
        let status_with_urls = concat!(
            "### Re: GH #227 and #228 dispatched — opus-5.5\n\n",
            "Both new `#gh-fix` heads (\"fix then close\") are claimed and with their own ",
            "subagents.\n\n",
            "- **GH #227:** https://github.com/btakita/agent-doc/issues/227\n",
            "- **GH #228:** https://github.com/btakita/agent-doc/issues/228\n\n",
            "> #gh-fix https://github.com/btakita/agent-doc/issues/218 waits on PR 222\n\n",
            "GH #218, GH #221 and `release + publish` are unchanged.\n",
        );
        assert!(
            answered_free_text_head_node_keys_with_claims(
                CLAIMED_MENTION_DOC,
                status_with_urls,
                Some(CLAIMED_MENTION_DOC),
                &claimed,
            )
            .unwrap()
            .is_empty(),
            "a status mention strikes no claimed head"
        );
        // Even unclaimed, the live status (issue numbers plus the shared preset
        // body, no URLs) answers no `#gh-fix` head.
        let live_status = concat!(
            "### Re: GH #227 and #228 dispatched — opus-5.5\n\n",
            "Both new `#gh-fix` heads (\"fix then close\") are claimed and with their own ",
            "subagents.\n\nGH #218, GH #221 and `release + publish` are unchanged.\n",
        );
        assert!(
            answered_free_text_head_node_keys(
                CLAIMED_MENTION_DOC,
                live_status,
                Some(CLAIMED_MENTION_DOC),
            )
            .unwrap()
            .is_empty(),
            "a preset-body mention strikes no unclaimed `#gh-fix` head either"
        );
        // An exact agent-authored echo of ONE claimed head strikes only it.
        let closing = concat!(
            "### Re: GH 221 — opus-5.5\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/221\n\n",
            "PR 222 merged; GH 221 closed as fixed.\n",
        );
        let projected = project_answered_free_text_strike_with_claims(
            CLAIMED_MENTION_DOC,
            closing,
            Some(CLAIMED_MENTION_DOC),
            &claimed,
        )
        .unwrap()
        .expect("the exact echo strikes its claimed head");
        assert_eq!(projected.node_keys.len(), 1);
        let content = &projected.target_content;
        assert!(content.contains("~~#gh-fix https://github.com/btakita/agent-doc/issues/221~~"));
        for n in [218, 227, 228] {
            assert!(
                content.contains(&format!(
                    "- #gh-fix https://github.com/btakita/agent-doc/issues/{n}\n"
                )),
                "claimed head {n} stays live"
            );
        }
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

        // The pin matches for identity, but a strike wraps only the work text.
        assert_eq!(
            document_queue::render(&updated),
            "- ~~deploy~~\n- do [#tail]\n"
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
    fn consume_queue_nodes_by_key_strips_every_lifecycle_marker_before_strike_text() {
        for marker in agent_doc_document::queue_projection::LIFECYCLE_MARKERS {
            let content = format!(
                "<!-- agent:queue -->\n- {marker} do [#head]\n- do [#tail]\n<!-- /agent:queue -->\n"
            );
            let mut keys = queue_prompt_node_keys_for_count(&content, 1).unwrap().keys;
            let key = keys.remove(0);

            let updated = consume_queue_nodes_by_key(&content, &[key]).unwrap();

            assert!(
                updated.contains("- ~~do [#head]~~\n"),
                "{marker}: {updated}"
            );
            assert!(
                !updated.contains(&format!("~~{marker}")),
                "{marker}: {updated}"
            );
            assert!(updated.contains("- do [#tail]\n"), "{marker}: {updated}");
        }
    }

    /// The bare structural `consume` op is the fallback the orphan
    /// `queue consume --id` strike takes when `node_replace_ops_from_diff`
    /// finds no op. It must strike only the work text: no lifecycle marker,
    /// owner annotation, or pin inside the strike, on the first strike and on a
    /// repeat.
    #[test]
    fn bare_structural_consume_strikes_only_the_work_text() {
        let mut prefixes = vec![String::new(), "📌".to_string()];
        for marker in agent_doc_document::queue_projection::LIFECYCLE_MARKERS {
            prefixes.push(marker.to_string());
            for annotation in [
                "[#subagent:owner#1]",
                "⟦subagent:owner#1⟧",
                "⟦owner=subagent:owner;generation=1⟧",
            ] {
                prefixes.push(format!("{marker} {annotation}"));
            }
        }
        for prefix in prefixes {
            let row = format!("{prefix} do [#orphan]");
            let content = format!(
                "<!-- agent:queue -->\n- {}\n- do [#tail]\n<!-- /agent:queue -->\n",
                row.trim()
            );
            let keys = agent_doc_markdown_ast::mutations::item_nodes(&content, "queue")
                .unwrap()
                .into_iter()
                .filter(|node| node.item.raw.contains("#orphan"))
                .map(|node| node.node_key)
                .collect::<Vec<_>>();
            assert_eq!(keys.len(), 1, "{prefix}");
            let struck =
                apply_structural_ops_to_content(&content, &queue_consume_node_ops(&keys)).unwrap();
            assert_eq!(
                struck,
                "<!-- agent:queue -->\n- ~~do [#orphan]~~\n- do [#tail]\n<!-- /agent:queue -->\n",
                "{prefix:?}"
            );
            let struck_keys = agent_doc_markdown_ast::mutations::item_nodes(&struck, "queue")
                .unwrap()
                .into_iter()
                .filter(|node| node.item.struck)
                .map(|node| node.node_key)
                .collect::<Vec<_>>();
            assert_eq!(struck_keys.len(), 1, "{prefix:?}: {struck}");
            assert_eq!(
                apply_structural_ops_to_content(&struck, &queue_consume_node_ops(&struck_keys))
                    .unwrap(),
                struck,
                "{prefix:?}: a repeat strike is a no-op"
            );
            let body = struck
                .strip_prefix("<!-- agent:queue -->\n")
                .and_then(|rest| rest.strip_suffix("<!-- /agent:queue -->\n"))
                .unwrap();
            match &document_queue::parse(body).unwrap()[0] {
                QueueEntry::Completed(prompt) => assert_eq!(prompt.text, "do [#orphan]"),
                other => panic!("{prefix:?}: expected Completed, got {other:?}"),
            }
        }
    }

    #[test]
    fn consume_queue_nodes_by_key_strips_terminal_attempt_owner_annotation() {
        for annotation in [
            "[#subagent:sdk#7]",
            "⟦subagent:sdk#7⟧",
            "⟦owner=subagent:sdk;generation=7⟧",
        ] {
            let content = format!(
                "<!-- agent:queue -->\n- ☑️ {annotation} finish sdk demo\n- do [#tail]\n<!-- /agent:queue -->\n"
            );
            let key = queue_prompt_node_keys_for_count(&content, 1)
                .unwrap()
                .keys
                .remove(0);

            let updated = consume_queue_nodes_by_key(&content, &[key]).unwrap();

            assert!(updated.contains("- ~~finish sdk demo~~\n"), "{updated}");
            assert!(!updated.contains(annotation), "{updated}");
        }
    }

    /// GH #374: `respond` derives queue node keys through the markdown-AST
    /// overlay after `element::parse` + `QueueSet` already accepted the runnable
    /// component. With `## Run <!-- agent:run ... -->` and
    /// `## Loop <!-- agent:loop ... -->` heading-anchored openers the overlay
    /// used to miss both components, so consumption failed with
    /// "failed to derive queue node keys: component `queue` was not found".
    #[test]
    fn heading_anchored_run_and_loop_openers_consume_by_ast_node_key() {
        let content = concat!(
            "---\nqueue: go\n---\n\n",
            "## Run <!-- agent:run id=main priority subagents go -->\n",
            "- answer the operator\n",
            "- do [#tail]\n",
            "<!-- /agent:run -->\n\n",
            "## Loop <!-- agent:loop id=poll interval=5m -->\n",
            "- poll ci\n",
            "<!-- /agent:loop -->\n",
        );

        let selected = queue_prompt_node_keys_for_count(content, 1)
            .expect("heading-anchored run component resolves for consumption");
        assert!(selected.ast_backed, "{selected:?}");
        let consumed = consume_queue_nodes_by_key(content, &selected.keys).unwrap();

        assert!(
            consumed.contains(concat!(
                "## Run <!-- agent:run id=main priority subagents go -->\n",
                "- ~~answer the operator~~\n",
                "- do [#tail]\n",
            )),
            "{consumed}"
        );
        assert!(consumed.contains("- poll ci\n"), "{consumed}");
    }

    #[test]
    fn remove_queue_nodes_by_key_reaps_only_the_selected_item() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- do [#head]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n",
        );
        let key = queue_prompt_node_keys_for_count(content, 1)
            .unwrap()
            .keys
            .remove(0);
        let struck = consume_queue_nodes_by_key(content, std::slice::from_ref(&key)).unwrap();

        let reaped = remove_queue_nodes_by_key(&struck, &[key]).unwrap();

        assert!(!reaped.contains("[#head]"), "{reaped}");
        assert!(reaped.contains("- do [#tail]\n"), "{reaped}");
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

    /// `#queuefreetextdup`: an identical free-text line queued twice is an
    /// intentional repeat in a scheduling FIFO; answering it once strikes only
    /// the first copy.
    #[test]
    fn answered_free_text_strike_leaves_a_later_identical_repeat_live() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-repeat\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- release + publish\n",
            "- do [#between]\n",
            "- release + publish\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: release — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> release + publish\n\n",
            "Released and published.\n",
        );

        let projected = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("the first copy is answered");

        assert_eq!(projected.node_keys.len(), 1, "{:?}", projected.node_keys);
        let target = &projected.target_content;
        let first = target
            .find("~~release + publish~~")
            .expect("first copy struck");
        let repeat = target
            .rfind("- release + publish\n")
            .expect("the later repeat stays live");
        assert!(
            first < repeat,
            "the FIRST copy is the struck one:\n{target}"
        );
        assert!(target.contains("- do [#between]"));
    }

    #[test]
    fn answered_free_text_strike_does_not_consume_later_heads_named_by_closeout_prose() {
        let document = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- Run the selected queue synchronization repair\n",
            "- Review https://example.com/pull/48/changes#r1\n",
            "- Audit https://example.com/issues/99 for the follow-up\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: queue synchronization repair\n\n",
            "> **Queue prompt:** Run the selected queue synchronization repair\n\n",
            "Completed the selected repair.\n\n",
            "### Re: Review https://example.com/pull/48/changes#r1\n\n",
            "The later review remains queued.\n\n",
            "### Re: Audit https://example.com/issues/99 for the follow-up\n\n",
            "The later audit remains queued.\n",
        );

        let projection = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("the explicitly echoed selected head must strike");

        assert_eq!(projection.node_keys.len(), 1, "{:?}", projection.node_keys);
        assert!(
            projection
                .target_content
                .contains("- ~~Run the selected queue synchronization repair~~"),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("- Review https://example.com/pull/48/changes#r1\n"),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("- Audit https://example.com/issues/99 for the follow-up\n"),
            "{}",
            projection.target_content
        );
    }

    /// `#ftstrike-multiqueue`: the queue graph may select a later queue when an
    /// earlier queue is empty. Answer projection must address the selected
    /// head's component occurrence, not assume the first `agent:queue` block.
    #[test]
    fn answered_free_text_strike_reaches_later_queue_component() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-graph-projection\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue id=main -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues -->\n",
            "- 🚧 If an issue was added, also add `- release + publish`\n",
            "- follow up after publishing\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "> **Queue prompt:** If an issue was added, also add `- release + publish`\n\n",
            "Added the release and publish follow-up.\n",
        );

        let projection = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("the answered head in the later queue is projected");

        assert_eq!(projection.node_keys.len(), 1);
        assert!(
            projection
                .target_content
                .contains("- ~~If an issue was added, also add `- release + publish`~~"),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("- follow up after publishing\n")
        );
    }

    /// `#ftstrikeonce` (live, agent-doc-bugs.md 2026-09-30): queue consume
    /// already struck the answered copy this cycle; the answered-head
    /// projection must not then strike the operator's NEXT repeat.
    #[test]
    fn answered_free_text_strike_does_not_reapply_an_answer_consume_already_struck() {
        let head = concat!(
            "---\n",
            "agent_doc_session: queue-repeat\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
        );
        let baseline = format!(
            "{head}- release + publish\n- #gh-fix https://x/issues/71\n- release + publish\n<!-- /agent:queue -->\n"
        );
        let consumed = format!(
            "{head}- ~~release + publish~~\n- #gh-fix https://x/issues/71\n- release + publish\n<!-- /agent:queue -->\n"
        );
        let response = concat!(
            "### Re: release — gpt-5\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "Released and published.\n",
        );

        let projected =
            project_answered_free_text_strike(&consumed, response, Some(&baseline)).unwrap();
        assert!(
            projected.as_ref().is_none_or(|p| p.node_keys.is_empty()),
            "consume already applied this answer: {:?}",
            projected.map(|p| p.target_content)
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
    fn operator_action_gate_blocks_reactive_free_text_strike_until_annotation_clears() {
        let head = "publish the release after operator confirmation";
        let blocked_head = format!(
            "🙋 [#subagent-operator-request-publish-confirmation] 🚧 ⟦subagent:publisher#1⟧ {head}"
        );
        let document = active_queue_doc(&format!("- {blocked_head}\n- do [#later]\n"));
        let response =
            format!("### Re: publication\n\n> **Queue prompt:** {head}\n\nThe release is ready.\n");

        assert!(
            project_answered_free_text_strike(&document, &response, Some(&document))
                .unwrap()
                .is_none(),
            "the reactive projection must keep a row live while operator action is open"
        );

        let cleared_head =
            agent_doc_document::queue_projection::apply_operator_action_ids(&blocked_head, &[]);
        let cleared_document = active_queue_doc(&format!("- {cleared_head}\n- do [#later]\n"));
        let projected = project_answered_free_text_strike(
            &cleared_document,
            &response,
            Some(&cleared_document),
        )
        .unwrap()
        .expect("todo completion clears the gate and re-enables the normal strike projection");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(projected.target_content.contains(STRUCK_FREE_TEXT_NOTE));
    }

    /// `#answeredrequeststrike` live shape (haiven-dev sdk-demo.md,
    /// 2026-10-28): a reported free-text row whose operator request was
    /// answered locally (`[#request-id]: <answer>`) and whose todo closed shows
    /// no `🙋`, so its exact closeout echo strikes it. An open linked todo still
    /// holds it, on every completion path.
    #[test]
    fn answeredrequeststrike_answered_request_with_closed_todo_strikes_on_echo() {
        let id = "subagent-operator-request-slack-app-bootstrap";
        let work = format!(
            "Setup the PR status app for the sdk-reviews channel. [#{id}]: I ran this before in Codex."
        );
        let row = format!("☑️ [#subagent:slack-pr-status#1] {work}");
        let document = active_queue_doc(&format!("- {row}\n- do [#later]\n"));
        let response = format!("### Re: slack\n\n> **Queue prompt:** {work}\n\nDone.\n");

        let projected = project_answered_free_text_strike_with_claims(
            &document,
            &response,
            Some(&document),
            std::slice::from_ref(&row),
        )
        .unwrap()
        .expect("an answered request with a closed todo no longer vetoes the strike");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(
            crate::queue_heads::free_text_queue_head_is_completed_residue_with_claim(
                &document, &response, &row, true,
            ),
            "the session-check residue set agrees with the strike set"
        );

        let held = format!(
            "{document}\n<!-- agent:todo -->\n- [ ] [#{id}] Confirm Slack access.\n<!-- /agent:todo -->\n"
        );
        assert!(
            project_answered_free_text_strike_with_claims(
                &held,
                &response,
                Some(&held),
                std::slice::from_ref(&row),
            )
            .unwrap()
            .is_none(),
            "an open linked todo still holds the row until the closeout checks it off"
        );
        assert!(
            !crate::queue_heads::free_text_queue_head_is_completed_residue_with_claim(
                &held, &response, &row, true,
            )
        );
        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&held),
                &held,
                &response,
                &[],
            )
            .unwrap()
        );
    }

    #[test]
    fn answered_free_text_strike_retires_only_its_completion_evidence() {
        let item = "#upgrade";
        let other_item = "later worker result";
        let completion =
            agent_doc_document::subagent_completion::cell("subagent:upgrade", item, "upgrade-key");
        let other_completion = agent_doc_document::subagent_completion::cell(
            "subagent:later",
            other_item,
            "later-key",
        );
        let document = format!(
            concat!(
                "---\nagent_doc_session: queue-completion-retirement\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "{completion}\n{other_completion}\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue -->\n",
                "- {item}\n",
                "- {other_item}\n",
                "<!-- /agent:queue -->\n",
            ),
            completion = completion,
            other_completion = other_completion,
            item = item,
            other_item = other_item,
        );
        let response = format!(
            "### Re: upgrade — gpt-5\n\n> **Queue prompt:** {item}\n\nIntegrated and verified.\n"
        );

        let projected = project_answered_free_text_strike(&document, &response, Some(&document))
            .unwrap()
            .expect("answered worker head should project a target");
        let evidence =
            agent_doc_document::subagent_completion::completion_evidence(&projected.target_content);

        assert!(
            projected.target_content.contains(&format!(
                "- ~~{item}~~ — auto-struck: answered this cycle (#ftstrike)"
            )),
            "the integrated row must close"
        );
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].item, other_item);
        assert_eq!(evidence[0].key, "later-key");
    }

    fn strike_retirement_doc(exchange: &str, queue: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: strike-retirement\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n",
            ),
            exchange = exchange,
            queue = queue,
        )
    }

    /// `#strikeretires` (live 2026-10-26: a 52h-old `release_publish_patch`
    /// cell painted `🔄 release + publish` onto a new release request). A
    /// strike successor retires the struck row's completion cell, so a later
    /// row with the same text carries no completion evidence. Unrelated cells
    /// and heading-shaped prose stay byte for byte.
    #[test]
    fn strike_successor_retires_the_struck_rows_completion_cell() {
        let struck = "release + publish";
        let other = "later worker result";
        let cell = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:release_publish_patch",
            struck,
            "release-key",
            "Released 0.35.500.",
        )
        .unwrap();
        let other_cell = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:later",
            other,
            "later-key",
            "Still integrating.",
        )
        .unwrap();
        let prose = format!(
            "### Re: Subagent completed: `subagent:x` returned work for queue item “{struck}” — agent-doc\nOperator note, not a binary cell.\n"
        );
        let exchange = format!("{cell}\n\n{other_cell}\n\n{prose}");
        let before = strike_retirement_doc(
            &exchange,
            &format!("- ☑️ [#subagent:release_publish_patch#3] {struck}\n- {other}\n"),
        );
        let after = strike_retirement_doc(&exchange, &format!("- ~~{struck}~~\n- {other}\n"));

        let retirement = retire_struck_rows_completion_feedback(&before, &after);

        assert_eq!(retirement.struck_heads, vec![struck.to_string()]);
        assert_eq!(
            retirement.retired_keys,
            HashSet::from(["release-key".to_string()])
        );
        assert!(
            !retirement.content.contains(&cell),
            "{}",
            retirement.content
        );
        assert!(retirement.content.contains(&other_cell));
        assert!(
            retirement.content.contains(&prose),
            "operator prose is kept"
        );
        assert!(retirement.content.contains(&format!("- ~~{struck}~~\n")));

        // A later operator row with the same text has no evidence to inherit.
        let readded = retirement.content.replace(
            &format!("- ~~{struck}~~\n"),
            &format!("- ~~{struck}~~\n- {struck}\n"),
        );
        let evidence = agent_doc_document::subagent_completion::completion_evidence(&readded);
        assert!(
            evidence.iter().all(|evidence| evidence.item != struck),
            "{evidence:?}"
        );
    }

    /// A strike that leaves a live row with the same work identity (a sibling
    /// queue's mirror copy, a duplicate) keeps the evidence for that row, and
    /// a deleted row is cancellation (`#queue-removal-cancels`), not a strike.
    #[test]
    fn strike_retirement_keeps_evidence_of_live_or_deleted_rows() {
        let item = "finish the sdk";
        let cell = agent_doc_document::subagent_completion::cell("subagent:sdk", item, "sdk-key");
        let exchange = format!("{cell}\n");
        let before = strike_retirement_doc(&exchange, &format!("- {item}\n- {item}\n"));

        let duplicate_live = strike_retirement_doc(&exchange, &format!("- ~~{item}~~\n- {item}\n"));
        let retirement = retire_struck_rows_completion_feedback(&before, &duplicate_live);
        assert!(retirement.struck_heads.is_empty());
        assert_eq!(retirement.content, duplicate_live);

        let deleted = strike_retirement_doc(&exchange, "");
        let retirement = retire_struck_rows_completion_feedback(&before, &deleted);
        assert!(retirement.struck_heads.is_empty());
        assert_eq!(retirement.content, deleted);

        // A row already struck before the successor is not this strike's.
        let already = strike_retirement_doc(&exchange, &format!("- ~~{item}~~\n"));
        assert!(heads_struck_by_successor(&already, &already).is_empty());
    }

    /// `#strikeretires`: an annotated strike struck the same work. The
    /// backlog/done strike (`#qftbklgstrike`) bakes its note inside the
    /// strikethrough and the answered strike (`#qstrikenote`) appends it
    /// after; either way the struck row is the work text, so its evidence
    /// retires. Before, the baked-in note made the struck identity differ from
    /// the live row's, and the strike closed nothing.
    #[test]
    fn annotated_strikes_strike_the_work_text() {
        let item = "Fix JB File Cache Conflict dialogs on every save";
        let before = strike_retirement_doc(
            "",
            &format!("- ☑️ [#subagent:jbcache#1] {item}\n- do [#stillopen]\n"),
        );
        for struck in [
            format!("- ~~{item} — auto-struck: completed by #jbcache (#qftbklgstrike)~~\n"),
            format!("- ~~{item}~~ — auto-struck: answered this cycle (#ftstrike)\n"),
        ] {
            let after = strike_retirement_doc("", &format!("{struck}- do [#stillopen]\n"));
            assert_eq!(
                heads_struck_by_successor(&before, &after),
                vec![item.to_string()],
                "{struck}"
            );
        }
    }

    /// A preflight-selected head can join the live queue after the response
    /// baseline (for example through queue projection). An exact prompt echo
    /// plus answer proves that this is selected work, not an operator draft
    /// that should be deferred to the next auto-queue cycle.
    #[test]
    fn selected_head_added_after_baseline_is_struck_by_exact_response_evidence() {
        let baseline = concat!(
            "---\nqueue_active: true\nprompt_presets:\n",
            "  '#gh-fix': fix then close\n",
            "---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- unrelated later work\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: issue 177 — gpt-6.1-sol\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/177\n\n",
            "Fixed on main in b013de126 and closed.\n",
        );
        let current = format!(
            concat!(
                "---\nqueue_active: true\nprompt_presets:\n",
                "  '#gh-fix': fix then close\n",
                "---\n\n",
                "<!-- agent:exchange -->\n{response}<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n",
                "- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/177\n",
                "- unrelated later work\n",
                "<!-- /agent:queue -->\n",
            ),
            response = response,
        );

        let selection_authority =
            ["#gh-fix https://github.com/btakita/agent-doc/issues/177".to_string()];
        let projected = project_answered_free_text_strike_with_authority(
            &current,
            response,
            Some(baseline),
            &[],
            &selection_authority,
        )
        .unwrap()
        .expect("a selected and explicitly answered late head should be consumed");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(projected.target_content.contains(
            "- ~~#gh-fix https://github.com/btakita/agent-doc/issues/177~~ — auto-struck: answered this cycle (#ftstrike)"
        ));
        assert!(projected.target_content.contains("- unrelated later work"));

        assert!(
            project_answered_free_text_strike(&current, response, Some(baseline))
                .unwrap()
                .is_none(),
            "a lifecycle marker without durable selection authority cannot consume post-baseline work"
        );

        let operator_draft = current.replacen("- 🚧 #gh-fix", "- #gh-fix", 1);
        assert!(
            project_answered_free_text_strike(&operator_draft, response, Some(baseline))
                .unwrap()
                .is_none(),
            "an unselected head absent from the baseline remains next-cycle work"
        );
    }

    #[test]
    fn selected_plain_free_text_added_after_baseline_is_struck_by_exact_echo() {
        let head = "Transfer this sdk demo thread/context/backlog/todo/exchange/etc to sdk-demo.md";
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n<!-- /agent:queue -->\n",
        );
        let response = format!(
            "### Re: sdk transfer — gpt-6.1-sol\n\n> **Queue prompt:** {head}\n\nTransfer prepared and verified.\n"
        );
        let current = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange -->\n{response}<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n- 🚧 {head}\n<!-- /agent:queue -->\n",
            ),
            response = response,
            head = head,
        );

        let projected = project_answered_free_text_strike_with_authority(
            &current,
            &response,
            Some(baseline),
            &[],
            &[head.to_string()],
        )
        .unwrap()
        .expect("durably selected late free-text head should close atomically");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(
            !projected.target_content.contains(&format!("- 🚧 {head}")),
            "the final answered head should be removed by lossless queue drain: {}",
            projected.target_content
        );
        assert!(
            project_answered_free_text_strike(&current, &response, Some(baseline))
                .unwrap()
                .is_none(),
            "the visible marker and exact echo are insufficient without durable selection"
        );
    }

    #[test]
    fn answered_free_text_auto_strike_skips_loop_copy_and_strikes_ordinary_copy() {
        let head = "#upgrade";
        let response =
            format!("### Re: upgrade — gpt-6.1-sol\n\n> **Queue prompt:** {head}\n\nUpgraded.\n");
        let document = format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange -->\n{response}<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue loop interval=1h id=hourly -->\n",
                "- {head}\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:queue go id=release response=\"inline\" -->\n",
                "- {head}\n",
                "<!-- /agent:queue -->\n",
            ),
            response = response,
            head = head,
        );

        let projected = project_answered_free_text_strike(&document, &response, Some(&document))
            .unwrap()
            .expect("the ordinary queue copy should be struck");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(projected.target_content.contains(concat!(
            "<!-- agent:queue loop interval=1h id=hourly -->\n",
            "- #upgrade\n",
            "<!-- /agent:queue -->",
        )));
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                &response,
            ))
            .response_id()
            .to_string();
        let attribute_id = response_id.trim_start_matches("[#").trim_end_matches(']');
        assert!(projected.target_content.contains(&format!(
            "<!-- agent:queue go id=release response=\"inline\" -->\n- ~~#upgrade~~ — auto-struck: answered this cycle (#ftstrike); response: {response_id}\n  - **Response {response_id}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - <details data-agent-doc-response-id=\"{attribute_id}\">\n      <summary>Response</summary>\n      \n      ### Re: upgrade — gpt-6.1-sol\n      \n      > **Queue prompt:** #upgrade\n      \n      Upgraded.\n      </details>\n<!-- /agent:queue -->"
        )));
    }

    #[test]
    fn answered_free_text_projection_without_queue_component_is_a_noop() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-optional\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: document-only prompt — gpt-5\n\n",
            "> **Queue prompt:** document-only prompt\n\n",
            "Answered without queue scheduling.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] unrelated backlog item [#unrelated]\n",
            "<!-- /agent:backlog -->\n",
        );
        let response = concat!(
            "### Re: document-only prompt — gpt-5\n\n",
            "> **Queue prompt:** document-only prompt\n\n",
            "Answered without queue scheduling.\n",
        );

        assert_eq!(
            project_answered_free_text_strike(document, response, Some(document)).unwrap(),
            None,
            "a missing optional queue component has no lifecycle state to project"
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

        // `#queuestopremove`: the torn legacy inactive flag is no control, so
        // the live remaining head re-arms the default `go`.
        assert!(projected.target_content.contains("queue: go"));
        assert!(
            projected
                .target_content
                .contains("- ~~completed work~~ — auto-struck: answered this cycle (#ftstrike)")
        );
        assert!(projected.target_content.contains("- remaining work"));
    }

    #[test]
    fn answered_free_text_projection_drains_terminal_queue_in_same_projection() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-projection\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: final question — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> final question\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority start auto -->\n",
            "- final question\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: final question — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> final question\n\n",
            "Answered.\n",
        );

        let projected = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("terminal answered head should project a drained queue");

        assert!(
            !projected.target_content.contains("queue:"),
            "a terminal drain clears the control (#queuestopremove):\n{}",
            projected.target_content
        );
        assert!(
            projected
                .target_content
                .contains("<!-- agent:queue priority -->\n<!-- /agent:queue -->")
        );
        assert!(!projected.target_content.contains("final question~~"));
        assert!(
            project_answered_free_text_strike(&projected.target_content, response, Some(document))
                .unwrap()
                .is_none(),
            "terminal drain must be idempotent"
        );
    }

    /// GH #361: finalization's document-wide auto-strike never owns a `loop`
    /// row. The pass-owned normal consume is what strikes it for restoration.
    #[test]
    fn answered_free_text_projection_does_not_strike_a_loop_queue_row() {
        let document = concat!(
            "---\n",
            "agent_doc_session: queue-projection\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: final question — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> final question\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:loop interval=1h -->\n",
            "- final question\n",
            "<!-- /agent:loop -->\n",
        );
        let response = concat!(
            "### Re: final question — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> final question\n\n",
            "Answered.\n",
        );

        assert!(
            project_answered_free_text_strike(document, response, Some(document))
                .unwrap()
                .is_none(),
            "#ftstrike must leave loop rows live for pass-owned consumption"
        );
    }

    /// `#ftstrikeloopindex` live shape (agent-doc-bugs.md cycle-1793042953867,
    /// 2026-10-26): the exchange holds an `agent:boundary` marker, a run queue
    /// follows, then a `loop` queue. Node keys number components over the
    /// overlay (which counts the boundary), the loop exclusion numbered them
    /// over `element::parse` (which does not), so the loop's prefix named the
    /// run queue and no echoed row struck: seven claimed-and-reported `☑️`
    /// rows and one unclaimed `📥` row all stayed live.
    const LOOP_INDEX_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: earlier — opus\n\nEarlier answer.\n",
        "<!-- agent:boundary:3ec64169 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "## Queue: Main <!-- agent:run id=\"main\" subagents priority go -->\n",
        "- ☑️ [#subagent:reportedend#1] end a Reported queue claim on every strike path, so a later identical row can never pick up ☑️ from the claim ledger\n",
        "- ☑️ [#subagent:cellrepair#1] one-time repair: retire stale subagent completion cells left behind by strikes\n",
        "- 🚧 [#subagent:doctorexit#1] `agent-doc doctor` exits 0 even when its outcome is blocked\n",
        "- 📥 I deleted a queue item in docs.md and added some of the deleted queue item prompt into an existing queue item\n",
        "<!-- /agent:run -->\n\n",
        "## Loop: Issues <!-- agent:loop id=\"issues\" interval=\"1h\" priority -->\n",
        "- 📥 Watch the issue tracker for new issues and add them to the backlog\n",
        "<!-- /agent:loop -->\n",
    );

    const LOOP_INDEX_RESPONSE: &str = concat!(
        "### Re: integration batch — opus\n\n",
        "> **Queue prompt:** end a Reported queue claim on every strike path, so a later identical row can never pick up ☑️ from the claim ledger\n\n",
        "On main and installed.\n\n",
        "> **Queue prompt:** one-time repair: retire stale subagent completion cells left behind by strikes\n\n",
        "**Deferred:** the branch waits for your decision on the command name.\n\n",
        "> **Queue prompt:** I deleted a queue item in docs.md and added some of the deleted queue item prompt into an existing queue item\n\n",
        "Same cause and fix as the attach item; it re-attaches now.\n\n",
        "> **Queue prompt:** Watch the issue tracker for new issues and add them to the backlog\n\n",
        "No new issues this pass.\n",
    );

    #[test]
    fn ftstrike_strikes_echoed_run_rows_beside_a_loop_queue_after_the_exchange_boundary() {
        for claimed in [
            Vec::new(),
            vec![
                "☑️ [#subagent:reportedend#1] end a Reported queue claim on every strike path, so a later identical row can never pick up ☑️ from the claim ledger".to_string(),
                "☑️ [#subagent:cellrepair#1] one-time repair: retire stale subagent completion cells left behind by strikes".to_string(),
            ],
        ] {
            let projected = project_answered_free_text_strike_with_claims(
                LOOP_INDEX_DOC,
                LOOP_INDEX_RESPONSE,
                Some(LOOP_INDEX_DOC),
                &claimed,
            )
            .unwrap()
            .expect("the echoed run-queue rows must strike (#claimstrike, #ftstrike)");
            let target = &projected.target_content;
            assert_eq!(projected.node_keys.len(), 2, "{:?}", projected.node_keys);
            assert!(
                target.contains("- ~~end a Reported queue claim on every strike path"),
                "the echoed reported claimed row strikes:\n{target}"
            );
            assert!(
                target.contains("- ~~I deleted a queue item in docs.md"),
                "the echoed unclaimed row strikes:\n{target}"
            );
            assert!(
                target.contains("- ☑️ [#subagent:cellrepair#1] one-time repair"),
                "the **Deferred:** row stays live (#deferstrike):\n{target}"
            );
            assert!(
                target.contains("- 🚧 [#subagent:doctorexit#1]"),
                "the unechoed running row stays live:\n{target}"
            );
            assert!(
                target.contains("- 📥 Watch the issue tracker for new issues"),
                "GH #361: the loop row is never #ftstrike'd:\n{target}"
            );
        }
    }

    /// GH #364: preset-expansion evidence in ordinary response prose must not
    /// bypass loop ownership and terminalize the repeating queue.
    #[test]
    fn preset_prose_cannot_auto_strike_or_stop_a_loop_queue_gh364() {
        let document = concat!(
            "---\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc. Are the current issues fixed? Check for new issues. File a gh issue for any new issues in btakita/agent-doc.\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: upgrade pass — gpt-5\n\n",
            "Upgrade agent-doc. Are the current issues fixed? Check for new issues. File a gh issue for any new issues in btakita/agent-doc.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue loop interval=1h id=queue-2 priority -->\n",
            "- #upgrade\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: upgrade pass — gpt-5\n\n",
            "Upgrade agent-doc. Are the current issues fixed? Check for new issues. File a gh issue for any new issues in btakita/agent-doc.\n",
        );

        assert!(
            project_answered_free_text_strike(document, response, Some(document))
                .unwrap()
                .is_none(),
            "plain preset-expansion prose must not #ftstrike or terminalize a loop queue"
        );
    }

    /// `presetquotestrike` (live, agent-doc-bugs.md 2026-10-02, agent-doc
    /// 0.35.434): one response quoted all five live heads, each in its own
    /// singular `> **Queue prompt:**` block, then answered them together. Only
    /// the plain head (the last block) struck; the four `#gh-fix <url>` preset
    /// heads were kept even though each exact text was quoted.
    const PRESET_QUOTE_STRIKE_RESPONSE: &str = concat!(
        "### Re: parallel drain — GH #80/#87/#93/#94, IDEA background — opus-5.5\n\n",
        "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/80\n\n",
        "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/87\n\n",
        "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/93\n\n",
        "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/94\n\n",
        "> **Queue prompt:** The Idea Plugin formatting is not active. The element content should have a background. It's gone now.\n\n",
        "**All of it shipped, is installed, and GH #80, #87, #93 and #94 are closed.**\n",
    );

    fn preset_quote_strike_document() -> String {
        concat!(
            "---\n",
            "agent_doc_session: preset-quote-strike\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#gh-fix': fix then closerelease\n",
            "  '#spec-test-build-install-commit-push': spec, test, build, install, commit, push\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#trackedwork] tracked work item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" priority go -->\n",
            "- ~~#gh-fix https://github.com/btakita/agent-doc/issues/92~~\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/80\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/87\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/93\n",
            "- #gh-fix https://github.com/btakita/agent-doc/issues/94\n",
            "- The Idea Plugin formatting is not active. The element content should have a background. It's gone now.\n",
            "- release + publish\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string()
    }

    fn assert_all_five_heads_struck(projected: &AnsweredFreeTextStrikeProjection) {
        assert_eq!(
            projected.node_keys.len(),
            5,
            "every quoted head strikes:\n{}",
            projected.target_content
        );
        for issue in [80, 87, 93, 94] {
            let struck = format!(
                "- ~~#gh-fix https://github.com/btakita/agent-doc/issues/{issue}~~ — auto-struck: {STRUCK_FREE_TEXT_NOTE}"
            );
            assert!(
                projected.target_content.contains(&struck),
                "preset head #{issue} must strike:\n{}",
                projected.target_content
            );
        }
        assert!(projected.target_content.contains("- release + publish\n"));
        assert!(projected.target_content.contains(
            "- ~~The Idea Plugin formatting is not active. The element content should have a background. It's gone now.~~"
        ));
    }

    #[test]
    fn quoted_preset_token_heads_strike_alongside_plain_free_text() {
        let document = preset_quote_strike_document();
        let projected = project_answered_free_text_strike(
            &document,
            PRESET_QUOTE_STRIKE_RESPONSE,
            Some(&document),
        )
        .unwrap()
        .expect("quoted heads should project a strike");
        assert_all_five_heads_struck(&projected);
    }

    #[test]
    fn preset_expansion_evidence_strikes_without_literal_head_echo() {
        let document = concat!(
            "---\nqueue_active: true\n",
            "prompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc and verify the current issues.\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- [#upgrade]\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "### Re: upgrade\n\n",
            "Upgrade agent-doc and verify the current issues. Completed and tested."
        );

        let projected = project_answered_free_text_strike(document, response, Some(document))
            .unwrap()
            .expect("resolved expansion evidence should strike the preset head");
        assert_eq!(projected.node_keys.len(), 1);
        assert!(
            !projected.target_content.contains("[#upgrade]"),
            "the sole consumed head is losslessly cleared with the queue body:\n{}",
            projected.target_content
        );
    }

    /// The live respond auto-reopened a fresh cycle whose baseline was refreshed
    /// to HEAD and carried no preflight-selected queue prompt. The strike must
    /// not depend on which head preflight selected.
    #[test]
    fn quoted_preset_token_heads_strike_in_an_auto_reopened_cycle() {
        let document = preset_quote_strike_document();
        for baseline in [None, Some(document.as_str())] {
            let projected = project_answered_free_text_strike(
                &document,
                PRESET_QUOTE_STRIKE_RESPONSE,
                baseline,
            )
            .unwrap()
            .expect("quoted heads should project a strike");
            assert_all_five_heads_struck(&projected);
        }
    }

    /// The `#bareechodrop` guard still holds: a free-text head quoted in a bare
    /// singular block that runs on into an id-backed directive's echo was
    /// listed, not answered, and the id-backed head is never struck by a quote.
    #[test]
    fn quoted_run_ending_in_id_backed_echo_still_strikes_nothing() {
        let document = preset_quote_strike_document().replace(
            "- The Idea Plugin",
            "- do [#trackedwork]\n- The Idea Plugin",
        );
        let response = concat!(
            "### Re: tracked work — opus-5.5\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/80\n\n",
            "> **Queue prompt:** do [#trackedwork]\n\n",
            "Did the tracked work only.\n",
        );
        let projected =
            project_answered_free_text_strike(&document, response, Some(&document)).unwrap();
        assert!(
            projected.as_ref().is_none_or(|p| p.node_keys.is_empty()),
            "nothing strikes: {:?}",
            projected.map(|p| p.target_content)
        );
    }

    /// `#structuredqueueanswer` (live, haiven-dev api.md 2026-10-30): a
    /// coordinator recorded many explicit prompt/response pairs. A later
    /// id-backed pair made every earlier free-text pair look like the bare
    /// listing guarded by `#bareechodrop`, so integrated reported rows stayed
    /// `☑️` indefinitely.
    #[test]
    fn structured_queue_response_before_id_backed_echo_strikes_its_reported_row() {
        let document = preset_quote_strike_document().replace(
            "- The Idea Plugin",
            "- do [#trackedwork]\n- The Idea Plugin",
        );
        let response = concat!(
            "### Re: coordinated closeout — opus-5.5\n\n",
            "> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/80\n",
            ">\n",
            "> **Queue response:** Fixed, verified, and published.\n\n",
            "> **Queue prompt:** do [#trackedwork]\n",
            ">\n",
            "> **Queue response:** The tracked item remains open.\n",
        );

        let projected = project_answered_free_text_strike(&document, response, Some(&document))
            .unwrap()
            .expect("the explicitly answered preset row must strike");

        assert_eq!(projected.node_keys.len(), 1, "{}", projected.target_content);
        assert!(
            projected
                .target_content
                .contains("- ~~#gh-fix https://github.com/btakita/agent-doc/issues/80~~")
        );
        assert!(projected.target_content.contains("- do [#trackedwork]\n"));
    }

    /// `#annotateddeferstrike` live shapes (agent-doc-bugs.md 2026-10-11): an
    /// annotated head answered with the canonical three-part shape -- the
    /// `> **Queue prompt:**` echo, the `> **Operator note:**` quote that
    /// `#qheadannotation` requires, then the `**Deferred:**` paragraph -- was
    /// consumed because the operator-note quote sat between the echo and the
    /// deferral.
    fn annotated_defer_doc(head: &str) -> String {
        format!(
            concat!(
                "---\nqueue_active: true\n---\n\n",
                "<!-- agent:queue go -->\n",
                "- {}\n",
                "- later work\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#clearnoticeverify] [operator-verify] Eyeball the queued Clear notices\n",
                "<!-- /agent:backlog -->\n",
            ),
            head
        )
    }

    fn annotated_defer_response(head: &str, note: &str, deferred: bool) -> String {
        let tail = if deferred {
            "**Deferred:** this went to a background subagent (claim `subagent:clearnotice`). It will report the notice evidence.\n"
        } else {
            "Verified: the queued notice wording and the single `/clear` at idle both match.\n"
        };
        format!(
            "### Re: #clearnoticeverify — testing it myself — opus-5.5\n\n> **Queue prompt:** {head}\n\n> **Operator note:** {note}\n\n{tail}"
        )
    }

    #[test]
    fn annotateddeferstrike_do_head_with_operator_note_then_deferral_is_kept() {
        let head = "do [#clearnoticeverify]: Test yourself";
        let doc = annotated_defer_doc(head);
        let response = annotated_defer_response(head, "Test yourself", true);
        assert!(
            crate::queue_head_annotation::annotation_addressed_in(&response, "Test yourself"),
            "the deferral paragraph still answers the operator note (#qheadannotation)"
        );
        assert!(
            crate::queue_response::response_defers_free_text_head(&response, head),
            "the deferral after the operator-note quote is recognized"
        );
        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&doc),
                &doc,
                &response,
                &[],
            )
            .unwrap(),
            "a deferred annotated id head is not consumed by its `### Re: #id` heading"
        );
        assert_eq!(
            queue_targeted_completion_id_for_current_head(
                Path::new("session.md"),
                Some(&doc),
                &doc,
                &response,
                &[],
            )
            .unwrap(),
            None,
            "the heading of a deferred head is not a completion id"
        );
        // An explicit `--done` still resolves the head: the flag closes the
        // backlog item itself.
        let done = vec!["clearnoticeverify".to_string()];
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&doc),
                &doc,
                &response,
                &done,
            )
            .unwrap(),
            "an explicit --done naming the head still consumes it"
        );
        assert_eq!(
            queue_targeted_completion_id_for_current_head(
                Path::new("session.md"),
                Some(&doc),
                &doc,
                &response,
                &done,
            )
            .unwrap()
            .as_deref(),
            Some("clearnoticeverify")
        );
    }

    #[test]
    fn annotateddeferstrike_free_text_annotated_head_with_operator_note_then_deferral_is_kept() {
        let head = "[#clearnoticeverify]: verified?";
        let doc = annotated_defer_doc(head);
        let response = annotated_defer_response(head, "verified?", true);
        assert!(
            crate::queue_response::queue_prompt_text_is_free_text(&doc, head)
                || crate::queue_head_annotation::queue_head_annotation(head).is_none(),
            "the `[#id]: note` shape is answered through the free-text quote path"
        );
        assert!(
            answered_free_text_head_node_keys(&doc, &response, Some(&doc))
                .unwrap()
                .is_empty(),
            "the deferred head is not auto-struck (#ftstrike)"
        );
        assert!(
            !queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&doc),
                &doc,
                &response,
                &[],
            )
            .unwrap(),
            "the deferred head is not consumed"
        );
        assert!(
            crate::queue_response::latest_free_text_head_echo_is_deferral(&response, head),
            "the committed exchange keeps the head out of the completed residue"
        );
    }

    #[test]
    fn annotateddeferstrike_plain_free_text_head_with_operator_note_then_deferral_is_kept() {
        let head = "release + publish";
        let doc = annotated_defer_doc(head);
        let response = annotated_defer_response(head, "and tag it", true);
        assert!(
            answered_free_text_head_node_keys(&doc, &response, Some(&doc))
                .unwrap()
                .is_empty(),
            "an operator-note quote between echo and deferral does not hide the deferral"
        );
    }

    #[test]
    fn annotateddeferstrike_operator_note_answer_without_deferral_is_still_consumed() {
        let head = "[#clearnoticeverify]: verified?";
        let doc = annotated_defer_doc(head);
        let response = annotated_defer_response(head, "verified?", false);
        assert!(!crate::queue_response::response_defers_free_text_head(
            &response, head
        ));
        assert_eq!(
            answered_free_text_head_node_keys(&doc, &response, Some(&doc))
                .unwrap()
                .len(),
            1,
            "an answered annotated free-text head still strikes"
        );

        let id_head = "do [#clearnoticeverify]: Test yourself";
        let id_doc = annotated_defer_doc(id_head);
        let id_response = annotated_defer_response(id_head, "Test yourself", false);
        assert!(!crate::queue_response::response_defers_free_text_head(
            &id_response,
            id_head
        ));
        assert!(
            queue_consumption_allowed_for_response(
                Path::new("session.md"),
                Some(&id_doc),
                &id_doc,
                &id_response,
                &["clearnoticeverify".to_string()],
            )
            .unwrap(),
            "an answered annotated id head closed with --done is consumed"
        );
    }

    /// The operator-note skip is narrow: the deferral of the NEXT quoted head
    /// is not borrowed by the head before it.
    #[test]
    fn annotateddeferstrike_next_heads_deferral_does_not_defer_the_answered_head() {
        let response = concat!(
            "> **Queue prompt:** first answered head here\n\n",
            "Done: the first head is finished.\n\n",
            "> **Queue prompt:** second head\n\n",
            "**Deferred:** the second head waits for review.\n",
        );
        assert!(!crate::queue_response::response_defers_free_text_head(
            response,
            "first answered head here"
        ));
        let quote_only_then_other_echo = concat!(
            "> **Queue prompt:** first answered head here\n\n",
            "> **Queue prompt:** second head\n\n",
            "**Deferred:** the second head waits for review.\n",
        );
        assert!(!crate::queue_response::response_defers_free_text_head(
            quote_only_then_other_echo,
            "first answered head here"
        ));
    }

    /// `#multilinequeuehead` (live, agent-doc-bugs.md 2026-10-12): the response
    /// quoted a multi-line head (title line + pasted fenced log) as its
    /// `> **Queue prompt:**` echo. Matching worked, but the strike appended the
    /// `#ftstrike` note to the `~~~done` fence's closing `~~~` line. A closing
    /// fence line cannot carry text, so the fence never closed, swallowed
    /// `<!-- /agent:queue -->`, and the projection failed with `unclosed
    /// component`. Closeout committed with the head live and session-check
    /// INTERRUPTED on completed free-text residue.
    #[test]
    fn multilinequeuehead_echo_strikes_a_multiline_head_into_a_closed_done_fence() {
        use crate::document_queue::multiline_pasted_fixture::{BETA_HEAD, QUEUE_BODY};
        let response = concat!(
            "### Re: batch landed\n\n",
            "> **Queue prompt:** alpha.md issue\n",
            "> ```\n",
            "> • Blocked by hook\n",
            ">   └ Queue continuation, not an error: agent-doc kept the active `agent:queue auto` moving.\n",
            "> The next queue prompt is \"do [#nextitem]\". Continue THIS turn in-pane and\n",
            "> do not send the final answer yet.\n",
            "> ```\n\n",
            "Fixed. The continuation message is now one short paragraph.\n",
        );
        let content = doc_with_queue_and_exchange(QUEUE_BODY, response);
        let alpha = crate::queue_heads::active_free_text_queue_heads(&content)
            .into_iter()
            .find(|head| head.contains("alpha.md issue"))
            .expect("alpha head is live");
        assert!(
            crate::queue_heads::free_text_queue_head_is_completed_residue(
                &content, response, &alpha
            )
        );

        let projection = project_answered_free_text_strike(&content, response, Some(&content))
            .expect("strike projection must not fail on a multi-line head")
            .expect("the echoed multi-line head is struck");
        let target = &projection.target_content;
        let components = element::parse(target).expect("struck document stays well-formed");
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        let body = queue.content(target);
        assert!(
            body.contains("~~~done\nalpha.md issue\n```\n"),
            "struck as a `~~~done` fence without the in-progress marker:\n{body}"
        );
        assert!(
            !body.contains(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR),
            "the fence is its own completion marker:\n{body}"
        );
        assert!(body.contains("```\n~~~\nbeta.md issue\n"), "{body}");
        assert_eq!(
            crate::queue_heads::active_free_text_queue_heads(target),
            vec![BETA_HEAD.to_string()]
        );
        // Strike and residue agree: nothing answered remains, and the
        // projection is a fixed point.
        assert!(
            !crate::queue_heads::free_text_queue_head_is_completed_residue(
                target, response, &alpha
            )
        );
        assert!(
            answered_free_text_head_node_keys(target, response, Some(target))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn answered_bare_paragraph_strikes_only_its_own_multiline_node() {
        let queue = concat!(
            "- Does the queue split free text after a list item?\n",
            "The terminal windows closed because a scope was left behind.\n",
            "> diagnostic detail\n",
        );
        let response = concat!(
            "### Re: terminal scope\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> The terminal windows closed because a scope was left behind.\n",
            "> > diagnostic detail\n\n",
            "The scope cleanup now runs at the ownership boundary.\n",
        );
        let content = doc_with_queue_and_exchange(queue, response);
        let projection = project_answered_free_text_strike(&content, response, Some(&content))
            .unwrap()
            .expect("the exact bare paragraph echo should strike its node");
        let heads = crate::queue_heads::active_free_text_queue_heads(&projection.target_content);
        assert_eq!(
            heads,
            vec!["Does the queue split free text after a list item?".to_string()]
        );
        assert!(
            projection.target_content.contains(concat!(
                "~~~done\n",
                "The terminal windows closed because a scope was left behind.\n",
                "> diagnostic detail\n",
                "~~~\n",
            )),
            "{}",
            projection.target_content
        );
    }

    #[test]
    fn multilinequeuehead_annotation_never_touches_a_done_fence() {
        let fence = "~~~done\nalpha.md issue\n```\nlog\n```\n~~~\n";
        assert_eq!(annotate_struck_free_text_line(fence), fence);
        assert_eq!(annotate_struck_free_text_line("~~~\n"), "~~~\n");
        let before = queue_doc("---\nalpha.md issue\n```\nlog\n```\n---\n");
        let after = queue_doc(fence);
        assert_eq!(
            annotate_newly_struck_free_text_heads(&before, &after).unwrap(),
            after
        );
    }
}
