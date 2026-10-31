//! Pure beginning-of-turn queue synchronization payload (`#queuesyncpayload`).
//!
//! Preflight repairs derived queue projections before calling this module, then
//! projects every still-live item -- including items in inactive queue blocks --
//! into one coordinator obligation.  This module deliberately performs no I/O:
//! the preflight host discovers documents and supplies their authoritative text
//! and durable claims.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::document_queue::{self, QueueEntry};
use crate::queue_claim::{QueueClaim, QueueClaimState, claim_identity_at};
use crate::queue_graph::QueueGraph;

/// Main-agent instructions paired with a non-empty sync payload.
pub const QUEUE_SYNC_GUIDANCE: &str = "Beginning-of-turn queue reconciliation found incomplete work. Handle every entry before treating the turn as terminal. The main agent owns document convergence, claims, dispatch/start acknowledgement, steering, waiting, integration, and closeout. For `subagent_dispatch`, claim then spawn then start the worker and retain coordinator ownership. For `main_inline`, do not spawn: execute inline and reconcile the row. Running work must be waited on or steered; reported/checkmark work must be integrated and closed; lost work must be recovered or requeued; reconcile/unknown and scan errors must be investigated; operator-gated work must remain explicitly gated. Workers never self-claim or self-close.";

/// Normalize TTL expiry before projection. The caller supplies one clock cut so
/// every document in an aggregate scan uses the same notion of "now".
pub fn normalize_claims_for_queue_sync(claims: &mut [QueueClaim], now_secs: u64) {
    for claim in claims {
        if claim.state != QueueClaimState::Reported && claim.is_expired(now_secs) {
            claim.state = QueueClaimState::Lost;
        }
    }
}

/// Whether this item belongs to the document whose turn is starting or to a
/// sibling session document discovered by preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueSyncDocumentScope {
    Current,
    Related,
}

/// Who executes the work.  The main agent always owns reconciliation,
/// integration, document convergence, and terminal correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueExecutionMode {
    /// The queue marker has `subagents` / `fan-out`; main claims, dispatches,
    /// starts, steers, waits for, integrates, and closes a subagent attempt.
    SubagentDispatch,
    /// The queue marker has no subagent mode; main executes the item itself and
    /// then reconciles it.  It must not use a native spawn for this item.
    MainInline,
}

/// Typed main-agent responsibility for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueCoordinatorRole {
    DispatchAndCoordinate,
    ExecuteAndCoordinate,
}

/// Typed work executor.  This is separate from coordinator ownership so a
/// consumer cannot mistake a subagent executor for the document owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueExecutorRole {
    Subagent,
    MainAgent,
}

/// Exhaustive incomplete-item partition emitted by preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueSyncLifecycle {
    Pending,
    ClaimedUnstarted,
    Running,
    ReportedCheckmark,
    Lost,
    Blocked,
    ReconcileUnknown,
    OperatorGated,
}

/// The next coordinator operation.  Workers never self-claim or self-close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueSyncOperation {
    Dispatch,
    InlineExecuteThenReconcile,
    ForwardSteering,
    Wait,
    IntegrateClose,
    RecoverRequeue,
    Reconcile,
    OperatorGate,
}

/// One exact live queue row and the coordinator action it still requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncItem {
    pub document_path: String,
    pub document_scope: QueueSyncDocumentScope,
    pub queue_id: String,
    pub component_occurrence: usize,
    /// Exact current queue-row payload, including any visible lifecycle marker.
    pub item: String,
    pub lifecycle: QueueSyncLifecycle,
    pub operation: QueueSyncOperation,
    pub execution_mode: QueueExecutionMode,
    pub coordinator_role: QueueCoordinatorRole,
    pub executor_role: QueueExecutorRole,
    /// Heuristic: the same queue id + work identity was live in the previous
    /// preflight baseline, so this item has survived at least one turn boundary.
    pub incomplete_since_previous_turn: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
}

/// A session-relevant document could not be inspected.  This is action debt,
/// not an empty/drained result: the coordinator must reconcile the named
/// document before terminal success can be inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncScanError {
    pub document_path: String,
    pub detail: String,
    pub operation: QueueSyncOperation,
}

/// Explicit lifecycle partitions.  Keeping these as separate fields makes it
/// difficult for a consumer to accidentally inspect only a selected/active
/// queue and treat every other incomplete state as drained.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncPayload {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scan_errors: Vec<QueueSyncScanError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claimed_unstarted: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub running: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reported_checkmark: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lost: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reconcile_unknown: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_gated: Vec<QueueSyncItem>,
}

impl QueueSyncPayload {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.scan_errors.len()
            + self.pending.len()
            + self.claimed_unstarted.len()
            + self.running.len()
            + self.reported_checkmark.len()
            + self.lost.len()
            + self.blocked.len()
            + self.reconcile_unknown.len()
            + self.operator_gated.len()
    }

    /// Stable partition order used by terminal gates and aggregate sync.
    pub fn items(&self) -> impl Iterator<Item = &QueueSyncItem> {
        self.pending
            .iter()
            .chain(&self.claimed_unstarted)
            .chain(&self.running)
            .chain(&self.reported_checkmark)
            .chain(&self.lost)
            .chain(&self.blocked)
            .chain(&self.reconcile_unknown)
            .chain(&self.operator_gated)
    }

    pub fn extend(&mut self, other: Self) {
        self.scan_errors.extend(other.scan_errors);
        self.pending.extend(other.pending);
        self.claimed_unstarted.extend(other.claimed_unstarted);
        self.running.extend(other.running);
        self.reported_checkmark.extend(other.reported_checkmark);
        self.lost.extend(other.lost);
        self.blocked.extend(other.blocked);
        self.reconcile_unknown.extend(other.reconcile_unknown);
        self.operator_gated.extend(other.operator_gated);
    }

    fn push(&mut self, item: QueueSyncItem) {
        match item.lifecycle {
            QueueSyncLifecycle::Pending => self.pending.push(item),
            QueueSyncLifecycle::ClaimedUnstarted => self.claimed_unstarted.push(item),
            QueueSyncLifecycle::Running => self.running.push(item),
            QueueSyncLifecycle::ReportedCheckmark => self.reported_checkmark.push(item),
            QueueSyncLifecycle::Lost => self.lost.push(item),
            QueueSyncLifecycle::Blocked => self.blocked.push(item),
            QueueSyncLifecycle::ReconcileUnknown => self.reconcile_unknown.push(item),
            QueueSyncLifecycle::OperatorGated => self.operator_gated.push(item),
        }
    }
}

/// Project every live item in every queue component of one document.
///
/// `previous_content` is only age evidence; it never changes the required
/// operation.  `claims` are the already-followed durable attempts for the
/// supplied current document.
pub fn project_document_queue_sync(
    document_path: impl Into<String>,
    document_scope: QueueSyncDocumentScope,
    current_content: &str,
    previous_content: Option<&str>,
    claims: &[QueueClaim],
) -> Result<QueueSyncPayload> {
    let components = agent_doc_element::element::parse(current_content)?;
    let graph = QueueGraph::parse(current_content, &components)?;
    let frontmatter_queue = agent_doc_frontmatter::frontmatter::parse(current_content)
        .ok()
        .and_then(|(frontmatter, _)| {
            crate::control_binding::frontmatter_queue_control(&frontmatter).map(str::to_string)
        });
    let previous = previous_live_items(previous_content);
    let claims = claims
        .iter()
        .map(|claim| (claim.identity.clone(), claim))
        .collect::<HashMap<_, _>>();
    let document_path = document_path.into();
    let mut payload = QueueSyncPayload::default();

    for block in graph.blocks() {
        let queue_paused = crate::control_binding::explicit_queue_pause_mode(
            &block.component.attrs,
            frontmatter_queue.as_deref(),
        );
        let queue_subagents =
            crate::subagent_intent::queue_subagents_mode(&block.component.attrs).is_some();
        let executable = document_queue::executable_prompt_indexes(&block.entries)
            .into_iter()
            .collect::<HashSet<_>>();
        for (index, entry) in block.entries.iter().enumerate() {
            match entry {
                QueueEntry::Prompt(prompt) if executable.contains(&index) => {
                    let identity = claim_identity_at(&block.entries, index)
                        .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text));
                    let claim = claims.get(&identity).copied();
                    let execution_mode = if queue_subagents
                        && !crate::subagent_intent::opts_out_of_queue_subagents(&prompt.text)
                    {
                        QueueExecutionMode::SubagentDispatch
                    } else {
                        QueueExecutionMode::MainInline
                    };
                    let mut lifecycle = classify_lifecycle(&prompt.text, claim);
                    // An inline queue's selected row is cosmetically `Running`
                    // even though it has no worker claim. It is main-agent work
                    // to execute now, not unknown subagent state to reconcile.
                    if claim.is_none()
                        && execution_mode == QueueExecutionMode::MainInline
                        && agent_doc_document::queue_projection::lifecycle_marker(&prompt.text)
                            == Some(
                                agent_doc_document::queue_projection::QueueLifecycleMarker::Running,
                            )
                    {
                        lifecycle = QueueSyncLifecycle::Pending;
                    }
                    if queue_paused && lifecycle == QueueSyncLifecycle::Pending {
                        lifecycle = QueueSyncLifecycle::Blocked;
                    }
                    let operation = if lifecycle == QueueSyncLifecycle::Running
                        && running_item_changed_since_previous_turn(
                            &previous,
                            &block.id,
                            &identity,
                            &prompt.text,
                            claim.expect("running lifecycle has a claim"),
                        ) {
                        QueueSyncOperation::ForwardSteering
                    } else {
                        operation_for(lifecycle, execution_mode)
                    };
                    let (coordinator_role, executor_role) = roles_for(execution_mode);
                    payload.push(QueueSyncItem {
                        document_path: document_path.clone(),
                        document_scope,
                        queue_id: block.id.clone(),
                        component_occurrence: block.occurrence,
                        item: prompt.text.clone(),
                        lifecycle,
                        operation,
                        execution_mode,
                        coordinator_role,
                        executor_role,
                        incomplete_since_previous_turn: previous
                            .by_identity
                            .contains_key(&(block.id.clone(), identity)),
                        owner: claim.map(|claim| claim.owner.clone()),
                        worker_id: claim
                            .and_then(|claim| claim.worker.as_ref())
                            .map(|worker| worker.worker_id.clone()),
                        generation: claim.map(|claim| claim.generation),
                    });
                }
                QueueEntry::Freeform(text) if !document_queue::is_noise_freeform_line(text) => {
                    let execution_mode = if queue_subagents {
                        QueueExecutionMode::SubagentDispatch
                    } else {
                        QueueExecutionMode::MainInline
                    };
                    let (coordinator_role, executor_role) = roles_for(execution_mode);
                    payload.push(QueueSyncItem {
                        document_path: document_path.clone(),
                        document_scope,
                        queue_id: block.id.clone(),
                        component_occurrence: block.occurrence,
                        item: text.clone(),
                        lifecycle: QueueSyncLifecycle::ReconcileUnknown,
                        operation: QueueSyncOperation::Reconcile,
                        execution_mode,
                        coordinator_role,
                        executor_role,
                        incomplete_since_previous_turn: previous.by_identity.contains_key(&(
                            block.id.clone(),
                            crate::queue_claim::claim_identity(text),
                        )),
                        owner: None,
                        worker_id: None,
                        generation: None,
                    });
                }
                _ => {}
            }
        }
    }
    Ok(payload)
}

#[derive(Default)]
struct PreviousLiveItems {
    by_identity: HashMap<(String, agent_doc_element_queue::QueueItemIdentity), String>,
    normalized_texts: HashSet<String>,
}

fn previous_live_items(content: Option<&str>) -> PreviousLiveItems {
    let Some(content) = content else {
        return PreviousLiveItems::default();
    };
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return PreviousLiveItems::default();
    };
    let Ok(graph) = QueueGraph::parse(content, &components) else {
        return PreviousLiveItems::default();
    };
    let mut previous = PreviousLiveItems::default();
    for block in graph.blocks() {
        for (index, entry) in block.entries.iter().enumerate() {
            let (identity, text) = match entry {
                QueueEntry::Prompt(prompt) => (
                    claim_identity_at(&block.entries, index)
                        .unwrap_or_else(|| crate::queue_claim::claim_identity(&prompt.text)),
                    prompt.text.as_str(),
                ),
                QueueEntry::Freeform(text) if !document_queue::is_noise_freeform_line(text) => {
                    (crate::queue_claim::claim_identity(text), text.as_str())
                }
                _ => continue,
            };
            let normalized = document_queue::normalized_queue_line_for_match(text);
            previous.normalized_texts.insert(normalized.clone());
            previous
                .by_identity
                .insert((block.id.clone(), identity), normalized);
        }
    }
    previous
}

fn running_item_changed_since_previous_turn(
    previous: &PreviousLiveItems,
    queue_id: &str,
    identity: &agent_doc_element_queue::QueueItemIdentity,
    current_text: &str,
    claim: &QueueClaim,
) -> bool {
    let current = document_queue::normalized_queue_line_for_match(current_text);
    if previous
        .by_identity
        .get(&(queue_id.to_string(), identity.clone()))
        .is_some_and(|prior| prior != &current)
    {
        return true;
    }
    claim.followed_generation == claim.generation
        && claim.followed_from.iter().any(|prior| {
            previous
                .normalized_texts
                .contains(&document_queue::normalized_queue_line_for_match(prior))
        })
}

fn classify_lifecycle(item: &str, claim: Option<&QueueClaim>) -> QueueSyncLifecycle {
    if !agent_doc_document::queue_projection::operator_action_ids(item).is_empty()
        || item.to_ascii_lowercase().contains("[operator-verify]")
    {
        return QueueSyncLifecycle::OperatorGated;
    }
    if let Some(claim) = claim {
        return match claim.state {
            QueueClaimState::Claimed => QueueSyncLifecycle::ClaimedUnstarted,
            QueueClaimState::Running => QueueSyncLifecycle::Running,
            QueueClaimState::Reported => QueueSyncLifecycle::ReportedCheckmark,
            QueueClaimState::Lost => QueueSyncLifecycle::Lost,
        };
    }
    use agent_doc_document::queue_projection::QueueLifecycleMarker;
    match agent_doc_document::queue_projection::lifecycle_marker(item) {
        None | Some(QueueLifecycleMarker::Pending) => QueueSyncLifecycle::Pending,
        Some(QueueLifecycleMarker::Blocked) => QueueSyncLifecycle::Blocked,
        Some(QueueLifecycleMarker::Reported) => QueueSyncLifecycle::ReportedCheckmark,
        Some(QueueLifecycleMarker::Lost) => QueueSyncLifecycle::Lost,
        Some(QueueLifecycleMarker::AwaitingOperator) => QueueSyncLifecycle::OperatorGated,
        Some(
            QueueLifecycleMarker::Claimed
            | QueueLifecycleMarker::Running
            | QueueLifecycleMarker::Reconciling
            | QueueLifecycleMarker::Unknown
            | QueueLifecycleMarker::Skipped,
        ) => QueueSyncLifecycle::ReconcileUnknown,
    }
}

fn operation_for(
    lifecycle: QueueSyncLifecycle,
    execution_mode: QueueExecutionMode,
) -> QueueSyncOperation {
    match lifecycle {
        QueueSyncLifecycle::Pending | QueueSyncLifecycle::ClaimedUnstarted => {
            match execution_mode {
                QueueExecutionMode::SubagentDispatch => QueueSyncOperation::Dispatch,
                QueueExecutionMode::MainInline => QueueSyncOperation::InlineExecuteThenReconcile,
            }
        }
        QueueSyncLifecycle::Running | QueueSyncLifecycle::Blocked => QueueSyncOperation::Wait,
        QueueSyncLifecycle::ReportedCheckmark => QueueSyncOperation::IntegrateClose,
        QueueSyncLifecycle::Lost => match execution_mode {
            QueueExecutionMode::SubagentDispatch => QueueSyncOperation::RecoverRequeue,
            QueueExecutionMode::MainInline => QueueSyncOperation::InlineExecuteThenReconcile,
        },
        QueueSyncLifecycle::ReconcileUnknown => QueueSyncOperation::Reconcile,
        QueueSyncLifecycle::OperatorGated => QueueSyncOperation::OperatorGate,
    }
}

fn roles_for(execution_mode: QueueExecutionMode) -> (QueueCoordinatorRole, QueueExecutorRole) {
    match execution_mode {
        QueueExecutionMode::SubagentDispatch => (
            QueueCoordinatorRole::DispatchAndCoordinate,
            QueueExecutorRole::Subagent,
        ),
        QueueExecutionMode::MainInline => (
            QueueCoordinatorRole::ExecuteAndCoordinate,
            QueueExecutorRole::MainAgent,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{QueueClaimLedger, QueueWorkerBinding};

    fn claim(item: &str, state: QueueClaimState, worker: Option<&str>) -> QueueClaim {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(item, "subagent:test", 100, 3_600);
        let identity = crate::queue_claim::claim_identity(item);
        let stored = ledger
            .claims
            .iter_mut()
            .find(|stored| stored.identity == identity)
            .expect("stored claim");
        stored.state = state;
        stored.worker = worker.map(|worker_id| QueueWorkerBinding {
            worker_id: worker_id.to_string(),
            started_at_secs: 101,
        });
        stored.clone()
    }

    #[test]
    fn partitions_every_incomplete_item_across_inactive_queues() {
        let previous = concat!(
            "<!-- agent:run id=main subagents go -->\n",
            "- old pending\n",
            "- reported\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:run id=inactive -->\n",
            "- inline work\n",
            "<!-- /agent:run -->\n",
        );
        let current = concat!(
            "<!-- agent:run id=main subagents go -->\n",
            "- 📥 old pending\n",
            "- ☑️ reported\n",
            "- ⚠️ lost work\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:run id=inactive -->\n",
            "- inline work\n",
            "- ⏳ [operator-verify] approve\n",
            "<!-- /agent:run -->\n",
        );
        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            current,
            Some(previous),
            &[],
        )
        .unwrap();

        assert_eq!(payload.len(), 5);
        assert_eq!(payload.pending[0].operation, QueueSyncOperation::Dispatch);
        assert!(payload.pending[0].incomplete_since_previous_turn);
        assert_eq!(
            payload.reported_checkmark[0].operation,
            QueueSyncOperation::IntegrateClose
        );
        assert_eq!(
            payload.lost[0].operation,
            QueueSyncOperation::RecoverRequeue
        );
        assert_eq!(
            payload
                .pending
                .iter()
                .find(|item| item.queue_id == "inactive")
                .unwrap()
                .operation,
            QueueSyncOperation::InlineExecuteThenReconcile
        );
        assert_eq!(payload.operator_gated.len(), 1);
    }

    #[test]
    fn durable_claims_supply_owner_worker_generation_and_action() {
        let content = concat!(
            "<!-- agent:run id=main subagents -->\n",
            "- first\n",
            "- second\n",
            "- third\n",
            "<!-- /agent:run -->\n",
        );
        let claims = vec![
            claim("first", QueueClaimState::Claimed, None),
            claim("second", QueueClaimState::Running, Some("worker-2")),
            claim("third", QueueClaimState::Reported, Some("worker-3")),
        ];
        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &claims,
        )
        .unwrap();

        assert_eq!(
            payload.claimed_unstarted[0].operation,
            QueueSyncOperation::Dispatch
        );
        assert_eq!(
            payload.claimed_unstarted[0].owner.as_deref(),
            Some("subagent:test")
        );
        assert_eq!(payload.running[0].worker_id.as_deref(), Some("worker-2"));
        assert_eq!(payload.running[0].operation, QueueSyncOperation::Wait);
        assert_eq!(payload.reported_checkmark[0].generation, Some(1));
    }

    #[test]
    fn queue_without_subagents_never_recommends_native_dispatch() {
        let content = "<!-- agent:run id=main -->\n- #subagents fix it\n- ⚠️ retry it\n- 🚧 selected work\n<!-- /agent:run -->\n";
        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &[],
        )
        .unwrap();

        assert!(payload.items().all(|item| {
            item.execution_mode == QueueExecutionMode::MainInline
                && item.executor_role == QueueExecutorRole::MainAgent
                && item.operation == QueueSyncOperation::InlineExecuteThenReconcile
        }));
    }

    #[test]
    fn changed_running_row_is_forward_steering_debt() {
        let previous =
            "<!-- agent:run id=main subagents -->\n- do [#fix]: old wording\n<!-- /agent:run -->\n";
        let current = "<!-- agent:run id=main subagents -->\n- 🚧 do [#fix]: corrected wording\n<!-- /agent:run -->\n";
        let claims = vec![claim(
            "do [#fix]: old wording",
            QueueClaimState::Running,
            Some("worker-fix"),
        )];

        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            current,
            Some(previous),
            &claims,
        )
        .unwrap();

        assert_eq!(payload.running.len(), 1);
        assert_eq!(
            payload.running[0].operation,
            QueueSyncOperation::ForwardSteering
        );
    }

    #[test]
    fn explicitly_paused_queue_is_wait_debt_not_inline_execution() {
        let content = concat!(
            "---\nqueue: pause\n---\n",
            "<!-- agent:run id=main -->\n- 📥 held work\n<!-- /agent:run -->\n",
        );
        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &[],
        )
        .unwrap();

        assert_eq!(payload.blocked.len(), 1);
        assert_eq!(payload.blocked[0].operation, QueueSyncOperation::Wait);
    }
}
