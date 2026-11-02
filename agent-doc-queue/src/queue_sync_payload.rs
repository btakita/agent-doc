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
pub const QUEUE_SYNC_GUIDANCE: &str = "Beginning-of-turn queue reconciliation found incomplete work. Handle every entry before treating the turn as terminal. The main agent owns document convergence, claims, dispatch/start acknowledgement, steering, waiting, integration, and closeout. For `subagent_dispatch`, claim then spawn then start the worker and retain coordinator ownership. For `main_inline`, do not spawn: execute inline and reconcile the row. Running work must be waited on or steered; reported/checkmark work must be integrated and closed; lost work must be recovered or requeued. Every reported or `lost_reported` / `⚠️☑️` row is urgent nonterminal coordinator debt: compare its durable outbox/report receipt and exact retained agent-history response with the root-authored exchange response. An abbreviated receipt is valid completion evidence but is not proof that the full worker result reached exchange. Materialize every `recovered_response` in the root response exactly once, integrate effects, close, and strike in this turn. `consolidated_exchange` means that exact result is already present outside binary completion cells and must not be replayed. If response history is missing, ambiguous, or unsupported, keep the row open, surface `missing_response_debt` as the concrete blocker, and never redispatch or silently strike it. Reconcile/unknown, unresolved backlog queue placement, and scan errors must be investigated; operator-gated work must remain explicitly gated. Workers never self-claim or self-close.";

/// Normalize TTL expiry before projection. The caller supplies one clock cut so
/// every document in an aggregate scan uses the same notion of "now".
pub fn normalize_claims_for_queue_sync(claims: &mut [QueueClaim], now_secs: u64) {
    for claim in claims {
        if claim.state != QueueClaimState::Reported && claim.is_expired(now_secs) {
            claim.mark_lost();
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
    LostReported,
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

/// Durable proof about whether a lost attempt executed. Retry is safe only for
/// `NeverStarted`; `Unknown` deliberately fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueSyncExecutionEvidence {
    NeverStarted,
    Started,
    CompletedOutbox,
    Unknown,
}

/// Where the exact worker response needed for recovered-loss closeout is
/// currently available. `LostReported` is never terminal merely because a
/// completion key or receipt exists: coordinator integration needs the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueSyncResponseEvidence {
    NotApplicable,
    /// The full history-attested worker response is already present in the
    /// root-authored exchange outside binary completion-receipt cells.
    ConsolidatedExchange,
    /// A binary completion receipt exists. This is completion/outbox evidence,
    /// but an abbreviated receipt never proves full root-response delivery.
    ExchangeOutbox,
    AgentHistory,
    Missing,
    AmbiguousHistory,
    UnsupportedHistory,
}

/// Durable side-effect receipt recovered from the exact attempt lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncEffectEvidence {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sink_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_state: Option<String>,
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
    pub execution_evidence: QueueSyncExecutionEvidence,
    /// True only when exact fenced completion was recovered after the attempt
    /// had already become lost. The visible row projects `⚠️☑️` as well.
    pub completion_outbox_recovered: bool,
    /// A generation-fenced binary completion cell carries a response receipt.
    /// The receipt may be intentionally abbreviated, so this is completion
    /// evidence rather than proof of full root-response delivery.
    #[serde(default)]
    pub completion_response_receipt: bool,
    /// Durable response/completion backlink for this exact attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// Hash of the full durable response capture, when present in the receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_capture_sha256: Option<String>,
    /// Exact response source for reported-result integration.
    pub response_evidence: QueueSyncResponseEvidence,
    /// Response recovered from exact retained agent history when the exchange
    /// outbox was missing. The coordinator must materialize it in exchange and
    /// integrate it before striking the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_response: Option<String>,
    /// Explicit nonterminal debt: completion exists but its response body is
    /// unavailable or ambiguous. Never strike or redispatch this row.
    pub missing_response_debt: bool,
    /// A retry is authorized only by durable proof that this exact attempt was
    /// never started. Absence of readable provenance is not proof.
    pub retry_safe: bool,
    /// Exact committed/pushed/integrated/released receipts recovered from the
    /// attempt lineage. Coordinators process these instead of replaying an
    /// uncertain effect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovered_effects: Vec<QueueSyncEffectEvidence>,
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

/// An active backlog/pending item requested queue placement that cannot be
/// resolved without a coordinator choice. Bare `queue` is ambiguous when more
/// than one queue exists; an explicit missing queue id is likewise retained as
/// typed reconcile debt instead of being treated as a scan failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncPlacementDebt {
    pub document_path: String,
    pub document_scope: QueueSyncDocumentScope,
    pub item_id: String,
    pub item_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_queue_id: Option<String>,
    pub eligible_queue_ids: Vec<String>,
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
    pub placement_reconcile: Vec<QueueSyncPlacementDebt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claimed_unstarted: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub running: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reported_checkmark: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lost_reported: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lost: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reconcile_unknown: Vec<QueueSyncItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_gated: Vec<QueueSyncItem>,
}

/// One inert, item-addressed recovery response emitted by declarative sync.
/// Its id is attempt-stable: new evidence updates the diagnosis instead of
/// manufacturing a duplicate response for the same queue slot/generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSyncRecoveryResponse {
    pub response_id: String,
    pub item: String,
    pub summary: String,
    pub next_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl QueueSyncRecoveryResponse {
    pub fn render_markdown(&self) -> String {
        let mut rendered = format!("**Response [#{}]:** {}", self.response_id, self.summary);
        if !self.next_actions.is_empty() {
            rendered.push_str("\n- **Next actions:**");
            for action in &self.next_actions {
                rendered.push_str("\n  - **Action:** ");
                rendered.push_str(action);
            }
        }
        if !self.choices.is_empty() {
            rendered.push_str("\n- **Choices:**");
            for choice in &self.choices {
                rendered.push_str("\n  - ");
                rendered.push_str(choice);
            }
        }
        rendered
    }
}

impl QueueSyncPayload {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.scan_errors.len()
            + self.placement_reconcile.len()
            + self.pending.len()
            + self.claimed_unstarted.len()
            + self.running.len()
            + self.reported_checkmark.len()
            + self.lost_reported.len()
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
            .chain(&self.lost_reported)
            .chain(&self.lost)
            .chain(&self.blocked)
            .chain(&self.reconcile_unknown)
            .chain(&self.operator_gated)
    }

    pub fn extend(&mut self, other: Self) {
        self.scan_errors.extend(other.scan_errors);
        self.placement_reconcile.extend(other.placement_reconcile);
        self.pending.extend(other.pending);
        self.claimed_unstarted.extend(other.claimed_unstarted);
        self.running.extend(other.running);
        self.reported_checkmark.extend(other.reported_checkmark);
        self.lost_reported.extend(other.lost_reported);
        self.lost.extend(other.lost);
        self.blocked.extend(other.blocked);
        self.reconcile_unknown.extend(other.reconcile_unknown);
        self.operator_gated.extend(other.operator_gated);
    }

    /// Evidence-derived recovery surfaces for attempts that cannot be advanced
    /// automatically. The operation field remains the same-turn coordinator
    /// dispatch intent; only operator/external decisions are left parked.
    pub fn recovery_responses(&self) -> Vec<QueueSyncRecoveryResponse> {
        let mut responses = Vec::new();
        let mut seen = HashSet::new();
        for item in self.items().filter(|item| {
            item.missing_response_debt
                || item.lifecycle == QueueSyncLifecycle::ReconcileUnknown
                || (item.lifecycle == QueueSyncLifecycle::Lost && !item.retry_safe)
        }) {
            let identity = format!("{:?}", crate::queue_claim::claim_identity(&item.item));
            let stable_key = format!(
                "{}\0{}\0{}\0{}\0{}",
                item.document_path,
                item.queue_id,
                item.component_occurrence,
                identity,
                item.generation.unwrap_or_default(),
            );
            let response_id = format!("r-{}", agent_doc_hash::short_content_hash(&stable_key));
            if !seen.insert(response_id.clone()) {
                continue;
            }
            let worker = item.worker_id.as_deref().unwrap_or("unbound");
            let generation = item
                .generation
                .map(|generation| generation.to_string())
                .unwrap_or_else(|| "legacy-0".to_string());
            let summary = format!(
                "Nonterminal recovery inventory: lifecycle={:?}; dispatch_intent={:?}; generation={generation}; worker={worker}; response={:?}; receipt={}; effects={}.",
                item.lifecycle,
                item.operation,
                item.response_evidence,
                item.completion_response_receipt,
                item.recovered_effects.len(),
            );
            let (next_actions, choices) = if item.missing_response_debt {
                (
                    vec![format!(
                        "Blocked on external evidence: recover the exact full response for generation {generation} from worker {worker}; retain the row until capture, receipt, exchange projection, and editor authority agree."
                    )],
                    vec![
                        format!("**recover-worker-{worker}:** Evidence: generation {generation} is completion-proven but its full capture is unavailable. Effect: supply the exact response from this worker lineage and resume integration. Risk: accepting the wrong generation would corrupt attribution. Reversible until closeout. Recommended when worker history or retained output exists."),
                        format!("**operator-cancel:** Evidence: generation {generation} cannot advance without a response or explicit response-free proof. Effect: record typed operator cancellation and leave no inferred response. Risk: abandons any unrecovered worker result. Reversible only by a new generation. Choose only when recovery is impossible or the result is intentionally discarded."),
                    ],
                )
            } else {
                (
                    vec![format!(
                        "Proposed same-turn coordinator action ({:?}): inspect generation {generation}, worker {worker}, capture and effect receipts; advance only on complete proof.",
                        item.operation,
                    )],
                    Vec::new(),
                )
            };
            responses.push(QueueSyncRecoveryResponse {
                response_id,
                item: item.item.clone(),
                summary,
                next_actions,
                choices,
            });
        }
        for debt in &self.placement_reconcile {
            let stable_key = format!(
                "{}\0placement\0{}",
                debt.document_path, debt.item_id
            );
            let response_id = format!("r-{}", agent_doc_hash::short_content_hash(&stable_key));
            if !seen.insert(response_id.clone()) {
                continue;
            }
            let choices = debt
                .eligible_queue_ids
                .iter()
                .map(|queue_id| {
                    format!(
                        "**queue-{queue_id}:** Evidence: `{queue_id}` is an eligible queue and the requested placement is {}. Effect: route this exact item to `{queue_id}`. Risk: choosing the wrong coordinator changes execution order/ownership. Reversible by an editor-authoritative move before dispatch. Recommended only when `{queue_id}` matches the item's intended owner.",
                        debt.requested_queue_id.as_deref().unwrap_or("ambiguous")
                    )
                })
                .collect();
            responses.push(QueueSyncRecoveryResponse {
                response_id,
                item: debt.item_text.clone(),
                summary: format!(
                    "Nonterminal placement decision for item #{}; requested={}; eligible={}.",
                    debt.item_id,
                    debt.requested_queue_id.as_deref().unwrap_or("ambiguous"),
                    debt.eligible_queue_ids.join(","),
                ),
                next_actions: vec![
                    "Blocked on operator placement judgment; keep the item open until one eligible queue is selected."
                        .to_string(),
                ],
                choices,
            });
        }
        for error in &self.scan_errors {
            let stable_key = format!("{}\0scan-error", error.document_path);
            let response_id = format!("r-{}", agent_doc_hash::short_content_hash(&stable_key));
            if !seen.insert(response_id.clone()) {
                continue;
            }
            responses.push(QueueSyncRecoveryResponse {
                response_id,
                item: error.document_path.clone(),
                summary: format!(
                    "Nonterminal document scan debt: operation={:?}; evidence={}",
                    error.operation, error.detail
                ),
                next_actions: vec![
                    "Blocked on document authority/readability; restore access to the named document and rerun queue sync before advancing any inferred state."
                        .to_string(),
                ],
                choices: Vec::new(),
            });
        }
        responses
    }

    fn push(&mut self, item: QueueSyncItem) {
        match item.lifecycle {
            QueueSyncLifecycle::Pending => self.pending.push(item),
            QueueSyncLifecycle::ClaimedUnstarted => self.claimed_unstarted.push(item),
            QueueSyncLifecycle::Running => self.running.push(item),
            QueueSyncLifecycle::ReportedCheckmark => self.reported_checkmark.push(item),
            QueueSyncLifecycle::LostReported => self.lost_reported.push(item),
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
                        execution_evidence: execution_evidence(claim),
                        completion_outbox_recovered: claim
                            .is_some_and(|claim| claim.completion_recovered_after_loss),
                        completion_response_receipt: false,
                        response_id: None,
                        response_capture_sha256: None,
                        response_evidence: if claim
                            .is_some_and(|claim| claim.completion_recovered_after_loss)
                        {
                            QueueSyncResponseEvidence::Missing
                        } else {
                            QueueSyncResponseEvidence::NotApplicable
                        },
                        recovered_response: None,
                        missing_response_debt: claim
                            .is_some_and(|claim| claim.completion_recovered_after_loss),
                        retry_safe: false,
                        recovered_effects: Vec::new(),
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
                        execution_evidence: QueueSyncExecutionEvidence::Unknown,
                        completion_outbox_recovered: false,
                        completion_response_receipt: false,
                        response_id: None,
                        response_capture_sha256: None,
                        response_evidence: QueueSyncResponseEvidence::NotApplicable,
                        recovered_response: None,
                        missing_response_debt: false,
                        retry_safe: false,
                        recovered_effects: Vec::new(),
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
            QueueClaimState::Reported if claim.completion_recovered_after_loss => {
                QueueSyncLifecycle::LostReported
            }
            QueueClaimState::Reported => QueueSyncLifecycle::ReportedCheckmark,
            QueueClaimState::Lost => QueueSyncLifecycle::Lost,
        };
    }
    use agent_doc_document::queue_projection::QueueLifecycleMarker;
    match agent_doc_document::queue_projection::lifecycle_marker(item) {
        None | Some(QueueLifecycleMarker::Pending) => QueueSyncLifecycle::Pending,
        Some(QueueLifecycleMarker::Blocked) => QueueSyncLifecycle::Blocked,
        Some(QueueLifecycleMarker::Reported) => QueueSyncLifecycle::ReportedCheckmark,
        // A visible loss without its durable attempt is provenance-unknown. It
        // must be reconciled, never treated as retry authorization.
        Some(
            QueueLifecycleMarker::Lost
            | QueueLifecycleMarker::LostClaimed
            | QueueLifecycleMarker::LostRunning
            | QueueLifecycleMarker::LostReported,
        ) => {
            QueueSyncLifecycle::ReconcileUnknown
        }
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
        QueueSyncLifecycle::LostReported => QueueSyncOperation::IntegrateClose,
        QueueSyncLifecycle::Lost => QueueSyncOperation::Reconcile,
        QueueSyncLifecycle::ReconcileUnknown => QueueSyncOperation::Reconcile,
        QueueSyncLifecycle::OperatorGated => QueueSyncOperation::OperatorGate,
    }
}

fn execution_evidence(claim: Option<&QueueClaim>) -> QueueSyncExecutionEvidence {
    let Some(claim) = claim else {
        return QueueSyncExecutionEvidence::Unknown;
    };
    if claim.state == QueueClaimState::Reported && claim.completion_recovered_after_loss {
        QueueSyncExecutionEvidence::CompletedOutbox
    } else if claim.worker.is_some() {
        QueueSyncExecutionEvidence::Started
    } else if claim.state == QueueClaimState::Lost {
        QueueSyncExecutionEvidence::Unknown
    } else {
        QueueSyncExecutionEvidence::NeverStarted
    }
}

/// Merge exact-attempt lineage evidence into one projected item. A lost item
/// becomes redispatchable only when the caller proved `NeverStarted` from a
/// durable lineage record; all other outcomes fail closed.
pub fn apply_attempt_evidence(
    item: &mut QueueSyncItem,
    execution: QueueSyncExecutionEvidence,
    effects: Vec<QueueSyncEffectEvidence>,
) {
    item.execution_evidence = execution;
    item.recovered_effects = effects;
    item.retry_safe = item.lifecycle == QueueSyncLifecycle::Lost
        && execution == QueueSyncExecutionEvidence::NeverStarted;
    if item.lifecycle == QueueSyncLifecycle::Lost {
        item.operation = if item.retry_safe {
            match item.execution_mode {
                QueueExecutionMode::SubagentDispatch => QueueSyncOperation::RecoverRequeue,
                QueueExecutionMode::MainInline => QueueSyncOperation::InlineExecuteThenReconcile,
            }
        } else {
            QueueSyncOperation::Reconcile
        };
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
        assert_eq!(payload.lost.len(), 0);
        let orphan_lost = payload
            .reconcile_unknown
            .iter()
            .find(|item| item.item.contains("lost work"))
            .expect("a visible lost marker without durable provenance");
        assert_eq!(orphan_lost.operation, QueueSyncOperation::Reconcile);
        assert!(!orphan_lost.retry_safe);
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
    fn lost_attempts_retry_only_when_durable_state_proves_they_never_started() {
        let content = concat!(
            "<!-- agent:run id=main subagents -->\n",
            "- ⚠️ unstarted\n",
            "- ⚠️ started\n",
            "- ⚠️☑️ recovered\n",
            "- ⚠️ orphan\n",
            "<!-- /agent:run -->\n",
        );
        let mut recovered = claim(
            "recovered",
            QueueClaimState::Reported,
            Some("worker-recovered"),
        );
        recovered.completion_recovered_after_loss = true;
        let claims = vec![
            claim("unstarted", QueueClaimState::Lost, None),
            claim("started", QueueClaimState::Lost, Some("worker-started")),
            recovered,
        ];

        let mut payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &claims,
        )
        .unwrap();

        let unstarted = payload
            .lost
            .iter_mut()
            .find(|item| item.item.contains("unstarted"))
            .unwrap();
        apply_attempt_evidence(
            unstarted,
            QueueSyncExecutionEvidence::NeverStarted,
            Vec::new(),
        );
        assert_eq!(unstarted.operation, QueueSyncOperation::RecoverRequeue);
        assert_eq!(
            unstarted.execution_evidence,
            QueueSyncExecutionEvidence::NeverStarted
        );
        assert!(unstarted.retry_safe);

        let started = payload
            .lost
            .iter()
            .find(|item| item.item.contains("started") && !item.item.contains("unstarted"))
            .unwrap();
        assert_eq!(started.operation, QueueSyncOperation::Reconcile);
        assert_eq!(
            started.execution_evidence,
            QueueSyncExecutionEvidence::Started
        );
        assert!(!started.retry_safe);

        let recovered = &payload.lost_reported[0];
        assert_eq!(recovered.operation, QueueSyncOperation::IntegrateClose);
        assert_eq!(
            recovered.execution_evidence,
            QueueSyncExecutionEvidence::CompletedOutbox
        );
        assert!(recovered.completion_outbox_recovered);
        assert_eq!(
            recovered.response_evidence,
            QueueSyncResponseEvidence::Missing
        );
        assert!(recovered.missing_response_debt);
        assert!(!recovered.retry_safe);

        let orphan = payload
            .reconcile_unknown
            .iter()
            .find(|item| item.item.contains("orphan"))
            .unwrap();
        assert_eq!(orphan.operation, QueueSyncOperation::Reconcile);
        assert_eq!(
            orphan.execution_evidence,
            QueueSyncExecutionEvidence::Unknown
        );
        assert!(!orphan.retry_safe);
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
        }));
        assert_eq!(payload.pending.len(), 2);
        assert_eq!(payload.reconcile_unknown.len(), 1);
        assert_eq!(
            payload.reconcile_unknown[0].operation,
            QueueSyncOperation::Reconcile
        );
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

    #[test]
    fn recovery_responses_are_attempt_stable_and_choices_are_evidence_bound() {
        let content =
            "<!-- agent:run id=main subagents -->\n- ⚠️☑️ recovered\n<!-- /agent:run -->\n";
        let mut recovered = claim(
            "recovered",
            QueueClaimState::Reported,
            Some("worker-recovered"),
        );
        recovered.completion_recovered_after_loss = true;
        let mut payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &[recovered],
        )
        .unwrap();

        let first = payload.recovery_responses().pop().unwrap();
        assert_eq!(first.choices.len(), 2);
        assert!(first.render_markdown().contains("**Next actions:**"));
        assert!(first.render_markdown().contains("**Choices:**"));
        assert!(first.choices.iter().all(|choice| {
            choice.contains("Evidence:")
                && choice.contains("Effect:")
                && choice.contains("Risk:")
                && choice.contains("Reversible")
        }));

        payload.lost_reported[0].response_evidence =
            QueueSyncResponseEvidence::AmbiguousHistory;
        let updated = payload.recovery_responses().pop().unwrap();
        assert_eq!(first.response_id, updated.response_id);
        assert_ne!(first.summary, updated.summary);
    }

    #[test]
    fn safe_reconcile_intent_has_no_operator_choice() {
        let content = "<!-- agent:run id=main subagents -->\n- ⚠️ orphan\n<!-- /agent:run -->\n";
        let payload = project_document_queue_sync(
            "/tmp/session.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &[],
        )
        .unwrap();
        let recovery = payload.recovery_responses().pop().unwrap();
        assert!(recovery.choices.is_empty());
        assert!(recovery.next_actions[0].contains("same-turn coordinator action"));
        assert_eq!(
            payload.reconcile_unknown[0].operation,
            QueueSyncOperation::Reconcile
        );
    }
}
