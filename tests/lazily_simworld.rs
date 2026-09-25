use agent_doc_state_backbone::write_pipeline::{
    DocumentWriteEvent, DocumentWritePhase, DocumentWritePipeline,
};
use lazily_sim::{
    CodecVersion, DurableCommit, DurableCommitOutcome, DurableContractError, DurableOwnerCore,
    DurableOwnerId, DurableOwnerMode, DurablePosition, DurableStateMutation, FenceToken,
    InboxIdentity, ReplayEvent, ReplayGraph, ReplayHarness, ReplayLog, ReplayObservation,
    ReplayProofError, ReplayValue, SchemaVersion, VersionedBytes,
};

const LAZILY_SIM_COMMIT: &str = "92656d6840e9171b3e55a9e6b11205f820ec984c";

#[derive(Debug)]
struct WriteReplayGraph {
    phase: DocumentWritePhase,
    rejected: u64,
}

impl Default for WriteReplayGraph {
    fn default() -> Self {
        Self {
            phase: DocumentWritePhase::IntentCaptured,
            rejected: 0,
        }
    }
}

impl ReplayGraph for WriteReplayGraph {
    fn apply(&mut self, event: &ReplayEvent) {
        let event = write_event(&event.name);
        match DocumentWritePipeline::transition(self.phase, event) {
            Some(next) => self.phase = next,
            None => self.rejected += 1,
        }
    }

    fn observe(&self) -> ReplayObservation {
        ReplayObservation::new()
            .with("lazily_commit", LAZILY_SIM_COMMIT)
            .with("phase", phase_name(self.phase))
            .with("rank", i64::from(self.phase.rank()))
            .with("terminal", self.phase.is_terminal())
            .with("rejected", self.rejected as i64)
    }
}

fn write_event(name: &str) -> DocumentWriteEvent {
    match name {
        "intent_captured" => DocumentWriteEvent::IntentCaptured,
        "canonical_applied" => DocumentWriteEvent::CanonicalApplied,
        "replica_accepted" => DocumentWriteEvent::ReplicaAccepted,
        "replica_visible" => DocumentWriteEvent::ReplicaVisible,
        "disk_projected" => DocumentWriteEvent::DiskProjected,
        "committed" => DocumentWriteEvent::Committed,
        "retry_requested" => DocumentWriteEvent::RetryRequested,
        "endpoint_lost" => DocumentWriteEvent::EndpointLost,
        "endpoint_restored" => DocumentWriteEvent::EndpointRestored,
        other => panic!("unknown simulated write event {other:?}"),
    }
}

fn phase_name(phase: DocumentWritePhase) -> &'static str {
    match phase {
        DocumentWritePhase::IntentCaptured => "intent_captured",
        DocumentWritePhase::CanonicalApplied => "canonical_applied",
        DocumentWritePhase::ReplicaAccepted => "replica_accepted",
        DocumentWritePhase::ReplicaVisible => "replica_visible",
        DocumentWritePhase::DiskProjected => "disk_projected",
        DocumentWritePhase::Committed => "committed",
    }
}

fn write_log(records: &[(&str, i64)]) -> ReplayLog {
    ReplayLog::from_records(records.iter().copied()).expect("valid materialized schedule")
}

fn versioned(value: &str) -> VersionedBytes {
    VersionedBytes::new(
        SchemaVersion::new(1).expect("schema version"),
        CodecVersion::new(1).expect("codec version"),
        value,
    )
}

fn durable_commit(
    owner: &DurableOwnerId,
    inbox: &str,
    fingerprint: &str,
    expected_position: u64,
    fence: u64,
    phase: DocumentWritePhase,
) -> DurableCommit {
    DurableCommit {
        owner_id: owner.clone(),
        expected_position: DurablePosition::new(expected_position),
        fence: FenceToken::new(fence),
        inbox_identity: InboxIdentity::new(inbox).expect("stable inbox identity"),
        ingress_fingerprint: fingerprint.as_bytes().to_vec(),
        state: DurableStateMutation::AppendEvents(vec![versioned(phase_name(phase))]),
        effects: Vec::new(),
        receipts: Vec::new(),
    }
}

#[test]
fn lazily_replay_fingerprint_proves_production_write_transition_schedule() {
    let schedule = write_log(&[
        ("canonical_applied", 1),
        ("endpoint_lost", 1),
        ("retry_requested", 1),
        ("committed", 1),
        ("endpoint_restored", 1),
        ("replica_accepted", 1),
        ("replica_accepted", 1),
        ("replica_visible", 1),
        ("disk_projected", 1),
        ("committed", 1),
    ]);
    let harness = ReplayHarness::new(WriteReplayGraph::default);

    let fingerprint = harness
        .prove(&schedule, 3)
        .expect("same materialized schedule must replay byte-identically");
    assert_eq!(fingerprint.checkpoints().len(), schedule.len() + 1);

    let reordered = write_log(&[
        ("canonical_applied", 1),
        ("endpoint_lost", 1),
        ("retry_requested", 1),
        ("replica_accepted", 1),
        ("endpoint_restored", 1),
        ("committed", 1),
        ("replica_accepted", 1),
        ("replica_visible", 1),
        ("disk_projected", 1),
        ("committed", 1),
    ]);
    assert!(matches!(
        harness.verify(&reordered, &fingerprint),
        Err(ReplayProofError::LogMismatch { .. })
    ));
}

#[test]
fn lazily_durable_owner_recovers_exactly_once_closeout_progress() {
    let owner = DurableOwnerId::new("agent-doc.closeout.write-1").expect("owner identity");
    let mut durable = DurableOwnerCore::new(
        owner.clone(),
        DurableOwnerMode::EventHistory,
        FenceToken::new(1),
    );
    let mut phase = DocumentWritePhase::IntentCaptured;

    for (index, event) in [
        DocumentWriteEvent::CanonicalApplied,
        DocumentWriteEvent::ReplicaAccepted,
        DocumentWriteEvent::ReplicaVisible,
    ]
    .into_iter()
    .enumerate()
    {
        phase = DocumentWritePipeline::transition(phase, event).expect("ordered write proof");
        let position = index as u64;
        let commit = durable_commit(
            &owner,
            &format!("delivery-{index}"),
            phase_name(phase),
            position,
            1,
            phase,
        );
        assert_eq!(
            durable.commit(commit.clone()),
            Ok(DurableCommitOutcome::Committed {
                through: DurablePosition::new(position + 1)
            })
        );
        assert_eq!(
            durable.commit(commit),
            Ok(DurableCommitOutcome::Duplicate {
                through: DurablePosition::new(position + 1)
            }),
            "transport replay must not append a second durable event"
        );
        durable = DurableOwnerCore::recover(durable.into_image())
            .expect("a crash may rebuild the exact durable prefix");
    }

    durable
        .advance_fence(FenceToken::new(2))
        .expect("replacement owner advances the fence");
    let before_stale_attempt = durable.image().clone();
    let stale = durable_commit(&owner, "delivery-3", "disk_projected", 3, 1, phase);
    assert!(matches!(
        durable.commit(stale),
        Err(DurableContractError::StaleFence { .. })
    ));
    assert_eq!(durable.image(), &before_stale_attempt);

    for (offset, event) in [
        DocumentWriteEvent::DiskProjected,
        DocumentWriteEvent::Committed,
    ]
    .into_iter()
    .enumerate()
    {
        phase = DocumentWritePipeline::transition(phase, event).expect("ordered write proof");
        let position = 3 + offset as u64;
        durable
            .commit(durable_commit(
                &owner,
                &format!("delivery-{}", position),
                phase_name(phase),
                position,
                2,
                phase,
            ))
            .expect("replacement owner continues from the durable prefix");
    }

    assert!(phase.is_terminal());
    assert_eq!(durable.image().position, DurablePosition::new(5));
    assert_eq!(durable.image().history.len(), 5);
    let proof = durable
        .complete_history_fingerprint(
            SchemaVersion::new(1).expect("schema version"),
            CodecVersion::new(1).expect("codec version"),
            ReplayValue::Str(phase_name(phase).to_string()),
        )
        .expect("complete closeout history is fingerprintable");
    assert_eq!(proof.through, DurablePosition::new(5));
}
