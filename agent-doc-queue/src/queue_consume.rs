//! Pure queue prompt consumption policy.
//!
//! This module owns entry-level consume planning helpers. Callers still own
//! file IO, snapshot persistence, IPC transport, and editor convergence.

use std::collections::{BTreeSet, HashMap, HashSet};
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
        ConsumedPromptResponseEcho, normalize_done_id, normalize_queue_prompt_text,
        queue_prompt_done_id, queue_prompt_text_is_free_text, queue_prompt_text_matches,
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
/// Binary-owned sentinels for the closeout-maintained operator-work tree.
///
/// Generated response metadata stays readable and unstruck: the sentinels make
/// those evidence rows inert to scheduling, distinguish them from
/// operator-authored response edges, and keep refresh/removal byte-addressable
/// without introducing another queue entry type.
pub const WAITING_ON_YOU_ROOT_MARKER: &str = "<!-- agent-doc:waiting-on-you -->";
pub const WAITING_ON_YOU_ITEM_MARKER: &str = agent_doc_markdown_ast::reply_row::WAITING_ITEM_MARKER;
/// Trailing glyph that is the only ownership mark of a generated packet row
/// (`#barereplymarker`). Recognition lives in
/// [`agent_doc_markdown_ast::reply_row`]; legacy rows that still carry a
/// hidden reply comment are recognized there and re-rendered bare.
pub const AGENT_DOC_REPLY_GLYPH: &str = agent_doc_markdown_ast::reply_row::AGENT_DOC_REPLY_GLYPH;
/// Legacy hidden reply comment: accepted on read, never rendered.
pub const LEGACY_AGENT_DOC_REPLY_MARKER: &str =
    agent_doc_markdown_ast::reply_row::LEGACY_REPLY_MARKER;
/// Older legacy ownership comment: accepted on read, never rendered.
pub const LEGACY_WAITING_METADATA_MARKER: &str =
    agent_doc_markdown_ast::reply_row::LEGACY_WAITING_METADATA_MARKER;

fn agent_doc_reply_suffix() -> String {
    AGENT_DOC_REPLY_GLYPH.to_string()
}
pub(crate) const WAITING_ON_YOU_ROOT_TEXT: &str =
    "Waiting on you — answer by adding a child line under an item";

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

/// The document-level result of reconciling terminal operator-request rows
/// with their same-id `agent:todo` receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOperatorRequestTodoReconciliation {
    pub content: String,
    pub completed_ids: Vec<String>,
}

/// Check off operator-request todos whose exact request/decision queue row has
/// reached a terminal strike.
///
/// `terminal_heads` carries rows a projection just consumed, including queues
/// which clear completed rows instead of retaining their strike. Already
/// materialized struck rows are also inspected so preflight can repair stale
/// residue from an older binary. A live duplicate of the same request id wins:
/// the todo stays open until every projected copy is terminal. Only a todo's
/// own reserved request id is eligible; cited ids and unrelated todos are never
/// changed.
pub fn reconcile_terminal_operator_request_todos(
    content: &str,
    terminal_heads: &[String],
) -> Result<TerminalOperatorRequestTodoReconciliation> {
    let rows = queue_rows(content);
    let mut terminal_ids = BTreeSet::new();
    let mut live_ids = HashSet::new();

    for head in terminal_heads {
        if let Some(id) = exact_operator_request_row_id(head) {
            terminal_ids.insert(id);
        }
    }
    for (struck, text) in rows {
        let Some(id) = exact_operator_request_row_id(&text) else {
            continue;
        };
        if struck {
            terminal_ids.insert(id);
        } else {
            live_ids.insert(id);
        }
    }

    let open_todo_ids = agent_doc_element::todo::open_todo_ids(content);
    let actionable_ids = terminal_ids
        .into_iter()
        .filter(|id| !live_ids.contains(id) && open_todo_ids.contains(id))
        .collect::<Vec<_>>();
    let mut reconciled = if actionable_ids.is_empty() {
        content.to_string()
    } else {
        // A prior interrupted cycle can leave duplicate todo receipts carrying
        // the same own id. Fold them before selecting the id so the ordinary
        // mutation API remains fail-closed for genuinely ambiguous selectors.
        agent_doc_element::todo::reconcile_todo_in_document(content)?.0
    };
    let mut completed_ids = Vec::new();
    for id in actionable_ids {
        let outcome = agent_doc_element::todo::set_todo_item_state_in_document(
            &reconciled,
            &id,
            agent_doc_element::todo::TodoTransition::Done,
        )?;
        reconciled = outcome.content;
        completed_ids.push(id);
    }

    Ok(TerminalOperatorRequestTodoReconciliation {
        content: reconciled,
        completed_ids,
    })
}

/// A request/decision row is deliberately narrower than an arbitrary row
/// which merely cites a request id. Accept only the reserved id as the row's
/// complete head, optionally followed by the canonical `: answer` form.
fn exact_operator_request_row_id(text: &str) -> Option<String> {
    let head = agent_doc_element_queue::subprompts::head_without_subprompts(text);
    let head = agent_doc_element_queue::strip_attempt_projection_prefix(head).trim();
    let (id, after) = head.strip_prefix("[#")?.split_once(']')?;
    if !agent_doc_document::queue_projection::is_reserved_operator_action_id(id) {
        return None;
    }
    let after = after.trim_start();
    (after.is_empty() || after.starts_with(':')).then(|| id.to_ascii_lowercase())
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
        agent_doc_markdown_ast::mutations::item_nodes_all(before, component)
            .map_err(|err| anyhow::anyhow!("node diff: failed to parse before: {err}"))?
            .into_iter()
            .map(|n| {
                (
                    n.node_key,
                    before[n.item.start_byte..n.item.end_byte].to_string(),
                )
            })
            .collect();
    let after_nodes = agent_doc_markdown_ast::mutations::item_nodes_all(after, component)
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

fn prompt_response_echo_at(
    entries: &[QueueEntry],
    index: usize,
    identity: String,
) -> ConsumedPromptResponseEcho {
    let display = document_queue::nest_vector_at(entries, index, &identity)
        .unwrap_or_else(|| identity.clone());
    ConsumedPromptResponseEcho { identity, display }
}

/// Response echoes for the first `count` executable prompts, captured from the
/// concrete pre-consume rows rather than looked up by leaf text afterwards.
pub fn first_n_queue_prompt_response_echoes(
    entries: &[QueueEntry],
    count: usize,
) -> Vec<ConsumedPromptResponseEcho> {
    document_queue::executable_prompt_indexes(entries)
        .into_iter()
        .filter_map(|index| match entries.get(index) {
            Some(QueueEntry::Prompt(prompt)) => Some(prompt_response_echo_at(
                entries,
                index,
                strip_in_progress_marker(&prompt.text),
            )),
            _ => None,
        })
        .take(count)
        .collect()
}

/// Response echoes for every live prompt selected by stable done id.
///
/// The predicate intentionally mirrors [`mark_entries_completed_by_done_ids`]
/// so explicit ids remain authoritative while each selected occurrence keeps
/// its own parent-qualified display vector.
pub fn queue_prompt_response_echoes_for_done_ids(
    entries: &[QueueEntry],
    done_ids: &[String],
) -> Vec<ConsumedPromptResponseEcho> {
    if done_ids.is_empty() {
        return Vec::new();
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            QueueEntry::Prompt(prompt)
                if queue_prompt_done_id(&prompt.text).is_some_and(|id| done_ids.contains(&id)) =>
            {
                Some(prompt_response_echo_at(
                    entries,
                    index,
                    struck_work_text(&prompt.text).to_string(),
                ))
            }
            _ => None,
        })
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
        let mut occurrences = std::collections::HashMap::<String, usize>::new();
        let row_heads = crate::queue_response::queue_heads_by_normalized_prompt_text(doc);
        let loop_prefixes = loop_queue_node_prefixes(doc);
        if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(doc, "queue") {
            for node in nodes.into_iter().filter(|node| {
                !loop_prefixes
                    .iter()
                    .any(|prefix| node.node_key.starts_with(prefix))
            }) {
                let text = node.item.text.trim();
                let normalized = crate::queue_response::normalized_authored_queue_prompt_text(text);
                let occurrence = occurrences.entry(normalized.clone()).or_default();
                let head = row_heads
                    .get(&normalized)
                    .and_then(|heads| heads.get(*occurrence))
                    .cloned()
                    .unwrap_or_else(|| text.to_string());
                *occurrence += 1;
                if !node.item.struck {
                    continue;
                }
                *counts
                    .entry(normalize_queue_prompt_text(&head))
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
    let mut occurrences = std::collections::HashMap::<String, usize>::new();
    let row_heads = crate::queue_response::queue_heads_by_normalized_prompt_text(content);
    for (node_index, node) in nodes.iter().enumerate() {
        if is_loop_queue_node(&node.node_key) {
            continue;
        }
        let text = node.item.text.trim();
        let normalized = crate::queue_response::normalized_authored_queue_prompt_text(text);
        let occurrence = occurrences.entry(normalized.clone()).or_default();
        let head = row_heads
            .get(&normalized)
            .and_then(|heads| heads.get(*occurrence))
            .cloned()
            .unwrap_or_else(|| text.to_string());
        *occurrence += 1;
        if node.item.struck {
            continue;
        }
        // A row with descendants is their context, not executable work.  Its
        // prose may appear in a submitted nest vector, but that must never turn
        // the parent into an answered queue head.
        if mutation_node_is_group_parent(content, &nodes, node_index) {
            continue;
        }
        if text.is_empty() || !crate::queue_response::queue_prompt_text_is_free_text(content, &head)
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
        if struck_texts.contains(&normalize_queue_prompt_text(&head)) {
            continue;
        }
        // `#bugautostruck`: the in-progress marker proves only which queue head
        // was selected for this cycle. It is not evidence that the response
        // addressed that head. Require exact quoted-prompt proof for ordinary
        // free text, or resolved-expansion proof for a prompt-preset head.
        let exact_vector_claimed = claimed.claims(&head);
        let legacy_bare_claimed = !exact_vector_claimed && claimed.claims(text);
        let head_has_explicit_answer_evidence =
            crate::queue_response::free_text_head_has_explicit_answer_evidence(
                response_body,
                &head,
            ) || crate::queue_response::response_explicitly_echoes_nest_vector(
                content,
                response_body,
                &head,
            ) || crate::queue_response::response_explicitly_echoes_nested_row_tree(
                response_body,
                &head,
            );
        let bare_has_explicit_answer_evidence =
            crate::queue_response::free_text_head_has_explicit_answer_evidence(response_body, text)
                || crate::queue_response::response_explicitly_echoes_nest_vector(
                    content,
                    response_body,
                    text,
                );
        let has_explicit_answer_evidence = head_has_explicit_answer_evidence
            || (!exact_vector_claimed && bare_has_explicit_answer_evidence);
        let row_claimed = exact_vector_claimed || legacy_bare_claimed;
        let row_selected = selected.claims(&head) || selected.claims(text);
        let head_answered = crate::queue_response::queue_head_answered_by_response_with_claim(
            content,
            response_body,
            &head,
            exact_vector_claimed,
        ) || (exact_vector_claimed
            && crate::queue_response::response_explicitly_echoes_nested_row_tree(
                response_body,
                &head,
            ));
        let bare_answered = !exact_vector_claimed
            && crate::queue_response::queue_head_answered_by_response_with_claim(
                content,
                response_body,
                text,
                legacy_bare_claimed,
            );
        // `#adresponsechildlive`: this projection scans every queue row. Do
        // not let a later row close merely because closeout prose, a `### Re:`
        // heading, or an unlabeled quote names it. Claimed work remains on the
        // stricter exact-echo contract; unclaimed registered presets may still
        // use their resolved-expansion proof.
        let has_conservative_strike_evidence = if row_claimed {
            has_explicit_answer_evidence
        } else {
            has_explicit_answer_evidence
                || crate::queue_response::free_text_head_has_conservative_strike_evidence(
                    content,
                    response_body,
                    &head,
                )
                || crate::queue_response::free_text_head_has_conservative_strike_evidence(
                    content,
                    response_body,
                    text,
                )
        };
        if !has_conservative_strike_evidence {
            continue;
        }
        if !head_answered && !bare_answered {
            continue;
        }
        // `#ftstrikedefer`: the quoted echo is the responding agent's assertion
        // that it answered the head, not proof. When the response says next to
        // that quote that the head is still outstanding, believe the sentence
        // over the echo and leave it queued. A nest-vector quote is that leaf's
        // echo (`#nestvector`).
        let nested_tree_evidence =
            crate::queue_response::response_explicitly_echoes_nested_row_tree(response_body, &head);
        let deferred =
            crate::queue_response::response_defers_queue_head(content, response_body, &head)
                || (!exact_vector_claimed
                    && crate::queue_response::response_defers_queue_head(
                        content,
                        response_body,
                        text,
                    ))
                || (exact_vector_claimed
                    && nested_tree_evidence
                    && crate::queue_response::response_defers_queue_head(
                        content,
                        response_body,
                        text,
                    ));
        if deferred {
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
            && !crate::queue_response::free_text_head_present_in_baseline(baseline, &head)
            && !crate::queue_response::free_text_head_present_in_baseline(baseline, text)
            && !((row_claimed || row_selected) && has_explicit_answer_evidence)
        {
            continue;
        }
        // `#subagentinlineveto`: an answered head the `subagents` attribute
        // owns, with no worker attempt behind it, was run inline. The answer
        // is real but the lifecycle is not: keep the row queued for dispatch
        // and report it, never close it as if a subagent had run it.
        if dispatch_owned.claims(text) && !row_claimed {
            decision.inline_vetoed_heads.push(
                agent_doc_document::queue_projection::strip_priority_markers(text)
                    .trim()
                    .to_string(),
            );
            continue;
        }
        struck_texts.insert(normalize_queue_prompt_text(&head));
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

    if let Ok(nodes) = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue") {
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

/// Whether a terminal queue row is owned by one of the exact durable head
/// identities supplied by the caller.
///
/// This is deliberately narrower than substring/search matching: controller
/// replay uses it only to prove that a selected or claimed head introduced
/// after the response baseline already reached its terminal live successor.
pub fn struck_queue_contains_owned_head(content: &str, heads: &[String]) -> bool {
    if heads.is_empty() {
        return false;
    }
    let owned = crate::queue_claim::ClaimedQueueItems::none().with_heads(heads);
    agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
        .ok()
        .is_some_and(|nodes| {
            nodes
                .into_iter()
                .any(|node| node.item.struck && owned.claims(node.item.text.trim()))
        })
}

/// Whether this exact response carries answer evidence for an already terminal
/// free-text queue row.
///
/// This is restart proof for a split landing: the live queue may already hold
/// the strike while a separately persisted recovery snapshot still needs the
/// same terminal projection. Pending rows are excluded before answer matching,
/// so a weak mention or dispatch veto cannot manufacture a terminal effect.
pub fn struck_queue_contains_head_answered_by(content: &str, response_body: &str) -> bool {
    agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
        .ok()
        .is_some_and(|nodes| {
            nodes.into_iter().any(|node| {
                node.item.struck
                    && crate::queue_response::free_text_head_answered_by_response(
                        response_body,
                        node.item.text.trim(),
                    )
            })
        })
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
    // A terminal request/decision row and its operator-owned receipt are one
    // lifecycle fact. Reconcile them in this same derived target so the async
    // strike effect cannot publish a struck request beside an open todo.
    let annotated = reconcile_terminal_operator_request_todos(&annotated, &consumed_heads)?.content;
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
        .map(|line| {
            let (core, newline) = line
                .strip_suffix('\n')
                .map_or((line, ""), |core| (core, "\n"));
            if agent_doc_markdown_ast::exchange_tree::response_id_from_heading(core.trim())
                .is_some()
            {
                format!(
                    "{}{}",
                    agent_doc_markdown_ast::exchange_tree::strip_response_id_from_heading(core),
                    newline
                )
            } else {
                line.to_string()
            }
        })
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

/// Collapse historical copies of one queue response projection to one
/// canonical node per public response id.
///
/// The first full `**Response [#r-…]**` row in document order is retained.
/// Later full rows become inert `**Closed by:** [#r-…]` backlinks and only
/// binary-owned metadata beneath those rows is removed. Unknown descendants
/// remain byte-for-byte so an operator reply nested under a generated choice
/// or action cannot be discarded by the migration.
pub fn canonicalize_duplicate_queue_response_projections(content: &str) -> Result<String> {
    let components = element::parse(content)?;
    let mut seen = HashSet::<String>::new();
    let mut edits = Vec::new();
    for queue in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = queue.content(content);
        let repaired = canonicalize_duplicate_response_projections_in_queue(body, &mut seen);
        if repaired != body {
            edits.push((queue.open_end, queue.close_start, repaired));
        }
    }
    let mut repaired = content.to_string();
    for (start, end, body) in edits.into_iter().rev() {
        repaired.replace_range(start..end, &body);
    }
    Ok(repaired)
}

/// Public response ids that own more than one full queue response node.
pub fn duplicate_queue_response_projection_ids(content: &str) -> Result<BTreeSet<String>> {
    let mut seen = HashSet::<String>::new();
    let mut duplicates = BTreeSet::new();
    for queue in element::parse(content)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        for line in queue.content(content).lines() {
            let item = line.trim();
            let Some(item) = item
                .strip_prefix("- ")
                .or_else(|| item.strip_prefix("* "))
                .or_else(|| item.strip_prefix("+ "))
            else {
                continue;
            };
            let Some(response_id) =
                agent_doc_markdown_ast::exchange_tree::response_id_from_lookup_marker(item)
            else {
                continue;
            };
            if !seen.insert(response_id.to_string()) {
                duplicates.insert(response_id.to_string());
            }
        }
    }
    Ok(duplicates)
}

fn canonicalize_duplicate_response_projections_in_queue(
    body: &str,
    seen: &mut HashSet<String>,
) -> String {
    let lines = body.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0usize);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }

    let mut parents = vec![None; lines.len()];
    let mut stack = Vec::<(usize, usize)>::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if !trimmed.starts_with("- ") && !trimmed.starts_with("* ") && !trimmed.starts_with("+ ") {
            continue;
        }
        let indent = line.len() - trimmed.len();
        while stack
            .last()
            .is_some_and(|(prior_indent, _)| *prior_indent >= indent)
        {
            stack.pop();
        }
        parents[index] = stack.last().map(|(_, parent)| *parent);
        stack.push((indent, index));
    }

    let response_id_at = |index: usize| -> Option<String> {
        let item = lines.get(index)?.trim();
        let item = item
            .strip_prefix("- ")
            .or_else(|| item.strip_prefix("* "))
            .or_else(|| item.strip_prefix("+ "))?;
        agent_doc_markdown_ast::exchange_tree::response_id_from_lookup_marker(item)
            .map(str::to_string)
    };
    let response_ids = (0..lines.len()).map(response_id_at).collect::<Vec<_>>();
    let mut duplicate_markers = HashMap::<usize, String>::new();
    for (index, response_id) in response_ids.iter().enumerate() {
        let Some(response_id) = response_id else {
            continue;
        };
        if !seen.insert(response_id.clone()) {
            duplicate_markers.insert(index, response_id.clone());
        }
    }
    if duplicate_markers.is_empty() {
        return body.to_string();
    }

    let root_response_marker = |mut index: usize| -> Option<usize> {
        loop {
            if response_ids.get(index).is_some_and(Option::is_some) {
                return Some(index);
            }
            index = parents.get(index).copied().flatten()?;
        }
    };
    let mut generated = agent_doc_markdown_ast::overlay::response_details_child_ranges(body);
    generated.extend(agent_doc_markdown_ast::overlay::response_row_child_ranges(
        body,
    ));
    generated.sort_unstable_by_key(|(start, end)| (*start, std::cmp::Reverse(*end)));
    generated.dedup();

    let mut replacements = Vec::<(usize, usize, String)>::new();
    let mut removed = Vec::<(usize, usize)>::new();
    for (start, end) in generated {
        let Ok(line_index) = offsets.binary_search(&start) else {
            continue;
        };
        let Some(owner) = root_response_marker(line_index) else {
            continue;
        };
        let Some(response_id) = duplicate_markers.get(&owner) else {
            continue;
        };
        if line_index == owner {
            let line = &body[start..end];
            let marker =
                agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id)
                    .expect("parsed response lookup ids are canonical");
            replacements.push((
                start,
                end,
                line.replacen(&marker, &format!("**Closed by:** {response_id}"), 1),
            ));
        } else if !removed
            .iter()
            .any(|(outer_start, outer_end)| *outer_start <= start && end <= *outer_end)
        {
            removed.push((start, end));
        }
    }
    replacements.extend(
        removed
            .into_iter()
            .map(|(start, end)| (start, end, String::new())),
    );
    replacements.sort_unstable_by_key(|(start, _, _)| *start);
    let mut repaired = body.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        repaired.replace_range(start..end, &replacement);
    }
    repaired
}

/// Detach binary-owned response projections for response ids whose durable
/// evidence no longer exists.
///
/// Only generated metadata beneath `agent:queue` is eligible: response detail
/// or summary children, canonical lookup rows, and `response: [#r-…]`
/// annotations on already-struck rows. Open queue work and operator-authored
/// descendants remain byte-identical. Callers must independently prove that
/// every supplied id is absent from live/indexed response evidence and is not
/// pinned by a decision before publishing this projection.
pub fn detach_struck_queue_response_projections(
    content: &str,
    response_ids: &BTreeSet<String>,
) -> Result<String> {
    if response_ids.is_empty() {
        return Ok(content.to_string());
    }
    let components = element::parse(content)?;
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    let body = queue.content(content);
    let mut generated_ranges = response_lookup_projection_ranges(body, response_ids);
    generated_ranges.extend(
        agent_doc_markdown_ast::overlay::response_details_child_ranges(body)
            .into_iter()
            .filter(|(start, end)| {
                response_ids.iter().any(|response_id| {
                    generated_projection_owns_response_id(&body[*start..*end], response_id)
                })
            }),
    );
    generated_ranges.sort_unstable();
    generated_ranges.dedup();

    let mut detached = body.to_string();
    for (start, end) in generated_ranges.into_iter().rev() {
        if generated_projection_has_struck_parent(body, start) {
            detached.replace_range(start..end, "");
        }
    }

    let detached = detached
        .split_inclusive('\n')
        .map(|line| detach_response_annotations_from_struck_line(line, response_ids))
        .collect::<String>();
    Ok(queue.replace_content(content, &detached))
}

fn response_lookup_projection_ranges(
    body: &str,
    response_ids: &BTreeSet<String>,
) -> Vec<(usize, usize)> {
    let lines = body.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in &lines {
        offsets.push(offsets.last().copied().unwrap_or_default() + line.len());
    }
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line
            .trim()
            .strip_prefix("- ")
            .unwrap_or_else(|| line.trim());
        let owns_target = response_ids.iter().any(|response_id| {
            agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id).as_deref()
                == Some(trimmed)
        });
        if !owns_target {
            index += 1;
            continue;
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let mut end = index + 1;
        while end < lines.len() {
            let next = lines[end];
            if next.trim().is_empty() {
                end += 1;
                continue;
            }
            let next_indent = next.len() - next.trim_start_matches([' ', '\t']).len();
            if next_indent <= indent {
                break;
            }
            end += 1;
        }
        ranges.push((offsets[index], offsets[end]));
        index = end;
    }
    ranges
}

fn generated_projection_owns_response_id(projection: &str, response_id: &str) -> bool {
    let first = projection
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .strip_prefix("- ")
        .unwrap_or_else(|| projection.lines().next().unwrap_or_default().trim());
    let lookup = agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id);
    if lookup.as_deref() == Some(first) {
        return true;
    }
    let Some(attribute_id) = response_id
        .strip_prefix("[#")
        .and_then(|id| id.strip_suffix(']'))
    else {
        return false;
    };
    first.starts_with("<details ")
        && first.contains(&format!("data-agent-doc-response-id=\"{attribute_id}\""))
}

fn generated_projection_has_struck_parent(body: &str, projection_start: usize) -> bool {
    let projection_line = body[projection_start..].lines().next().unwrap_or_default();
    let projection_indent =
        projection_line.len() - projection_line.trim_start_matches([' ', '\t']).len();
    body[..projection_start].lines().rev().find_map(|line| {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - trimmed.len();
        ((trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
            && indent < projection_indent)
            .then_some(line_is_struck_queue_row(line))
    }) == Some(true)
}

fn line_is_struck_queue_row(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let body = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed)
        .trim_start();
    body.strip_prefix("~~")
        .is_some_and(|struck| struck.contains("~~"))
}

fn detach_response_annotations_from_struck_line(
    line: &str,
    response_ids: &BTreeSet<String>,
) -> String {
    let (core, newline) = line
        .strip_suffix('\n')
        .map_or((line, ""), |core| (core, "\n"));
    if !line_is_struck_queue_row(core) {
        return line.to_string();
    }
    let Some(strike_end) = core.rfind("~~").map(|offset| offset + 2) else {
        return line.to_string();
    };
    let mut repaired = core.to_string();
    for response_id in response_ids {
        let needle = format!("{RESPONSE_ID_NOTE_PREFIX}{response_id}");
        let Some(note_start) = repaired[strike_end..].find(&needle).map(|i| strike_end + i) else {
            continue;
        };
        if !repaired[note_start + needle.len()..].trim().is_empty() {
            continue;
        }
        let prefix = &repaired[strike_end..note_start];
        if prefix.ends_with("; ") {
            repaired.truncate(note_start - 2);
        } else if prefix == agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR {
            repaired.truncate(strike_end);
        }
    }
    format!("{repaired}{newline}")
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
    let keys = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
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
    if !element::parse(content)?
        .iter()
        .any(|component| component.name == "queue")
    {
        return Ok(Vec::new());
    }
    let mut live = Vec::new();
    for node in agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")? {
        if !node.item.struck
            && let Some(id) = queue_prompt_done_id(&node.item.text)
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
        if !element::parse(content)?
            .iter()
            .any(|component| component.name == "queue")
        {
            return Ok((HashSet::new(), HashSet::new()));
        }
        let mut live = HashSet::new();
        let mut completed = HashSet::new();
        for node in agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")? {
            if let Some(id) = queue_prompt_done_id(&node.item.text)
                && done_ids.contains(&id)
            {
                if node.item.struck {
                    completed.insert(id);
                } else {
                    live.insert(id);
                }
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
    let nodes = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
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
    let nodes = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
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
    let nodes = agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue")
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

/// Node keys for the exact reported worker-attempt projection nested beneath
/// an open waiting/Todo parent identified by `target_id` (`#nestedechostrike`).
///
/// Waiting-tree parents are intentionally free text while their generated
/// waiting metadata is present, and the child owns a distinct subagent id. The
/// ordinary id-backed selector therefore finds neither row for
/// `--ack-id <parent>`. This selector admits only the binary-owned Reported row
/// whose durable owner/generation was proved by the IO guard and which cites
/// the exact parent id.
pub fn reported_attempt_projection_node_keys_for_parent_id(
    content: &str,
    target_id: &str,
    owner: &str,
    generation: u64,
) -> Result<Vec<String>> {
    let target_id =
        agent_doc_element_backlog::backlog::normalize_pending_id(target_id).to_ascii_lowercase();
    if target_id.is_empty() {
        return Ok(Vec::new());
    }
    let nodes =
        agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue").map_err(|err| {
            anyhow::anyhow!("reported-attempt reap: failed to derive queue node keys: {err}")
        })?;
    Ok(nodes
        .into_iter()
        .filter(|node| !node.item.struck)
        .filter(|node| {
            let text = node.item.text.trim();
            agent_doc_document::queue_projection::lifecycle_marker(text)
                == Some(agent_doc_document::queue_projection::QueueLifecycleMarker::Reported)
                && agent_doc_document::queue_projection::owner_projection(text).is_some_and(
                    |projection| projection.owner == owner && projection.generation == generation,
                )
                && agent_doc_element_backlog::backlog::extract_pending_hash_ids(text)
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target_id))
        })
        .map(|node| node.node_key)
        .collect())
}

/// `#nestedreceiptack`: node keys of the binary-owned Reported projection of
/// attempt `owner`/`generation` anywhere inside the subtree of a live queue row
/// whose directive id is `target_id`.
///
/// [`reported_attempt_projection_node_keys_for_parent_id`] requires the
/// projection to cite the parent id, which holds for the direct
/// `Resolve the open operator action ... [#id]` child. A waiting-tree parent
/// whose generated `**Response:**` structure nests the attempt deeper renders
/// it as `Reconcile the reported work, then complete the parent item.` with no
/// id (2026-11-03, agent-doc-bugs.md `subagent:vscode_workspace#1`). The
/// owner/generation pair is already proven by the caller's durable evidence, so
/// containment under the exact parent is the remaining structural proof.
pub fn reported_attempt_projection_node_keys_under_parent_id(
    content: &str,
    target_id: &str,
    owner: &str,
    generation: u64,
) -> Result<Vec<String>> {
    let target_id =
        agent_doc_element_backlog::backlog::normalize_pending_id(target_id).to_ascii_lowercase();
    if target_id.is_empty() {
        return Ok(Vec::new());
    }
    let nodes =
        agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue").map_err(|err| {
            anyhow::anyhow!("reported-attempt reap: failed to derive queue node keys: {err}")
        })?;
    let indent = |start: usize| {
        content
            .get(start..)
            .and_then(|tail| tail.lines().next())
            .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
            .unwrap_or(0)
    };
    let mut keys = reported_attempt_projection_node_keys_for_parent_id(
        content, &target_id, owner, generation,
    )?;
    for (index, parent) in nodes.iter().enumerate() {
        if parent.item.struck
            || !queue_prompt_done_id(parent.item.text.trim())
                .is_some_and(|id| id.eq_ignore_ascii_case(&target_id))
        {
            continue;
        }
        let parent_indent = indent(parent.item.start_byte);
        for node in nodes[index + 1..]
            .iter()
            .take_while(|node| indent(node.item.start_byte) > parent_indent)
        {
            let text = node.item.text.trim();
            if !node.item.struck
                && agent_doc_document::queue_projection::lifecycle_marker(text)
                    == Some(agent_doc_document::queue_projection::QueueLifecycleMarker::Reported)
                && agent_doc_document::queue_projection::owner_projection(text).is_some_and(
                    |projection| projection.owner == owner && projection.generation == generation,
                )
                && !keys.contains(&node.node_key)
            {
                keys.push(node.node_key.clone());
            }
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
    // Top-level queue rows carry a navigation heading before every lifecycle
    // marker.  The heading is presentation, so skip it while recognizing the
    // struck payload that receives the deterministic explanation suffix.
    let content = content.strip_prefix("### ").unwrap_or(content);
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
    #[derive(Clone, Copy)]
    struct Line<'a> {
        start: usize,
        text: &'a str,
    }

    let mut lines = Vec::new();
    let mut cursor = 0usize;
    for text in response.split_inclusive('\n') {
        lines.push(Line {
            start: cursor,
            text,
        });
        cursor += text.len();
    }
    if cursor < response.len() {
        lines.push(Line {
            start: cursor,
            text: &response[cursor..],
        });
    }

    let prompt_echo_lines = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let quoted = line.text.trim_start().strip_prefix('>')?.trim_start();
            (quoted.starts_with("**Queue prompt:**") || quoted.starts_with("**Queue prompts:**"))
                .then_some(index)
        })
        .collect::<Vec<_>>();

    // Without explicit prompt anchors the whole turn is one intentional
    // response cell, including any H3 verification or artifact subsections.
    if prompt_echo_lines.is_empty() {
        return (!response.trim().is_empty())
            .then(|| QueueAddressedResponseCell {
                body: response.to_string(),
                heading_count: lines
                    .iter()
                    .filter(|line| line.text.trim().starts_with("### "))
                    .count(),
            })
            .into_iter()
            .collect();
    }

    // Prompt echoes, not headings, are the durable per-head boundary. A
    // response may legitimately add `### Verification`, `### Artifacts`, or
    // other H3 subsections; splitting at every H3 detached those subsections
    // from their owning head and made their recovery evidence disappear. For
    // canonical heading→echo responses, pull the immediately preceding H3
    // across the boundary. Echo→heading retained payloads naturally keep the
    // following heading in the same cell.
    let mut starts = Vec::with_capacity(prompt_echo_lines.len());
    for (echo_ordinal, &echo_line) in prompt_echo_lines.iter().enumerate() {
        let mut start_line = echo_line;
        while start_line > 0 && lines[start_line - 1].text.trim().is_empty() {
            start_line -= 1;
        }
        if start_line > 0 && lines[start_line - 1].text.trim().starts_with("### ") {
            start_line -= 1;
        }
        let start = if echo_ordinal == 0 {
            0
        } else {
            lines[start_line].start
        };
        starts.push(start);
    }
    starts.dedup();

    starts
        .iter()
        .enumerate()
        .filter_map(|(index, &start)| {
            let end = starts.get(index + 1).copied().unwrap_or(response.len());
            let body = &response[start..end];
            (!body.trim().is_empty()).then(|| QueueAddressedResponseCell {
                body: body.to_string(),
                heading_count: body
                    .lines()
                    .filter(|line| line.trim().starts_with("### "))
                    .count(),
            })
        })
        .collect()
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
        binding_key: usize,
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
    let mut inherited_policy = None;
    let mut response_candidates = Vec::new();
    for (binding_key, cell) in queue_addressed_response_cells(response_body)
        .into_iter()
        .enumerate()
    {
        let policy = if cell.heading_count == 0 {
            let Some(policy) = inherited_policy.clone() else {
                continue;
            };
            policy
        } else {
            if response_node_cursor + cell.heading_count > response_nodes.len() {
                continue;
            }
            let nodes =
                &response_nodes[response_node_cursor..response_node_cursor + cell.heading_count];
            response_node_cursor += cell.heading_count;
            let policy =
                agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response_nodes(
                    nodes,
                );
            inherited_policy = Some(policy.clone());
            policy
        };
        let has_scoped_outcome = cell.body.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && !response_summary_structural_line(line)
        });
        if !policy.response_id().is_empty() && has_scoped_outcome {
            response_candidates.push(ResponseCandidate {
                body: cell.body,
                policy,
                binding_key,
            });
        }
    }
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
        binding_key: usize::MAX,
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
                // `#answerack`: an answer this projection struck already names
                // its consuming response in its `**Consumed by:**` child.
                && !after
                    .get(node.item.end_byte..)
                    .and_then(|rest| rest.trim_start_matches('\n').lines().next())
                    .is_some_and(|line| {
                        line.trim_start()
                            .strip_prefix("- ")
                            .is_some_and(crate::answer_ack::is_consumed_by)
                    })
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
    // `#answerack`: operator answers inside waiting packets that this response
    // consumed. An answer whose stable id the response cites is consumed by the
    // one section citing it (or by the whole response when several do); an
    // answer beneath a row this closeout terminalizes is consumed below.
    let live_answers = crate::answer_ack::live_packet_answers(after).unwrap_or_default();
    let mut answer_acks = Vec::<(crate::answer_ack::AnswerRow, ResponseCandidate)>::new();
    for answer in &live_answers {
        let cited = |body: &str| {
            answer
                .ids
                .iter()
                .any(|id| crate::answer_ack::cites_id(body, id))
        };
        let matching = response_candidates
            .iter()
            .filter(|candidate| cited(&candidate.body))
            .collect::<Vec<_>>();
        let candidate = match matching.as_slice() {
            [one] => (*one).clone(),
            _ if cited(&composite_candidate.body) => composite_candidate.clone(),
            _ => continue,
        };
        answer_acks.push((answer.row, candidate));
    }
    if newly_struck.is_empty() && operator_request_nodes.is_empty() && answer_acks.is_empty() {
        return Ok(after.to_string());
    }

    // A closeout may carry several independently authored `### Re:` sections.
    // Bind each newly completed prompt to the one section that explicitly
    // targets or echoes it. The former composite-cell projection gave every
    // prompt the aggregate turn's id and summary, which made unrelated queue
    // decisions indistinguishable. Preserve intentional one-section batch
    // closeouts (for example several explicit `--done` ids), but a closeout
    // that supplies distinct sections must map those sections one-to-one.
    type ProjectionIdentity = (agent_doc_element_queue::QueueItemIdentity, Option<usize>);
    let projection_identity = |text: &str| -> ProjectionIdentity {
        let (base, effect) = document_queue::split_queue_effect_annotation(text);
        (
            agent_doc_element_queue::queue_prompt_stable_identity(&base),
            effect,
        )
    };
    let mut candidates_by_node_key =
        Vec::<(String, ResponseCandidate, bool, ProjectionIdentity)>::new();
    for node in &newly_struck {
        // Queue rows can move between parents while an editor cut and a
        // retained closeout converge. The response echo then names the row's
        // admitted ancestry while `after` exposes its new ancestry. Correlate
        // the two cuts by the durable q-id when present, otherwise by the
        // normalized work-text identity. If duplicate text existed in the old
        // cut, the candidate set below still has to collapse to exactly one
        // response section; ambiguity remains a refusal.
        let stable_identity = projection_identity(node.item.work_text());
        let prior_spellings = before_nodes
            .iter()
            .filter(|prior| projection_identity(prior.item.work_text()) == stable_identity)
            .map(|prior| prior.item.text.as_str())
            .collect::<Vec<_>>();
        let prior_targets = prior_spellings
            .iter()
            .flat_map(|prior| {
                let mut targets = vec![(*prior).to_string()];
                if let Some(vector) = crate::queue_response::nest_vector_in_document(before, prior)
                    && !targets.contains(&vector)
                {
                    targets.push(vector);
                }
                targets
            })
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
                ) || prior_targets.iter().any(|prior| {
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
            [index] => response_candidates[*index].clone(),
            [] if stable_identity.1.is_none()
                && (response_candidates.len() <= 1 || explicitly_addressed_sections <= 1) =>
            {
                composite_candidate.clone()
            }
            [] if explicitly_addressed_sections == 0 => composite_candidate.clone(),
            [] => anyhow::bail!(
                "newly completed queue prompt has no unique response section: {}",
                node.item.text.trim()
            ),
            _ => anyhow::bail!(
                "newly completed queue prompt matches multiple response sections: {}",
                node.item.text.trim()
            ),
        };
        let has_local_report_state =
            agent_doc_element_queue::queue_report_state_annotation(node.item.work_text())
                .is_some_and(|(_, state)| {
                    matches!(
                        state,
                        agent_doc_element_queue::QueueReportState::Local
                            | agent_doc_element_queue::QueueReportState::PublishedLocal
                    )
                });
        if has_local_report_state || response_has_unpublished_work_evidence(&candidate.body) {
            anyhow::bail!(
                "refusing to terminalize unpublished queue work: {}. Keep the row live until it has a scoped commit; when commit authorization is absent, retain the row with a blocking operator request. After commit, retain a tracked integration action until a PR or named integration-branch receipt exists",
                node.item.text.trim()
            );
        }
        candidates_by_node_key.push((node.node_key.clone(), candidate, true, stable_identity));
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
            .any(|(node_key, _, _, _)| node_key == &node.node_key)
        {
            // Live request rows get the child subtree only. The inline
            // `; response: [#r-…]` completion note belongs to terminal rows.
            candidates_by_node_key.push((
                node.node_key.clone(),
                candidate,
                false,
                projection_identity(node.item.work_text()),
            ));
        }
    }

    for (node_key, candidate, terminal, _) in &candidates_by_node_key {
        if !*terminal {
            continue;
        }
        let Some(node) = nodes.iter().find(|node| &node.node_key == node_key) else {
            continue;
        };
        for row in crate::answer_ack::answers_under_row(after, node.item.start_byte, &live_answers)?
        {
            let unpicked = live_answers
                .iter()
                .any(|answer| answer.row == row && !answer.picked_up);
            if unpicked && !answer_acks.iter().any(|(acked, _)| *acked == row) {
                answer_acks.push((row, candidate.clone()));
            }
        }
    }

    let mut after = after.to_string();
    let mut identities_by_binding = HashMap::<usize, HashSet<ProjectionIdentity>>::new();
    for (_, candidate, _, identity) in &candidates_by_node_key {
        identities_by_binding
            .entry(candidate.binding_key)
            .or_default()
            .insert(identity.clone());
    }
    let mut projection_by_node_key = HashMap::<String, (String, String, bool, bool)>::new();
    for (node_key, candidate, annotate_inline_id, _) in candidates_by_node_key {
        let response_id = response_lookup_id_for_document(&after, &candidate.policy)?;
        if response_id.is_empty() {
            continue;
        }
        after = annotate_exchange_response_with_lookup_id(&after, &candidate.policy, &response_id)?;
        let shared_batch = identities_by_binding
            .get(&candidate.binding_key)
            .is_some_and(|identities| identities.len() > 1);
        projection_by_node_key.insert(
            node_key,
            (
                candidate.body,
                response_id,
                annotate_inline_id,
                shared_batch,
            ),
        );
    }

    let mut resolved_acks = Vec::new();
    for (row, candidate) in answer_acks {
        let response_id = response_lookup_id_for_document(&after, &candidate.policy)?;
        if response_id.is_empty() {
            continue;
        }
        after = annotate_exchange_response_with_lookup_id(&after, &candidate.policy, &response_id)?;
        resolved_acks.push((row, response_id));
    }
    after = crate::answer_ack::apply_answer_acknowledgements(&after, &resolved_acks)?;

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
    // projected below several queue rows. Render its choices only on the first
    // row that owns the request; unrelated or duplicate rows receive nothing.
    let mut rendered_choice_packet_ids = HashSet::<String>::new();
    // A response is likewise one document node even when it closes several
    // queue rows. The first row in document order owns the full response tree;
    // later rows retain only a searchable, inert backlink. Repeating the full
    // tree gave one public response id several apparent definitions and let a
    // later response become nested inside an arbitrary duplicate.
    let mut response_projection_counts = HashMap::<String, usize>::new();
    for (_, response_id, _, _) in projection_by_node_key.values() {
        *response_projection_counts
            .entry(response_id.clone())
            .or_default() += 1;
    }
    // `#guidancescope`: deletion guidance is scoped by the queue tree, not by
    // the closing row's own response alone.
    let closure_tree =
        crate::queue_tree::queue_tree(&after, &crate::queue_tree::QueueTreeOptions::default()).ok();
    let mut canonical_response_owners = HashMap::<String, String>::new();
    let mut request_response_owners = HashMap::<String, String>::new();
    for node in &nodes {
        let Some((_, response_id, _, _)) = projection_by_node_key.get(&node.node_key) else {
            continue;
        };
        canonical_response_owners
            .entry(response_id.clone())
            .or_insert_with(|| node.node_key.clone());
        if operator_request_packets
            .iter()
            .any(|packet| operator_request_packet_belongs_to_row(packet, &node.item.text))
        {
            request_response_owners
                .entry(response_id.clone())
                .or_insert_with(|| node.node_key.clone());
        }
    }
    // When one of the rows owns an operator request, put the canonical response
    // there so the full response tree and its decision stay together. If more
    // than one row owns a distinct request, later owners keep only their scoped
    // choices beneath the response backlink.
    canonical_response_owners.extend(request_response_owners);
    for node in &nodes {
        let Some((projected_response_body, response_id, _annotate_inline_id, shared_batch)) =
            projection_by_node_key.get(&node.node_key)
        else {
            continue;
        };
        let raw = &after[node.item.start_byte..node.item.end_byte];
        let indent = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let descendant_end = descendant_region_end(&after, node.item.end_byte, indent);
        let owned_scope_end = descendant_end.max(node.item.end_byte);
        let owned_scope = &after[node.item.start_byte..owned_scope_end];
        // The addressed-at timestamp is metadata for this response projection,
        // not part of its public identity. A detached replay can recompute the
        // same response/request after the wall clock crosses a second boundary.
        // Compare the complete rendered subtree with only generated timestamp
        // and tree-scoped guidance lines normalized so choices, summaries,
        // details, and response ids must still match exactly.
        let normalized_owned_scope = normalize_response_projection_volatile_metadata(owned_scope);
        let child_indent = " ".repeat(indent + 2);
        let response_mode = queue_response_ranges
            .iter()
            .find(|&&(start, end, _)| node.item.start_byte >= start && node.item.start_byte < end)
            .map_or(QueueResponseMode::Summary, |&(_, _, mode)| mode);
        let canonical_owner = canonical_response_owners
            .get(response_id)
            .is_some_and(|owner| owner == &node.node_key);
        let lookup_children = if canonical_owner {
            let repeated_response = response_projection_counts
                .get(response_id)
                .is_some_and(|count| *count > 1);
            response_lookup_children(
                ResponseLookupContext {
                    source_document: before,
                    queue_item: &node.item.text,
                    // Distinct addressed cells can inherit one heading and
                    // therefore one response id. Once their projection is
                    // canonicalized, the owner must retain the complete turn,
                    // including any request packet that appeared in a later
                    // addressed cell.
                    response_body: if repeated_response {
                        response_body
                    } else {
                        projected_response_body
                    },
                    response_id,
                    addressed_at,
                    shared_batch: if repeated_response {
                        false
                    } else {
                        *shared_batch
                    },
                    closure: closure_tree.as_ref().and_then(|tree| {
                        crate::queue_tree::row_closure_at_offset(tree, &after, node.item.start_byte)
                    }),
                },
                response_mode,
                &mut rendered_choice_packet_ids,
            )
        } else {
            vec![response_back_reference_child(
                &node.item.text,
                projected_response_body,
                response_id,
                &mut rendered_choice_packet_ids,
            )]
        };
        let missing_children = lookup_children
            .iter()
            .filter(|child| {
                let rendered = render_response_lookup_child(child, &child_indent);
                !normalized_owned_scope
                    .contains(&normalize_response_projection_volatile_metadata(&rendered))
            })
            .cloned()
            .collect::<Vec<_>>();
        let has_inline_id = raw.contains(&format!("{RESPONSE_ID_NOTE_PREFIX}{response_id}"));
        if has_inline_id || !missing_children.is_empty() {
            edits.push((
                node.item.start_byte,
                node.item.end_byte,
                indent,
                has_inline_id,
                response_id.clone(),
                missing_children,
            ));
        }
    }

    let mut out = after;
    for (start, end, indent, has_inline_id, response_id, children) in edits.into_iter().rev() {
        let mut replacement = if has_inline_id {
            remove_inline_response_id(&out[start..end], &response_id)
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

fn response_back_reference_child(
    queue_item: &str,
    response_body: &str,
    response_id: &str,
    rendered_choice_packet_ids: &mut HashSet<String>,
) -> String {
    debug_assert!(agent_doc_markdown_ast::exchange_tree::is_compact_response_id(response_id));
    // `#waitingpacketowner`: a back-reference links the closing response. It
    // carries a Choices block only for a request this row itself owns and that
    // no other row of this response has rendered, and then only that packet's
    // own options. It never substitutes another decision's choices (which
    // grafted one item's options onto its siblings) and never re-copies the
    // offered options beneath a row that is itself one of those options.
    let mut child = format!("**Closed by:** {response_id}");
    for packet in crate::operator_request_packet::blocking_operator_request_packets(response_body)
        .into_iter()
        .filter(|packet| operator_request_packet_belongs_to_row(packet, queue_item))
        .filter(|packet| !row_is_offered_option_of(packet, queue_item))
        .filter(|packet| rendered_choice_packet_ids.insert(packet.id.clone()))
    {
        child.push_str("\n- **Choices:**");
        child.push_str(&format!("\n  - **Request**: [#{}]", packet.id));
        for choice in packet.choice_rows {
            child.push_str("\n  - ");
            child.push_str(&choice);
        }
    }
    child
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

struct ResponseLookupContext<'a> {
    source_document: &'a str,
    queue_item: &'a str,
    response_body: &'a str,
    response_id: &'a str,
    addressed_at: &'a str,
    shared_batch: bool,
    /// Closability of the queue tree around the closing row, read from the
    /// queue as it stands after this response struck its rows.
    closure: Option<crate::queue_tree::RowClosure>,
}

fn response_lookup_children(
    context: ResponseLookupContext<'_>,
    mode: QueueResponseMode,
    rendered_choice_packet_ids: &mut HashSet<String>,
) -> Vec<String> {
    // `patch:exchange` is the response transport envelope. The exchange
    // materializer consumes it, but this queue projection receives the original
    // captured payload so it must cross the same visibility boundary explicitly.
    // Strip only one complete outer pair; literal examples inside an ordinary
    // response remain operator-visible.
    let response_body = response_body_without_exchange_transport(context.response_body);
    let mut children = Vec::new();
    match mode {
        QueueResponseMode::Summary => children.push(
            response_summary_row_child(&context, response_body, rendered_choice_packet_ids)
                .expect("response lookup ids are canonical response ids"),
        ),
        QueueResponseMode::Inline => {
            let marker = agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(
                context.response_id,
            )
            .expect("response lookup ids are canonical response ids");
            let details = response_details_child(response_body, context.response_id)
                .expect("response lookup ids are canonical response ids");
            children.push(format!(
                "{marker}\n- **Timestamp:** {}\n{}",
                context.addressed_at.trim(),
                render_nested_response_content(&details)
            ));
        }
        QueueResponseMode::Lookup => {
            let marker = agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(
                context.response_id,
            )
            .expect("response lookup ids are canonical response ids");
            children.push(format!(
                "{marker}\n- **Timestamp:** {}",
                context.addressed_at.trim()
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
/// `**State:**` values of a struck row's response summary: the closability
/// the Guidance row spells out. `#treestatusquery` reads them back to decide
/// whether a struck subtree is deletable.
pub const RESPONSE_STATE_UNPUBLISHED: &str = "Unpublished local work remains.";
pub const RESPONSE_STATE_WAITING_ON_OPERATOR: &str = "Waiting on operator input.";
pub const RESPONSE_STATE_FOLLOW_UP: &str = "Follow-up work remains.";
pub const RESPONSE_STATE_COMPLETE: &str = "Complete.";
/// The `**State:**` label of a response summary row.
pub const RESPONSE_STATE_LABEL: &str = "**State:**";

const ACTIONABLE_RESPONSE_LEAD_LABELS: &[&str] = &[
    "**outcome:**",
    "**current state:**",
    "**blocked:**",
    "**blocker:**",
    "**next action:**",
];

fn response_summary_row_child(
    context: &ResponseLookupContext<'_>,
    response_body: &str,
    rendered_choice_packet_ids: &mut HashSet<String>,
) -> Option<String> {
    let ResponseLookupContext {
        source_document,
        queue_item,
        response_id,
        addressed_at,
        shared_batch,
        ref closure,
        ..
    } = *context;
    if !agent_doc_markdown_ast::exchange_tree::is_compact_response_id(response_id) {
        return None;
    }
    // `#responsesummarypick`: one response commonly answers a chat prompt and
    // closes queue rows in the same turn. The row's summary is the paragraph or
    // bullet that names this row, never the response's first paragraph when
    // that paragraph is about something else.
    let item_scoped_summary = response_summary_for_queue_item(response_body, queue_item);
    let mut summary = match item_scoped_summary {
        Some(summary) => summary,
        None if shared_batch => "This item was completed as part of a shared batch; no unrelated batch prose was copied into this row."
            .to_string(),
        None => response_summary_lead(response_body),
    };
    let next_actions = if shared_batch {
        Vec::new()
    } else {
        response_next_actions(response_body)
    };
    // Even when this response closed a batch, operator requests remain scoped
    // to their own live origin rows. Batch prose is intentionally replaced by
    // the generic per-row summary above, but suppressing the matching request
    // would hide the only active operator decision.
    let packets = crate::operator_request_packet::blocking_operator_request_packets(response_body)
        .into_iter()
        .filter(|packet| operator_request_packet_belongs_to_row(packet, queue_item))
        .filter(|packet| !row_is_offered_option_of(packet, queue_item))
        .collect::<Vec<_>>();
    let has_blocking_request = !packets.is_empty()
        || response_body
            .lines()
            .any(|line| line.trim() == "**Operator input needed**");
    let mut choice_projections = Vec::new();
    for packet in packets {
        // A request is one live operator decision. If duplicate queue
        // occurrences match its origin, only the first owner receives the
        // inert choice tree; later rows receive no queue-shaped reference.
        if !rendered_choice_packet_ids.insert(packet.id.clone()) {
            continue;
        }
        if !summary.contains(&packet.request) {
            if !summary.is_empty() {
                summary.push(' ');
            }
            summary.push_str(&packet.request);
        }
        choice_projections.push((packet.id, packet.choice_rows));
    }
    let decision_projection =
        structured_choice_projection(source_document, queue_item, response_body, shared_batch);
    let decision_choices = decision_projection
        .as_ref()
        .map(|projection| projection.choices.clone());
    let decision_request_id = decision_projection
        .as_ref()
        .and_then(|projection| projection.request_id.clone());
    let marker = agent_doc_markdown_ast::exchange_tree::response_id_lookup_marker(response_id)?;
    let has_local_report_state = agent_doc_element_queue::queue_report_state_annotation(queue_item)
        .is_some_and(|(_, state)| {
            matches!(
                state,
                agent_doc_element_queue::QueueReportState::Local
                    | agent_doc_element_queue::QueueReportState::PublishedLocal
            )
        });
    let has_unpublished_response_evidence =
        !shared_batch && response_has_unpublished_work_evidence(response_body);
    let has_unpublished_work = has_local_report_state || has_unpublished_response_evidence;
    let metadata_value = |label: &str| {
        response_body.lines().find_map(|line| {
            let line = line.trim().strip_prefix("- ").unwrap_or(line.trim());
            line.strip_prefix(label)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    };
    let mut child = format!(
        "{marker}\n- **Timestamp:** {}\n- **Summary:** {summary}",
        addressed_at.trim()
    );
    child.push_str("\n- **State:** ");
    child.push_str(if has_unpublished_work {
        RESPONSE_STATE_UNPUBLISHED
    } else if has_blocking_request {
        RESPONSE_STATE_WAITING_ON_OPERATOR
    } else if !next_actions.is_empty() {
        RESPONSE_STATE_FOLLOW_UP
    } else {
        RESPONSE_STATE_COMPLETE
    });
    child.push_str("\n- **Guidance:** ");
    if has_unpublished_work {
        child.push_str(
            "More work remains. Keep this tree until commit/install/publication receipts resolve the local work, or create a tracked follow-up carrying its worktree, branch, and required next action.",
        );
        child.push_str("\n- **Worktree:** ");
        child.push_str(
            &metadata_value("**Worktree:**")
                .unwrap_or_else(|| "not recorded; add it to the tracked follow-up".to_string()),
        );
        child.push_str("\n- **Branch:** ");
        child.push_str(
            &metadata_value("**Branch:**")
                .unwrap_or_else(|| "not recorded; add it to the tracked follow-up".to_string()),
        );
        child.push_str("\n- **Integration:** ");
        child.push_str(&metadata_value("**Integration:**").unwrap_or_else(|| {
            "pending; retain a tracked action until a PR or named integration-branch receipt exists"
                .to_string()
        }));
    } else if has_blocking_request {
        child.push_str("More work remains. Keep this tree and answer the operator request below.");
    } else if !next_actions.is_empty() {
        child.push_str("More work remains. Keep this tree and follow the next actions below.");
    } else {
        child.push_str(&completed_row_deletion_guidance(closure.as_ref()));
    }
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
        for (index, (packet_id, packet_choices)) in choice_projections.into_iter().enumerate() {
            child.push_str("\n- **Choices:**");
            child.push_str(&format!("\n  - **Request**: [#{packet_id}]"));
            // `#waitingtriage`: a decision's options stand in for a packet's
            // only when that decision records this very request (or names no
            // request, which only a single-row response keeps); another
            // decision's options never land under this request's row.
            let decision_is_this_request = decision_request_id
                .as_deref()
                .is_none_or(|id| id.eq_ignore_ascii_case(&packet_id));
            let choices = match &decision_choices {
                Some(choices) if index == 0 && decision_is_this_request => choices.clone(),
                _ => packet_choices,
            };
            for choice in choices {
                child.push_str("\n  - ");
                child.push_str(&choice);
            }
        }
    }
    if !next_actions.is_empty() {
        child.push_str("\n- **Next actions:**");
        for projection in next_actions {
            let action_indent = if let Some(decision) = projection.decision {
                child.push_str("\n  - **Decision:** ");
                child.push_str(&decision);
                "\n    - **Action:** "
            } else {
                "\n  - **Action:** "
            };
            for action in projection.actions {
                child.push_str(action_indent);
                child.push_str(&action);
            }
        }
    }
    Some(child)
}

/// Deletion guidance for a completed row with no tracked or unpublished work
/// (`#guidancescope`). The row's own response decides that the row is done;
/// the queue tree decides how much of it may go: the row alone, the row with
/// its nested rows, or the whole tree under its top-level row. When an open
/// row keeps the subtree or the tree alive, the guidance names it.
fn completed_row_deletion_guidance(closure: Option<&crate::queue_tree::RowClosure>) -> String {
    const DONE: &str = "the item is complete and has no remaining tracked or unpublished work.";
    let Some(closure) = closure else {
        return format!(
            "This row is complete; {DONE} Its nested rows and tree were not checked, so confirm they are struck before deleting it."
        );
    };
    let nested = match closure.nested_rows {
        1 => "1 nested row".to_string(),
        count => format!("{count} nested rows"),
    };
    if let Some(open) = &closure.first_open_nested {
        return format!(
            "More work remains beneath this row. This item is complete, but keep this row and its {nested}: {} is still open.",
            open.label()
        );
    }
    let row_scope = if closure.nested_rows == 0 {
        "Safe to delete this completed row".to_string()
    } else {
        format!("Safe to delete this row and its {nested}")
    };
    match &closure.top_level {
        None => format!("{row_scope}; {DONE}"),
        Some(top) => match &closure.first_open_in_tree {
            None => format!(
                "Safe to delete the whole tree under {}; every row in it is struck and {DONE}",
                top.label()
            ),
            Some(open) => format!(
                "{row_scope}; {DONE} Keep the tree under {}: {} is still open.",
                top.label(),
                open.label()
            ),
        },
    }
}

/// Whether `queue_item` is itself one of `packet`'s offered options (an
/// option an older binary dispatched as its own row). Its packet's Choices are
/// never re-copied beneath it (`#waitingpacketowner`).
fn row_is_offered_option_of(
    packet: &crate::operator_request_packet::BlockingOperatorRequestPacket,
    queue_item: &str,
) -> bool {
    let row_work = agent_doc_element_queue::strip_attempt_projection_prefix(queue_item).trim();
    packet.choice_rows.iter().any(|choice| {
        choice
            .split_once("**: ")
            .is_some_and(|(label, _)| row_work.starts_with(&format!("{label}**: ")))
    })
}

fn operator_request_packet_belongs_to_row(
    packet: &crate::operator_request_packet::BlockingOperatorRequestPacket,
    queue_item: &str,
) -> bool {
    agent_doc_document::queue_projection::operator_action_ids(queue_item)
        .iter()
        .any(|id| id.eq_ignore_ascii_case(&packet.id))
        || packet.queue_item.as_deref().is_some_and(|origin| {
            agent_doc_document::queue_projection::operator_request_item_matches_row(
                queue_item, origin,
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredChoiceProjection {
    /// The request the decision records, when it names one.
    request_id: Option<String>,
    context: Option<String>,
    question: Option<String>,
    selected: String,
    choices: Vec<String>,
}

fn structured_choice_projection(
    source_document: &str,
    queue_item: &str,
    response_body: &str,
    shared_batch: bool,
) -> Option<StructuredChoiceProjection> {
    // `#waitingpacketowner`: a response may record decisions for several
    // items. A request-scoped decision belongs only to the row that carries
    // (or originated) that request; taking the response's first decision for
    // every row it closed grafted one item's choices onto its siblings.
    let response_packets =
        crate::operator_request_packet::blocking_operator_request_packets(response_body)
            .into_iter()
            .chain(
                crate::operator_request_packet::blocking_operator_request_packets(source_document),
            )
            .collect::<Vec<_>>();
    let decision_belongs = |request_id: &str| {
        queue_item.contains(&format!("[#{request_id}]"))
            || agent_doc_document::queue_projection::operator_action_ids(queue_item)
                .iter()
                .any(|id| id.eq_ignore_ascii_case(request_id))
            || response_packets.iter().any(|packet| {
                packet.id.eq_ignore_ascii_case(request_id)
                    && operator_request_packet_belongs_to_row(packet, queue_item)
            })
    };
    let request_less_decisions = crate::operator_request_packet::structured_choice_decisions(
        queue_prompt_section_for_row(response_body, queue_item),
    );
    let decision = crate::operator_request_packet::structured_choice_decisions(response_body)
        .into_iter()
        // `#waitingtriage`: a decision that names no request belongs only to
        // the row whose `> **Queue prompt:**` section records it, and never
        // to every row of a shared batch; taking it for each closed row put
        // one item's options under its siblings.
        .find(|decision| match decision.request_id.as_deref() {
            Some(request_id) => decision_belongs(request_id),
            None => !shared_batch && request_less_decisions.contains(decision),
        })
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
        request_id: decision.request_id.clone(),
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

/// The part of `response_body` that answers `queue_item` (`#waitingtriage`).
///
/// A response that echoes two or more rows as `> **Queue prompt:** …`
/// sections answers each row in its own section: the section whose echo
/// names the row (its stable `#id`, or its whole work text), up to the next
/// echo. A response with at most one echo answers its row as a whole. When
/// no echo names the row, nothing in the response is scoped to it.
fn queue_prompt_section_for_row<'a>(response_body: &'a str, queue_item: &str) -> &'a str {
    const ECHO: &str = "> **Queue prompt:**";
    let starts = response_body
        .match_indices('\n')
        .map(|(offset, _)| offset + 1)
        .chain(std::iter::once(0))
        .filter(|offset| response_body[*offset..].starts_with(ECHO))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if starts.len() < 2 {
        return response_body;
    }
    let work = struck_work_text(queue_item);
    let work = work
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .unwrap_or(work);
    let id = agent_doc_element_queue::queue_prompt_stable_id(work)
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| !id.is_empty());
    let (base, _) = document_queue::split_queue_effect_annotation(work);
    let text = base.split_whitespace().collect::<Vec<_>>().join(" ");
    for (position, start) in starts.iter().enumerate() {
        let end = starts
            .get(position + 1)
            .copied()
            .unwrap_or(response_body.len());
        let section = &response_body[*start..end];
        let echo = section.lines().next().unwrap_or_default()[ECHO.len()..].trim();
        let echo_text = echo.split_whitespace().collect::<Vec<_>>().join(" ");
        let names_row = match id.as_deref() {
            Some(id) => echo.to_ascii_lowercase().contains(&format!("#{id}]")),
            None => !text.is_empty() && echo_text == text,
        };
        if names_row {
            return section;
        }
    }
    ""
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

fn response_has_unpublished_work_evidence(response_body: &str) -> bool {
    let normalized = response_body.to_ascii_lowercase();
    [
        "remains uncommitted",
        "still uncommitted",
        "is uncommitted",
        "uncommitted and uninstalled",
        "remains uninstalled",
        "still uninstalled",
        "is uninstalled",
        "not committed",
        "not installed",
        "remains local",
    ]
    .into_iter()
    .any(|needle| normalized.contains(needle))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResponseNextActionProjection {
    decision: Option<String>,
    actions: Vec<String>,
}

fn response_next_actions(response_body: &str) -> Vec<ResponseNextActionProjection> {
    let lines = response_body.lines().collect::<Vec<_>>();
    let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");

    // Prose responses commonly present each real decision as a Markdown
    // subsection with `- Decision:` and `- Next action:` fields. Prefer those
    // row-scoped fields over a generic lead so the active queue tree preserves
    // every actionable unit without requiring the operator to read exchange.
    let mut detailed = Vec::<ResponseNextActionProjection>::new();
    let mut current_decision = None::<usize>;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            current_decision = None;
            continue;
        }
        let Some(bullet) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let decision = bullet
            .strip_prefix("Decision:")
            .or_else(|| bullet.strip_prefix("**Decision:**"));
        if let Some(decision) = decision {
            let decision = normalize(decision);
            if decision.is_empty() {
                current_decision = None;
            } else {
                detailed.push(ResponseNextActionProjection {
                    decision: Some(decision),
                    actions: Vec::new(),
                });
                current_decision = Some(detailed.len() - 1);
            }
            continue;
        }
        let action = bullet
            .strip_prefix("Next action:")
            .or_else(|| bullet.strip_prefix("**Next action:**"));
        if let Some(action) = action {
            let action = normalize(action);
            if action.is_empty() {
                continue;
            }
            if let Some(index) = current_decision {
                if !detailed[index].actions.contains(&action) {
                    detailed[index].actions.push(action);
                }
            } else if !detailed.iter().any(|candidate| {
                candidate.decision.is_none() && candidate.actions.contains(&action)
            }) {
                detailed.push(ResponseNextActionProjection {
                    decision: None,
                    actions: vec![action],
                });
            }
        }
    }
    if !detailed.is_empty() {
        return detailed;
    }

    let mut actions = Vec::<ResponseNextActionProjection>::new();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(action) = trimmed.strip_prefix("**Next action:**") {
            let action = normalize(action);
            if !action.is_empty()
                && !actions
                    .iter()
                    .any(|candidate| candidate.actions.contains(&action))
            {
                actions.push(ResponseNextActionProjection {
                    decision: None,
                    actions: vec![action],
                });
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
                let action = normalize(action);
                if !action.is_empty()
                    && !actions
                        .iter()
                        .any(|candidate| candidate.actions.contains(&action))
                {
                    actions.push(ResponseNextActionProjection {
                        decision: None,
                        actions: vec![action],
                    });
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

/// Minimum normalized length before a free-text row's work text is used as a
/// quote needle. Shorter rows (`fix it`, `retry`) occur incidentally in prose.
const RESPONSE_SUMMARY_MIN_QUOTED_ROW_CHARS: usize = 16;

/// The paragraph or list item of `response_body` that names `queue_item`.
///
/// `#responsesummarypick`: a closeout response may answer a chat prompt and
/// close several queue rows in one turn. Summarizing its first paragraph gave
/// a struck row the answer to a different prompt. A row is named by its
/// durable id (`#id`, `[#id]`, `` `do [#id]` ``) or, for a free-text row
/// without one, by a verbatim quote of its work text. Headings, blockquotes
/// (including the `> **Queue prompt:**` echo, which names the row without
/// describing its outcome), fenced code, HTML comments, and operator request
/// packets never qualify. The first naming unit wins; `None` means nothing in
/// the response names this row, and the caller keeps its existing fallback.
fn response_summary_for_queue_item(response_body: &str, queue_item: &str) -> Option<String> {
    let work = struck_work_text(queue_item);
    let work = work
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .unwrap_or(work);
    let id = agent_doc_element_queue::queue_prompt_stable_id(work)
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| !id.is_empty());
    let quoted_row = id
        .is_none()
        .then(|| {
            let (base, _) = document_queue::split_queue_effect_annotation(work);
            base.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|row| row.chars().count() >= RESPONSE_SUMMARY_MIN_QUOTED_ROW_CHARS)
        .map(|row| row.to_lowercase());
    if id.is_none() && quoted_row.is_none() {
        return None;
    }
    let names_item = |unit: &str| {
        let lower = unit.to_lowercase();
        if let Some(id) = id.as_deref() {
            return lower.match_indices('#').any(|(offset, _)| {
                let tail = &lower[offset + 1..];
                tail.strip_prefix(id).is_some_and(|after| {
                    !after.chars().next().is_some_and(|next| {
                        next.is_ascii_alphanumeric() || matches!(next, '-' | '_')
                    })
                })
            });
        }
        quoted_row
            .as_deref()
            .is_some_and(|quoted| lower.contains(quoted))
    };

    let mut prose = response_body.to_string();
    for packet in crate::operator_request_packet::blocking_operator_request_blocks(response_body) {
        prose = prose.replacen(packet, "", 1);
    }
    response_summary_units(&prose)
        .into_iter()
        .find(|unit| names_item(unit))
        .map(|unit| soft_bound_response_summary(&unit, actionable_response_lead(&unit)))
}

/// Paragraphs and list items of `prose`, whitespace-normalized, in document
/// order. A list item keeps its continuation lines and drops its marker.
fn response_summary_units(prose: &str) -> Vec<String> {
    fn list_item_body(line: &str) -> Option<&str> {
        for marker in ["- ", "* ", "+ "] {
            if let Some(body) = line.strip_prefix(marker) {
                return Some(body);
            }
        }
        let (number, body) = line.split_once(". ")?;
        (!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
            .then_some(body)
    }

    let mut units = Vec::new();
    let mut current = Vec::<&str>::new();
    let flush = |current: &mut Vec<&str>, units: &mut Vec<String>| {
        if !current.is_empty() {
            let unit = current
                .iter()
                .flat_map(|line| line.split_whitespace())
                .collect::<Vec<_>>()
                .join(" ");
            if !unit.is_empty() {
                units.push(unit);
            }
            current.clear();
        }
    };
    let mut fence: Option<&str> = None;
    for line in prose.lines() {
        let trimmed = line.trim();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = ["```", "~~~"]
            .into_iter()
            .find(|open| trimmed.starts_with(open))
        {
            flush(&mut current, &mut units);
            fence = Some(open);
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut current, &mut units);
            continue;
        }
        if let Some(body) = list_item_body(trimmed) {
            flush(&mut current, &mut units);
            current.push(body);
            continue;
        }
        if response_summary_structural_line(trimmed) {
            flush(&mut current, &mut units);
            continue;
        }
        current.push(trimmed);
    }
    flush(&mut current, &mut units);
    units
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

fn normalize_response_projection_volatile_metadata(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let (core, newline) = line
            .strip_suffix("\r\n")
            .map(|core| (core, "\r\n"))
            .or_else(|| line.strip_suffix('\n').map(|core| (core, "\n")))
            .unwrap_or((line, ""));
        let trimmed = core.trim_start_matches([' ', '\t']);
        if trimmed.starts_with("- **Timestamp:** ") {
            normalized.push_str(&core[..core.len() - trimmed.len()]);
            normalized.push_str("- **Timestamp:** <response-timestamp>");
            normalized.push_str(newline);
        } else if trimmed.starts_with("- **Guidance:** ") {
            // `#guidancescope`: deletion guidance reads the surrounding queue
            // tree, which a sibling row can change between a projection and
            // its replay. The guidance is advice about that tree, not part of
            // the response's identity, so a replay never stacks a second copy.
            normalized.push_str(&core[..core.len() - trimmed.len()]);
            normalized.push_str("- **Guidance:** <tree-scoped-guidance>");
            normalized.push_str(newline);
        } else {
            normalized.push_str(line);
        }
    }
    normalized
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

fn remove_inline_response_id(line: &str, response_id: &str) -> String {
    let semicolon_note = format!("; {RESPONSE_ID_NOTE_PREFIX}{response_id}");
    if line.contains(&semicolon_note) {
        return line.replacen(&semicolon_note, "", 1);
    }

    let direct_note = format!(
        "{}{RESPONSE_ID_NOTE_PREFIX}{response_id}",
        agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
    );
    if line.contains(&direct_note) {
        return line.replacen(&direct_note, "", 1);
    }

    line.to_string()
}

/// Project unresolved operator-owned todo work into one inert queue tree.
///
/// This is a pure closeout projection. `agent:todo` remains the durable source
/// of truth; this function only maintains its queue-facing view. Generated
/// response rows remain readable, unstruck evidence tagged with the binary
/// metadata marker. Queue parsing treats the entire waiting subtree as one
/// graph projection: metadata and operator answer edges are never independent
/// work; an answer selects the durable waiting item parent.
///
/// `#waitingdonetodoopen`: a response closeout (non-empty `response_body`)
/// also checks off the Todo of every waiting item whose tree is complete
/// (every row closed, no open ask, choice, attempt, or unpicked reply) and
/// archives the item, instead of leaving a Next row that asks the operator to
/// tick the box. A declarative sync (empty `response_body`) never changes
/// `agent:todo`; it shows such an item as one open ask (`you: 1`).
pub fn project_waiting_on_you_tree(document: &str, response_body: &str) -> Result<Option<String>> {
    let first = project_waiting_on_you_tree_pass(document, response_body, &HashSet::new())?;
    if response_body.trim().is_empty() {
        return Ok(first);
    }
    let projected = first.as_deref().unwrap_or(document);
    let (checked, completed) = check_off_complete_waiting_todos(projected)?;
    if completed.is_empty() {
        return Ok(first);
    }
    let rerun = project_waiting_on_you_tree_pass(&checked, response_body, &completed)?;
    let out = rerun.unwrap_or(checked);
    Ok((out != document).then_some(out))
}

fn project_waiting_on_you_tree_pass(
    document: &str,
    response_body: &str,
    completed: &HashSet<String>,
) -> Result<Option<String>> {
    let adopted = adopt_legacy_waiting_items(document)?;
    let base = adopted.as_deref().unwrap_or(document);
    let projected = project_waiting_items_from_todos(base, response_body, completed)?;
    let projected = projected.as_deref().unwrap_or(base);
    let normalized = normalize_waiting_queues(projected)?;
    let out = normalized.as_deref().unwrap_or(projected);
    Ok((out != document).then(|| out.to_string()))
}

/// `#waitingdonetodoopen`: check off, in `agent:todo`, the open Todo of every
/// projected waiting item whose tree is complete. Returns the updated
/// document and the ids it checked off. A Todo that cannot be resolved
/// unambiguously is left open; its item keeps asking once.
fn check_off_complete_waiting_todos(document: &str) -> Result<(String, HashSet<String>)> {
    let todos = open_waiting_todos(document);
    let by_lower = todos
        .iter()
        .map(|todo| (todo.id.to_ascii_lowercase(), todo.id.clone()))
        .collect::<HashMap<_, _>>();
    let open_todo_ids = by_lower.keys().cloned().collect::<HashSet<_>>();
    let mut complete = Vec::new();
    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(document);
        if !body.contains(WAITING_ON_YOU_ITEM_MARKER) {
            continue;
        }
        let entries = document_queue::parse(body)?;
        complete.extend(crate::waiting_status::complete_waiting_todo_ids(
            &entries,
            &open_todo_ids,
        ));
    }
    let mut out = document.to_string();
    let mut completed = HashSet::new();
    for id in complete {
        let Some(todo_id) = by_lower.get(&id) else {
            continue;
        };
        if completed.contains(todo_id) || !agent_doc_element::todo::todo_owns_id(&out, todo_id) {
            continue;
        }
        let Ok(outcome) = agent_doc_element::todo::set_todo_item_state_in_document(
            &out,
            todo_id,
            agent_doc_element::todo::TodoTransition::Done,
        ) else {
            continue;
        };
        if outcome.changed {
            out = outcome.content;
            completed.insert(todo_id.clone());
        }
    }
    Ok((out, completed))
}

/// `#waitingpacketowner`: adopt the items of a legacy, unmarked Waiting-on-you
/// root as waiting items.
///
/// An older document can hold a `Waiting on you` heading that lost its hidden
/// root marker, with Todo-id children that lost their item marker. Their
/// packets were then ordinary queue leaves: offered Choices were dispatched
/// one worker each and operator replies were claimed on their own. Marking the
/// child row (only the hidden sentinel is appended; no visible byte changes)
/// restores packet ownership in place. Nothing is moved or regenerated.
fn adopt_legacy_waiting_items(document: &str) -> Result<Option<String>> {
    let open_ids = open_waiting_todos(document)
        .into_iter()
        .map(|todo| todo.id)
        .collect::<HashSet<_>>();
    if open_ids.is_empty() {
        return Ok(None);
    }
    let mut replacements = Vec::new();
    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(document);
        if !body.contains(WAITING_ON_YOU_ROOT_TEXT) {
            continue;
        }
        let mut entries = document_queue::parse(body)?;
        let mut changed = false;
        for root in 0..entries.len() {
            let QueueEntry::Prompt(root_prompt) = &entries[root] else {
                continue;
            };
            if agent_doc_element_queue::struck_work_text(&root_prompt.text).trim()
                != WAITING_ON_YOU_ROOT_TEXT
            {
                continue;
            }
            let child_indent = root_prompt.indent + 2;
            let end = crate::waiting_packet::subtree_end(&entries, root);
            for entry in entries.iter_mut().take(end).skip(root + 1) {
                let QueueEntry::Prompt(prompt) = entry else {
                    continue;
                };
                if prompt.indent != child_indent || prompt.text.contains(WAITING_ON_YOU_ITEM_MARKER)
                {
                    continue;
                }
                let work = agent_doc_element_queue::strip_attempt_projection_prefix(&prompt.text);
                let Some(id) = work
                    .strip_prefix("[#")
                    .and_then(|rest| rest.split_once(']'))
                    .map(|(id, _)| id.to_ascii_lowercase())
                else {
                    continue;
                };
                if !open_ids.contains(&id) {
                    continue;
                }
                prompt.text = with_waiting_item_marker(&prompt.text);
                changed = true;
            }
        }
        if changed {
            replacements.push((
                component.open_end,
                component.close_start,
                document_queue::render(&entries),
            ));
        }
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    let mut out = document.to_string();
    for (start, end, body) in replacements.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    Ok(Some(out))
}

/// Append the hidden waiting-item sentinel to a head's first line, before any
/// trailing binary `⟦effect:N⟧` annotation so effect identity stays trailing.
fn with_waiting_item_marker(text: &str) -> String {
    let (head, tail) = text
        .split_once('\n')
        .map_or((text, None), |(head, tail)| (head, Some(tail)));
    let trimmed = head.trim_end();
    let marked = match trimmed.rsplit_once(" ⟦effect:") {
        Some((work, effect))
            if effect
                .strip_suffix('⟧')
                .is_some_and(|raw| raw.parse::<usize>().is_ok()) =>
        {
            format!("{work} {WAITING_ON_YOU_ITEM_MARKER} ⟦effect:{effect}")
        }
        _ => format!("{trimmed} {WAITING_ON_YOU_ITEM_MARKER}"),
    };
    match tail {
        Some(tail) => format!("{marked}\n{tail}"),
        None => marked,
    }
}

/// Closeout normalization of every waiting item in every queue
/// (`#waitingpacketowner`).
fn normalize_waiting_queues(document: &str) -> Result<Option<String>> {
    if !document.contains(WAITING_ON_YOU_ITEM_MARKER) {
        return Ok(None);
    }
    let open_todo_ids = open_waiting_todos(document)
        .into_iter()
        .map(|todo| todo.id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut replacements = Vec::new();
    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(document);
        if !body.contains(WAITING_ON_YOU_ITEM_MARKER) {
            continue;
        }
        // `#closedbygraft`: drop verbatim Choices re-copies older binaries
        // grafted into Closed-by back-references.
        let degrafted = crate::waiting_packet::strip_closed_by_choice_grafts(body, &open_todo_ids);
        let body = degrafted.as_deref().unwrap_or(body);
        let parsed = document_queue::parse(body)?;
        let normalized = crate::waiting_packet::normalize_waiting_packets(&parsed);
        let entries = normalized.as_deref().unwrap_or(&parsed);
        // `#waitingitemstatus`: every live head carries its derived state and
        // roll-up, recomputed from the normalized packet.
        // `#waitingtriage`: the document's newest response time is "now", so
        // the stale-debt flag is a deterministic function of the document.
        let as_of = latest_response_timestamp(document)
            .and_then(crate::waiting_subtree::parse_timestamp_minutes)
            .map(|(minutes, _)| minutes);
        let with_status =
            crate::waiting_status::project_waiting_status(entries, &open_todo_ids, as_of);
        if degrafted.is_some() || normalized.is_some() || with_status.is_some() {
            let rendered = with_status.as_deref().unwrap_or(entries);
            replacements.push((
                component.open_end,
                component.close_start,
                document_queue::render(rendered),
            ));
        }
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    let mut out = document.to_string();
    for (start, end, body) in replacements.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    Ok(Some(out))
}

fn project_waiting_items_from_todos(
    document: &str,
    response_body: &str,
    completed: &HashSet<String>,
) -> Result<Option<String>> {
    let packets = waiting_operator_packets(document, response_body);
    let todos = open_waiting_todos(document);
    // An operator may answer a request directly beneath its original queue
    // parent before the binary-owned Waiting-on-you mirror is materialized.
    // That origin row already suppresses the duplicate mirror, so adopt it as
    // the durable waiting item and graft the same inert response structure
    // onto it. Otherwise its answer child is mistaken for unrelated queue work
    // and dispatch loses the request/decision boundary entirely.
    let adopted = project_external_waiting_items(document, &todos, &packets)?;
    let projected_document = adopted.as_deref().unwrap_or(document);
    let location = waiting_tree_location(projected_document)?;
    let external_ids =
        queue_prompt_ids_outside_waiting_tree(projected_document, location.as_ref())?;
    let existing = location
        .as_ref()
        .map(|location| existing_waiting_children(projected_document, location))
        .transpose()?
        .unwrap_or_default();
    let response_timestamp = latest_response_timestamp(response_body)
        .or_else(|| latest_response_timestamp(document))
        .unwrap_or(FALLBACK_RESPONSE_TIMESTAMP);
    // `#waitingarchive`: a child that carries an open Todo's id and operator
    // text but lost its hidden item marker (or sits beside a binary-only
    // duplicate) represents that item. Without this it was invisible to
    // `existing`, a stub was regenerated beside it, and the two swapped
    // places on every rebuild. Adopting it appends only the hidden sentinel.
    let adoptable = location
        .as_ref()
        .map(|location| adoptable_waiting_segments(projected_document, location))
        .unwrap_or_default();
    let existing_has_reply = |id: &str| {
        existing
            .get(id)
            .is_some_and(|(_, has_operator_reply)| *has_operator_reply)
    };

    let open_ids = todos
        .iter()
        .map(|todo| todo.id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut kept_raws = HashSet::<String>::new();
    let mut children = Vec::new();
    let mut child_ids = Vec::new();
    for todo in todos {
        if external_ids.contains(&todo.id) {
            continue;
        }
        child_ids.push(todo.id.to_ascii_lowercase());
        let timestamp = packets
            .get(&todo.id)
            .and_then(|packet| packet.response_timestamp.as_deref())
            .unwrap_or(response_timestamp);
        // `#waitingoperatortext`: keep an existing child that holds operator
        // text, and never downgrade an existing packet to the generic stub
        // when no fresher operator packet is available to regenerate from.
        if !existing_has_reply(&todo.id)
            && let Some((source, marked)) = adoptable.get(&todo.id.to_ascii_lowercase())
        {
            kept_raws.insert(source.clone());
            let child = normalize_agent_doc_reply_rows(marked)?;
            children.push(ensure_waiting_response_timestamp(&child, timestamp));
            continue;
        }
        if let Some((raw, has_operator_reply)) = existing.get(&todo.id)
            && (*has_operator_reply || !packets.contains_key(&todo.id))
        {
            kept_raws.insert(raw.clone());
            let child = normalize_agent_doc_reply_rows(raw)?;
            children.push(ensure_waiting_response_timestamp(&child, timestamp));
            continue;
        }
        children.push(render_waiting_child(
            &todo,
            packets.get(&todo.id),
            timestamp,
        ));
    }

    let Some(location) = location else {
        return Ok(None);
    };

    // `#waitingarchive`: the regenerated tree keeps only the children of open
    // Todos. Every other child segment used to vanish with the rebuild,
    // together with any operator text under it. Now a segment of a closed
    // Todo moves byte-for-byte into `agent:done` under a one-line receipt,
    // and a segment that matches no rendered item (no Todo id, a duplicate,
    // a struck head of an open Todo) but holds operator text stays in the
    // tree after the item it followed. Only binary-only segments of still
    // open Todos, which the rebuild regenerates, are dropped.
    let dispositions = waiting_segment_dispositions(
        projected_document,
        &location,
        &kept_raws,
        &open_ids,
        completed,
    );
    let mut archived = Vec::new();
    let mut retained = Vec::new();
    for (segment, disposition) in dispositions {
        match disposition {
            SegmentDisposition::Archive(reason) => archived.push((segment, reason)),
            SegmentDisposition::Retain { after } => retained.push((segment, after)),
        }
    }
    let archive_entries = archived
        .iter()
        .map(|(segment, reason)| {
            crate::waiting_archive::render_archive_entry(
                &crate::waiting_archive::archive_date(response_timestamp),
                &location.queue,
                segment,
                *reason,
            )
        })
        .collect::<Vec<_>>();
    let archive_target = if archive_entries.is_empty() {
        None
    } else {
        // Probe whether the document can hold the entries; a document with no
        // managed component keeps the segments in its tree instead.
        crate::waiting_archive::append_to_done(projected_document, &archive_entries)?
    };
    if !archive_entries.is_empty() && archive_target.is_none() {
        for (segment, _) in archived {
            retained.push((segment, None));
        }
    }
    let children = interleave_retained_segments(children, &child_ids, retained);

    let replacement = if children.is_empty() {
        String::new()
    } else {
        let mut tree = format!("- ### {WAITING_ON_YOU_ROOT_TEXT} {WAITING_ON_YOU_ROOT_MARKER}\n");
        for child in children {
            tree.push_str(&child);
        }
        tree
    };
    let mut out = projected_document.to_string();
    if location.start == location.end && !replacement.is_empty() {
        let separator = if location.start > 0 && !out[..location.start].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        out.insert_str(location.start, &format!("{separator}{replacement}"));
    } else {
        out.replace_range(location.start..location.end, &replacement);
    }
    if archive_target.is_some() {
        out = crate::waiting_archive::append_to_done(&out, &archive_entries)?
            .ok_or_else(|| anyhow::anyhow!("agent:done vanished while archiving waiting items"))?;
    }
    Ok((out != document).then_some(out))
}

/// Live (unstruck) child segments with an id and operator text, keyed by the
/// lowercased id (first such segment wins), each with its text carrying the
/// hidden item marker.
fn adoptable_waiting_segments(
    document: &str,
    location: &WaitingTreeLocation,
) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    if location.start == location.end {
        return out;
    }
    let tree = &document[location.start..location.end];
    let root_line = tree.lines().next().unwrap_or_default();
    let root_indent = root_line.len() - root_line.trim_start_matches([' ', '\t']).len();
    for segment in crate::waiting_archive::waiting_tree_segments(tree, root_indent) {
        let Some(id) = segment.id.clone() else {
            continue;
        };
        if segment.struck || !crate::waiting_archive::segment_has_operator_rows(&segment.raw) {
            continue;
        }
        let head = segment.raw.lines().next().unwrap_or_default();
        let marked = if head.contains(WAITING_ON_YOU_ITEM_MARKER) {
            segment.raw.clone()
        } else {
            with_waiting_item_marker(&segment.raw)
        };
        out.entry(id).or_insert((segment.raw, marked));
    }
    out
}

/// What a rebuild does with one existing child segment it does not keep.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentDisposition {
    /// Move it into `agent:done` (`#waitingarchive`).
    Archive(crate::waiting_archive::ArchiveReason),
    /// Keep it in the tree after the rendered item with this id (`None`:
    /// before every rendered item).
    Retain { after: Option<String> },
}

/// The existing child segments a rebuild does not keep verbatim, and what
/// happens to each. A segment the rebuild regenerates from binary text alone
/// (an open Todo's item with no operator rows) is not listed: it is replaced.
fn waiting_segment_dispositions(
    document: &str,
    location: &WaitingTreeLocation,
    kept_raws: &HashSet<String>,
    open_ids: &HashSet<String>,
    completed: &HashSet<String>,
) -> Vec<(crate::waiting_archive::WaitingSegment, SegmentDisposition)> {
    if location.start == location.end {
        return Vec::new();
    }
    let tree = &document[location.start..location.end];
    let root_line = tree.lines().next().unwrap_or_default();
    let root_indent = root_line.len() - root_line.trim_start_matches([' ', '\t']).len();
    let mut previous_open_id: Option<String> = None;
    let mut out = Vec::new();
    for segment in crate::waiting_archive::waiting_tree_segments(tree, root_indent) {
        let anchor = previous_open_id.clone();
        if let Some(id) = segment.id.as_ref().filter(|id| open_ids.contains(*id)) {
            previous_open_id = Some(id.clone());
        }
        if kept_raws.contains(&segment.raw) {
            continue;
        }
        let disposition = match segment.id.as_deref() {
            Some(id) if !open_ids.contains(id) && completed.contains(id) => {
                SegmentDisposition::Archive(crate::waiting_archive::ArchiveReason::TreeComplete)
            }
            Some(id) if !open_ids.contains(id) => SegmentDisposition::Archive(
                crate::waiting_archive::ArchiveReason::TodoClosed,
            ),
            _ if crate::waiting_archive::segment_has_operator_rows(&segment.raw) => {
                SegmentDisposition::Retain { after: anchor }
            }
            _ => continue,
        };
        out.push((segment, disposition));
    }
    out
}

/// Place retained segments after the rendered item they followed, in their
/// original order.
fn interleave_retained_segments(
    children: Vec<String>,
    child_ids: &[String],
    retained: Vec<(crate::waiting_archive::WaitingSegment, Option<String>)>,
) -> Vec<String> {
    if retained.is_empty() {
        return children;
    }
    let rendered = child_ids.iter().collect::<HashSet<_>>();
    let mut leading = Vec::new();
    let mut after = HashMap::<String, Vec<String>>::new();
    for (segment, anchor) in retained {
        let raw = if segment.raw.ends_with('\n') {
            segment.raw
        } else {
            format!("{}\n", segment.raw)
        };
        match anchor.filter(|id| rendered.contains(id)) {
            Some(id) => after.entry(id).or_default().push(raw),
            None => leading.push(raw),
        }
    }
    let mut out = leading;
    for (child, id) in children.into_iter().zip(child_ids) {
        out.push(child);
        if let Some(segments) = after.remove(id) {
            out.extend(segments);
        }
    }
    out
}

/// Adopt an answered origin row as the waiting item for its matching open Todo.
///
/// `agent:todo` remains authoritative. This projection changes only the queue
/// view: the operator-authored descendants stay byte-for-byte work evidence,
/// while binary metadata is regenerated as inert marked rows. Once adopted,
/// the ordinary waiting-item scheduler selects the parent identity and carries
/// those descendants as `decision_evidence` instead of dispatching them as
/// independent tasks.
fn project_external_waiting_items(
    document: &str,
    todos: &[WaitingTodo],
    packets: &HashMap<String, WaitingOperatorPacket>,
) -> Result<Option<String>> {
    let todo_by_id = todos
        .iter()
        .map(|todo| (todo.id.as_str(), todo))
        .collect::<HashMap<_, _>>();
    let mut replacements = Vec::new();

    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(document);
        let parsed = document_queue::parse_spans(body)?;
        let spans = parsed
            .iter()
            .map(|(_, span)| span.clone())
            .collect::<Vec<_>>();
        let entries = parsed
            .into_iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        let response_metadata_ranges =
            agent_doc_markdown_ast::overlay::response_row_child_ranges(body);
        let mut updated = entries.clone();
        let mut insertions = Vec::new();

        for index in 0..entries.len() {
            let QueueEntry::Prompt(prompt) = &entries[index] else {
                continue;
            };
            if prompt.text.contains(WAITING_ON_YOU_ITEM_MARKER) {
                continue;
            }
            let Some((request_id, todo)) =
                agent_doc_document::queue_projection::operator_request_annotation_ids(&prompt.text)
                    .into_iter()
                    .find_map(|id| todo_by_id.get(id.as_str()).map(|todo| (id, *todo)))
            else {
                continue;
            };
            let subtree_end = entries[index + 1..]
                .iter()
                .position(|entry| {
                    queue_entry_indent(entry).is_some_and(|indent| indent <= prompt.indent)
                })
                .map_or(entries.len(), |offset| index + 1 + offset);
            let has_operator_decision = entries[index + 1..subtree_end]
                .iter()
                .enumerate()
                .any(|(offset, entry)| {
                    let child_index = index + 1 + offset;
                    let generated_response_metadata = response_metadata_ranges
                        .iter()
                        .any(|&(start, end)| {
                            spans[child_index].start >= start && spans[child_index].start < end
                        });
                    !generated_response_metadata
                        && matches!(entry, QueueEntry::Prompt(child)
                    if !crate::waiting_packet::is_agent_doc_reply_entry(&entries, child_index)
                        && !agent_doc_element_queue::struck_work_text(&child.text)
                            .trim_start()
                            .starts_with("**")
                        && !agent_doc_element_queue::struck_work_text(&child.text).trim().is_empty())
                });
            if !has_operator_decision {
                continue;
            }

            let QueueEntry::Prompt(adopted) = &mut updated[index] else {
                unreachable!("source and projected queue entry shapes are identical")
            };
            adopted.text = format!("{} {}", adopted.text.trim_end(), WAITING_ON_YOU_ITEM_MARKER);
            let packet = packets.get(&request_id);
            let response_timestamp = packet
                .and_then(|packet| packet.response_timestamp.as_deref())
                .or_else(|| latest_response_timestamp(document))
                .unwrap_or(FALLBACK_RESPONSE_TIMESTAMP);
            let metadata = render_waiting_metadata(
                todo,
                packet,
                prompt.indent + 2,
                response_timestamp,
            );
            insertions.push((subtree_end, document_queue::parse_fragment(&metadata)?));
        }

        if insertions.is_empty() {
            continue;
        }
        for (at, metadata) in insertions.into_iter().rev() {
            updated.splice(at..at, metadata);
        }
        replacements.push((
            component.open_end,
            component.close_start,
            document_queue::render(&updated),
        ));
    }

    if replacements.is_empty() {
        return Ok(None);
    }
    let mut out = document.to_string();
    for (start, end, body) in replacements.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    Ok(Some(out))
}

/// Migrate legacy generated reply rows without touching operator-authored
/// response edges nested beside them.
fn normalize_agent_doc_reply_rows(raw: &str) -> Result<String> {
    let without_lifecycle =
        agent_doc_markdown_ast::overlay::normalize_generated_response_lifecycle_markers(raw);
    Ok(agent_doc_markdown_ast::overlay::normalize_agent_doc_reply_rows(&without_lifecycle))
}

#[derive(Debug, Clone)]
struct WaitingTreeLocation {
    start: usize,
    end: usize,
    component_open_end: usize,
    component_close_start: usize,
    /// The queue component's `id` attribute, or its marker name.
    queue: String,
}

fn waiting_tree_location(document: &str) -> Result<Option<WaitingTreeLocation>> {
    let components = element::parse(document)?;
    let queue_components = components
        .iter()
        .filter(|component| component.name == "queue")
        .collect::<Vec<_>>();
    let mut roots = queue_components
        .iter()
        .filter_map(|component| {
            let body = component.content(document);
            let marker = body.find(WAITING_ON_YOU_ROOT_MARKER)?;
            let line_start = body[..marker].rfind('\n').map_or(0, |offset| offset + 1);
            Some((*component, component.open_end + line_start))
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        roots.len() <= 1,
        "multiple binary-owned Waiting on you queue trees are present"
    );
    if let Some((component, start)) = roots.pop() {
        let line_end = document[start..]
            .find('\n')
            .map_or(document.len(), |offset| start + offset + 1);
        let raw = &document[start..line_end];
        let indent = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        return Ok(Some(WaitingTreeLocation {
            start,
            end: descendant_region_end(document, line_end, indent),
            component_open_end: component.open_end,
            component_close_start: component.close_start,
            queue: component_queue_name(component),
        }));
    }

    let mut main = queue_components
        .iter()
        .filter(|component| component.attrs.get("id").map(String::as_str) == Some("main"));
    let target = match (main.next(), main.next(), queue_components.as_slice()) {
        (Some(component), None, _) => Some(*component),
        (None, None, [component]) => Some(*component),
        _ => None,
    };
    Ok(target.map(|component| WaitingTreeLocation {
        start: component.close_start,
        end: component.close_start,
        component_open_end: component.open_end,
        component_close_start: component.close_start,
        queue: component_queue_name(component),
    }))
}

fn component_queue_name(component: &element::Component) -> String {
    component
        .attrs
        .get("id")
        .cloned()
        .unwrap_or_else(|| component.name.clone())
}

fn queue_prompt_ids_outside_waiting_tree(
    document: &str,
    waiting: Option<&WaitingTreeLocation>,
) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        for (entry, span) in document_queue::parse_spans(component.content(document))? {
            let absolute = component.open_end + span.start;
            if waiting.is_some_and(|location| absolute >= location.start && absolute < location.end)
            {
                continue;
            }
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            ids.extend(document_queue::queue_prompt_reference_ids(&prompt.text));
        }
    }
    Ok(ids)
}

fn existing_waiting_children(
    document: &str,
    location: &WaitingTreeLocation,
) -> Result<HashMap<String, (String, bool)>> {
    if location.start == location.end {
        return Ok(HashMap::new());
    }
    let body = &document[location.component_open_end..location.component_close_start];
    let entries = document_queue::parse_spans(body)?;
    let root_relative = location.start - location.component_open_end;
    let Some(root_index) = entries
        .iter()
        .position(|(_, span)| span.start == root_relative)
    else {
        return Ok(HashMap::new());
    };
    let mut children = HashMap::new();
    let mut index = root_index + 1;
    while index < entries.len() {
        let (entry, span) = &entries[index];
        let Some(indent) = queue_entry_indent(entry) else {
            index += 1;
            continue;
        };
        if indent == 0 {
            break;
        }
        let QueueEntry::Prompt(prompt) = entry else {
            index += 1;
            continue;
        };
        if indent != 2 || !prompt.text.contains(WAITING_ON_YOU_ITEM_MARKER) {
            index += 1;
            continue;
        }
        // The item's identity is its Todo id. A worker attempt projected on
        // the head (`☑️ [#subagent:x#1] [#id] ...`) is not that identity;
        // reading the owner annotation as the id regenerated a claimed item
        // and dropped its attempt projection.
        let Some(id) = document_queue::queue_prompt_reference_ids(
            agent_doc_element_queue::strip_attempt_projection_prefix(&prompt.text),
        )
        .into_iter()
        .next() else {
            index += 1;
            continue;
        };
        let mut child_end = location.end - location.component_open_end;
        let mut has_operator_reply = false;
        let mut next = index + 1;
        while next < entries.len() {
            let (candidate, candidate_span) = &entries[next];
            // Any list row at the child indent ends this item, including an
            // inert one: an unmatched item typed or left there is its own
            // child, never part of the item above it (`#waitingarchive`).
            if let Some(candidate_indent) = crate::waiting_packet::entry_list_indent(candidate) {
                if candidate_indent <= 2 {
                    child_end = candidate_span.start;
                    break;
                }
                if matches!(candidate, QueueEntry::Prompt(_))
                    && !crate::waiting_packet::is_agent_doc_reply_spanned_entry(&entries, next)
                {
                    has_operator_reply = true;
                }
            }
            next += 1;
        }
        let absolute_start = location.component_open_end + span.start;
        let absolute_end = location.component_open_end + child_end;
        let raw = &document[absolute_start..absolute_end];
        // `#waitingoperatortext`: rows nested under the generated
        // `**Response:**` structure parse as inert response metadata, not
        // prompts, so the prompt scan above missed an operator answer typed
        // under a choice (`- commit + integrate + push`). The refresh then
        // regenerated the whole packet and deleted it (2026-11-03,
        // agent-doc-bugs.md, f7f3fd84bc). Any list row in the subtree that the
        // binary did not author is operator text and pins the existing child.
        let has_operator_reply = has_operator_reply || waiting_child_has_operator_rows(raw);
        // Two children can carry one Todo id (a duplicate an older binary
        // left). The one holding operator text represents the item, else the
        // first; the other stays an unmatched child (`#waitingarchive`).
        // Last-wins made the choice depend on position, so the kept child and
        // its duplicate swapped places on every rebuild.
        match children.get(&id) {
            Some((_, kept_has_reply)) if *kept_has_reply || !has_operator_reply => {}
            _ => {
                children.insert(id, (raw.to_string(), has_operator_reply));
            }
        }
        index = next.max(index + 1);
    }
    Ok(children)
}

/// True when a waiting child's subtree (below its item row) holds a list row
/// the binary did not author: not a reply/metadata row, and not a binary-owned
/// worker-attempt projection (`#waitingoperatortext`).
pub(crate) fn waiting_child_has_operator_rows(raw: &str) -> bool {
    let lines = raw.lines().collect::<Vec<_>>();
    lines.iter().enumerate().skip(1).any(|(index, line)| {
        let Some(text) = line.trim_start().strip_prefix("- ") else {
            return false;
        };
        let text = text.trim();
        if text.is_empty()
            || agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_at(&lines, index)
        {
            return false;
        }
        let work = agent_doc_element_queue::struck_work_text(text);
        let binary_attempt = agent_doc_document::queue_projection::lifecycle_marker(work.trim())
            .is_some()
            && agent_doc_document::queue_projection::owner_projection(work.trim()).is_some();
        !binary_attempt
    })
}

fn queue_entry_indent(entry: &QueueEntry) -> Option<usize> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt.indent),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct WaitingTodo {
    id: String,
    action: String,
}

/// Lowercased ids of every open `agent:todo` item: the waiting items a
/// closeout keeps live.
pub(crate) fn open_waiting_todo_ids(document: &str) -> HashSet<String> {
    open_waiting_todos(document)
        .into_iter()
        .map(|todo| todo.id.to_ascii_lowercase())
        .collect()
}

fn open_waiting_todos(document: &str) -> Vec<WaitingTodo> {
    let Ok(components) = element::parse(document) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut todos = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "todo")
    {
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(component.content(document));
        for item in items {
            if item.is_done() || item.id.is_empty() || !seen.insert(item.id.clone()) {
                continue;
            }
            todos.push(WaitingTodo {
                id: item.id,
                action: one_line(&item.text),
            });
        }
    }
    todos
}

#[derive(Debug, Clone, Default)]
struct WaitingOperatorPacket {
    request: String,
    context: String,
    choices: Vec<String>,
    response_timestamp: Option<String>,
}

fn waiting_operator_packets(
    document: &str,
    response_body: &str,
) -> HashMap<String, WaitingOperatorPacket> {
    let mut packets = HashMap::new();
    for source in [document, response_body] {
        for (id, packet) in parse_waiting_operator_packets(source) {
            packets.insert(id, packet);
        }
    }
    packets
}

fn parse_waiting_operator_packets(source: &str) -> Vec<(String, WaitingOperatorPacket)> {
    #[derive(Clone, Copy)]
    enum Field {
        Request,
        Context,
        Choices,
        Other,
    }

    let lines = source.lines().collect::<Vec<_>>();
    let mut packets = Vec::new();
    let mut current_response_timestamp = None;
    let mut index = 0;
    while index < lines.len() {
        if let Some(timestamp) = response_heading_timestamp(lines[index]) {
            current_response_timestamp = Some(timestamp.to_string());
            index += 1;
            continue;
        }
        let Some(id) = operator_packet_id(lines[index]) else {
            index += 1;
            continue;
        };
        let mut packet = WaitingOperatorPacket {
            response_timestamp: current_response_timestamp.clone(),
            ..Default::default()
        };
        let mut blocking = false;
        let mut field = Field::Other;
        index += 1;
        while index < lines.len() {
            let line = lines[index];
            if operator_packet_id(line).is_some()
                || line.trim_start().starts_with("### Re:")
                || line.trim_start().starts_with("<!-- /agent:")
            {
                break;
            }
            let trimmed = line.trim();
            let de_bulleted = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            let plain = de_bulleted.replace('*', "");
            if let Some((label, value)) = plain.split_once(':') {
                let label = label.trim().to_ascii_lowercase();
                let value = value.trim();
                match label.as_str() {
                    "blocking" => {
                        blocking = value.eq_ignore_ascii_case("true");
                        field = Field::Other;
                    }
                    "request" => {
                        field = Field::Request;
                        if !value.is_empty() {
                            packet.request = one_line(value);
                        }
                    }
                    "context" => {
                        field = Field::Context;
                        if !value.is_empty() {
                            packet.context = one_line(value);
                        }
                    }
                    "choices" => field = Field::Choices,
                    _ if matches!(field, Field::Choices) => {
                        packet.choices.push(one_line(de_bulleted));
                    }
                    _ => field = Field::Other,
                }
            } else if !de_bulleted.is_empty() {
                match field {
                    Field::Request => append_field(&mut packet.request, de_bulleted),
                    Field::Context => append_field(&mut packet.context, de_bulleted),
                    Field::Choices => packet.choices.push(one_line(de_bulleted)),
                    Field::Other => {}
                }
            }
            index += 1;
        }
        if blocking && !packet.request.is_empty() {
            packets.push((id, packet));
        }
    }
    packets
}

fn operator_packet_id(line: &str) -> Option<String> {
    if !line
        .to_ascii_lowercase()
        .contains("subagent operator request")
    {
        return None;
    }
    let marker = line.find("[#")? + 2;
    let end = line[marker..].find(']')? + marker;
    let id = &line[marker..end];
    (!id.is_empty()).then(|| id.to_ascii_lowercase())
}

fn append_field(target: &mut String, value: &str) {
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(&one_line(value));
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn response_heading_timestamp(line: &str) -> Option<&str> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let without_hashes = trimmed.trim_start_matches('#').trim_start();
    let rest = without_hashes.strip_prefix("Re:")?.trim();
    let (_, timestamp) = rest.rsplit_once(" · ")?;
    let timestamp = timestamp.trim();
    (!timestamp.is_empty()).then_some(timestamp)
}

pub(crate) fn latest_response_timestamp(source: &str) -> Option<&str> {
    source
        .lines()
        .filter_map(response_heading_timestamp)
        .next_back()
}

fn ensure_waiting_response_timestamp(child: &str, response_timestamp: &str) -> String {
    let lines = child.split_inclusive('\n').collect::<Vec<_>>();
    let is_reply =
        |index: usize| agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_at(&lines, index);
    if lines
        .iter()
        .enumerate()
        .any(|(index, line)| line.trim_start().starts_with("- **Timestamp:** ") && is_reply(index))
    {
        return child.to_string();
    }

    let mut out = String::with_capacity(child.len() + response_timestamp.len() + 80);
    let mut inserted = false;
    for (index, line) in lines.iter().enumerate() {
        out.push_str(line);
        if !inserted && line.contains("**Response:**") && is_reply(index) {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("      - **Timestamp:** ");
            out.push_str(response_timestamp);
            out.push(' ');
            out.push_str(&agent_doc_reply_suffix());
            out.push('\n');
            inserted = true;
        }
    }
    out
}

fn render_waiting_child(
    todo: &WaitingTodo,
    packet: Option<&WaitingOperatorPacket>,
    response_timestamp: &str,
) -> String {
    let mut out = format!(
        "  - [#{}] {} {}\n",
        todo.id, todo.action, WAITING_ON_YOU_ITEM_MARKER
    );
    out.push_str(&render_waiting_metadata(
        todo,
        packet,
        4,
        response_timestamp,
    ));
    out.replace("> **Queue prompt:**", "&gt; **Queue prompt:**")
}

fn render_waiting_metadata(
    todo: &WaitingTodo,
    packet: Option<&WaitingOperatorPacket>,
    indent: usize,
    response_timestamp: &str,
) -> String {
    let summary = packet
        .map(|packet| packet.request.as_str())
        .filter(|summary| !summary.is_empty())
        .unwrap_or(&todo.action);
    let row = " ".repeat(indent);
    let child = " ".repeat(indent + 2);
    let grandchild = " ".repeat(indent + 4);
    let reply_suffix = agent_doc_reply_suffix();
    let mut out = format!("{row}- **Response:** {summary} {reply_suffix}");
    // `#waitingresponsededup`: this is a free-text Response node, so its text
    // is itself the summary. A nested Summary belongs only under an identity
    // container (`Response [#id]`), which this waiting projection does not
    // render. Never duplicate or paraphrase this free-text evidence into a
    // second metadata node.
    out.push_str(&format!("\n{child}- **Timestamp:** "));
    out.push_str(response_timestamp);
    out.push(' ');
    out.push_str(&reply_suffix);
    if let Some(packet) = packet {
        if !packet.context.is_empty() {
            out.push_str(&format!("\n{child}- **Context:** "));
            out.push_str(&packet.context);
            out.push(' ');
            out.push_str(&reply_suffix);
        }
        out.push_str(&format!("\n{child}- **Question:** "));
        out.push_str(&packet.request);
        out.push(' ');
        out.push_str(&reply_suffix);
        if !packet.choices.is_empty() {
            out.push_str(&format!("\n{child}- **Choices:** "));
            out.push_str(&reply_suffix);
            for choice in &packet.choices {
                out.push_str(&format!("\n{grandchild}- "));
                out.push_str(choice);
                out.push(' ');
                out.push_str(&reply_suffix);
            }
        }
    } else {
        out.push_str(&format!(
            "\n{child}- **Question:** What decision or completion result should be recorded? "
        ));
        out.push_str(&reply_suffix);
    }
    out.push_str(&format!("\n{child}- **Next actions:** "));
    out.push_str(&reply_suffix);
    out.push_str(&format!("\n{grandchild}- **Action:** "));
    out.push_str(&todo.action);
    out.push(' ');
    out.push_str(&reply_suffix);
    out.push('\n');
    // A Todo may discuss agent-doc's own exchange syntax. The waiting tree is
    // generated queue state, so encode that boundary token in the mirror while
    // leaving the Todo authority untouched.
    out.replace("> **Queue prompt:**", "&gt; **Queue prompt:**")
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

    fn waiting_tree_document(queue_body: &str, todo_body: &str, response: &str) -> String {
        format!(
            "<!-- agent:queue id=\"main\" -->\n{queue_body}<!-- /agent:queue -->\n\
             <!-- agent:todo -->\n{todo_body}<!-- /agent:todo -->\n\
             <!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n"
        )
    }

    /// `#answerack`: the `tdclireleasedownloads` shape from `sdk-dev.md`. The
    /// operator answered the waiting item inside its packet; the response
    /// deferred the work but cited the answer's id.
    const ANSWERACK_QUEUE: &str = concat!(
        "- ### 📥 Waiting on you — answer by adding a child line under an item [#q-4775b8697412]\n",
        "  - [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->\n",
        "    - **Response:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "    - **Next actions:** <!-- agent-doc:waiting-metadata -->\n",
        "        - **Action:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
    );
    const ANSWERACK_TODO: &str =
        "- [ ] [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads\n";
    const ANSWERACK_RESPONSE: &str = concat!(
        "### Re: haiven-cli public downloads — test\n\n",
        "> **Queue prompt:** [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads\n\n",
        "**Deferred:** The operator reply on this leaf is public and open source (`[#q-f09f08bcf721]`). An org owner sets the repository public.\n",
    );

    #[test]
    fn a_deferred_response_that_cites_an_answer_acknowledges_it_as_consumed() {
        let before = waiting_tree_document(ANSWERACK_QUEUE, ANSWERACK_TODO, "");
        let after = waiting_tree_document(ANSWERACK_QUEUE, ANSWERACK_TODO, ANSWERACK_RESPONSE);

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, ANSWERACK_RESPONSE)
                .unwrap();
        let response_id = annotated
            .lines()
            .find_map(|line| {
                line.strip_prefix("### Re: haiven-cli public downloads — test ")
                    .map(str::to_string)
            })
            .expect("the consuming response receives a lookup id");
        assert!(
            annotated.contains(&format!(
                concat!(
                    "          - ~~haiven-cli will be public + open source [#q-f09f08bcf721]~~\n",
                    "            - **Consumed by:** {}. Keep this waiting item: ",
                    "[#tdclireleasedownloads] is still open.\n",
                ),
                response_id
            )),
            "{annotated}"
        );
        // The deferred head itself stays live: only its consumed answer closed.
        assert!(annotated.contains(
            "  - [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->\n"
        ));
        // Replaying the same closeout changes nothing.
        let replayed = annotate_newly_struck_queue_items_with_response_id(
            &before,
            &annotated,
            ANSWERACK_RESPONSE,
        )
        .unwrap();
        assert_eq!(replayed, annotated);
        // The waiting refresh keeps the acknowledgement and no longer asks the
        // operator for an answer the packet already holds.
        let refreshed = project_waiting_on_you_tree(&annotated, ANSWERACK_RESPONSE)
            .unwrap()
            .unwrap_or_else(|| annotated.clone());
        assert!(
            refreshed.contains("- ~~haiven-cli will be public + open source [#q-f09f08bcf721]~~"),
            "{refreshed}"
        );
        assert!(
            refreshed.contains(&format!("- **Consumed by:** {response_id}.")),
            "{refreshed}"
        );
        assert!(
            !refreshed.contains("no answer or attempt in its packet yet"),
            "{refreshed}"
        );
    }

    #[test]
    fn an_uncited_answer_stays_live() {
        let response = ANSWERACK_RESPONSE.replace(" (`[#q-f09f08bcf721]`)", "");
        let before = waiting_tree_document(ANSWERACK_QUEUE, ANSWERACK_TODO, "");
        let after = waiting_tree_document(ANSWERACK_QUEUE, ANSWERACK_TODO, &response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        assert_eq!(
            annotated,
            agent_doc_markdown_ast::overlay::normalize_generated_response_lifecycle_markers(&after)
        );
    }

    #[test]
    fn terminalizing_a_waiting_item_consumes_its_answers_and_states_tree_closability() {
        let queue = ANSWERACK_QUEUE.replace(
            "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
            concat!(
                "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
                "          - mark [#tdclireleasedownloads] as done unless there's more left to do\n",
            ),
        );
        let struck = queue.replace(
            "  - [#tdclireleasedownloads] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->\n",
            "  - ~~[#tdclireleasedownloads] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->~~\n",
        );
        let response = ANSWERACK_RESPONSE.replace("**Deferred:**", "**Outcome:**");
        let done_todo = ANSWERACK_TODO.replace("- [ ]", "- [x]");
        let before = waiting_tree_document(&queue, ANSWERACK_TODO, "");
        let after = waiting_tree_document(&struck, &done_todo, &response);

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        assert_eq!(
            annotated.matches("- **Consumed by:** [#r-").count(),
            2,
            "both the cited and the uncited answer under the terminalized item are consumed:\n{annotated}"
        );
        assert!(
            annotated.contains(
                "- ~~mark [#tdclireleasedownloads] as done unless there's more left to do~~\n"
            ),
            "{annotated}"
        );
        assert!(
            annotated.contains("**Guidance:** Safe to delete the whole tree under [#q-4775b8697412]; every row in it is struck"),
            "{annotated}"
        );

        // With the todo still open, the struck copy is not deletable, and the
        // guidance names the row that keeps it open.
        let after_open = waiting_tree_document(&struck, ANSWERACK_TODO, &response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after_open, &response)
                .unwrap();
        assert!(
            annotated.contains(
                "Keep the tree under [#q-4775b8697412]: [#tdclireleasedownloads] is still open."
            ),
            "{annotated}"
        );
    }

    #[test]
    fn struck_consolidate_choice_request_closes_its_todo_and_only_its_todo() {
        let request_id = "subagent-operator-request-consolidate-choice-receipt";
        let document = format!(
            "<!-- agent:queue -->\n- ~~[#{request_id}]~~\n<!-- /agent:queue -->\n\
             <!-- agent:todo -->\n\
             - [ ] [#{request_id}] Decide whether the committed exchange response and integration receipt may close the lost consolidate-choice attempt\n\
             - [ ] [#unrelated-decision] Keep this independent decision open\n\
             <!-- /agent:todo -->\n"
        );

        let outcome = reconcile_terminal_operator_request_todos(&document, &[]).unwrap();

        assert_eq!(outcome.completed_ids, vec![request_id]);
        assert!(
            outcome.content.contains(&format!("- [x] [#{request_id}]")),
            "{}",
            outcome.content
        );
        assert!(
            outcome
                .content
                .contains("- [ ] [#unrelated-decision] Keep this independent decision open"),
            "{}",
            outcome.content
        );
    }

    #[test]
    fn duplicate_terminal_request_rows_and_todos_reconcile_once() {
        let request_id = "subagent-operator-request-duplicate-choice";
        let document = format!(
            "<!-- agent:queue -->\n\
             - ~~[#{request_id}]~~\n\
             - ~~[#{request_id}]: integrate~~\n\
             <!-- /agent:queue -->\n\
             <!-- agent:todo -->\n\
             - [x] [#{request_id}] Choose the integration sink\n\
             - [ ] [#{request_id}] Choose the integration sink\n\
             <!-- /agent:todo -->\n"
        );

        let outcome = reconcile_terminal_operator_request_todos(&document, &[]).unwrap();

        assert_eq!(outcome.completed_ids, vec![request_id]);
        assert_eq!(
            outcome
                .content
                .matches(&format!("- [x] [#{request_id}]"))
                .count(),
            1,
            "duplicate todo copies should fold to one terminal receipt:\n{}",
            outcome.content
        );
        assert!(!outcome.content.contains(&format!("- [ ] [#{request_id}]")));
    }

    #[test]
    fn live_duplicate_request_keeps_todo_open_and_citations_are_not_identity() {
        let request_id = "subagent-operator-request-still-open";
        let document = format!(
            "<!-- agent:queue -->\n\
             - ~~[#{request_id}]~~\n\
             - [#{request_id}]\n\
             <!-- /agent:queue -->\n\
             <!-- agent:todo -->\n\
             - [ ] [#{request_id}] Await the remaining decision copy\n\
             - [ ] [#citation-only] Follow up on [#{request_id}] later\n\
             <!-- /agent:todo -->\n"
        );

        let outcome = reconcile_terminal_operator_request_todos(&document, &[]).unwrap();

        assert!(outcome.completed_ids.is_empty());
        assert_eq!(outcome.content, document);

        let without_live_duplicate = document.replacen(&format!("- [#{request_id}]\n"), "", 1);
        let outcome =
            reconcile_terminal_operator_request_todos(&without_live_duplicate, &[]).unwrap();
        assert!(outcome.content.contains("- [ ] [#citation-only]"));
        assert!(outcome.content.contains(&format!("- [x] [#{request_id}]")));
    }

    #[test]
    fn closeout_projects_open_todos_into_one_inert_waiting_tree() {
        let response = "### Re: operator work — test\n\nReview the open operator items.";
        let document = waiting_tree_document(
            "- ### existing queue work\n",
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );

        let projected = project_waiting_on_you_tree(&document, response)
            .unwrap()
            .expect("open todo should create the tree");
        assert_eq!(projected.matches(WAITING_ON_YOU_ROOT_MARKER).count(), 1);
        assert!(
            projected.contains(
                "- ⟦you: 1 · agent: 0 · done: 0⟧ [#approve-pr] Approve and merge PR #42."
            ),
            "{projected}"
        );
        assert!(
            !projected.contains("**Summary:** Approve and merge PR #42."),
            "an identical Response/summary must project once: {projected}"
        );
        assert!(projected.contains("**Next actions:**"));
        assert!(projected.contains("**Action:** Approve and merge PR #42."));
        assert!(projected.contains(&agent_doc_reply_suffix()), "{projected}");
        assert!(
            !projected.contains(LEGACY_AGENT_DOC_REPLY_MARKER)
                && !projected.contains(LEGACY_WAITING_METADATA_MARKER),
            "new projections carry only the bare trailing glyph: {projected}"
        );
        for line in projected
            .lines()
            .filter(|line| line.contains(AGENT_DOC_REPLY_GLYPH))
        {
            assert!(
                line.ends_with(&format!(" {AGENT_DOC_REPLY_GLYPH}")),
                "the glyph is the row's last token: {line}"
            );
        }
        assert!(
            !projected.contains("~~**Response:")
                && !projected.contains("~~**Question:")
                && !projected.contains("~~**Action:"),
            "response evidence stays readable and unstruck: {projected}"
        );

        let queue = element::parse(&projected)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&projected)).unwrap();
        assert!(
            document_queue::prompts(&entries)
                .into_iter()
                .all(|prompt| !prompt.text.contains("Waiting on you")
                    && !prompt.text.contains("Approve and merge")),
            "generated waiting rows must never dispatch: {entries:?}"
        );
        assert_eq!(
            project_waiting_on_you_tree(&projected, response).unwrap(),
            None,
            "the closeout projection must be replay-idempotent"
        );
    }

    #[test]
    fn waiting_tree_does_not_duplicate_identical_response_and_summary() {
        let action = "Reopen the run-game sessions after the policy decision.";
        let document = waiting_tree_document(
            "",
            &format!("- [ ] [#rungamesessions-reopen] {action}\n"),
            "",
        );

        let projected = project_waiting_on_you_tree(&document, "")
            .unwrap()
            .expect("open Todo should create the waiting projection");
        assert_eq!(projected.matches(action).count(), 4, "{projected}");
        // Durable Todo + waiting item + Response + Action are the four
        // legitimate occurrences. A fifth would be the redundant Summary.
        assert!(!projected.contains("**Summary:**"), "{projected}");
    }

    #[test]
    fn waiting_tree_encodes_conversation_scaffold_mentioned_by_a_todo() {
        let document = waiting_tree_document(
            "",
            "- [ ] [#echo-guard] Fix the `> **Queue prompt:**` steering false positive.\n",
            "",
        );

        let projected = project_waiting_on_you_tree(&document, "")
            .unwrap()
            .expect("the open Todo should create the waiting tree");

        assert!(
            projected.contains("`&gt; **Queue prompt:**`"),
            "{projected}"
        );
        let queue = element::parse(&projected)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        assert!(
            !queue.content(&projected).contains("> **Queue prompt:**"),
            "generated queue content must not contain the exchange boundary token: {projected}"
        );
    }

    #[test]
    fn waiting_tree_uses_blocking_packet_context_question_and_choices() {
        let id = "subagent-operator-request-release-target";
        let response = format!(
            "### Re: release target — test · 2026-11-02T10:45-05:00\n\n\
             [#{id}] subagent operator request\n\
             - *kind:* question\n\
             - *blocking:* true\n\
             - *request:* Which release target should be used?\n\
             - *context:* The build is ready.\n\
             - *choices:*\n\
               - **staging**: Publish to staging.\n\
               - **production**: Publish to production.\n"
        );
        let document = waiting_tree_document(
            "",
            &format!("- [ ] [#{id}] Answer the release question.\n"),
            &response,
        );

        let projected = project_waiting_on_you_tree(&document, &response)
            .unwrap()
            .expect("blocking todo should create the tree");
        assert!(
            projected.contains("**Context:** The build is ready."),
            "{projected}"
        );
        assert!(
            projected.contains("**Question:** Which release target should be used?"),
            "{projected}"
        );
        assert!(
            projected.contains(&format!(
                "**Timestamp:** 2026-11-02T10:45-05:00 {}",
                agent_doc_reply_suffix()
            )),
            "{projected}"
        );
        assert!(
            projected.contains("**staging**: Publish to staging."),
            "{projected}"
        );
        assert!(
            projected.contains("**production**: Publish to production."),
            "{projected}"
        );
        assert!(
            !projected.contains("**Summary:**"),
            "a free-text Response is already its own summary: {projected}"
        );
    }

    #[test]
    fn operator_decision_under_origin_parent_adopts_durable_response_structure() {
        let id = "subagent-operator-request-late-admission-commit-v2";
        let parent = "Recover protected preflight cycles and reconcile late admission.";
        let response = format!(
            "### Re: late admission — test\n\n\
             [#{id}] subagent operator request\n\
             - *queue_item:*\n\
               - {parent}\n\
             - *kind:* permission\n\
             - *blocking:* true\n\
             - *request:* May this worker create one local git commit?\n\
             - *context:* The fix and focused tests are complete.\n\
             - *choices:*\n\
               - **Authorize local commit**: Create one local commit.\n\
               - **Leave uncommitted**: Keep the verified worktree uncommitted.\n"
        );
        let document = waiting_tree_document(
            &format!("- {parent} [#{id}]\n  - commit + integrate\n"),
            &format!("- [ ] [#{id}] Choose whether to create a local commit.\n"),
            &response,
        );

        let projected = project_waiting_on_you_tree(&document, &response)
            .unwrap()
            .expect("the answered origin row should adopt the waiting projection");
        assert!(
            !projected.contains(WAITING_ON_YOU_ROOT_MARKER),
            "{projected}"
        );
        assert!(
            projected.contains(&format!("{parent} [#{id}] {WAITING_ON_YOU_ITEM_MARKER}")),
            "{projected}"
        );
        assert!(
            projected.contains("**Response:** May this worker create one local git commit?"),
            "{projected}"
        );
        assert!(
            projected.contains("**Context:** The fix and focused tests are complete."),
            "{projected}"
        );
        assert!(
            projected.contains("**Authorize local commit**"),
            "{projected}"
        );

        let queue = element::parse(&projected)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&projected)).unwrap();
        let executable = document_queue::executable_prompt_indexes(&entries);
        assert_eq!(executable, vec![0], "{entries:#?}");
        let packet = document_queue::prompt_context_packet_at(&entries, executable[0]).unwrap();
        assert!(packet.origin.item_text.starts_with(parent), "{packet:#?}");
        assert_eq!(packet.decision_evidence, ["commit + integrate"]);
        assert_eq!(
            packet.producer.kind,
            agent_doc_prompt_context::lineage::PromptProducerKind::Operator
        );
        assert_eq!(
            project_waiting_on_you_tree(&projected, &response).unwrap(),
            None,
            "adoption must be replay-idempotent"
        );
    }

    #[test]
    fn live_queue_row_prevents_waiting_tree_duplication() {
        let response = "### Re: approval — test\n\nWaiting for approval.";
        let document = waiting_tree_document(
            "- ### Review PR #42 [#approve-pr]\n",
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );

        assert_eq!(
            project_waiting_on_you_tree(&document, response).unwrap(),
            None
        );
    }

    #[test]
    fn external_live_row_removes_an_existing_waiting_child() {
        let response = "### Re: approval — test\n\nWaiting for approval.";
        let document = waiting_tree_document(
            "",
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );
        let projected = project_waiting_on_you_tree(&document, response)
            .unwrap()
            .unwrap();
        let externalized = projected.replace(
            "<!-- agent:queue id=\"main\" -->\n",
            "<!-- agent:queue id=\"main\" -->\n- ### Review PR #42 [#approve-pr]\n",
        );

        let cleaned = project_waiting_on_you_tree(&externalized, response)
            .unwrap()
            .expect("the generated duplicate should be removed");
        assert!(!cleaned.contains(WAITING_ON_YOU_ROOT_MARKER), "{cleaned}");
        assert_eq!(cleaned.matches("[#approve-pr]").count(), 2, "{cleaned}");
    }

    #[test]
    fn operator_reply_selects_waiting_item_until_the_todo_resolves_then_tree_leaves() {
        let response = "### Re: approval — test\n\nWaiting for approval.";
        let document = waiting_tree_document(
            "",
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );
        let projected = project_waiting_on_you_tree(&document, response)
            .unwrap()
            .unwrap();
        let with_reply = projected.replacen(
            "<!-- agent-doc:waiting-item -->\n",
            "<!-- agent-doc:waiting-item -->\n    - approved\n",
            1,
        );
        // The reply moves the item's state from `🙋` (operator owes) to `📥`
        // (answered, waiting for an attempt); nothing else changes, and the
        // next refresh is a fixed point.
        let with_reply = project_waiting_on_you_tree(&with_reply, response)
            .unwrap()
            .expect("the state token follows the reply");
        assert!(
            with_reply.contains(concat!(
                "  - ⟦📥 you: 0 · agent: 1 · done: 0⟧ [#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->\n",
                "    - **Next:** agent — pick up your reply “approved” and attempt `#approve-pr` (evidence: your reply) ▫️\n",
                "    - approved\n",
            )),
            "refresh must preserve an operator reply byte-for-byte: {with_reply}"
        );
        assert_eq!(
            project_waiting_on_you_tree(&with_reply, response).unwrap(),
            None
        );
        let queue = element::parse(&with_reply)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&with_reply)).unwrap();
        let executable = document_queue::executable_prompt_indexes(&entries);
        assert_eq!(executable.len(), 1, "{entries:#?}");
        let head = document_queue::lineage_claim_text(&entries, executable[0]).unwrap();
        assert!(head.contains("[#approve-pr]"), "{head}");
        assert!(
            !head.contains("approved"),
            "response evidence is not work: {head}"
        );
        let packet = document_queue::prompt_context_packet_at(&entries, executable[0]).unwrap();
        assert_eq!(packet.decision_evidence, ["approved"]);

        let resolved = with_reply.replace(
            "- [ ] [#approve-pr] Approve and merge PR #42.",
            "- [x] [#approve-pr] Approve and merge PR #42.",
        );
        let cleaned = project_waiting_on_you_tree(&resolved, response)
            .unwrap()
            .expect("resolved todo should remove the tree");
        assert!(!cleaned.contains(WAITING_ON_YOU_ROOT_MARKER), "{cleaned}");
        // `#waitingarchive`: the closed item leaves the queue, but its packet,
        // including the operator's reply, moves into agent:done verbatim.
        let queue = element::parse(&cleaned)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        assert!(!queue.content(&cleaned).contains("approved"), "{cleaned}");
        let done = element::parse(&cleaned)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "done")
            .expect("the archive creates agent:done");
        let done = done.content(&cleaned);
        assert!(done.contains("  >     - approved\n"), "{done}");
        assert_eq!(
            done.matches(crate::waiting_archive::WAITING_ARCHIVE_MARKER)
                .count(),
            1,
            "{done}"
        );
        assert_eq!(
            project_waiting_on_you_tree(&cleaned, response).unwrap(),
            None,
            "archiving is a fixed point"
        );
    }

    /// `#waitingoperatortext`: the 2026-11-03 agent-doc-bugs.md loss
    /// (f7f3fd84bc). The operator typed `commit + integrate + push` beneath a
    /// choice inside the generated `**Response:**` packet. Those rows parse as
    /// inert response metadata, so the refresh saw no operator reply and
    /// regenerated the packet as the generic stub, deleting the answer and the
    /// Context/Choices/Outcome rows. A closeout whose response never mentions
    /// the item must leave that child byte-for-byte.
    #[test]
    fn refresh_never_regenerates_over_an_operator_answer_inside_the_response_packet() {
        // The exact pre-loss child from agent-doc-bugs.md at 1acbe00f09.
        let child = r##"  - [#subagent-operator-request-queue-ast-strings-persist] Answer the blocking subagent question for queue item ‘The update to agent-doc-bugs.md `agent:Run is malformed.` My string quote `- ### /agent-doc did not start a session cycle. Connected admission failed three times, so no document was 5:01 PM…’ in agent:exchange: Choose whether to commit and push, commit only, or leave uncommitted the completed AST string/list-item fix. (choices: Commit and push, Commit only, Leave uncommitted) <!-- agent-doc:waiting-item -->
    - **Response:** Choose whether to commit and push, commit only, or leave uncommitted the completed AST string/list-item fix. ▫️ <!-- agent-doc:reply -->
      - **Timestamp:** 2026-11-02T18:17-05:00 ▫️ <!-- agent-doc:reply -->
      - **Context:** The implementation and targeted verification are complete. The changes are currently uncommitted on `fix/queue-ast-string-literals-20261102` in `/home/brian/worktrees/agent-doc-queue-ast-string-literals-20261102`. The unresolved `#spec-test-build-install-commit-push` preset supplies no commit or push authorization. The target scope is the two modified Rust files for this queue item only. Committing records the work locally; pushing publishes the branch remotely; leaving it uncommitted preserves the current isolated worktree state. ▫️ <!-- agent-doc:reply -->
      - **Question:** Choose whether to commit and push, commit only, or leave uncommitted the completed AST string/list-item fix. ▫️ <!-- agent-doc:reply -->
      - **Choices:** ▫️ <!-- agent-doc:reply -->
        - **Commit and push**: Create a local commit and publish this branch. ▫️ <!-- agent-doc:reply -->
          - commit + integrate + push
        - **Commit only**: Create a local commit without publishing it. ▫️ <!-- agent-doc:reply -->
        - **Leave uncommitted**: Keep the verified changes only in the isolated worktree. ▫️ <!-- agent-doc:reply -->
        - **Outcome:** The compact-supersession wedge is diagnosed and fixed in one uncommitted file. Exact stale compact-target writes are retired while newer/composed writes and the captured response remain. The state-backbone suite passed 149/149 tests. ▫️ <!-- agent-doc:reply -->
      - **Next actions:** ▫️ <!-- agent-doc:reply -->
        - **Action:** Answer the blocking subagent question for queue item ‘The update to agent-doc-bugs.md `agent:Run is malformed.` My string quote `- ### /agent-doc did not start a session cycle. Connected admission failed three times, so no document was 5:01 PM…’ in agent:exchange: Choose whether to commit and push, commit only, or leave uncommitted the completed AST string/list-item fix. (choices: Commit and push, Commit only, Leave uncommitted) ▫️ <!-- agent-doc:reply -->
"##;
        let todo = r##"- [ ] [#subagent-operator-request-queue-ast-strings-persist] Answer the blocking subagent question for queue item ‘The update to agent-doc-bugs.md `agent:Run is malformed.` My string quote `- ### /agent-doc did not start a session cycle. Connected admission failed three times, so no document was 5:01 PM…’ in agent:exchange: Choose whether to commit and push, commit only, or leave uncommitted the completed AST string/list-item fix. (choices: Commit and push, Commit only, Leave uncommitted)
"##;
        let queue = format!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n{child}"
        );
        let response =
            "### Re: unrelated closeout · 2026-11-03T18:25-05:00\n\nNothing about that item.";
        let document = waiting_tree_document(&queue, todo, response);

        let refreshed = project_waiting_on_you_tree(&document, response).unwrap();
        let refreshed = refreshed.as_deref().unwrap_or(&document);
        // Only the binary-owned state token and Next row are added.
        let without_token = refreshed.replace("⟦📥 you: 0 · agent: 1 · done: 0⟧ ", "");
        let lines = without_token.lines().collect::<Vec<_>>();
        let without_token = (0..lines.len())
            .filter(|index| !crate::waiting_status::is_next_row_at(&lines, *index))
            .map(|index| format!("{}\n", lines[index]))
            .collect::<String>();
        // `#barereplymarker`: the generated rows migrate to the bare trailing
        // glyph; every other byte, including the operator's answer, survives.
        let migrated = child.replace(&format!(" {LEGACY_AGENT_DOC_REPLY_MARKER}"), "");
        assert!(
            without_token.contains(&migrated),
            "the operator-answered packet must survive byte-for-byte:\n{refreshed}"
        );
        assert!(
            !refreshed.contains(LEGACY_AGENT_DOC_REPLY_MARKER),
            "{refreshed}"
        );
        assert!(refreshed.contains("          - commit + integrate + push\n"));
        assert!(!refreshed.contains("What decision or completion result should be recorded?"));
    }

    /// Without an operator answer, an existing rich packet is still not
    /// downgraded to the generic stub when no fresher packet is available.
    #[test]
    fn refresh_keeps_an_existing_packet_when_no_fresher_packet_exists() {
        let reply = agent_doc_reply_suffix();
        let child = format!(
            concat!(
                "  - [#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->\n",
                "    - **Response:** Approve and merge PR #42 after CI. {r}\n",
                "      - **Timestamp:** 2026-11-02T18:17-05:00 {r}\n",
                "      - **Context:** CI is green on the release branch. {r}\n",
                "      - **Question:** Merge now or wait for the soak? {r}\n",
                "      - **Next actions:** {r}\n",
                "        - **Action:** Approve and merge PR #42. {r}\n",
            ),
            r = reply,
        );
        let queue = format!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n{child}"
        );
        let response = "### Re: unrelated · 2026-11-03T18:25-05:00\n\nNothing.";
        let document = waiting_tree_document(
            &queue,
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );
        let refreshed = project_waiting_on_you_tree(&document, response).unwrap();
        let refreshed = refreshed.as_deref().unwrap_or(&document);
        assert!(
            refreshed.contains("CI is green on the release branch."),
            "{refreshed}"
        );
        assert!(!refreshed.contains("What decision or completion result should be recorded?"));
    }

    #[test]
    fn refresh_migrates_legacy_reply_rows_without_losing_answer_edges() {
        let response = "### Re: approval — test · 2026-11-02T10:45-05:00\n\nWaiting for approval.";
        let legacy = concat!(
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->\n",
            "    - ~~**Response:** Approve and merge PR #42. <!-- agent-doc:waiting-metadata -->~~\n",
            "      - ~~**Question:** What decision should be recorded? <!-- agent-doc:waiting-metadata -->~~\n",
            "        - approved\n",
        );
        let document = waiting_tree_document(
            legacy,
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            response,
        );

        let repaired = project_waiting_on_you_tree(&document, response)
            .unwrap()
            .expect("legacy struck evidence should migrate");
        assert!(repaired.contains("        - approved\n"), "{repaired}");
        assert!(repaired.contains("- **Response:** Approve and merge PR #42."));
        assert!(repaired.contains("- **Timestamp:** 2026-11-02T10:45-05:00"));
        assert!(repaired.contains(&agent_doc_reply_suffix()), "{repaired}");
        assert!(
            !repaired.contains(LEGACY_AGENT_DOC_REPLY_MARKER)
                && !repaired.contains(LEGACY_WAITING_METADATA_MARKER),
            "{repaired}"
        );
        assert!(!repaired.contains("~~**Response:"), "{repaired}");
        assert!(!repaired.contains("~~**Question:"), "{repaired}");
    }

    #[test]
    fn waiting_tree_targets_main_among_multiple_queue_components() {
        let response = "### Re: approval — test\n\nWaiting for approval.";
        let document = format!(
            "<!-- agent:queue id=\"other\" -->\n- ### other\n<!-- /agent:queue -->\n{}",
            waiting_tree_document(
                "- ### main work\n",
                "- [ ] [#approve-pr] Approve and merge PR #42.\n",
                response,
            )
        );
        let projected = project_waiting_on_you_tree(&document, response)
            .unwrap()
            .unwrap();
        let (before_main, after_main) = projected
            .split_once("<!-- agent:queue id=\"main\" -->")
            .unwrap();
        assert!(!before_main.contains(WAITING_ON_YOU_ROOT_MARKER));
        assert!(after_main.contains(WAITING_ON_YOU_ROOT_MARKER));
    }

    fn inline_response_queue_doc(body: &str) -> String {
        format!("<!-- agent:queue response=\"inline\" -->\n{body}<!-- /agent:queue -->\n")
    }

    fn active_queue_doc(body: &str) -> String {
        format!("---\nqueue: go\n---\n\n<!-- agent:queue go -->\n{body}<!-- /agent:queue -->\n")
    }

    #[test]
    fn stale_response_detach_removes_only_generated_projections_under_struck_rows() {
        const STALE: &str = "[#r-0123456789ab]";
        const KEPT: &str = "[#r-abcdef012345]";
        let separator = agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR;
        let document = queue_doc(&format!(
            concat!(
                "- ~~first completed~~{separator}response: {stale}\n",
                "  - **Response {stale}**\n",
                "    - **Timestamp:** 2026-11-01T00:00:00.000Z\n",
                "- ~~second completed~~{separator}answered this cycle (#ftstrike); response: {stale}\n",
                "- ~~kept completed~~{separator}response: {kept}\n",
                "  - **Response {kept}**\n",
                "    - **Timestamp:** 2026-11-01T00:00:00.000Z\n",
                "    - **Summary:** discusses {stale} as plain prose\n",
                "- open work\n",
                "  - **Response {stale}**\n",
                "    - **Timestamp:** 2026-11-01T00:00:00.000Z\n",
                "- discuss ~~old~~ syntax{separator}response: {stale}\n",
            ),
            separator = separator,
            stale = STALE,
            kept = KEPT,
        ));

        let detached = detach_struck_queue_response_projections(
            &document,
            &BTreeSet::from([STALE.to_string()]),
        )
        .unwrap();

        assert!(detached.contains("- ~~first completed~~\n"), "{detached}");
        assert!(
            detached.contains(&format!(
                "- ~~second completed~~{separator}answered this cycle (#ftstrike)\n"
            )),
            "{detached}"
        );
        assert!(
            detached.contains(&format!("response: {KEPT}")),
            "{detached}"
        );
        assert!(
            detached.contains(&format!("- open work\n  - **Response {STALE}**")),
            "an open row must remain byte-identical:\n{detached}"
        );
        assert!(
            detached.contains(&format!(
                "- discuss ~~old~~ syntax{separator}response: {STALE}\n"
            )),
            "inline strikethrough is not a completed queue row:\n{detached}"
        );
        assert_eq!(
            detached.matches(STALE).count(),
            3,
            "only valid response prose and the two open-row forms retain the id:\n{detached}"
        );
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

        assert!(annotated.contains("- ~~do [#fresh]~~\n"), "{annotated}");
        assert!(!annotated.contains(&format!("response: {response_id}")));
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

    fn response_summary_of_row(annotated: &str, row: &str) -> String {
        let start = annotated
            .find(row)
            .unwrap_or_else(|| panic!("missing row {row}:\n{annotated}"));
        let rest = &annotated[start + row.len()..];
        let end = rest.find("\n- ").unwrap_or(rest.len());
        let tree = &rest[..end];
        tree.lines()
            .find_map(|line| line.trim().strip_prefix("- **Summary:** "))
            .unwrap_or_else(|| panic!("row {row} has no Summary child:\n{tree}"))
            .to_string()
    }

    #[test]
    fn single_item_summary_is_the_bullet_naming_the_struck_row_not_the_first_paragraph() {
        // `#responsesummarypick`: the live `do [#dispatchpanetimeout]` closeout.
        // The response first answered a chat prompt, then listed what the turn
        // did with the queue, then echoed a different, still-live row.
        let response = concat!(
            "### Re: backlog → agent:run, and the queue — test\n\n",
            "**They are in it now.** This turn's preflight mirrored every open `agent:backlog` item into `agent:run id=\"main\"` as a 📥 `do [#…]` row: 26 of 26.\n\n",
            "**What this turn did with the queue:**\n\n",
            "- `do [#dispatchpanetimeout]` is **done**: `f6a6c53eae` is on main, pushed and installed, and `make check` passed.\n",
            "- `do [#reportedcloseoutdeadend]` is now claimed and bound to the worker already fixing it.\n\n",
            "> **Queue prompt:** Run the remote smoke test on a real Coder workspace.\n\n",
            "**Deferred:** this waits for you to provide a real Coder workspace.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- ### do [#dispatchpanetimeout]\n- ### 🚧 do [#reportedcloseoutdeadend]\n- ### Run the remote smoke test on a real Coder workspace.\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ### ~~do [#dispatchpanetimeout]~~\n- ### 🚧 do [#reportedcloseoutdeadend]\n- ### Run the remote smoke test on a real Coder workspace.\n",
            response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let summary = response_summary_of_row(&annotated, "- ### ~~do [#dispatchpanetimeout]~~");
        assert_eq!(
            summary,
            "`do [#dispatchpanetimeout]` is **done**: `f6a6c53eae` is on main, pushed and installed, and `make check` passed.",
            "{annotated}"
        );
        assert!(!summary.contains("They are in it now"), "{annotated}");
    }

    #[test]
    fn multi_item_summaries_each_name_their_own_row() {
        // Two independently addressed sections, each led by unrelated prose:
        // every struck row gets the text naming it, never the section lead or
        // its sibling's text.
        let response = concat!(
            "### Re: alpha — test\n\n",
            "> **Queue prompt:** do [#alpha]\n\n",
            "The chat question about install order is answered below.\n\n",
            "- `do [#alpha]` is done: alpha landed as `aaaaaaa`.\n\n",
            "### Re: beta — test\n\n",
            "> **Queue prompt:** do [#beta]\n\n",
            "Unrelated: the dogfood watch is quiet.\n\n",
            "- `do [#beta]` is done: beta landed as `bbbbbbb`.\n",
        );
        let before = doc_with_queue_and_exchange("- do [#alpha]\n- do [#beta]\n", "");
        let after = doc_with_queue_and_exchange("- ~~do [#alpha]~~\n- ~~do [#beta]~~\n", response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        assert_eq!(
            response_summary_of_row(&annotated, "- ~~do [#alpha]~~"),
            "`do [#alpha]` is done: alpha landed as `aaaaaaa`.",
            "{annotated}"
        );
        assert_eq!(
            response_summary_of_row(&annotated, "- ~~do [#beta]~~"),
            "`do [#beta]` is done: beta landed as `bbbbbbb`.",
            "{annotated}"
        );

        // One response heading closing both rows: the canonical owner's summary
        // is its own bullet, not the shared lead and not the sibling bullet;
        // the sibling keeps its inert back-reference.
        let response = concat!(
            "### Re: batch — test\n\n",
            "The chat question about install order is answered: install runs last.\n\n",
            "- `do [#beta]` is done: beta landed as `bbbbbbb`.\n",
            "- `do [#alpha]` is done: alpha landed as `aaaaaaa`.\n",
        );
        let after = doc_with_queue_and_exchange("- ~~do [#alpha]~~\n- ~~do [#beta]~~\n", response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        assert_eq!(
            response_summary_of_row(&annotated, "- ~~do [#alpha]~~"),
            "`do [#alpha]` is done: alpha landed as `aaaaaaa`.",
            "{annotated}"
        );
        assert!(
            !annotated.contains("**Summary:** The chat question"),
            "{annotated}"
        );
        assert_eq!(
            annotated.matches("- **Closed by:** [#r-").count(),
            1,
            "{annotated}"
        );
    }

    #[test]
    fn summary_falls_back_to_the_first_paragraph_when_nothing_names_the_row() {
        // The prompt echo names the row but describes no outcome, and the
        // `#gammaray` mention is a different id sharing a prefix.
        let response = concat!(
            "### Re: gamma — test\n\n",
            "> **Queue prompt:** do [#gamma]\n\n",
            "Gamma's repair is merged and verified.\n\n",
            "- `#gammaray` stays open for later.\n",
        );
        let annotated = annotate_newly_struck_queue_items_with_response_id(
            &queue_doc("- do [#gamma]\n"),
            &queue_doc("- ~~do [#gamma]~~\n"),
            response,
        )
        .unwrap();
        assert_eq!(
            response_summary_of_row(&annotated, "- ~~do [#gamma]~~"),
            "Gamma's repair is merged and verified.",
            "{annotated}"
        );
        assert_eq!(
            response_summary_for_queue_item(response, "do [#gamma]"),
            None,
            "a blockquote echo or a prefix-sharing id never names the row"
        );
        assert_eq!(
            response_summary_for_queue_item(
                "Intro.\n\n- Finished: run the integration suite on staging now.\n",
                "run the integration suite on staging now"
            )
            .as_deref(),
            Some("Finished: run the integration suite on staging now."),
            "a free-text row without an id is named by a quote of its work text"
        );
    }

    #[test]
    fn response_child_removes_duplicate_inline_response_note() {
        let response = "### Re: fresh — test\n\nCompleted.\n";
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();
        let before = queue_doc("- do [#fresh]\n");
        let after = queue_doc(&format!(
            "- ~~do [#fresh]~~ — auto-struck: answered this cycle (#ftstrike); response: {response_id}\n"
        ));

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();

        assert!(annotated.contains(&format!("  - **Response {response_id}**\n")));
        assert!(annotated.contains("    - **Summary:** Completed.\n"));
        assert_eq!(
            annotated
                .matches(&format!("**Response {response_id}**"))
                .count(),
            1
        );
        assert!(!annotated.contains(&format!("response: {response_id}")));
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
                "- ~~Investigate alpha [#q-alpha]~~\n  - **Response {}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - **Summary:** Alpha is fixed and verified.\n",
                ids[0]
            )),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!(
                "- ~~Investigate beta [#q-beta]~~\n  - **Response {}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - **Summary:** Beta needs the operator to choose a deployment target.",
                ids[1]
            )),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!("### Re: alpha — test {}\n", ids[0])),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!("### Re: beta — test {}\n", ids[1])),
            "{annotated}"
        );
        assert!(!annotated.contains("auto-struck: response:"), "{annotated}");
        assert_eq!(
            annotated
                .matches(&format!("**Response {}**", ids[0]))
                .count(),
            1,
            "the queue response parent remains while the exchange marker moves to its heading"
        );
        assert_eq!(
            annotated
                .matches(&format!("**Response {}**", ids[1]))
                .count(),
            1,
            "the queue response parent remains while the exchange marker moves to its heading"
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
    fn one_heading_batch_has_one_canonical_response_and_row_backlinks() {
        let response = concat!(
            "### Re: batch closeout — test\n\n",
            "> **Queue prompt:** Publish alpha [#q-alpha] ⟦published⟧\n\n",
            "**Outcome:** Alpha shipped and its pull request is merged.\n\n",
            "> **Queue prompt:** Fix beta [#q-beta] ⟦published⟧\n\n",
            "**Current state:** Beta is fixed and published.\n",
            "**Next action:** Verify beta in production.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- Publish alpha [#q-alpha] ⟦published⟧\n- Fix beta [#q-beta] ⟦published⟧\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ~~Publish alpha [#q-alpha] ⟦published⟧~~\n- ~~Fix beta [#q-beta] ⟦published⟧~~\n",
            response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);
        let alpha_start = queue.find("- ~~Publish alpha").unwrap();
        let beta_start = queue.find("- ~~Fix beta").unwrap();
        let alpha = &queue[alpha_start..beta_start];
        let beta = &queue[beta_start..];

        assert!(
            alpha.contains("**Summary:** **Outcome:** Alpha shipped"),
            "{alpha}"
        );
        assert!(
            alpha.contains(
                "**Guidance:** More work remains. Keep this tree and follow the next actions below."
            ),
            "{alpha}"
        );
        assert!(
            alpha.contains("**Action:** Verify beta in production."),
            "the canonical node retains material from the complete response turn: {alpha}"
        );
        assert!(beta.contains("**Closed by:** [#r-"), "{beta}");
        assert!(!beta.contains("**Summary:**"), "{beta}");
        assert!(!beta.contains("**Guidance:**"), "{beta}");
        assert_eq!(queue.matches("- **Response [#r-").count(), 1, "{queue}");
        assert_eq!(queue.matches("- **Closed by:** [#r-").count(), 1, "{queue}");
        let entries = document_queue::parse(queue).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "generated state and guidance must stay inert:\n{entries:#?}\n{queue}"
        );
    }

    #[test]
    fn decision_sections_project_every_action_into_the_active_queue_tree() {
        let response = concat!(
            "### Re: ADR next steps — test\n\n",
            "> **Queue prompt:** Tell me the next actionable steps [#q-adr]\n\n",
            "**Next action:** The owner provides or defers the decisions below.\n\n",
            "#### 1. Permission copy\n\n",
            "- Decision: name the online-play permission and agree its wording.\n",
            "- Owner: Product.\n",
            "- Next action: Product writes the final wording, or defers the workshop.\n\n",
            "#### 2. Session channel\n\n",
            "- Decision: accept `channel_type = 'session'` beside ADR 0040.\n",
            "- Owner: API owner.\n",
            "- Next action: the API owner records the decision, or defers the spec.\n\n",
            "#### 3. Existing decisions\n\n",
            "- Next action: record the two decisions when an edit is authorized.\n",
            "\n#### 4. Deferred decision\n\n",
            "- Decision: choose whether to defer the retention workshop.\n",
        );
        let before =
            doc_with_queue_and_exchange("- Tell me the next actionable steps [#q-adr]\n", "");
        let after = doc_with_queue_and_exchange(
            "- ~~Tell me the next actionable steps [#q-adr]~~\n",
            response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);

        assert!(
            queue.contains(
                "- **Next actions:**\n      - **Decision:** name the online-play permission and agree its wording.\n        - **Action:** Product writes the final wording, or defers the workshop."
            ),
            "{queue}"
        );
        assert!(
            queue.contains(
                "      - **Decision:** accept `channel_type = 'session'` beside ADR 0040.\n        - **Action:** the API owner records the decision, or defers the spec."
            ),
            "{queue}"
        );
        assert!(
            queue.contains(
                "      - **Action:** record the two decisions when an edit is authorized."
            ),
            "{queue}"
        );
        assert!(
            queue.contains("      - **Decision:** choose whether to defer the retention workshop."),
            "a decision without an action still needs its own visible subitem:\n{queue}"
        );
        assert!(
            !queue.contains("**Action:** The owner provides or defers the decisions below."),
            "the generic lead must not replace the concrete actions:\n{queue}"
        );
        let entries = document_queue::parse(queue).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "generated decision/action descendants must stay inert:\n{entries:#?}\n{queue}"
        );
    }

    #[test]
    fn uncommitted_uninstalled_outcome_cannot_terminalize_the_queue_row() {
        let response = concat!(
            "### Re: state drift repair — test\n\n",
            "> **Queue prompt:** do [#adadvprstatedrift]\n\n",
            "**Current state:** The patch remains uncommitted and uninstalled.\n",
            "**Worktree:** `/worktrees/adadvprstatedrift`\n",
            "**Branch:** `fix/adadvprstatedrift`\n",
        );
        let before = queue_doc("- do [#adadvprstatedrift]\n");
        let after = queue_doc("- ~~do [#adadvprstatedrift]~~\n");

        let error = annotate_newly_struck_queue_items_with_response_id(&before, &after, response)
            .expect_err("a dirty isolated worktree is not terminal completion");
        let error = error.to_string();
        assert!(
            error.contains("Keep the row live until it has a scoped commit"),
            "{error}"
        );
        assert!(error.contains("blocking operator request"), "{error}");
        assert!(
            error.contains("PR or named integration-branch receipt"),
            "{error}"
        );
    }

    #[test]
    fn canonical_duplicate_rows_share_one_response_lifecycle_projection() {
        let response = concat!(
            "### Re: canonical item — test\n\n",
            "> **Queue prompt:** do [#canonical-work]\n\n",
            "**Outcome:** The canonical work is complete and verified.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- ### docs\n  - do [#canonical-work]\n- ### main\n  - do [#canonical-work]\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ### docs\n  - ~~do [#canonical-work]~~\n- ### main\n  - ~~do [#canonical-work]~~\n",
            response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        assert_eq!(
            annotated
                .matches("**Summary:** **Outcome:** The canonical work is complete and verified.")
                .count(),
            1,
            "the response must have one canonical projection:\n{annotated}"
        );
        assert_eq!(
            annotated
                .matches("**Guidance:** Safe to delete the whole tree under `docs`")
                .count(),
            1,
            "canonical guidance must not be duplicated:\n{annotated}"
        );
        assert_eq!(
            annotated.matches("- **Closed by:** [#r-").count(),
            1,
            "the duplicate row must point to the canonical response:\n{annotated}"
        );
    }

    /// `#guidancescope`: render the Guidance line a complete response gives
    /// the row `closed` once `before_queue` becomes `after_queue`.
    fn completed_row_guidance(before_queue: &str, after_queue: &str, closed: &str) -> String {
        let response = format!(
            "### Re: {closed} — test\n\n> **Queue prompt:** do [#{closed}]\n\n**Outcome:** The {closed} work is complete and verified.\n"
        );
        let before = doc_with_queue_and_exchange(before_queue, "");
        let after = doc_with_queue_and_exchange(after_queue, &response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let guidance = annotated
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("- **Guidance:** "))
            .collect::<Vec<_>>();
        assert_eq!(guidance.len(), 1, "{annotated}");
        guidance[0].to_string()
    }

    #[test]
    fn guidance_scopes_a_lone_top_level_row_to_the_row() {
        let guidance = completed_row_guidance(
            "- do [#solo]\n- do [#other]\n",
            "- ~~do [#solo]~~\n- do [#other]\n",
            "solo",
        );
        assert_eq!(
            guidance,
            "Safe to delete this completed row; the item is complete and has no remaining tracked or unpublished work."
        );
    }

    #[test]
    fn guidance_counts_the_struck_rows_nested_beneath_a_top_level_row() {
        let guidance = completed_row_guidance(
            "- do [#parent]\n  - ~~step one~~\n  - ~~step two~~\n",
            "- ~~do [#parent]~~\n  - ~~step one~~\n  - ~~step two~~\n",
            "parent",
        );
        assert!(
            guidance.starts_with("Safe to delete this row and its 2 nested rows;"),
            "{guidance}"
        );
    }

    #[test]
    fn guidance_names_the_open_nested_row_that_keeps_the_subtree_alive() {
        let guidance = completed_row_guidance(
            "- do [#parent]\n  - ~~do [#first]~~\n  - do [#second]\n",
            "- ~~do [#parent]~~\n  - ~~do [#first]~~\n  - do [#second]\n",
            "parent",
        );
        assert!(
            guidance.starts_with("More work remains beneath this row."),
            "{guidance}"
        );
        assert!(
            guidance.contains("keep this row and its 2 nested rows: [#second] is still open."),
            "{guidance}"
        );
        assert!(!guidance.contains("Safe to delete"), "{guidance}");
    }

    #[test]
    fn guidance_keeps_the_tree_alive_while_a_sibling_is_open() {
        let guidance = completed_row_guidance(
            "- ### epic\n  - do [#done-child]\n  - do [#open-child]\n",
            "- ### epic\n  - ~~do [#done-child]~~\n  - do [#open-child]\n",
            "done-child",
        );
        assert_eq!(
            guidance,
            "Safe to delete this completed row; the item is complete and has no remaining tracked or unpublished work. Keep the tree under `epic`: [#open-child] is still open."
        );
    }

    #[test]
    fn guidance_offers_the_whole_tree_only_when_every_row_in_it_is_struck() {
        let guidance = completed_row_guidance(
            "- ### epic\n  - ~~do [#earlier]~~\n  - do [#last]\n    - ~~step~~\n",
            "- ### epic\n  - ~~do [#earlier]~~\n  - ~~do [#last]~~\n    - ~~step~~\n",
            "last",
        );
        assert_eq!(
            guidance,
            "Safe to delete the whole tree under `epic`; every row in it is struck and the item is complete and has no remaining tracked or unpublished work."
        );
    }

    #[test]
    fn replaying_a_projection_after_the_tree_changed_never_stacks_guidance() {
        let response = "### Re: replay — test\n\n> **Queue prompt:** do [#done-child]\n\n**Outcome:** The work is complete and verified.\n";
        let before = doc_with_queue_and_exchange(
            "- ### epic\n  - do [#done-child]\n  - do [#open-child]\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ### epic\n  - ~~do [#done-child]~~\n  - do [#open-child]\n",
            response,
        );
        let first =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        assert!(first.contains("[#open-child] is still open"), "{first}");
        // The sibling closes before a detached replay of the same response.
        let changed = first.replace("  - do [#open-child]", "  - ~~do [#open-child]~~");
        let replayed =
            annotate_newly_struck_queue_items_with_response_id(&before, &changed, response)
                .unwrap();
        assert_eq!(
            replayed.matches("**Guidance:**").count(),
            1,
            "a replay keeps the existing projection:\n{replayed}"
        );
    }

    #[test]
    fn guidance_tree_scope_ignores_rows_of_other_top_level_trees() {
        let guidance = completed_row_guidance(
            "- ### epic\n  - do [#only]\n- ### other\n  - do [#elsewhere]\n",
            "- ### epic\n  - ~~do [#only]~~\n- ### other\n  - do [#elsewhere]\n",
            "only",
        );
        assert!(
            guidance.starts_with("Safe to delete the whole tree under `epic`;"),
            "{guidance}"
        );
    }

    #[test]
    fn historical_duplicate_response_migration_preserves_operator_descendants() {
        const RESPONSE: &str = "[#r-0123456789ab]";
        const NESTED: &str = "[#r-abcdef012345]";
        let content = queue_doc(&format!(
            concat!(
                "- ~~canonical~~ — auto-struck: response: {0}\n",
                "  - **Response {0}**\n",
                "    - **Timestamp:** 2026-11-01T12:00:00Z\n",
                "    - **Summary:** Batch completed.\n",
                "- ~~duplicate~~ — auto-struck: response: {0}\n",
                "  - **Response {0}**\n",
                "    - **Timestamp:** 2026-11-01T12:00:00Z\n",
                "    - **Summary:** Batch completed.\n",
                "    - **Choices:**\n",
                "      - **publish**: Publish it.\n",
                "        - Operator chose staging; preserve this reply exactly.\n",
                "    - ~~nested completion~~ — auto-struck: response: {1}\n",
                "      - **Response {1}**\n",
                "        - **Timestamp:** 2026-11-01T12:01:00Z\n",
                "        - **Summary:** Nested work completed.\n",
            ),
            RESPONSE, NESTED
        ));

        let repaired = canonicalize_duplicate_queue_response_projections(&content).unwrap();
        assert_eq!(
            repaired
                .matches(&format!("- **Response {RESPONSE}**"))
                .count(),
            1,
            "one canonical response must remain:\n{repaired}"
        );
        assert_eq!(
            repaired
                .matches(&format!("- **Closed by:** {RESPONSE}"))
                .count(),
            1,
            "the duplicate becomes one inert backlink:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("Batch completed.").count(),
            1,
            "{repaired}"
        );
        assert_eq!(
            repaired.matches("2026-11-01T12:00:00Z").count(),
            1,
            "{repaired}"
        );
        assert!(
            repaired.contains("        - Operator chose staging; preserve this reply exactly.\n"),
            "operator-authored descendants stay byte-identical:\n{repaired}"
        );
        assert!(
            repaired.contains("    - ~~nested completion~~ — auto-struck:"),
            "an operator-owned nested row must survive:\n{repaired}"
        );
        assert_eq!(
            repaired
                .matches(&format!("- **Response {NESTED}**"))
                .count(),
            1,
            "a distinct nested response must remain canonical:\n{repaired}"
        );
        assert_eq!(
            canonicalize_duplicate_queue_response_projections(&repaired).unwrap(),
            repaired,
            "the historical migration must be idempotent"
        );
        let queue = element::parse(&repaired)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let entries = document_queue::parse(queue.content(&repaired)).unwrap();
        assert!(
            document_queue::prompts(&entries)
                .iter()
                .any(|prompt| prompt.text.contains("Operator chose staging")),
            "preserved operator descendants remain actionable:\n{entries:#?}\n{repaired}"
        );
    }

    #[test]
    fn historical_duplicate_response_migration_is_document_wide() {
        const RESPONSE: &str = "[#r-0123456789ab]";
        let content = format!(
            concat!(
                "<!-- agent:queue id=docs -->\n",
                "- ~~docs~~\n",
                "  - **Response {0}**\n",
                "    - **Timestamp:** first\n",
                "<!-- /agent:queue -->\n",
                "<!-- agent:queue id=main -->\n",
                "- ~~main~~\n",
                "  - **Response {0}**\n",
                "    - **Timestamp:** second\n",
                "- ~~main duplicate~~\n",
                "  - **Response {0}**\n",
                "    - **Timestamp:** third\n",
                "<!-- /agent:queue -->\n",
            ),
            RESPONSE
        );

        let repaired = canonicalize_duplicate_queue_response_projections(&content).unwrap();
        assert_eq!(
            repaired
                .matches(&format!("**Response {RESPONSE}**"))
                .count(),
            1
        );
        assert_eq!(
            repaired
                .matches(&format!("**Closed by:** {RESPONSE}"))
                .count(),
            2
        );
        assert!(repaired.contains("**Timestamp:** first"), "{repaired}");
        assert!(!repaired.contains("**Timestamp:** second"), "{repaired}");
        assert!(!repaired.contains("**Timestamp:** third"), "{repaired}");
    }

    #[test]
    fn one_effect_echo_cannot_close_a_distinct_effect_row() {
        let response = concat!(
            "### Re: publish effect one — test\n\n",
            "> **Queue prompt:** release + publish ⟦effect:1⟧\n\n",
            "**Outcome:** Effect one is published.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- release + publish ⟦effect:1⟧\n- release + publish ⟦effect:2⟧\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ~~release + publish ⟦effect:1⟧~~\n- ~~release + publish ⟦effect:2⟧~~\n",
            response,
        );

        let error = annotate_newly_struck_queue_items_with_response_id(&before, &after, response)
            .expect_err("effect:2 is a separate stable effect and was not answered");
        assert!(
            error
                .to_string()
                .contains("newly completed queue prompt has no unique response section"),
            "{error:#}"
        );
    }

    #[test]
    fn five_head_inline_closeout_keeps_each_response_snippet_and_artifact_with_its_head() {
        let cases = [
            ("alpha", "111111111111", "artifacts/alpha.json"),
            ("beta", "222222222222", "artifacts/beta.json"),
            ("gamma", "333333333333", "artifacts/gamma.json"),
            ("delta", "444444444444", "artifacts/delta.json"),
            ("epsilon", "555555555555", "artifacts/epsilon.json"),
        ];
        let mut response = String::new();
        let mut before_queue = String::new();
        let mut after_queue = String::new();
        for (name, response_hash, artifact) in cases {
            response.push_str(&format!(
                "### Re: {name} — test\n\n\
                 > **Queue prompt:** Investigate {name} [#q-{name}]\n\n\
                 **Outcome:** {name} is fixed and verified.\n\n\
                 ### Artifacts\n\n\
                 %% response [#r-{response_hash}] %%\n\
                 - Artifact: `{artifact}`.\n\n"
            ));
            before_queue.push_str(&format!("- Investigate {name} [#q-{name}]\n"));
            after_queue.push_str(&format!("- ~~Investigate {name} [#q-{name}]~~\n"));
        }
        let before = inline_response_queue_doc(&before_queue);
        let after = inline_response_queue_doc(&after_queue);

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);

        for (index, (name, response_hash, artifact)) in cases.iter().enumerate() {
            let row = format!("- ~~Investigate {name} [#q-{name}]~~");
            let start = queue.find(&row).unwrap();
            let end = cases
                .get(index + 1)
                .and_then(|(next, _, _)| {
                    queue[start + row.len()..]
                        .find(&format!("- ~~Investigate {next} [#q-{next}]~~"))
                        .map(|offset| start + row.len() + offset)
                })
                .unwrap_or(queue.len());
            let owned_projection = &queue[start..end];

            assert!(
                owned_projection.contains(&format!("**Outcome:** {name} is fixed and verified.")),
                "missing own summary for {name}:\n{owned_projection}"
            );
            assert!(
                owned_projection.contains(&format!("%% response [#r-{response_hash}] %%")),
                "missing own response snippet for {name}:\n{owned_projection}"
            );
            assert!(
                owned_projection.contains(artifact),
                "missing own artifact for {name}:\n{owned_projection}"
            );
            for (other, other_hash, other_artifact) in cases {
                if other == *name {
                    continue;
                }
                assert!(
                    !owned_projection.contains(&format!("%% response [#r-{other_hash}] %%"))
                        && !owned_projection.contains(other_artifact),
                    "{name} inherited {other}'s response evidence:\n{owned_projection}"
                );
            }
        }

        let entries = document_queue::parse(queue).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "inline response descendants must remain inert:\n{entries:#?}\n{queue}"
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
            annotated.contains(&format!("~~{todo}~~\n    - **Response {}**", ids[0])),
            "{annotated}"
        );
        assert!(
            annotated.contains(&format!("~~{PER_ITEM}~~\n    - **Response {}**", ids[2])),
            "the old sdk-dev lineage must bind the reparented row to its own section: {annotated}"
        );
        assert!(!annotated.contains(&format!("**Response {aggregate}**")));
        assert!(!annotated.contains("auto-struck: response:"), "{annotated}");
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
    fn each_addressed_response_heading_carries_its_own_response_id() {
        let prompts = [
            "lossless lost-work reconciliation [#q-111111111111]",
            "explicit queue acknowledgement guardrails [#q-222222222222]",
            "integration, verification, and installation [#q-333333333333]",
        ];
        let before = doc_with_queue_and_exchange(
            &prompts
                .iter()
                .map(|prompt| format!("- {prompt}\n"))
                .collect::<String>(),
            "",
        );
        let response = prompts
            .iter()
            .map(|prompt| {
                let topic = prompt
                    .split_once(" [#q-")
                    .map(|(topic, _)| topic)
                    .unwrap();
                format!(
                    "> **Queue prompt:** {prompt}\n\n### Re: {topic} — gpt-5 · 2026-10-31T17:18-04:00\n\nCompleted {topic}.\n\n"
                )
            })
            .collect::<String>();
        let after = doc_with_queue_and_exchange(
            &prompts
                .iter()
                .map(|prompt| format!("- ~~{prompt}~~\n"))
                .collect::<String>(),
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

        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), prompts.len());
        for ((prompt, node), response_id) in prompts.iter().zip(&sections).zip(&ids) {
            let topic = prompt.split_once(" [#q-").map(|(topic, _)| topic).unwrap();
            assert!(
                annotated.contains(&format!(
                    "### Re: {topic} — gpt-5 · 2026-10-31T17:18-04:00 {response_id}"
                )),
                "the section must expose the id of its own response cell: {annotated}"
            );
            assert!(
                annotated.contains(&format!("~~{prompt}~~\n  - **Response {response_id}**")),
                "the completed prompt and its heading must share one id: {annotated}"
            );
            assert_eq!(
                agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response_nodes(
                    std::slice::from_ref(node),
                )
                .response_id(),
                response_id
            );
        }
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

        assert!(annotated.contains("- ~~do [#subagent-response-exchange-gap]~~\n"));
        assert!(!annotated.contains("auto-struck: response:"), "{annotated}");
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
- *queue_item:*\n  - do [#release]\n\
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

        let annotated = annotate_newly_struck_queue_items_with_response_id_at(
            &before,
            &after,
            &response,
            "2026-11-02T05:05:08Z",
        )
        .unwrap();
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
            annotate_newly_struck_queue_items_with_response_id_at(
                &before,
                &annotated,
                &response,
                "2026-11-02T05:05:09Z",
            )
            .unwrap(),
            annotated,
            "replaying the durable request evidence across a timestamp boundary must not duplicate its subtree"
        );

        let distinct_response = response.replace(
            "publication needs an operator choice.",
            "publication still needs an operator choice.",
        );
        let distinct_after = doc_with_queue_and_exchange(queue_body, &distinct_response);
        let distinct = annotate_newly_struck_queue_items_with_response_id_at(
            &before,
            &distinct_after,
            &distinct_response,
            "2026-11-02T05:05:10Z",
        )
        .unwrap();
        assert_eq!(
            distinct.matches("- **Response [#r-").count(),
            2,
            "a genuinely different response keeps its distinct identity: {distinct}"
        );
    }

    #[test]
    fn multiple_operator_requests_project_only_beside_their_own_waiting_rows() {
        let craig_item = "publish Craig fixture [#q-craig]";
        let dillon_item = "publish Dillon fixture [#q-dillon]";
        let craig_request = "subagent-operator-request-craig-access";
        let dillon_request = "subagent-operator-request-dillon-followup";
        let response = format!(
            "### Re: fixture access choices — test\n\n\
**Current state:** Both fixture changes are ready, but each needs its own operator decision.\n\n\
[#{craig_request}] subagent operator request\n\
- *queue_item:*\n  - {craig_item}\n\
- *kind:* permission\n\
- *blocking:* true\n\
- *request:*\n  - Grant repository access for the Craig fixture?\n\
- *choices:*\n  - **Grant both**: Run both PUTs.\n  - **Do not grant**: Leave access unchanged.\n\n\
[#{dillon_request}] subagent operator request\n\
- *queue_item:*\n  - {dillon_item}\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n  - How should Dillon be added?\n\
- *choices:*\n  - **Add Dillon by follow-up PR**: Open a separate PR.\n  - **Keep current PR only**: Do not add Dillon.\n"
        );
        let before = queue_doc(&format!("- {craig_item}\n- {dillon_item}\n"));
        let after = doc_with_queue_and_exchange(
            &format!(
                "- 🙋 [#{craig_request}] {craig_item}\n- 🙋 [#{dillon_request}] {dillon_item}\n"
            ),
            &response,
        );

        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue_body = queue.content(&annotated);
        let craig_start = queue_body
            .find(&format!("- 🙋 [#{craig_request}]"))
            .unwrap();
        let dillon_start = queue_body
            .find(&format!("- 🙋 [#{dillon_request}]"))
            .unwrap();
        let craig_scope = &queue_body[craig_start..dillon_start];
        let dillon_scope = &queue_body[dillon_start..];

        assert!(
            craig_scope.contains("**Grant both**: Run both PUTs."),
            "{queue_body}"
        );
        assert!(!craig_scope.contains("Add Dillon by follow-up PR"));
        assert!(dillon_scope.contains("**Add Dillon by follow-up PR**: Open a separate PR."));
        assert!(!dillon_scope.contains("**Grant both**"));
        assert!(!queue_body.contains("**Reference**:"), "{queue_body}");
        let entries = document_queue::parse(queue_body).unwrap();
        assert!(
            document_queue::prompts(&entries).is_empty(),
            "offered choices and waiting rows are not dispatchable queue work:\n{entries:#?}"
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
    fn shared_operator_request_projects_one_row_scoped_choice_tree() {
        let response = concat!(
            "### Re: aggregate repair — test\n\n",
            "> **Queue prompt:** repair alpha [#q-alpha]\n\n",
            "Alpha is repaired.\n\n",
            "> **Queue prompt:** repair beta [#q-beta]\n\n",
            "Beta is repaired.\n\n",
            "> **Queue prompt:** repair gamma [#q-gamma]\n\n",
            "Gamma is repaired.\n\n",
            "[#subagent-operator-request-shared-release] subagent operator request\n",
            "- *queue_item:*\n  - repair gamma [#q-gamma]\n",
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
            "one matching row must own the one canonical choice tree:\n{annotated}"
        );
        assert_eq!(
            annotated
                .matches("**Reference**: [#subagent-operator-request-shared-release]")
                .count(),
            0,
            "a row-scoped request must not appear under unrelated completed rows:\n{annotated}"
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
        let response_id =
            agent_doc_markdown_ast::exchange_tree::ResponseTurnCellPolicy::from_response(Some(
                response,
            ))
            .response_id()
            .to_string();
        assert_eq!(
            annotated
                .matches(&format!("- **Response {response_id}**"))
                .count(),
            1,
            "one response id must have one canonical response node:\n{annotated}"
        );
        assert_eq!(
            annotated
                .matches(&format!("- **Closed by:** {response_id}"))
                .count(),
            2,
            "the other completed rows must retain one-line backlinks:\n{annotated}"
        );
        assert_eq!(
            annotated.matches("- **Timestamp:**").count(),
            1,
            "canonical response metadata must not be copied:\n{annotated}"
        );
        assert_eq!(
            annotated.matches("- **Summary:**").count(),
            1,
            "canonical response metadata must not be copied:\n{annotated}"
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
                "  - ~~do [#nested]~~\n    - **Response {response_id}**\n      - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n      - <details data-agent-doc-response-id=\"{}\">\n        <summary>Response</summary>\n        \n        ### Re: nested\n        \n        Completed `tasks/nested.md`.\n        </details>\n",
                response_id.trim_start_matches("[#").trim_end_matches(']'),
            )),
            "{annotated}"
        );
        assert!(!annotated.contains(&format!("response: {response_id}")));
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
        assert_eq!(exchange.matches(response_id).count(), 2, "{exchange}");
        assert!(
            exchange.contains(&format!("### Re: nested {response_id}")),
            "{exchange}"
        );
        assert!(
            exchange.contains(&format!("### Verification {response_id}")),
            "{exchange}"
        );
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
            !queue.contains(&format!("response: {response_id}")),
            "{queue}"
        );
        assert!(exchange.contains(response_id) && queue.contains(response_id));
        assert!(!annotated.contains(policy.cell_id()), "{annotated}");
    }

    #[test]
    fn answered_free_text_projection_uses_only_its_response_child() {
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
                .contains(&format!("  - **Response {response_id}**\n"))
        );
        assert!(
            projected
                .target_content
                .contains("    - **Summary:** The completed prompt now links to this response.")
        );
        assert!(
            !projected
                .target_content
                .contains(&format!("response: {response_id}"))
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

    /// `#goalpresetterminal` project preset: a `/goal` review loop declared only
    /// in `.agent-doc/project.md` (installed through the IO guard), whose body
    /// declares its final states.
    const GOAL_LOOP_PRESET: &str = concat!(
        "---\npresets:\n",
        "  '#adversarial-pr': >-\n",
        "    /goal Bring the pull request named by this queue item to an\n",
        "    independently review-clean exact remote head. Each cycle: 1. Review.\n",
        "    Start a fresh independent reviewer against the exact current head.\n",
        "    2. Decide. 3. Address. Start a separate fixer using #address.\n",
        "    4. Fetch the newly published remote head and start the next cycle.\n\n",
        "    Final states. Your final message must begin with exactly one of these\n",
        "    lines:\n",
        "    - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`.\n",
        "    Use it only when a fresh posted review on the latest unchanged remote\n",
        "    head has zero surviving findings.\n",
        "    - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`,\n",
        "    followed by the exact review URLs.\n\n",
        "    Coordinator: a hand-back that does not begin with REVIEW-CLEAN or\n",
        "    BLOCKED is an interrupted loop, not a completion.\n",
        "  '#address': 'Address every actionable review item on the exact current head.'\n",
        "---\n",
    );

    fn goal_loop_project_presets() -> agent_doc_frontmatter::frontmatter::PromptPresets {
        agent_doc_frontmatter::frontmatter::parse(GOAL_LOOP_PRESET)
            .unwrap()
            .0
            .prompt_presets
    }

    /// The session document of the regression: the preset is NOT in its
    /// frontmatter (it is project-inherited), and the goal row is a nested
    /// leaf under its pull request, claimed by a worker, beside a sibling pull
    /// request's identical leaf.
    fn goal_loop_session(row_12: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n\
             <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:run id=\"sample-prs\" subagents priority go -->\n\
             - https://github.com/sample-org/sample-repo/pull/12\n\
             \x20 - ~~#address ⟦effect:1⟧ ⟦published⟧~~\n\
             \x20 - {row_12}\n\
             - https://github.com/sample-org/sample-repo/pull/13\n\
             \x20 - 🚧 [#subagent:pr13-review1#1] #adversarial-pr\n\
             <!-- /agent:run -->\n"
        )
    }

    const PR12_ECHO: &str =
        "> **Queue prompt:** https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr";

    fn goal_loop_response(body: &str) -> String {
        format!(
            "### Re: sample-repo PR 12 goal loop\n\n\
             > **Queue prompt:**\n>\n> https://github.com/sample-org/sample-repo/pull/13: #adversarial-pr\n\n\n\
             {PR12_ECHO}\n\n{body}\n"
        )
    }

    fn goal_loop_row_strikes(content: &str, response: &str) -> bool {
        let claims = vec![
            "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr".to_string(),
            "#adversarial-pr".to_string(),
        ];
        let decision = answered_free_text_strike_decision(
            content,
            response,
            Some(content),
            &claims,
            &claims,
            &[],
        )
        .unwrap();
        !decision.node_keys.is_empty()
    }

    /// `#goalpresetterminal` regression (anonymized from a live incident): one
    /// posted review whose result is an actionable P2 struck a project-preset
    /// `/goal` review-loop row before its fixer ran. The binary now derives the
    /// preset's final states (`REVIEW-CLEAN`, `BLOCKED`) and closes the row only
    /// on a response line that begins with one, while the ordinary deferral
    /// guards still apply on top.
    #[test]
    fn goal_loop_preset_row_closes_only_on_a_declared_final_state() {
        let content = goal_loop_session("🚧 [#subagent:pr12-review1#1] #adversarial-pr");
        const LEAF: &str = "#adversarial-pr";
        // A paraphrase none of the nonterminal-review phrases catches.
        let review_with_p2 = goal_loop_response(
            "Posted one plain COMMENTED review at \
             https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000001 \
             against exact head `aaaa111`.\n\n\
             Verdict: changes needed. One P2 finding: reconnect drops the retained outbox.\n\n\
             The review effect is published.",
        );

        // Without the project presets installed the row reads as plain free
        // text, which is exactly how the paraphrased review struck it.
        assert!(
            goal_loop_row_strikes(&content, &review_with_p2),
            "precondition: the deny-list alone misses the paraphrased P2"
        );

        let _project = crate::goal_preset_terminal::InheritedPresetsGuard::install(
            &goal_loop_project_presets(),
        );

        // 1. A posted review with an actionable finding does not strike the row.
        assert!(!goal_loop_row_strikes(&content, &review_with_p2));
        assert!(crate::queue_response::response_defers_queue_head(
            &content,
            &review_with_p2,
            LEAF
        ));
        assert!(crate::queue_response::latest_queue_head_echo_is_deferral(
            &content,
            &review_with_p2,
            LEAF
        ));
        assert!(
            !crate::queue_heads::free_text_queue_head_is_completed_residue_with_claim(
                &content,
                &review_with_p2,
                LEAF,
                true
            ),
            "session-check must not demand a strike of the still-live loop row"
        );

        // 2. The interrupted attempt is requeued (no claim marker), and the
        // same unstruck row stays the dispatchable work: still not struck, and
        // the queue still holds exactly one PR 12 goal row.
        let requeued = goal_loop_session(LEAF);
        assert!(!goal_loop_row_strikes(&requeued, &review_with_p2));
        assert_eq!(
            requeued
                .lines()
                .filter(|line| line.trim() == "- #adversarial-pr")
                .count(),
            1
        );

        // 3. A published fix without a fresh independent review of the new
        // head is a step, not the result.
        let fix_published = goal_loop_response(
            "The fixer pushed `bbbb222` to the pull-request head branch and replied to the \
             review; all repository checks passed on the new head.",
        );
        assert!(!goal_loop_row_strikes(&content, &fix_published));
        let clean_word_in_prose = goal_loop_response(
            "A fresh review of `bbbb222` looks clean so far; the final re-query is pending.",
        );
        assert!(!goal_loop_row_strikes(&content, &clean_word_in_prose));

        // 4. Only a declared final state on the latest unchanged head closes it.
        let review_clean = goal_loop_response(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000002 \
             base=cccc333 head=bbbb222 cycles=2\n\n\
             A fresh independent review of the unchanged head has zero surviving findings.",
        );
        assert!(goal_loop_row_strikes(&content, &review_clean));
        assert!(!crate::queue_response::latest_queue_head_echo_is_deferral(
            &content,
            &review_clean,
            LEAF
        ));
        let blocked = goal_loop_response(
            "`BLOCKED restore the hosted runner budget base=cccc333 head=bbbb222 cycles=2`\n\n\
             Review URLs: https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000002",
        );
        assert!(goal_loop_row_strikes(&content, &blocked));

        // The ordinary deferral guards still veto a final-state line.
        let deferred_clean = goal_loop_response(
            "**Deferred:** waiting on the final remote-head re-query.\n\n\
             REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1000000002 \
             base=cccc333 head=bbbb222 cycles=2",
        );
        assert!(!goal_loop_row_strikes(&content, &deferred_clean));

        // The latest echo is the current statement: an older clean cell does
        // not close a row whose newest cell reports another actionable review.
        let accumulated = format!("{review_clean}\n{review_with_p2}");
        assert!(crate::queue_response::latest_queue_head_echo_is_deferral(
            &content,
            &accumulated,
            LEAF
        ));
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
                "- ### ~~#subagents do [#preflightdeadline]~~\n",
            )
            .replace(
                "- pick up steering\n",
                &format!("- ### ~~pick up steering~~{note}\n"),
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
        assert!(updated.contains("- ### ~~do [#alpha]~~\n"));
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

    /// `#queueprojectiondrift` (haiven-dev `tasks/cli.md`, 2026-11-01): five
    /// PR parents shared the same `#adversarial-pr` leaf. Exact REVIEW-CLEAN
    /// vector echoes close only pulls 10-12; vector deferrals keep 8-9. A later
    /// bare shared-leaf deferral must not veto the already-finished siblings.
    #[test]
    fn repeated_leaf_closeout_is_scoped_to_each_exact_parent_vector() {
        let _project = crate::goal_preset_terminal::InheritedPresetsGuard::install(
            &goal_loop_project_presets(),
        );
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:run subagents go -->\n",
            "- https://github.com/haiven-dev/haiven-cli/pull/8\n",
            "  - #adversarial-pr\n",
            "- https://github.com/haiven-dev/haiven-cli/pull/9\n",
            "  - #adversarial-pr\n",
            "- https://github.com/haiven-dev/haiven-cli/pull/10\n",
            "  - #adversarial-pr\n",
            "- https://github.com/haiven-dev/haiven-cli/pull/11\n",
            "  - #adversarial-pr\n",
            "- https://github.com/haiven-dev/haiven-cli/pull/12\n",
            "  - #adversarial-pr\n",
            "<!-- /agent:run -->\n",
        );
        let vectors = (8..=12)
            .map(|pull| {
                format!("https://github.com/haiven-dev/haiven-cli/pull/{pull}: #adversarial-pr")
            })
            .collect::<Vec<_>>();
        let response = concat!(
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-cli/pull/10: #adversarial-pr\n\n",
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-cli/pull/10#pullrequestreview-10 base=aaaa head=bbbb cycles=1\n\n",
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-cli/pull/11: #adversarial-pr\n\n",
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-cli/pull/11#pullrequestreview-11 base=aaaa head=bbbb cycles=1\n\n",
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-cli/pull/12: #adversarial-pr\n\n",
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-cli/pull/12#pullrequestreview-12 base=aaaa head=bbbb cycles=1\n\n",
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-cli/pull/8: #adversarial-pr\n\n",
            "**Deferred:** pull 8 changed after review.\n\n",
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-cli/pull/9: #adversarial-pr\n\n",
            "**Deferred:** pull 9 still needs a fresh review.\n\n",
            "> **Queue prompt:** #adversarial-pr\n\n",
            "**Deferred:** this shared leaf is not a substitute for a parent vector.\n",
        );

        let keys =
            answered_free_text_head_node_keys_with_claims(doc, response, Some(doc), &vectors)
                .unwrap();
        assert_eq!(keys.len(), 3, "only the three REVIEW-CLEAN vectors close");

        let projected =
            project_answered_free_text_strike_with_claims(doc, response, Some(doc), &vectors)
                .unwrap()
                .expect("the three finished vectors project strikes");
        assert_eq!(
            projected
                .target_content
                .matches("~~#adversarial-pr~~")
                .count(),
            3,
            "the shared bare deferral does not keep or close every sibling"
        );
        for pull in [8, 9] {
            let open = format!(
                "- https://github.com/haiven-dev/haiven-cli/pull/{pull}\n  - #adversarial-pr\n"
            );
            assert!(projected.target_content.contains(&open));
        }
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
            reconciled.contains("- ### ~~:pushpin: do [#pzjy]~~\n"),
            "pinned completed prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- ### ~~plain queued report~~\n"),
            "answered free-text prompt should stay struck:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- ### do [#new]\n"),
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
    fn response_echoes_capture_full_concrete_ancestry_and_keep_leaf_compatibility() {
        let entries = entries(concat!(
            "- Root context\n",
            "  - do [#parent]\n",
            "    - same leaf\n",
            "- Sibling root\n",
            "  - same leaf\n",
            "- top-level leaf\n",
        ));

        let echoes = first_n_queue_prompt_response_echoes(&entries, 3);

        assert_eq!(
            echoes,
            vec![
                ConsumedPromptResponseEcho {
                    identity: "same leaf".to_string(),
                    display: "Root context: #parent: same leaf".to_string(),
                },
                ConsumedPromptResponseEcho {
                    identity: "same leaf".to_string(),
                    display: "Sibling root: same leaf".to_string(),
                },
                ConsumedPromptResponseEcho {
                    identity: "top-level leaf".to_string(),
                    display: "top-level leaf".to_string(),
                },
            ]
        );
    }

    #[test]
    fn done_id_response_echoes_keep_explicit_id_selection_parent_qualified() {
        let entries = entries(concat!(
            "- Parent A\n",
            "  - do [#same] first\n",
            "- Parent B\n",
            "  - do [#same] second\n",
        ));

        let echoes = queue_prompt_response_echoes_for_done_ids(&entries, &["same".to_string()]);

        assert_eq!(
            echoes
                .iter()
                .map(|echo| echo.display.as_str())
                .collect::<Vec<_>>(),
            vec!["Parent A: do [#same] first", "Parent B: do [#same] second"]
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
                "- ### do [#head]\n",
                "- ### ~~do [#opportunistic]~~\n",
                "- ### ~~do [#already]~~\n",
                "- ### do [#tail]\n",
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
                "- ### ~~[#8667]~~\n",
                "- ### do [#liveone]\n",
                "- ### ~~do [#8667] follow-up~~\n",
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
            "- ### ~~do [#gatedphase]~~\n- ### do [#stillopen]\n"
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
            "- ### ~~deploy~~\n- ### do [#tail]\n"
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
                updated.contains("- ### ~~do [#head]~~\n"),
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
                "<!-- agent:queue -->\n- ### ~~do [#orphan]~~\n- do [#tail]\n<!-- /agent:queue -->\n",
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

            assert!(updated.contains("- ### ~~finish sdk demo~~\n"), "{updated}");
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
                "- ### ~~answer the operator~~\n",
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
        assert!(
            updated.contains("- ### ~~do [#orphangone]~~\n"),
            "{updated}"
        );
        assert!(
            updated.contains("- Please keep [#orphangone] visible in the report\n"),
            "{updated}"
        );
        assert!(updated.contains("- do [#other]\n"), "{updated}");
    }

    #[test]
    fn reported_attempt_projection_keys_target_only_the_proved_waiting_parent_child() {
        let content = concat!(
            "<!-- agent:run subagents -->\n",
            "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
            "  - [#smoke] Provide a workspace <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Still gated. <!-- agent-doc:waiting-metadata -->\n",
            "    - 🙋☑️ [#subagent:smoke#2] Resolve the parent. [#smoke]\n",
            "    - 🙋☑️ [#subagent:smoke-old#1] Stale duplicate. [#smoke]\n",
            "  - [#other] Keep another item open <!-- agent-doc:waiting-item -->\n",
            "    - 🙋🚧 [#subagent:other#1] Still running. [#other]\n",
            "    - ☑️ This prose cites [#smoke] without an owner projection\n",
            "<!-- /agent:run -->\n",
        );

        assert!(
            id_backed_head_node_keys(content, "smoke")
                .unwrap()
                .is_empty()
        );
        let keys = reported_attempt_projection_node_keys_for_parent_id(
            content,
            "smoke",
            "subagent:smoke",
            2,
        )
        .expect("the generated reported child is addressable through its parent id");
        assert_eq!(keys.len(), 1);
        let updated = consume_queue_nodes_by_key(content, &keys).unwrap();
        assert!(
            updated.contains("  - [#smoke] Provide a workspace <!-- agent-doc:waiting-item -->\n"),
            "{updated}"
        );
        assert!(!updated.contains("[#subagent:smoke#2]"), "{updated}");
        assert!(updated.contains("[#subagent:smoke-old#1]"), "{updated}");
        assert!(updated.contains("[#subagent:other#1]"), "{updated}");
        assert!(
            updated.contains("☑️ This prose cites [#smoke] without an owner projection"),
            "{updated}"
        );
    }

    /// `#adnestedclaimedidrow`: ID closeout must inspect every runnable
    /// component and preserve the tree parent when the claimed row is a child.
    #[test]
    fn later_component_nested_claimed_id_row_is_found_and_completed() {
        let content = concat!(
            "## Default <!-- agent:run id=default -->\n",
            "- unrelated default work\n",
            "<!-- /agent:run -->\n\n",
            "## Main <!-- agent:run id=main subagents -->\n",
            "- https://github.com/example/project/pull/42\n",
            "  - 🚧 [#subagent:adnestedclaimedidrow#1] ### 🚧 [#subagent:adnestedclaimedidrow#1] do [#adnestedclaimedidrow]\n",
            "<!-- /agent:run -->\n",
        );

        let parsed_nodes =
            agent_doc_markdown_ast::mutations::item_nodes_all(content, "queue").unwrap();
        assert!(
            parsed_nodes.iter().any(|node| {
                queue_prompt_done_id(&node.item.text).as_deref() == Some("adnestedclaimedidrow")
            }),
            "the exact projected row retains its done id: {parsed_nodes:#?}"
        );
        let projected = parsed_nodes
            .iter()
            .find(|node| {
                queue_prompt_done_id(&node.item.text).as_deref() == Some("adnestedclaimedidrow")
            })
            .unwrap();
        assert!(
            !queue_prompt_text_is_free_text(content, &projected.item.text),
            "the exact row is id-backed after normalization: {projected:#?}; normalized={:?}",
            normalize_queue_prompt_text(&projected.item.text),
        );
        let keys = id_backed_head_node_keys(content, "adnestedclaimedidrow").unwrap();
        assert_eq!(
            keys.len(),
            1,
            "the nested row in the later queue is addressable"
        );

        let (completed, count) = mark_queue_prompts_completed_by_done_ids_in_content(
            content,
            &["adnestedclaimedidrow".to_string()],
        )
        .unwrap();
        assert_eq!(count, 1, "only the claimed child is completed");
        assert!(
            completed.contains("- https://github.com/example/project/pull/42\n"),
            "the context parent stays live:\n{completed}"
        );
        assert!(
            completed.contains("  - ~~do [#adnestedclaimedidrow]~~\n"),
            "the lifecycle projection is removed from the struck child:\n{completed}"
        );
        assert!(
            live_queue_prompt_done_ids(&completed, &["adnestedclaimedidrow".to_string()])
                .unwrap()
                .is_empty(),
            "the all-component closeout witness sees no live copy"
        );
        assert!(
            done_ids_have_atomic_queue_completion(
                content,
                &completed,
                &["adnestedclaimedidrow".to_string()],
            )
            .unwrap(),
            "the all-component atomicity witness accepts the child strike"
        );
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
    fn answered_nested_vector_strikes_only_the_matching_parent_child() {
        let document = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ### https://github.com/haiven-dev/haiven-api/pull/822\n",
            "  - 📥 #adversarial-pr ⟦effect:1⟧ [#q-111111111111]\n",
            "- ### https://github.com/haiven-dev/haiven-api/pull/823\n",
            "  - ☑️ [#subagent:adversarial-pr-823#1] #adversarial-pr ⟦effect:2⟧ [#q-222222222222]\n",
            "<!-- /agent:queue -->\n",
        );
        let response = concat!(
            "> **Queue prompt:** https://github.com/haiven-dev/haiven-api/pull/823: #adversarial-pr\n\n",
            "Completed the adversarial review for pull request 823.\n",
        );

        let vector = "https://github.com/haiven-dev/haiven-api/pull/823: #adversarial-pr";
        let response_heads = crate::queue_response::queue_heads_by_normalized_prompt_text(document);
        assert_eq!(
            response_heads.get("#adversarial-pr"),
            Some(&vec![
                "https://github.com/haiven-dev/haiven-api/pull/822: #adversarial-pr".to_string(),
                vector.to_string(),
            ]),
            "{response_heads:?}"
        );
        let projection = project_answered_free_text_strike_with_claims(
            document,
            response,
            Some(document),
            &[vector.to_string()],
        )
        .unwrap()
        .expect("the parent-scoped child is answered");

        assert_eq!(projection.node_keys.len(), 1, "{:?}", projection.node_keys);
        assert!(
            projection.target_content.contains(
                "- ### https://github.com/haiven-dev/haiven-api/pull/822\n  - 📥 #adversarial-pr ⟦effect:1⟧ [#q-111111111111]\n"
            ),
            "the sibling vector stays live:\n{}",
            projection.target_content
        );
        assert!(
            projection.target_content.contains(
                "- ### https://github.com/haiven-dev/haiven-api/pull/823\n  - ~~#adversarial-pr ⟦effect:2⟧ [#q-222222222222]~~ — auto-struck: answered this cycle (#ftstrike)"
            ),
            "the addressed vector is struck:\n{}",
            projection.target_content
        );
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
                .contains("- ### ~~Run the selected queue synchronization repair~~"),
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
                .contains("- ### ~~If an issue was added, also add `- release + publish`~~"),
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
            projected.target_content.contains(
                "- ### ~~staging deploy~~ — auto-struck: answered this cycle (#ftstrike)"
            )
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
                "- ### ~~{item}~~ — auto-struck: answered this cycle (#ftstrike)"
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
            "- ### ~~#gh-fix https://github.com/btakita/agent-doc/issues/177~~ — auto-struck: answered this cycle (#ftstrike)"
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
            "<!-- agent:queue go id=release response=\"inline\" -->\n- ### ~~#upgrade~~ — auto-struck: answered this cycle (#ftstrike)\n  - **Response {response_id}**\n    - **Timestamp:** {FALLBACK_RESPONSE_TIMESTAMP}\n    - <details data-agent-doc-response-id=\"{attribute_id}\">\n      <summary>Response</summary>\n      \n      ### Re: upgrade — gpt-6.1-sol\n      \n      > **Queue prompt:** #upgrade\n      \n      Upgraded.\n      </details>\n<!-- /agent:queue -->"
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
            projected.target_content.contains(
                "- ### ~~completed work~~ — auto-struck: answered this cycle (#ftstrike)"
            )
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
                target.contains("- ### ~~end a Reported queue claim on every strike path"),
                "the echoed reported claimed row strikes:\n{target}"
            );
            assert!(
                target.contains("- ### ~~I deleted a queue item in docs.md"),
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
                "- ### ~~#gh-fix https://github.com/btakita/agent-doc/issues/{issue}~~ — auto-struck: {STRUCK_FREE_TEXT_NOTE}"
            );
            assert!(
                projected.target_content.contains(&struck),
                "preset head #{issue} must strike:\n{}",
                projected.target_content
            );
        }
        assert!(projected.target_content.contains("- release + publish\n"));
        assert!(projected.target_content.contains(
            "- ### ~~The Idea Plugin formatting is not active. The element content should have a background. It's gone now.~~"
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
                .contains("- ### ~~#gh-fix https://github.com/btakita/agent-doc/issues/80~~")
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

    #[test]
    fn a_response_decision_for_another_request_is_never_grafted_onto_a_closed_row() {
        // `#waitingpacketowner`: the response records a decision for a
        // different item. The row it closes owns no part of that decision.
        let response = concat!(
            "### Re: batch — test\n\n",
            "> **Queue prompt:** Publish alpha [#q-alpha]\n\n",
            "**Outcome:** Alpha shipped.\n\n",
            "**Decision record** for `[#subagent-operator-request-other-item]`:\n",
            "- **Question:** Which colour should the other item use?\n",
            "- **Choices presented:** red, blue.\n",
            "- **Selected:** **red**. Paint the other item red.\n",
        );
        let before = doc_with_queue_and_exchange("- Publish alpha [#q-alpha]\n", "");
        let after = doc_with_queue_and_exchange("- ~~Publish alpha [#q-alpha]~~\n", response);
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);
        assert!(
            queue.contains("**Summary:** **Outcome:** Alpha shipped."),
            "{queue}"
        );
        assert!(!queue.contains("Which colour"), "{queue}");
        assert!(!queue.contains("**Selected:**"), "{queue}");
    }

    /// `#waitingtriage`: one response closed two rows and recorded a decision
    /// that names no request. A shared batch's decision belongs to no single
    /// row, so neither row receives its options (before the fix, both rows
    /// carried the same Choices block, and an unrelated waiting item showed
    /// another decision's options).
    #[test]
    fn a_shared_batch_decision_without_a_request_is_grafted_under_no_row() {
        let response = concat!(
            "### Re: batch — test\n\n",
            "> **Queue prompt:** Publish alpha [#q-alpha]\n\n",
            "**Outcome:** Alpha shipped.\n\n",
            "> **Queue prompt:** Publish beta [#q-beta]\n\n",
            "**Outcome:** Beta shipped.\n\n",
            "**Decision record**:\n",
            "- **Question:** Which colour should the beta banner use?\n",
            "- **Choices presented:** red, blue.\n",
            "- **Selected:** **red**. Paint the beta banner red.\n",
        );
        let before = doc_with_queue_and_exchange(
            "- Publish alpha [#q-alpha]\n- Publish beta [#q-beta]\n",
            "",
        );
        let after = doc_with_queue_and_exchange(
            "- ~~Publish alpha [#q-alpha]~~\n- ~~Publish beta [#q-beta]~~\n",
            response,
        );
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);
        assert!(
            queue.contains("[#r-"),
            "the row links the response: {queue}"
        );
        assert!(!queue.contains("**Choices:**"), "{queue}");
        assert!(!queue.contains("**red**"), "{queue}");
        assert!(!queue.contains("**Selected:**"), "{queue}");

        // The same decision recorded in alpha's own section is alpha's.
        let own = response.replace(
            "**Outcome:** Alpha shipped.\n\n",
            "**Outcome:** Alpha shipped.\n\n**Decision record**:\n- **Question:** Which colour should the alpha banner use?\n- **Choices presented:** green, gold.\n- **Selected:** **green**. Paint the alpha banner green.\n\n",
        );
        let after = doc_with_queue_and_exchange(
            "- ~~Publish alpha [#q-alpha]~~\n- ~~Publish beta [#q-beta]~~\n",
            &own,
        );
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &own).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);
        assert!(
            queue.contains("**green**: Offered choice. — **Selected**"),
            "{queue}"
        );
        assert!(!queue.contains("**red**"), "{queue}");
    }

    #[test]
    fn a_closed_offered_choice_links_its_response_without_recopying_the_choices() {
        // `#waitingpacketowner`: an offered option that an older binary
        // dispatched as its own row is closed by a link. Its packet's options
        // are never re-copied beneath one of those options.
        let row = "**Grant both**: Run both PUTs. [#q-grant]";
        let response = format!(
            "### Re: batch — test\n\n\
> **Queue prompt:** Publish alpha [#q-alpha]\n\n\
**Outcome:** Alpha shipped.\n\n\
> **Queue prompt:** {row}\n\n\
**Outcome:** Granted.\n\n\
[#subagent-operator-request-grant-followup] subagent operator request\n\
- *queue_item:*\n  - {row}\n\
- *kind:* question\n\
- *blocking:* true\n\
- *request:*\n  - Confirm the grant?\n\
- *choices:*\n  - **Grant both**: Run both PUTs.\n  - **Do not grant**: Leave access unchanged.\n"
        );
        let before =
            doc_with_queue_and_exchange(&format!("- Publish alpha [#q-alpha]\n- {row}\n"), "");
        let after = doc_with_queue_and_exchange(
            &format!("- ~~Publish alpha [#q-alpha]~~\n- ~~{row}~~\n"),
            &response,
        );
        let annotated =
            annotate_newly_struck_queue_items_with_response_id(&before, &after, &response).unwrap();
        let queue = element::parse(&annotated)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue = queue.content(&annotated);
        let choice = &queue[queue.find("- ~~**Grant both**").unwrap()..];
        assert!(
            choice.contains("**Closed by:** [#r-") || choice.contains("**Response [#r-"),
            "{queue}"
        );
        assert!(!choice.contains("**Choices:**"), "{queue}");
        assert!(!queue.contains("**Do not grant**"), "{queue}");
    }
}
