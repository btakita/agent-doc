//! State-vector sync protocol primitive (`#crdtauth1sv`).
//!
//! The deferred half of `#crdtauth1`. Where [`crate::crdt::merge_by_component`]
//! rebuilds throwaway yrs docs from text and runs a whole-`.yrs`-snapshot merge
//! every cycle, this module models the **incremental per-replica state-vector
//! exchange** the CRDT-authority plan calls for
//! (`tasks/agent-doc/plan-crdt-authority-model.md`, phase 2):
//!
//! - Each [`ReplicaState`] keeps its yrs `Doc` **across cycles** (a durable
//!   replica), instead of being rebuilt from text each merge.
//! - A sync exchanges only the updates the other side is **missing**:
//!   `peer.encode_state_as_update(my_state_vector)`. The state vector is a compact
//!   per-client causal summary, not the whole document — so a sync ships a delta,
//!   not a snapshot.
//! - Convergence is **commutative, associative, idempotent**, and yrs buffers an
//!   update whose causal dependencies have not arrived yet (causal buffering), so
//!   out-of-order / lagged delivery self-heals once the gap is filled — never a
//!   permanent conflict, never data loss.
//!
//! This is the protocol **primitive + tests** only. Rewiring the live
//! `merge_by_component` call sites onto it and wiring the FFI-node ↔ supervisor
//! channel are follow-up rungs. The authority gate (sync only under
//! `CrdtAuthority::MultiReplica`; the `GitAuthoritative` / ephemeral path keeps
//! rebuilding from git) lives in `agent-doc-orchestration` over this primitive,
//! since the authority layer lives there.

use anyhow::Result;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact, Update};

const TEXT_KEY: &str = "content";

/// A durable per-replica CRDT state — one participant (an editor's FFI node, or
/// the supervisor) in a multi-replica document session.
///
/// Unlike [`crate::crdt::CrdtDoc`] used by the rebuild-from-text merge, a
/// `ReplicaState` is **kept across cycles**: it accumulates ops and exchanges
/// only deltas with its peers via [`ReplicaState::state_vector`] /
/// [`ReplicaState::diff`] / [`ReplicaState::apply_update`].
pub struct ReplicaState {
    doc: Doc,
}

impl ReplicaState {
    /// Create a fresh empty replica with a stable `client_id`. Distinct replicas
    /// MUST use distinct client ids so concurrent inserts order deterministically
    /// (yrs orders concurrent inserts at the same position by ascending client
    /// id, matching the agent-before-human convention in [`crate::crdt`]).
    pub fn new(client_id: u64) -> Self {
        let doc = Doc::with_client_id(client_id);
        doc.get_or_insert_text(TEXT_KEY);
        Self { doc }
    }

    /// Bootstrap a replica from a previously encoded state (the durable boundary
    /// checkpoint / a peer's full snapshot on first contact).
    pub fn from_encoded(client_id: u64, state: &[u8]) -> Result<Self> {
        let replica = Self::new(client_id);
        replica.apply_update(state)?;
        Ok(replica)
    }

    /// The current converged text.
    pub fn text(&self) -> String {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let txn = self.doc.transact();
        text.get_string(&txn)
    }

    /// Apply a local edit: delete `delete_len` chars at `offset`, then insert
    /// `insert` there (one transaction). Mirrors [`crate::crdt::CrdtDoc::apply_edit`].
    pub fn apply_local_edit(&self, offset: u32, delete_len: u32, insert: &str) {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let mut txn = self.doc.transact_mut();
        if delete_len > 0 {
            text.remove_range(&mut txn, offset, delete_len);
        }
        if !insert.is_empty() {
            text.insert(&mut txn, offset, insert);
        }
    }

    /// This replica's state vector, encoded for the wire. The state vector is the
    /// **compact per-client causal summary** (each client's clock), NOT the whole
    /// document: a peer replies with [`diff`](Self::diff) carrying only the ops
    /// this vector is missing.
    pub fn state_vector(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.state_vector().encode_v1()
    }

    /// The incremental update carrying exactly the ops `their_sv` is missing
    /// (`encode_state_as_update(their_sv)`) — a delta, never a whole-document
    /// snapshot. This is the sync reply a replica sends a peer that announced
    /// `their_sv`.
    pub fn diff(&self, their_sv: &[u8]) -> Result<Vec<u8>> {
        let sv = StateVector::decode_v1(their_sv)
            .map_err(|e| anyhow::anyhow!("decode state vector: {e}"))?;
        let txn = self.doc.transact();
        Ok(txn.encode_state_as_update_v1(&sv))
    }

    /// Apply a remote update. **Idempotent** (re-applying a known update is a
    /// no-op) and **causal-buffered** by yrs (an update whose causal dependencies
    /// have not arrived is buffered and integrated once they do), so duplicate or
    /// out-of-order delivery converges rather than corrupting.
    pub fn apply_update(&self, update: &[u8]) -> Result<()> {
        let update =
            Update::decode_v1(update).map_err(|e| anyhow::anyhow!("decode update: {e}"))?;
        let mut txn = self.doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply update: {e}"))?;
        Ok(())
    }

    /// The full encoded state — a durable projection / boundary checkpoint, or the
    /// snapshot a peer needs on first contact (no shared history yet).
    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    }
}

/// One incremental sync round between two replicas: each announces its state
/// vector, receives only the updates it is missing, and applies them. After the
/// round both replicas have converged. This models a single FFI-node ↔ supervisor
/// exchange; repeated rounds (or one round per delivered op) keep live replicas in
/// sync without ever shipping a whole-document snapshot after the first contact.
pub fn sync(a: &ReplicaState, b: &ReplicaState) -> Result<()> {
    let a_sv = a.state_vector();
    let b_sv = b.state_vector();
    // Each side computes the delta the OTHER is missing, then applies what it is
    // told. Order between the two applies does not matter (commutative merge).
    let a_to_b = a.diff(&b_sv)?;
    let b_to_a = b.diff(&a_sv)?;
    b.apply_update(&a_to_b)?;
    a.apply_update(&b_to_a)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_sv() -> Vec<u8> {
        ReplicaState::new(999).state_vector()
    }

    #[test]
    fn incremental_sync_converges_divergent_edits() {
        let base = ReplicaState::new(1);
        base.apply_local_edit(0, 0, "hello world");
        let snapshot = base.encode_state();

        let a = ReplicaState::from_encoded(1, &snapshot).unwrap();
        let b = ReplicaState::from_encoded(2, &snapshot).unwrap();
        // Concurrent, non-overlapping edits: a prepends, b appends.
        a.apply_local_edit(0, 0, "A: ");
        let b_len = b.text().chars().count() as u32;
        b.apply_local_edit(b_len, 0, " [B]");

        sync(&a, &b).unwrap();
        assert_eq!(a.text(), b.text(), "replicas converge after one sync round");
        assert!(a.text().contains("A: ") && a.text().contains("[B]"));
    }

    #[test]
    fn diff_ships_only_missing_delta_not_whole_snapshot() {
        let base = ReplicaState::new(1);
        base.apply_local_edit(0, 0, "the quick brown fox jumps over the lazy dog");
        let snapshot = base.encode_state();
        let a = ReplicaState::from_encoded(1, &snapshot).unwrap();
        let b = ReplicaState::from_encoded(2, &snapshot).unwrap();

        // b already knows the shared base — a's delta for b carries only NEW ops.
        a.apply_local_edit(0, 0, "X");
        let delta = a.diff(&b.state_vector()).unwrap();
        let full = a.encode_state();
        assert!(
            delta.len() < full.len(),
            "incremental delta ({}) must be smaller than the whole snapshot ({})",
            delta.len(),
            full.len()
        );
        // Applying just that delta converges b.
        b.apply_update(&delta).unwrap();
        assert_eq!(a.text(), b.text());
    }

    #[test]
    fn apply_update_is_idempotent() {
        let base = ReplicaState::new(1);
        base.apply_local_edit(0, 0, "content");
        let a = ReplicaState::from_encoded(1, &base.encode_state()).unwrap();
        let b = ReplicaState::from_encoded(2, &base.encode_state()).unwrap();
        a.apply_local_edit(0, 0, "PREFIX ");
        let delta = a.diff(&b.state_vector()).unwrap();

        b.apply_update(&delta).unwrap();
        let once = b.text();
        // Re-deliver the SAME update — must be a no-op (no duplicated insert).
        b.apply_update(&delta).unwrap();
        assert_eq!(b.text(), once, "re-applying a known update is idempotent");
    }

    #[test]
    fn out_of_order_updates_self_heal_via_causal_buffering() {
        let a = ReplicaState::new(1);
        a.apply_local_edit(0, 0, "first");
        // u1 = everything a has so far (delta against an empty peer).
        let u1 = a.diff(&empty_sv()).unwrap();
        let sv_after_u1 = a.state_vector();
        // a's second op, captured as a delta that causally DEPENDS on u1.
        let a_len = a.text().chars().count() as u32;
        a.apply_local_edit(a_len, 0, " second");
        let u2 = a.diff(&sv_after_u1).unwrap();

        // Deliver OUT OF ORDER: u2 (which depends on u1) before u1.
        let b = ReplicaState::new(2);
        b.apply_update(&u2).unwrap(); // buffered by yrs: deps missing
        assert_ne!(
            b.text(),
            a.text(),
            "u2 alone cannot apply — its deps are missing"
        );
        b.apply_update(&u1).unwrap(); // fills the gap → buffered u2 integrates
        assert_eq!(
            b.text(),
            a.text(),
            "out-of-order delivery self-heals once causal deps arrive"
        );
    }

    #[test]
    fn bidirectional_sync_converges_and_keeps_both_contributions() {
        let a = ReplicaState::new(1);
        a.apply_local_edit(0, 0, "alpha");
        let b = ReplicaState::new(2);
        b.apply_local_edit(0, 0, "beta");
        sync(&a, &b).unwrap();
        assert_eq!(a.text(), b.text());
        assert!(a.text().contains("alpha") && a.text().contains("beta"));
    }

    #[test]
    fn repeated_sync_is_stable_no_op_after_convergence() {
        let a = ReplicaState::new(1);
        a.apply_local_edit(0, 0, "stable");
        let b = ReplicaState::new(2);
        sync(&a, &b).unwrap();
        let converged = a.text();
        // A second round with no new ops must not change anything.
        sync(&a, &b).unwrap();
        assert_eq!(a.text(), converged);
        assert_eq!(b.text(), converged);
    }

    #[test]
    fn encode_state_roundtrips_text() {
        let a = ReplicaState::new(7);
        a.apply_local_edit(0, 0, "roundtrip me");
        let restored = ReplicaState::from_encoded(7, &a.encode_state()).unwrap();
        assert_eq!(restored.text(), "roundtrip me");
    }
}
