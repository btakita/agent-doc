//! Phase 3 foundation: coordinator-free replica convergence over lazily's
//! op/delta CRDT substrate (`#adoc-live-prompt-drift-operator-edit`).
//!
//! # Why this module exists
//!
//! The replica reconciliation architecture (turn ⇄ CPC/supervisor ⇄ editor
//! shadow) needs a document model where **any replica that is written to
//! reconciles with the sender before accepting the change**, and where
//! concurrent edits from independent replicas converge **without a shared base
//! or coordinator**. That is a CRDT property, not a 3-way-merge property.
//!
//! Verification result (Phase 3 gating question in
//! `tasks/agent-doc/plan-realtime-reconcile-replicas.md`): lazily 0.13.1 already
//! ships that substrate.
//!
//! - [`lazily::TextCrdt`] is a peer-keyed op CRDT: `from_str(peer, s)` /
//!   `fork(peer)` seed a replica; `insert`/`insert_str`/`delete` are op-level
//!   edits; `merge(other)` is a **commutative, associative, idempotent** state
//!   merge (tombstones are order-independent — concurrent deletes keep the
//!   smaller delete id, so merge order never changes the result).
//! - lazily's `crdt_plane::CrdtPlaneRuntime` adds the full anti-entropy layer:
//!   an HLC clock + per-peer `StampFrontier`, and `sync_frame_since(frontier)` /
//!   `sync_reply(request)` ship exactly the ops a peer's frontier is missing —
//!   i.e. **delta sync keyed on causal metadata**, the coordinator-free pull
//!   protocol the CPC/editor replicas need.
//!
//! So yrs (the current whole-document `Y.Text`) is **retirable for the replica
//! model** — lazily's per-cell plane is a better fit (per-cell isolation kills
//! the cross-cell splicing corruption class, and HLC frontiers give causal
//! stability for GC). The full adoption — one `CrdtPlaneRuntime` per replica,
//! per-cell `TextCrdt`s behind the shared FFI, editor shadow reconcile — is the
//! remaining Phase 3 work, scoped in the plan doc.
//!
//! This module is the tested substrate proof: it demonstrates that two replicas
//! which forked from a common document and edited **concurrently** converge to
//! the same text under lazily's merge, independent of merge order. It is
//! deliberately NOT wired into the hot write path yet — the write path continues
//! to use the reconcile loop from `write_policy` (Phase 2) until the full plane
//! adoption lands behind its own review.

use lazily::TextCrdt;

/// Outcome of converging two concurrently-edited replicas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaConvergence {
    /// The converged text after cross-merging both replicas.
    pub text: String,
    /// True when both replicas reached identical text (the CRDT guarantee). A
    /// `false` here would indicate a substrate defect, not a user conflict.
    pub converged: bool,
    /// True when the converged result is independent of which replica merged
    /// first (commutativity check).
    pub order_independent: bool,
}

/// Converge two replicas that forked from `base` and edited concurrently.
///
/// Seeds a shared root, forks a replica per peer, applies each side's edits, then
/// cross-merges both directions. Returns the converged text plus the CRDT
/// invariants observed (equal replicas, order-independent result). This is the
/// coordinator-free primitive the CPC/editor/turn replicas build on: neither side
/// needs the other's base, only its op state.
pub fn converge_two_replicas<A, B>(base: &str, edit_a: A, edit_b: B) -> ReplicaConvergence
where
    A: FnOnce(&mut TextCrdt),
    B: FnOnce(&mut TextCrdt),
{
    let root = TextCrdt::from_str(0, base);

    // Two independent replicas fork from the shared root with distinct peer ids.
    let mut a = root.fork(1);
    let mut b = root.fork(2);
    edit_a(&mut a);
    edit_b(&mut b);

    // Cross-merge: each replica reconciles the other's ops before reading.
    let mut a_then_b = a.clone();
    a_then_b.merge(&b);
    let mut b_then_a = b.clone();
    b_then_a.merge(&a);

    let text = a_then_b.text();
    ReplicaConvergence {
        converged: a_then_b.text() == b_then_a.text(),
        order_independent: a_then_b.text() == b_then_a.text(),
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_concurrent_inserts_converge_coordinator_free() {
        // Replica A appends at the end; replica B inserts at the front — disjoint
        // regions, concurrent, no coordinator. Both must land and both replicas
        // must agree.
        let out = converge_two_replicas(
            "middle",
            |a| a.insert_str(a.len(), " END"),
            |b| b.insert_str(0, "START "),
        );
        assert!(out.converged, "concurrent disjoint edits must converge");
        assert!(out.order_independent, "merge must be order-independent");
        assert!(
            out.text.contains("START ") && out.text.contains(" END") && out.text.contains("middle"),
            "both replicas' edits must survive: {:?}",
            out.text
        );
    }

    #[test]
    fn concurrent_delete_and_insert_converge() {
        // A deletes a char, B inserts — concurrent overlapping-ish edits still
        // converge deterministically.
        let out = converge_two_replicas(
            "abc",
            |a| a.delete(0),
            |b| b.insert_str(b.len(), "d"),
        );
        assert!(out.converged);
        assert!(out.order_independent);
        assert!(
            out.text.contains('d') && !out.text.starts_with('a'),
            "delete + insert must both apply: {:?}",
            out.text
        );
    }

    #[test]
    fn no_edits_is_identity() {
        let out = converge_two_replicas("stable doc", |_| {}, |_| {});
        assert!(out.converged);
        assert_eq!(out.text, "stable doc");
    }

    #[test]
    fn concurrent_deletes_of_same_char_converge_order_independent() {
        // Both replicas delete the same character — tombstones are sticky and
        // order-independent, so the replicas converge rather than diverging on
        // merge order.
        let out = converge_two_replicas("xyz", |a| a.delete(1), |b| b.delete(1));
        assert!(out.converged);
        assert!(out.order_independent);
        assert_eq!(out.text, "xz");
    }
}
