# Plan — retire the filesystem sidecars onto lazily reliable sync (#sidecar-retirement / #lzsync)

The concrete implementation of **S6** from
[`plan-live-editor-reactive-backbone.md`](plan-live-editor-reactive-backbone.md) and the
"eliminate the durable filesystem sidecars entirely" north star. S6 was previously
rejected on premises that S5 (2026-07-11) invalidated; this plan supersedes that decision.

## Goal

Replace the cross-process filesystem **sidecars** — the plugin-owner lease and the
`.agent-doc/live-buffer/` snapshots — with **lazily reliable cross-process sync**, so
editor liveness / open-set / ownership crosses the plugin⇄controller boundary as an
epoch/frontier-cursored reactive stream instead of a disk file read as authority.

**Why:** the sidecar race class is *a disk file written by process A (editor) and read as
authority by process B (controller) with no atomic epoch handshake*. Every failure mode —
**stale** (dead-pid lease still on disk), **phantom** (sidecar from a crashed/duplicate
editor), **divergent** (live-buffer edit-epoch behind the real buffer) — lets out-of-band
disk state drive a hot-path merge/commit. Enumerated in
[`plan-sidecar-authority-hot-path.md`](plan-sidecar-authority-hot-path.md) §"The trap":
frontmatter-drop commit spin (`#fmdrop`), stale-`already_applied` scramble
(`#stale-already-applied`), stuck `response_captured`, live-buffer divergence misreport. An
epoch/frontier handshake makes a stale/phantom write *just another idempotent event the
dedup absorbs*, not a file that overwrites authority.

## Grounding facts (verified 2026-07-11)

### lazily has the right *types*, not the right *reliability*
- `IpcMessage = { Snapshot(Snapshot) | Delta(Delta) | CrdtSync(CrdtSync) }`
  (`lazily-rs/src/ipc.rs:1357`). `Delta = { base_epoch, epoch, ops: Vec<DeltaOp> }` —
  "coalesced operations for this flush" (`ipc.rs:1072`); `DeltaOp` = CellSet / SlotValue /
  Invalidate / NodeAdd / NodeRemove / EdgeAdd / EdgeRemove (`ipc.rs:772`). **So a lazily
  delta is already an epoch-batched delta set** — the plan-live-editor "single-step" premise
  was wrong.
- **Reliability gap.** `BridgeHub::poll` `?`-propagates any send error and unwinds
  (`bridge.rs:179`); no outbox, ack, retry, or resend anywhere in bridge/ipc/transport. The
  out-epoch is bumped *before* the send (`bridge.rs:178`→`:179`), so a failed send leaves a
  **permanent gap**. Reconnect = a fresh peer at `out_epoch:0`, no backfill, no resume cursor.
- **Resync is a dead signal.** `Delta::apply_status → ResyncRequired` (`ipc.rs:1117`) says
  "discard, request a fresh Snapshot" but has **zero production callers** (only tests). The
  gap-detect→request-snapshot→apply loop is unimplemented; every consumer would hand-roll it.
- **CrdtSync plane is idempotent + resumable** ("safe to resend", frontier-based;
  `crdt_plane.rs:434`) — but one-shot/pull: `sync_frame_since(frontier)` / `sync_reply(request)`
  are methods you call; **no periodic driver** re-sends until a peer catches up.
- **Transport-agnostic.** `IpcSink`/`IpcSource` are abstract traits under `feature="ipc"`
  (`ipc.rs:1774`); the byte carrier is the `DataChannel` trait (`webrtc_transport.rs:40`,
  send_frame/try_recv_frame/is_open). In-memory + WebSocket impls exist; **no Unix-domain-socket
  impl**. `BridgeHub` is `#[cfg(feature="webrtc")]`-gated (`lib.rs:59`) despite depending only
  on the abstract `ipc` traits.
- **No per-document channel.** One `BridgeHub` = one graph, N peers; no topic/doc field on any
  message. Per-doc isolation today is agent-doc's own `document_hash` tag on every frame +
  `slot_id = fnv1a(document_hash, type_tag, entity_key)` namespacing.

### agent-doc already has the retry-safe half
- Wire: `WireDelta { base_epoch, epoch, document_hash, ops }`
  (`agent-doc-state-wire/src/lib.rs:196`); a delta **may span multiple epochs**
  (`epoch > base_epoch+1`, `:189`). Epoch = **per-document count of accepted (deduped) events**
  (`agent-doc-state-backbone/src/lib.rs:733`); a re-emit does not bump it (idempotent ⇒ no-op
  delta). `build_delta` diffs two whole projections (`state-wire:503`), not an op log.
- Transport today: an **idempotent pull** — `agent_doc_state_subscribe(document_hash,
  last_epoch)` (`ffi.rs:2389`) / RPC `state_subscribe` (`rpc.rs:6924`) returns snapshot
  (`last_epoch==0`) or delta, over the controller's existing Unix socket. Re-requesting the
  same `last_epoch` returns the same result → **retry = re-poll from your cursor**, inherently
  at-least-once + idempotent. Durable: the `EventLedger` is replayed from the SQLite state DB
  on controller construction (`project_controller.rs:360`,`:1401`), so epoch/projection survive
  a recycle.
- Sidecars to retire:
  - **plugin-owner lease** `{ consumer_id, pid, heartbeat_secs }`
    (`agent-doc-plugin-owner/src/lib.rs:55`) — editor writes, controller reads for the `#6b5h`
    disk-write guard (pid-liveness).
  - **live-buffer** `LiveBufferSnapshot { path, len, hash, edit_epoch, state_vector_b64,
    editor_id, content, no_unsaved_operator_edits, … }` (`agent-doc-debounce/src/lib.rs:244`) —
    editor writes per keystroke-batch; controller reads for divergence **and the open-set**.
    The `#lbreap` live-buffer files are the **open-set ground truth** the reactive
    `editor_open_docs`/`editor_attach` registries reconcile from (`editor_open_docs.rs:18`;
    `ffi.rs:596` marks closed when `live_buffer_snapshots(path).is_empty()`).

## Design principle — mechanism in lazily, policy injected

The correctness-critical, **cross-language-identical** protocol belongs in lazily (ported +
conformance-pinned rs/kt/js, the same discipline S5 used for `StateGraphMirror`). The
**environment-specific** choices are injected behind traits so the reactive core stays lean
and portable.

| Concern | Home | Rationale |
|---|---|---|
| Idempotent apply, epoch/frontier reconciliation, dedup | **lazily** (exists) | subtle, identical everywhere |
| Gap-detect → request-snapshot → apply (**resync**) | **lazily** (new `ResyncCoordinator`) | pure protocol; currently a dead signal |
| Reliable-sync loop **shape** (drain→send→mark→resync-on-reconnect) | **lazily** (new `SyncDriver`, feature-gated) | prevents N buggy hand-rolls |
| At-least-once replay-from-cursor semantics | **lazily** (new `DurableOutbox` trait + protocol) | cursor math is protocol; storage is not |
| Concrete persistence backend | **app** (agent-doc SQLite) | storage-agnostic by design |
| Byte transport (which socket) | **app** (UDS `DataChannel`/`IpcSink`) | deployment choice |
| Retry cadence / backoff / threading / clock | **app** (injected `Clock`/scheduler) | policy, no async runtime in core |

## New lazily surface (sketch — finalized in Phase 0/2)

```
// #resync-coord — pure protocol, no I/O, no storage.
trait SnapshotProvider { fn snapshot(&self, from_epoch: u64) -> IpcMessage; } // app supplies
struct ResyncCoordinator { /* last applied epoch per source */ }
impl ResyncCoordinator {
    // Feed an inbound message; returns Apply | RequestSnapshot(from) | Ignore(dup).
    fn ingest(&mut self, msg: &IpcMessage) -> ResyncAction;
}

// #durable-outbox — interface + replay protocol; app plugs the store.
trait DurableOutbox {
    fn append(&mut self, epoch: u64, msg: &IpcMessage);   // before send
    fn ack_through(&mut self, epoch: u64);                // on delivery proof
    fn replay_from(&self, cursor: u64) -> impl Iterator<Item = (u64, IpcMessage)>;
}
// ships: InMemoryOutbox (default) + a reference file impl for tests.

// #sync-driver — loop shape; Clock + transport + outbox + coordinator injected.
struct SyncDriver<S: IpcSink, R: IpcSource, O: DurableOutbox, C: Clock> { ... }
impl SyncDriver { fn tick(&mut self) -> Result<Progress, DriverError>; } // called by app scheduler
```

Plus: **(i)** relax/extend `Delta` to carry agent-doc's multi-epoch-span batch — either allow
`epoch > base_epoch+1` on the `Delta` invariant (preferred; `apply_status` already models the
gap) or add an explicit `accepted_count`; **(ii)** ungate `BridgeHub`/`ipc` from `webrtc` so a
UDS carrier works without the feature; **(iii)** an **OR-set / LWW liveness** cell type on the
CrdtSync plane for the open-set/lease push (idempotent, frontier-resumable — the natural fit for
"editor pid X opened doc Y", tolerant of re-delivery).

## Invariants any replacement MUST preserve
- Epoch/projection **re-derivable from the durable ledger** after a controller recycle.
- Open-set + owner-lease **survive a recycle** without the editor re-announcing (today: disk;
  replacement: a durable outbox + resume-from-cursor).
- **Idempotent** — a re-emit/replay is a no-op (no epoch bump, no double-apply).
- **Per-doc isolation** — a stale-overlay for doc B cannot flip doc A's authority.
- **Conformance pins** across rs/js/kt kept as cross-language drift catches.
- Snapshots/sidecars are **backup/audit, never hot-path authority** (`AGENTS.md`); the sync
  stream, not a file, is the authority.

## Phased execution (each phase: functionality + tests together; conformance rs/js/kt)

### Phase 0 — lazily-spec (#lzsync-spec) — FIRST
Specify the reliable-sync protocol before any code:
- The `ResyncCoordinator` state machine (inbound → Apply/RequestSnapshot/Ignore) with the
  multi-epoch-span `Delta` semantics reconciled (chosen: `epoch ≥ base_epoch+1`).
- The `DurableOutbox` contract: append-before-send, ack-through, replay-from-cursor, and the
  at-least-once + idempotent-apply ⇒ exactly-once-effect guarantee.
- The `SyncDriver` loop contract (drain, send-with-outbox, on-send-fail retain, on-reconnect
  resync-from-cursor) with the injected `Clock`/transport boundary.
- The OR-set/LWW **liveness** cell semantics for the open-set/lease push.
- New conformance fixtures under `lazily-spec/conformance/` covering: a dropped-frame gap →
  resync → convergence; outbox replay after a simulated crash; idempotent re-delivery; a
  multi-epoch-span delta apply. (These become the rs/js/kt cross-language pins.)
- Version bump + `cell-model.md`/wire-schema updates.

### Phase 1 — lazily-formal (#lzsync-lean) — SECOND
Lean proofs of the protocol (mirroring the existing Materialization/AsyncMaterialization
proofs):
- **Resync convergence:** a receiver that drops an arbitrary delta suffix then applies the
  resync Snapshot reaches the *same* graph as one that saw every delta (gap recovery is
  state-equivalent, not lossy).
- **Outbox at-least-once ⇒ exactly-once effect:** replay-from-cursor delivers every op ≥ once,
  and idempotent apply makes the net effect exactly-once (no lost, no doubled).
- **CrdtSync convergence under retry:** repeated `sync_frame_since(frontier)` rounds converge
  and re-delivery is a no-op (LWW/OR-set liveness is a lattice join).
- **Multi-epoch delta apply equivalence:** applying one `Delta` with N ops equals applying N
  single-op deltas in order (batch = fold).
- `lake build` green; formal version bump.

### Phase 2 — lazily-rs → lazily-kt → lazily-js (#lzsync-impl) — THIRD
Implement to the spec, conformance-pinned across all three:
- `ResyncCoordinator`, `DurableOutbox` (trait + `InMemoryOutbox` + reference file impl),
  `SyncDriver` skeleton, OR-set/LWW liveness cell.
- Multi-epoch-span `Delta` support; ungate `BridgeHub`/`ipc` from `webrtc`.
- A generic byte-transport `IpcSink`/`IpcSource` pairing usable over a caller-supplied stream
  (so agent-doc can wrap its UDS) — the lazily side is transport-agnostic; the UDS glue is
  agent-doc's (Phase 3A).
- Port order rs → kt → js; each passes the Phase-0 conformance fixtures. Per-binding coverage
  rows; `make coverage-check`. (Respect the STOP-releases-for-**agent-doc** directive — this is
  lazily; follow lazily's own release conventions, GH Packages/npm/crates as usual.)

### Phase 3 — S6 agent-doc integration (#s6-integration) — LAST
Three workstreams, gated behind a dual-run flag so the sidecars stay authoritative until parity
is proven:
- **3A — UDS carrier.** Implement `IpcSink`/`IpcSource` (or a `DataChannel`) over the existing
  `agent-doc-ipc-io`/`ipc_socket.rs` Unix socket the controller already hosts. No new socket.
- **3B — controller→consumer stays pull, on lazily types.** Adopt lazily's `Delta`/`Snapshot`
  types + `ResyncCoordinator` in place of the bespoke `WireDelta` fold, keeping the
  `state_subscribe(last_epoch)` pull + SQLite resume (already retry-safe). Mostly a type
  unification; behavior-preserving.
- **3C — plugin→controller open-set/liveness push on the CrdtSync plane.** Model editor
  open-set + owner-lease as OR-set/LWW liveness cells; the plugin pushes via `SyncDriver` +
  `DurableOutbox` (agent-doc plugs its SQLite as the store) so a push that fails while the
  controller is down is re-sent on reconnect from the frontier. The controller derives
  `editor_open_docs`/`editor_attach`/the `#6b5h` lease decision from the synced cells instead of
  scanning `.agent-doc/live-buffer/` + `plugin-owner/*.json`.
- **Migration & cutover:** dual-run (sidecar write + sync push) → assert the synced open-set/lease
  matches the sidecar-derived one across a SimWorld of open/close/crash/recycle sequences →
  switch the hot path to read the synced cells → stop reading the sidecars → delete the sidecar
  writers (`record_live_buffer_*`, lease write) and the reapers (`#lbreap`,
  `reap_stale_jetbrains_live_buffers`). Keep one durable projection (the outbox/ledger) as the
  recycle-recovery source that replaces the sidecar's on-disk durability.

## Risks / open questions
- **Per-doc scoping on `BridgeHub`:** one hub/transport-pair per document vs a single hub with
  `document_hash`+fnv1a NodeId namespacing and per-node `PeerPermissions`. Decide in Phase 0;
  the pull path (3B) may not need `BridgeHub` at all (it is not fan-out).
- **Epoch-semantics choice** (`epoch ≥ base+1` vs explicit `accepted_count`) is a wire-compat
  decision that ripples through spec/formal/impl — settle it in Phase 0.
- **Durability equivalence:** the outbox + SQLite must give the *exact* recycle-survival the
  sidecars give today; the migration SimWorld must include a mid-push controller recycle.
- **Do not regress** the S4b reactive authority or the S5 `StateGraphMirror` conformance pins.
- Whole-editor crash detection still needs the S4b OS process-exit watcher — the liveness cell's
  "editor gone" input is the pidfd/poller exit event, not a missing file.

## Test conventions
- SimWorld over mocks for the sync/liveness/recycle sequences; deterministic model + shared pure
  decision fn; do not touch the fixed-seed fuzz.
- Cross-language conformance (rs/js/kt) is the drift catch, pinned to the Phase-0 fixtures.
- Lean proofs are the correctness backstop for the protocol; implementation must match them.
