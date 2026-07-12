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
- **Wire codec is already solved — use `msgpack` (decided 2026-07-11).** lazily has three
  negotiable frame codecs (`ipc.rs` `IpcCodec`): `json` (canonical/FFI, blob bytes as int arrays),
  `msgpack` (`rmp-serde` `to_vec_named`, self-describing, evolution-safe), `postcard` (positional,
  Rust-only fast path). For the cross-language plugin(kt/js)⇄controller(rs) boundary the reliable
  sync stream negotiates **`msgpack`**, not JSON: it is the only *portable* binary codec (postcard
  is Rust-affine, capnproto/protobuf would force an IDL+codegen toolchain into 8 bindings incl.
  weak zig/dart support, and payloads are already opaque bytes so their typed-field edge is wasted).
  msgpack is *less* byte-compact than postcard but that is the price of portability; vs the JSON
  status quo it is a net win (no int-array blob bloat). Formalized in
  `lazily-spec/protocol.md` § Frame codecs + the conformance matrix (`json` **and** `msgpack` MUST
  round-trip all three `IpcMessage` variants); pinned in `lazily-rs/tests/ipc.rs` `mod msgpack`.
  Conformance for the map codecs is **semantic round-trip, not byte-identical** (msgpack map key
  order is encoder-defined) — only `postcard` is byte-canonical.

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
| Frame codec (which serialization) | **lazily** (`msgpack` cross-lang default; `json` canonical/FFI) | protocol; portable + evolution-safe (see grounding fact) |
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

### Phase 0 — lazily-spec (#lzsync-spec) — FIRST — DONE (2026-07-11, lazily-spec f18d7e9)
Specified the reliable-sync protocol before any code (`make check` green: 110 schema tests +
lean build + coverage-check). Landed:
- `protocol.md` § **Reliable Sync (`#lzsync`)**: the `ResyncCoordinator` decision table
  (`Apply` / `RequestSnapshot{from}` / `Ignore`, with the resyncing single-request-per-gap
  sub-state), the `DurableOutbox` contract (append-before-send, `ack_through`,
  `replay_from(cursor)` ⇒ at-least-once + idempotent-apply = exactly-once effect), the
  `SyncDriver.tick()` loop shape (drain→append-then-send→retain-on-fail→resync-on-reconnect, with
  injected `Clock`/transport and the `webrtc`-ungate note), and the **OR-set/LWW liveness cells** on
  the CrdtSync plane (open-set membership + per-pid `alive`/lease; whole-editor-death cascade;
  derived live-doc aggregate).
- **Multi-epoch-span `Delta`** chosen and specified as `epoch ≥ base_epoch + 1` (the accepted-event
  fold; span-1 is the ordinary case, so every existing `Delta` fixture stays valid). `schemas/delta.json`
  description updated.
- Two new externally-tagged control frames **`ResyncRequest` / `OutboxAck`** (`schemas/reliable-sync.json`),
  required to round-trip through **both `json` and `msgpack`** (semantic round-trip for the map codecs).
- **Conformance fixtures** `conformance/reliable-sync/`: `multi_epoch_delta`, `resync_gap_converge`,
  `idempotent_redelivery`, `outbox_replay_after_crash`, `liveness_orset_lww` — the rs/js/kt
  cross-language pins; new `tests/test_schema_conformance.py` block validates well-formedness + the
  control-frame serde. `README.md` (Reliable Sync Conformance + Versioning) and `cell-model.md`
  (cross-process liveness as a CRDT cell) cross-refs.
- **Additive, non-breaking**: no `protocol_major_version` bump; `ReliableSync.lean` (Phase 1) is the
  named formal backstop the fixtures pin to.

Original Phase 0 checklist (all satisfied):
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
- **Codec: `msgpack` is the negotiated cross-language wire default** (decided; see grounding
  fact + `protocol.md` § Frame codecs). Any new reliable-sync frame type (resync request,
  outbox-cursor ack, liveness cell op) MUST round-trip through both `json` and `msgpack` in the
  conformance fixtures, semantic-round-trip (not byte-identical) for the map codecs. `lazily-rs`
  already pins the three existing `IpcMessage` variants across `json`/`msgpack`/`postcard`
  (`tests/ipc.rs`); extend the same coverage to the new frames.
- Version bump + `cell-model.md`/wire-schema updates.

### Phase 1 — lazily-formal (#lzsync-lean) — SECOND — DONE (2026-07-11, lazily-formal 1749fbd)
`LazilyFormal/ReliableSync.lean` (22 theorems, no `sorry`/`axiom`, `lake build` green) proves all
four required results, named exactly as the spec cites them:
- **`multi_epoch_apply_eq_fold`** — a coalesced multi-epoch-span `Delta` (N ops) produces the same
  state as its expansion into N single-op unit deltas (batch = fold); `applyDelta_advances_epoch`
  proves the atomic advance to `d.epoch`.
- **`resync_convergence`** — dropping an arbitrary delta suffix then adopting the resync `Snapshot`
  reaches the same graph as seeing every delta (gap recovery is state-equivalent, not lossy). Plus
  the `ResyncCoordinator` decision table (`ingest_apply_on_contiguous` / `ingest_ignore_on_redelivery`
  / `ingest_request_on_gap`) and `step_redelivery_noop`.
- **`outbox_at_least_once_exactly_once_effect`** — replaying already-applied frames before the new
  frames reaches the identical `(state, last)` as delivering only the new frames once (no op lost,
  none doubled) — the at-least-once + idempotent-apply ⇒ exactly-once-effect guarantee.
- **`crdt_liveness_convergence_under_retry`** — the OR-set/LWW liveness join is a semilattice
  (`joinReg_{comm,assoc,idem}`, `joinOR_{comm,assoc,idem}`), so out-of-order and re-delivered
  liveness ops converge and a retry is a no-op; `orset_add_wins_over_stale_remove`.
README proof-group + compute-layer-table row added.

Original Phase 1 checklist (all satisfied):
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

### Phase 2 — lazily-rs → lazily-kt → lazily-js (#lzsync-impl) — THIRD — rs DONE (lazily-rs 3200cb3), kt/js REMAINING
**lazily-rs reference (DONE, 3200cb3; `#lzsync` targets green, `make check` green except a
pre-existing unrelated `benchmark-check` missing-p50/p95-rows baseline):**
- `src/reliable_sync.rs`: `ResyncCoordinator` (Apply/RequestSnapshot/Ignore, multi-epoch-span aware,
  single-request-per-gap `resyncing` state), `DurableOutbox` trait + `InMemoryOutbox`
  (append-before-send, `ack_through` retention, `replay_from` cursor), `OrSet` (observed-remove,
  add-wins) + `WireLwwRegister` (WireStamp-keyed LWW) liveness cells.
- **Control frames as `IpcMessage` variants** (not a side channel): `ResyncRequest` / `OutboxAck`
  added to the `IpcMessage` enum + FFI message kinds 4/5; updated the exhaustive matches (transport
  spill, webrtc filter, bridge authorize) + all 3 FFI kind fns. `Delta::span()` + multi-epoch docs.
  Decision + rationale mirrored back into **lazily-spec** (be91e0f: protocol.md IpcMessage
  enumeration + FFI kind table; the C enum was also stale, now CrdtSync=3/ResyncRequest=4/OutboxAck=5).
- `tests/reliable_sync_conformance.rs` replays all 5 `conformance/reliable-sync/` fixtures + a
  reference `FileOutbox` crash-replay helper + the `ResyncRequest`/`OutboxAck` json+msgpack codec
  round-trip; new `make test-reliable-sync-conformance` wired into `check`. Inline unit tests.
- **`SyncDriver` loop skeleton — DONE (lazily-rs, behind `feature="ipc"`).** `reliable_sync.rs`
  adds the full-duplex `SyncDriver<S: IpcSink, R: IpcSource, O: DurableOutbox, C: Clock, P:
  SnapshotProvider>` implementing the spec loop shape in one `tick()`:
  drain (append-before-send, retain-and-stall on sink failure) → resync-on-reconnect
  (`on_reconnect()` replays the unacked outbox suffix from the peer ack cursor + re-advertises our
  receiver cursor) → receive (route `OutboxAck` → `ack_through`; `ResyncRequest` → answer with a
  `SnapshotProvider` snapshot; feed `Snapshot`/`Delta` through `ResyncCoordinator` → `Apply`/emit
  `ResyncRequest`/`Ignore`; `CrdtSync` handed straight to the host). Injected `Clock` + transport +
  outbox; no threads/runtime in core (cadence/backoff are host policy). `Progress`/`DriverError`
  exported. **8 SimWorld-style deterministic tests** (scripted sink/source doubles, no real socket):
  drain+ack-prune, retain-on-fail+reconnect-replay, apply+advertise-cursor, idempotent redelivery,
  inbound-gap→request, answer-request→provider-snapshot, source-read-error, gap→snapshot converge.
  `cargo clippy --all-targets --all-features -D warnings` + the `ipc,ipc-msgpack` reliable_sync lib
  and conformance targets green. **It needs only the `ipc` feature — no `webrtc`** (it composes
  `IpcSink`/`IpcSource`, not `BridgeHub`), which confirms the plan insight that the pull/driver path
  does not need `BridgeHub` fan-out.
- **`webrtc`-ungate of `BridgeHub`/`ipc` — deliberately NOT done (not needed by the pull path).**
  The reliable-sync driver + coordinator + outbox all live behind `feature="ipc"` and never touch
  `BridgeHub` (the `webrtc`-gated fan-out hub). Since Phase 3A's UDS carrier and 3B's pull consumer
  ride `IpcSink`/`IpcSource` directly through the `SyncDriver`, the `webrtc`-ungate is unnecessary
  scope; revisit only if a genuine multi-peer fan-out requirement for `BridgeHub` appears.
- **Pure-protocol coordinator/outbox/liveness** — what kt/js mirror — remains complete.

**lazily-kt port (DONE, 67c980f; `./gradlew test` green):** `ReliableSync.kt`
(`ResyncCoordinator`/`DurableOutbox`+`InMemoryOutbox`/`OrSet`/`WireLwwRegister`), `Ipc.kt`
`ResyncRequest`/`OutboxAck` data classes + `IpcMessage` variants + `Delta.span()`, FFI kinds 4/5,
exhaustive `when`-matches updated (WebRtcTransport/Transport/IpcConformanceTest);
`ReliableSyncConformanceTest.kt` (7 tests) replays all 5 fixtures + a reference `FileOutbox`.

**lazily-js port (DONE, 81116e5; `npm run build` + `npm test` green, 336 tests):** `index.js`
`ResyncAction`/`ResyncCoordinator`/`InMemoryOutbox`/`OrSet`/`WireLwwRegister`/`wireStampGreater` +
`ResyncRequest`/`OutboxAck` classes + `IpcMessage` variants + `Delta.span()` + FFI kinds 4/5;
`ffi.js`/`distributed.js` dispatch handle the control frames; `index.d.ts` declarations;
`test/reliable-sync.test.js` (7 tests) replays all 5 fixtures + a reference `FileOutbox`. **This port
surfaced two more spec drifts, both closed:** the `IpcMessage` enumeration + FFI kind table
(be91e0f) and the `schemas/ffi.json` `LazilyFfiMessageKind` enum → `[0..5]` (680a1df).

**REMAINING (Phase 2 tracking only):** the shared `lazily-spec/coverage.json` "Reliable sync" row
(rs/kt/js ✅; py/dart/zig/go/cpp —) + `make coverage-sync`/`coverage-check`. Deferred to a dedicated
careful pass because `coverage.json` is a hot multi-session file that fans out to every sibling
binding README (one-commit-per-binding discipline; avoid clobbering concurrent flips). The rs
`SyncDriver` loop skeleton + `webrtc`-ungate are the transport-wiring slice folded into Phase 3A.

Original Phase 2 checklist:
- `ResyncCoordinator`, `DurableOutbox` (trait + `InMemoryOutbox` + reference file impl),
  `SyncDriver` skeleton, OR-set/LWW liveness cell.
- Multi-epoch-span `Delta` support; ungate `BridgeHub`/`ipc` from `webrtc`.
- A generic byte-transport `IpcSink`/`IpcSource` pairing usable over a caller-supplied stream
  (so agent-doc can wrap its UDS) — the lazily side is transport-agnostic; the UDS glue is
  agent-doc's (Phase 3A).
- Port order rs → kt → js; each passes the Phase-0 conformance fixtures. Per-binding coverage
  rows; `make coverage-check`. (Respect the STOP-releases-for-**agent-doc** directive — this is
  lazily; follow lazily's own release conventions, GH Packages/npm/crates as usual.)

### Phase 3 — S6 agent-doc integration (#s6-integration) — LAST — IN PROGRESS
Three workstreams, gated behind a dual-run flag so the sidecars stay authoritative until parity
is proven.

**Prerequisite DONE — lazily bump 0.29.0 → 0.31.0 across agent-doc (this pass).** agent-doc pinned
lazily `0.29.0`, but the reliable-sync surface (`DurableOutbox`/`ResyncCoordinator`/`OrSet`/
`WireLwwRegister`/`Delta`/`Snapshot` + the `ResyncRequest`/`OutboxAck` `IpcMessage` variants) ships
in `0.31.0`. Bumped all 12 lazily-consuming crates 0.29.0 → 0.31.0; 0.29→0.30 (`#lzfamilysync`) and
0.30→0.31 (reliable sync) are both additive feature releases, so the workspace `cargo check
--workspace` stays green (non-breaking, mirrors the earlier 0.21→0.29 milestone).

**lazily 0.32.0 RELEASED + re-bump DONE (this pass).** `SyncDriver` was a later lazily HEAD commit
(unreleased at 0.31.0); Phase 3C's push loop needs it, so **lazily-rs `v0.32.0` was tagged, pushed,
and published to crates.io** (additive over 0.31.0; `--all-features` clippy clean, reliable-sync +
ipc tests green). All 14 agent-doc lazily-consuming crates re-bumped `0.31.0 → 0.32.0` (uniform —
cargo `^0.31.0` excludes 0.32.0, so a partial bump would split lazily; `cargo check --workspace`
green). Companion **lazily-spec** landed the first-class `IpcSink`/`IpcSource` **transport-seam
contract** (protocol.md § SyncDriver: send-one / recv-poll `Ok(None)`=exhausted-or-closed /
recv-`Err`=reconnect-signal / sink-fail=retain-and-stall) + the **backpressure contract** (host
policy, not driver mechanism: unbounded enqueue + at-least-once outbox retention by design; bound via
stall/retained signals + coalescing + a spill-to-disk `DurableOutbox`) + a coverage row (rs ✅ only;
kt/js lack `SyncDriver`). Deliberately **not** formalized in lazily-formal — a sink/source trait has
no algebraic content; the invariants are already proven over frame sequences, above the transport.
(tsift's 5 lazily-consuming crates were also bumped 0.21.6 → 0.32.0, caret, separately.)

**3C store DONE (this pass) — `SqliteOutbox`.** `agent-doc-sqlite::reliable_sync_outbox::SqliteOutbox`
implements lazily's `DurableOutbox` against SQLite (new `reliable_sync_outbox` +
`reliable_sync_outbox_cursor` tables, self-owned `CREATE IF NOT EXISTS` schema — no edit to the
shared `initialize_state_db`). Per-`document_hash` channel; frames stored as serde_json of
`IpcMessage`; append-before-send / `ack_through` prune / `replay_from(cursor)` ascending suffix /
persisted ack cursor. The infallible trait methods log SQLite/serde failures loudly to stderr (no
silent swallow). **6 tests** incl. the recycle-survival invariant (drop + reopen → the un-acked
suffix and durable ack cursor are exactly preserved — the on-disk durability the sidecars gave) and
per-`document_hash` isolation. This is the durable store 3C plugs behind the `SyncDriver`; the
push-loop wiring + dual-run cutover remain.
- **3A — UDS carrier — DONE (this pass) — `agent-doc-reliable-sync-io`.** A new crate implements
  lazily's `IpcSink`/`IpcSource` over the **existing** `agent-doc-ipc-io` controller socket — no new
  socket. Each reliable-sync frame rides in one NDJSON control message
  `{"type":"reliable_sync","document_hash":"<hash>","codec":"msgpack","frame":"<base64>"}`: the frame
  body is the `IpcMessage` msgpack-encoded (the decided cross-language wire codec), base64'd into a
  JSON envelope so a mixed-codec listener still routes by `type`/`document_hash` without decoding the
  opaque frame. `encode_envelope`/`decode_envelope` (the latter returns `None` for non-reliable-sync
  messages, `Some(Err)` for a malformed frame — never a silent drop). `ReliableSyncSink<T:
  EnvelopeTransport>` is the plugin→controller push sink (a transport error surfaces as the sink error
  → the `SyncDriver` retains the frame in the outbox = at-least-once); `SocketEnvelopeTransport` is the
  real socket impl, and the sink is generic over `EnvelopeTransport` so the send path is unit-testable
  without a live socket. `reliable_sync_channel(document_hash)` returns a `ReliableSyncInbox`
  (listener pushes decoded frames) + `QueueIpcSource` (`recv` is non-blocking: `Ok(None)` when empty
  or the inbox is dropped — the `SyncDriver` polls, never blocks). **8 tests**: per-variant envelope
  round-trip (Snapshot/Delta/ResyncRequest/OutboxAck), non-reliable-sync rejection, malformed-frame
  errors, sink→decode loopback via a fake transport, sink-surfaces-failure, channel FIFO drain,
  dropped-inbox-reads-closed, deliver-after-source-dropped. `cargo test`/`clippy`/`fmt` green. The
  listener→inbox routing wiring lands with 3C (the consumer of these building blocks).
- **3B — controller→consumer stays pull, on lazily types.** Adopt lazily's `Delta`/`Snapshot`
  types + `ResyncCoordinator` in place of the bespoke `WireDelta` fold, keeping the
  `state_subscribe(last_epoch)` pull + SQLite resume (already retry-safe). Mostly a type
  unification; behavior-preserving.
- **3C — plugin→controller open-set/liveness push on the CrdtSync plane — RECEIVER CORE + PUSH LOOP
  DONE (this pass), FFI/listener/plugin-emission REMAINING.** Landed in `agent-doc-reliable-sync-io`:
  - **`liveness::LivenessProjection`** — the controller's derived-authority engine that *replaces the
    sidecar scan*. Folds a `LivenessOp` (`Open{doc,pid,tag}` / `Close{doc,pid,observed_tags}` /
    `Alive{pid,value,stamp}`) into lazily's proven convergent cells (`OrSet` open-set membership,
    add-wins; `WireLwwRegister<bool>` per-pid `alive`, highest-stamp-wins) and derives `is_open`,
    `pid_alive` (absent ⇒ presumed alive until a death signal), `open_pids`, `open_docs` (the `#lbreap`
    live-buffer-scan replacement), and `live_docs` (the derived aggregate with the whole-editor-death
    cascade for free). Keys namespaced by `document_hash` (per-doc isolation). Uses lazily's types, not
    a re-implementation, so it inherits the `ReliableSync.crdt_liveness_convergence_under_retry` /
    `orset_add_wins_over_stale_remove` / `joinReg_*` proofs. **9 tests pinned to the
    `conformance/reliable-sync/liveness_orset_lww.json` scenarios**: add-wins-over-stale-remove,
    order-independence + redelivery-noop, LWW highest-stamp, whole-editor-death cascade, shared-doc
    stays-live-via-second-pid, plus frame round-trip / foreign-frame rejection / malformed-frame error /
    decoded-frame fold.
  - **CrdtSync carriage** — `encode_liveness_frame`/`decode_liveness_frame` pack a `LivenessOp` batch
    into one `IpcMessage::CrdtSync` op as inline bytes (spec § `#lzsync-liveness`: liveness rides the
    CrdtSync plane), tagged with a sentinel node so a foreign graph `CrdtSync` is not mis-folded; so the
    batch flows through the same `SyncDriver` + `DurableOutbox` as every other frame (the driver hands an
    applied `CrdtSync` straight to the host).
  - **Full push-loop SimWorld** — a `push_loop_simworld` test wires the **real** lazily `SyncDriver`
    over this crate's `ReliableSyncSink` carrier + `InMemoryOutbox`, and proves the Phase-3C guarantee
    end to end: a liveness push sent while the controller socket is **down** is retained in the outbox
    (append-before-send), then **replayed on reconnect** so the controller's derived `open_docs`
    converges (at-least-once) — and a re-delivered frame is idempotent. SimWorld over mocks (scripted
    connected-flag transport, no real socket).
  - **`dual_run_enabled()`** (env `AGENT_DOC_RELIABLE_SYNC_DUAL_RUN`, **default OFF**) — the flag the
    controller checks so the plane runs in shadow while the sidecars stay authoritative.
  - **Controller receive wiring + dual-run parity oracle DONE (this pass).** `plane::ControllerLivenessPlane`
    (reliable-sync-io) folds inbound frames via `ingest(document_hash, epoch, &IpcMessage)` (idempotent;
    returns the per-channel ack cursor for the receipt so the plugin outbox prunes/resumes) and owns the
    derived-authority `LivenessProjection`. **Wired into the controller RPC** (`rpc.rs`): a new
    `"reliable_sync"` dispatch arm → `handle_reliable_sync` decodes the 3A envelope from
    `diagnostic_payload` (reusing `decode_envelope`, so msgpack stays inside reliable-sync-io — no new
    controller-io feature), gates on `dual_run_enabled()` (**OFF ⇒ ack 0, sidecars authoritative**), and
    on ON folds into a global `CONTROLLER_LIVENESS_PLANE` (a `LazyLock<Mutex<…>>`, mirroring the
    stateless-handler `RelayHub` pattern). **Dual-run parity SimWorld** (`plane::parity`): a
    `SidecarOpenSetModel` oracle + a `drive()` harness push every open/close/crash event through *both*
    the synced plane and the sidecar model and assert `open_docs`/`live_docs` agree **after every event**;
    plus a **recycle** test (controller loses the projection → replays the plugin's retained outbox suffix
    → rebuilds the exact derived authority) and a redelivery-idempotence test. 22 crate tests + 1 handler
    test (default-OFF path), clippy/fmt green.
  - **Plugin-push Rust core DONE (this pass) — `push::LivenessPushEndpoint`.** The editor-plugin *send*
    half, generic over `DurableOutbox` (SqliteOutbox in prod, InMemoryOutbox in tests) + an injected
    `LivenessPushTransport` (the `reliable_sync` controller RPC in prod): `enqueue` assigns the next epoch
    and durably `append`s the frame **before** any send; `flush` replays every un-acked frame through the
    transport, prunes on the returned ack cursor, and **retains-and-stalls** on a transport failure so a
    push lost while the controller is down re-sends on the next flush/reconnect. **Full plugin→controller
    push loop proven end to end** in a SimWorld whose fake transport folds pushed frames into a real
    `ControllerLivenessPlane` and returns its ack: deliver-fold-prune, retain-on-fail→replay-on-reconnect,
    recycle epoch-monotonicity (never re-use an acked epoch), idempotent redelivery. 26 crate tests;
    clippy/fmt green.
  - **FFI + RPC transport + SqliteOutbox registry DONE (this pass).** `controller-io` exposes the
    `reliable_sync` RPC **client**: `push_reliable_sync_liveness(project_root, epoch, envelope)` +
    `RpcLivenessPushTransport` (impls reliable-sync-io's `LivenessPushTransport` over `request_controller`;
    `ControllerReliableSyncResponse` made `pub`). `src/ffi.rs` exports the C-ABI entry points the editor
    plugins call: **`agent_doc_reliable_sync_liveness_enqueue(project_root, document_hash, ops_json)`**
    (parses a `LivenessOp` JSON batch, gets-or-creates a per-`(root,doc)` `LivenessPushEndpoint` backed by
    a durable `SqliteOutbox` at `.agent-doc/reliable_sync_outbox.db`, resuming the epoch counter past the
    acked cursor via `LivenessPushEndpoint::resuming` so a recycle never re-uses an epoch) and
    **`agent_doc_reliable_sync_liveness_flush(project_root, document_hash)`** (flushes via
    `RpcLivenessPushTransport`, returns the ack cursor). Global endpoint registry (`LazyLock<Mutex<HashMap>>`,
    stateless-FFI pattern). Workspace compiles; clippy/fmt green.
  - **Plugin liveness emission DONE (this pass) — design B (operator-chosen): each plugin hosts a real
    lazily liveness graph → FFI push.** The controller-side S4b exit watcher now injects `Alive{false}`
    (`record_reliable_sync_editor_exit`, wired in `process_exit_watcher.rs`). **JetBrains** (v0.2.235,
    `buildPlugin` green): `ReliableSyncLivenessGraph.kt` holds this editor's open-set as lazily-kt `OrSet`s
    (add-wins re-open, reactive `isOpen()`) and derives the externally-tagged `LivenessOp` batch;
    `ReliableSyncLivenessListener.kt` (`FileEditorManagerListener`) resolves the canonical `document_hash`
    (`agent_doc_document_id_for_path`), pushes via the two FFI entry points off the EDT; `NativeLib.kt`
    JNA decls; `plugin.xml` registration. **VSCode** (v0.2.43, `check-types` + esbuild + vsix green):
    `reliableSyncLiveness.ts` — symmetric lazily-js `OrSet` graph + `onDidOpen/CloseTextDocument` →
    `native.ts` koffi wrappers → FFI; registered in `extension.ts activate`. The FFI enqueue is gated on
    `dual_run_enabled()` (no-op by default), so both plugins are safe on every install — sidecars stay
    authoritative until the operator opts in.
  - **REMAINING (`[operator-verify]` slice + agent-doc release blocked by STOP-releases):** switch
    `editor_open_docs`/`editor_attach`/the `#6b5h` lease to READ `LivenessProjection` (the hot-path
    authority flip); the cutover (turn dual-run ON → confirm parity live in a real IDE/VSCodium →
    switch the hot path → delete the sidecar writers/reapers); and **3B** (adopt lazily `Delta`/`Snapshot`
    in the `state_subscribe` pull — changes the FFI wire the plugins parse). The live-editor eyeball
    (both editors emit + the derived open-set matches the sidecar-derived one, and a real editor crash
    cascades to not-live) is the `[operator-verify]` gate.
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
