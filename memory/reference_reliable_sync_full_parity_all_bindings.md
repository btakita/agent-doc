---
name: reference_reliable_sync_full_parity_all_bindings
description: "Reliable Sync (#lzsync +"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 4d612b67-421c-4a53-a225-d00061965199
---

**Reliable Sync (`#lzsync` + `#sync-driver`) = FULL PARITY across all 8 lazily bindings** as of
2026-07-12. Both spec coverage rows ("resync coordinator + at-least-once durable outbox + OR-set/LWW
liveness" and "transport seam + full-duplex SyncDriver loop") are ✅ for Rust/Python/Kotlin/JS/Dart/Zig/Go/C++.

The four remaining ports (py/dart/zig/go) were done this session, mirroring the canonical Rust impl
`src/lazily-rs/src/reliable_sync.rs`. **API surface** (same in every binding): `ResyncCoordinator`
(receiver decision fn: Apply/RequestSnapshot/Ignore, multi-epoch-span aware, single-request-per-gap
suppression via a `resyncing` flag), `DurableOutbox` interface + `InMemoryOutbox` (append-before-send,
`ack_through` retention/prune, `replay_from(cursor)`, `retained_epochs`), `OrSet` (add-wins observed-remove,
commutative join), `WireLwwRegister<V>` (stamp-max LWW), `SyncDriver` composing them over
`IpcSink`/`IpcSource`/`Clock`/`SnapshotProvider` seams (tick loop order: replay-pending → drain
append-before-send/retain-on-fail → receive/route → advertise ack; sink failure = retain+stall not error,
source recv error = DriverError::Source), `Progress`, `DriverError`. Two new `IpcMessage` control frames
`ResyncRequest{from_epoch}` / `OutboxAck{through_epoch}` wire as `{"ResyncRequest":{"from_epoch":N}}` /
`{"OutboxAck":{"through_epoch":N}}`, **FFI message kinds 4 / 5** (kinds 0–3 = Snapshot/Delta/CrdtSync).

**Parity bar = 5 replayed conformance fixtures** (`lazily-spec/conformance/reliable-sync/`):
`resync_gap_converge`, `idempotent_redelivery`, `multi_epoch_delta`, `outbox_replay_after_crash`,
`liveness_orset_lww` + SyncDriver loop-shape unit tests + control-frame json round-trip. The other two
fixtures (`coalesce_bounds_outbox`, `liveness_lease_eviction`) are **model-only pins — NO reference
binding replays them**; don't add them for parity. Fixtures must be COPIED into each binding's local
conformance dir (CI has no lazily-spec sibling); each test does local-first, sibling-fallback loading.

**Released 2026-07-12:** py **0.23.0** (PyPI + GH; twine w/ `btak/PYPI_PUBLISH_TOKEN`), go **v0.8.0**
(GH release + proxy-warm), zig **v0.17.0** (GH release; version in `build.zig.zon` `.version` line —
NOT `.minimum_zig_version`), dart **v0.12.0** (GH release only, matching prior dart convention; pub.dev
publish is operator OAuth, not part of the established flow). spec `coverage.json` flipped + `sync-coverage.mjs`
regenerated all READMEs (rs/kt/js/cpp/formal README refresh left uncommitted per hot-file convention).
Superproject gitlinks NOT bumped (concurrent-session hazard). See [[reference_lazily_release_targets]].
