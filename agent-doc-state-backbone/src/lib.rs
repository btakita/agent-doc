//! Typed state backbone for durable agent-doc projections.
//!
//! The cycle FSM remains the closeout authority for one response turn. This
//! module gives the surrounding state domains a shared event/reducer shape:
//! append-only facts, deterministic projections, generation-aware owner checks,
//! and small local state machines for closed subdomains.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use lazily::{
    CausalReceipt, ReceiptApplyStatus, ReceiptOutcome, ReceiptProjection, ThreadSafeContext,
    ThreadSafeStateMachine,
};
use serde::{Deserialize, Serialize};

use agent_doc_turn::CyclePhase;
use agent_doc_turn::cp_projection::TurnSteeringProjection;
use agent_doc_turn::{CycleEvent, CyclePhaseMachine};

/// Phase E (`#adstatechart`) local-process Harel state chart consolidation.
pub mod adstatechart;
pub mod closeout_gate;
pub mod retained_write;
pub mod write_pipeline;
pub mod write_source;

pub use write_source::{CloseoutStage, DocumentWriteSource};

/// Project-scoped supervisor graph document. Most state facts are per session
/// document; supervisor recycle is a project-wide gate shared by every routed
/// document, so the CP folds it under this reserved document id.
pub const PROJECT_SUPERVISOR_DOCUMENT_HASH: &str = "__agent_doc_project_supervisor__";
pub const ROUTE_SUBMIT_IN_FLIGHT_TTL_SECS: u64 = 30;
pub const ROUTE_SUBMIT_READY_PROBE_TTL_SECS: u64 = 150;
pub const ROUTE_SUBMIT_BLOCKED_TTL_SECS: u64 = 120;
pub const ROUTE_DISPATCH_SUBMIT_REASON: &str = "dispatch_submit";
pub const ROUTE_DISPATCH_ONLY_READY_PROBE_REASON: &str = "dispatch_only_ready_probe";
pub const QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS: u64 = 60;
pub const QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED: &str = "operator_deferred_clear";
pub const QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_MANUAL_COOLDOWN: &str =
    "operator_manual_clear_cooldown";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateDomain {
    Document,
    Queue,
    Closeout,
    Transport,
    Supervisor,
    Route,
    Proof,
}

/// Typed causes for a document target remaining in the Lazily write lineage.
///
/// The JSON representation intentionally stays a snake-case string so existing
/// controller databases remain readable. `Legacy` preserves forward/backward
/// compatibility without returning the internal API to free-text state checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentWriteDeferredReason {
    EditorOwnerWithoutRegisteredReplica,
    EditorDeliveryWorkerStale,
    CrdtDeliveryAckPending,
    MergeUnsavedEditorCutWithDeferredTarget,
    RetainEditorReconnectLineageBeforeDiskProjection,
    PendingUserDecisionExternalDiskVsEditor,
    ExtendPendingEditorReconnectTarget,
    Legacy(String),
}

impl DocumentWriteDeferredReason {
    pub fn token(&self) -> &str {
        match self {
            Self::EditorOwnerWithoutRegisteredReplica => "editor_owner_without_registered_replica",
            Self::EditorDeliveryWorkerStale => "editor_delivery_worker_stale",
            Self::CrdtDeliveryAckPending => "crdt_delivery_ack_pending",
            Self::MergeUnsavedEditorCutWithDeferredTarget => {
                "merge_unsaved_editor_cut_with_deferred_target"
            }
            Self::RetainEditorReconnectLineageBeforeDiskProjection => {
                "retain_editor_reconnect_lineage_before_disk_projection"
            }
            Self::PendingUserDecisionExternalDiskVsEditor => {
                "pending_user_decision_external_disk_vs_editor"
            }
            Self::ExtendPendingEditorReconnectTarget => "extend_pending_editor_reconnect_target",
            Self::Legacy(token) => token,
        }
    }
}

impl From<&str> for DocumentWriteDeferredReason {
    fn from(value: &str) -> Self {
        match value {
            "editor_owner_without_registered_replica" => Self::EditorOwnerWithoutRegisteredReplica,
            "editor_delivery_worker_stale" => Self::EditorDeliveryWorkerStale,
            "crdt_delivery_ack_pending" => Self::CrdtDeliveryAckPending,
            "merge_unsaved_editor_cut_with_deferred_target" => {
                Self::MergeUnsavedEditorCutWithDeferredTarget
            }
            "retain_editor_reconnect_lineage_before_disk_projection" => {
                Self::RetainEditorReconnectLineageBeforeDiskProjection
            }
            "pending_user_decision_external_disk_vs_editor" => {
                Self::PendingUserDecisionExternalDiskVsEditor
            }
            "extend_pending_editor_reconnect_target" => Self::ExtendPendingEditorReconnectTarget,
            token => Self::Legacy(token.to_string()),
        }
    }
}

impl From<String> for DocumentWriteDeferredReason {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl fmt::Display for DocumentWriteDeferredReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token())
    }
}

impl PartialEq<&str> for DocumentWriteDeferredReason {
    fn eq(&self, other: &&str) -> bool {
        self.token() == *other
    }
}

impl Serialize for DocumentWriteDeferredReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.token())
    }
}

impl<'de> Deserialize<'de> for DocumentWriteDeferredReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from)
    }
}

impl StateDomain {
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Queue => "queue",
            Self::Closeout => "closeout",
            Self::Transport => "transport",
            Self::Supervisor => "supervisor",
            Self::Route => "route",
            Self::Proof => "proof",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEvent {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    pub fact: StateFact,
}

impl StateEvent {
    pub fn new(event_id: impl Into<String>, fact: StateFact) -> Self {
        Self {
            event_id: event_id.into(),
            causation_id: None,
            fact,
        }
    }

    pub fn with_causation(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }

    pub fn document_hash(&self) -> &str {
        self.fact.document_hash()
    }

    pub fn domain(&self) -> StateDomain {
        self.fact.domain()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StateFact {
    PreflightStarted {
        document_hash: String,
        cycle_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tracked_work_maintenance_required: Option<bool>,
    },
    /// Complete turn intent checkpoint stored in the durable Lazily ledger.
    ///
    /// This replaces the former `.agent-doc/state/cycles/*.json` hot-path
    /// authority. The opaque JSON is owned by `agent-doc-cycle-state-io`; the
    /// backbone only retains and projects the newest checkpoint for a cycle.
    TurnIntentCheckpointed {
        document_hash: String,
        cycle_id: String,
        #[serde(default)]
        checkpoint_sequence: u64,
        state_sha256: String,
        state_json: String,
    },
    RealtimeSteeringObserved {
        document_hash: String,
        cycle_id: String,
        steering: TurnSteeringProjection,
        content_hash: String,
    },
    BaselineSaved {
        document_hash: String,
        cycle_id: String,
        baseline_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        baseline_path: Option<String>,
    },
    /// Content-bearing merge baseline used by normal document transitions.
    /// Filesystem snapshots are cold recovery projections and never feed this
    /// authority.
    DocumentBaselineCheckpointed {
        document_hash: String,
        generation: u64,
        content_hash: String,
        content: String,
    },
    /// Explicitly clear the normal merge baseline without deleting history.
    DocumentBaselineCleared {
        document_hash: String,
        generation: u64,
    },
    /// Content-bearing checkpoint used by undo/extract after an agent write.
    UndoCheckpointed {
        document_hash: String,
        generation: u64,
        content_hash: String,
        content: String,
    },
    /// Clear the active undo checkpoint while retaining its ledger history.
    UndoCheckpointCleared {
        document_hash: String,
        generation: u64,
    },
    /// Cold CRDT recovery projection retained in the durable state ledger.
    /// Live editor reads always come from Lazily; this is restart evidence only.
    CrdtRecoveryProjectionCheckpointed {
        document_hash: String,
        generation: u64,
        projection_sha256: String,
        projection_base64: String,
        lineage: String,
    },
    /// Clear the cold CRDT restart projection while retaining ledger history.
    CrdtRecoveryProjectionCleared {
        document_hash: String,
        generation: u64,
    },
    FileWatchChangeObserved {
        document_hash: String,
        path: String,
        watch_generation: u64,
        content_hash: String,
    },
    /// A document projection written by agent-doc reached the filesystem.
    ///
    /// This durable fact replaces the former `.agent-doc/write-provenance`
    /// sidecar. The generation makes replay monotonic even when events are
    /// duplicated or delivered out of order.
    DocumentDiskWriteObserved {
        document_hash: String,
        generation: u64,
        content_len: u64,
        content_hash: String,
        write_id: String,
        actor: String,
    },
    DocumentAuthorityObserved {
        document_hash: String,
        authority: DocumentAuthority,
        authority_epoch: u64,
        source: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editor_id: Option<String>,
    },
    DocumentWriteDeferred {
        document_hash: String,
        intent_id: String,
        expected_hash: String,
        /// Full canonical/editor content the target was derived from. Older
        /// events may omit this field; reconnect recovery can use disk only
        /// when it still proves `expected_hash` in that compatibility case.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_content: Option<String>,
        target_hash: String,
        target_content: String,
        source: DocumentWriteSource,
        reason: DocumentWriteDeferredReason,
    },
    DocumentWriteConverged {
        document_hash: String,
        intent_id: String,
        target_hash: String,
        /// Free-text tag for the *caller that cleared* the intent — a diagnostic
        /// actor label, deliberately not the intent's discriminant.
        source: String,
        /// The converged intent's own discriminant (`#adwritesourceenum`).
        ///
        /// Kept separate from `source` because they answer different questions:
        /// `source` is "who settled this", `intent_source` is "which closeout
        /// stage's write this was". Only the latter can be ordered. Older events
        /// omit it and deserialize as `Unknown("")`, which carries no stage and
        /// therefore never supersedes anything.
        #[serde(default)]
        intent_source: DocumentWriteSource,
    },
    QueueHeadSelected {
        document_hash: String,
        node_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backlog_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_text: Option<String>,
        drainable: bool,
        /// Hosting epoch this queue fact was produced under (`#xdocsuper3`). When
        /// present and behind the document's current hosting epoch the fact is
        /// rejected as a stale-overlay replay; `None` keeps legacy/un-hosted
        /// producers backward compatible (accepted at any epoch).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hosting_epoch: Option<u64>,
    },
    QueueHeadDeferred {
        document_hash: String,
        node_key: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hosting_epoch: Option<u64>,
    },
    QueueHeadCompleted {
        document_hash: String,
        node_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backlog_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hosting_epoch: Option<u64>,
    },
    QueueWorklistProjected {
        document_hash: String,
        queue_hash: String,
        entries: Vec<QueueWorklistEntry>,
        active: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hosting_epoch: Option<u64>,
    },
    /// A supervisor-owned `/clear` or equivalent context-clear command is
    /// pending/settling for this document. The CP queue projection is the
    /// durable authority for this gate.
    QueueContextClearStarted {
        document_hash: String,
        file: String,
        target: String,
        harness: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_sha256: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_bytes: Option<usize>,
        clear_epoch: u64,
        marked_secs: u64,
    },
    /// The supervisor-owned context-clear window settled; queue drains may resume
    /// once this fact is folded into the CP graph.
    QueueContextClearSettled {
        document_hash: String,
        file: String,
        target: String,
        harness: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        clear_epoch: u64,
        marked_secs: u64,
    },
    /// An explicit operator clear was deferred while a queue-owned pane was busy;
    /// the supervisor idle watch should submit it at the next dispatch-ready gap.
    QueueContextClearDeferred {
        document_hash: String,
        file: String,
        target: String,
        harness: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_sha256: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_bytes: Option<usize>,
        clear_epoch: u64,
        marked_secs: u64,
    },
    /// A clean closeout still required queue continuation; the next preflight
    /// reconciles this one-shot queue-stall signal.
    QueueDrainStallContinuationRecorded {
        document_hash: String,
        file: String,
        cycle_id: String,
        stall_epoch: u64,
        recorded_secs: u64,
    },
    /// The one-shot continuation-pending queue-stall signal was reconciled or
    /// superseded by active supervisor drain progress.
    QueueDrainStallContinuationCleared {
        document_hash: String,
        file: String,
        stall_epoch: u64,
        cleared_secs: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A route-owned supervisor process begins hosting (or switches to) a
    /// document on a tmux pane/session (`#xdocsuper1`/`#xdocsuper3`).
    ///
    /// This is the pane→document host binding modeled as a backbone transition.
    /// When the `pane_session` hosting this document changes, or the same pane
    /// re-hosts at a higher `lease_epoch`, the projection treats it as a fresh
    /// host/switch: the document's stale per-document queue overlay is dropped
    /// and the document's hosting epoch advances, so any queue fact carrying an
    /// older `hosting_epoch` becomes a no-op by construction.
    SupervisorHosting {
        document_hash: String,
        pane_session: String,
        lease_epoch: u64,
    },
    /// Project-scoped supervisor recycle/restart was REQUESTED (stale binary,
    /// `admin recycle`, install fan-out, or a wedge trigger) but has not begun.
    /// The CP owns this pending-intent fact on the Lazily statechart so route
    /// callers and the editor projection see the pending restart durably.
    SupervisorRecycleRequested {
        document_hash: String,
        reason: String,
        recycle_epoch: u64,
        marked_secs: u64,
    },
    /// Project-scoped supervisor recycle began. Route callers wait on this
    /// Lazily state transition.
    SupervisorRecycleStarted {
        document_hash: String,
        reason: String,
        recycle_epoch: u64,
        marked_secs: u64,
    },
    /// Project-scoped supervisor recycle settled; route callers may inject
    /// triggers again once this fact is folded into the CP graph.
    SupervisorRecycleSettled {
        document_hash: String,
        reason: String,
        recycle_epoch: u64,
        marked_secs: u64,
    },
    /// Route input delivery is active for this document. This replaces the
    /// old route-submit marker files with a controller-backed route projection.
    RouteSubmitStarted {
        document_hash: String,
        pane_id: String,
        harness: String,
        reason: String,
        submit_epoch: u64,
        marked_secs: u64,
    },
    /// Route input delivery has settled, either through observed acceptance or
    /// by leaving the protected pane-input window.
    RouteSubmitSettled {
        document_hash: String,
        pane_id: String,
        harness: String,
        reason: String,
        submit_epoch: u64,
        marked_secs: u64,
    },
    /// Route input was accepted but did not produce dispatch-start proof; idle
    /// queue draining remains suppressed for the bounded blocked window.
    RouteSubmitBlocked {
        document_hash: String,
        pane_id: String,
        harness: String,
        reason: String,
        submit_epoch: u64,
        marked_secs: u64,
    },
    ResponseCaptured {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
        response_sha256: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_body: Option<String>,
        /// Full replayable turn payload, including typed component mutations.
        /// The canonical response body remains separate for materialization proof.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent_body: Option<String>,
        /// Sanitized, replayable typed closeout mutations (backlog, icebox,
        /// review, queue, and status). Transport overrides and sibling commits
        /// are deliberately excluded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mutation_plan_json: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_hash: Option<String>,
        /// Full editor-visible document content used as the response replay
        /// baseline. Hashes remain useful indexes, but recovery must retain
        /// the bytes needed to reconcile a partially materialized response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        baseline_content: Option<String>,
    },
    /// One Lazily-authoritative owner may advance a closeout cycle at a time.
    /// The claim expires unless the owner refreshes it; an exact release cannot
    /// clear a newer takeover claim.
    CloseoutOwnerClaimed {
        document_hash: String,
        cycle_id: String,
        owner_id: String,
        owner_pid: u32,
        role: String,
        claimed_secs: u64,
        expires_secs: u64,
    },
    CloseoutOwnerReleased {
        document_hash: String,
        cycle_id: String,
        owner_id: String,
        reason: String,
        released_secs: u64,
    },
    /// Retire one content-bearing response projection after its materialization
    /// has been durably superseded (for example by compact archive placement).
    CapturedResponseRetired {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
        reason: String,
    },
    /// Reactivate the exact retired payload after false-stale proof succeeds.
    CapturedResponseReactivated {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
    },
    /// Latest streamed response draft for an open turn. Draft checkpoints are
    /// durable ledger facts, not filesystem sidecars, and do not advance the
    /// closeout phase.
    ResponseDraftCheckpointed {
        document_hash: String,
        cycle_id: String,
        checkpoint_id: String,
        checkpoint_count: u64,
        response_sha256: String,
        response_body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_hash: Option<String>,
    },
    ResponseDraftCleared {
        document_hash: String,
        cycle_id: String,
        reason: String,
    },
    ResponseReplayObserved {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
    },
    WriteApplied {
        document_hash: String,
        cycle_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patch_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_hash: Option<String>,
    },
    /// One body-aware assistant response cell was applied to the canonical
    /// realtime document. This is both the durable CRDT operation receipt and
    /// the closeout `write_applied` transition; no editor-visible patch receipt
    /// is required to rediscover it after a restart.
    ResponseCellAdded {
        document_hash: String,
        cycle_id: String,
        operation_id: String,
        cell_id: String,
        response_sha256: String,
        content_hash: String,
        applied: bool,
    },
    CommitObserved {
        document_hash: String,
        cycle_id: String,
        commit: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_hash: Option<String>,
    },
    SessionCheckPassed {
        document_hash: String,
        cycle_id: String,
    },
    CycleAbandoned {
        document_hash: String,
        cycle_id: String,
        reason: String,
    },
    /// The exact retained response proved that a narrowly classified capture
    /// retirement was false. This is the only event allowed to reopen that
    /// terminal projection, and it carries the capture identity plus the
    /// abandonment reason that must match the prior state.
    FalseStaleCaptureReactivated {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
        response_sha256: String,
        retirement_reason: String,
    },
    DocumentCellMergeAckRecorded {
        document_hash: String,
        cycle_id: String,
        component: String,
        id: String,
        reason: String,
        detail: String,
    },
    DocumentCellMergeAckCarriedForward {
        document_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_cycle_id: Option<String>,
        target_cycle_id: String,
        component: String,
        id: String,
        reason: String,
        detail: String,
    },
    OwnerGenerationChanged {
        document_hash: String,
        owner: StateOwner,
        generation: u64,
    },
    EditorPatchQueued {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
    },
    EditorPatchApplied {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
    },
    EditorPatchRejected {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
        reason: String,
    },
    VisibleWriteCommitCandidateObserved {
        document_hash: String,
        patch_id: String,
        model_revision: u64,
        editor_visible_hash: String,
        commit_candidate_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit_candidate_content: Option<String>,
        source: String,
    },
    VisibleWriteMaterializedCarryForwardObserved {
        document_hash: String,
        model_revision: u64,
        live_buffer_hash: String,
        file_content_hash: String,
        commit_candidate_hash: String,
        source: String,
    },
    IpcProofInsufficient {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
        reason: String,
    },
    EditorPatchRetryRequested {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
        reason: String,
    },
    ForceDiskFallbackRecorded {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
        reason: String,
    },
    TerminalCloseoutProofRecorded {
        document_hash: String,
        cycle_id: String,
        last_event: String,
        did_commit: bool,
        file_hash: String,
        snapshot_hash: String,
        head_hash: String,
        state_file_hash_matches: bool,
        state_snapshot_hash_matches: bool,
        agreement: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_sha256: Option<String>,
        recorded_at_ms: u64,
    },
    CloseoutRecoveryEvidenceRecorded {
        document_hash: String,
        evidence_key: String,
        visible_markdown_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_cycle_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_cycle_phase: Option<CyclePhase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_capture_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_capture_cycle_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_capture_state: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_capture_response_sha256: Option<String>,
        response_body: CloseoutRecoveryResponseBodyEvidence,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queue_only_drift: Option<CloseoutRecoveryQueueOnlyDriftEvidence>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_head_drift: Option<CloseoutRecoveryDriftEvidence>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_visible_drift: Option<CloseoutRecoveryDriftEvidence>,
        editor_ipc: CloseoutRecoveryEditorIpcEvidence,
        binary_freshness: CloseoutRecoveryBinaryFreshnessEvidence,
        recorded_at_ms: u64,
    },
    ActorLifecycleObserved {
        document_hash: String,
        owner: StateOwner,
        generation: u64,
        event: ActorLifecycleEvent,
    },
    AgentRestartPerformed {
        document_hash: String,
        owner: StateOwner,
        generation: u64,
    },
    CapabilityProofObserved {
        document_hash: String,
        owner: StateOwner,
        generation: u64,
        capability: String,
    },
    StartingActorTimeoutRecorded {
        document_hash: String,
        pane_id: String,
        generation: u64,
        log_line: String,
    },
    StartingActorTimeoutCleared {
        document_hash: String,
        pane_id: String,
        generation: u64,
    },
    /// Durable ownership miss for a routed session start. This replaces the
    /// former per-document startup-miss JSON marker.
    StartupMissRecorded {
        document_hash: String,
        file: String,
        pane_id: String,
        session_id: String,
        harness: String,
        timestamp: u64,
        origin: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cycle_baseline_id: Option<String>,
    },
    /// Identity-CAS clear for the current startup miss. A delayed clear cannot
    /// erase a newer miss for another pane/session.
    StartupMissCleared {
        document_hash: String,
        pane_id: String,
        session_id: String,
        timestamp: u64,
    },
    RoutePaneObserved {
        document_hash: String,
        pane_id: String,
        actor_generation: u64,
    },
    RouteReadinessObserved {
        document_hash: String,
        actor_generation: u64,
        event: RouteReadinessEvent,
    },
    DispatchProofObserved {
        document_hash: String,
        actor_generation: u64,
        proof_id: String,
    },
    ProofMarkerObserved {
        document_hash: String,
        marker: String,
        source: String,
    },
    ProofMarkerDisproved {
        document_hash: String,
        marker: String,
        source: String,
    },
}

impl StateFact {
    pub fn document_hash(&self) -> &str {
        match self {
            Self::PreflightStarted { document_hash, .. }
            | Self::TurnIntentCheckpointed { document_hash, .. }
            | Self::RealtimeSteeringObserved { document_hash, .. }
            | Self::BaselineSaved { document_hash, .. }
            | Self::DocumentBaselineCheckpointed { document_hash, .. }
            | Self::DocumentBaselineCleared { document_hash, .. }
            | Self::UndoCheckpointed { document_hash, .. }
            | Self::UndoCheckpointCleared { document_hash, .. }
            | Self::CrdtRecoveryProjectionCheckpointed { document_hash, .. }
            | Self::CrdtRecoveryProjectionCleared { document_hash, .. }
            | Self::FileWatchChangeObserved { document_hash, .. }
            | Self::DocumentDiskWriteObserved { document_hash, .. }
            | Self::DocumentAuthorityObserved { document_hash, .. }
            | Self::DocumentWriteDeferred { document_hash, .. }
            | Self::DocumentWriteConverged { document_hash, .. }
            | Self::QueueHeadSelected { document_hash, .. }
            | Self::QueueHeadDeferred { document_hash, .. }
            | Self::QueueHeadCompleted { document_hash, .. }
            | Self::QueueWorklistProjected { document_hash, .. }
            | Self::QueueContextClearStarted { document_hash, .. }
            | Self::QueueContextClearSettled { document_hash, .. }
            | Self::QueueContextClearDeferred { document_hash, .. }
            | Self::QueueDrainStallContinuationRecorded { document_hash, .. }
            | Self::QueueDrainStallContinuationCleared { document_hash, .. }
            | Self::ResponseCaptured { document_hash, .. }
            | Self::CloseoutOwnerClaimed { document_hash, .. }
            | Self::CloseoutOwnerReleased { document_hash, .. }
            | Self::CapturedResponseRetired { document_hash, .. }
            | Self::CapturedResponseReactivated { document_hash, .. }
            | Self::ResponseDraftCheckpointed { document_hash, .. }
            | Self::ResponseDraftCleared { document_hash, .. }
            | Self::ResponseReplayObserved { document_hash, .. }
            | Self::WriteApplied { document_hash, .. }
            | Self::ResponseCellAdded { document_hash, .. }
            | Self::CommitObserved { document_hash, .. }
            | Self::SessionCheckPassed { document_hash, .. }
            | Self::CycleAbandoned { document_hash, .. }
            | Self::FalseStaleCaptureReactivated { document_hash, .. }
            | Self::DocumentCellMergeAckRecorded { document_hash, .. }
            | Self::DocumentCellMergeAckCarriedForward { document_hash, .. }
            | Self::OwnerGenerationChanged { document_hash, .. }
            | Self::EditorPatchQueued { document_hash, .. }
            | Self::EditorPatchApplied { document_hash, .. }
            | Self::EditorPatchRejected { document_hash, .. }
            | Self::VisibleWriteCommitCandidateObserved { document_hash, .. }
            | Self::VisibleWriteMaterializedCarryForwardObserved { document_hash, .. }
            | Self::IpcProofInsufficient { document_hash, .. }
            | Self::EditorPatchRetryRequested { document_hash, .. }
            | Self::ForceDiskFallbackRecorded { document_hash, .. }
            | Self::TerminalCloseoutProofRecorded { document_hash, .. }
            | Self::CloseoutRecoveryEvidenceRecorded { document_hash, .. }
            | Self::ActorLifecycleObserved { document_hash, .. }
            | Self::AgentRestartPerformed { document_hash, .. }
            | Self::CapabilityProofObserved { document_hash, .. }
            | Self::StartingActorTimeoutRecorded { document_hash, .. }
            | Self::StartingActorTimeoutCleared { document_hash, .. }
            | Self::StartupMissRecorded { document_hash, .. }
            | Self::StartupMissCleared { document_hash, .. }
            | Self::RoutePaneObserved { document_hash, .. }
            | Self::RouteReadinessObserved { document_hash, .. }
            | Self::DispatchProofObserved { document_hash, .. }
            | Self::RouteSubmitStarted { document_hash, .. }
            | Self::RouteSubmitSettled { document_hash, .. }
            | Self::RouteSubmitBlocked { document_hash, .. }
            | Self::ProofMarkerObserved { document_hash, .. }
            | Self::ProofMarkerDisproved { document_hash, .. }
            | Self::SupervisorHosting { document_hash, .. }
            | Self::SupervisorRecycleRequested { document_hash, .. }
            | Self::SupervisorRecycleStarted { document_hash, .. }
            | Self::SupervisorRecycleSettled { document_hash, .. } => document_hash,
        }
    }

    pub fn domain(&self) -> StateDomain {
        match self {
            Self::PreflightStarted { .. }
            | Self::TurnIntentCheckpointed { .. }
            | Self::RealtimeSteeringObserved { .. }
            | Self::ResponseCaptured { .. }
            | Self::CloseoutOwnerClaimed { .. }
            | Self::CloseoutOwnerReleased { .. }
            | Self::CapturedResponseRetired { .. }
            | Self::CapturedResponseReactivated { .. }
            | Self::ResponseDraftCheckpointed { .. }
            | Self::ResponseDraftCleared { .. }
            | Self::ResponseReplayObserved { .. }
            | Self::WriteApplied { .. }
            | Self::ResponseCellAdded { .. }
            | Self::CommitObserved { .. }
            | Self::SessionCheckPassed { .. }
            | Self::CycleAbandoned { .. }
            | Self::FalseStaleCaptureReactivated { .. }
            | Self::DocumentCellMergeAckRecorded { .. }
            | Self::DocumentCellMergeAckCarriedForward { .. } => StateDomain::Closeout,
            Self::BaselineSaved { .. }
            | Self::DocumentBaselineCheckpointed { .. }
            | Self::DocumentBaselineCleared { .. }
            | Self::UndoCheckpointed { .. }
            | Self::UndoCheckpointCleared { .. }
            | Self::CrdtRecoveryProjectionCheckpointed { .. }
            | Self::CrdtRecoveryProjectionCleared { .. }
            | Self::FileWatchChangeObserved { .. }
            | Self::DocumentDiskWriteObserved { .. }
            | Self::DocumentAuthorityObserved { .. }
            | Self::DocumentWriteDeferred { .. }
            | Self::DocumentWriteConverged { .. } => StateDomain::Document,
            Self::QueueHeadSelected { .. }
            | Self::QueueHeadDeferred { .. }
            | Self::QueueHeadCompleted { .. }
            | Self::QueueWorklistProjected { .. }
            | Self::QueueContextClearStarted { .. }
            | Self::QueueContextClearSettled { .. }
            | Self::QueueContextClearDeferred { .. }
            | Self::QueueDrainStallContinuationRecorded { .. }
            | Self::QueueDrainStallContinuationCleared { .. } => StateDomain::Queue,
            Self::EditorPatchQueued { .. }
            | Self::EditorPatchApplied { .. }
            | Self::EditorPatchRejected { .. }
            | Self::VisibleWriteCommitCandidateObserved { .. }
            | Self::VisibleWriteMaterializedCarryForwardObserved { .. }
            | Self::IpcProofInsufficient { .. }
            | Self::EditorPatchRetryRequested { .. }
            | Self::ForceDiskFallbackRecorded { .. } => StateDomain::Transport,
            Self::OwnerGenerationChanged { .. }
            | Self::ActorLifecycleObserved { .. }
            | Self::AgentRestartPerformed { .. }
            | Self::CapabilityProofObserved { .. }
            | Self::SupervisorHosting { .. }
            | Self::SupervisorRecycleRequested { .. }
            | Self::SupervisorRecycleStarted { .. }
            | Self::SupervisorRecycleSettled { .. }
            | Self::StartupMissRecorded { .. }
            | Self::StartupMissCleared { .. } => StateDomain::Supervisor,
            Self::StartingActorTimeoutRecorded { .. }
            | Self::StartingActorTimeoutCleared { .. }
            | Self::RoutePaneObserved { .. }
            | Self::RouteReadinessObserved { .. }
            | Self::DispatchProofObserved { .. }
            | Self::RouteSubmitStarted { .. }
            | Self::RouteSubmitSettled { .. }
            | Self::RouteSubmitBlocked { .. } => StateDomain::Route,
            Self::ProofMarkerObserved { .. }
            | Self::ProofMarkerDisproved { .. }
            | Self::TerminalCloseoutProofRecorded { .. }
            | Self::CloseoutRecoveryEvidenceRecorded { .. } => StateDomain::Proof,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::PreflightStarted { .. } => "preflight_started",
            Self::TurnIntentCheckpointed { .. } => "turn_intent_checkpointed",
            Self::RealtimeSteeringObserved { .. } => "realtime_steering_observed",
            Self::BaselineSaved { .. } => "baseline_saved",
            Self::DocumentBaselineCheckpointed { .. } => "document_baseline_checkpointed",
            Self::DocumentBaselineCleared { .. } => "document_baseline_cleared",
            Self::UndoCheckpointed { .. } => "undo_checkpointed",
            Self::UndoCheckpointCleared { .. } => "undo_checkpoint_cleared",
            Self::CrdtRecoveryProjectionCheckpointed { .. } => {
                "crdt_recovery_projection_checkpointed"
            }
            Self::CrdtRecoveryProjectionCleared { .. } => "crdt_recovery_projection_cleared",
            Self::FileWatchChangeObserved { .. } => "file_watch_change_observed",
            Self::DocumentDiskWriteObserved { .. } => "document_disk_write_observed",
            Self::DocumentAuthorityObserved { .. } => "document_authority_observed",
            Self::DocumentWriteDeferred { .. } => "document_write_deferred",
            Self::DocumentWriteConverged { .. } => "document_write_converged",
            Self::QueueHeadSelected { .. } => "queue_head_selected",
            Self::QueueHeadDeferred { .. } => "queue_head_deferred",
            Self::QueueHeadCompleted { .. } => "queue_head_completed",
            Self::QueueWorklistProjected { .. } => "queue_worklist_projected",
            Self::QueueContextClearStarted { .. } => "queue_context_clear_started",
            Self::QueueContextClearSettled { .. } => "queue_context_clear_settled",
            Self::QueueContextClearDeferred { .. } => "queue_context_clear_deferred",
            Self::QueueDrainStallContinuationRecorded { .. } => {
                "queue_drain_stall_continuation_recorded"
            }
            Self::QueueDrainStallContinuationCleared { .. } => {
                "queue_drain_stall_continuation_cleared"
            }
            Self::SupervisorHosting { .. } => "supervisor_hosting",
            Self::ResponseCaptured { .. } => "response_captured",
            Self::CloseoutOwnerClaimed { .. } => "closeout_owner_claimed",
            Self::CloseoutOwnerReleased { .. } => "closeout_owner_released",
            Self::CapturedResponseRetired { .. } => "captured_response_retired",
            Self::CapturedResponseReactivated { .. } => "captured_response_reactivated",
            Self::ResponseDraftCheckpointed { .. } => "response_draft_checkpointed",
            Self::ResponseDraftCleared { .. } => "response_draft_cleared",
            Self::ResponseReplayObserved { .. } => "response_replay_observed",
            Self::WriteApplied { .. } => "write_applied",
            Self::ResponseCellAdded { .. } => "response_cell_added",
            Self::CommitObserved { .. } => "commit_observed",
            Self::SessionCheckPassed { .. } => "session_check_passed",
            Self::CycleAbandoned { .. } => "cycle_abandoned",
            Self::FalseStaleCaptureReactivated { .. } => "false_stale_capture_reactivated",
            Self::DocumentCellMergeAckRecorded { .. } => "document_cell_merge_ack_recorded",
            Self::DocumentCellMergeAckCarriedForward { .. } => {
                "document_cell_merge_ack_carried_forward"
            }
            Self::OwnerGenerationChanged { .. } => "owner_generation_changed",
            Self::EditorPatchQueued { .. } => "editor_patch_queued",
            Self::EditorPatchApplied { .. } => "editor_patch_applied",
            Self::EditorPatchRejected { .. } => "editor_patch_rejected",
            Self::VisibleWriteCommitCandidateObserved { .. } => {
                "visible_write_commit_candidate_observed"
            }
            Self::VisibleWriteMaterializedCarryForwardObserved { .. } => {
                "visible_write_materialized_carry_forward_observed"
            }
            Self::IpcProofInsufficient { .. } => "ipc_proof_insufficient",
            Self::EditorPatchRetryRequested { .. } => "editor_patch_retry_requested",
            Self::ForceDiskFallbackRecorded { .. } => "force_disk_fallback_recorded",
            Self::TerminalCloseoutProofRecorded { .. } => "terminal_closeout_proof_recorded",
            Self::CloseoutRecoveryEvidenceRecorded { .. } => "closeout_recovery_evidence_recorded",
            Self::ActorLifecycleObserved { .. } => "actor_lifecycle_observed",
            Self::AgentRestartPerformed { .. } => "agent_restart_performed",
            Self::CapabilityProofObserved { .. } => "capability_proof_observed",
            Self::StartingActorTimeoutRecorded { .. } => "starting_actor_timeout_recorded",
            Self::StartingActorTimeoutCleared { .. } => "starting_actor_timeout_cleared",
            Self::StartupMissRecorded { .. } => "startup_miss_recorded",
            Self::StartupMissCleared { .. } => "startup_miss_cleared",
            Self::RoutePaneObserved { .. } => "route_pane_observed",
            Self::RouteReadinessObserved { .. } => "route_readiness_observed",
            Self::DispatchProofObserved { .. } => "dispatch_proof_observed",
            Self::RouteSubmitStarted { .. } => "route_submit_started",
            Self::RouteSubmitSettled { .. } => "route_submit_settled",
            Self::RouteSubmitBlocked { .. } => "route_submit_blocked",
            Self::ProofMarkerObserved { .. } => "proof_marker_observed",
            Self::ProofMarkerDisproved { .. } => "proof_marker_disproved",
            Self::SupervisorRecycleRequested { .. } => "supervisor_recycle_requested",
            Self::SupervisorRecycleStarted { .. } => "supervisor_recycle_started",
            Self::SupervisorRecycleSettled { .. } => "supervisor_recycle_settled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventLedger {
    events: Vec<StateEvent>,
}

impl EventLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, event: StateEvent) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[StateEvent] {
        &self.events
    }

    pub fn project(&self) -> StateBackboneProjection {
        StateBackboneProjection::from_events(self.events())
    }

    /// Return the globally-deduped accepted event stream in append order.
    ///
    /// The ledger is append-only and may carry duplicate `event_id`s (re-emits,
    /// CRDT/supervisor replays). The projection's `seen_event_ids` absorbs those
    /// during a global fold; this helper exposes the same deduped subsequence so
    /// per-document epoch counts and bounded replay stay consistent with
    /// [`StateBackboneProjection::from_events`].
    pub fn accepted_events(&self) -> Vec<&StateEvent> {
        let mut seen = BTreeSet::new();
        self.events
            .iter()
            .filter(|event| seen.insert(event.event_id.clone()))
            .collect()
    }

    /// Monotonic per-document epoch = number of accepted (deduped) events that
    /// target `document_hash`. A re-emit does not bump the epoch (idempotent ⇒
    /// no-op delta). This is the `lazily-spec` `epoch` for the document's state
    /// graph (`#lazilystatesync2`).
    pub fn document_epoch(&self, document_hash: &str) -> u64 {
        self.accepted_events()
            .iter()
            .filter(|event| event.document_hash() == document_hash)
            .count() as u64
    }

    /// The document's current hosting epoch (`#xdocsuper3`), or `None` when the
    /// document has no `SupervisorHosting` fact yet. Live queue-fact producers
    /// (supervisor host loop / FFI) call this and stamp the returned value onto
    /// new `QueueHead*` facts so a later host/switch makes them stale.
    pub fn document_hosting_epoch(&self, document_hash: &str) -> Option<u64> {
        self.project_document(document_hash)
            .and_then(|projection| projection.hosting_epoch())
    }

    /// Project the current state for a single document from the deduped event
    /// stream. Returns `None` when no accepted event targets the document.
    pub fn project_document(&self, document_hash: &str) -> Option<DocumentStateProjection> {
        let accepted: Vec<&StateEvent> = self
            .accepted_events()
            .into_iter()
            .filter(|event| event.document_hash() == document_hash)
            .collect();
        if accepted.is_empty() {
            return None;
        }
        let mut projection = DocumentStateProjection::new(document_hash);
        for event in accepted {
            projection.apply_fact(&event.fact);
        }
        Some(projection)
    }

    /// Project the document state as it stood after the first `epoch` accepted
    /// events for that document were applied. Used to derive deltas since a
    /// caller's `last_epoch` (`#lazilystatesync2`). `epoch` is clamped to the
    /// current document epoch.
    pub fn project_document_at_epoch(
        &self,
        document_hash: &str,
        epoch: u64,
    ) -> DocumentStateProjection {
        let mut projection = DocumentStateProjection::new(document_hash);
        let mut accepted_for_doc: u64 = 0;
        for event in self.accepted_events() {
            if event.document_hash() != document_hash {
                continue;
            }
            if accepted_for_doc >= epoch {
                break;
            }
            projection.apply_fact(&event.fact);
            accepted_for_doc += 1;
        }
        projection
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StateBackboneProjection {
    pub documents: BTreeMap<String, DocumentStateProjection>,
    #[serde(skip)]
    seen_event_ids: BTreeSet<String>,
}

impl StateBackboneProjection {
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a StateEvent>) -> Self {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event);
        }
        projection
    }

    pub fn document(&self, document_hash: &str) -> Option<&DocumentStateProjection> {
        self.documents.get(document_hash)
    }

    pub fn project_supervisor_recycle(&self) -> SupervisorRecycleProjection {
        self.document(PROJECT_SUPERVISOR_DOCUMENT_HASH)
            .map(|document| document.supervisor.recycle.clone())
            .unwrap_or_default()
    }

    pub fn apply(&mut self, event: &StateEvent) {
        if !self.seen_event_ids.insert(event.event_id.clone()) {
            return;
        }
        let document = self
            .documents
            .entry(event.document_hash().to_string())
            .or_insert_with(|| DocumentStateProjection::new(event.document_hash()));
        document.apply(&event.fact);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentStateProjection {
    pub document_hash: String,
    pub document: DocumentProjection,
    pub queue: QueueProjection,
    pub closeout: CloseoutProjection,
    pub transport: TransportProjection,
    pub visible_write: VisibleWriteProjection,
    pub supervisor: SupervisorProjection,
    pub route: RouteProjection,
    pub proof: ProofProjection,
    /// Pane→document host binding for this document (`#xdocsuper1`/`#xdocsuper3`).
    /// Tracks which `pane_session` currently hosts the document and the hosting
    /// epoch that gates the queue overlay. `None` until the first
    /// `SupervisorHosting` fact is observed (legacy/un-hosted documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosting: Option<SupervisorHostingProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_stale_events: Vec<RejectedStaleEvent>,
}

impl DocumentStateProjection {
    /// Return the durable document-write effect that still owns the current
    /// captured response, if any.
    ///
    /// The effect target and the content-bearing capture already provide the
    /// correlation needed here. Keeping that relationship in the live Lazily
    /// projection avoids a second capture journal or a recovery-time SQLite
    /// scan.
    /// Whether `intent`'s target is already the observed authority content.
    ///
    /// Convergence is **derived**, not remembered. `DocumentWriteConverged` is an
    /// event some code path has to emit, and a path that commits without emitting it
    /// leaves the journal entry alive forever — observed 2026-07-25, where a cycle
    /// committed successfully (disk == live == HEAD) yet the retained entry survived
    /// and blocked *every* subsequent cycle for the document, while `doctor`,
    /// `session-check`, and `repair` all reported clean.
    ///
    /// Comparing the intent's `target_hash` against the authority's observed
    /// `content_hash` answers the same question from state that already exists, so a
    /// missed emission self-heals on the next read instead of wedging permanently.
    /// The durable journal prune stays an effect gated on this signal
    /// (`#lzdurablesink`) — this is the fact, not the sink write.
    ///
    /// Equality here means the authority literally holds the target content, which is
    /// exactly what "the write landed" means, so treating it as converged cannot
    /// discard a response that is not actually present.
    pub fn write_intent_converged(&self, intent: &DocumentWriteIntentProjection) -> bool {
        self.document
            .latest_authority
            .as_ref()
            .and_then(|authority| authority.content_hash.as_deref())
            .is_some_and(|observed| observed == intent.target_hash)
    }

    pub fn retained_captured_response_write(&self) -> Option<&DocumentWriteIntentProjection> {
        let capture = self.closeout.captured_response.as_ref()?;
        let retains_capture = |pending: &&DocumentWriteIntentProjection| {
            agent_doc_turn::response_replay::response_materialized_in_content(
                &capture.response_body,
                &pending.target_content,
            ) && !self.write_intent_converged(pending)
        };

        self.document
            .pending_write_journal
            .iter()
            .find(retains_capture)
            .or_else(|| self.document.pending_write.as_ref().filter(retains_capture))
    }

    /// Construct an empty projection for `document_hash`. Public so the wire
    /// delta derivation (`state_wire`) can build the cold/empty projection used
    /// when a document has no accepted events yet (`#lazilystatesync2`).
    pub fn new(document_hash: &str) -> Self {
        Self {
            document_hash: document_hash.to_string(),
            document: DocumentProjection::default(),
            queue: QueueProjection::default(),
            closeout: CloseoutProjection::default(),
            transport: TransportProjection::default(),
            visible_write: VisibleWriteProjection::default(),
            supervisor: SupervisorProjection::default(),
            route: RouteProjection::default(),
            proof: ProofProjection::default(),
            hosting: None,
            rejected_stale_events: Vec::new(),
        }
    }

    pub fn projection_summary(&self) -> ProjectionSummary {
        ProjectionSummary::from_document(self)
    }

    /// The document's current hosting epoch (`#xdocsuper3`), or `None` when no
    /// `SupervisorHosting` fact has been observed yet. Producers stamp new queue
    /// facts with this value so a later host/switch makes them stale.
    pub fn hosting_epoch(&self) -> Option<u64> {
        self.hosting.as_ref().map(|hosting| hosting.hosting_epoch)
    }

    pub fn applied_visible_write_candidate(
        &self,
        commit_candidate_hash: &str,
    ) -> Option<&VisibleWriteCommitCandidateProjection> {
        self.visible_write
            .applied_candidate(&self.transport, commit_candidate_hash)
    }

    pub fn applied_visible_write_candidate_for_patch(
        &self,
        patch_id: &str,
    ) -> Option<&VisibleWriteCommitCandidateProjection> {
        self.visible_write
            .applied_candidate_for_patch(&self.transport, patch_id)
    }

    pub fn materialized_visible_write_carry_forward(
        &self,
        commit_candidate_hash: &str,
        file_content_hash: &str,
        live_buffer_hash: &str,
    ) -> Option<&VisibleWriteMaterializedCarryForwardProjection> {
        self.visible_write.materialized_carry_forward(
            commit_candidate_hash,
            file_content_hash,
            live_buffer_hash,
        )
    }

    /// Apply a single state fact to this document projection. Public so the
    /// wire/delta derivation (`state_wire`) can replay a bounded slice of the
    /// accepted event stream into a fresh projection without going through the
    /// backbone's global dedup map (`#lazilystatesync2`).
    pub fn apply_fact(&mut self, fact: &StateFact) {
        self.apply(fact);
    }

    fn apply(&mut self, fact: &StateFact) {
        match fact {
            StateFact::TurnIntentCheckpointed {
                cycle_id,
                checkpoint_sequence,
                state_sha256,
                state_json,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() != Some(cycle_id)
                    && self.retained_captured_response_write().is_some()
                {
                    // A new preflight checkpoint must not hide the exact
                    // capture owned by a still-pending document-write effect.
                    // Replaying the ledger after a controller restart must
                    // reach the same decision.
                    self.reject_stale(StateDomain::Closeout, StateOwner::DocumentWriter);
                } else {
                    self.closeout.turn_intent_checkpoint = Some(TurnIntentCheckpointProjection {
                        cycle_id: cycle_id.clone(),
                        checkpoint_sequence: *checkpoint_sequence,
                        state_sha256: state_sha256.clone(),
                        state_json: state_json.clone(),
                    });
                }
            }
            StateFact::BaselineSaved {
                cycle_id,
                baseline_hash,
                baseline_path,
                ..
            } => {
                self.document.latest_baseline = Some(BaselineProjection {
                    cycle_id: cycle_id.clone(),
                    baseline_hash: baseline_hash.clone(),
                    baseline_path: baseline_path.clone(),
                });
            }
            StateFact::DocumentBaselineCheckpointed {
                generation,
                content_hash,
                content,
                ..
            } => {
                self.document.merge_baseline_generation = *generation;
                self.document.merge_baseline = Some(DocumentBaselineProjection {
                    generation: *generation,
                    content_hash: content_hash.clone(),
                    content: content.clone(),
                });
            }
            StateFact::DocumentBaselineCleared { generation, .. } => {
                self.document.merge_baseline_generation = *generation;
                self.document.merge_baseline = None;
            }
            StateFact::UndoCheckpointed {
                generation,
                content_hash,
                content,
                ..
            } => {
                self.document.undo_checkpoint_generation = *generation;
                self.document.undo_checkpoint = Some(DocumentBaselineProjection {
                    generation: *generation,
                    content_hash: content_hash.clone(),
                    content: content.clone(),
                });
            }
            StateFact::UndoCheckpointCleared { generation, .. } => {
                self.document.undo_checkpoint_generation = *generation;
                self.document.undo_checkpoint = None;
            }
            StateFact::CrdtRecoveryProjectionCheckpointed {
                generation,
                projection_sha256,
                projection_base64,
                lineage,
                ..
            } => {
                self.document.crdt_recovery_projection_generation = *generation;
                self.document.crdt_recovery_projection = Some(CrdtRecoveryProjection {
                    generation: *generation,
                    projection_sha256: projection_sha256.clone(),
                    projection_base64: projection_base64.clone(),
                    lineage: lineage.clone(),
                });
            }
            StateFact::CrdtRecoveryProjectionCleared { generation, .. } => {
                self.document.crdt_recovery_projection_generation = *generation;
                self.document.crdt_recovery_projection = None;
            }
            StateFact::FileWatchChangeObserved {
                path,
                watch_generation,
                content_hash,
                ..
            } => {
                self.document.latest_file_watch_change = Some(FileWatchChangeProjection {
                    path: path.clone(),
                    watch_generation: *watch_generation,
                    content_hash: content_hash.clone(),
                });
            }
            StateFact::DocumentDiskWriteObserved {
                generation,
                content_len,
                content_hash,
                write_id,
                actor,
                ..
            } => {
                let accept = self
                    .document
                    .latest_disk_write
                    .as_ref()
                    .is_none_or(|current| *generation > current.generation);
                if accept {
                    self.document.latest_disk_write = Some(DocumentDiskWriteProjection {
                        generation: *generation,
                        content_len: *content_len,
                        content_hash: content_hash.clone(),
                        write_id: write_id.clone(),
                        actor: actor.clone(),
                    });
                } else {
                    self.reject_stale(StateDomain::Document, StateOwner::DocumentWriter);
                }
                if self
                    .closeout
                    .response_cell
                    .as_ref()
                    .is_some_and(|cell| cell.content_hash.eq_ignore_ascii_case(content_hash))
                {
                    self.closeout
                        .prove_write_through(write_pipeline::DocumentWritePhase::DiskProjected);
                }
            }
            StateFact::DocumentAuthorityObserved {
                authority,
                authority_epoch,
                source,
                reason,
                content_hash,
                editor_id,
                ..
            } => {
                if !self.document.apply_authority(
                    *authority,
                    *authority_epoch,
                    source,
                    reason,
                    content_hash.as_deref(),
                    editor_id.as_deref(),
                ) {
                    self.reject_stale(StateDomain::Document, StateOwner::DocumentWriter);
                }
            }
            StateFact::DocumentWriteDeferred {
                intent_id,
                expected_hash,
                expected_content,
                target_hash,
                target_content,
                source,
                reason,
                ..
            } => {
                let pending = DocumentWriteIntentProjection {
                    intent_id: intent_id.clone(),
                    expected_hash: expected_hash.clone(),
                    expected_content: expected_content.clone(),
                    target_hash: target_hash.clone(),
                    target_content: target_content.clone(),
                    source: source.clone(),
                    reason: reason.clone(),
                    ordinal: self.document.next_write_fact_ordinal(),
                };
                if *reason == DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor {
                    // External file-cache conflicts and agent-owned response
                    // delivery are independent durable lineages. One must
                    // never replace or clear the other.
                    self.document.pending_external_disk = Some(pending);
                } else {
                    self.document
                        .seed_pending_write_journal_from_legacy_projection();
                    if !self
                        .document
                        .pending_write_journal
                        .iter()
                        .any(|existing| existing.intent_id == pending.intent_id)
                    {
                        self.document.pending_write_journal.push(pending.clone());
                    }
                    self.document.pending_write = Some(pending);
                }
            }
            StateFact::DocumentWriteConverged {
                intent_id,
                target_hash,
                intent_source,
                ..
            } => {
                self.document.latest_converged_write = Some(ConvergedWriteProjection {
                    intent_id: intent_id.clone(),
                    target_hash: target_hash.clone(),
                    source: intent_source.clone(),
                    ordinal: self.document.next_write_fact_ordinal(),
                });
                self.document
                    .seed_pending_write_journal_from_legacy_projection();
                if let Some(index) =
                    self.document
                        .pending_write_journal
                        .iter()
                        .position(|pending| {
                            pending.intent_id == *intent_id && pending.target_hash == *target_hash
                        })
                {
                    // Each newer deferred target is composed from every older
                    // retained agent intent. Convergence of that target settles
                    // the whole included prefix while preserving any later
                    // intent that raced ahead of the ACK.
                    self.document.pending_write_journal.drain(..=index);
                    self.document.pending_write =
                        self.document.pending_write_journal.last().cloned();
                } else if self.document.pending_write.as_ref().is_some_and(|pending| {
                    pending.intent_id == *intent_id && pending.target_hash == *target_hash
                }) {
                    self.document.pending_write = None;
                }
                if self
                    .closeout
                    .response_cell
                    .as_ref()
                    .is_some_and(|cell| cell.content_hash.eq_ignore_ascii_case(target_hash))
                {
                    self.closeout
                        .prove_write_through(write_pipeline::DocumentWritePhase::DiskProjected);
                }
                if self
                    .document
                    .pending_external_disk
                    .as_ref()
                    .is_some_and(|pending| {
                        pending.intent_id == *intent_id && pending.target_hash == *target_hash
                    })
                {
                    self.document.pending_external_disk = None;
                }
            }
            StateFact::QueueHeadSelected {
                node_key,
                backlog_id,
                prompt_text,
                drainable,
                hosting_epoch,
                ..
            } => {
                if self.hosting_epoch_current(*hosting_epoch) {
                    self.queue.apply_selected(
                        node_key,
                        backlog_id.as_deref(),
                        prompt_text.as_deref(),
                        *drainable,
                    );
                } else {
                    self.reject_stale(StateDomain::Queue, StateOwner::QueueOrchestrator);
                }
            }
            StateFact::QueueHeadDeferred {
                node_key,
                reason,
                hosting_epoch,
                ..
            } => {
                if self.hosting_epoch_current(*hosting_epoch) {
                    self.queue.apply_deferred(node_key, reason);
                } else {
                    self.reject_stale(StateDomain::Queue, StateOwner::QueueOrchestrator);
                }
            }
            StateFact::QueueHeadCompleted {
                node_key,
                backlog_id,
                hosting_epoch,
                ..
            } => {
                if self.hosting_epoch_current(*hosting_epoch) {
                    self.queue.apply_completed(node_key, backlog_id.as_deref());
                } else {
                    self.reject_stale(StateDomain::Queue, StateOwner::QueueOrchestrator);
                }
            }
            StateFact::QueueWorklistProjected {
                queue_hash,
                entries,
                active,
                hosting_epoch,
                ..
            } => {
                if self.hosting_epoch_current(*hosting_epoch) {
                    self.queue.apply_worklist(queue_hash, entries, *active);
                } else {
                    self.reject_stale(StateDomain::Queue, StateOwner::QueueOrchestrator);
                }
            }
            StateFact::QueueContextClearStarted {
                file,
                target,
                harness,
                command,
                source,
                head_sha256,
                head_bytes,
                clear_epoch,
                marked_secs,
                ..
            } => {
                self.queue.apply_context_clear_event(
                    QueueContextClearEvent::Started,
                    QueueContextClearFields {
                        file,
                        target,
                        harness,
                        command,
                        source: source.as_deref(),
                        head_sha256: head_sha256.as_deref(),
                        head_bytes: *head_bytes,
                    },
                    *clear_epoch,
                    *marked_secs,
                );
            }
            StateFact::QueueContextClearSettled {
                file,
                target,
                harness,
                command,
                source,
                clear_epoch,
                marked_secs,
                ..
            } => {
                self.queue.apply_context_clear_event(
                    QueueContextClearEvent::Settled,
                    QueueContextClearFields {
                        file,
                        target,
                        harness,
                        command,
                        source: source.as_deref(),
                        head_sha256: None,
                        head_bytes: None,
                    },
                    *clear_epoch,
                    *marked_secs,
                );
            }
            StateFact::QueueContextClearDeferred {
                file,
                target,
                harness,
                command,
                source,
                head_sha256,
                head_bytes,
                clear_epoch,
                marked_secs,
                ..
            } => {
                self.queue.apply_context_clear_event(
                    QueueContextClearEvent::Deferred,
                    QueueContextClearFields {
                        file,
                        target,
                        harness,
                        command,
                        source: source.as_deref(),
                        head_sha256: head_sha256.as_deref(),
                        head_bytes: *head_bytes,
                    },
                    *clear_epoch,
                    *marked_secs,
                );
            }
            StateFact::QueueDrainStallContinuationRecorded {
                file,
                cycle_id,
                stall_epoch,
                recorded_secs,
                ..
            } => {
                self.queue.apply_drain_stall_event(
                    QueueDrainStallEvent::Recorded,
                    QueueDrainStallFields {
                        file,
                        cycle_id: Some(cycle_id),
                        reason: None,
                    },
                    *stall_epoch,
                    *recorded_secs,
                );
            }
            StateFact::QueueDrainStallContinuationCleared {
                file,
                stall_epoch,
                cleared_secs,
                reason,
                ..
            } => {
                self.queue.apply_drain_stall_event(
                    QueueDrainStallEvent::Cleared,
                    QueueDrainStallFields {
                        file,
                        cycle_id: None,
                        reason: reason.as_deref(),
                    },
                    *stall_epoch,
                    *cleared_secs,
                );
            }
            StateFact::PreflightStarted {
                cycle_id,
                session_id,
                tracked_work_maintenance_required,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() != Some(cycle_id)
                    && self.retained_captured_response_write().is_some()
                {
                    self.reject_stale(StateDomain::Closeout, StateOwner::DocumentWriter);
                } else {
                    self.closeout
                        .apply_cycle_event(cycle_id, CycleEvent::StartPreflight);
                    self.closeout.session_id = session_id.clone();
                    self.closeout.tracked_work_maintenance_required =
                        *tracked_work_maintenance_required;
                }
            }
            StateFact::RealtimeSteeringObserved {
                cycle_id, steering, ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self.closeout.phase.is_some_and(CyclePhase::is_open)
                {
                    self.closeout.realtime_steering = steering.clone();
                }
            }
            StateFact::ResponseCaptured {
                cycle_id,
                capture_id,
                response_sha256,
                response_body,
                intent_body,
                mutation_plan_json,
                file_hash,
                snapshot_hash,
                baseline_content,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::ResponseCaptured);
                self.closeout.write_phase =
                    Some(write_pipeline::DocumentWritePhase::IntentCaptured);
                self.closeout.captured_response_retired_reason = None;
                self.closeout.response_draft = None;
                self.closeout.response_draft_clear_reason =
                    Some("final_response_captured".to_string());
                self.closeout.capture_id = Some(capture_id.clone());
                self.closeout.response_sha256 = Some(response_sha256.clone());
                if file_hash.is_some() {
                    self.closeout.response_file_hash = file_hash.clone();
                }
                if snapshot_hash.is_some() {
                    self.closeout.response_snapshot_hash = snapshot_hash.clone();
                }
                if let Some(response_body) = response_body {
                    self.closeout.captured_response = Some(CapturedResponseProjection {
                        cycle_id: cycle_id.clone(),
                        capture_id: capture_id.clone(),
                        response_sha256: response_sha256.clone(),
                        response_body: response_body.clone(),
                        intent_body: intent_body.clone(),
                        mutation_plan_json: mutation_plan_json.clone(),
                        file_hash: file_hash
                            .clone()
                            .or_else(|| self.closeout.response_file_hash.clone()),
                        snapshot_hash: snapshot_hash
                            .clone()
                            .or_else(|| self.closeout.response_snapshot_hash.clone()),
                        baseline_content: baseline_content.clone(),
                    });
                    self.closeout.pending_response = Some(PendingResponseProjection {
                        cycle_id: cycle_id.clone(),
                        capture_id: capture_id.clone(),
                        response_sha256: response_sha256.clone(),
                        response_body: intent_body.clone().unwrap_or_else(|| response_body.clone()),
                    });
                }
            }
            StateFact::CloseoutOwnerClaimed {
                cycle_id,
                owner_id,
                owner_pid,
                role,
                claimed_secs,
                expires_secs,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self.closeout.phase.is_some_and(CyclePhase::is_open)
                {
                    self.closeout.owner = Some(CloseoutOwnerProjection {
                        cycle_id: cycle_id.clone(),
                        owner_id: owner_id.clone(),
                        owner_pid: *owner_pid,
                        role: role.clone(),
                        claimed_secs: *claimed_secs,
                        expires_secs: *expires_secs,
                    });
                }
            }
            StateFact::CloseoutOwnerReleased {
                cycle_id, owner_id, ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self.closeout.owner.as_ref().is_some_and(|owner| {
                        owner.cycle_id == *cycle_id && owner.owner_id == *owner_id
                    })
                {
                    self.closeout.owner = None;
                }
            }
            StateFact::CapturedResponseRetired {
                cycle_id,
                capture_id,
                reason,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self
                        .closeout
                        .captured_response
                        .as_ref()
                        .is_some_and(|capture| capture.capture_id == *capture_id)
                {
                    // Retirement removes this capture from the active closeout
                    // path, but its payload remains durable evidence. Exact
                    // false-stale recovery must be able to prove and reactivate
                    // the same response without consulting a filesystem
                    // sidecar or recapturing agent output.
                    self.closeout.captured_response_retired_reason = Some(reason.clone());
                    if self
                        .closeout
                        .pending_response
                        .as_ref()
                        .is_some_and(|pending| {
                            pending.cycle_id == *cycle_id && pending.capture_id == *capture_id
                        })
                    {
                        self.closeout.pending_response = None;
                        self.closeout.pending_response_clear_reason = Some(reason.clone());
                    }
                }
            }
            StateFact::CapturedResponseReactivated {
                cycle_id,
                capture_id,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self
                        .closeout
                        .captured_response
                        .as_ref()
                        .is_some_and(|capture| capture.capture_id == *capture_id)
                {
                    self.closeout.captured_response_retired_reason = None;
                }
            }
            StateFact::ResponseDraftCheckpointed {
                cycle_id,
                checkpoint_id,
                checkpoint_count,
                response_sha256,
                response_body,
                file_hash,
                ..
            } => {
                let should_replace = self.closeout.response_draft.as_ref().is_none_or(|draft| {
                    draft.cycle_id != *cycle_id || *checkpoint_count >= draft.checkpoint_count
                });
                if should_replace {
                    self.closeout.response_draft = Some(ResponseDraftProjection {
                        cycle_id: cycle_id.clone(),
                        checkpoint_id: checkpoint_id.clone(),
                        checkpoint_count: *checkpoint_count,
                        response_sha256: response_sha256.clone(),
                        response_body: response_body.clone(),
                        file_hash: file_hash.clone(),
                    });
                }
            }
            StateFact::ResponseDraftCleared {
                cycle_id, reason, ..
            } => {
                if self
                    .closeout
                    .response_draft
                    .as_ref()
                    .is_some_and(|draft| draft.cycle_id == *cycle_id)
                {
                    self.closeout.response_draft = None;
                    self.closeout.response_draft_clear_reason = Some(reason.clone());
                }
            }
            StateFact::ResponseReplayObserved {
                cycle_id,
                capture_id,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id)
                    && self.closeout.capture_id.as_deref() == Some(capture_id)
                {
                    self.closeout.capture_replayed = true;
                }
            }
            StateFact::WriteApplied {
                cycle_id,
                patch_id,
                file_hash,
                snapshot_hash,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::WriteApplied);
                self.closeout.patch_id = patch_id.clone();
                self.closeout
                    .prove_write_through(write_pipeline::DocumentWritePhase::CanonicalApplied);
                if patch_id.as_ref().is_some_and(|patch_id| {
                    self.transport.patch_terminal_receipt_outcome(patch_id)
                        == Some(ReceiptOutcome::Applied)
                }) {
                    self.closeout
                        .prove_write_through(write_pipeline::DocumentWritePhase::ReplicaVisible);
                }
                self.closeout
                    .clear_pending_response_for_cycle(cycle_id, "write_applied");
                self.closeout.refresh_capture_content_hashes(
                    cycle_id,
                    file_hash.as_deref(),
                    snapshot_hash.as_deref(),
                );
            }
            StateFact::ResponseCellAdded {
                cycle_id,
                operation_id,
                cell_id,
                response_sha256,
                content_hash,
                applied,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::WriteApplied);
                self.closeout.patch_id = Some(operation_id.clone());
                self.closeout.response_cell = Some(ResponseCellProjection {
                    operation_id: operation_id.clone(),
                    cell_id: cell_id.clone(),
                    response_sha256: response_sha256.clone(),
                    content_hash: content_hash.clone(),
                    applied: *applied,
                });
                if *applied {
                    self.closeout
                        .prove_write_through(write_pipeline::DocumentWritePhase::CanonicalApplied);
                }
                self.closeout
                    .clear_pending_response_for_cycle(cycle_id, "response_cell_added");
            }
            StateFact::CommitObserved {
                cycle_id,
                commit,
                file_hash,
                snapshot_hash,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::Committed);
                self.closeout.commit = Some(commit.clone());
                self.closeout
                    .prove_write_through(write_pipeline::DocumentWritePhase::Committed);
                self.closeout.realtime_steering = TurnSteeringProjection::none();
                self.closeout
                    .clear_pending_response_for_cycle(cycle_id, "committed");
                self.closeout.refresh_capture_content_hashes(
                    cycle_id,
                    file_hash.as_deref(),
                    snapshot_hash.as_deref(),
                );
            }
            StateFact::SessionCheckPassed { cycle_id, .. } => {
                if self.closeout.cycle_id.as_deref() == Some(cycle_id) {
                    self.closeout.session_check_passed = true;
                }
            }
            StateFact::CycleAbandoned {
                cycle_id, reason, ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::Abandoned);
                self.closeout.abandoned_reason = Some(reason.clone());
                self.closeout.realtime_steering = TurnSteeringProjection::none();
                self.closeout
                    .clear_pending_response_for_cycle(cycle_id, "abandoned");
            }
            StateFact::FalseStaleCaptureReactivated {
                cycle_id,
                capture_id,
                response_sha256,
                retirement_reason,
                ..
            } => {
                let exact_false_stale_retirement = self.closeout.cycle_id.as_deref()
                    == Some(cycle_id)
                    && self.closeout.phase == Some(CyclePhase::Abandoned)
                    && self.closeout.abandoned_reason.as_deref() == Some(retirement_reason)
                    && self.closeout.capture_id.as_deref() == Some(capture_id)
                    && self.closeout.response_sha256.as_deref() == Some(response_sha256);
                if exact_false_stale_retirement {
                    self.closeout.phase = Some(CyclePhase::ResponseCaptured);
                    self.closeout.abandoned_reason = None;
                    self.closeout.session_check_passed = false;
                }
            }
            StateFact::DocumentCellMergeAckRecorded {
                cycle_id,
                component,
                id,
                reason,
                detail,
                ..
            } => {
                self.closeout.apply_semantic_merge_ack(
                    component,
                    id,
                    reason,
                    detail,
                    Some(cycle_id),
                    false,
                );
            }
            StateFact::DocumentCellMergeAckCarriedForward {
                source_cycle_id,
                target_cycle_id,
                component,
                id,
                reason,
                detail,
                ..
            } => {
                if self.closeout.cycle_id.as_deref() != Some(target_cycle_id) {
                    self.closeout.cycle_id = Some(target_cycle_id.clone());
                    self.closeout.phase = Some(CyclePhase::PreflightStarted);
                    self.closeout.session_check_passed = false;
                }
                self.closeout.apply_semantic_merge_ack(
                    component,
                    id,
                    reason,
                    detail,
                    source_cycle_id.as_deref(),
                    true,
                );
            }
            StateFact::OwnerGenerationChanged {
                owner, generation, ..
            } => {
                self.supervisor.change_generation(*owner, *generation);
                if *owner == StateOwner::RouteDispatch {
                    self.route.generation = Some(*generation);
                    self.route.readiness = RouteReadinessPhase::Unknown;
                }
                if *owner == StateOwner::EditorIpcBridge {
                    self.transport.editor_generation = Some(*generation);
                }
            }
            StateFact::EditorPatchQueued {
                patch_id,
                actor_generation,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport.apply_patch_event(
                        patch_id,
                        TransportPatchEvent::PatchQueued,
                        *actor_generation,
                    );
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::EditorPatchApplied {
                patch_id,
                actor_generation,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport
                        .apply_patch_applied_receipt(patch_id, *actor_generation);
                    if self.closeout.patch_id.as_deref() == Some(patch_id) {
                        self.closeout.prove_write_through(
                            write_pipeline::DocumentWritePhase::ReplicaVisible,
                        );
                    }
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::EditorPatchRejected {
                patch_id,
                actor_generation,
                reason,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    if self.transport.apply_patch_rejected_receipt(
                        patch_id,
                        *actor_generation,
                        reason,
                    ) {
                        self.transport.last_rejected_reason = Some(reason.clone());
                    }
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::VisibleWriteCommitCandidateObserved {
                patch_id,
                model_revision,
                editor_visible_hash,
                commit_candidate_hash,
                commit_candidate_content,
                source,
                ..
            } => {
                self.visible_write.observe_commit_candidate(
                    patch_id,
                    *model_revision,
                    editor_visible_hash,
                    commit_candidate_hash,
                    commit_candidate_content.as_deref(),
                    source,
                );
            }
            StateFact::VisibleWriteMaterializedCarryForwardObserved {
                model_revision,
                live_buffer_hash,
                file_content_hash,
                commit_candidate_hash,
                source,
                ..
            } => {
                self.visible_write.observe_materialized_carry_forward(
                    *model_revision,
                    live_buffer_hash,
                    file_content_hash,
                    commit_candidate_hash,
                    source,
                );
            }
            StateFact::IpcProofInsufficient {
                patch_id,
                actor_generation,
                reason,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport.apply_patch_event(
                        patch_id,
                        TransportPatchEvent::ProofInsufficient,
                        *actor_generation,
                    );
                    self.transport.last_unproven_reason = Some(reason.clone());
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::EditorPatchRetryRequested {
                patch_id,
                actor_generation,
                reason,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport.apply_patch_event(
                        patch_id,
                        TransportPatchEvent::RetryRequested,
                        *actor_generation,
                    );
                    self.transport.last_retry_reason = Some(reason.clone());
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::ForceDiskFallbackRecorded {
                patch_id,
                actor_generation,
                reason,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport.apply_patch_event(
                        patch_id,
                        TransportPatchEvent::ForceDiskFallback,
                        *actor_generation,
                    );
                    self.transport.force_disk_reason = Some(reason.clone());
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
            }
            StateFact::ActorLifecycleObserved {
                owner,
                generation,
                event,
                ..
            } => {
                if self.current_generation_matches(*owner, *generation) {
                    self.supervisor
                        .apply_lifecycle_event(*owner, *generation, *event);
                } else {
                    self.reject_stale(StateDomain::Supervisor, *owner);
                }
            }
            StateFact::AgentRestartPerformed {
                owner, generation, ..
            } => {
                if self.current_generation_matches(*owner, *generation) {
                    self.supervisor.apply_lifecycle_event(
                        *owner,
                        *generation,
                        ActorLifecycleEvent::RestartPerformed,
                    );
                } else {
                    self.reject_stale(StateDomain::Supervisor, *owner);
                }
            }
            StateFact::CapabilityProofObserved {
                owner,
                generation,
                capability,
                ..
            } => {
                if self.current_generation_matches(*owner, *generation) {
                    self.supervisor
                        .capability_proof(*owner, *generation, capability);
                } else {
                    self.reject_stale(StateDomain::Supervisor, *owner);
                }
            }
            StateFact::SupervisorHosting {
                pane_session,
                lease_epoch,
                ..
            } => {
                self.apply_supervisor_hosting(pane_session, *lease_epoch);
            }
            StateFact::SupervisorRecycleRequested {
                reason,
                recycle_epoch,
                marked_secs,
                ..
            } => {
                self.supervisor.apply_recycle_event(
                    SupervisorRecycleEvent::Requested,
                    reason,
                    *recycle_epoch,
                    *marked_secs,
                );
            }
            StateFact::SupervisorRecycleStarted {
                reason,
                recycle_epoch,
                marked_secs,
                ..
            } => {
                self.supervisor.apply_recycle_event(
                    SupervisorRecycleEvent::Started,
                    reason,
                    *recycle_epoch,
                    *marked_secs,
                );
            }
            StateFact::SupervisorRecycleSettled {
                reason,
                recycle_epoch,
                marked_secs,
                ..
            } => {
                self.supervisor.apply_recycle_event(
                    SupervisorRecycleEvent::Settled,
                    reason,
                    *recycle_epoch,
                    *marked_secs,
                );
            }
            StateFact::StartingActorTimeoutRecorded {
                pane_id,
                generation,
                log_line,
                ..
            } => {
                self.route.starting_actor_timeout = Some(StartingActorTimeoutProjection {
                    pane_id: pane_id.clone(),
                    generation: *generation,
                    log_line: log_line.clone(),
                });
            }
            StateFact::StartingActorTimeoutCleared {
                pane_id,
                generation,
                ..
            } => {
                if self
                    .route
                    .starting_actor_timeout
                    .as_ref()
                    .is_some_and(|timeout| {
                        timeout.pane_id == *pane_id && timeout.generation == *generation
                    })
                {
                    self.route.starting_actor_timeout = None;
                }
            }
            StateFact::StartupMissRecorded {
                file,
                pane_id,
                session_id,
                harness,
                timestamp,
                origin,
                cycle_baseline_id,
                ..
            } => {
                self.supervisor.startup_miss = Some(StartupMissProjection {
                    file: file.clone(),
                    pane_id: pane_id.clone(),
                    session_id: session_id.clone(),
                    harness: harness.clone(),
                    timestamp: *timestamp,
                    origin: origin.clone(),
                    cycle_baseline_id: cycle_baseline_id.clone(),
                });
            }
            StateFact::StartupMissCleared {
                pane_id,
                session_id,
                timestamp,
                ..
            } => {
                if self.supervisor.startup_miss.as_ref().is_some_and(|miss| {
                    miss.pane_id == *pane_id
                        && miss.session_id == *session_id
                        && miss.timestamp == *timestamp
                }) {
                    self.supervisor.startup_miss = None;
                }
            }
            StateFact::RoutePaneObserved {
                pane_id,
                actor_generation,
                ..
            } => {
                if self.current_generation_matches(StateOwner::RouteDispatch, *actor_generation) {
                    self.route.pane_id = Some(pane_id.clone());
                    self.route.generation = Some(*actor_generation);
                    self.route
                        .apply_readiness_event(RouteReadinessEvent::PaneObserved);
                } else {
                    self.reject_stale(StateDomain::Route, StateOwner::RouteDispatch);
                }
            }
            StateFact::RouteReadinessObserved {
                actor_generation,
                event,
                ..
            } => {
                if self.current_generation_matches(StateOwner::RouteDispatch, *actor_generation) {
                    self.route.generation = Some(*actor_generation);
                    self.route.apply_readiness_event(*event);
                } else {
                    self.reject_stale(StateDomain::Route, StateOwner::RouteDispatch);
                }
            }
            StateFact::DispatchProofObserved {
                actor_generation,
                proof_id,
                ..
            } => {
                if self.current_generation_matches(StateOwner::RouteDispatch, *actor_generation) {
                    self.route.generation = Some(*actor_generation);
                    self.route
                        .apply_readiness_event(RouteReadinessEvent::DispatchProven);
                    self.route.dispatch_proofs.insert(proof_id.clone());
                } else {
                    self.reject_stale(StateDomain::Route, StateOwner::RouteDispatch);
                }
            }
            StateFact::RouteSubmitStarted {
                pane_id,
                harness,
                reason,
                submit_epoch,
                marked_secs,
                ..
            } => {
                self.route.apply_submit_event(
                    RouteSubmitEvent::Started,
                    pane_id,
                    harness,
                    reason,
                    *submit_epoch,
                    *marked_secs,
                );
            }
            StateFact::RouteSubmitSettled {
                pane_id,
                harness,
                reason,
                submit_epoch,
                marked_secs,
                ..
            } => {
                self.route.apply_submit_event(
                    RouteSubmitEvent::Settled,
                    pane_id,
                    harness,
                    reason,
                    *submit_epoch,
                    *marked_secs,
                );
            }
            StateFact::RouteSubmitBlocked {
                pane_id,
                harness,
                reason,
                submit_epoch,
                marked_secs,
                ..
            } => {
                self.route.apply_submit_event(
                    RouteSubmitEvent::Blocked,
                    pane_id,
                    harness,
                    reason,
                    *submit_epoch,
                    *marked_secs,
                );
            }
            StateFact::ProofMarkerObserved { marker, source, .. } => {
                self.proof
                    .apply(marker, source, ProofGateEvent::MarkerObserved);
            }
            StateFact::ProofMarkerDisproved { marker, source, .. } => {
                self.proof
                    .apply(marker, source, ProofGateEvent::MarkerDisproved);
            }
            StateFact::TerminalCloseoutProofRecorded {
                cycle_id,
                last_event,
                did_commit,
                file_hash,
                snapshot_hash,
                head_hash,
                state_file_hash_matches,
                state_snapshot_hash_matches,
                agreement,
                capture_id,
                response_sha256,
                recorded_at_ms,
                ..
            } => {
                self.proof
                    .apply_terminal_closeout(TerminalCloseoutProofProjection {
                        cycle_id: cycle_id.clone(),
                        last_event: last_event.clone(),
                        did_commit: *did_commit,
                        file_hash: file_hash.clone(),
                        snapshot_hash: snapshot_hash.clone(),
                        head_hash: head_hash.clone(),
                        state_file_hash_matches: *state_file_hash_matches,
                        state_snapshot_hash_matches: *state_snapshot_hash_matches,
                        agreement: agreement.clone(),
                        capture_id: capture_id.clone(),
                        response_sha256: response_sha256.clone(),
                        recorded_at_ms: *recorded_at_ms,
                    });
            }
            StateFact::CloseoutRecoveryEvidenceRecorded {
                evidence_key,
                visible_markdown_hash,
                snapshot_hash,
                active_cycle_id,
                active_cycle_phase,
                active_capture_id,
                active_capture_cycle_id,
                active_capture_state,
                active_capture_response_sha256,
                response_body,
                queue_only_drift,
                snapshot_head_drift,
                snapshot_visible_drift,
                editor_ipc,
                binary_freshness,
                recorded_at_ms,
                ..
            } => {
                self.proof
                    .apply_closeout_recovery_evidence(CloseoutRecoveryEvidenceProjection {
                        evidence_key: evidence_key.clone(),
                        visible_markdown_hash: visible_markdown_hash.clone(),
                        snapshot_hash: snapshot_hash.clone(),
                        active_cycle_id: active_cycle_id.clone(),
                        active_cycle_phase: *active_cycle_phase,
                        active_capture_id: active_capture_id.clone(),
                        active_capture_cycle_id: active_capture_cycle_id.clone(),
                        active_capture_state: active_capture_state.clone(),
                        active_capture_response_sha256: active_capture_response_sha256.clone(),
                        response_body: response_body.clone(),
                        queue_only_drift: queue_only_drift.clone(),
                        snapshot_head_drift: *snapshot_head_drift,
                        snapshot_visible_drift: *snapshot_visible_drift,
                        editor_ipc: editor_ipc.clone(),
                        binary_freshness: binary_freshness.clone(),
                        recorded_at_ms: *recorded_at_ms,
                    });
            }
        }
    }

    fn current_generation_matches(&self, owner: StateOwner, generation: u64) -> bool {
        self.supervisor
            .owners
            .get(&owner)
            .and_then(|projection| projection.generation)
            .is_none_or(|current| current == generation)
    }

    /// Apply a `SupervisorHosting` transition (`#xdocsuper1`/`#xdocsuper3`).
    ///
    /// Determines whether this host event is a fresh host or a switch — the
    /// `pane_session` differs from the one currently hosting the document, or the
    /// same pane re-hosts at a strictly higher `lease_epoch`. On a fresh
    /// host/switch the document's hosting epoch advances and the stale
    /// per-document queue overlay is dropped, so any queue fact carrying the old
    /// `hosting_epoch` is rejected by [`Self::hosting_epoch_current`] — the
    /// stale-overlay replay becomes a no-op by construction. A re-emit of the
    /// same `(pane_session, lease_epoch)` is idempotent (no reset, no epoch bump).
    fn apply_supervisor_hosting(&mut self, pane_session: &str, lease_epoch: u64) {
        let (is_switch, next_hosting_epoch) = match &self.hosting {
            None => (true, 1),
            Some(current) => {
                let switched =
                    current.pane_session != pane_session || lease_epoch > current.lease_epoch;
                if switched {
                    (true, current.hosting_epoch + 1)
                } else {
                    (false, current.hosting_epoch)
                }
            }
        };
        if is_switch {
            // First step on every document handoff/host: drop the prior
            // document's stale in-memory queue overlay before any new-hosting
            // queue fact can be accepted. Queue facts at the old hosting epoch
            // are then rejected even if reordered/replayed.
            self.queue = QueueProjection::default();
        }
        self.hosting = Some(SupervisorHostingProjection {
            pane_session: pane_session.to_string(),
            lease_epoch,
            hosting_epoch: next_hosting_epoch,
        });
    }

    /// A queue fact's `hosting_epoch` is current when it matches the document's
    /// hosting epoch. `None` (legacy/un-hosted producers) is always accepted so
    /// the gate is backward compatible; a fact stamped with an epoch older than
    /// the current hosting is a stale-overlay replay and rejected.
    fn hosting_epoch_current(&self, fact_hosting_epoch: Option<u64>) -> bool {
        match (fact_hosting_epoch, &self.hosting) {
            (None, _) => true,
            (Some(_), None) => true,
            (Some(fact_epoch), Some(hosting)) => fact_epoch >= hosting.hosting_epoch,
        }
    }

    fn reject_stale(&mut self, domain: StateDomain, owner: StateOwner) {
        self.rejected_stale_events
            .push(RejectedStaleEvent { domain, owner });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_readiness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_transport_patch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_transport_phase: Option<String>,
    pub proof_markers: usize,
}

impl ProjectionSummary {
    pub fn from_document(projection: &DocumentStateProjection) -> Self {
        let latest_patch = projection.transport.patches.iter().next_back();
        Self {
            route_readiness: serde_name(&projection.route.readiness),
            route_pane_id: projection.route.pane_id.clone(),
            latest_transport_patch_id: latest_patch.map(|(patch_id, _)| patch_id.clone()),
            latest_transport_phase: latest_patch.and_then(|(_, patch)| serde_name(&patch.phase)),
            proof_markers: projection.proof.markers.len(),
        }
    }

    pub fn compact(&self) -> String {
        format!(
            "route={} pane={} transport={}:{} proof_markers={}",
            self.route_readiness.as_deref().unwrap_or("unknown"),
            self.route_pane_id.as_deref().unwrap_or("-"),
            self.latest_transport_patch_id.as_deref().unwrap_or("-"),
            self.latest_transport_phase.as_deref().unwrap_or("-"),
            self.proof_markers
        )
    }
}

fn serde_name<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(str::to_string))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedStaleEvent {
    pub domain: StateDomain,
    pub owner: StateOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DocumentProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_baseline: Option<BaselineProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_baseline: Option<DocumentBaselineProjection>,
    /// Monotonic checkpoint/clear generation, retained even when no baseline is active.
    #[serde(default)]
    pub merge_baseline_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_checkpoint: Option<DocumentBaselineProjection>,
    /// Monotonic checkpoint/clear generation for undo state.
    #[serde(default)]
    pub undo_checkpoint_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crdt_recovery_projection: Option<CrdtRecoveryProjection>,
    /// Monotonic checkpoint/clear generation for cold CRDT restart state.
    #[serde(default)]
    pub crdt_recovery_projection_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_file_watch_change: Option<FileWatchChangeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_disk_write: Option<DocumentDiskWriteProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_authority: Option<DocumentAuthorityProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_write: Option<DocumentWriteIntentProjection>,
    /// Ordered durable agent-write intents. `pending_write` remains the newest
    /// compatibility projection; this journal keeps each composed change
    /// independently observable and recoverable until an ACK settles the target
    /// that includes it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_write_journal: Vec<DocumentWriteIntentProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_external_disk: Option<DocumentWriteIntentProjection>,
    /// Monotone counter behind `DocumentWriteIntentProjection::ordinal` and
    /// [`ConvergedWriteProjection::ordinal`]. Advanced by every write fact the
    /// projection applies, so replay assigns the same ordinals every time.
    #[serde(default)]
    pub write_fact_ordinal: u64,
    /// The newest write that reached convergence (`#adwritesourceenum`).
    ///
    /// A closeout's later stage routinely overtakes an earlier stage's retained
    /// intent and then converges, draining itself out of `pending_write_journal`
    /// and leaving nothing to compare against. Keeping the last converged write
    /// is what lets settlement ask "was I superseded by my own successor" as an
    /// ordering comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_converged_write: Option<ConvergedWriteProjection>,
}

/// The newest write that converged on a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergedWriteProjection {
    pub intent_id: String,
    pub target_hash: String,
    pub source: DocumentWriteSource,
    #[serde(default)]
    pub ordinal: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentWriteIntentProjection {
    pub intent_id: String,
    pub expected_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_content: Option<String>,
    pub target_hash: String,
    pub target_content: String,
    pub source: DocumentWriteSource,
    pub reason: DocumentWriteDeferredReason,
    /// Monotone ordinal the projection assigns to every write fact it applies
    /// (`#adwritesourceenum`).
    ///
    /// Stage ordering alone cannot say whether a converged write came *before*
    /// or *after* a retained intent — a `post_commit_reposition` from the
    /// previous cycle outranks this cycle's `pending_write` by stage while being
    /// strictly older in time. The ordinal supplies that missing half, and being
    /// assigned during replay keeps it deterministic. `#[serde(default)]` so
    /// projections written before this field deserialize as ordinal 0.
    #[serde(default)]
    pub ordinal: u64,
}

impl DocumentProjection {
    /// Next ordinal in the document's write-fact sequence.
    ///
    /// Assigned during projection (not at emit time) so replaying the same
    /// ledger produces the same ordinals — the property that lets a stale
    /// controller and a fresh one agree on which write came later.
    fn next_write_fact_ordinal(&mut self) -> u64 {
        self.write_fact_ordinal = self.write_fact_ordinal.saturating_add(1);
        self.write_fact_ordinal
    }

    /// The stage of a converged write that is strictly newer than `intent` and
    /// strictly later in the closeout sequence — i.e. `intent`'s own successor
    /// overtook it (`#adwritesourceenum`).
    ///
    /// Both halves are required. Stage alone cannot order across cycles (last
    /// cycle's `post_commit_reposition` outranks this cycle's `pending_write` by
    /// stage while being older in time), and the ordinal alone says nothing
    /// about whether the newer write belongs to the same closeout sequence.
    pub fn superseding_closeout_stage(
        &self,
        intent: &DocumentWriteIntentProjection,
    ) -> Option<CloseoutStage> {
        let converged = self.latest_converged_write.as_ref()?;
        if converged.ordinal <= intent.ordinal {
            return None;
        }
        let stage = converged.source.closeout_stage()?;
        intent.source.superseded_by(&converged.source).then_some(stage)
    }

    fn seed_pending_write_journal_from_legacy_projection(&mut self) {
        if self.pending_write_journal.is_empty()
            && let Some(pending) = self.pending_write.clone()
        {
            self.pending_write_journal.push(pending);
        }
    }

    fn apply_authority(
        &mut self,
        authority: DocumentAuthority,
        authority_epoch: u64,
        source: &str,
        reason: &str,
        content_hash: Option<&str>,
        editor_id: Option<&str>,
    ) -> bool {
        let accept = match &self.latest_authority {
            None => true,
            Some(current) if authority_epoch > current.authority_epoch => true,
            Some(current) if authority_epoch == current.authority_epoch => {
                authority.editor_active() && !current.authority.editor_active()
            }
            Some(_) => false,
        };
        if !accept {
            return false;
        }
        self.latest_authority = Some(DocumentAuthorityProjection {
            authority,
            authority_epoch,
            source: source.to_string(),
            reason: reason.to_string(),
            content_hash: content_hash.map(str::to_string),
            editor_id: editor_id.map(str::to_string),
        });
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentAuthority {
    /// No active editor replica owns the document; disk participates as the
    /// current document replica.
    DiskReplica,
    /// A live editor relay supplied the authoritative current text.
    EditorRelay,
    /// A live editor owns the document, but no relay replica has registered yet.
    EditorAttachedMissingReplica,
    /// A live editor owns the document, but relay delivery has not converged.
    EditorSyncPending,
}

impl DocumentAuthority {
    pub fn editor_active(self) -> bool {
        matches!(
            self,
            Self::EditorRelay | Self::EditorAttachedMissingReplica | Self::EditorSyncPending
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentAuthorityProjection {
    pub authority: DocumentAuthority,
    pub authority_epoch: u64,
    pub source: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineProjection {
    pub cycle_id: String,
    pub baseline_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentBaselineProjection {
    pub generation: u64,
    pub content_hash: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrdtRecoveryProjection {
    pub generation: u64,
    pub projection_sha256: String,
    pub projection_base64: String,
    pub lineage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWatchChangeProjection {
    pub path: String,
    pub watch_generation: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentDiskWriteProjection {
    pub generation: u64,
    pub content_len: u64,
    pub content_hash: String,
    pub write_id: String,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QueueProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_head: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub active_heads: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub heads: BTreeMap<String, QueueHeadProjection>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub completed_heads: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worklist: Vec<QueueWorklistEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worklist_queue_hash: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worklist_active: bool,
    #[serde(default)]
    pub context_clear: QueueContextClearProjection,
    #[serde(default)]
    pub drain_stall: QueueDrainStallProjection,
}

impl QueueProjection {
    fn apply_selected(
        &mut self,
        node_key: &str,
        backlog_id: Option<&str>,
        prompt_text: Option<&str>,
        drainable: bool,
    ) {
        let head = self
            .heads
            .entry(node_key.to_string())
            .or_insert_with(|| QueueHeadProjection::new(backlog_id));
        head.backlog_id = backlog_id.map(str::to_string).or(head.backlog_id.clone());
        head.prompt_text = prompt_text.map(str::to_string).or(head.prompt_text.clone());
        head.drainable = drainable;
        head.defer_reason = None;
        if head.transition(QueueHeadEvent::Selected) {
            self.active_head = Some(node_key.to_string());
            self.active_heads.insert(node_key.to_string());
        }
    }

    fn apply_deferred(&mut self, node_key: &str, reason: &str) {
        let head = self
            .heads
            .entry(node_key.to_string())
            .or_insert_with(|| QueueHeadProjection::new(None));
        head.defer_reason = Some(reason.to_string());
        head.drainable = false;
        head.transition(QueueHeadEvent::Deferred);
        if self.active_head.as_deref() == Some(node_key) {
            self.active_head = None;
        }
        self.active_heads.remove(node_key);
    }

    fn apply_completed(&mut self, node_key: &str, backlog_id: Option<&str>) {
        let head = self
            .heads
            .entry(node_key.to_string())
            .or_insert_with(|| QueueHeadProjection::new(backlog_id));
        head.backlog_id = backlog_id.map(str::to_string).or(head.backlog_id.clone());
        head.transition(QueueHeadEvent::Completed);
        self.completed_heads.insert(node_key.to_string());
        if self.active_head.as_deref() == Some(node_key) {
            self.active_head = None;
        }
        self.active_heads.remove(node_key);
    }

    fn apply_worklist(&mut self, queue_hash: &str, entries: &[QueueWorklistEntry], active: bool) {
        self.worklist_queue_hash = Some(queue_hash.to_string());
        self.worklist_active = active;
        self.worklist = if active { entries.to_vec() } else { Vec::new() };
    }

    fn apply_context_clear_event(
        &mut self,
        event: QueueContextClearEvent,
        fields: QueueContextClearFields<'_>,
        clear_epoch: u64,
        marked_secs: u64,
    ) {
        self.context_clear
            .apply_event(event, fields, clear_epoch, marked_secs);
    }

    fn apply_drain_stall_event(
        &mut self,
        event: QueueDrainStallEvent,
        fields: QueueDrainStallFields<'_>,
        stall_epoch: u64,
        event_secs: u64,
    ) {
        self.drain_stall
            .apply_event(event, fields, stall_epoch, event_secs);
    }
}

#[derive(Debug, Clone, Copy)]
struct QueueDrainStallFields<'a> {
    file: &'a str,
    cycle_id: Option<&'a str>,
    reason: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QueueDrainStallProjection {
    pub phase: QueueDrainStallPhase,
    pub file: String,
    pub cycle_id: String,
    pub stall_epoch: u64,
    pub recorded_secs: u64,
    pub cleared_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_reason: Option<String>,
}

impl QueueDrainStallProjection {
    fn apply_event(
        &mut self,
        event: QueueDrainStallEvent,
        fields: QueueDrainStallFields<'_>,
        stall_epoch: u64,
        event_secs: u64,
    ) {
        if stall_epoch < self.stall_epoch {
            return;
        }
        if let Some(next) = QueueDrainStallMachine::transition(self.phase, event) {
            self.phase = next;
            self.file = fields.file.to_string();
            self.stall_epoch = stall_epoch;
            match event {
                QueueDrainStallEvent::Recorded => {
                    self.cycle_id = fields.cycle_id.unwrap_or_default().to_string();
                    self.recorded_secs = event_secs;
                    self.cleared_secs = 0;
                    self.clear_reason = None;
                }
                QueueDrainStallEvent::Cleared => {
                    self.cleared_secs = event_secs;
                    self.clear_reason = fields.reason.map(str::to_string);
                }
            }
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.phase, QueueDrainStallPhase::Pending)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueueDrainStallPhase {
    #[default]
    Idle,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDrainStallEvent {
    Recorded,
    Cleared,
}

/// A lazily context whose **lifetime is part of its type** (`#stategraphjoin`).
///
/// Full rationale on the scope types themselves lives in [`agent_doc_state_scope`].
/// The short form: a shared context is necessary but not sufficient, because a bare
/// `&ThreadSafeContext` lets a cell join a graph with the wrong lifetime, and neither
/// direction is caught at runtime — both surface much later as a stale value.
///
/// The scope types live in the leaf crate [`agent_doc_state_scope`] so every crate
/// holding reactive state can name one.
///
/// They started here, but this crate depends on `agent-doc-turn` — so the crates
/// holding the remaining islands could not join a scope without a dependency cycle.
/// A leaf crate with nothing but `lazily` under it can be depended on from anywhere,
/// which is what makes the rule enforceable workspace-wide rather than in one crate.
/// Re-exported so existing `agent_doc_state_backbone::DocumentScope` paths keep
/// working.
pub use agent_doc_state_scope::{DocumentScope, ProcessScope, TurnScope};

/// Constructors for the nine document-scoped machines, as an extension trait.
///
/// It is a trait rather than an inherent `impl` because [`DocumentScope`] now lives in
/// the leaf crate `agent-doc-state-scope` (the orphan rule), and moving it there is
/// what let the crates below this one join a scope at all. The ergonomics are
/// unchanged for anyone who imports the trait; `Machine::new_in(&scope, ..)` remains
/// the direct form.
pub trait DocumentScopeMachines {
    fn queue_drain_stall(&self, initial: QueueDrainStallPhase) -> QueueDrainStallMachine;

    fn queue_context_clear(&self, initial: QueueContextClearPhase) -> QueueContextClearMachine;

    fn supervisor_recycle(&self, initial: SupervisorRecyclePhase) -> SupervisorRecycleMachine;

    fn route_submit(&self, initial: RouteSubmitPhase) -> RouteSubmitMachine;

    fn queue_head(&self, initial: QueueHeadPhase) -> QueueHeadMachine;

    fn transport_patch(&self, initial: TransportPatchPhase) -> TransportPatchMachine;

    fn actor_lifecycle(&self, initial: ActorLifecyclePhase) -> ActorLifecycleMachine;

    fn route_readiness(&self, initial: RouteReadinessPhase) -> RouteReadinessMachine;

    fn proof_gate(&self, initial: ProofGatePhase) -> ProofGateMachine;
}

impl DocumentScopeMachines for DocumentScope {
    fn queue_drain_stall(&self, initial: QueueDrainStallPhase) -> QueueDrainStallMachine {
        QueueDrainStallMachine::new_in(self, initial)
    }

    fn queue_context_clear(&self, initial: QueueContextClearPhase) -> QueueContextClearMachine {
        QueueContextClearMachine::new_in(self, initial)
    }

    fn supervisor_recycle(&self, initial: SupervisorRecyclePhase) -> SupervisorRecycleMachine {
        SupervisorRecycleMachine::new_in(self, initial)
    }

    fn route_submit(&self, initial: RouteSubmitPhase) -> RouteSubmitMachine {
        RouteSubmitMachine::new_in(self, initial)
    }

    fn queue_head(&self, initial: QueueHeadPhase) -> QueueHeadMachine {
        QueueHeadMachine::new_in(self, initial)
    }

    fn transport_patch(&self, initial: TransportPatchPhase) -> TransportPatchMachine {
        TransportPatchMachine::new_in(self, initial)
    }

    fn actor_lifecycle(&self, initial: ActorLifecyclePhase) -> ActorLifecycleMachine {
        ActorLifecycleMachine::new_in(self, initial)
    }

    fn route_readiness(&self, initial: RouteReadinessPhase) -> RouteReadinessMachine {
        RouteReadinessMachine::new_in(self, initial)
    }

    fn proof_gate(&self, initial: ProofGatePhase) -> ProofGateMachine {
        ProofGateMachine::new_in(self, initial)
    }
}

pub struct QueueDrainStallMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueDrainStallPhase, QueueDrainStallEvent>,
}

impl QueueDrainStallMachine {
    pub fn new(initial: QueueDrainStallPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_drain_stall);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: QueueDrainStallPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_queue_drain_stall);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> QueueDrainStallPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: QueueDrainStallEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> QueueDrainStallPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: QueueDrainStallPhase,
        event: QueueDrainStallEvent,
    ) -> Option<QueueDrainStallPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_queue_drain_stall(
    _current: &QueueDrainStallPhase,
    event: &QueueDrainStallEvent,
) -> Option<QueueDrainStallPhase> {
    match event {
        QueueDrainStallEvent::Recorded => Some(QueueDrainStallPhase::Pending),
        QueueDrainStallEvent::Cleared => Some(QueueDrainStallPhase::Idle),
    }
}

#[derive(Debug, Clone, Copy)]
struct QueueContextClearFields<'a> {
    file: &'a str,
    target: &'a str,
    harness: &'a str,
    command: &'a str,
    source: Option<&'a str>,
    head_sha256: Option<&'a str>,
    head_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QueueContextClearProjection {
    pub phase: QueueContextClearPhase,
    pub file: String,
    pub target: String,
    pub harness: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_bytes: Option<usize>,
    pub clear_epoch: u64,
    pub marked_secs: u64,
}

impl QueueContextClearProjection {
    fn apply_event(
        &mut self,
        event: QueueContextClearEvent,
        fields: QueueContextClearFields<'_>,
        clear_epoch: u64,
        marked_secs: u64,
    ) {
        if clear_epoch < self.clear_epoch {
            return;
        }
        if let Some(next) = QueueContextClearMachine::transition(self.phase, event) {
            self.phase = next;
            self.file = fields.file.to_string();
            self.target = fields.target.to_string();
            self.harness = fields.harness.to_string();
            self.command = fields.command.to_string();
            self.source = fields.source.map(str::to_string);
            if matches!(
                event,
                QueueContextClearEvent::Deferred | QueueContextClearEvent::Started
            ) {
                self.head_sha256 = fields.head_sha256.map(str::to_string);
                self.head_bytes = fields.head_bytes;
            }
            self.clear_epoch = clear_epoch;
            self.marked_secs = marked_secs;
        }
    }

    pub fn ttl_secs(&self) -> u64 {
        match self.phase {
            QueueContextClearPhase::InFlight => QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,
            QueueContextClearPhase::Deferred | QueueContextClearPhase::Idle => 0,
        }
    }

    pub fn is_pending_at(&self, now_secs: u64) -> bool {
        matches!(self.phase, QueueContextClearPhase::InFlight)
            && now_secs.saturating_sub(self.marked_secs) <= self.ttl_secs()
    }

    pub fn is_deferred_operator_clear(&self) -> bool {
        matches!(self.phase, QueueContextClearPhase::Deferred)
            && self.source.as_deref() == Some(QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_DEFERRED)
    }

    pub fn is_manual_operator_clear_cooldown(&self) -> bool {
        matches!(self.phase, QueueContextClearPhase::InFlight)
            && self.source.as_deref() == Some(QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_MANUAL_COOLDOWN)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueueContextClearPhase {
    #[default]
    Idle,
    Deferred,
    InFlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueContextClearEvent {
    Deferred,
    Started,
    Settled,
}

pub struct QueueContextClearMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueContextClearPhase, QueueContextClearEvent>,
}

impl QueueContextClearMachine {
    pub fn new(initial: QueueContextClearPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_context_clear);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: QueueContextClearPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_queue_context_clear);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> QueueContextClearPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: QueueContextClearEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> QueueContextClearPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: QueueContextClearPhase,
        event: QueueContextClearEvent,
    ) -> Option<QueueContextClearPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_queue_context_clear(
    _current: &QueueContextClearPhase,
    event: &QueueContextClearEvent,
) -> Option<QueueContextClearPhase> {
    match event {
        QueueContextClearEvent::Deferred => Some(QueueContextClearPhase::Deferred),
        QueueContextClearEvent::Started => Some(QueueContextClearPhase::InFlight),
        QueueContextClearEvent::Settled => Some(QueueContextClearPhase::Idle),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueWorklistEntryKind {
    Prompt,
    Preset,
    Dispatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueWorklistEntry {
    pub kind: QueueWorklistEntryKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog_id: Option<String>,
    pub drainable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueHeadProjection {
    pub phase: QueueHeadPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<String>,
    pub drainable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defer_reason: Option<String>,
}

impl QueueHeadProjection {
    fn new(backlog_id: Option<&str>) -> Self {
        Self {
            phase: QueueHeadPhase::Pending,
            backlog_id: backlog_id.map(str::to_string),
            prompt_text: None,
            drainable: false,
            defer_reason: None,
        }
    }

    fn transition(&mut self, event: QueueHeadEvent) -> bool {
        if let Some(next) = QueueHeadMachine::transition(self.phase, event) {
            self.phase = next;
            true
        } else {
            false
        }
    }
}

fn turn_steering_projection_is_none(steering: &TurnSteeringProjection) -> bool {
    !steering.is_present()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CloseoutProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<CloseoutOwnerProjection>,
    /// Newest complete turn-intent checkpoint from the state ledger. Normal
    /// execution reads this projection; filesystem projections are recovery-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_intent_checkpoint: Option<TurnIntentCheckpointProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<CyclePhase>,
    /// Monotonic proof frontier for the current immutable response intent.
    /// Retry/reconnect events never move this state backward or create a new
    /// intent; stronger durable facts prove the intervening stages in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_phase: Option<write_pipeline::DocumentWritePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_response: Option<CapturedResponseProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_response_retired_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_draft: Option<ResponseDraftProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_draft_clear_reason: Option<String>,
    #[serde(default)]
    pub capture_replayed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_cell: Option<ResponseCellProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub session_check_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracked_work_maintenance_required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandoned_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_response: Option<PendingResponseProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_response_clear_reason: Option<String>,
    #[serde(default, skip_serializing_if = "turn_steering_projection_is_none")]
    pub realtime_steering: TurnSteeringProjection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_semantic_merge_acks: Vec<DocumentCellMergeAckProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnIntentCheckpointProjection {
    pub cycle_id: String,
    pub checkpoint_sequence: u64,
    pub state_sha256: String,
    pub state_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseDraftProjection {
    pub cycle_id: String,
    pub checkpoint_id: String,
    pub checkpoint_count: u64,
    pub response_sha256: String,
    pub response_body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
}

impl CloseoutProjection {
    /// Decide a closeout-owner compare-and-swap from the Lazily projection.
    ///
    /// The caller supplies process liveness as typed evidence; the decision never
    /// consults SQLite or a filesystem lock. The document actor must serialize
    /// this decision with the resulting `CloseoutOwnerClaimed` fact append.
    pub fn decide_owner_claim(
        &self,
        request: &CloseoutOwnerClaimRequest,
        current_owner_alive: Option<bool>,
    ) -> CloseoutOwnerClaimOutcome {
        let Some(cycle_id) = self.cycle_id.as_deref() else {
            return CloseoutOwnerClaimOutcome::CycleSuperseded;
        };
        if !self.phase.is_some_and(CyclePhase::is_open)
            || request
                .expected_cycle_id
                .as_deref()
                .is_some_and(|expected| expected != cycle_id)
        {
            return CloseoutOwnerClaimOutcome::CycleSuperseded;
        }

        // `#closeoutterminalreactive`: ask the derived facts before the clock.
        // `release_reason` answers "superseded by a new turn" and "owner process
        // is gone" first, and only falls through to lease expiry as the stopgap.
        // Previously this consulted `is_active_at` directly, so an owner claimed
        // during a turn that had since closed still blocked the next turn for
        // the full lease — a timeout standing in for a fact already known.
        if let Some(owner) = self.owner.as_ref()
            && owner.owner_id != request.owner_id
            && owner
                .release_reason(
                    cycle_id,
                    request.now_secs,
                    current_owner_alive,
                    request.allow_dead_owner_takeover,
                )
                .is_none()
        {
            return CloseoutOwnerClaimOutcome::HeldByOther(owner.clone());
        }

        CloseoutOwnerClaimOutcome::Acquired(CloseoutOwnerProjection {
            cycle_id: cycle_id.to_string(),
            owner_id: request.owner_id.clone(),
            owner_pid: request.owner_pid,
            role: request.role.clone(),
            claimed_secs: request.now_secs,
            expires_secs: request.now_secs.saturating_add(request.lease_secs.max(1)),
        })
    }

    pub fn owner_release_matches(&self, cycle_id: &str, owner_id: &str) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|owner| owner.cycle_id == cycle_id && owner.owner_id == owner_id)
    }

    fn refresh_capture_content_hashes(
        &mut self,
        cycle_id: &str,
        file_hash: Option<&str>,
        snapshot_hash: Option<&str>,
    ) {
        if self.cycle_id.as_deref() != Some(cycle_id) {
            return;
        }
        if let Some(file_hash) = file_hash {
            self.response_file_hash = Some(file_hash.to_string());
        }
        if let Some(snapshot_hash) = snapshot_hash {
            self.response_snapshot_hash = Some(snapshot_hash.to_string());
        }
        if let Some(capture) = self.captured_response.as_mut()
            && capture.cycle_id == cycle_id
        {
            if let Some(file_hash) = file_hash {
                capture.file_hash = Some(file_hash.to_string());
            }
            if let Some(snapshot_hash) = snapshot_hash {
                capture.snapshot_hash = Some(snapshot_hash.to_string());
            }
        }
    }

    fn prove_write_through(&mut self, target: write_pipeline::DocumentWritePhase) {
        use write_pipeline::{DocumentWriteEvent as Event, DocumentWritePhase as Phase};

        let mut current = self.write_phase.unwrap_or(Phase::IntentCaptured);
        while current < target {
            let event = match current {
                Phase::IntentCaptured => Event::CanonicalApplied,
                Phase::CanonicalApplied => Event::ReplicaAccepted,
                Phase::ReplicaAccepted => Event::ReplicaVisible,
                Phase::ReplicaVisible => Event::DiskProjected,
                Phase::DiskProjected => Event::Committed,
                Phase::Committed => break,
            };
            let Some(next) = write_pipeline::transition_document_write(&current, &event) else {
                break;
            };
            current = next;
        }
        self.write_phase = Some(current);
    }

    fn apply_cycle_event(&mut self, cycle_id: &str, event: CycleEvent) {
        if self.cycle_id.as_deref() != Some(cycle_id) || matches!(event, CycleEvent::StartPreflight)
        {
            if self.cycle_id.as_deref() != Some(cycle_id) {
                // Every field below is evidence about one immutable turn intent.
                // Carrying any of it into a new cycle creates a projection whose
                // phase names the new cycle while its proofs still describe the
                // old one.
                // The checkpoint event is appended before the typed transition
                // event, so it may already describe this new cycle. Keep it;
                // `load` validates the checkpoint cycle against the projection.
                self.session_id = None;
                self.owner = None;
                self.write_phase = None;
                self.capture_id = None;
                self.response_sha256 = None;
                self.response_file_hash = None;
                self.response_snapshot_hash = None;
                self.captured_response = None;
                self.pending_response = None;
                self.pending_response_clear_reason = None;
                self.captured_response_retired_reason = None;
                self.response_draft = None;
                self.response_draft_clear_reason = None;
                self.capture_replayed = false;
                self.patch_id = None;
                self.response_cell = None;
                self.commit = None;
                self.session_check_passed = false;
                self.tracked_work_maintenance_required = None;
                self.abandoned_reason = None;
            }
            self.cycle_id = Some(cycle_id.to_string());
            self.phase = Some(CyclePhase::PreflightStarted);
            self.session_check_passed = false;
            self.response_file_hash = None;
            self.response_snapshot_hash = None;
            self.captured_response = None;
            self.owner = None;
            self.response_cell = None;
            self.write_phase = None;
            self.realtime_steering = TurnSteeringProjection::none();
            self.pending_semantic_merge_acks.clear();
        }
        let current = self.phase.unwrap_or(CyclePhase::PreflightStarted);
        if let Some(next) = CyclePhaseMachine::transition(current, event) {
            self.phase = Some(next);
        }
    }

    fn clear_pending_response_for_cycle(&mut self, cycle_id: &str, reason: &str) {
        if self.cycle_id.as_deref() == Some(cycle_id)
            && self
                .pending_response
                .as_ref()
                .is_none_or(|pending| pending.cycle_id == cycle_id)
        {
            self.pending_response = None;
            self.pending_response_clear_reason = Some(reason.to_string());
        }
    }

    fn apply_semantic_merge_ack(
        &mut self,
        component: &str,
        id: &str,
        reason: &str,
        detail: &str,
        recorded_cycle_id: Option<&str>,
        surfaced: bool,
    ) {
        if let Some(existing) = self
            .pending_semantic_merge_acks
            .iter_mut()
            .find(|existing| {
                existing.component == component && existing.id == id && existing.reason == reason
            })
        {
            existing.detail = detail.to_string();
            existing.recorded_cycle_id = recorded_cycle_id.map(str::to_string);
            existing.surfaced = surfaced;
            return;
        }
        self.pending_semantic_merge_acks
            .push(DocumentCellMergeAckProjection {
                component: component.to_string(),
                id: id.to_string(),
                reason: reason.to_string(),
                detail: detail.to_string(),
                recorded_cycle_id: recorded_cycle_id.map(str::to_string),
                surfaced,
            });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseCellProjection {
    pub operation_id: String,
    pub cell_id: String,
    pub response_sha256: String,
    pub content_hash: String,
    pub applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingResponseProjection {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedResponseProjection {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation_plan_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutOwnerProjection {
    pub cycle_id: String,
    pub owner_id: String,
    pub owner_pid: u32,
    pub role: String,
    pub claimed_secs: u64,
    pub expires_secs: u64,
}

/// Why an incumbent closeout owner stopped blocking a new claim.
///
/// `#closeoutterminalreactive`: the release reason is part of the decision, not
/// a log line reconstructed afterwards, because the two reasons mean opposite
/// things. `SupersededByNewTurn` is the system working — the turn advanced and
/// the old settlement is moot. `LeaseExpired` is the **stopgap firing**: a clock
/// released something a derived fact should have released first. Surfacing them
/// as distinct values is what lets the timeout be feedback rather than the
/// mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseoutOwnerRelease {
    /// The owner belongs to a turn that is no longer the open one.
    SupersededByNewTurn,
    /// The owner's process is gone and takeover was permitted.
    OwnerProcessGone,
    /// The lease ran out. The backstop — should never be the reason in a
    /// healthy system.
    LeaseExpired,
}

impl CloseoutOwnerRelease {
    pub const fn token(self) -> &'static str {
        match self {
            Self::SupersededByNewTurn => "superseded_by_new_turn",
            Self::OwnerProcessGone => "owner_process_gone",
            Self::LeaseExpired => "lease_expired",
        }
    }

    /// True when the clock, rather than a derived fact, is what freed the claim.
    ///
    /// A healthy system never answers `true` here; when it does, that is the
    /// feedback signal that a supersession path is missing.
    pub const fn is_stopgap(self) -> bool {
        matches!(self, Self::LeaseExpired)
    }
}

impl CloseoutOwnerProjection {
    pub fn is_active_at(&self, now_secs: u64) -> bool {
        now_secs < self.expires_secs
    }

    /// Whether this owner's claim still belongs to the open turn.
    pub fn belongs_to_cycle(&self, open_cycle_id: &str) -> bool {
        self.cycle_id == open_cycle_id
    }

    /// Why this owner does **not** block a new claim, or `None` if it does.
    ///
    /// Ordered so the derived facts answer first and the clock answers last
    /// (`#closeoutterminalreactive`). The turn check is the one that matters:
    /// an owner claimed during a turn that has since closed is moot the instant
    /// the next turn opens, and waiting out its lease is waiting on a guess for
    /// a fact already known. That is the 2026-07-26 wedge — a `session-check`
    /// probe from a superseded turn blocked every later probe until its lease
    /// ran out, and the refusal it produced told the operator to re-run the
    /// blocked command.
    pub fn release_reason(
        &self,
        open_cycle_id: &str,
        now_secs: u64,
        owner_alive: Option<bool>,
        allow_dead_owner_takeover: bool,
    ) -> Option<CloseoutOwnerRelease> {
        if !self.belongs_to_cycle(open_cycle_id) {
            return Some(CloseoutOwnerRelease::SupersededByNewTurn);
        }
        if allow_dead_owner_takeover && owner_alive == Some(false) {
            return Some(CloseoutOwnerRelease::OwnerProcessGone);
        }
        if !self.is_active_at(now_secs) {
            return Some(CloseoutOwnerRelease::LeaseExpired);
        }
        None
    }
}

/// Lease for a foreground owner that is actually writing a closeout.
///
/// Long on purpose: a real write closeout can take minutes, and stealing it
/// mid-flight is worse than waiting.
pub const CLOSEOUT_OWNER_LEASE_SECS: u64 = 300;

/// Lease for a **status-only** recovery probe (`session-check`).
///
/// `#closeoutwaitchurn`: `session-check` is status-only — its own refusal text
/// says so — but it claimed the full write-closeout lease. So a probe that
/// lingered blocked every subsequent probe for five minutes, and the refusal it
/// produced told the operator to run *that same command* again. The advice
/// manufactured the contention it was reporting, which is how a bounded wait
/// turns into a poll loop.
///
/// Observed 2026-07-26 on `tasks/agent-doc/agent-doc-bugs2.md`: a live
/// `agent-doc session-check` pid held `role=session_check_recovery` for 300s
/// while every retry failed against it.
///
/// Short because the probe's own work is short. If one genuinely needs longer
/// it renews; if it wedges, the next probe reclaims it in seconds instead of
/// minutes.
pub const CLOSEOUT_RECOVERY_LEASE_SECS: u64 = 20;

/// The role that marks a status-only `session-check` recovery claim.
pub const CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY: &str = "session_check_recovery";

/// Lease duration for `role`.
///
/// Scoped by role rather than fixed, because "how long may this owner block
/// everyone else" is a property of what the owner is doing, not of the lock.
pub fn closeout_owner_lease_secs(role: &str) -> u64 {
    match role {
        CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY => CLOSEOUT_RECOVERY_LEASE_SECS,
        _ => CLOSEOUT_OWNER_LEASE_SECS,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutOwnerClaimRequest {
    /// `None` claims the currently open cycle. Recovery supplies the exact
    /// captured cycle so a stale worker cannot claim a successor turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_cycle_id: Option<String>,
    pub owner_id: String,
    pub owner_pid: u32,
    pub role: String,
    pub now_secs: u64,
    pub lease_secs: u64,
    #[serde(default)]
    pub allow_dead_owner_takeover: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseoutOwnerClaimOutcome {
    Acquired(CloseoutOwnerProjection),
    HeldByOther(CloseoutOwnerProjection),
    CycleSuperseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCellMergeAckProjection {
    pub component: String,
    pub id: String,
    pub reason: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_cycle_id: Option<String>,
    #[serde(default)]
    pub surfaced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransportProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub patches: BTreeMap<String, TransportPatchProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rejected_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_unproven_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_retry_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_disk_reason: Option<String>,
    #[serde(skip)]
    receipt_projection: ReceiptProjection,
}

impl PartialEq for TransportProjection {
    fn eq(&self, other: &Self) -> bool {
        self.editor_generation == other.editor_generation
            && self.patches == other.patches
            && self.last_rejected_reason == other.last_rejected_reason
            && self.last_unproven_reason == other.last_unproven_reason
            && self.last_retry_reason == other.last_retry_reason
            && self.force_disk_reason == other.force_disk_reason
    }
}

impl Eq for TransportProjection {}

impl TransportProjection {
    fn apply_patch_event(
        &mut self,
        patch_id: &str,
        event: TransportPatchEvent,
        actor_generation: u64,
    ) -> bool {
        self.apply_patch_phase_event(patch_id, event, actor_generation)
    }

    fn apply_patch_applied_receipt(&mut self, patch_id: &str, actor_generation: u64) -> bool {
        self.apply_patch_receipt(
            patch_id,
            actor_generation,
            ReceiptOutcome::Applied,
            None,
            TransportPatchEvent::PatchApplied,
        )
    }

    fn apply_patch_rejected_receipt(
        &mut self,
        patch_id: &str,
        actor_generation: u64,
        reason: &str,
    ) -> bool {
        self.apply_patch_receipt(
            patch_id,
            actor_generation,
            ReceiptOutcome::Rejected,
            Some(reason),
            TransportPatchEvent::PatchRejected,
        )
    }

    pub fn patch_terminal_receipt_outcome(&self, patch_id: &str) -> Option<ReceiptOutcome> {
        self.receipt_projection
            .terminal_for(patch_id)
            .map(|receipt| receipt.outcome)
            .or_else(|| {
                self.patches
                    .get(patch_id)
                    .and_then(|patch| patch.phase.terminal_receipt_outcome())
            })
    }

    fn apply_patch_receipt(
        &mut self,
        patch_id: &str,
        actor_generation: u64,
        outcome: ReceiptOutcome,
        reason: Option<&str>,
        phase_event: TransportPatchEvent,
    ) -> bool {
        if let Some(existing) = self.patch_terminal_receipt_outcome(patch_id)
            && existing != outcome
        {
            return false;
        }

        let receipt = match outcome {
            ReceiptOutcome::Applied => CausalReceipt::applied(
                patch_receipt_id(patch_id, actor_generation, outcome),
                patch_id,
                "agent-doc.editor-ipc-bridge",
                actor_generation,
            ),
            ReceiptOutcome::Rejected => {
                let receipt = CausalReceipt::rejected(
                    patch_receipt_id(patch_id, actor_generation, outcome),
                    patch_id,
                    "agent-doc.editor-ipc-bridge",
                    actor_generation,
                );
                match reason {
                    Some(reason) => receipt.with_reason(reason),
                    None => receipt,
                }
            }
            ReceiptOutcome::Observed | ReceiptOutcome::Accepted => return false,
        };

        match self
            .receipt_projection
            .observe(self.editor_generation, receipt)
        {
            ReceiptApplyStatus::Recorded | ReceiptApplyStatus::Duplicate => {
                self.apply_patch_phase_event(patch_id, phase_event, actor_generation)
            }
            ReceiptApplyStatus::StaleGeneration { .. }
            | ReceiptApplyStatus::TerminalConflict { .. } => false,
        }
    }

    fn apply_patch_phase_event(
        &mut self,
        patch_id: &str,
        event: TransportPatchEvent,
        actor_generation: u64,
    ) -> bool {
        self.editor_generation = Some(actor_generation);
        let patch =
            self.patches
                .entry(patch_id.to_string())
                .or_insert_with(|| TransportPatchProjection {
                    phase: TransportPatchPhase::Queued,
                    actor_generation,
                });
        patch.actor_generation = actor_generation;
        if let Some(next) = TransportPatchMachine::transition(patch.phase, event) {
            patch.phase = next;
            true
        } else {
            false
        }
    }
}

fn patch_receipt_id(patch_id: &str, actor_generation: u64, outcome: ReceiptOutcome) -> String {
    let outcome = match outcome {
        ReceiptOutcome::Observed => "observed",
        ReceiptOutcome::Accepted => "accepted",
        ReceiptOutcome::Applied => "applied",
        ReceiptOutcome::Rejected => "rejected",
    };
    format!("agent-doc-editor:{patch_id}:{actor_generation}:{outcome}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VisibleWriteProjection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commit_candidates: BTreeMap<String, VisibleWriteCommitCandidateProjection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub materialized_carry_forward:
        BTreeMap<String, VisibleWriteMaterializedCarryForwardProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_model_revision: Option<u64>,
}

impl VisibleWriteProjection {
    pub fn observe_commit_candidate(
        &mut self,
        patch_id: &str,
        model_revision: u64,
        editor_visible_hash: &str,
        commit_candidate_hash: &str,
        commit_candidate_content: Option<&str>,
        source: &str,
    ) {
        let current_revision = self
            .commit_candidates
            .get(commit_candidate_hash)
            .map(|candidate| candidate.model_revision);
        if current_revision.is_some_and(|current| current > model_revision) {
            return;
        }
        self.latest_model_revision = Some(
            self.latest_model_revision
                .unwrap_or_default()
                .max(model_revision),
        );
        self.commit_candidates.insert(
            commit_candidate_hash.to_string(),
            VisibleWriteCommitCandidateProjection {
                patch_id: patch_id.to_string(),
                model_revision,
                editor_visible_hash: editor_visible_hash.to_string(),
                commit_candidate_hash: commit_candidate_hash.to_string(),
                commit_candidate_content: commit_candidate_content.map(str::to_string),
                source: source.to_string(),
            },
        );
    }

    pub fn applied_candidate<'a>(
        &'a self,
        transport: &'a TransportProjection,
        commit_candidate_hash: &str,
    ) -> Option<&'a VisibleWriteCommitCandidateProjection> {
        self.commit_candidates
            .get(commit_candidate_hash)
            .filter(|candidate| {
                transport
                    .patches
                    .get(&candidate.patch_id)
                    .is_some_and(|patch| patch.phase.is_applied())
            })
    }

    pub fn applied_candidate_for_patch<'a>(
        &'a self,
        transport: &'a TransportProjection,
        patch_id: &str,
    ) -> Option<&'a VisibleWriteCommitCandidateProjection> {
        self.commit_candidates
            .values()
            .filter(|candidate| candidate.patch_id == patch_id)
            .filter(|candidate| {
                transport
                    .patches
                    .get(&candidate.patch_id)
                    .is_some_and(|patch| patch.phase.is_applied())
            })
            .max_by_key(|candidate| candidate.model_revision)
    }

    pub fn observe_materialized_carry_forward(
        &mut self,
        model_revision: u64,
        live_buffer_hash: &str,
        file_content_hash: &str,
        commit_candidate_hash: &str,
        source: &str,
    ) {
        let current_revision = self
            .materialized_carry_forward
            .get(commit_candidate_hash)
            .map(|proof| proof.model_revision);
        if current_revision.is_some_and(|current| current > model_revision) {
            return;
        }
        self.latest_model_revision = Some(
            self.latest_model_revision
                .unwrap_or_default()
                .max(model_revision),
        );
        self.materialized_carry_forward.insert(
            commit_candidate_hash.to_string(),
            VisibleWriteMaterializedCarryForwardProjection {
                model_revision,
                live_buffer_hash: live_buffer_hash.to_string(),
                file_content_hash: file_content_hash.to_string(),
                commit_candidate_hash: commit_candidate_hash.to_string(),
                source: source.to_string(),
            },
        );
    }

    pub fn materialized_carry_forward(
        &self,
        commit_candidate_hash: &str,
        file_content_hash: &str,
        live_buffer_hash: &str,
    ) -> Option<&VisibleWriteMaterializedCarryForwardProjection> {
        self.materialized_carry_forward
            .get(commit_candidate_hash)
            .filter(|proof| {
                proof
                    .file_content_hash
                    .eq_ignore_ascii_case(file_content_hash)
                    && proof
                        .live_buffer_hash
                        .eq_ignore_ascii_case(live_buffer_hash)
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleWriteCommitCandidateProjection {
    pub patch_id: String,
    pub model_revision: u64,
    pub editor_visible_hash: String,
    pub commit_candidate_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_candidate_content: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleWriteMaterializedCarryForwardProjection {
    pub model_revision: u64,
    pub live_buffer_hash: String,
    pub file_content_hash: String,
    pub commit_candidate_hash: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportPatchProjection {
    pub phase: TransportPatchPhase,
    pub actor_generation: u64,
}

/// Pane→document host binding for a single document (`#xdocsuper1`/`#xdocsuper3`).
///
/// `pane_session` is the route-owned supervisor pane/session currently hosting
/// the document, `lease_epoch` is the supervisor lease incarnation that hosted
/// it, and `hosting_epoch` is the monotonic per-document counter that gates the
/// queue overlay. The counter advances on every fresh host / document switch so
/// queue facts from a prior hosting become no-ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorHostingProjection {
    pub pane_session: String,
    pub lease_epoch: u64,
    pub hosting_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SupervisorProjection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub owners: BTreeMap<StateOwner, OwnerProjection>,
    #[serde(default)]
    pub recycle: SupervisorRecycleProjection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_miss: Option<StartupMissProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupMissProjection {
    pub file: String,
    pub pane_id: String,
    pub session_id: String,
    pub harness: String,
    pub timestamp: u64,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_baseline_id: Option<String>,
}

impl SupervisorProjection {
    fn change_generation(&mut self, owner: StateOwner, generation: u64) {
        self.owners.insert(
            owner,
            OwnerProjection {
                generation: Some(generation),
                phase: ActorLifecyclePhase::Starting,
                capabilities: BTreeSet::new(),
            },
        );
    }

    fn apply_lifecycle_event(
        &mut self,
        owner: StateOwner,
        generation: u64,
        event: ActorLifecycleEvent,
    ) {
        let actor = self.owners.entry(owner).or_insert_with(|| OwnerProjection {
            generation: Some(generation),
            phase: ActorLifecyclePhase::Starting,
            capabilities: BTreeSet::new(),
        });
        actor.generation = Some(generation);
        if let Some(next) = ActorLifecycleMachine::transition(actor.phase, event) {
            actor.phase = next;
        }
    }

    fn capability_proof(&mut self, owner: StateOwner, generation: u64, capability: &str) {
        let actor = self.owners.entry(owner).or_insert_with(|| OwnerProjection {
            generation: Some(generation),
            phase: ActorLifecyclePhase::Starting,
            capabilities: BTreeSet::new(),
        });
        actor.generation = Some(generation);
        actor.capabilities.insert(capability.to_string());
        if let Some(next) = ActorLifecycleMachine::transition(
            actor.phase,
            ActorLifecycleEvent::CapabilityProofObserved,
        ) {
            actor.phase = next;
        }
    }

    fn apply_recycle_event(
        &mut self,
        event: SupervisorRecycleEvent,
        reason: &str,
        recycle_epoch: u64,
        marked_secs: u64,
    ) {
        if recycle_epoch < self.recycle.recycle_epoch {
            return;
        }
        if let Some(next) = SupervisorRecycleMachine::transition(self.recycle.phase, event) {
            self.recycle.phase = next;
            self.recycle.reason = Some(reason.to_string());
            self.recycle.recycle_epoch = recycle_epoch;
            self.recycle.marked_secs = marked_secs;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorRecycleProjection {
    pub phase: SupervisorRecyclePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub recycle_epoch: u64,
    pub marked_secs: u64,
}

impl Default for SupervisorRecycleProjection {
    fn default() -> Self {
        Self {
            phase: SupervisorRecyclePhase::Settled,
            reason: None,
            recycle_epoch: 0,
            marked_secs: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorRecyclePhase {
    Settled,
    /// A recycle/restart has been REQUESTED (stale binary, `admin recycle`,
    /// install fan-out, or a wedge trigger) but the `execve` has not begun. This
    /// is the "restart intent" state, modelled on the Lazily statechart so route
    /// callers and the editor projection see the pending restart durably. The
    /// request `reason` carries the cause (e.g. `stale_binary`).
    Requested,
    InFlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRecycleEvent {
    Requested,
    Started,
    Settled,
}

pub struct SupervisorRecycleMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<SupervisorRecyclePhase, SupervisorRecycleEvent>,
}

impl SupervisorRecycleMachine {
    pub fn new(initial: SupervisorRecyclePhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_supervisor_recycle);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: SupervisorRecyclePhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_supervisor_recycle);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> SupervisorRecyclePhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: SupervisorRecycleEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> SupervisorRecyclePhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: SupervisorRecyclePhase,
        event: SupervisorRecycleEvent,
    ) -> Option<SupervisorRecyclePhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_supervisor_recycle(
    current: &SupervisorRecyclePhase,
    event: &SupervisorRecycleEvent,
) -> Option<SupervisorRecyclePhase> {
    match event {
        // A pending request may be refreshed while still requested, but never
        // walks an in-flight recycle backwards. The refresh updates its durable
        // heartbeat without creating a second authority.
        SupervisorRecycleEvent::Requested => match current {
            SupervisorRecyclePhase::Settled | SupervisorRecyclePhase::Requested => {
                Some(SupervisorRecyclePhase::Requested)
            }
            SupervisorRecyclePhase::InFlight => None,
        },
        SupervisorRecycleEvent::Started => Some(SupervisorRecyclePhase::InFlight),
        SupervisorRecycleEvent::Settled => Some(SupervisorRecyclePhase::Settled),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateOwner {
    DocumentWriter,
    EditorIpcBridge,
    RouteDispatch,
    Supervisor,
    QueueOrchestrator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    pub phase: ActorLifecyclePhase,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RouteProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    pub readiness: RouteReadinessPhase,
    #[serde(default)]
    pub submit: RouteSubmitProjection,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub dispatch_proofs: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_actor_timeout: Option<StartingActorTimeoutProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartingActorTimeoutProjection {
    pub pane_id: String,
    pub generation: u64,
    pub log_line: String,
}

impl RouteProjection {
    fn apply_readiness_event(&mut self, event: RouteReadinessEvent) {
        if let Some(next) = RouteReadinessMachine::transition(self.readiness, event) {
            self.readiness = next;
        }
    }

    fn apply_submit_event(
        &mut self,
        event: RouteSubmitEvent,
        pane_id: &str,
        harness: &str,
        reason: &str,
        submit_epoch: u64,
        marked_secs: u64,
    ) {
        if submit_epoch < self.submit.submit_epoch {
            return;
        }
        if let Some(next) = RouteSubmitMachine::transition(self.submit.phase, event) {
            self.submit.phase = next;
            self.submit.pane_id = Some(pane_id.to_string());
            self.submit.harness = Some(harness.to_string());
            self.submit.reason = Some(reason.to_string());
            self.submit.submit_epoch = submit_epoch;
            self.submit.marked_secs = marked_secs;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RouteSubmitProjection {
    pub phase: RouteSubmitPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub submit_epoch: u64,
    pub marked_secs: u64,
}

impl RouteSubmitProjection {
    pub fn ttl_secs(&self) -> u64 {
        match self.phase {
            RouteSubmitPhase::Blocked => ROUTE_SUBMIT_BLOCKED_TTL_SECS,
            RouteSubmitPhase::InFlight
                if self.reason.as_deref() == Some(ROUTE_DISPATCH_ONLY_READY_PROBE_REASON) =>
            {
                ROUTE_SUBMIT_READY_PROBE_TTL_SECS
            }
            RouteSubmitPhase::InFlight => ROUTE_SUBMIT_IN_FLIGHT_TTL_SECS,
            RouteSubmitPhase::Idle => 0,
        }
    }

    pub fn is_pending_at(&self, now_secs: u64) -> bool {
        !matches!(self.phase, RouteSubmitPhase::Idle)
            && now_secs.saturating_sub(self.marked_secs) <= self.ttl_secs()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RouteSubmitPhase {
    #[default]
    Idle,
    InFlight,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSubmitEvent {
    Started,
    Settled,
    Blocked,
}

pub struct RouteSubmitMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<RouteSubmitPhase, RouteSubmitEvent>,
}

impl RouteSubmitMachine {
    pub fn new(initial: RouteSubmitPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_route_submit);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: RouteSubmitPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_route_submit);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> RouteSubmitPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: RouteSubmitEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> RouteSubmitPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: RouteSubmitPhase,
        event: RouteSubmitEvent,
    ) -> Option<RouteSubmitPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_route_submit(
    _current: &RouteSubmitPhase,
    event: &RouteSubmitEvent,
) -> Option<RouteSubmitPhase> {
    match event {
        RouteSubmitEvent::Started => Some(RouteSubmitPhase::InFlight),
        RouteSubmitEvent::Settled => Some(RouteSubmitPhase::Idle),
        RouteSubmitEvent::Blocked => Some(RouteSubmitPhase::Blocked),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProofProjection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub markers: BTreeMap<String, ProofMarkerProjection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub terminal_closeouts: BTreeMap<String, TerminalCloseoutProofProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_terminal_closeout_cycle_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub closeout_recovery_evidence: BTreeMap<String, CloseoutRecoveryEvidenceProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_closeout_recovery_evidence_key: Option<String>,
}

impl ProofProjection {
    fn apply(&mut self, marker: &str, source: &str, event: ProofGateEvent) {
        let projection =
            self.markers
                .entry(marker.to_string())
                .or_insert_with(|| ProofMarkerProjection {
                    phase: ProofGatePhase::Unknown,
                    sources: BTreeSet::new(),
                });
        projection.sources.insert(source.to_string());
        if let Some(next) = ProofGateMachine::transition(projection.phase, event) {
            projection.phase = next;
        }
    }

    fn apply_terminal_closeout(&mut self, proof: TerminalCloseoutProofProjection) {
        self.latest_terminal_closeout_cycle_id = Some(proof.cycle_id.clone());
        self.terminal_closeouts
            .insert(proof.cycle_id.clone(), proof);
    }

    fn apply_closeout_recovery_evidence(&mut self, evidence: CloseoutRecoveryEvidenceProjection) {
        self.latest_closeout_recovery_evidence_key = Some(evidence.evidence_key.clone());
        self.closeout_recovery_evidence
            .insert(evidence.evidence_key.clone(), evidence);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofMarkerProjection {
    pub phase: ProofGatePhase,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub sources: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCloseoutProofProjection {
    pub cycle_id: String,
    pub last_event: String,
    pub did_commit: bool,
    pub file_hash: String,
    pub snapshot_hash: String,
    pub head_hash: String,
    pub state_file_hash_matches: bool,
    pub state_snapshot_hash_matches: bool,
    pub agreement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutRecoveryEvidenceProjection {
    pub evidence_key: String,
    pub visible_markdown_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_cycle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_cycle_phase: Option<CyclePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_capture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_capture_cycle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_capture_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_capture_response_sha256: Option<String>,
    pub response_body: CloseoutRecoveryResponseBodyEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_only_drift: Option<CloseoutRecoveryQueueOnlyDriftEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_head_drift: Option<CloseoutRecoveryDriftEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_visible_drift: Option<CloseoutRecoveryDriftEvidence>,
    pub editor_ipc: CloseoutRecoveryEditorIpcEvidence,
    pub binary_freshness: CloseoutRecoveryBinaryFreshnessEvidence,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CloseoutRecoveryResponseBodyEvidence {
    NoActiveCapture,
    EmptyCapture { capture_id: String },
    PresentInVisible { capture_id: String },
    SupersededByVisibleExchange { capture_id: String, proof: String },
    MissingFromVisible { capture_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutRecoveryQueueOnlyDriftEvidence {
    pub file_hash_mismatch: bool,
    pub snapshot_hash_mismatch: bool,
    pub proven_queue_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseoutRecoveryDriftEvidence {
    BoundaryOnly,
    MetadataOnly,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CloseoutRecoveryEditorIpcEvidence {
    NoLiveBuffer {
        socket_degraded: bool,
    },
    FreshLiveBuffer {
        live_buffer_count: usize,
        socket_degraded: bool,
    },
    DivergedLiveBuffer {
        live_buffer_count: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editor_id: Option<String>,
        live_len: usize,
        live_hash: String,
        socket_degraded: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CloseoutRecoveryBinaryFreshnessEvidence {
    NoStaleWarning,
    Stale { warning: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueHeadPhase {
    Pending,
    Selected,
    Deferred,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueHeadEvent {
    Selected,
    Deferred,
    Completed,
    Requeued,
}

pub struct QueueHeadMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueHeadPhase, QueueHeadEvent>,
}

impl QueueHeadMachine {
    pub fn new(initial: QueueHeadPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_head);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: QueueHeadPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_queue_head);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> QueueHeadPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: QueueHeadEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> QueueHeadPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: QueueHeadPhase, event: QueueHeadEvent) -> Option<QueueHeadPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_queue_head(
    current: &QueueHeadPhase,
    event: &QueueHeadEvent,
) -> Option<QueueHeadPhase> {
    match event {
        QueueHeadEvent::Selected => match current {
            QueueHeadPhase::Pending | QueueHeadPhase::Selected | QueueHeadPhase::Deferred => {
                Some(QueueHeadPhase::Selected)
            }
            QueueHeadPhase::Completed => None,
        },
        QueueHeadEvent::Deferred => match current {
            QueueHeadPhase::Pending | QueueHeadPhase::Selected | QueueHeadPhase::Deferred => {
                Some(QueueHeadPhase::Deferred)
            }
            QueueHeadPhase::Completed => None,
        },
        QueueHeadEvent::Completed => Some(QueueHeadPhase::Completed),
        QueueHeadEvent::Requeued => match current {
            QueueHeadPhase::Pending | QueueHeadPhase::Selected | QueueHeadPhase::Deferred => {
                Some(QueueHeadPhase::Pending)
            }
            QueueHeadPhase::Completed => None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportPatchPhase {
    Queued,
    Applied,
    Rejected,
    InsufficientProof,
    Retrying,
    ForceDiskFallback,
}

impl TransportPatchPhase {
    pub fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }

    pub fn terminal_receipt_outcome(self) -> Option<ReceiptOutcome> {
        match self {
            Self::Applied => Some(ReceiptOutcome::Applied),
            Self::Rejected => Some(ReceiptOutcome::Rejected),
            Self::Queued | Self::InsufficientProof | Self::Retrying | Self::ForceDiskFallback => {
                None
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportPatchEvent {
    PatchQueued,
    PatchApplied,
    PatchRejected,
    ProofInsufficient,
    RetryRequested,
    ForceDiskFallback,
}

pub struct TransportPatchMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<TransportPatchPhase, TransportPatchEvent>,
}

impl TransportPatchMachine {
    pub fn new(initial: TransportPatchPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_transport_patch);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: TransportPatchPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_transport_patch);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> TransportPatchPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: TransportPatchEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> TransportPatchPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: TransportPatchPhase,
        event: TransportPatchEvent,
    ) -> Option<TransportPatchPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_transport_patch(
    current: &TransportPatchPhase,
    event: &TransportPatchEvent,
) -> Option<TransportPatchPhase> {
    match event {
        TransportPatchEvent::PatchQueued => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying => Some(TransportPatchPhase::Queued),
            TransportPatchPhase::Applied
            | TransportPatchPhase::Rejected
            | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::PatchApplied => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying
            | TransportPatchPhase::Applied => Some(TransportPatchPhase::Applied),
            TransportPatchPhase::Rejected | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::PatchRejected => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying
            | TransportPatchPhase::Rejected => Some(TransportPatchPhase::Rejected),
            TransportPatchPhase::Applied | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::ProofInsufficient => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying => Some(TransportPatchPhase::InsufficientProof),
            TransportPatchPhase::Applied
            | TransportPatchPhase::Rejected
            | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::RetryRequested => match current {
            TransportPatchPhase::InsufficientProof | TransportPatchPhase::Retrying => {
                Some(TransportPatchPhase::Retrying)
            }
            TransportPatchPhase::Queued
            | TransportPatchPhase::Applied
            | TransportPatchPhase::Rejected
            | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::ForceDiskFallback => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying
            | TransportPatchPhase::ForceDiskFallback => {
                Some(TransportPatchPhase::ForceDiskFallback)
            }
            TransportPatchPhase::Applied | TransportPatchPhase::Rejected => None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorLifecyclePhase {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Restarting,
    Stale,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorLifecycleEvent {
    StartingObserved,
    ReadyObserved,
    BusyObserved,
    WaitingInputObserved,
    RestartRequested,
    RestartPerformed,
    StaleObserved,
    ClosedObserved,
    CapabilityProofObserved,
}

pub struct ActorLifecycleMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<ActorLifecyclePhase, ActorLifecycleEvent>,
}

impl ActorLifecycleMachine {
    pub fn new(initial: ActorLifecyclePhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_actor_lifecycle);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: ActorLifecyclePhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_actor_lifecycle);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> ActorLifecyclePhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: ActorLifecycleEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> ActorLifecyclePhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: ActorLifecyclePhase,
        event: ActorLifecycleEvent,
    ) -> Option<ActorLifecyclePhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_actor_lifecycle(
    current: &ActorLifecyclePhase,
    event: &ActorLifecycleEvent,
) -> Option<ActorLifecyclePhase> {
    match event {
        ActorLifecycleEvent::StartingObserved => match current {
            ActorLifecyclePhase::Closed => None,
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::Ready
            | ActorLifecyclePhase::Busy
            | ActorLifecyclePhase::WaitingInput
            | ActorLifecyclePhase::Restarting
            | ActorLifecyclePhase::Stale => Some(ActorLifecyclePhase::Starting),
        },
        ActorLifecycleEvent::ReadyObserved | ActorLifecycleEvent::CapabilityProofObserved => {
            match current {
                ActorLifecyclePhase::Closed => None,
                ActorLifecyclePhase::Starting
                | ActorLifecyclePhase::Ready
                | ActorLifecyclePhase::Busy
                | ActorLifecyclePhase::WaitingInput
                | ActorLifecyclePhase::Restarting
                | ActorLifecyclePhase::Stale => Some(ActorLifecyclePhase::Ready),
            }
        }
        ActorLifecycleEvent::BusyObserved => match current {
            ActorLifecyclePhase::Ready | ActorLifecyclePhase::Busy => {
                Some(ActorLifecyclePhase::Busy)
            }
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::WaitingInput
            | ActorLifecyclePhase::Restarting
            | ActorLifecyclePhase::Stale
            | ActorLifecyclePhase::Closed => None,
        },
        ActorLifecycleEvent::WaitingInputObserved => match current {
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::Ready
            | ActorLifecyclePhase::Busy
            | ActorLifecyclePhase::WaitingInput => Some(ActorLifecyclePhase::WaitingInput),
            ActorLifecyclePhase::Restarting
            | ActorLifecyclePhase::Stale
            | ActorLifecyclePhase::Closed => None,
        },
        ActorLifecycleEvent::RestartRequested => match current {
            ActorLifecyclePhase::Closed => None,
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::Ready
            | ActorLifecyclePhase::Busy
            | ActorLifecyclePhase::WaitingInput
            | ActorLifecyclePhase::Restarting
            | ActorLifecyclePhase::Stale => Some(ActorLifecyclePhase::Restarting),
        },
        ActorLifecycleEvent::RestartPerformed => match current {
            ActorLifecyclePhase::Restarting | ActorLifecyclePhase::Stale => {
                Some(ActorLifecyclePhase::Starting)
            }
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::Ready
            | ActorLifecyclePhase::Busy
            | ActorLifecyclePhase::WaitingInput
            | ActorLifecyclePhase::Closed => None,
        },
        ActorLifecycleEvent::StaleObserved => match current {
            ActorLifecyclePhase::Closed => None,
            ActorLifecyclePhase::Starting
            | ActorLifecyclePhase::Ready
            | ActorLifecyclePhase::Busy
            | ActorLifecyclePhase::WaitingInput
            | ActorLifecyclePhase::Restarting
            | ActorLifecyclePhase::Stale => Some(ActorLifecyclePhase::Stale),
        },
        ActorLifecycleEvent::ClosedObserved => Some(ActorLifecyclePhase::Closed),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RouteReadinessPhase {
    #[default]
    Unknown,
    PaneKnown,
    PromptReady,
    DispatchAuthorized,
    DispatchAccepted,
    DispatchProven,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteReadinessEvent {
    PaneObserved,
    PromptReady,
    DispatchAuthorized,
    DispatchAccepted,
    DispatchProven,
    Blocked,
    Reset,
}

pub struct RouteReadinessMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<RouteReadinessPhase, RouteReadinessEvent>,
}

impl RouteReadinessMachine {
    pub fn new(initial: RouteReadinessPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_route_readiness);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: RouteReadinessPhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_route_readiness);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> RouteReadinessPhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: RouteReadinessEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> RouteReadinessPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: RouteReadinessPhase,
        event: RouteReadinessEvent,
    ) -> Option<RouteReadinessPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_route_readiness(
    current: &RouteReadinessPhase,
    event: &RouteReadinessEvent,
) -> Option<RouteReadinessPhase> {
    match event {
        RouteReadinessEvent::Reset => Some(RouteReadinessPhase::Unknown),
        RouteReadinessEvent::Blocked => Some(RouteReadinessPhase::Blocked),
        RouteReadinessEvent::PaneObserved => match current {
            RouteReadinessPhase::Unknown
            | RouteReadinessPhase::PaneKnown
            | RouteReadinessPhase::Blocked => Some(RouteReadinessPhase::PaneKnown),
            RouteReadinessPhase::PromptReady
            | RouteReadinessPhase::DispatchAuthorized
            | RouteReadinessPhase::DispatchAccepted
            | RouteReadinessPhase::DispatchProven => None,
        },
        RouteReadinessEvent::PromptReady => match current {
            RouteReadinessPhase::PaneKnown | RouteReadinessPhase::PromptReady => {
                Some(RouteReadinessPhase::PromptReady)
            }
            RouteReadinessPhase::Unknown
            | RouteReadinessPhase::DispatchAuthorized
            | RouteReadinessPhase::DispatchAccepted
            | RouteReadinessPhase::DispatchProven
            | RouteReadinessPhase::Blocked => None,
        },
        RouteReadinessEvent::DispatchAuthorized => match current {
            RouteReadinessPhase::PromptReady | RouteReadinessPhase::DispatchAuthorized => {
                Some(RouteReadinessPhase::DispatchAuthorized)
            }
            RouteReadinessPhase::Unknown
            | RouteReadinessPhase::PaneKnown
            | RouteReadinessPhase::DispatchAccepted
            | RouteReadinessPhase::DispatchProven
            | RouteReadinessPhase::Blocked => None,
        },
        RouteReadinessEvent::DispatchAccepted => match current {
            RouteReadinessPhase::DispatchAuthorized | RouteReadinessPhase::DispatchAccepted => {
                Some(RouteReadinessPhase::DispatchAccepted)
            }
            RouteReadinessPhase::Unknown
            | RouteReadinessPhase::PaneKnown
            | RouteReadinessPhase::PromptReady
            | RouteReadinessPhase::DispatchProven
            | RouteReadinessPhase::Blocked => None,
        },
        RouteReadinessEvent::DispatchProven => match current {
            RouteReadinessPhase::DispatchAccepted | RouteReadinessPhase::DispatchProven => {
                Some(RouteReadinessPhase::DispatchProven)
            }
            RouteReadinessPhase::Unknown
            | RouteReadinessPhase::PaneKnown
            | RouteReadinessPhase::PromptReady
            | RouteReadinessPhase::DispatchAuthorized
            | RouteReadinessPhase::Blocked => None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofGatePhase {
    Unknown,
    Observed,
    Disproved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofGateEvent {
    MarkerObserved,
    MarkerDisproved,
    Reset,
}

pub struct ProofGateMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<ProofGatePhase, ProofGateEvent>,
}

impl ProofGateMachine {
    pub fn new(initial: ProofGatePhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_proof_gate);
        Self { ctx, machine }
    }
    /// Construct this machine **inside an existing document state graph**
    /// (`#stategraphjoin`), so its cells share one `ThreadSafeContext` with the rest
    /// of the document's state instead of forming a private island.
    ///
    /// [`Self::new`] keeps its own context for standalone/pure-transition use; it is
    /// the isolated form, and every long-lived owner should prefer this one so cells
    /// can eventually derive from one another and invalidate together.
    pub fn new_in(scope: &DocumentScope, initial: ProofGatePhase) -> Self {
        let machine = ThreadSafeStateMachine::new(scope.ctx(), initial, transition_proof_gate);
        Self {
            ctx: scope.ctx().clone(),
            machine,
        }
    }

    /// Read this machine's state through an explicitly supplied context. Proves the
    /// cells really live in the graph's context rather than a private one.
    pub fn state_in(&self, ctx: &ThreadSafeContext) -> ProofGatePhase {
        self.machine.state(ctx)
    }


    pub fn send(&self, event: ProofGateEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> ProofGatePhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: ProofGatePhase, event: ProofGateEvent) -> Option<ProofGatePhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn transition_proof_gate(
    current: &ProofGatePhase,
    event: &ProofGateEvent,
) -> Option<ProofGatePhase> {
    match event {
        ProofGateEvent::Reset => Some(ProofGatePhase::Unknown),
        ProofGateEvent::MarkerObserved => match current {
            ProofGatePhase::Unknown | ProofGatePhase::Observed => Some(ProofGatePhase::Observed),
            ProofGatePhase::Disproved => None,
        },
        ProofGateEvent::MarkerDisproved => match current {
            ProofGatePhase::Unknown | ProofGatePhase::Disproved => Some(ProofGatePhase::Disproved),
            ProofGatePhase::Observed => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_at(cycle_id: &str, claimed: u64, lease: u64) -> CloseoutOwnerProjection {
        CloseoutOwnerProjection {
            cycle_id: cycle_id.to_string(),
            owner_id: "owner-old".to_string(),
            owner_pid: 4242,
            role: CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY.to_string(),
            claimed_secs: claimed,
            expires_secs: claimed + lease,
        }
    }

    #[test]
    fn an_owner_from_a_closed_turn_is_superseded_without_waiting_for_its_lease() {
        // #closeoutterminalreactive: the turn advanced, so the old settlement is
        // moot the instant the new turn opens. Waiting out its lease is waiting
        // on a guess for a fact already known. This is the 2026-07-26 wedge: a
        // session-check probe from a superseded turn blocked every later probe
        // until expiry, while its own refusal said to re-run the blocked command.
        let owner = owner_at("cycle-old", 1_000, CLOSEOUT_OWNER_LEASE_SECS);

        // Mid-lease, process alive: the clock alone would still be blocking.
        assert!(owner.is_active_at(1_010));
        assert_eq!(
            owner.release_reason("cycle-new", 1_010, Some(true), false),
            Some(CloseoutOwnerRelease::SupersededByNewTurn),
            "a new turn must release the old turn's claim without consulting the clock"
        );
    }

    #[test]
    fn an_owner_of_the_open_turn_still_blocks_while_its_lease_is_live() {
        // The other half: supersession must not become a way to steal a live
        // closeout out from under the process actually writing it.
        let owner = owner_at("cycle-open", 1_000, CLOSEOUT_OWNER_LEASE_SECS);

        assert_eq!(owner.release_reason("cycle-open", 1_010, Some(true), true), None);
        assert_eq!(
            owner.release_reason("cycle-open", 1_010, Some(false), true),
            Some(CloseoutOwnerRelease::OwnerProcessGone),
            "a dead owner is released by liveness evidence, not by its lease"
        );
    }

    #[test]
    fn lease_expiry_is_the_stopgap_and_is_reported_as_such() {
        // The timeout stays as a backstop, but it is distinguishable so it can
        // drive feedback: if this is ever the release reason in production, a
        // supersession path is missing.
        let owner = owner_at("cycle-open", 1_000, CLOSEOUT_RECOVERY_LEASE_SECS);
        let expired = 1_000 + CLOSEOUT_RECOVERY_LEASE_SECS;

        let reason = owner
            .release_reason("cycle-open", expired, Some(true), true)
            .expect("an expired lease must release");
        assert_eq!(reason, CloseoutOwnerRelease::LeaseExpired);
        assert!(reason.is_stopgap(), "the clock path must be flagged as the stopgap");
        assert!(!CloseoutOwnerRelease::SupersededByNewTurn.is_stopgap());
        assert!(!CloseoutOwnerRelease::OwnerProcessGone.is_stopgap());
    }

    #[test]
    fn a_stale_turn_owner_does_not_block_a_new_claim() {
        // End to end through the claim decision, which is what actually wedged.
        let mut closeout = CloseoutProjection {
            cycle_id: Some("cycle-new".to_string()),
            phase: Some(CyclePhase::PreflightStarted),
            ..Default::default()
        };
        closeout.owner = Some(owner_at("cycle-old", 1_000, CLOSEOUT_OWNER_LEASE_SECS));

        let outcome = closeout.decide_owner_claim(
            &CloseoutOwnerClaimRequest {
                expected_cycle_id: Some("cycle-new".to_string()),
                owner_id: "owner-new".to_string(),
                owner_pid: 5,
                role: CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY.to_string(),
                now_secs: 1_010,
                lease_secs: CLOSEOUT_RECOVERY_LEASE_SECS,
                allow_dead_owner_takeover: true,
            },
            // The old owner's process is still alive; only the turn moved on.
            Some(true),
        );

        assert!(
            matches!(outcome, CloseoutOwnerClaimOutcome::Acquired(_)),
            "expected Acquired, got {outcome:?}"
        );
    }

    #[test]
    fn a_status_only_recovery_probe_does_not_hold_the_write_closeout_lease() {
        // #closeoutwaitchurn: `session-check` is status-only, but it claimed the
        // full 300s write-closeout lease. A probe that lingered then blocked
        // every subsequent probe for five minutes — while the refusal it
        // produced told the operator to run that same command again. Observed
        // 2026-07-26 with a live `agent-doc session-check` pid holding
        // role=session_check_recovery for the full lease.
        let recovery = closeout_owner_lease_secs(CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY);
        let writing = closeout_owner_lease_secs("foreground_write_closeout");

        assert_eq!(writing, CLOSEOUT_OWNER_LEASE_SECS);
        assert_eq!(recovery, CLOSEOUT_RECOVERY_LEASE_SECS);
        assert!(
            recovery * 4 <= writing,
            "a status-only probe must reclaim in a small fraction of the write lease \
             (recovery={recovery}s, writing={writing}s)"
        );
    }

    #[test]
    fn an_expired_recovery_lease_stops_blocking_at_its_own_deadline() {
        // The lease is what a waiter is waiting on, so its deadline is the
        // waiter's worst case. Pin that the recovery deadline is derived from
        // the recovery lease and not from the write lease.
        let claimed = 1_000_u64;
        let owner = CloseoutOwnerProjection {
            cycle_id: "cycle-1".to_string(),
            owner_id: "owner-1".to_string(),
            owner_pid: 1,
            role: CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY.to_string(),
            claimed_secs: claimed,
            expires_secs: claimed + closeout_owner_lease_secs(CLOSEOUT_ROLE_SESSION_CHECK_RECOVERY),
        };

        assert!(owner.is_active_at(claimed + CLOSEOUT_RECOVERY_LEASE_SECS - 1));
        assert!(!owner.is_active_at(claimed + CLOSEOUT_RECOVERY_LEASE_SECS));
        assert!(
            !owner.is_active_at(claimed + CLOSEOUT_OWNER_LEASE_SECS),
            "a status-only probe must never block for the write-closeout lease"
        );
    }

    fn state_event(event_id: impl Into<String>, fact: StateFact) -> StateEvent {
        StateEvent::new(event_id, fact)
    }

    #[test]
    fn exact_false_stale_capture_reactivation_is_the_only_abandoned_cycle_reopen() {
        let mut projection = DocumentStateProjection::new("doc-reactivation");
        projection.apply(&StateFact::PreflightStarted {
            document_hash: "doc-reactivation".into(),
            cycle_id: "cycle-1".into(),
            session_id: None,
            tracked_work_maintenance_required: None,
        });
        projection.apply(&StateFact::ResponseCaptured {
            document_hash: "doc-reactivation".into(),
            cycle_id: "cycle-1".into(),
            capture_id: "cycle-1".into(),
            response_sha256: "response-1".into(),
            response_body: Some("response body".into()),
            intent_body: None,
            mutation_plan_json: None,
            file_hash: None,
            snapshot_hash: None,
            baseline_content: None,
        });
        projection.apply(&StateFact::CycleAbandoned {
            document_hash: "doc-reactivation".into(),
            cycle_id: "cycle-1".into(),
            reason: "repair_retire_superseded_captured_only_orphan".into(),
        });
        assert_eq!(projection.closeout.phase, Some(CyclePhase::Abandoned));

        projection.apply(&StateFact::FalseStaleCaptureReactivated {
            document_hash: "doc-reactivation".into(),
            cycle_id: "cycle-1".into(),
            capture_id: "wrong-capture".into(),
            response_sha256: "response-1".into(),
            retirement_reason: "repair_retire_superseded_captured_only_orphan".into(),
        });
        assert_eq!(
            projection.closeout.phase,
            Some(CyclePhase::Abandoned),
            "mismatched capture identity must remain terminal"
        );

        projection.apply(&StateFact::FalseStaleCaptureReactivated {
            document_hash: "doc-reactivation".into(),
            cycle_id: "cycle-1".into(),
            capture_id: "cycle-1".into(),
            response_sha256: "response-1".into(),
            retirement_reason: "repair_retire_superseded_captured_only_orphan".into(),
        });
        assert_eq!(
            projection.closeout.phase,
            Some(CyclePhase::ResponseCaptured)
        );
        assert!(projection.closeout.abandoned_reason.is_none());
    }

    #[test]
    fn closeout_owner_claim_is_a_lazily_projection_cas() {
        let mut projection = CloseoutProjection {
            cycle_id: Some("cycle-1".into()),
            phase: Some(CyclePhase::ResponseCaptured),
            ..CloseoutProjection::default()
        };
        let request = CloseoutOwnerClaimRequest {
            expected_cycle_id: Some("cycle-1".into()),
            owner_id: "foreground-1".into(),
            owner_pid: 101,
            role: "foreground_finalize".into(),
            now_secs: 10,
            lease_secs: 30,
            allow_dead_owner_takeover: true,
        };
        let acquired = projection.decide_owner_claim(&request, None);
        let CloseoutOwnerClaimOutcome::Acquired(owner) = acquired else {
            panic!("open Lazily cycle should be claimable");
        };
        assert_eq!(owner.cycle_id, "cycle-1");
        assert_eq!(owner.expires_secs, 40);
        projection.owner = Some(owner.clone());

        let recovery = CloseoutOwnerClaimRequest {
            expected_cycle_id: Some("cycle-1".into()),
            owner_id: "recovery-1".into(),
            owner_pid: 202,
            role: "session_check_recovery".into(),
            now_secs: 20,
            lease_secs: 30,
            allow_dead_owner_takeover: true,
        };
        assert_eq!(
            projection.decide_owner_claim(&recovery, Some(true)),
            CloseoutOwnerClaimOutcome::HeldByOther(owner.clone())
        );
        assert!(matches!(
            projection.decide_owner_claim(&recovery, Some(false)),
            CloseoutOwnerClaimOutcome::Acquired(CloseoutOwnerProjection {
                ref owner_id,
                owner_pid: 202,
                ..
            }) if owner_id == "recovery-1"
        ));

        let stale_cycle = CloseoutOwnerClaimRequest {
            expected_cycle_id: Some("cycle-0".into()),
            ..recovery
        };
        assert_eq!(
            projection.decide_owner_claim(&stale_cycle, Some(false)),
            CloseoutOwnerClaimOutcome::CycleSuperseded
        );
        assert!(projection.owner_release_matches("cycle-1", "foreground-1"));
        assert!(!projection.owner_release_matches("cycle-1", "recovery-1"));
    }

    #[test]
    fn exact_owner_release_cannot_clear_a_newer_lazily_claim() {
        let mut projection = DocumentStateProjection::new("doc-owner");
        projection.apply(&StateFact::PreflightStarted {
            document_hash: "doc-owner".into(),
            cycle_id: "cycle-1".into(),
            session_id: None,
            tracked_work_maintenance_required: None,
        });
        projection.apply(&StateFact::CloseoutOwnerClaimed {
            document_hash: "doc-owner".into(),
            cycle_id: "cycle-1".into(),
            owner_id: "owner-new".into(),
            owner_pid: 202,
            role: "recovery".into(),
            claimed_secs: 20,
            expires_secs: 50,
        });
        projection.apply(&StateFact::CloseoutOwnerReleased {
            document_hash: "doc-owner".into(),
            cycle_id: "cycle-1".into(),
            owner_id: "owner-old".into(),
            reason: "late_drop".into(),
            released_secs: 21,
        });
        assert_eq!(
            projection
                .closeout
                .owner
                .as_ref()
                .map(|owner| owner.owner_id.as_str()),
            Some("owner-new")
        );

        projection.apply(&StateFact::CloseoutOwnerReleased {
            document_hash: "doc-owner".into(),
            cycle_id: "cycle-1".into(),
            owner_id: "owner-new".into(),
            reason: "finished".into(),
            released_secs: 22,
        });
        assert!(projection.closeout.owner.is_none());
    }

    fn authority_event(
        event_id: &str,
        document_hash: &str,
        authority: DocumentAuthority,
        authority_epoch: u64,
    ) -> StateEvent {
        state_event(
            event_id,
            StateFact::DocumentAuthorityObserved {
                document_hash: document_hash.into(),
                authority,
                authority_epoch,
                source: "test".into(),
                reason: "test".into(),
                content_hash: None,
                editor_id: None,
            },
        )
    }

    #[test]
    fn realtime_steering_is_scoped_to_the_open_closeout_cycle() {
        use agent_doc_turn::cp_projection::{TurnSteeringProjection, TurnSteeringState};

        let mut projection = DocumentStateProjection::new("doc-steering");
        projection.apply(&StateFact::PreflightStarted {
            document_hash: "doc-steering".into(),
            cycle_id: "cycle-1".into(),
            session_id: None,
            tracked_work_maintenance_required: None,
        });
        projection.apply(&StateFact::RealtimeSteeringObserved {
            document_hash: "doc-steering".into(),
            cycle_id: "cycle-1".into(),
            steering: TurnSteeringProjection::observed_aggregate(
                TurnSteeringState::PromptTarget,
                2,
                Some("first prompt".into()),
                Some("both prompts verbatim".into()),
            ),
            content_hash: "hash-1".into(),
        });
        assert_eq!(projection.closeout.realtime_steering.count, 2);
        assert_eq!(
            projection.closeout.realtime_steering.verbatim.as_deref(),
            Some("both prompts verbatim")
        );

        projection.apply(&StateFact::CommitObserved {
            document_hash: "doc-steering".into(),
            cycle_id: "cycle-1".into(),
            commit: "abc123".into(),
            file_hash: None,
            snapshot_hash: None,
        });
        assert!(!projection.closeout.realtime_steering.is_present());

        projection.apply(&StateFact::PreflightStarted {
            document_hash: "doc-steering".into(),
            cycle_id: "cycle-2".into(),
            session_id: None,
            tracked_work_maintenance_required: None,
        });
        projection.apply(&StateFact::RealtimeSteeringObserved {
            document_hash: "doc-steering".into(),
            cycle_id: "cycle-1".into(),
            steering: TurnSteeringProjection::observed(
                TurnSteeringState::ContentEdit,
                Some("stale".into()),
            ),
            content_hash: "hash-stale".into(),
        });
        assert!(!projection.closeout.realtime_steering.is_present());
    }

    #[test]
    fn document_authority_rejects_late_disk_replica_after_active_editor() {
        let mut ledger = EventLedger::new();
        ledger.append(authority_event(
            "e1",
            "doc-authority",
            DocumentAuthority::EditorRelay,
            10,
        ));
        ledger.append(authority_event(
            "e2",
            "doc-authority",
            DocumentAuthority::DiskReplica,
            9,
        ));

        let projection = ledger
            .project_document("doc-authority")
            .expect("projection exists");
        let authority = projection
            .document
            .latest_authority
            .expect("authority projected");
        assert_eq!(authority.authority, DocumentAuthority::EditorRelay);
        assert_eq!(authority.authority_epoch, 10);
        assert_eq!(projection.rejected_stale_events.len(), 1);
        assert_eq!(
            projection.rejected_stale_events[0].domain,
            StateDomain::Document
        );
    }

    #[test]
    fn document_authority_allows_newer_disk_replica_after_detach() {
        let mut ledger = EventLedger::new();
        ledger.append(authority_event(
            "e1",
            "doc-authority-detach",
            DocumentAuthority::EditorRelay,
            10,
        ));
        ledger.append(authority_event(
            "e2",
            "doc-authority-detach",
            DocumentAuthority::DiskReplica,
            11,
        ));

        let projection = ledger
            .project_document("doc-authority-detach")
            .expect("projection exists");
        let authority = projection
            .document
            .latest_authority
            .expect("authority projected");
        assert_eq!(authority.authority, DocumentAuthority::DiskReplica);
        assert_eq!(authority.authority_epoch, 11);
        assert!(projection.rejected_stale_events.is_empty());
    }

    fn socket_apply_events(
        prefix: &str,
        document_hash: &str,
        patch_id: &str,
        generation: u64,
    ) -> Vec<StateEvent> {
        vec![
            state_event(
                format!("{prefix}:editor-generation"),
                StateFact::OwnerGenerationChanged {
                    document_hash: document_hash.into(),
                    owner: StateOwner::EditorIpcBridge,
                    generation,
                },
            ),
            state_event(
                format!("{prefix}:patch-queued"),
                StateFact::EditorPatchQueued {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                },
            ),
            state_event(
                format!("{prefix}:patch-applied"),
                StateFact::EditorPatchApplied {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                },
            ),
        ]
    }

    fn file_retry_then_apply_events(
        prefix: &str,
        document_hash: &str,
        patch_id: &str,
        generation: u64,
        reason: &str,
    ) -> Vec<StateEvent> {
        vec![
            state_event(
                format!("{prefix}:editor-generation-{generation}"),
                StateFact::OwnerGenerationChanged {
                    document_hash: document_hash.into(),
                    owner: StateOwner::EditorIpcBridge,
                    generation,
                },
            ),
            state_event(
                format!("{prefix}:patch-queued-{generation}"),
                StateFact::EditorPatchQueued {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                },
            ),
            state_event(
                format!("{prefix}:proof-insufficient"),
                StateFact::IpcProofInsufficient {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                    reason: reason.into(),
                },
            ),
            state_event(
                format!("{prefix}:retry-requested"),
                StateFact::EditorPatchRetryRequested {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                    reason: "retry_without_disk_write".into(),
                },
            ),
            state_event(
                format!("{prefix}:editor-generation-{}", generation + 1),
                StateFact::OwnerGenerationChanged {
                    document_hash: document_hash.into(),
                    owner: StateOwner::EditorIpcBridge,
                    generation: generation + 1,
                },
            ),
            state_event(
                format!("{prefix}:patch-requeued"),
                StateFact::EditorPatchQueued {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation + 1,
                },
            ),
            state_event(
                format!("{prefix}:patch-applied"),
                StateFact::EditorPatchApplied {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation + 1,
                },
            ),
        ]
    }

    fn route_started_events(
        prefix: &str,
        document_hash: &str,
        pane_id: &str,
        generation: u64,
    ) -> Vec<StateEvent> {
        vec![
            state_event(
                format!("{prefix}:route-generation"),
                StateFact::OwnerGenerationChanged {
                    document_hash: document_hash.into(),
                    owner: StateOwner::RouteDispatch,
                    generation,
                },
            ),
            state_event(
                format!("{prefix}:route-pane"),
                StateFact::RoutePaneObserved {
                    document_hash: document_hash.into(),
                    pane_id: pane_id.into(),
                    actor_generation: generation,
                },
            ),
            state_event(
                format!("{prefix}:route-prompt-ready"),
                StateFact::RouteReadinessObserved {
                    document_hash: document_hash.into(),
                    actor_generation: generation,
                    event: RouteReadinessEvent::PromptReady,
                },
            ),
            state_event(
                format!("{prefix}:route-started"),
                StateFact::RouteReadinessObserved {
                    document_hash: document_hash.into(),
                    actor_generation: generation,
                    event: RouteReadinessEvent::DispatchAuthorized,
                },
            ),
        ]
    }

    fn route_proven_events(
        prefix: &str,
        document_hash: &str,
        generation: u64,
        proof_id: &str,
    ) -> Vec<StateEvent> {
        vec![
            state_event(
                format!("{prefix}:route-accepted"),
                StateFact::RouteReadinessObserved {
                    document_hash: document_hash.into(),
                    actor_generation: generation,
                    event: RouteReadinessEvent::DispatchAccepted,
                },
            ),
            state_event(
                format!("{prefix}:route-proven"),
                StateFact::DispatchProofObserved {
                    document_hash: document_hash.into(),
                    actor_generation: generation,
                    proof_id: proof_id.into(),
                },
            ),
            state_event(
                format!("{prefix}:proof-marker-observed"),
                StateFact::ProofMarkerObserved {
                    document_hash: document_hash.into(),
                    marker: "dispatch_start".into(),
                    source: proof_id.into(),
                },
            ),
        ]
    }

    fn route_blocked_events(
        prefix: &str,
        document_hash: &str,
        generation: u64,
        reason: &str,
    ) -> Vec<StateEvent> {
        vec![
            state_event(
                format!("{prefix}:route-blocked"),
                StateFact::RouteReadinessObserved {
                    document_hash: document_hash.into(),
                    actor_generation: generation,
                    event: RouteReadinessEvent::Blocked,
                },
            ),
            state_event(
                format!("{prefix}:proof-marker-disproved"),
                StateFact::ProofMarkerDisproved {
                    document_hash: document_hash.into(),
                    marker: "dispatch_start".into(),
                    source: reason.into(),
                },
            ),
        ]
    }

    #[test]
    fn pending_response_projection_is_state_backbone_authority() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "p1",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: Some("session-1".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "p2",
            StateFact::ResponseCaptured {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                capture_id: "capture-1".into(),
                response_sha256: "sha-response".into(),
                response_body: Some("### Re: topic — gpt-5\n\nDone.\n".into()),
                intent_body: Some(
                    "<!-- agent:patch:exchange -->\nDone.\n<!-- /agent:patch:exchange -->\n<!-- agent:patch:backlog -->\n- [ ] [#bpcontract] Preserve every pending change.\n<!-- /agent:patch:backlog -->\n"
                        .into(),
                ),
                mutation_plan_json: Some("{\"pending_edit\":[\"#bpcontract=keep open\"]}".into()),
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        let captured = projected
            .closeout
            .captured_response
            .as_ref()
            .expect("canonical response proof");
        assert_eq!(captured.response_body, "### Re: topic — gpt-5\n\nDone.\n");
        assert!(
            captured
                .intent_body
                .as_deref()
                .is_some_and(|intent| intent.contains("[#bpcontract]")),
            "canonical response proof and replayable full intent must remain distinct"
        );
        let pending = projected.closeout.pending_response.unwrap();
        assert_eq!(pending.cycle_id, "cycle-1");
        assert_eq!(pending.capture_id, "capture-1");
        assert_eq!(
            pending.response_body,
            "<!-- agent:patch:exchange -->\nDone.\n<!-- /agent:patch:exchange -->\n<!-- agent:patch:backlog -->\n- [ ] [#bpcontract] Preserve every pending change.\n<!-- /agent:patch:backlog -->\n"
        );

        ledger.append(state_event(
            "p3",
            StateFact::WriteApplied {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                patch_id: Some("patch-1".into()),
                file_hash: None,
                snapshot_hash: None,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.closeout.pending_response, None);
        assert_eq!(
            projected.closeout.pending_response_clear_reason.as_deref(),
            Some("write_applied")
        );
    }

    #[test]
    fn response_cell_receipt_is_the_write_applied_transition_and_is_replay_safe() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "cell-preflight",
            StateFact::PreflightStarted {
                document_hash: "doc-cell".into(),
                cycle_id: "cycle-cell".into(),
                session_id: Some("session-cell".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "cell-pending",
            StateFact::ResponseCaptured {
                document_hash: "doc-cell".into(),
                cycle_id: "cycle-cell".into(),
                capture_id: "capture-cell".into(),
                response_sha256: "response-sha".into(),
                response_body: Some("### Re: cell — gpt-5\n\nDone.\n".into()),
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            },
        ));
        let receipt = state_event(
            "cell-receipt",
            StateFact::ResponseCellAdded {
                document_hash: "doc-cell".into(),
                cycle_id: "cycle-cell".into(),
                operation_id: "response-cell:cycle-cell:response-sha".into(),
                cell_id: "cell-id".into(),
                response_sha256: "response-sha".into(),
                content_hash: "content-sha".into(),
                applied: true,
            },
        );
        ledger.append(receipt.clone());
        ledger.append(receipt);
        assert_eq!(
            ledger.document_epoch("doc-cell"),
            3,
            "duplicate operation fact must not advance the accepted epoch"
        );

        let projected = ledger.project_document("doc-cell").unwrap();
        assert_eq!(projected.closeout.phase, Some(CyclePhase::WriteApplied));
        assert_eq!(projected.closeout.pending_response, None);
        assert_eq!(
            projected.closeout.pending_response_clear_reason.as_deref(),
            Some("response_cell_added")
        );
        let cell = projected
            .closeout
            .response_cell
            .expect("response cell receipt");
        assert_eq!(cell.cell_id, "cell-id");
        assert_eq!(cell.operation_id, "response-cell:cycle-cell:response-sha");
        assert!(cell.applied);
    }

    #[test]
    fn response_captured_projection_keeps_guard_payload_after_commit() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "r1",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: Some("session-1".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "r2",
            StateFact::ResponseCaptured {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                capture_id: "capture-1".into(),
                response_sha256: "sha-response".into(),
                response_body: Some("### Re: topic - gpt-5\n\nDone.\n".into()),
                intent_body: None,
                mutation_plan_json: None,
                file_hash: Some("file-sha".into()),
                snapshot_hash: Some("snapshot-sha".into()),
                baseline_content: Some("captured baseline\n".into()),
            },
        ));
        ledger.append(state_event(
            "r3",
            StateFact::WriteApplied {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                patch_id: Some("patch-1".into()),
                file_hash: None,
                snapshot_hash: None,
            },
        ));
        ledger.append(state_event(
            "r4",
            StateFact::CommitObserved {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                commit: "head-sha".into(),
                file_hash: None,
                snapshot_hash: None,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.closeout.phase, Some(CyclePhase::Committed));
        let captured = projected
            .closeout
            .captured_response
            .expect("captured response projection");
        assert_eq!(captured.cycle_id, "cycle-1");
        assert_eq!(captured.capture_id, "capture-1");
        assert_eq!(captured.response_sha256, "sha-response");
        assert_eq!(captured.response_body, "### Re: topic - gpt-5\n\nDone.\n");
        assert_eq!(captured.file_hash.as_deref(), Some("file-sha"));
        assert_eq!(captured.snapshot_hash.as_deref(), Some("snapshot-sha"));
        assert_eq!(
            captured.baseline_content.as_deref(),
            Some("captured baseline\n")
        );
    }

    #[test]
    fn write_applied_records_pending_response_clear_without_capture_event() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "p1",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: Some("session-1".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "p2",
            StateFact::WriteApplied {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                patch_id: Some("patch-1".into()),
                file_hash: None,
                snapshot_hash: None,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.closeout.pending_response, None);
        assert_eq!(
            projected.closeout.pending_response_clear_reason.as_deref(),
            Some("write_applied")
        );
    }

    #[test]
    fn document_cell_merge_ack_projection_carries_forward_for_one_cycle() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "cycle-1-start",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: None,
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "cycle-1-ack",
            StateFact::DocumentCellMergeAckRecorded {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                component: "exchange".into(),
                id: "node-1".into(),
                reason: "same_node_operator_override".into(),
                detail: "operator value won".into(),
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.closeout.pending_semantic_merge_acks.len(), 1);
        assert!(!projected.closeout.pending_semantic_merge_acks[0].surfaced);

        ledger.append(state_event(
            "cycle-2-start",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-2".into(),
                session_id: None,
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "cycle-2-carry",
            StateFact::DocumentCellMergeAckCarriedForward {
                document_hash: "doc-a".into(),
                source_cycle_id: Some("cycle-1".into()),
                target_cycle_id: "cycle-2".into(),
                component: "exchange".into(),
                id: "node-1".into(),
                reason: "same_node_operator_override".into(),
                detail: "operator value won".into(),
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.closeout.cycle_id.as_deref(), Some("cycle-2"));
        let ack = &projected.closeout.pending_semantic_merge_acks[0];
        assert!(ack.surfaced);
        assert_eq!(ack.recorded_cycle_id.as_deref(), Some("cycle-1"));

        ledger.append(state_event(
            "cycle-3-start",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-3".into(),
                session_id: None,
                tracked_work_maintenance_required: Some(false),
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        assert!(projected.closeout.pending_semantic_merge_acks.is_empty());
    }

    #[test]
    fn terminal_closeout_proof_projects_hash_agreement_payload() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "cycle-1-start",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: None,
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "cycle-1-commit",
            StateFact::CommitObserved {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                commit: "abc123".into(),
                file_hash: None,
                snapshot_hash: None,
            },
        ));
        ledger.append(state_event(
            "cycle-1-proof",
            StateFact::TerminalCloseoutProofRecorded {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                last_event: "commit_success".into(),
                did_commit: true,
                file_hash: "file-sha".into(),
                snapshot_hash: "head-sha".into(),
                head_hash: "head-sha".into(),
                state_file_hash_matches: false,
                state_snapshot_hash_matches: true,
                agreement: "snapshot_head_visible_drift".into(),
                capture_id: Some("capture-1".into()),
                response_sha256: Some("response-sha".into()),
                recorded_at_ms: 42,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(
            projected.proof.latest_terminal_closeout_cycle_id.as_deref(),
            Some("cycle-1")
        );
        let proof = projected
            .proof
            .terminal_closeouts
            .get("cycle-1")
            .expect("terminal proof projection");
        assert_eq!(proof.last_event, "commit_success");
        assert_eq!(proof.snapshot_hash, "head-sha");
        assert_eq!(proof.head_hash, "head-sha");
        assert_eq!(proof.file_hash, "file-sha");
        assert!(!proof.state_file_hash_matches);
        assert!(proof.state_snapshot_hash_matches);
        assert_eq!(proof.agreement, "snapshot_head_visible_drift");
        assert_eq!(proof.capture_id.as_deref(), Some("capture-1"));
        assert_eq!(proof.response_sha256.as_deref(), Some("response-sha"));
        assert_eq!(proof.recorded_at_ms, 42);
    }

    #[test]
    fn closeout_recovery_evidence_projects_recovery_payload() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "cycle-1-recovery-evidence",
            StateFact::CloseoutRecoveryEvidenceRecorded {
                document_hash: "doc-a".into(),
                evidence_key: "evidence-sha".into(),
                visible_markdown_hash: "visible-sha".into(),
                snapshot_hash: Some("snapshot-sha".into()),
                active_cycle_id: Some("cycle-1".into()),
                active_cycle_phase: Some(CyclePhase::ResponseCaptured),
                active_capture_id: Some("capture-1".into()),
                active_capture_cycle_id: Some("cycle-1".into()),
                active_capture_state: Some("captured".into()),
                active_capture_response_sha256: Some("response-sha".into()),
                response_body: CloseoutRecoveryResponseBodyEvidence::SupersededByVisibleExchange {
                    capture_id: "capture-1".into(),
                    proof: "heading is answered".into(),
                },
                queue_only_drift: Some(CloseoutRecoveryQueueOnlyDriftEvidence {
                    file_hash_mismatch: true,
                    snapshot_hash_mismatch: false,
                    proven_queue_only: true,
                }),
                snapshot_head_drift: Some(CloseoutRecoveryDriftEvidence::MetadataOnly),
                snapshot_visible_drift: Some(CloseoutRecoveryDriftEvidence::BoundaryOnly),
                editor_ipc: CloseoutRecoveryEditorIpcEvidence::DivergedLiveBuffer {
                    live_buffer_count: 1,
                    editor_id: Some("editor-1".into()),
                    live_len: 128,
                    live_hash: "live-sha".into(),
                    socket_degraded: true,
                },
                binary_freshness: CloseoutRecoveryBinaryFreshnessEvidence::Stale {
                    warning: "binary stale".into(),
                },
                recorded_at_ms: 42,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(
            projected
                .proof
                .latest_closeout_recovery_evidence_key
                .as_deref(),
            Some("evidence-sha")
        );
        let evidence = projected
            .proof
            .closeout_recovery_evidence
            .get("evidence-sha")
            .expect("closeout recovery evidence projection");
        assert_eq!(evidence.visible_markdown_hash, "visible-sha");
        assert_eq!(evidence.snapshot_hash.as_deref(), Some("snapshot-sha"));
        assert_eq!(evidence.active_cycle_id.as_deref(), Some("cycle-1"));
        assert_eq!(
            evidence.active_cycle_phase,
            Some(CyclePhase::ResponseCaptured)
        );
        assert_eq!(evidence.active_capture_id.as_deref(), Some("capture-1"));
        assert_eq!(
            evidence.active_capture_response_sha256.as_deref(),
            Some("response-sha")
        );
        assert_eq!(
            evidence
                .queue_only_drift
                .as_ref()
                .map(|drift| drift.proven_queue_only),
            Some(true)
        );
        assert_eq!(
            evidence.snapshot_head_drift,
            Some(CloseoutRecoveryDriftEvidence::MetadataOnly)
        );
        assert_eq!(
            evidence.snapshot_visible_drift,
            Some(CloseoutRecoveryDriftEvidence::BoundaryOnly)
        );
        assert!(matches!(
            evidence.response_body,
            CloseoutRecoveryResponseBodyEvidence::SupersededByVisibleExchange { .. }
        ));
        assert!(matches!(
            evidence.editor_ipc,
            CloseoutRecoveryEditorIpcEvidence::DivergedLiveBuffer { .. }
        ));
        assert!(matches!(
            evidence.binary_freshness,
            CloseoutRecoveryBinaryFreshnessEvidence::Stale { .. }
        ));
        assert_eq!(evidence.recorded_at_ms, 42);
    }

    #[test]
    fn replay_projection_reduces_events_idempotently() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "e1",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: Some("session-1".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "e2",
            StateFact::BaselineSaved {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                baseline_hash: "baseline-a".into(),
                baseline_path: Some(".agent-doc/baselines/doc-a.md".into()),
            },
        ));
        ledger.append(state_event(
            "e3",
            StateFact::OwnerGenerationChanged {
                document_hash: "doc-a".into(),
                owner: StateOwner::EditorIpcBridge,
                generation: 7,
            },
        ));
        ledger.append(state_event(
            "e4",
            StateFact::QueueHeadSelected {
                document_hash: "doc-a".into(),
                node_key: "queue-node-1".into(),
                backlog_id: Some("#advr-state".into()),
                prompt_text: None,
                drainable: true,
                hosting_epoch: None,
            },
        ));
        ledger.append(state_event(
            "e5",
            StateFact::ResponseCaptured {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                capture_id: "capture-1".into(),
                response_sha256: "sha-response".into(),
                response_body: None,
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            },
        ));
        ledger.append(state_event(
            "e6",
            StateFact::EditorPatchQueued {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
            },
        ));
        ledger.append(state_event(
            "e7",
            StateFact::IpcProofInsufficient {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
                reason: "missing_response_proof".into(),
            },
        ));
        ledger.append(state_event(
            "e8",
            StateFact::EditorPatchRetryRequested {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
                reason: "retry_without_disk_write".into(),
            },
        ));
        ledger.append(state_event(
            "e9",
            StateFact::EditorPatchApplied {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
            },
        ));
        ledger.append(state_event(
            "e10",
            StateFact::WriteApplied {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                patch_id: Some("patch-1".into()),
                file_hash: None,
                snapshot_hash: None,
            },
        ));
        ledger.append(state_event(
            "e11",
            StateFact::CommitObserved {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                commit: "abc123".into(),
                file_hash: None,
                snapshot_hash: None,
            },
        ));
        ledger.append(state_event(
            "e12",
            StateFact::SessionCheckPassed {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
            },
        ));
        ledger.append(state_event(
            "e13",
            StateFact::QueueHeadCompleted {
                document_hash: "doc-a".into(),
                node_key: "queue-node-1".into(),
                backlog_id: Some("#advr-state".into()),
                hosting_epoch: None,
            },
        ));

        let projection = ledger.project();
        let duplicate_replay =
            StateBackboneProjection::from_events(ledger.events().iter().chain(ledger.events()));
        assert_eq!(projection, duplicate_replay);

        let doc = projection.document("doc-a").unwrap();
        assert_eq!(doc.closeout.phase, Some(CyclePhase::Committed));
        assert!(doc.closeout.session_check_passed);
        assert_eq!(doc.queue.active_head, None);
        assert_eq!(
            doc.queue.heads["queue-node-1"].phase,
            QueueHeadPhase::Completed
        );
        assert_eq!(
            doc.transport.patches["patch-1"].phase,
            TransportPatchPhase::Applied
        );
    }

    #[test]
    fn file_watch_change_projects_latest_document_event() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "watch-1",
            StateFact::FileWatchChangeObserved {
                document_hash: "doc-watch".into(),
                path: "/tmp/doc-watch.md".into(),
                watch_generation: 1,
                content_hash: "hash-one".into(),
            },
        ));
        ledger.append(state_event(
            "watch-2",
            StateFact::FileWatchChangeObserved {
                document_hash: "doc-watch".into(),
                path: "/tmp/doc-watch.md".into(),
                watch_generation: 2,
                content_hash: "hash-two".into(),
            },
        ));

        let projection = ledger.project();
        let latest = projection
            .document("doc-watch")
            .and_then(|doc| doc.document.latest_file_watch_change.as_ref())
            .expect("watch projection");

        assert_eq!(latest.watch_generation, 2);
        assert_eq!(latest.content_hash, "hash-two");
    }

    #[test]
    fn document_disk_write_projection_is_monotonic_under_reordered_replay() {
        let newer = StateFact::DocumentDiskWriteObserved {
            document_hash: "doc-write".into(),
            generation: 20,
            content_len: 200,
            content_hash: "hash-new".into(),
            write_id: "write-new".into(),
            actor: "agent".into(),
        };
        let older = StateFact::DocumentDiskWriteObserved {
            document_hash: "doc-write".into(),
            generation: 10,
            content_len: 100,
            content_hash: "hash-old".into(),
            write_id: "write-old".into(),
            actor: "agent".into(),
        };

        let mut projection = DocumentStateProjection::new("doc-write");
        projection.apply_fact(&newer);
        projection.apply_fact(&older);
        projection.apply_fact(&newer);

        let latest = projection
            .document
            .latest_disk_write
            .expect("disk write projection");
        assert_eq!(latest.generation, 20);
        assert_eq!(latest.content_hash, "hash-new");
        assert_eq!(projection.rejected_stale_events.len(), 2);
    }

    #[test]
    fn cross_editor_live_ipc_projection_summaries_match_bridge_contract() {
        let mut events = Vec::new();
        events.extend(socket_apply_events(
            "jb-socket",
            "doc-jb-socket",
            "jb-socket-patch",
            11,
        ));
        events.extend(route_started_events(
            "jb-socket",
            "doc-jb-socket",
            "%11",
            31,
        ));
        events.extend(route_proven_events(
            "jb-socket",
            "doc-jb-socket",
            31,
            "jb-socket-dispatch",
        ));

        events.extend(file_retry_then_apply_events(
            "jb-file",
            "doc-jb-file",
            "jb-file-patch",
            21,
            "socket_ack_timeout",
        ));
        events.extend(route_started_events("jb-file", "doc-jb-file", "%12", 32));
        events.extend(route_blocked_events(
            "jb-file",
            "doc-jb-file",
            32,
            "route blocked before retry proof",
        ));

        events.extend(file_retry_then_apply_events(
            "vscode-file",
            "doc-vscode-file",
            "vscode-file-patch",
            41,
            "cp_delivery_timeout",
        ));
        events.extend(route_started_events(
            "vscode-file",
            "doc-vscode-file",
            "%13",
            33,
        ));
        events.extend(route_proven_events(
            "vscode-file",
            "doc-vscode-file",
            33,
            "vscode-file-dispatch",
        ));

        let projection = StateBackboneProjection::from_events(&events);

        let jb_socket = projection.document("doc-jb-socket").unwrap();
        assert_eq!(
            jb_socket.transport.patches["jb-socket-patch"].phase,
            TransportPatchPhase::Applied
        );
        assert_eq!(
            jb_socket.route.readiness,
            RouteReadinessPhase::DispatchProven
        );
        assert!(
            jb_socket
                .route
                .dispatch_proofs
                .contains("jb-socket-dispatch")
        );
        assert_eq!(
            serde_json::to_value(jb_socket.projection_summary()).unwrap(),
            serde_json::json!({
                "routeReadiness": "dispatch_proven",
                "routePaneId": "%11",
                "latestTransportPatchId": "jb-socket-patch",
                "latestTransportPhase": "applied",
                "proofMarkers": 1,
            })
        );
        assert_eq!(
            jb_socket.projection_summary().compact(),
            "route=dispatch_proven pane=%11 transport=jb-socket-patch:applied proof_markers=1"
        );

        let jb_file = projection.document("doc-jb-file").unwrap();
        assert_eq!(
            jb_file.transport.patches["jb-file-patch"].phase,
            TransportPatchPhase::Applied
        );
        assert_eq!(
            jb_file.transport.last_unproven_reason.as_deref(),
            Some("socket_ack_timeout")
        );
        assert_eq!(
            jb_file.transport.last_retry_reason.as_deref(),
            Some("retry_without_disk_write")
        );
        assert_eq!(jb_file.route.readiness, RouteReadinessPhase::Blocked);
        assert_eq!(
            jb_file.proof.markers["dispatch_start"].phase,
            ProofGatePhase::Disproved
        );
        assert_eq!(
            jb_file.projection_summary().compact(),
            "route=blocked pane=%12 transport=jb-file-patch:applied proof_markers=1"
        );

        let vscode_file = projection.document("doc-vscode-file").unwrap();
        assert_eq!(
            vscode_file.transport.patches["vscode-file-patch"].phase,
            TransportPatchPhase::Applied
        );
        assert_eq!(
            vscode_file.transport.last_unproven_reason.as_deref(),
            Some("cp_delivery_timeout")
        );
        assert_eq!(
            vscode_file.route.readiness,
            RouteReadinessPhase::DispatchProven
        );
        assert!(
            vscode_file
                .route
                .dispatch_proofs
                .contains("vscode-file-dispatch")
        );
        assert_eq!(
            vscode_file.projection_summary().compact(),
            "route=dispatch_proven pane=%13 transport=vscode-file-patch:applied proof_markers=1"
        );
    }

    #[test]
    fn starting_actor_timeout_is_a_clearable_route_projection() {
        let events = vec![
            state_event(
                "timeout-recorded",
                StateFact::StartingActorTimeoutRecorded {
                    document_hash: "doc-a".into(),
                    pane_id: "%15".into(),
                    generation: 9,
                    log_line: "actor still starting".into(),
                },
            ),
            state_event(
                "stale-timeout-clear",
                StateFact::StartingActorTimeoutCleared {
                    document_hash: "doc-a".into(),
                    pane_id: "%old".into(),
                    generation: 8,
                },
            ),
            state_event(
                "timeout-cleared",
                StateFact::StartingActorTimeoutCleared {
                    document_hash: "doc-a".into(),
                    pane_id: "%15".into(),
                    generation: 9,
                },
            ),
        ];

        let recorded = StateBackboneProjection::from_events(&events[..1]);
        assert_eq!(
            recorded
                .document("doc-a")
                .unwrap()
                .route
                .starting_actor_timeout,
            Some(StartingActorTimeoutProjection {
                pane_id: "%15".into(),
                generation: 9,
                log_line: "actor still starting".into(),
            })
        );

        let stale_clear = StateBackboneProjection::from_events(&events[..2]);
        assert!(
            stale_clear
                .document("doc-a")
                .unwrap()
                .route
                .starting_actor_timeout
                .is_some(),
            "a stale clear must not erase a newer route timeout record"
        );

        let cleared = StateBackboneProjection::from_events(&events);
        assert_eq!(
            cleared
                .document("doc-a")
                .unwrap()
                .route
                .starting_actor_timeout,
            None
        );
    }

    #[test]
    fn startup_miss_clear_is_identity_cas_and_cannot_erase_newer_owner() {
        let recorded = state_event(
            "startup-miss-recorded",
            StateFact::StartupMissRecorded {
                document_hash: "doc-a".into(),
                file: "/project/session.md".into(),
                pane_id: "%15".into(),
                session_id: "session-new".into(),
                harness: "claude".into(),
                timestamp: 20,
                origin: "routed_trigger".into(),
                cycle_baseline_id: Some("cycle-20".into()),
            },
        );
        let stale_clear = state_event(
            "startup-miss-stale-clear",
            StateFact::StartupMissCleared {
                document_hash: "doc-a".into(),
                pane_id: "%14".into(),
                session_id: "session-old".into(),
                timestamp: 19,
            },
        );
        let matching_clear = state_event(
            "startup-miss-matching-clear",
            StateFact::StartupMissCleared {
                document_hash: "doc-a".into(),
                pane_id: "%15".into(),
                session_id: "session-new".into(),
                timestamp: 20,
            },
        );

        let stale = StateBackboneProjection::from_events([&recorded, &stale_clear]);
        assert!(
            stale
                .document("doc-a")
                .unwrap()
                .supervisor
                .startup_miss
                .is_some()
        );

        let cleared =
            StateBackboneProjection::from_events([&recorded, &stale_clear, &matching_clear]);
        assert!(
            cleared
                .document("doc-a")
                .unwrap()
                .supervisor
                .startup_miss
                .is_none()
        );
    }

    #[test]
    fn stale_actor_reports_are_rejected_by_generation() {
        let events = vec![
            state_event(
                "e1",
                StateFact::OwnerGenerationChanged {
                    document_hash: "doc-a".into(),
                    owner: StateOwner::RouteDispatch,
                    generation: 2,
                },
            ),
            state_event(
                "e2",
                StateFact::RoutePaneObserved {
                    document_hash: "doc-a".into(),
                    pane_id: "%1".into(),
                    actor_generation: 1,
                },
            ),
            state_event(
                "e3",
                StateFact::RoutePaneObserved {
                    document_hash: "doc-a".into(),
                    pane_id: "%2".into(),
                    actor_generation: 2,
                },
            ),
        ];

        let projection = StateBackboneProjection::from_events(&events);
        let doc = projection.document("doc-a").unwrap();
        assert_eq!(doc.route.pane_id.as_deref(), Some("%2"));
        assert_eq!(doc.route.readiness, RouteReadinessPhase::PaneKnown);
        assert_eq!(
            doc.rejected_stale_events,
            vec![RejectedStaleEvent {
                domain: StateDomain::Route,
                owner: StateOwner::RouteDispatch,
            }]
        );
    }

    #[test]
    fn supervisor_hosting_switch_resets_stale_queue_overlay() {
        // `#xdocsuper1`/`#xdocsuper3`: a route-owned supervisor hosts doc-a, answers
        // a free-text queue head (completed), then the SAME pane re-hosts doc-a at a
        // higher lease epoch (a fresh host / stale-CRDT replay boundary). The prior
        // hosting's answered-head residue must be dropped, and a stale queue fact
        // stamped with the old hosting epoch must be rejected by construction.
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "host-1",
            StateFact::SupervisorHosting {
                document_hash: "doc-a".into(),
                pane_session: "%73:session-a".into(),
                lease_epoch: 1,
            },
        ));
        // Hosting epoch is now 1; the producer stamps queue facts with it.
        assert_eq!(ledger.document_hosting_epoch("doc-a"), Some(1));
        ledger.append(state_event(
            "sel-1",
            StateFact::QueueHeadSelected {
                document_hash: "doc-a".into(),
                node_key: "free-text-head".into(),
                backlog_id: None,
                prompt_text: None,
                drainable: true,
                hosting_epoch: Some(1),
            },
        ));
        ledger.append(state_event(
            "done-1",
            StateFact::QueueHeadCompleted {
                document_hash: "doc-a".into(),
                node_key: "free-text-head".into(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        let before = ledger.project_document("doc-a").unwrap();
        assert!(before.queue.completed_heads.contains("free-text-head"));

        // Fresh host on the same document (higher lease epoch) = switch boundary.
        ledger.append(state_event(
            "host-2",
            StateFact::SupervisorHosting {
                document_hash: "doc-a".into(),
                pane_session: "%73:session-a".into(),
                lease_epoch: 2,
            },
        ));
        let after = ledger.project_document("doc-a").unwrap();
        assert_eq!(after.hosting_epoch(), Some(2));
        assert!(
            after.queue.completed_heads.is_empty(),
            "fresh host must drop the prior hosting's answered-head residue"
        );
        assert!(after.queue.heads.is_empty());
        assert_eq!(after.queue.active_head, None);

        // A stale queue fact replayed at the OLD hosting epoch is a no-op.
        ledger.append(state_event(
            "stale-done",
            StateFact::QueueHeadCompleted {
                document_hash: "doc-a".into(),
                node_key: "free-text-head".into(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        let replayed = ledger.project_document("doc-a").unwrap();
        assert!(
            replayed.queue.completed_heads.is_empty(),
            "stale-epoch queue fact must be rejected, not re-inject the answered head"
        );
        assert_eq!(
            replayed.rejected_stale_events,
            vec![RejectedStaleEvent {
                domain: StateDomain::Queue,
                owner: StateOwner::QueueOrchestrator,
            }]
        );
    }

    #[test]
    fn cross_document_switch_does_not_contaminate_sibling_overlay() {
        // `#xdocsuper1`: ONE pane hosts doc-a (answers a head), then switches to
        // doc-b. doc-b's queue projection must read its own state with NO doc-a
        // overlay, and doc-a's state must be untouched by the switch.
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "a-host",
            StateFact::SupervisorHosting {
                document_hash: "doc-a".into(),
                pane_session: "%73:session-a".into(),
                lease_epoch: 1,
            },
        ));
        ledger.append(state_event(
            "a-done",
            StateFact::QueueHeadCompleted {
                document_hash: "doc-a".into(),
                node_key: "a-head".into(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        // Same pane now hosts doc-b.
        ledger.append(state_event(
            "b-host",
            StateFact::SupervisorHosting {
                document_hash: "doc-b".into(),
                pane_session: "%73:session-a".into(),
                lease_epoch: 1,
            },
        ));
        ledger.append(state_event(
            "b-sel",
            StateFact::QueueHeadSelected {
                document_hash: "doc-b".into(),
                node_key: "b-head".into(),
                backlog_id: None,
                prompt_text: None,
                drainable: true,
                hosting_epoch: Some(1),
            },
        ));

        let doc_b = ledger.project_document("doc-b").unwrap();
        assert_eq!(doc_b.hosting_epoch(), Some(1));
        assert!(
            !doc_b.queue.heads.contains_key("a-head"),
            "doc-b must not inherit doc-a's queue head"
        );
        assert!(doc_b.queue.completed_heads.is_empty());
        assert_eq!(doc_b.queue.active_head.as_deref(), Some("b-head"));

        let doc_a = ledger.project_document("doc-a").unwrap();
        assert!(
            doc_a.queue.completed_heads.contains("a-head"),
            "switching the pane to doc-b must not mutate doc-a's state"
        );
        assert_eq!(doc_a.hosting_epoch(), Some(1));
    }

    #[test]
    fn re_emit_same_hosting_is_idempotent() {
        // A re-emit of the same (pane_session, lease_epoch) must not bump the
        // hosting epoch or reset the queue overlay.
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "host-1",
            StateFact::SupervisorHosting {
                document_hash: "doc-a".into(),
                pane_session: "%5:session-a".into(),
                lease_epoch: 3,
            },
        ));
        ledger.append(state_event(
            "done-1",
            StateFact::QueueHeadCompleted {
                document_hash: "doc-a".into(),
                node_key: "head".into(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        // Different event_id, identical hosting binding (CRDT/supervisor replay).
        ledger.append(state_event(
            "host-1-replay",
            StateFact::SupervisorHosting {
                document_hash: "doc-a".into(),
                pane_session: "%5:session-a".into(),
                lease_epoch: 3,
            },
        ));
        let doc = ledger.project_document("doc-a").unwrap();
        assert_eq!(doc.hosting_epoch(), Some(1));
        assert!(
            doc.queue.completed_heads.contains("head"),
            "an idempotent re-host must not drop in-hosting queue state"
        );
    }

    #[test]
    fn visible_write_commit_candidate_requires_lazily_transport_event() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "editor-gen-7",
            StateFact::OwnerGenerationChanged {
                document_hash: "doc-a".into(),
                owner: StateOwner::EditorIpcBridge,
                generation: 7,
            },
        ));
        ledger.append(state_event(
            "patch-queued",
            StateFact::EditorPatchQueued {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
            },
        ));
        ledger.append(state_event(
            "candidate",
            StateFact::VisibleWriteCommitCandidateObserved {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                model_revision: 7,
                editor_visible_hash: "candidate-a".into(),
                commit_candidate_hash: "candidate-a".into(),
                commit_candidate_content: Some("editor-visible candidate".into()),
                source: "test".into(),
            },
        ));
        let doc = ledger.project_document("doc-a").unwrap();
        assert!(doc.applied_visible_write_candidate("candidate-a").is_none());

        ledger.append(state_event(
            "patch-applied",
            StateFact::EditorPatchApplied {
                document_hash: "doc-a".into(),
                patch_id: "patch-1".into(),
                actor_generation: 7,
            },
        ));
        let doc = ledger.project_document("doc-a").unwrap();
        let candidate = doc
            .applied_visible_write_candidate("candidate-a")
            .expect("applied candidate");
        assert_eq!(candidate.patch_id, "patch-1");
        assert_eq!(candidate.model_revision, 7);
        assert_eq!(
            candidate.commit_candidate_content.as_deref(),
            Some("editor-visible candidate")
        );
        assert!(doc.applied_visible_write_candidate("candidate-b").is_none());
    }

    #[test]
    fn deferred_document_write_remains_durable_until_matching_convergence() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "write-deferred",
            StateFact::DocumentWriteDeferred {
                document_hash: "doc-a".into(),
                intent_id: "intent-1".into(),
                expected_hash: "base".into(),
                expected_content: Some("editor-owned base".into()),
                target_hash: "target".into(),
                target_content: "editor-owned target".into(),
                source: "boundary".into(),
                reason: "editor_owner_without_registered_replica".into(),
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        let pending = projected
            .document
            .pending_write
            .as_ref()
            .expect("pending write");
        assert_eq!(pending.target_content, "editor-owned target");
        assert_eq!(
            pending.expected_content.as_deref(),
            Some("editor-owned base")
        );

        ledger.append(state_event(
            "wrong-write-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "intent-2".into(),
                target_hash: "target".into(),
                source: "test".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        assert!(
            ledger
                .project_document("doc-a")
                .unwrap()
                .document
                .pending_write
                .is_some(),
            "an unrelated receipt must not clear the intent"
        );

        ledger.append(state_event(
            "write-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "intent-1".into(),
                target_hash: "target".into(),
                source: "test".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        assert!(
            ledger
                .project_document("doc-a")
                .unwrap()
                .document
                .pending_write
                .is_none()
        );
    }

    #[test]
    fn superseding_closeout_stage_requires_a_newer_fact_at_a_later_stage() {
        let mut ledger = EventLedger::new();

        // A high-rank stage from before the retained intent must not satisfy it.
        // Stage ordering alone is insufficient across closeouts; the projection
        // ordinal supplies the chronological half of the proof.
        ledger.append(state_event(
            "older-reposition-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "older-reposition".into(),
                target_hash: "older-target".into(),
                source: "test".into(),
                intent_source: DocumentWriteSource::PostCommitReposition,
            },
        ));
        ledger.append(state_event(
            "write-deferred",
            StateFact::DocumentWriteDeferred {
                document_hash: "doc-a".into(),
                intent_id: "response-write".into(),
                expected_hash: "base".into(),
                expected_content: Some("base".into()),
                target_hash: "response-target".into(),
                target_content: "response".into(),
                source: DocumentWriteSource::PendingWrite,
                reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        let pending = projected.document.pending_write.as_ref().unwrap();
        assert_eq!(
            projected.document.superseding_closeout_stage(pending),
            None,
            "an older post-commit reposition must not supersede a newer response write"
        );

        // A newer write at the same stage is chronological evidence, but not a
        // closeout successor. Supersession is strict in both dimensions.
        ledger.append(state_event(
            "later-response-write-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "other-response-write".into(),
                target_hash: "other-response-target".into(),
                source: "test".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        let pending = projected.document.pending_write.as_ref().unwrap();
        assert_eq!(
            projected.document.superseding_closeout_stage(pending),
            None,
            "a newer write at the same stage must not count as a successor"
        );

        ledger.append(state_event(
            "queue-mirror-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "queue-mirror".into(),
                target_hash: "queue-mirror-target".into(),
                source: "test".into(),
                intent_source: DocumentWriteSource::PendingAddSync,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        let pending = projected.document.pending_write.as_ref().unwrap();
        assert_eq!(
            projected.document.superseding_closeout_stage(pending),
            Some(CloseoutStage::QueueMirror)
        );
    }

    /// `#stategraphjoin` — machines built from one [`DocumentScope`] must have their
    /// cells in THAT scope's context, not private ones.
    ///
    /// The assertion is deliberately not "it compiles": each machine already stores a
    /// context, so a private-island version type-checks identically. Reading state
    /// back through the *scope's* context is what distinguishes a joined graph from
    /// nine islands — a machine whose cells lived in its own context could not answer
    /// through this one.
    #[test]
    fn machines_built_from_a_scope_share_that_scopes_graph() {
        let document = DocumentScope::new();
        let drain = document.queue_drain_stall(QueueDrainStallPhase::Idle);
        let head = document.queue_head(QueueHeadPhase::Pending);

        assert_eq!(drain.state_in(document.ctx()), QueueDrainStallPhase::Idle);
        assert_eq!(head.state_in(document.ctx()), QueueHeadPhase::Pending);

        // A transition on one machine is visible through the shared context, and does
        // not disturb its neighbour in the same graph.
        assert!(drain.send(QueueDrainStallEvent::Recorded));
        assert_eq!(drain.state_in(document.ctx()), QueueDrainStallPhase::Pending);
        assert_eq!(
            head.state_in(document.ctx()),
            QueueHeadPhase::Pending,
            "joining one graph must not entangle unrelated machines"
        );

        // A separate scope is a separate lifetime and a separate graph.
        let other = DocumentScope::new();
        let other_drain = other.queue_drain_stall(QueueDrainStallPhase::Idle);
        assert_eq!(
            other_drain.state_in(other.ctx()),
            QueueDrainStallPhase::Idle,
            "a second scope starts its own graph"
        );
    }


    #[test]
    fn retained_capture_effect_rejects_overtaking_preflight_until_convergence() {
        let document_hash = "doc-retained";
        let cycle_1 = "cycle-retained";
        let cycle_2 = "cycle-overtaking";
        let response = "### Re: retained — gpt-5\n\nThe durable response.\n";
        let target = format!(
            "# Session\n\n<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            response.trim_end()
        );
        let mut ledger = EventLedger::new();

        ledger.append(state_event(
            "checkpoint-retained",
            StateFact::TurnIntentCheckpointed {
                document_hash: document_hash.into(),
                cycle_id: cycle_1.into(),
                checkpoint_sequence: 1,
                state_sha256: "checkpoint-retained-sha".into(),
                state_json: r#"{"cycle_id":"cycle-retained"}"#.into(),
            },
        ));
        ledger.append(state_event(
            "preflight-retained",
            StateFact::PreflightStarted {
                document_hash: document_hash.into(),
                cycle_id: cycle_1.into(),
                session_id: Some("session-retained".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));
        ledger.append(state_event(
            "capture-retained",
            StateFact::ResponseCaptured {
                document_hash: document_hash.into(),
                cycle_id: cycle_1.into(),
                capture_id: "capture-retained".into(),
                response_sha256: "response-retained-sha".into(),
                response_body: Some(response.into()),
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            },
        ));
        ledger.append(state_event(
            "write-retained",
            StateFact::DocumentWriteDeferred {
                document_hash: document_hash.into(),
                intent_id: "intent-retained".into(),
                expected_hash: "base".into(),
                expected_content: Some("# Session\n".into()),
                target_hash: "target-retained".into(),
                target_content: target,
                source: "finalize".into(),
                reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            },
        ));

        let retained = ledger.project_document(document_hash).unwrap();
        assert_eq!(
            retained
                .retained_captured_response_write()
                .map(|pending| pending.intent_id.as_str()),
            Some("intent-retained"),
        );

        // `#convergedderived` — the same retained entry must stop being retained the
        // moment the authority is observed holding its target, WITHOUT any
        // `DocumentWriteConverged` emission. A commit path that lands the write but
        // never emits that event used to leave this entry alive forever, blocking
        // every subsequent cycle for the document while `doctor`/`session-check`/
        // `repair` all reported clean (observed 2026-07-25).
        ledger.append(state_event(
            "authority-observed-target",
            StateFact::DocumentAuthorityObserved {
                document_hash: document_hash.into(),
                authority: DocumentAuthority::EditorRelay,
                authority_epoch: 1,
                source: "test".into(),
                reason: "converged".into(),
                content_hash: Some("target-retained".into()),
                editor_id: None,
            },
        ));
        let converged = ledger.project_document(document_hash).unwrap();
        assert!(
            converged.retained_captured_response_write().is_none(),
            "an intent whose target IS the observed authority content has converged \
             by derivation; a missed emission must self-heal, not wedge"
        );

        // A different authority content must NOT be mistaken for convergence.
        ledger.append(state_event(
            "authority-observed-other",
            StateFact::DocumentAuthorityObserved {
                document_hash: document_hash.into(),
                authority: DocumentAuthority::EditorRelay,
                authority_epoch: 2,
                source: "test".into(),
                reason: "still-pending".into(),
                content_hash: Some("some-other-content".into()),
                editor_id: None,
            },
        ));
        let still_retained = ledger.project_document(document_hash).unwrap();
        assert_eq!(
            still_retained
                .retained_captured_response_write()
                .map(|pending| pending.intent_id.as_str()),
            Some("intent-retained"),
            "an unlanded write must stay retained and keep blocking a new cycle"
        );

        // Reproduce the churn: a later session tries to checkpoint and start a
        // new preflight while the captured response is still awaiting ACK.
        ledger.append(state_event(
            "checkpoint-overtaking",
            StateFact::TurnIntentCheckpointed {
                document_hash: document_hash.into(),
                cycle_id: cycle_2.into(),
                checkpoint_sequence: 2,
                state_sha256: "checkpoint-overtaking-sha".into(),
                state_json: r#"{"cycle_id":"cycle-overtaking"}"#.into(),
            },
        ));
        ledger.append(state_event(
            "preflight-overtaking",
            StateFact::PreflightStarted {
                document_hash: document_hash.into(),
                cycle_id: cycle_2.into(),
                session_id: Some("session-overtaking".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));

        let still_retained = ledger.project_document(document_hash).unwrap();
        assert_eq!(still_retained.closeout.cycle_id.as_deref(), Some(cycle_1));
        assert_eq!(
            still_retained
                .closeout
                .turn_intent_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.cycle_id.as_str()),
            Some(cycle_1),
        );
        assert_eq!(
            still_retained
                .closeout
                .captured_response
                .as_ref()
                .map(|capture| capture.capture_id.as_str()),
            Some("capture-retained"),
        );

        ledger.append(state_event(
            "write-retained-converged",
            StateFact::DocumentWriteConverged {
                document_hash: document_hash.into(),
                intent_id: "intent-retained".into(),
                target_hash: "target-retained".into(),
                source: "editor_ack".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        ledger.append(state_event(
            "checkpoint-after-convergence",
            StateFact::TurnIntentCheckpointed {
                document_hash: document_hash.into(),
                cycle_id: cycle_2.into(),
                checkpoint_sequence: 3,
                state_sha256: "checkpoint-after-convergence-sha".into(),
                state_json: r#"{"cycle_id":"cycle-overtaking"}"#.into(),
            },
        ));
        ledger.append(state_event(
            "preflight-after-convergence",
            StateFact::PreflightStarted {
                document_hash: document_hash.into(),
                cycle_id: cycle_2.into(),
                session_id: Some("session-overtaking".into()),
                tracked_work_maintenance_required: Some(false),
            },
        ));

        let advanced = ledger.project_document(document_hash).unwrap();
        assert_eq!(advanced.closeout.cycle_id.as_deref(), Some(cycle_2));
        assert_eq!(
            advanced
                .closeout
                .turn_intent_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.checkpoint_sequence),
            Some(3),
        );
    }

    #[test]
    fn deferred_agent_changes_are_journaled_and_settle_monotonically() {
        let mut ledger = EventLedger::new();
        for (intent, target) in [("intent-1", "target-1"), ("intent-2", "target-2")] {
            ledger.append(state_event(
                format!("deferred-{intent}"),
                StateFact::DocumentWriteDeferred {
                    document_hash: "doc-a".into(),
                    intent_id: intent.into(),
                    expected_hash: "base".into(),
                    expected_content: Some("editor base".into()),
                    target_hash: target.into(),
                    target_content: format!("composed {target}"),
                    source: "finalize".into(),
                    reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
                },
            ));
        }

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.document.pending_write_journal.len(), 2);
        assert_eq!(
            projected
                .document
                .pending_write
                .as_ref()
                .map(|pending| pending.intent_id.as_str()),
            Some("intent-2")
        );

        // A late ACK for the older target settles only the prefix it proves;
        // the newer composed target remains pending.
        ledger.append(state_event(
            "converged-intent-1",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "intent-1".into(),
                target_hash: "target-1".into(),
                source: "editor_ack".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(projected.document.pending_write_journal.len(), 1);
        assert_eq!(
            projected.document.pending_write_journal[0].intent_id,
            "intent-2"
        );

        ledger.append(state_event(
            "converged-intent-2",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "intent-2".into(),
                target_hash: "target-2".into(),
                source: "editor_ack".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        assert!(projected.document.pending_write_journal.is_empty());
        assert!(projected.document.pending_write.is_none());
    }

    #[test]
    fn external_disk_candidate_does_not_replace_agent_write_lineage() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "agent-write-deferred",
            StateFact::DocumentWriteDeferred {
                document_hash: "doc-a".into(),
                intent_id: "agent-intent".into(),
                expected_hash: "editor-base".into(),
                expected_content: Some("editor base".into()),
                target_hash: "agent-target".into(),
                target_content: "agent response".into(),
                source: "finalize".into(),
                reason: DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            },
        ));
        ledger.append(state_event(
            "external-disk-deferred",
            StateFact::DocumentWriteDeferred {
                document_hash: "doc-a".into(),
                intent_id: "disk-intent".into(),
                expected_hash: "editor-base".into(),
                expected_content: Some("editor base".into()),
                target_hash: "disk-target".into(),
                target_content: "external disk edit".into(),
                source: "file_watch".into(),
                reason: DocumentWriteDeferredReason::PendingUserDecisionExternalDiskVsEditor,
            },
        ));

        let projected = ledger.project_document("doc-a").unwrap();
        assert_eq!(
            projected
                .document
                .pending_write
                .as_ref()
                .map(|pending| pending.intent_id.as_str()),
            Some("agent-intent")
        );
        assert_eq!(
            projected
                .document
                .pending_external_disk
                .as_ref()
                .map(|pending| pending.intent_id.as_str()),
            Some("disk-intent")
        );

        ledger.append(state_event(
            "external-disk-converged",
            StateFact::DocumentWriteConverged {
                document_hash: "doc-a".into(),
                intent_id: "disk-intent".into(),
                target_hash: "disk-target".into(),
                source: "editor_save".into(),
                intent_source: DocumentWriteSource::PendingWrite,
            },
        ));
        let projected = ledger.project_document("doc-a").unwrap();
        assert!(projected.document.pending_external_disk.is_none());
        assert_eq!(
            projected
                .document
                .pending_write
                .as_ref()
                .map(|pending| pending.intent_id.as_str()),
            Some("agent-intent")
        );
    }

    #[test]
    fn visible_write_materialized_carry_forward_requires_all_hashes() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "carry-forward",
            StateFact::VisibleWriteMaterializedCarryForwardObserved {
                document_hash: "doc-a".into(),
                model_revision: 3,
                live_buffer_hash: "live-a".into(),
                file_content_hash: "file-a".into(),
                commit_candidate_hash: "candidate-a".into(),
                source: "test".into(),
            },
        ));
        let doc = ledger.project_document("doc-a").unwrap();
        let proof = doc
            .materialized_visible_write_carry_forward("candidate-a", "file-a", "live-a")
            .expect("materialized carry-forward proof");
        assert_eq!(proof.model_revision, 3);
        assert!(
            doc.materialized_visible_write_carry_forward("candidate-a", "file-b", "live-a")
                .is_none(),
            "file hash must match the proof"
        );
        assert!(
            doc.materialized_visible_write_carry_forward("candidate-a", "file-a", "live-b")
                .is_none(),
            "live-buffer hash must match the proof"
        );
    }

    #[test]
    fn supervisor_recycle_projection_folds_requested_then_started_then_settled() {
        // The lazily statechart models the pending recycle/restart intent
        // (`Requested`) that used to live only in the on-disk recycle-request marker.
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "recycle-requested-1",
            StateFact::SupervisorRecycleRequested {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "stale_binary".into(),
                recycle_epoch: 1,
                marked_secs: 5,
            },
        ));
        let requested = ledger.project().project_supervisor_recycle();
        assert_eq!(requested.phase, SupervisorRecyclePhase::Requested);
        assert_eq!(requested.reason.as_deref(), Some("stale_binary"));

        // A duplicate request while already Requested is a no-op (chart stays put).
        ledger.append(state_event(
            "recycle-requested-2",
            StateFact::SupervisorRecycleRequested {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "admin_recycle".into(),
                recycle_epoch: 2,
                marked_secs: 6,
            },
        ));
        assert_eq!(
            ledger.project().project_supervisor_recycle().phase,
            SupervisorRecyclePhase::Requested,
            "a second request must not walk the chart backwards or double-arm"
        );

        // The actual recycle begins → InFlight, then settles.
        ledger.append(state_event(
            "recycle-started-1",
            StateFact::SupervisorRecycleStarted {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "stale_binary".into(),
                recycle_epoch: 3,
                marked_secs: 7,
            },
        ));
        assert_eq!(
            ledger.project().project_supervisor_recycle().phase,
            SupervisorRecyclePhase::InFlight
        );

        // A stale (lower-epoch) request after the recycle is in flight is rejected —
        // it must NOT re-arm a recycle already underway.
        ledger.append(state_event(
            "recycle-requested-stale",
            StateFact::SupervisorRecycleRequested {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "stale_race".into(),
                recycle_epoch: 2,
                marked_secs: 8,
            },
        ));
        assert_eq!(
            ledger.project().project_supervisor_recycle().phase,
            SupervisorRecyclePhase::InFlight,
            "a stale-epoch request must not re-arm an in-flight recycle"
        );

        ledger.append(state_event(
            "recycle-settled-1",
            StateFact::SupervisorRecycleSettled {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "watch_loop_started".into(),
                recycle_epoch: 3,
                marked_secs: 9,
            },
        ));
        assert_eq!(
            ledger.project().project_supervisor_recycle().phase,
            SupervisorRecyclePhase::Settled
        );
    }

    #[test]
    fn supervisor_recycle_projection_folds_started_and_settled() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "recycle-started-1",
            StateFact::SupervisorRecycleStarted {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "auto_install".into(),
                recycle_epoch: 1,
                marked_secs: 10,
            },
        ));
        let started = ledger.project().project_supervisor_recycle();
        assert_eq!(started.phase, SupervisorRecyclePhase::InFlight);
        assert_eq!(started.reason.as_deref(), Some("auto_install"));
        assert_eq!(started.recycle_epoch, 1);

        ledger.append(state_event(
            "recycle-settled-1",
            StateFact::SupervisorRecycleSettled {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "watch_loop_started".into(),
                recycle_epoch: 1,
                marked_secs: 11,
            },
        ));
        let settled = ledger.project().project_supervisor_recycle();
        assert_eq!(settled.phase, SupervisorRecyclePhase::Settled);
        assert_eq!(settled.reason.as_deref(), Some("watch_loop_started"));
        assert_eq!(settled.recycle_epoch, 1);

        ledger.append(state_event(
            "recycle-settled-0",
            StateFact::SupervisorRecycleSettled {
                document_hash: PROJECT_SUPERVISOR_DOCUMENT_HASH.into(),
                reason: "stale".into(),
                recycle_epoch: 0,
                marked_secs: 12,
            },
        ));
        let after_stale = ledger.project().project_supervisor_recycle();
        assert_eq!(after_stale.phase, SupervisorRecyclePhase::Settled);
        assert_eq!(after_stale.reason.as_deref(), Some("watch_loop_started"));
    }

    #[test]
    fn route_submit_projection_folds_started_blocked_settled_and_freshness() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "route-submit-started",
            StateFact::RouteSubmitStarted {
                document_hash: "doc-a".into(),
                pane_id: "%1".into(),
                harness: "codex".into(),
                reason: ROUTE_DISPATCH_ONLY_READY_PROBE_REASON.into(),
                submit_epoch: 1,
                marked_secs: 1_000,
            },
        ));

        let projection = ledger.project();
        let submit = &projection.document("doc-a").unwrap().route.submit;
        assert_eq!(submit.phase, RouteSubmitPhase::InFlight);
        assert_eq!(submit.ttl_secs(), ROUTE_SUBMIT_READY_PROBE_TTL_SECS);
        assert!(submit.is_pending_at(1_000 + ROUTE_SUBMIT_IN_FLIGHT_TTL_SECS + 1));
        assert!(!submit.is_pending_at(1_000 + ROUTE_SUBMIT_READY_PROBE_TTL_SECS + 1));

        ledger.append(state_event(
            "route-submit-blocked",
            StateFact::RouteSubmitBlocked {
                document_hash: "doc-a".into(),
                pane_id: "%1".into(),
                harness: "codex".into(),
                reason: "accepted_without_dispatch_start_proof".into(),
                submit_epoch: 2,
                marked_secs: 2_000,
            },
        ));
        let projection = ledger.project();
        let submit = &projection.document("doc-a").unwrap().route.submit;
        assert_eq!(submit.phase, RouteSubmitPhase::Blocked);
        assert_eq!(submit.ttl_secs(), ROUTE_SUBMIT_BLOCKED_TTL_SECS);
        assert!(submit.is_pending_at(2_000 + ROUTE_SUBMIT_BLOCKED_TTL_SECS));
        assert!(!submit.is_pending_at(2_000 + ROUTE_SUBMIT_BLOCKED_TTL_SECS + 1));

        ledger.append(state_event(
            "route-submit-stale-settled",
            StateFact::RouteSubmitSettled {
                document_hash: "doc-a".into(),
                pane_id: "%1".into(),
                harness: "codex".into(),
                reason: "stale_drop".into(),
                submit_epoch: 1,
                marked_secs: 2_001,
            },
        ));
        let projection = ledger.project();
        assert_eq!(
            projection.document("doc-a").unwrap().route.submit.phase,
            RouteSubmitPhase::Blocked
        );

        ledger.append(state_event(
            "route-submit-settled",
            StateFact::RouteSubmitSettled {
                document_hash: "doc-a".into(),
                pane_id: "%1".into(),
                harness: "codex".into(),
                reason: "guard_dropped".into(),
                submit_epoch: 2,
                marked_secs: 2_002,
            },
        ));
        let projection = ledger.project();
        assert_eq!(
            projection.document("doc-a").unwrap().route.submit.phase,
            RouteSubmitPhase::Idle
        );
    }

    #[test]
    fn queue_context_clear_projection_folds_started_settled_and_freshness() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "context-clear-started-1",
            StateFact::QueueContextClearStarted {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("operator_deferred_clear".into()),
                head_sha256: Some("head-hash".into()),
                head_bytes: Some(12),
                clear_epoch: 1,
                marked_secs: 1_000,
            },
        ));

        let projection = ledger.project();
        let context_clear = &projection.document("doc-a").unwrap().queue.context_clear;
        assert_eq!(context_clear.phase, QueueContextClearPhase::InFlight);
        assert_eq!(
            context_clear.ttl_secs(),
            QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS
        );
        assert!(context_clear.is_pending_at(1_000 + QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS));
        assert!(!context_clear.is_pending_at(1_000 + QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS + 1));
        assert_eq!(context_clear.head_sha256.as_deref(), Some("head-hash"));

        ledger.append(state_event(
            "context-clear-started-2",
            StateFact::QueueContextClearStarted {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("queue_slash_clear".into()),
                head_sha256: Some("new-head".into()),
                head_bytes: Some(24),
                clear_epoch: 2,
                marked_secs: 2_000,
            },
        ));
        ledger.append(state_event(
            "context-clear-stale-settled",
            StateFact::QueueContextClearSettled {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("stale".into()),
                clear_epoch: 1,
                marked_secs: 2_001,
            },
        ));
        let projection = ledger.project();
        let context_clear = &projection.document("doc-a").unwrap().queue.context_clear;
        assert_eq!(context_clear.phase, QueueContextClearPhase::InFlight);
        assert_eq!(context_clear.clear_epoch, 2);
        assert_eq!(context_clear.head_sha256.as_deref(), Some("new-head"));

        ledger.append(state_event(
            "context-clear-settled",
            StateFact::QueueContextClearSettled {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("settled".into()),
                clear_epoch: 2,
                marked_secs: 2_002,
            },
        ));
        let projection = ledger.project();
        assert_eq!(
            projection
                .document("doc-a")
                .unwrap()
                .queue
                .context_clear
                .phase,
            QueueContextClearPhase::Idle
        );
    }

    #[test]
    fn queue_context_clear_projection_tracks_deferred_operator_clear_without_ttl() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "context-clear-deferred",
            StateFact::QueueContextClearDeferred {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("operator_deferred_clear".into()),
                head_sha256: Some("head-hash".into()),
                head_bytes: Some(12),
                clear_epoch: 1,
                marked_secs: 1_000,
            },
        ));

        let projection = ledger.project();
        let context_clear = &projection.document("doc-a").unwrap().queue.context_clear;
        assert_eq!(context_clear.phase, QueueContextClearPhase::Deferred);
        assert!(context_clear.is_deferred_operator_clear());
        assert!(!context_clear.is_pending_at(1_000));
        assert!(!context_clear.is_pending_at(10_000));

        ledger.append(state_event(
            "context-clear-started",
            StateFact::QueueContextClearStarted {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some("operator_deferred_clear".into()),
                head_sha256: Some("head-hash".into()),
                head_bytes: Some(12),
                clear_epoch: 2,
                marked_secs: 2_000,
            },
        ));
        let projection = ledger.project();
        assert_eq!(
            projection
                .document("doc-a")
                .unwrap()
                .queue
                .context_clear
                .phase,
            QueueContextClearPhase::InFlight
        );
    }

    #[test]
    fn queue_context_clear_projection_tracks_manual_operator_cooldown_as_named_state() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "context-clear-manual-cooldown",
            StateFact::QueueContextClearStarted {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some(QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_MANUAL_COOLDOWN.into()),
                head_sha256: None,
                head_bytes: None,
                clear_epoch: 1,
                marked_secs: 1_000,
            },
        ));

        let projection = ledger.project();
        let context_clear = &projection.document("doc-a").unwrap().queue.context_clear;
        assert_eq!(context_clear.phase, QueueContextClearPhase::InFlight);
        assert!(context_clear.is_manual_operator_clear_cooldown());
        assert!(context_clear.is_pending_at(1_000 + QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS));
        assert!(!context_clear.is_pending_at(1_000 + QUEUE_CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS + 1));
        assert!(
            context_clear.is_manual_operator_clear_cooldown(),
            "manual cooldown remains source-identifiable after freshness TTL for source-aware helpers"
        );

        ledger.append(state_event(
            "context-clear-manual-cooldown-settled",
            StateFact::QueueContextClearSettled {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                target: "%1".into(),
                harness: "codex".into(),
                command: "/clear".into(),
                source: Some(QUEUE_CONTEXT_CLEAR_SOURCE_OPERATOR_MANUAL_COOLDOWN.into()),
                clear_epoch: 1,
                marked_secs: 1_100,
            },
        ));
        let projection = ledger.project();
        let context_clear = &projection.document("doc-a").unwrap().queue.context_clear;
        assert_eq!(context_clear.phase, QueueContextClearPhase::Idle);
        assert!(!context_clear.is_manual_operator_clear_cooldown());
    }

    #[test]
    fn queue_drain_stall_projection_folds_recorded_cleared_and_rejects_stale_clear() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "drain-stall-recorded-1",
            StateFact::QueueDrainStallContinuationRecorded {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                cycle_id: "cycle-1".into(),
                stall_epoch: 1,
                recorded_secs: 1_000,
            },
        ));

        let projection = ledger.project();
        let drain_stall = &projection.document("doc-a").unwrap().queue.drain_stall;
        assert_eq!(drain_stall.phase, QueueDrainStallPhase::Pending);
        assert!(drain_stall.is_pending());
        assert_eq!(drain_stall.cycle_id, "cycle-1");

        ledger.append(state_event(
            "drain-stall-recorded-2",
            StateFact::QueueDrainStallContinuationRecorded {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                cycle_id: "cycle-2".into(),
                stall_epoch: 2,
                recorded_secs: 2_000,
            },
        ));
        ledger.append(state_event(
            "drain-stall-stale-clear",
            StateFact::QueueDrainStallContinuationCleared {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                stall_epoch: 1,
                cleared_secs: 2_001,
                reason: Some("stale".into()),
            },
        ));
        let projection = ledger.project();
        let drain_stall = &projection.document("doc-a").unwrap().queue.drain_stall;
        assert_eq!(drain_stall.phase, QueueDrainStallPhase::Pending);
        assert_eq!(drain_stall.stall_epoch, 2);
        assert_eq!(drain_stall.cycle_id, "cycle-2");

        ledger.append(state_event(
            "drain-stall-clear",
            StateFact::QueueDrainStallContinuationCleared {
                document_hash: "doc-a".into(),
                file: "/tmp/plan.md".into(),
                stall_epoch: 2,
                cleared_secs: 2_002,
                reason: Some("preflight_reconciled".into()),
            },
        ));
        let projection = ledger.project();
        let drain_stall = &projection.document("doc-a").unwrap().queue.drain_stall;
        assert_eq!(drain_stall.phase, QueueDrainStallPhase::Idle);
        assert!(!drain_stall.is_pending());
        assert_eq!(
            drain_stall.clear_reason.as_deref(),
            Some("preflight_reconciled")
        );
    }

    #[test]
    fn transport_patch_terminals_are_lazily_receipts() {
        let mut transport = TransportProjection {
            editor_generation: Some(7),
            ..TransportProjection::default()
        };

        assert!(
            !transport.apply_patch_applied_receipt("patch-1", 6),
            "stale receipt generations must not mutate transport state"
        );
        assert!(!transport.patches.contains_key("patch-1"));
        assert!(transport.apply_patch_applied_receipt("patch-1", 7));
        assert_eq!(
            transport.patch_terminal_receipt_outcome("patch-1"),
            Some(ReceiptOutcome::Applied)
        );
        assert!(
            !transport.apply_patch_rejected_receipt("patch-1", 7, "late reject"),
            "a conflicting terminal receipt must fail closed"
        );
        assert_eq!(
            transport.patches["patch-1"].phase,
            TransportPatchPhase::Applied
        );

        let serialized = serde_json::to_string(&transport).unwrap();
        let restored: TransportProjection = serde_json::from_str(&serialized).unwrap();
        assert_eq!(transport, restored);
        assert_eq!(
            restored.patch_terminal_receipt_outcome("patch-1"),
            Some(ReceiptOutcome::Applied),
            "terminal receipt outcome is reconstructable from persisted phase"
        );
    }

    #[test]
    fn local_state_machines_guard_closed_domains() {
        assert_eq!(
            QueueHeadMachine::transition(QueueHeadPhase::Completed, QueueHeadEvent::Selected),
            None
        );
        let mut queue = QueueProjection::default();
        queue.apply_completed("node-1", None);
        queue.apply_selected("node-1", None, None, true);
        assert_eq!(queue.active_head, None);
        assert_eq!(queue.heads["node-1"].phase, QueueHeadPhase::Completed);
        let mut deferred_queue = QueueProjection::default();
        deferred_queue.apply_selected("node-2", Some("alpha"), Some("do [#alpha]"), true);
        deferred_queue.apply_deferred("node-2", "stop_fence");
        assert_eq!(deferred_queue.active_head, None);
        assert_eq!(
            deferred_queue.heads["node-2"].phase,
            QueueHeadPhase::Deferred
        );
        assert!(!deferred_queue.heads["node-2"].drainable);
        assert_eq!(
            deferred_queue.heads["node-2"].defer_reason.as_deref(),
            Some("stop_fence")
        );
        deferred_queue.apply_selected("node-2", None, None, true);
        assert_eq!(deferred_queue.active_head.as_deref(), Some("node-2"));
        assert_eq!(
            deferred_queue.heads["node-2"].phase,
            QueueHeadPhase::Selected
        );
        assert_eq!(deferred_queue.heads["node-2"].defer_reason, None);
        assert!(deferred_queue.heads["node-2"].drainable);
        deferred_queue.apply_worklist(
            "queue-hash-a",
            &[QueueWorklistEntry {
                kind: QueueWorklistEntryKind::Prompt,
                text: "do [#alpha]".into(),
                node_key: Some("node-2".into()),
                backlog_id: Some("alpha".into()),
                drainable: true,
            }],
            true,
        );
        assert!(deferred_queue.worklist_active);
        assert_eq!(
            deferred_queue.worklist_queue_hash.as_deref(),
            Some("queue-hash-a")
        );
        assert_eq!(deferred_queue.worklist[0].text, "do [#alpha]");
        deferred_queue.apply_worklist("queue-hash-b", &[], false);
        assert!(!deferred_queue.worklist_active);
        assert_eq!(
            deferred_queue.worklist_queue_hash.as_deref(),
            Some("queue-hash-b")
        );
        assert!(deferred_queue.worklist.is_empty());
        assert_eq!(
            TransportPatchMachine::transition(
                TransportPatchPhase::InsufficientProof,
                TransportPatchEvent::RetryRequested,
            ),
            Some(TransportPatchPhase::Retrying)
        );
        assert_eq!(
            TransportPatchMachine::transition(
                TransportPatchPhase::Applied,
                TransportPatchEvent::ForceDiskFallback,
            ),
            None
        );
        assert_eq!(
            ActorLifecycleMachine::transition(
                ActorLifecyclePhase::Ready,
                ActorLifecycleEvent::BusyObserved,
            ),
            Some(ActorLifecyclePhase::Busy)
        );
        assert_eq!(
            ActorLifecycleMachine::transition(
                ActorLifecyclePhase::Closed,
                ActorLifecycleEvent::ReadyObserved,
            ),
            None
        );
        assert_eq!(
            RouteReadinessMachine::transition(
                RouteReadinessPhase::DispatchAccepted,
                RouteReadinessEvent::DispatchProven,
            ),
            Some(RouteReadinessPhase::DispatchProven)
        );
        assert_eq!(
            RouteReadinessMachine::transition(
                RouteReadinessPhase::Unknown,
                RouteReadinessEvent::DispatchAccepted,
            ),
            None
        );
        assert_eq!(
            ProofGateMachine::transition(ProofGatePhase::Observed, ProofGateEvent::MarkerDisproved),
            None
        );
    }
}
