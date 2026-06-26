# Plan: Distributed CRDT cell plane (`#lzcrdtplane`)

**Status:** Scoped — plan written this cycle; implementation is multi-phase and
drains as concrete per-phase backlog IDs (do **not** attempt the whole plane in
one cycle).
**Created:** 2026-06-25
**Session:** `tasks/software/lazily-rs.md`
**Spec:** `src/lazily-rs/SPEC.md` §Distributed; `src/lazily-spec/cell-model.md`
(`merge: crdt`)
**Decision (locked, from the backlog item):** purpose-built HLC + LWW/MV/PN
registers that match lazily's lazy reactive graph + memo-equality invariants.
Do **NOT** adopt the YRS/YJS document model (wrong replication unit). Keep the
IPC Snapshot/Delta path as the **single-producer mirror**; CRDT merge lives
only at the **multi-writer distributed boundary**. Optional later: borrow YRS's
binary anti-entropy sync engine purely as a *transport*, never its data model.

## What already exists (build on, do not rebuild)

**The local merge substrate is already built — do not re-implement it.** A code
audit (2026-06-25) found `crdt.rs` already ships the registers and the
`CellCrdt`/`ReplicatedCell` layer; the gap is purely the *network plane* on top.

- **`crdt.rs`** —
  - `MergeMechanism` (`crdt` implemented; `Lww`/`Custom` reserved),
    `HlcStamp { wall_time, logical, peer }` (public fields, `Copy + Ord`, total
    order `(wall, logical, peer)`), and `Hlc::{send, recv}`.
  - **`CellCrdt` trait** (`merge_from` commutative/associative/idempotent +
    `value`) and **`ReplicatedCell<C>`** (binds a `CellCrdt` to a reactive cell;
    `merge_remote`, `update`).
  - **Registers already implemented:** `LwwRegister<T>`, `MvRegister<T>` (keeps
    concurrent writes), `PnCounter`. So plan Phases "VV" and "registers" are
    **DONE** — Phase 2 below is reduced to *exposing* what is needed.
  - **`VersionVector(BTreeMap<u64,u64>)`** — a Lamport **counter** vector, today
    `pub` but with **private** `get/bump/dominates` used only inside
    `MvRegister`. It is counter-based, NOT `HlcStamp`-based, and has no public
    `frontier`/`min`. The plane's GC frontier needs an **`HlcStamp` frontier**
    (tombstones are stamped with `HlcStamp`), which this type does not provide —
    see Phase 1.
- **`seq_crdt.rs` / `text_crdt.rs`** — `SeqCrdt` (move-aware sibling order) and
  `TextCrdt` (RGA free text), each owning a per-replica `Hlc`, a `merge`, and
  the **GC mechanism** already shipped (`gc(watermark: HlcStamp)` /
  `gc_with(is_stable)`), awaiting an all-replicas-aware watermark *source*.
- **`distributed.rs`** — `PeerPermissions` (pure per-peer read/write/effect
  allowlist; default-deny). No transport, no anti-entropy, no peer state.
- **`ipc.rs`** — single-producer Snapshot/Delta mirror, epoch contract,
  permission-filtered projection, and (new, `#lzwirekey`) `NodeKey` wire-stable
  keyed addressing + `KeyIndex`.
- **No multi-peer coordination exists yet** — grep finds no anti-entropy, op
  log, peer-state, or sync code; only forward-referencing comments in
  `seq_crdt.rs`/`text_crdt.rs` pointing at this plane.

The plane is the missing **multi-writer coordination layer**: an `HlcStamp`
stability frontier, delta exchange/merge across ≥2 writers driving the existing
registers + Seq/Text CRDTs, and the watermark that drives tombstone GC.

## Design

A `CrdtPlane` owns, per shared session:

1. **Replica identity + clock.** One `PeerId` + one `Hlc`. Every locally
   originated op is stamped via `Hlc::send`; every observed remote stamp is fed
   to `Hlc::recv` so the local clock dominates the causal past.
2. **Version vector.** `VersionVector = HashMap<PeerId, HlcStamp>` — the highest
   stamp this replica has *durably observed* from each peer (including itself).
   Advanced on every applied local or remote op.
3. **Register kinds at the multi-writer boundary.** Per-cell registers chosen by
   the cell's declared `merge`:
   - **LWW register** — value + winning `HlcStamp`; merge keeps the higher
     stamp (total order breaks ties deterministically by peer).
   - **MV (multi-value) register** — concurrent writes (neither stamp dominates
     in the version vector) are *both* retained until a causally-later write
     supersedes them; surfaces as a set the app resolves.
   - **PN counter** — per-peer increment/decrement maps; merge is per-peer max
     of each direction; value is the summed difference.
   All three are commutative/associative/idempotent and reuse `HlcStamp`
   ordering; they sit *above* `SeqCrdt`/`TextCrdt` (which own sequence/text
   convergence) and *beside* the existing `merge: crdt` per-cell value merge.
4. **Anti-entropy exchange.** A pairwise protocol: a peer advertises its version
   vector; the responder ships every op the requester is missing (stamp not
   `<=` the requester's vector entry for that op's origin peer). Pluggable
   transport (reuse the `IpcSink`/`IpcSource` seam and the `webrtc` / `str0m` /
   `ws` backends already present). Bounded, idempotent, resumable.
5. **Causal-stability frontier (drives GC — unblocks `#r86a`/`#lztombgcwire`).**
   `frontier = min over all known peers of vv[peer]` (the all-replicas-aware
   watermark). A delete whose stamp `<= frontier` is collectable on **every**
   replica. The plane periodically calls `SeqCrdt::gc(frontier)` /
   `TextCrdt::gc_with(|s| s <= frontier)`. A single replica's local clock is
   explicitly **not** a sound watermark — only the version-vector minimum is.
6. **Single-producer mirror stays untouched.** When a session has `< 2` live
   writers, the plane is inert and the IPC Snapshot/Delta mirror behaves exactly
   as today (no merge, zero tombstone accrual). The plane is gated behind the
   opt-in `distributed` feature.

## Phases (each = one concrete backlog ID, drained in its own cycle)

Registers + `ReplicatedCell` + `CellCrdt` already exist, so the phases below are
the *plane* — peer state, exchange, GC wiring — not the local merge primitives.

1. **`#lzcrdtplane1` — `HlcStamp` stability frontier.** Add the per-peer
   `HlcStamp` frontier the GC needs (distinct from the existing counter
   `VersionVector`, which `MvRegister` uses and which has no public min). Either
   a new `StampFrontier(BTreeMap<PeerId, HlcStamp>)` with `observe`/`merge`/
   `frontier(min over membership)`/`dominates`, or a public `HlcStamp` facet on
   the existing type. A `CrdtPlane` skeleton owns `PeerId` + `Hlc` + the
   frontier; `tick`/`observe_remote` advance both. Unit tests: per-peer max,
   `min`=frontier, `None` until every expected peer is seen. No transport yet.
   *(A first cut of this was drafted and withdrawn this cycle once the audit
   showed the name `VersionVector` is already taken — land it as a distinct
   type to avoid the public-API collision.)*
2. **`#lzcrdtplane2` — Expose registers via `MergeMechanism`.** The register
   types exist; promote `MergeMechanism::Lww` from reserved → implemented and
   wire `LwwRegister`/`MvRegister`/`PnCounter` to the `merge:` selector + a
   `ReplicatedCell` constructor per mechanism. Property tests
   (commutative/associative/idempotent) via the regression property harness if
   not already covered.
3. **`#lzcrdtplane3` — Op log + delta exchange.** A causal op log keyed by
   `(PeerId, HlcStamp)`; `missing_since(frontier)` and `apply_remote(ops)` with
   idempotent re-apply, driving the registers + `SeqCrdt`/`TextCrdt`. Localhost
   two-replica integration test driving convergence through the existing
   `IpcSink`/`IpcSource` / `webrtc` / `ws` transport seam.
4. **`#lzcrdtplane4` — Frontier-driven GC (closes `#r86a`/`#lztombgcwire`).**
   Compute `frontier = min(stamp-frontier over membership)`, drive periodic
   `SeqCrdt::gc` / `TextCrdt::gc_with` from it. Test: two replicas, a delete
   becomes collectable only once *both* have observed it (not before).
5. **`#lzcrdtplane5` — Plane ↔ runtime wiring + SPEC/PROTOCOL.** Connect the
   plane to `merge: crdt` root cells and (where available) the `NodeKey`
   producer projection from `#lzwirekey`; document the §Distributed wire
   additions, the stamp-frontier exchange, and the watermark/GC contract; add a
   Lean/spec obligation for the convergence + stability invariants.

## Non-goals / open questions

- Not adopting YRS/YJS data model; YRS sync engine is an *optional later*
  transport, not a dependency now.
- Byzantine peers / auth beyond the existing `PeerPermissions` allowlist are out
  of scope.
- Wall-clock source injection: `Hlc::{send,recv}` take `now_micros`; the plane
  must thread a monotonic-ish clock without assuming synchronized wall clocks
  (HLC tolerates skew; the version-vector frontier, not wall time, is the GC
  authority).
- Multi-producer `NodeKey` uniqueness (last-writer rule) is owned here, not by
  the wire protocol (`#lzwirekey` left it to the plane).
