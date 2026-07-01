//! Pure CRDT-authority policy (`#crdtauth1`).
//!
//! This is the additive authority layer over the merge-ownership state machine
//! ([`agent_doc_merge::ownership`], `#mergestatemachine`). It reframes that SM
//! from "who may write the buffer now" to "who is the CRDT authority" — the
//! step-1 rung of the CRDT authority model.
//!
//! ## The two authority states
//!
//! - [`CrdtAuthority::GitAuthoritative`] — **Detached**: no live editor (pure CLI /
//!   Codex / headless). Git is the source of truth and the CRDT is **ephemeral**:
//!   rebuilt from the committed document + that turn's ops, with no durable `.yrs`
//!   treated as authoritative. This is most headless dogfooding traffic, and it
//!   carries zero stale-`.yrs` class by construction.
//! - [`CrdtAuthority::MultiReplica`] — **EditorAttached**: a live editor plugin is
//!   attached. The supervisor hosts the canonical replica; each editor hosts its
//!   own. Git/disk are durable **projections checkpointed at boundaries**, not the
//!   live coordination medium.
//!
//! The `attach` / `detach` event is the transition. Callers that own IO or
//! per-document ledgers should observe their facts, then pass the pure liveness
//! or ownership phase into this module.

use agent_doc_merge::ownership::{
    MergeOwnershipEvent, MergeOwnershipPhase, OwnershipLiveness, ownership_probe,
};
use agent_doc_state_backbone::{DocumentStateProjection, EventLedger, TransportPatchPhase};
use serde::{Deserialize, Serialize};

/// Which replica is the CRDT authority for a document, and the durability
/// semantics that follow from it.
///
/// This is the additive authority layer over [`MergeOwnershipPhase`]: it answers
/// "who is the CRDT authority" rather than "who may write the buffer now". The two
/// states are the Detached and EditorAttached states of the one authority model
/// (they supersede the design thread's "Option A vs Option B" framing — A is
/// `GitAuthoritative`, B is `MultiReplica`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrdtAuthority {
    /// **Detached** — no live editor. Git is authoritative; the CRDT is
    /// **ephemeral** (rebuilt from the committed document + that turn's ops, no
    /// durable `.yrs` treated as authoritative). Most headless traffic.
    GitAuthoritative,
    /// **EditorAttached** — a live editor plugin owns/coordinates a replica. The
    /// supervisor hosts the canonical replica; editors host their own. Git/disk
    /// are durable projections checkpointed at boundaries, not the live
    /// coordination medium.
    MultiReplica,
}

impl CrdtAuthority {
    /// Whether the CRDT is **ephemeral** under this authority — rebuilt from the
    /// committed document + that turn's ops, with no durable `.yrs` treated as
    /// authoritative. True only under [`CrdtAuthority::GitAuthoritative`].
    ///
    /// Later rungs (state-vector sync, disk demotion) consult this to decide
    /// whether a persisted replica is authoritative or merely a recovery
    /// projection.
    pub fn crdt_is_ephemeral(self) -> bool {
        matches!(self, CrdtAuthority::GitAuthoritative)
    }

    /// Whether disk (`.agent-doc/crdt/<hash>.yrs`) is a **durable projection
    /// checkpointed at boundaries** rather than the live coordination medium.
    /// True only under [`CrdtAuthority::MultiReplica`].
    ///
    /// This is the inverse of [`Self::crdt_is_ephemeral`]: a durable-projection
    /// authority keeps disk as a checkpoint; an ephemeral authority does not treat
    /// any persisted `.yrs` as a source of truth.
    pub fn disk_is_durable_projection(self) -> bool {
        matches!(self, CrdtAuthority::MultiReplica)
    }

    /// Whether a live editor is attached (multi-replica coordination is active).
    pub fn editor_attached(self) -> bool {
        matches!(self, CrdtAuthority::MultiReplica)
    }
}

/// Derive the CRDT authority from a **proven** merge-ownership phase.
///
/// This is the pure mapping from the existing SM's vocabulary to the authority
/// layer. A phase that proves a live editor coordinates the buffer
/// ([`Attached`](MergeOwnershipPhase::Attached),
/// [`EditorOwnsBuffer`](MergeOwnershipPhase::EditorOwnsBuffer),
/// [`BinaryWriteRequested`](MergeOwnershipPhase::BinaryWriteRequested),
/// [`IpcAckProven`](MergeOwnershipPhase::IpcAckProven)) is
/// [`MultiReplica`](CrdtAuthority::MultiReplica); the no-editor phases
/// ([`Detached`](MergeOwnershipPhase::Detached),
/// [`Committed`](MergeOwnershipPhase::Committed)) are
/// [`GitAuthoritative`](CrdtAuthority::GitAuthoritative).
///
/// NOTE: [`Attached`](MergeOwnershipPhase::Attached) is the ambiguous `#6b5h`
/// state (a connectable listener that may be a *stale* listener with no editor
/// behind it). This pure mapping treats the raw `Attached` phase as editor-present
/// — callers that hold liveness facts should resolve the ambiguity first via
/// [`authority_from_liveness`], which demotes a stale listener to
/// `GitAuthoritative`.
pub fn authority_for(phase: MergeOwnershipPhase) -> CrdtAuthority {
    match phase {
        MergeOwnershipPhase::Detached | MergeOwnershipPhase::Committed => {
            CrdtAuthority::GitAuthoritative
        }
        MergeOwnershipPhase::Attached
        | MergeOwnershipPhase::EditorOwnsBuffer
        | MergeOwnershipPhase::BinaryWriteRequested
        | MergeOwnershipPhase::IpcAckProven => CrdtAuthority::MultiReplica,
    }
}

/// Resolve the CRDT authority from editor-attachment **liveness facts**, demoting
/// a stale listener (`#6b5h`) to [`GitAuthoritative`](CrdtAuthority::GitAuthoritative).
///
/// Starts from the ambiguous [`Attached`](MergeOwnershipPhase::Attached) state and
/// lets [`ownership_probe`] resolve it from the lease:
/// - lease present **and** pid live → a real editor is behind the listener →
///   [`MultiReplica`](CrdtAuthority::MultiReplica) (keyed off pid liveness, not
///   heartbeat freshness, so an idle editor is still recognized).
/// - lease present but pid dead, **or** no lease at all (stale listener) → no live
///   editor → [`GitAuthoritative`](CrdtAuthority::GitAuthoritative).
pub fn authority_from_liveness(liveness: &OwnershipLiveness) -> CrdtAuthority {
    let resolved = ownership_probe(MergeOwnershipPhase::Attached, liveness);
    let phase = match resolved {
        Some(MergeOwnershipEvent::EditorBufferObserved) => MergeOwnershipPhase::EditorOwnsBuffer,
        // Dead pid / stale listener (no lease) both route to the no-editor path.
        _ => MergeOwnershipPhase::Detached,
    };
    authority_for(phase)
}

/// Derive the CRDT authority for a single document from a backbone projection,
/// riding the hosting-epoch substrate.
///
/// Authority follows the live editor. A document with a live editor transport
/// (the `EditorIpcBridge` actor has an active generation, and the latest editor
/// transport patch is not a terminal force-disk fallback) is
/// [`CrdtAuthority::MultiReplica`]; otherwise the document is headless and
/// [`CrdtAuthority::GitAuthoritative`].
///
/// This is per-document by construction: it consults only the passed document's
/// own projection. A stale-overlay replay for a different document
/// (rejected/dropped by the hosting-epoch reset) never reaches this projection,
/// so it cannot flip this document's authority.
pub fn authority_for_projection(projection: &DocumentStateProjection) -> CrdtAuthority {
    if document_has_live_editor_transport(projection) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

/// Derive the CRDT authority for `document_hash` from the event ledger. Returns
/// [`CrdtAuthority::GitAuthoritative`] for an unknown document (no projection
/// yet): a document the supervisor has never hosted with an editor is headless
/// until proven otherwise.
pub fn authority_for_document(ledger: &EventLedger, document_hash: &str) -> CrdtAuthority {
    match ledger.project_document(document_hash) {
        Some(projection) => authority_for_projection(&projection),
        None => CrdtAuthority::GitAuthoritative,
    }
}

/// Whether a document's projection proves a live editor transport replica.
fn document_has_live_editor_transport(projection: &DocumentStateProjection) -> bool {
    if projection.transport.editor_generation.is_none() {
        return false;
    }
    match projection.transport.patches.iter().next_back() {
        Some((_, patch)) => !matches!(patch.phase, TransportPatchPhase::ForceDiskFallback),
        None => true,
    }
}

/// Run one incremental state-vector sync round between two replicas ONLY under a
/// multi-replica authority (`#crdtauth1sv` seam).
///
/// This wires the state-vector sync primitive ([`agent_doc_merge::crdt_sync`]) to
/// the authority SM: the incremental protocol is engaged exactly when the
/// authority proves a live multi-replica session ([`CrdtAuthority::MultiReplica`]).
/// Under [`CrdtAuthority::GitAuthoritative`] the CRDT is ephemeral — git is the
/// source of truth, rebuilt from text each turn — so there is no second live
/// writer to converge with and the sync is skipped. Returns whether a sync round
/// actually ran.
pub fn sync_under_authority(
    authority: CrdtAuthority,
    a: &agent_doc_merge::crdt_sync::ReplicaState,
    b: &agent_doc_merge::crdt_sync::ReplicaState,
) -> anyhow::Result<bool> {
    if authority.editor_attached() {
        agent_doc_merge::crdt_sync::sync(a, b)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// The state-vector commit barrier gated by authority (`#crdtauth3`).
///
/// Under [`CrdtAuthority::MultiReplica`] it flushes every live editor's ops into
/// the canonical replica ("flush all live editors to SV=N") and reports whether a
/// snapshot of `canonical` is a consistent cut — the quiescence point `finalize`
/// should snapshot at instead of a patch-ack. Under
/// [`CrdtAuthority::GitAuthoritative`] there are no live editor replicas to flush
/// (git is the source of truth, rebuilt from text), so the barrier is trivially
/// satisfied and the canonical replica is left untouched. Returns whether a
/// snapshot is safe to commit.
pub fn commit_barrier_under_authority(
    authority: CrdtAuthority,
    canonical: &agent_doc_merge::crdt_sync::ReplicaState,
    editors: &[&agent_doc_merge::crdt_sync::ReplicaState],
) -> anyhow::Result<bool> {
    if authority.editor_attached() {
        agent_doc_merge::crdt_sync::flush_to_commit_barrier(canonical, editors)
    } else {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_merge::crdt_sync::ReplicaState;
    use agent_doc_merge::ownership::OwnershipLiveness;
    use agent_doc_state_backbone::{EventLedger, StateEvent, StateFact, StateOwner};

    const ALL_PHASES: [MergeOwnershipPhase; 6] = [
        MergeOwnershipPhase::Detached,
        MergeOwnershipPhase::Attached,
        MergeOwnershipPhase::EditorOwnsBuffer,
        MergeOwnershipPhase::BinaryWriteRequested,
        MergeOwnershipPhase::IpcAckProven,
        MergeOwnershipPhase::Committed,
    ];

    #[test]
    fn detached_and_committed_are_git_authoritative_ephemeral() {
        for phase in [
            MergeOwnershipPhase::Detached,
            MergeOwnershipPhase::Committed,
        ] {
            let authority = authority_for(phase);
            assert_eq!(authority, CrdtAuthority::GitAuthoritative, "{phase:?}");
            assert!(
                authority.crdt_is_ephemeral(),
                "{phase:?}: GitAuthoritative implies an ephemeral CRDT"
            );
            assert!(
                !authority.disk_is_durable_projection(),
                "{phase:?}: no durable-authority assumption under GitAuthoritative"
            );
            assert!(!authority.editor_attached());
        }
    }

    #[test]
    fn editor_phases_are_multi_replica_durable_projection() {
        for phase in [
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
            MergeOwnershipPhase::BinaryWriteRequested,
            MergeOwnershipPhase::IpcAckProven,
        ] {
            let authority = authority_for(phase);
            assert_eq!(authority, CrdtAuthority::MultiReplica, "{phase:?}");
            assert!(
                !authority.crdt_is_ephemeral(),
                "{phase:?}: MultiReplica uses durable-projection semantics, not ephemeral"
            );
            assert!(
                authority.disk_is_durable_projection(),
                "{phase:?}: disk is a durable boundary-checkpointed projection under MultiReplica"
            );
            assert!(authority.editor_attached());
        }
    }

    #[test]
    fn authority_for_is_total_over_all_phases() {
        for phase in ALL_PHASES {
            // Every phase resolves to exactly one authority; no panic.
            let _ = authority_for(phase);
        }
    }

    #[test]
    fn live_editor_liveness_resolves_multi_replica() {
        // lease present + pid live → real editor (idle or not) → MultiReplica.
        for heartbeat_fresh in [true, false] {
            let authority = authority_from_liveness(&OwnershipLiveness {
                lease_present: true,
                pid_live: true,
                heartbeat_fresh,
            });
            assert_eq!(
                authority,
                CrdtAuthority::MultiReplica,
                "a live editor (heartbeat_fresh={heartbeat_fresh}) is multi-replica"
            );
        }
    }

    #[test]
    fn dead_pid_editor_demotes_to_git_authoritative() {
        // lease present but pid dead → editor gone → git-authoritative.
        let authority = authority_from_liveness(&OwnershipLiveness {
            lease_present: true,
            pid_live: false,
            heartbeat_fresh: true,
        });
        assert_eq!(authority, CrdtAuthority::GitAuthoritative);
        assert!(authority.crdt_is_ephemeral());
    }

    #[test]
    fn stale_listener_no_lease_demotes_to_git_authoritative() {
        // #6b5h: connectable listener, no editor buffer behind it → git-authoritative.
        let authority = authority_from_liveness(&OwnershipLiveness {
            lease_present: false,
            pid_live: false,
            heartbeat_fresh: false,
        });
        assert_eq!(
            authority,
            CrdtAuthority::GitAuthoritative,
            "a stale listener must not claim multi-replica authority"
        );
        assert!(authority.crdt_is_ephemeral());
    }

    #[test]
    fn authority_agrees_with_disk_write_gate_on_the_headless_path() {
        // The authority layer must agree with the merge-ownership disk-write gate
        // on the no-editor (headless) path: the only *write-active* phase that
        // both permits a direct disk write AND is git-authoritative is Detached.
        //
        // - Detached: permits disk write AND GitAuthoritative (the headless path).
        // - IpcAckProven: permits a disk write too, but it is a *post-ack editor*
        //   phase (the disk sync lands behind the editor's apply), so it stays
        //   MultiReplica — disk-write-permitted is NOT a sufficient condition for
        //   GitAuthoritative.
        // - Committed: GitAuthoritative but terminal (no write), so it does not
        //   permit a write — GitAuthoritative is NOT a sufficient condition for
        //   disk-write-permitted either.
        use agent_doc_merge::ownership::disk_write_permitted;

        assert!(disk_write_permitted(MergeOwnershipPhase::Detached));
        assert_eq!(
            authority_for(MergeOwnershipPhase::Detached),
            CrdtAuthority::GitAuthoritative,
            "the headless direct-disk path is git-authoritative"
        );

        assert!(disk_write_permitted(MergeOwnershipPhase::IpcAckProven));
        assert_eq!(
            authority_for(MergeOwnershipPhase::IpcAckProven),
            CrdtAuthority::MultiReplica,
            "a post-ack editor write still rides the multi-replica authority"
        );

        assert_eq!(
            authority_for(MergeOwnershipPhase::Committed),
            CrdtAuthority::GitAuthoritative
        );
        assert!(
            !disk_write_permitted(MergeOwnershipPhase::Committed),
            "Committed is terminal — git-authoritative does not imply a live disk write"
        );
    }

    #[test]
    fn backbone_projection_authority_follows_live_editor_transport() {
        let doc = "doc-authority";
        let mut ledger = EventLedger::new();

        assert_eq!(
            authority_for_document(&ledger, doc),
            CrdtAuthority::GitAuthoritative,
            "unknown documents are headless until a live editor is proven"
        );

        ledger.append(StateEvent::new(
            "editor-generation",
            StateFact::OwnerGenerationChanged {
                document_hash: doc.to_string(),
                owner: StateOwner::EditorIpcBridge,
                generation: 7,
            },
        ));
        let projection = ledger.project_document(doc).expect("projection");
        assert_eq!(
            authority_for_projection(&projection),
            CrdtAuthority::MultiReplica,
            "an active editor generation proves a live replica"
        );

        ledger.append(StateEvent::new(
            "patch-queued",
            StateFact::EditorPatchQueued {
                document_hash: doc.to_string(),
                patch_id: "patch-1".to_string(),
                actor_generation: 7,
            },
        ));
        ledger.append(StateEvent::new(
            "force-disk-fallback",
            StateFact::ForceDiskFallbackRecorded {
                document_hash: doc.to_string(),
                patch_id: "patch-1".to_string(),
                actor_generation: 7,
                reason: "editor unreachable".to_string(),
            },
        ));
        assert_eq!(
            authority_for_document(&ledger, doc),
            CrdtAuthority::GitAuthoritative,
            "terminal force-disk fallback demotes the document to headless authority"
        );
    }

    #[test]
    fn state_vector_sync_runs_only_under_multi_replica_authority() {
        // MultiReplica → the incremental sync engages and the replicas converge.
        let a = ReplicaState::new(1);
        a.apply_local_edit(0, 0, "alpha");
        let b = ReplicaState::new(2);
        b.apply_local_edit(0, 0, "beta");
        let ran = sync_under_authority(CrdtAuthority::MultiReplica, &a, &b).unwrap();
        assert!(
            ran,
            "a multi-replica authority engages the state-vector sync"
        );
        assert_eq!(a.text(), b.text(), "synced replicas converge");
        assert!(a.text().contains("alpha") && a.text().contains("beta"));

        // GitAuthoritative → ephemeral CRDT; no live peer to converge with, so the
        // sync is skipped and the replicas are left untouched.
        let c = ReplicaState::new(1);
        c.apply_local_edit(0, 0, "gamma");
        let d = ReplicaState::new(2);
        d.apply_local_edit(0, 0, "delta");
        let ran = sync_under_authority(CrdtAuthority::GitAuthoritative, &c, &d).unwrap();
        assert!(
            !ran,
            "the git-authoritative/ephemeral path does not sync live replicas"
        );
        assert_ne!(c.text(), d.text(), "skipped sync leaves replicas untouched");
    }

    #[test]
    fn commit_barrier_is_gated_by_authority() {
        // MultiReplica → flush the live editor and confirm a consistent cut.
        let canonical = ReplicaState::new(1);
        let editor = ReplicaState::new(2);
        editor.apply_local_edit(0, 0, "live");
        assert!(
            commit_barrier_under_authority(CrdtAuthority::MultiReplica, &canonical, &[&editor])
                .unwrap()
        );
        assert!(
            canonical.text().contains("live"),
            "the barrier flushed the live editor into the canonical replica"
        );

        // GitAuthoritative → no live editors to flush; trivially ready, canonical
        // untouched (the headless path snapshots git, not a live replica).
        let c2 = ReplicaState::new(1);
        let e2 = ReplicaState::new(2);
        e2.apply_local_edit(0, 0, "ignored");
        assert!(
            commit_barrier_under_authority(CrdtAuthority::GitAuthoritative, &c2, &[&e2]).unwrap()
        );
        assert_eq!(
            c2.text(),
            "",
            "the git-authoritative barrier does not flush live replicas"
        );
    }
}
