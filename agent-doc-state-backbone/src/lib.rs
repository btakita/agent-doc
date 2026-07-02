//! Typed state backbone for durable agent-doc projections.
//!
//! The cycle FSM remains the closeout authority for one response turn. This
//! module gives the surrounding state domains a shared event/reducer shape:
//! append-only facts, deterministic projections, generation-aware owner checks,
//! and small local state machines for closed subdomains.

use std::collections::{BTreeMap, BTreeSet};

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

use agent_doc_turn::CyclePhase;
use agent_doc_turn::{CycleEvent, CyclePhaseMachine};

/// Phase E (`#adstatechart`) local-process Harel state chart consolidation.
pub mod adstatechart;

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
    },
    BaselineSaved {
        document_hash: String,
        cycle_id: String,
        baseline_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        baseline_path: Option<String>,
    },
    FileWatchChangeObserved {
        document_hash: String,
        path: String,
        watch_generation: u64,
        content_hash: String,
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
    ResponseCaptured {
        document_hash: String,
        cycle_id: String,
        capture_id: String,
        response_sha256: String,
    },
    WriteApplied {
        document_hash: String,
        cycle_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patch_id: Option<String>,
    },
    CommitObserved {
        document_hash: String,
        cycle_id: String,
        commit: String,
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
    EditorAckObserved {
        document_hash: String,
        patch_id: String,
        actor_generation: u64,
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
            | Self::BaselineSaved { document_hash, .. }
            | Self::FileWatchChangeObserved { document_hash, .. }
            | Self::QueueHeadSelected { document_hash, .. }
            | Self::QueueHeadDeferred { document_hash, .. }
            | Self::QueueHeadCompleted { document_hash, .. }
            | Self::QueueWorklistProjected { document_hash, .. }
            | Self::ResponseCaptured { document_hash, .. }
            | Self::WriteApplied { document_hash, .. }
            | Self::CommitObserved { document_hash, .. }
            | Self::SessionCheckPassed { document_hash, .. }
            | Self::CycleAbandoned { document_hash, .. }
            | Self::OwnerGenerationChanged { document_hash, .. }
            | Self::EditorPatchQueued { document_hash, .. }
            | Self::EditorAckObserved { document_hash, .. }
            | Self::IpcProofInsufficient { document_hash, .. }
            | Self::EditorPatchRetryRequested { document_hash, .. }
            | Self::ForceDiskFallbackRecorded { document_hash, .. }
            | Self::ActorLifecycleObserved { document_hash, .. }
            | Self::AgentRestartPerformed { document_hash, .. }
            | Self::CapabilityProofObserved { document_hash, .. }
            | Self::RoutePaneObserved { document_hash, .. }
            | Self::RouteReadinessObserved { document_hash, .. }
            | Self::DispatchProofObserved { document_hash, .. }
            | Self::ProofMarkerObserved { document_hash, .. }
            | Self::ProofMarkerDisproved { document_hash, .. }
            | Self::SupervisorHosting { document_hash, .. } => document_hash,
        }
    }

    pub fn domain(&self) -> StateDomain {
        match self {
            Self::PreflightStarted { .. }
            | Self::ResponseCaptured { .. }
            | Self::WriteApplied { .. }
            | Self::CommitObserved { .. }
            | Self::SessionCheckPassed { .. }
            | Self::CycleAbandoned { .. } => StateDomain::Closeout,
            Self::BaselineSaved { .. } | Self::FileWatchChangeObserved { .. } => {
                StateDomain::Document
            }
            Self::QueueHeadSelected { .. }
            | Self::QueueHeadDeferred { .. }
            | Self::QueueHeadCompleted { .. }
            | Self::QueueWorklistProjected { .. } => StateDomain::Queue,
            Self::EditorPatchQueued { .. }
            | Self::EditorAckObserved { .. }
            | Self::IpcProofInsufficient { .. }
            | Self::EditorPatchRetryRequested { .. }
            | Self::ForceDiskFallbackRecorded { .. } => StateDomain::Transport,
            Self::OwnerGenerationChanged { .. }
            | Self::ActorLifecycleObserved { .. }
            | Self::AgentRestartPerformed { .. }
            | Self::CapabilityProofObserved { .. }
            | Self::SupervisorHosting { .. } => StateDomain::Supervisor,
            Self::RoutePaneObserved { .. }
            | Self::RouteReadinessObserved { .. }
            | Self::DispatchProofObserved { .. } => StateDomain::Route,
            Self::ProofMarkerObserved { .. } | Self::ProofMarkerDisproved { .. } => {
                StateDomain::Proof
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::PreflightStarted { .. } => "preflight_started",
            Self::BaselineSaved { .. } => "baseline_saved",
            Self::FileWatchChangeObserved { .. } => "file_watch_change_observed",
            Self::QueueHeadSelected { .. } => "queue_head_selected",
            Self::QueueHeadDeferred { .. } => "queue_head_deferred",
            Self::QueueHeadCompleted { .. } => "queue_head_completed",
            Self::QueueWorklistProjected { .. } => "queue_worklist_projected",
            Self::SupervisorHosting { .. } => "supervisor_hosting",
            Self::ResponseCaptured { .. } => "response_captured",
            Self::WriteApplied { .. } => "write_applied",
            Self::CommitObserved { .. } => "commit_observed",
            Self::SessionCheckPassed { .. } => "session_check_passed",
            Self::CycleAbandoned { .. } => "cycle_abandoned",
            Self::OwnerGenerationChanged { .. } => "owner_generation_changed",
            Self::EditorPatchQueued { .. } => "editor_patch_queued",
            Self::EditorAckObserved { .. } => "editor_ack_observed",
            Self::IpcProofInsufficient { .. } => "ipc_proof_insufficient",
            Self::EditorPatchRetryRequested { .. } => "editor_patch_retry_requested",
            Self::ForceDiskFallbackRecorded { .. } => "force_disk_fallback_recorded",
            Self::ActorLifecycleObserved { .. } => "actor_lifecycle_observed",
            Self::AgentRestartPerformed { .. } => "agent_restart_performed",
            Self::CapabilityProofObserved { .. } => "capability_proof_observed",
            Self::RoutePaneObserved { .. } => "route_pane_observed",
            Self::RouteReadinessObserved { .. } => "route_readiness_observed",
            Self::DispatchProofObserved { .. } => "dispatch_proof_observed",
            Self::ProofMarkerObserved { .. } => "proof_marker_observed",
            Self::ProofMarkerDisproved { .. } => "proof_marker_disproved",
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

    /// Apply a single state fact to this document projection. Public so the
    /// wire/delta derivation (`state_wire`) can replay a bounded slice of the
    /// accepted event stream into a fresh projection without going through the
    /// backbone's global dedup map (`#lazilystatesync2`).
    pub fn apply_fact(&mut self, fact: &StateFact) {
        self.apply(fact);
    }

    fn apply(&mut self, fact: &StateFact) {
        match fact {
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
            StateFact::PreflightStarted {
                cycle_id,
                session_id,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::StartPreflight);
                self.closeout.session_id = session_id.clone();
            }
            StateFact::ResponseCaptured {
                cycle_id,
                capture_id,
                response_sha256,
                ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::ResponseCaptured);
                self.closeout.capture_id = Some(capture_id.clone());
                self.closeout.response_sha256 = Some(response_sha256.clone());
            }
            StateFact::WriteApplied {
                cycle_id, patch_id, ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::WriteApplied);
                self.closeout.patch_id = patch_id.clone();
            }
            StateFact::CommitObserved {
                cycle_id, commit, ..
            } => {
                self.closeout
                    .apply_cycle_event(cycle_id, CycleEvent::Committed);
                self.closeout.commit = Some(commit.clone());
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
            StateFact::EditorAckObserved {
                patch_id,
                actor_generation,
                ..
            } => {
                if self.current_generation_matches(StateOwner::EditorIpcBridge, *actor_generation) {
                    self.transport.apply_patch_event(
                        patch_id,
                        TransportPatchEvent::AckObserved,
                        *actor_generation,
                    );
                } else {
                    self.reject_stale(StateDomain::Transport, StateOwner::EditorIpcBridge);
                }
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
            StateFact::ProofMarkerObserved { marker, source, .. } => {
                self.proof
                    .apply(marker, source, ProofGateEvent::MarkerObserved);
            }
            StateFact::ProofMarkerDisproved { marker, source, .. } => {
                self.proof
                    .apply(marker, source, ProofGateEvent::MarkerDisproved);
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
    pub latest_file_watch_change: Option<FileWatchChangeProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineProjection {
    pub cycle_id: String,
    pub baseline_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWatchChangeProjection {
    pub path: String,
    pub watch_generation: u64,
    pub content_hash: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CloseoutProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<CyclePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub session_check_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandoned_reason: Option<String>,
}

impl CloseoutProjection {
    fn apply_cycle_event(&mut self, cycle_id: &str, event: CycleEvent) {
        if self.cycle_id.as_deref() != Some(cycle_id) || matches!(event, CycleEvent::StartPreflight)
        {
            self.cycle_id = Some(cycle_id.to_string());
            self.phase = Some(CyclePhase::PreflightStarted);
            self.session_check_passed = false;
        }
        let current = self.phase.unwrap_or(CyclePhase::PreflightStarted);
        if let Some(next) = CyclePhaseMachine::transition(current, event) {
            self.phase = Some(next);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TransportProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub patches: BTreeMap<String, TransportPatchProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_unproven_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_retry_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_disk_reason: Option<String>,
}

impl TransportProjection {
    fn apply_patch_event(
        &mut self,
        patch_id: &str,
        event: TransportPatchEvent,
        actor_generation: u64,
    ) {
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
        }
    }
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
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub dispatch_proofs: BTreeSet<String>,
}

impl RouteProjection {
    fn apply_readiness_event(&mut self, event: RouteReadinessEvent) {
        if let Some(next) = RouteReadinessMachine::transition(self.readiness, event) {
            self.readiness = next;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProofProjection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub markers: BTreeMap<String, ProofMarkerProjection>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofMarkerProjection {
    pub phase: ProofGatePhase,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub sources: BTreeSet<String>,
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
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_head);
        Self { ctx, machine }
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
    Acked,
    InsufficientProof,
    Retrying,
    ForceDiskFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportPatchEvent {
    PatchQueued,
    AckObserved,
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
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_transport_patch);
        Self { ctx, machine }
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
            TransportPatchPhase::Acked | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::AckObserved => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying
            | TransportPatchPhase::Acked => Some(TransportPatchPhase::Acked),
            TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::ProofInsufficient => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying => Some(TransportPatchPhase::InsufficientProof),
            TransportPatchPhase::Acked | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::RetryRequested => match current {
            TransportPatchPhase::InsufficientProof | TransportPatchPhase::Retrying => {
                Some(TransportPatchPhase::Retrying)
            }
            TransportPatchPhase::Queued
            | TransportPatchPhase::Acked
            | TransportPatchPhase::ForceDiskFallback => None,
        },
        TransportPatchEvent::ForceDiskFallback => match current {
            TransportPatchPhase::Queued
            | TransportPatchPhase::InsufficientProof
            | TransportPatchPhase::Retrying
            | TransportPatchPhase::ForceDiskFallback => {
                Some(TransportPatchPhase::ForceDiskFallback)
            }
            TransportPatchPhase::Acked => None,
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
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_actor_lifecycle);
        Self { ctx, machine }
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
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_route_readiness);
        Self { ctx, machine }
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
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_proof_gate);
        Self { ctx, machine }
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

    fn state_event(event_id: impl Into<String>, fact: StateFact) -> StateEvent {
        StateEvent::new(event_id, fact)
    }

    fn socket_ack_events(
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
                format!("{prefix}:patch-acked"),
                StateFact::EditorAckObserved {
                    document_hash: document_hash.into(),
                    patch_id: patch_id.into(),
                    actor_generation: generation,
                },
            ),
        ]
    }

    fn file_retry_then_ack_events(
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
                format!("{prefix}:patch-acked"),
                StateFact::EditorAckObserved {
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
    fn replay_projection_reduces_events_idempotently() {
        let mut ledger = EventLedger::new();
        ledger.append(state_event(
            "e1",
            StateFact::PreflightStarted {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                session_id: Some("session-1".into()),
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
            StateFact::EditorAckObserved {
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
            },
        ));
        ledger.append(state_event(
            "e11",
            StateFact::CommitObserved {
                document_hash: "doc-a".into(),
                cycle_id: "cycle-1".into(),
                commit: "abc123".into(),
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
            TransportPatchPhase::Acked
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
    fn cross_editor_live_ipc_projection_summaries_match_bridge_contract() {
        let mut events = Vec::new();
        events.extend(socket_ack_events(
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

        events.extend(file_retry_then_ack_events(
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

        events.extend(file_retry_then_ack_events(
            "vscode-file",
            "doc-vscode-file",
            "vscode-file-patch",
            41,
            "file_ipc_timeout",
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
            TransportPatchPhase::Acked
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
                "latestTransportPhase": "acked",
                "proofMarkers": 1,
            })
        );
        assert_eq!(
            jb_socket.projection_summary().compact(),
            "route=dispatch_proven pane=%11 transport=jb-socket-patch:acked proof_markers=1"
        );

        let jb_file = projection.document("doc-jb-file").unwrap();
        assert_eq!(
            jb_file.transport.patches["jb-file-patch"].phase,
            TransportPatchPhase::Acked
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
            "route=blocked pane=%12 transport=jb-file-patch:acked proof_markers=1"
        );

        let vscode_file = projection.document("doc-vscode-file").unwrap();
        assert_eq!(
            vscode_file.transport.patches["vscode-file-patch"].phase,
            TransportPatchPhase::Acked
        );
        assert_eq!(
            vscode_file.transport.last_unproven_reason.as_deref(),
            Some("file_ipc_timeout")
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
            "route=dispatch_proven pane=%13 transport=vscode-file-patch:acked proof_markers=1"
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
                TransportPatchPhase::Acked,
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
