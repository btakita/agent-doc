//! CRDT-authority state machine (`#crdtauth1`).
//!
//! An additive authority layer riding the existing merge-ownership state machine
//! ([`crate::merge_control_state_machine`], `#mergestatemachine`) and the
//! per-document hosting-epoch backbone ([`crate::state_backbone`], `#xdocsuper1/3`,
//! commit `66ac6c64`). It reframes that SM from *"who may write the buffer now"*
//! to *"who is the CRDT authority"* — the step-1 rung of the CRDT authority model
//! (`tasks/agent-doc/plan-crdt-authority-model.md`).
//!
//! ## The two authority states
//!
//! - [`CrdtAuthority::GitAuthoritative`] — **Detached**: no live editor (pure CLI /
//!   Codex / headless). Git is the source of truth and the CRDT is **ephemeral**:
//!   rebuilt from the committed document + that turn's ops, with no durable `.yrs`
//!   treated as authoritative. This is most headless dogfooding traffic, and it
//!   carries zero stale-`.yrs` class by construction (the recovery `reset
//!   --from-current` works precisely because it discards the persisted CRDT).
//! - [`CrdtAuthority::MultiReplica`] — **EditorAttached**: a live editor plugin is
//!   attached. The supervisor hosts the canonical replica; each editor hosts its
//!   own. Git/disk are durable **projections checkpointed at boundaries**, not the
//!   live coordination medium.
//!
//! The `attach` / `detach` event is the transition, and it rides the hosting-epoch
//! substrate: per-document isolation is mandatory (a stale-overlay replay for one
//! document must not leak into another's authority — the authority derives
//! *per-document* from that document's own projection facts).
//!
//! ## Scope (`#crdtauth1`)
//!
//! This is the authority **state concept + its transitions + wiring to the
//! existing SM/backbone** only. The state-vector sync protocol, the
//! `merge_by_component` replacement, and the wire/IPC ack scheme are explicitly
//! deferred to later rungs (steps 2–6 of the plan). This layer is purely
//! *derived from* the existing [`MergeOwnershipPhase`] / hosting-epoch facts and
//! changes no runtime disk-write/merge behavior; it only exposes the authority
//! the later rungs consult.

use serde::{Deserialize, Serialize};

use crate::merge_control_state_machine::{
    MergeOwnershipEvent, MergeOwnershipPhase, OwnershipLiveness, ownership_probe,
};
use crate::state_backbone::{DocumentStateProjection, EventLedger, TransportPatchPhase};

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
/// `GitAuthoritative` exactly as [`crate::merge_control_state_machine::disk_write_permitted_for_file`]
/// routes a stale listener to the disk path.
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
///
/// Decision-equivalent to inverting
/// [`crate::merge_control_state_machine::disk_write_permitted_for_file`]:
/// disk-write-permitted (no live editor) ⇔ `GitAuthoritative`.
pub fn authority_from_liveness(liveness: &OwnershipLiveness) -> CrdtAuthority {
    let resolved = ownership_probe(MergeOwnershipPhase::Attached, liveness);
    let phase = match resolved {
        Some(MergeOwnershipEvent::EditorBufferObserved) => MergeOwnershipPhase::EditorOwnsBuffer,
        // Dead pid / stale listener (no lease) both route to the no-editor path.
        _ => MergeOwnershipPhase::Detached,
    };
    authority_for(phase)
}

/// Read the live editor-attachment facts for a document from its plugin-owner
/// lease sidecar and resolve the CRDT authority. This is the write-path entry
/// point, mirroring [`OwnershipLiveness::for_file`]: authority is observed fresh
/// (no persistent authority phase is stored in this rung — it derives from the
/// existing facts on demand).
pub fn authority_for_file(file: &str) -> CrdtAuthority {
    authority_from_liveness(&OwnershipLiveness::for_file(file))
}

/// Derive the CRDT authority for a single document from a backbone projection,
/// riding the hosting-epoch substrate.
///
/// Authority follows the live editor. A document with a live editor transport
/// (the `EditorIpcBridge` actor has an active generation, and the latest editor
/// transport patch is not a terminal force-disk fallback) is
/// [`MultiReplica`](CrdtAuthority::MultiReplica); otherwise the document is
/// headless and [`GitAuthoritative`](CrdtAuthority::GitAuthoritative).
///
/// This is **per-document by construction**: it consults only the passed
/// document's own projection. A stale-overlay replay for a *different* document
/// (rejected/dropped by the hosting-epoch reset) never reaches this projection, so
/// it cannot flip this document's authority — `#xdocsuper1/3` is the load-bearing
/// isolation here, exactly as the model requires.
pub fn authority_for_projection(projection: &DocumentStateProjection) -> CrdtAuthority {
    if document_has_live_editor_transport(projection) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

/// Derive the CRDT authority for `document_hash` from the event ledger. Returns
/// [`GitAuthoritative`](CrdtAuthority::GitAuthoritative) for an unknown document
/// (no projection yet) — a document the supervisor has never hosted with an editor
/// is headless until proven otherwise (fail-safe to the cheapest, zero-stale
/// state).
pub fn authority_for_document(ledger: &EventLedger, document_hash: &str) -> CrdtAuthority {
    match ledger.project_document(document_hash) {
        Some(projection) => authority_for_projection(&projection),
        None => CrdtAuthority::GitAuthoritative,
    }
}

/// Whether a document's projection proves a live editor transport replica.
///
/// A live editor is proven when the editor-IPC-bridge transport has an active
/// generation and the latest editor transport patch has not terminally fallen
/// back to a disk write (a force-disk fallback is the supervisor abandoning the
/// editor replica for this turn — the document drops back to the git-authoritative
/// path). A document with no editor transport at all is headless.
fn document_has_live_editor_transport(projection: &DocumentStateProjection) -> bool {
    if projection.transport.editor_generation.is_none() {
        return false;
    }
    // The latest editor transport patch tells us whether the editor replica is
    // still the coordination medium or the supervisor abandoned it to disk.
    match projection.transport.patches.iter().next_back() {
        // A terminal force-disk fallback is the supervisor abandoning the editor
        // replica for this turn — the document drops back to git-authoritative.
        Some((_, patch)) => !matches!(patch.phase, TransportPatchPhase::ForceDiskFallback),
        // An editor generation with no patch yet is a freshly-attached editor.
        None => true,
    }
}

/// Run one incremental state-vector sync round between two replicas ONLY under a
/// multi-replica authority (`#crdtauth1sv` seam).
///
/// This wires the state-vector sync primitive ([`agent_doc_core::crdt_sync`]) to
/// the authority SM: the incremental protocol is engaged exactly when the
/// authority proves a live multi-replica session ([`CrdtAuthority::MultiReplica`]).
/// Under [`CrdtAuthority::GitAuthoritative`] the CRDT is ephemeral — git is the
/// source of truth, rebuilt from text each turn — so there is no second live
/// writer to converge with and the sync is skipped. Returns whether a sync round
/// actually ran.
pub fn sync_under_authority(
    authority: CrdtAuthority,
    a: &agent_doc_core::crdt_sync::ReplicaState,
    b: &agent_doc_core::crdt_sync::ReplicaState,
) -> anyhow::Result<bool> {
    if authority.editor_attached() {
        agent_doc_core::crdt_sync::sync(a, b)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge_control_state_machine::OwnershipLiveness;
    use agent_doc_core::crdt_sync::ReplicaState;

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
        use crate::merge_control_state_machine::disk_write_permitted;

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
}
