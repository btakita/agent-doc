//! State-vector sync protocol primitive (`#crdtauth1sv`).
//!
//! The deferred half of `#crdtauth1`. Where [`crate::crdt::merge_by_component`]
//! rebuilds throwaway CRDTs from text and runs a whole-state merge
//! every cycle, this module models the **incremental per-replica state-vector
//! exchange** the CRDT-authority plan calls for
//! (`tasks/agent-doc/plan-crdt-authority-model.md`, phase 2):
//!
//! - Each [`ReplicaState`] keeps its lazily [`TextCrdt`] **across cycles** (a durable
//!   replica), instead of being rebuilt from text each merge.
//! - A sync exchanges only the updates the other side is **missing**:
//!   `peer.encode_state_as_update(my_state_vector)`. The state vector is a compact
//!   per-client causal summary, not the whole document — so a sync ships a delta,
//!   not a snapshot.
//! - Convergence is **commutative, associative, idempotent**; op identities make
//!   duplicate and out-of-order delivery safe, so lagged delivery self-heals once
//!   the gap is filled — never a permanent conflict, never data loss.
//!
//! This is the protocol **primitive + tests** only. Rewiring the live
//! `merge_by_component` call sites onto it and wiring the FFI-node ↔ supervisor
//! channel are follow-up rungs. The authority gate (sync only under
//! `CrdtAuthority::MultiReplica`; the `GitAuthoritative` / ephemeral path keeps
//! rebuilding from git) lives in `agent-doc-orchestration` over this primitive,
//! since the authority layer lives there.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use lazily::{OpId, TextCrdt, TextOp, TextVersionVector};
use std::{cell::RefCell, collections::HashMap};

/// A durable per-replica CRDT state — one participant (an editor's FFI node, or
/// the supervisor) in a multi-replica document session.
///
/// Backed by a lazily [`TextCrdt`] (replacing the former Yjs `Doc`). Unlike
/// [`crate::crdt::CrdtDoc`] used by the rebuild-from-text merge, a `ReplicaState`
/// is **kept across cycles**: it accumulates ops and exchanges only deltas with its
/// peers via [`ReplicaState::state_vector`] (a `version_vector` frontier) /
/// [`ReplicaState::diff`] (`delta_since`) / [`ReplicaState::apply_update`]
/// (`apply_delta`). Version vectors remain compact JSON. Text-op updates use a
/// versioned zstd-compressed MessagePack envelope, with legacy JSON decoding so
/// durable pending/outbox data written by older binaries remains replayable.
pub struct ReplicaState {
    text: RefCell<TextCrdt>,
}

const COMPACT_TEXT_OPS_MAGIC: &[u8] = b"ADCR1:";
/// Columnar/delta envelope (`#bootstrapsize`). See [`encode_columnar_ops`].
const COLUMNAR_TEXT_OPS_MAGIC: &[u8] = b"ADCR2:";

/// Struct-of-arrays projection of a `TextOp` batch, with the two integer columns
/// that dominate the payload delta-encoded.
///
/// `#bootstrapsize`: measured on a real 57KB session document whose bootstrap had
/// grown to 815,754 bytes (88,630 ops, ZERO tombstones, ONE peer). Array-of-structs
/// interleaves `id`/`origin`/`deleted` per character, so zstd never sees the
/// regularity: counters are near-sequential, `peer` is usually constant, and
/// `origin` is nearly always the preceding character (that document had just
/// **2 distinct** origin deltas across 88,630 ops). Grouping each field into its
/// own column and storing counters as deltas exposes all of it. Measured on that
/// same real payload: 815,754 -> 416,392 bytes, a 2.0x reduction.
///
/// Note a plain struct-of-arrays regrouping that keeps `OpId` opaque is NOT
/// enough — measured at only 1.2x. The win requires decomposing `OpId` into
/// scalar columns so the deltas are visible.
#[derive(serde::Serialize, serde::Deserialize)]
struct ColumnarOps {
    /// Every op's character, concatenated. Compresses as ordinary text.
    chars: String,
    /// `id.counter`, delta-encoded against the previous op's counter.
    id_counter_delta: Vec<i64>,
    /// `id.peer` per op. Usually one repeated value.
    id_peer: Vec<u64>,
    /// Per op: 1 when `origin` is `Some`, 0 for `None` (document start).
    origin_present: Vec<u8>,
    /// For present origins only: `id.counter - origin.counter`.
    origin_counter_delta: Vec<i64>,
    /// For present origins only: `origin.peer`.
    origin_peer: Vec<u64>,
    /// Sparse tombstones: index into the op sequence, plus the delete's own OpId.
    del_idx: Vec<u32>,
    del_counter: Vec<u64>,
    del_peer: Vec<u64>,
}

/// Rebuild `OpId`s from scalar columns.
///
/// `OpId`'s fields are private and it exposes no constructor, but it derives
/// serde and encodes as a 2-element array, so a `(counter, peer)` tuple has a
/// byte-identical MessagePack representation. Done in one bulk conversion rather
/// than per-op. `opid_serde_shape_is_a_counter_peer_pair` pins this assumption so
/// a lazily upgrade that changes the representation fails loudly here instead of
/// silently corrupting a payload.
fn opids_from_pairs(pairs: &[(u64, u64)]) -> Result<Vec<OpId>> {
    let bytes = rmp_serde::to_vec(pairs).context("encode OpId scalar pairs")?;
    rmp_serde::from_slice(&bytes).context("rebuild OpIds from scalar pairs")
}

fn encode_columnar_ops(ops: &[TextOp]) -> Result<Vec<u8>> {
    let mut columns = ColumnarOps {
        chars: String::with_capacity(ops.len()),
        id_counter_delta: Vec::with_capacity(ops.len()),
        id_peer: Vec::with_capacity(ops.len()),
        origin_present: Vec::with_capacity(ops.len()),
        origin_counter_delta: Vec::new(),
        origin_peer: Vec::new(),
        del_idx: Vec::new(),
        del_counter: Vec::new(),
        del_peer: Vec::new(),
    };
    let mut previous_counter: i64 = 0;
    for (index, op) in ops.iter().enumerate() {
        columns.chars.push(op.ch);
        let counter = i64::try_from(op.id.counter()).context("op counter exceeds i64")?;
        columns.id_counter_delta.push(counter - previous_counter);
        previous_counter = counter;
        columns.id_peer.push(op.id.peer());
        match op.origin {
            Some(origin) => {
                columns.origin_present.push(1);
                let origin_counter =
                    i64::try_from(origin.counter()).context("origin counter exceeds i64")?;
                columns.origin_counter_delta.push(counter - origin_counter);
                columns.origin_peer.push(origin.peer());
            }
            None => columns.origin_present.push(0),
        }
        if let Some(deleted) = op.deleted {
            columns
                .del_idx
                .push(u32::try_from(index).context("op index exceeds u32")?);
            columns.del_counter.push(deleted.counter());
            columns.del_peer.push(deleted.peer());
        }
    }
    let packed = rmp_serde::to_vec(&columns).context("encode columnar text ops as MessagePack")?;
    let compressed =
        zstd::stream::encode_all(packed.as_slice(), 3).context("compress columnar text ops")?;
    let compressed = BASE64_STANDARD.encode(compressed);
    let mut encoded = Vec::with_capacity(COLUMNAR_TEXT_OPS_MAGIC.len() + compressed.len());
    encoded.extend_from_slice(COLUMNAR_TEXT_OPS_MAGIC);
    encoded.extend_from_slice(compressed.as_bytes());
    Ok(encoded)
}

fn decode_columnar_ops(compressed: &[u8]) -> Result<Vec<TextOp>> {
    let compressed = BASE64_STANDARD
        .decode(compressed)
        .context("decode columnar text-op base64")?;
    let packed =
        zstd::stream::decode_all(compressed.as_slice()).context("decompress columnar text ops")?;
    let columns: ColumnarOps =
        rmp_serde::from_slice(&packed).context("decode columnar text ops")?;

    let count = columns.id_counter_delta.len();
    let mut absolute_counters = Vec::with_capacity(count);
    let mut previous_counter: i64 = 0;
    for delta in &columns.id_counter_delta {
        previous_counter += *delta;
        absolute_counters.push(previous_counter);
    }
    let id_pairs = absolute_counters
        .iter()
        .zip(columns.id_peer.iter())
        .map(|(counter, peer)| {
            Ok((
                u64::try_from(*counter).context("negative op counter")?,
                *peer,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let ids = opids_from_pairs(&id_pairs)?;

    let origin_pairs = {
        let mut cursor = 0usize;
        let mut pairs = Vec::with_capacity(columns.origin_counter_delta.len());
        for (index, present) in columns.origin_present.iter().enumerate() {
            if *present == 0 {
                continue;
            }
            let counter = *absolute_counters
                .get(index)
                .context("origin column longer than op column")?;
            let delta = *columns
                .origin_counter_delta
                .get(cursor)
                .context("missing origin counter delta")?;
            let peer = *columns
                .origin_peer
                .get(cursor)
                .context("missing origin peer")?;
            pairs.push((
                u64::try_from(counter - delta).context("negative origin counter")?,
                peer,
            ));
            cursor += 1;
        }
        pairs
    };
    let origins = opids_from_pairs(&origin_pairs)?;

    let del_pairs = columns
        .del_counter
        .iter()
        .zip(columns.del_peer.iter())
        .map(|(counter, peer)| (*counter, *peer))
        .collect::<Vec<_>>();
    let deletes = opids_from_pairs(&del_pairs)?;
    let mut deleted_by_index = std::collections::HashMap::with_capacity(deletes.len());
    for (slot, index) in columns.del_idx.iter().enumerate() {
        deleted_by_index.insert(
            *index as usize,
            *deletes.get(slot).context("missing delete OpId")?,
        );
    }

    let mut origin_cursor = 0usize;
    let mut ops = Vec::with_capacity(count);
    for (index, ch) in columns.chars.chars().enumerate() {
        let origin = if columns.origin_present.get(index).copied().unwrap_or(0) == 1 {
            let origin = origins
                .get(origin_cursor)
                .copied()
                .context("missing rebuilt origin")?;
            origin_cursor += 1;
            Some(origin)
        } else {
            None
        };
        ops.push(TextOp {
            id: *ids.get(index).context("missing rebuilt op id")?,
            ch,
            origin,
            deleted: deleted_by_index.get(&index).copied(),
        });
    }
    Ok(ops)
}

/// Encode a text-op batch for controller, FFI, and reliable-sync transport.
///
/// Character CRDTs carry an op per codepoint plus tombstones. JSON made a
/// modest markdown buffer expand into tens of megabytes and starved foreground
/// controller reads. MessagePack removes field-name repetition and zstd makes
/// the retained frame proportional enough for interactive closeout.
pub fn encode_update_ops(ops: &[TextOp]) -> Result<Vec<u8>> {
    // `#bootstrapsize`: prefer the columnar/delta envelope (measured 2.0x smaller
    // on a real 815KB bootstrap). It is self-verified before use: a wire format
    // that fails to round-trip must never reach a peer, so any error — or any
    // mismatch — silently falls back to the ADCR1 envelope, which every reader
    // still decodes.
    if let Ok(columnar) = encode_columnar_ops(ops)
        && let Some(body) = columnar.strip_prefix(COLUMNAR_TEXT_OPS_MAGIC)
        && decode_columnar_ops(body).is_ok_and(|decoded| ops_equivalent(&decoded, ops))
    {
        return Ok(columnar);
    }
    let packed = rmp_serde::to_vec(ops).context("encode text ops as MessagePack")?;
    let compressed =
        zstd::stream::encode_all(packed.as_slice(), 3).context("compress MessagePack text ops")?;
    // Keep the envelope UTF-8 so the existing NUL-terminated editor/JNA seam
    // carries compact updates safely during rolling upgrades.
    let compressed = BASE64_STANDARD.encode(compressed);
    let mut encoded = Vec::with_capacity(COMPACT_TEXT_OPS_MAGIC.len() + compressed.len());
    encoded.extend_from_slice(COMPACT_TEXT_OPS_MAGIC);
    encoded.extend_from_slice(compressed.as_bytes());
    Ok(encoded)
}

/// True when two op batches are field-for-field identical. Used to self-verify a
/// newly encoded envelope before it is allowed onto the wire.
fn ops_equivalent(left: &[TextOp], right: &[TextOp]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(a, b)| {
            a.id == b.id && a.ch == b.ch && a.origin == b.origin && a.deleted == b.deleted
        })
}

/// Decode the columnar envelope, the compact envelope, or a pre-upgrade JSON
/// `Vec<TextOp>`. Every historical format stays readable: durable pending/outbox
/// data written by older binaries must remain replayable after an upgrade.
pub fn decode_update_ops(update: &[u8]) -> Result<Vec<TextOp>> {
    let ops = if let Some(compressed) = update.strip_prefix(COLUMNAR_TEXT_OPS_MAGIC) {
        decode_columnar_ops(compressed)?
    } else if let Some(compressed) = update.strip_prefix(COMPACT_TEXT_OPS_MAGIC) {
        let compressed = BASE64_STANDARD
            .decode(compressed)
            .context("decode compact text-op base64")?;
        let packed = zstd::stream::decode_all(compressed.as_slice())
            .context("decompress MessagePack text ops")?;
        rmp_serde::from_slice(&packed).context("decode MessagePack text ops")?
    } else {
        serde_json::from_slice(update).context("decode legacy JSON text ops")?
    };
    validate_text_ops(&ops)?;
    Ok(ops)
}

/// Reject a malformed text-op graph before [`TextCrdt::apply_delta`] can
/// materialize it.
///
/// Every valid insert is minted after its origin, so following `origin` links
/// must strictly decrease the Lamport counter. That single invariant makes an
/// origin cycle impossible. A retained editor generation once submitted a
/// self-origin edge after controller replacement; `TextCrdt::text()` then
/// traversed that one-node cycle forever while the hub mutex remained held.
/// Validate the immutable graph and delete clock at the wire boundary so an
/// invalid replica is quarantined without mutating canonical or mirror state.
pub fn validate_text_ops(ops: &[TextOp]) -> Result<()> {
    let mut immutable = HashMap::with_capacity(ops.len());
    for op in ops {
        if let Some(origin) = op.origin {
            anyhow::ensure!(
                origin.counter() < op.id.counter(),
                "invalid text-op origin: origin counter {} must precede op counter {}",
                origin.counter(),
                op.id.counter(),
            );
        }
        if let Some(deleted) = op.deleted {
            anyhow::ensure!(
                deleted.counter() > op.id.counter(),
                "invalid text-op deletion: delete counter {} must follow op counter {}",
                deleted.counter(),
                op.id.counter(),
            );
        }
        if let Some((ch, origin)) = immutable.insert(op.id, (op.ch, op.origin)) {
            anyhow::ensure!(
                ch == op.ch && origin == op.origin,
                "invalid text-op batch: duplicate id has conflicting immutable fields",
            );
        }
    }
    Ok(())
}

impl ReplicaState {
    /// Create a fresh empty replica with a stable `client_id` (the CRDT peer id).
    /// Distinct replicas MUST use distinct ids so concurrent inserts order
    /// deterministically (lazily tiebreaks same-position concurrent inserts by
    /// peer).
    pub fn new(client_id: u64) -> Self {
        Self {
            text: RefCell::new(TextCrdt::new(client_id)),
        }
    }

    /// Create a replica seeded from plain text in linear time.
    pub fn from_text(client_id: u64, text: &str) -> Self {
        Self {
            text: RefCell::new(TextCrdt::from_str(client_id, text)),
        }
    }

    /// Bootstrap a replica from a previously encoded state (the durable boundary
    /// checkpoint / a peer's full snapshot on first contact). `apply_delta` of the
    /// snapshot preserves each element's `OpId`, so the reconstructed replica shares
    /// identity with its origin and later deltas merge conflict-free.
    pub fn from_encoded(client_id: u64, state: &[u8]) -> Result<Self> {
        let replica = Self::new(client_id);
        replica.apply_update(state)?;
        Ok(replica)
    }

    /// The current converged text.
    pub fn text(&self) -> String {
        self.text.borrow().text()
    }

    /// Apply a local edit: delete `delete_len` chars at char `offset`, then insert
    /// `insert` there. `offset` and `delete_len` are CODEPOINT (char) units — they
    /// index lazily's char-indexed [`TextCrdt`] directly. Editor plugins convert
    /// their UTF-16 offsets to codepoint offsets before calling the FFI
    /// (`agent_doc_replica_apply_local`), so this must never re-interpret them as
    /// byte offsets: `text[..byte_off]` slices mid-multibyte-character and panics,
    /// which (under `panic = unwind` + the FFI `catch_unwind` guard) degrades the
    /// editor sync for that file, or (under `panic = abort`) kills the host IDE.
    /// Both values saturate to the current char count.
    pub fn apply_local_edit(&self, offset: u32, delete_len: u32, insert: &str) {
        let mut t = self.text.borrow_mut();
        let cur = t.text();
        let total_chars = cur.chars().count();
        // Whole-buffer replace fast path: offset 0 deleting everything (or an
        // empty doc) is a linear `replace_all`, avoiding the quadratic per-char
        // delete path. `delete_len` is in codepoints, so compare against the
        // codepoint count (NOT the byte length).
        if offset == 0 && (total_chars == 0 || delete_len as usize >= total_chars) {
            t.replace_all(insert);
            return;
        }
        let start_char = (offset as usize).min(total_chars);
        if delete_len > 0 {
            let end_char = (start_char + delete_len as usize).min(total_chars);
            t.delete_range(start_char, end_char - start_char);
        }
        if !insert.is_empty() {
            t.insert_str(start_char, insert);
        }
    }

    /// This replica's version vector, encoded (JSON) for the wire. The **compact
    /// per-peer frontier**, NOT the whole document: a peer replies with
    /// [`diff`](Self::diff) carrying only the ops this frontier is missing.
    pub fn state_vector(&self) -> Vec<u8> {
        serde_json::to_vec(&self.text.borrow().version_vector()).unwrap_or_default()
    }

    /// Whether this replica already contains every operation named by `state_vector`.
    ///
    /// A decodable vector is not necessarily an ancestor of this replica: a peer
    /// can retain operations from an obsolete branch after the canonical replica
    /// was rebuilt from a text projection. Incremental bootstrap is safe only when
    /// every retained counter is at or behind this replica's counter.
    pub fn covers_state_vector(&self, state_vector: &[u8]) -> Result<bool> {
        let retained: TextVersionVector =
            serde_json::from_slice(state_vector).context("decode version vector")?;
        let current = self.text.borrow().version_vector();
        Ok(retained.iter().all(|(peer, retained_counter)| {
            current.get(peer).copied().unwrap_or_default() >= *retained_counter
        }))
    }

    /// The incremental update carrying exactly the ops `their_sv` is missing
    /// (`delta_since(their_vv)`) — a delta, never a whole-document snapshot. This is
    /// the sync reply a replica sends a peer that announced `their_sv`.
    pub fn diff(&self, their_sv: &[u8]) -> Result<Vec<u8>> {
        let their_vv: TextVersionVector =
            serde_json::from_slice(their_sv).context("decode version vector")?;
        let delta = self.text.borrow().delta_since(&their_vv);
        encode_update_ops(&delta).context("encode delta")
    }

    /// Apply a remote update (a `TextOp` delta). **Idempotent** (re-applying a known
    /// delta is a no-op) and order-independent (commutative/associative merge), so
    /// duplicate or out-of-order delivery converges rather than corrupting.
    pub fn apply_update(&self, update: &[u8]) -> Result<()> {
        let ops = decode_update_ops(update).context("decode delta")?;
        self.text.borrow_mut().apply_delta(&ops);
        Ok(())
    }

    /// The full encoded state — a durable projection / boundary checkpoint, or the
    /// snapshot a peer needs on first contact. It is `delta_since(∅)`: every op the
    /// replica holds, as a `TextOp` list.
    pub fn encode_state(&self) -> Vec<u8> {
        let snapshot = self.text.borrow().delta_since(&TextVersionVector::new());
        encode_update_ops(&snapshot).unwrap_or_default()
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

/// Whether `canonical` has already integrated every op the `peer` replica holds —
/// i.e. the canonical replica is synced *through* the peer's current state vector.
///
/// Non-destructive: the ops the peer has beyond `canonical` are applied to a
/// throwaway clone of `canonical`; if that changes the clone's state vector,
/// canonical was missing them.
fn canonical_covers(canonical: &ReplicaState, peer: &ReplicaState) -> Result<bool> {
    let missing = peer.diff(&canonical.state_vector())?;
    let probe = ReplicaState::from_encoded(0, &canonical.encode_state())?;
    let before = probe.state_vector();
    probe.apply_update(&missing)?;
    Ok(probe.state_vector() == before)
}

/// State-vector **commit barrier** (`#crdtauth3`): is `canonical` a **consistent
/// cut** — has it integrated every live editor's ops up to that editor's current
/// state vector?
///
/// This replaces the fragile finalize **patch-ack** ("did a queued patch
/// round-trip?") with a **state-vector ack** ("is the canonical replica synced
/// through SV=N for every live editor?"), the structural root fix for the
/// `no_ack` / `ipc_proof_insufficient` / post-commit-worktree-corruption class:
/// a commit can only snapshot a state that provably holds every editor's last
/// keystrokes, so un-propagated editor ops can never be lost at the commit
/// instant. With no live editors the barrier is trivially satisfied (the headless
/// / git-authoritative path).
pub fn commit_barrier_ready(canonical: &ReplicaState, editors: &[&ReplicaState]) -> Result<bool> {
    for editor in editors {
        if !canonical_covers(canonical, editor)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Drive the commit barrier: **flush every live editor's missing ops into
/// `canonical`** ("flush all live editors to SV=N"), then confirm the barrier is
/// satisfied. After this returns `Ok(true)`, a snapshot of `canonical`
/// ([`ReplicaState::encode_state`]) is a consistent cut safe to write to git.
///
/// This is the quiescence point `finalize` should snapshot at: instead of hoping
/// a patch ACK round-tripped, it provably holds every editor's ops.
pub fn flush_to_commit_barrier(
    canonical: &ReplicaState,
    editors: &[&ReplicaState],
) -> Result<bool> {
    for editor in editors {
        let missing = editor.diff(&canonical.state_vector())?;
        canonical.apply_update(&missing)?;
    }
    commit_barrier_ready(canonical, editors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_sv() -> Vec<u8> {
        ReplicaState::new(999).state_vector()
    }

    fn op_id(counter: u64, peer: u64) -> OpId {
        rmp_serde::from_slice(&rmp_serde::to_vec(&(counter, peer)).unwrap()).unwrap()
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
    fn malformed_origin_cycle_is_rejected_before_replica_mutation() {
        let replica = ReplicaState::from_text(7, "canonical stays responsive");
        let id = op_id(42, 7);
        let malformed = serde_json::to_vec(&vec![TextOp {
            id,
            ch: 'x',
            origin: Some(id),
            deleted: None,
        }])
        .unwrap();

        let error = replica.apply_update(&malformed).unwrap_err();
        assert!(
            format!("{error:#}").contains("invalid text-op origin"),
            "unexpected error: {error:#}"
        );
        assert_eq!(
            replica.text(),
            "canonical stays responsive",
            "validation must run before apply_delta mutates or materializes the malformed graph"
        );
    }

    #[test]
    fn conflicting_immutable_fields_for_one_op_id_are_rejected() {
        let id = op_id(2, 7);
        let origin = op_id(1, 7);
        let malformed = vec![
            TextOp {
                id,
                ch: 'a',
                origin: Some(origin),
                deleted: None,
            },
            TextOp {
                id,
                ch: 'b',
                origin: Some(origin),
                deleted: None,
            },
        ];

        let error = validate_text_ops(&malformed).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("duplicate id has conflicting immutable fields")
        );
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
        b.apply_update(&u2).unwrap(); // dependency gap is safe until the missing op arrives
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
    fn opid_serde_shape_is_a_counter_peer_pair() {
        // `#bootstrapsize`: the columnar decoder rebuilds OpIds by deserializing
        // (counter, peer) tuples, because OpId has no public constructor. If a
        // lazily upgrade changes OpId's serde representation this test fails
        // loudly, instead of the codec silently producing corrupt ops.
        let replica = ReplicaState::new(42);
        replica.apply_local_edit(0, 0, "x");
        let ops = decode_update_ops(&replica.encode_state()).unwrap();
        let id = ops[0].id;
        let via_tuple: OpId =
            rmp_serde::from_slice(&rmp_serde::to_vec(&(id.counter(), id.peer())).unwrap()).unwrap();
        assert_eq!(
            via_tuple, id,
            "OpId must serialize as a (counter, peer) pair"
        );
    }

    #[test]
    fn columnar_envelope_roundtrips_inserts_and_tombstones() {
        let replica = ReplicaState::new(7);
        replica.apply_local_edit(0, 0, "hello columnar world");
        // Delete an interior run so tombstones exercise the sparse columns.
        replica.apply_local_edit(5, 10, "");
        replica.apply_local_edit(0, 0, "prefix ");
        let encoded = replica.encode_state();
        assert!(
            encoded.starts_with(COLUMNAR_TEXT_OPS_MAGIC),
            "encode_update_ops must prefer the columnar envelope"
        );
        let restored = ReplicaState::from_encoded(7, &encoded).unwrap();
        assert_eq!(restored.text(), replica.text());
    }

    #[test]
    fn columnar_and_compact_envelopes_decode_to_identical_ops() {
        let replica = ReplicaState::new(3);
        replica.apply_local_edit(0, 0, "alpha beta gamma");
        replica.apply_local_edit(6, 11, "");
        let ops = decode_update_ops(&replica.encode_state()).unwrap();

        // The legacy ADCR1 envelope must still decode to exactly the same ops.
        let packed = rmp_serde::to_vec(&ops).unwrap();
        let compressed = zstd::stream::encode_all(packed.as_slice(), 3).unwrap();
        let mut legacy = COMPACT_TEXT_OPS_MAGIC.to_vec();
        legacy.extend_from_slice(BASE64_STANDARD.encode(compressed).as_bytes());
        let from_legacy = decode_update_ops(&legacy).unwrap();

        assert!(
            ops_equivalent(&from_legacy, &ops),
            "ADCR1 and ADCR2 envelopes must carry identical ops"
        );
    }

    #[test]
    fn columnar_envelope_is_smaller_than_the_compact_envelope() {
        // Realistic shape: one peer, mostly sequential counters, origin almost
        // always the preceding character — the redundancy the columns expose.
        let replica = ReplicaState::new(1);
        replica.apply_local_edit(0, 0, &"lorem ipsum dolor sit amet ".repeat(400));
        let columnar = replica.encode_state();
        let ops = decode_update_ops(&columnar).unwrap();
        let packed = rmp_serde::to_vec(&ops).unwrap();
        let compressed = zstd::stream::encode_all(packed.as_slice(), 3).unwrap();
        let legacy_len = COMPACT_TEXT_OPS_MAGIC.len() + BASE64_STANDARD.encode(compressed).len();
        assert!(
            columnar.len() < legacy_len,
            "columnar envelope ({}) must be smaller than ADCR1 ({legacy_len})",
            columnar.len()
        );
    }

    #[test]
    fn encode_state_roundtrips_text() {
        let a = ReplicaState::new(7);
        a.apply_local_edit(0, 0, "roundtrip me");
        let restored = ReplicaState::from_encoded(7, &a.encode_state()).unwrap();
        assert_eq!(restored.text(), "roundtrip me");
    }

    #[test]
    fn legacy_json_update_remains_decodable() {
        let source = ReplicaState::from_text(7, "legacy pending response\n");
        let ops = source.text.borrow().delta_since(&TextVersionVector::new());
        let legacy = serde_json::to_vec(&ops).unwrap();
        let restored = ReplicaState::from_encoded(8, &legacy).unwrap();
        assert_eq!(restored.text(), source.text());
    }

    #[test]
    fn compact_update_has_versioned_header_and_roundtrips() {
        let source = ReplicaState::from_text(7, "compact response\n");
        let encoded = source.encode_state();
        // `#bootstrapsize`: the encoder now emits the columnar ADCR2 envelope.
        // The contract this test guards is unchanged — a versioned header plus a
        // faithful round-trip — only the preferred version moved.
        assert!(encoded.starts_with(COLUMNAR_TEXT_OPS_MAGIC));
        let restored = ReplicaState::from_encoded(8, &encoded).unwrap();
        assert_eq!(restored.text(), source.text());
    }

    #[test]
    fn compact_state_bounds_large_document_and_tombstone_churn() {
        let initial = (0..1_500)
            .map(|index| format!("queue item {index:04}: durable closeout content\n"))
            .collect::<String>();
        assert!(initial.len() > 60_000);
        let source = ReplicaState::from_text(7, &initial);
        let replacement = initial.replace("queue item", "response  ");
        source.apply_local_edit(0, u32::MAX, &replacement);

        let ops = source.text.borrow().delta_since(&TextVersionVector::new());
        let legacy = serde_json::to_vec(&ops).unwrap();
        let compact = source.encode_state();
        assert!(compact.len() < 2_000_000, "compact bytes={}", compact.len());
        assert!(
            compact.len() * 4 < legacy.len(),
            "compact={} legacy={}",
            compact.len(),
            legacy.len()
        );
        let restored = ReplicaState::from_encoded(8, &compact).unwrap();
        assert_eq!(restored.text(), replacement);
    }

    #[test]
    fn apply_local_edit_uses_codepoint_offsets_on_multibyte_text() {
        // Editor plugins send CODEPOINT (char) offsets/lengths, NOT byte offsets.
        // `あ` is 3 UTF-8 bytes; `✓` is 3 bytes. A byte-offset interpretation would
        // slice `text[..1]` (mid-`あ`) and panic. Codepoint offsets must index the
        // char-indexed TextCrdt directly. Regression for the IDEA SIGABRT crash
        // where a multibyte README panicked inside the FFI `apply_local`.
        let a = ReplicaState::new(1);
        a.apply_local_edit(0, 0, "あb✓c"); // 4 codepoints, 9 bytes
        assert_eq!(a.text(), "あb✓c");

        // Insert at codepoint offset 1 (between あ and b): byte 1 is mid-`あ`.
        a.apply_local_edit(1, 0, "X");
        assert_eq!(a.text(), "あXb✓c");

        // Delete one codepoint at offset 2 (the b). delete_len is in codepoints.
        a.apply_local_edit(2, 1, "");
        assert_eq!(a.text(), "あX✓c");

        // Replace the whole buffer via the offset-0 + full-delete fast path:
        // delete_len is a codepoint count and must compare against the codepoint
        // count (4), not the byte length (8), else the fast path is skipped.
        a.apply_local_edit(0, 4, "全文入れ替え");
        assert_eq!(a.text(), "全文入れ替え");
    }

    #[test]
    fn from_text_and_full_replace_preserve_delta_convergence() {
        let canonical = ReplicaState::from_text(1, &"seeded line\n".repeat(512));
        let member = ReplicaState::from_encoded(2, &canonical.encode_state()).unwrap();

        canonical.apply_local_edit(0, u32::MAX, "replacement\nbody\n");
        let update = canonical.diff(&member.state_vector()).unwrap();
        member.apply_update(&update).unwrap();

        assert_eq!(canonical.text(), "replacement\nbody\n");
        assert_eq!(member.text(), canonical.text());
    }

    #[test]
    fn commit_barrier_blocks_until_editor_ops_are_flushed() {
        let canonical = ReplicaState::new(1);
        canonical.apply_local_edit(0, 0, "base");
        // An editor branches from canonical and types locally (un-propagated).
        let editor = ReplicaState::from_encoded(2, &canonical.encode_state()).unwrap();
        let len = editor.text().chars().count() as u32;
        editor.apply_local_edit(len, 0, " typed");

        // The barrier is NOT ready: a commit now would lose the editor's keystrokes.
        assert!(
            !commit_barrier_ready(&canonical, &[&editor]).unwrap(),
            "barrier must block while an editor has un-propagated ops"
        );

        // Flush → ready; canonical is now a consistent cut holding the editor's ops.
        assert!(flush_to_commit_barrier(&canonical, &[&editor]).unwrap());
        assert!(commit_barrier_ready(&canonical, &[&editor]).unwrap());
        assert!(canonical.text().contains("typed"));

        // The snapshot at the barrier round-trips to the same text (consistent cut).
        let restored = ReplicaState::from_encoded(9, &canonical.encode_state()).unwrap();
        assert_eq!(restored.text(), canonical.text());
    }

    #[test]
    fn commit_barrier_is_trivially_ready_with_no_live_editors() {
        // Headless / git-authoritative: no editor replicas → nothing to lose.
        let canonical = ReplicaState::new(1);
        canonical.apply_local_edit(0, 0, "headless");
        assert!(commit_barrier_ready(&canonical, &[]).unwrap());
    }

    #[test]
    fn commit_barrier_reopens_on_a_new_unsynced_op() {
        let canonical = ReplicaState::new(1);
        let e1 = ReplicaState::new(2);
        let e2 = ReplicaState::new(3);
        e1.apply_local_edit(0, 0, "one");
        e2.apply_local_edit(0, 0, "two");
        assert!(flush_to_commit_barrier(&canonical, &[&e1, &e2]).unwrap());
        assert!(canonical.text().contains("one") && canonical.text().contains("two"));

        // A fresh un-synced keystroke on e1 re-opens the barrier.
        e1.apply_local_edit(0, 0, "Z");
        assert!(
            !commit_barrier_ready(&canonical, &[&e1, &e2]).unwrap(),
            "a new editor op after the cut must re-open the barrier"
        );
    }
}
