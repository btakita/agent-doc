# Plan: document-op replication so the canonical is never frozen (`#docop-plane`)

## Problem

`live_editors == 0` phantom lease: an editor is connected with the doc open, but its
CRDT replica registration lapsed (controller recycle, editor restart, socket/FFI
reconnect gap, hub rebuilt-from-projection with zero members). Two failures follow:

1. **Detection** — decision sites read `hub.live_count()` (registered replicas) or the
   fragile `observe_editor_open` → open-docs registry → plugin-owner **lease file**
   cold-miss chain, so a connected editor reads as headless → disk/baseline authority.
2. **Content (root)** — even when authority is "kept" for the editor, the resolve path
   serves the **relay canonical**, which is frozen-stale *because no replica fed it*.
   The operator's edits live only in the passive live-buffer sidecar the relay never
   reconciled. This is how a deleted queue head (`#sy71`) resurrected.

Detection is a band-aid. The root fix is that the connected plugin **continuously,
durably replicates its document ops into the canonical**, so the canonical is never
frozen and "editor authority" always serves live operator content. Then `live_editors`
is irrelevant to content, and the sidecar + lease chain become redundant (removable).

## The stack already exists (reuse, don't invent)

- `lazily::SqliteOutbox` (in `agent-doc-sqlite`) — generic per-`document_hash` durable
  outbox: append-before-send, retain-until-acked, ack-cursor prune. **Not liveness-specific.**
- `lazily::SyncDriver` — drains append-before-send, **retain-and-stall on transport
  failure (backpressure)**, replays un-acked on reconnect, requests snapshot on inbound gap.
- `lazily::ResyncCoordinator` — receiver: `ingest_delta`/`ingest_snapshot` → `ResyncAction`
  (detects gaps, requests snapshot), idempotent redelivery.
- `lazily::IpcPayload = Vec<u8>` — frames carry **opaque bytes**, so a yrs `CrdtDoc`
  update rides a `Delta`/`Snapshot` frame directly.
- The **liveness plane** (`agent-doc-reliable-sync-io`: `ControllerLivenessPlane`,
  `LivenessPushEndpoint`, `plane.rs`/`push.rs`) is the working template — it already
  shipped shadow-dual-run → parity oracle → authority flip. **Mirror its rollout exactly.**

The gap: the document-op path (`crdt-relay-io`: `register_replica_for_file` /
`apply_cpc_write_for_file` / `pull_replica_updates_for_file`) is bespoke and **non-durable**.
That is the only thing to replace.

## "Backpressure/retry instead of cold-miss, remove the lease chain"

Both fall out of this: once document ops flow over `SyncDriver`+`SqliteOutbox`, the durable
outbox **is** the backpressure (retain-and-stall) and retry (replay-on-reconnect / resync).
The canonical rehydrates from the durable outbox on recycle (same CRDT lineage), so the
divergent plugin-owner **lease sidecar** is no longer a needed backstop and is removed.
A cold reader (short-lived CLI) resyncs/replays the durable outbox rather than reading a
lease file — one lineage, no second source of truth. (Read-availability caveat: a cold
reader still needs a bounded answer — it comes from replaying the durable outbox, not a
blocking controller round-trip.)

## Phased rollout (conviction gate after each; SimWorld parity, no big-bang)

- **P0 (landed, green):** `observe_editor_open` is plane-primary — the `live_editors==0`
  *decision* rides the lazily OR-set liveness plane, lease chain demoted to cold-miss.
  (`agent-doc-document-realtime-io`, `make check` green.)
- **P1 — pure document-op channel + parity oracle (spike, shadow only):** yrs-update ↔
  `Delta`/`Snapshot` (opaque payload) adapter; push endpoint on `SqliteOutbox`+`SyncDriver`;
  controller ingest via `ResyncCoordinator` folding into the canonical `CrdtDoc`. SimWorld
  parity test: reconnect-gap + recycle → canonical converges to editor ops with zero loss.
  **No authority flip.** GATE: parity oracle green vs the current relay path.
- **P2 — dual-run shadow in the controller:** run the doc-op plane beside the bespoke
  relay path; log divergence live (like liveness `dual_run`). GATE: zero divergence in
  dogfooding for N sessions.
- **P3 — authority flip:** canonical is fed by the doc-op plane; `apply_cpc_write`/pull
  become plane ops. `live_editors` no longer gates content. GATE: resurrection repro
  (`#sy71`-class) cannot reproduce.
- **P4 — retire sidecar + lease chain:** remove the plugin-owner lease cold-miss and the
  live-buffer sidecar reconcile from the hot path (durability only). GATE: `audit-docs` +
  full `make check` + tmux CI green.

## Plugin work (P1→P3)

JetBrains (Kotlin) + VS Code (TS) push document deltas through the reliable-sync RPC
(append-to-outbox before send), same envelope as liveness frames
(`{"type":"reliable_sync",...,"frame":"<base64>"}`). Thin: the durability/backpressure
logic stays in the shared FFI/Rust (Shared Foundation), plugins are event reporters.

## Guardrails (per operator refactor-spiral preference)

Spike P1 pure+tested before any controller/plugin wiring. Each phase is independently
shippable and reversible. Do NOT flip authority until the parity oracle + dual-run prove
zero divergence. Keep `SidecarOpenSetModel` as the parity oracle through P3.

## Completion evidence (2026-07-13)

- P1–P3 are implemented: the file-scoped document-op channel uses lazily 0.38's durable
  storage-independent outbox, the controller folds retained `TextOp` frames into the
  canonical, and zero registered relay members no longer demotes that canonical to disk.
- JetBrains and VS Code publish incremental deltas from a tracked frontier and advance it
  only after durable delivery. Genuine reattach recovery sends one bounded visible-text
  adopt; the full operation-log plugin path is removed and its old native ABI is retained
  only as a compatibility no-op.
- Feedback regressions cover same-text self-echo and twenty tombstone-churning cycles;
  restart/outbox replay, controller parity, and plugin transport paths are covered by the
  workspace suites.
- Verification passed: `make check`, `make tmux-ci`, 148 VS Code tests, JetBrains
  `test buildPlugin`, and `make install-full` for agent-doc 0.34.87 / plugin 0.2.241.
