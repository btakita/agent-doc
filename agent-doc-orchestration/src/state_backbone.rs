//! Typed state backbone for durable agent-doc projections.
//!
//! The cycle FSM remains the closeout authority for one response turn. This
//! module gives the surrounding state domains a shared event/reducer shape:
//! append-only facts, deterministic projections, generation-aware owner checks,
//! and small local state machines for closed subdomains.

use std::collections::{BTreeMap, BTreeSet};

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

use crate::cycle_state::CyclePhase;
use crate::cycle_state_machine::{CycleEvent, CyclePhaseMachine};

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
    QueueHeadSelected {
        document_hash: String,
        node_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backlog_id: Option<String>,
        drainable: bool,
    },
    QueueHeadDeferred {
        document_hash: String,
        node_key: String,
        reason: String,
    },
    QueueHeadCompleted {
        document_hash: String,
        node_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backlog_id: Option<String>,
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
            | Self::QueueHeadSelected { document_hash, .. }
            | Self::QueueHeadDeferred { document_hash, .. }
            | Self::QueueHeadCompleted { document_hash, .. }
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
            | Self::ProofMarkerDisproved { document_hash, .. } => document_hash,
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
            Self::BaselineSaved { .. } => StateDomain::Document,
            Self::QueueHeadSelected { .. }
            | Self::QueueHeadDeferred { .. }
            | Self::QueueHeadCompleted { .. } => StateDomain::Queue,
            Self::EditorPatchQueued { .. }
            | Self::EditorAckObserved { .. }
            | Self::IpcProofInsufficient { .. }
            | Self::EditorPatchRetryRequested { .. }
            | Self::ForceDiskFallbackRecorded { .. } => StateDomain::Transport,
            Self::OwnerGenerationChanged { .. }
            | Self::ActorLifecycleObserved { .. }
            | Self::AgentRestartPerformed { .. }
            | Self::CapabilityProofObserved { .. } => StateDomain::Supervisor,
            Self::RoutePaneObserved { .. }
            | Self::RouteReadinessObserved { .. }
            | Self::DispatchProofObserved { .. } => StateDomain::Route,
            Self::ProofMarkerObserved { .. } | Self::ProofMarkerDisproved { .. } => {
                StateDomain::Proof
            }
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_stale_events: Vec<RejectedStaleEvent>,
}

impl DocumentStateProjection {
    fn new(document_hash: &str) -> Self {
        Self {
            document_hash: document_hash.to_string(),
            document: DocumentProjection::default(),
            queue: QueueProjection::default(),
            closeout: CloseoutProjection::default(),
            transport: TransportProjection::default(),
            supervisor: SupervisorProjection::default(),
            route: RouteProjection::default(),
            proof: ProofProjection::default(),
            rejected_stale_events: Vec::new(),
        }
    }

    pub fn projection_summary(&self) -> ProjectionSummary {
        ProjectionSummary::from_document(self)
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
            StateFact::QueueHeadSelected {
                node_key,
                backlog_id,
                drainable,
                ..
            } => {
                self.queue
                    .apply_selected(node_key, backlog_id.as_deref(), *drainable);
            }
            StateFact::QueueHeadDeferred {
                node_key, reason, ..
            } => {
                self.queue.apply_deferred(node_key, reason);
            }
            StateFact::QueueHeadCompleted {
                node_key,
                backlog_id,
                ..
            } => {
                self.queue.apply_completed(node_key, backlog_id.as_deref());
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineProjection {
    pub cycle_id: String,
    pub baseline_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QueueProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_head: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub heads: BTreeMap<String, QueueHeadProjection>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub completed_heads: BTreeSet<String>,
}

impl QueueProjection {
    fn apply_selected(&mut self, node_key: &str, backlog_id: Option<&str>, drainable: bool) {
        let head = self
            .heads
            .entry(node_key.to_string())
            .or_insert_with(|| QueueHeadProjection::new(backlog_id));
        head.backlog_id = backlog_id.map(str::to_string).or(head.backlog_id.clone());
        head.drainable = drainable;
        if head.transition(QueueHeadEvent::Selected) {
            self.active_head = Some(node_key.to_string());
        }
    }

    fn apply_deferred(&mut self, node_key: &str, reason: &str) {
        let head = self
            .heads
            .entry(node_key.to_string())
            .or_insert_with(|| QueueHeadProjection::new(None));
        head.defer_reason = Some(reason.to_string());
        head.transition(QueueHeadEvent::Deferred);
        if self.active_head.as_deref() == Some(node_key) {
            self.active_head = None;
        }
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
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueHeadProjection {
    pub phase: QueueHeadPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog_id: Option<String>,
    pub drainable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defer_reason: Option<String>,
}

impl QueueHeadProjection {
    fn new(backlog_id: Option<&str>) -> Self {
        Self {
            phase: QueueHeadPhase::Pending,
            backlog_id: backlog_id.map(str::to_string),
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
                drainable: true,
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
    fn local_state_machines_guard_closed_domains() {
        assert_eq!(
            QueueHeadMachine::transition(QueueHeadPhase::Completed, QueueHeadEvent::Selected),
            None
        );
        let mut queue = QueueProjection::default();
        queue.apply_completed("node-1", None);
        queue.apply_selected("node-1", None, true);
        assert_eq!(queue.active_head, None);
        assert_eq!(queue.heads["node-1"].phase, QueueHeadPhase::Completed);
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
