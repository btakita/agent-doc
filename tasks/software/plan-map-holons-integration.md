# plan: Map Holons ↔ lazily-rs integration (ongoing)

**Status:** living document. Discovery phase — direct line to **Steve Melville
(creator of Map Holons)** is open, so several sections are *questions to confirm
with Steve* rather than settled design. Update this file as answers land.

**Contact:** Steve Melville (Map Holons author). Brian is in direct contact —
use it to resolve the OPEN QUESTIONS below rather than guessing the holon model.

## Goal

Let a Map Holons graph and a lazily-rs reactive graph observe and update each
other: holon changes propagate into lazily as reactive `Delta`s, and lazily-side
writes (where permitted) propagate back to holons — across WebRTC / WebSocket /
in-process IPC channels.

## What is solid on the lazily-rs side

The transport and reactive-graph plumbing already exists (see the prior session
answer + `tasks/software/plan-lazily-multichannel-bridge.md`):

- `IpcSink`/`IpcSource` (`src/ipc.rs`) — transport-agnostic seam carrying
  `IpcMessage` (`Snapshot` / `Delta`), the reactive-graph wire protocol.
- Existing transports: in-process, socket-style, WebRTC (`webrtc_transport.rs` +
  `str0m_backend.rs`). WebSocket is queued as **#akp3 / #lzwstransport**.
- `PeerPermissions` / `RemoteOp` / `OpKind` (`src/distributed.rs`) — default-deny,
  omission-not-redaction access boundary, applied at snapshot/delta construction.
- Wire identity: `NodeId(u64)` + `Snapshot` (nodes + edges) + `Delta` (incremental
  ops, epoch-versioned).
- **#97xn / #lzmuxbridge** — the multi-channel bridge hub that fans one graph
  across heterogeneous channels. The Map Holons adapter consumes this hub.

So the integration is **not** a transport problem. It is a **schema-adapter**
problem: translate the holon model into `NodeId` / `Snapshot` / `Delta` and back.

## OPEN QUESTIONS for Steve Melville

These gate the adapter design — confirm before writing adapter code:

1. **Holon identity.** What is a holon's stable identifier, and what is its
   cardinality/shape (UUID, content-hash, composite)? It must map deterministically
   to/from lazily's `NodeId(u64)` (or we widen `NodeId`).
2. **Holon ↔ node/edge mapping.** Is a holon a single node, or a node + its
   relationship edges? Are holon "relationships" directed/typed in a way that maps
   onto lazily's `EdgeSnapshot`?
3. **Change feed.** Does Map Holons expose an incremental change/event stream we
   can convert to `Delta`, or only full reads (forcing us to diff snapshots)?
4. **Write semantics & authority.** Can lazily push writes back into holons? If so,
   what is the conflict model (CRDT? single-writer? last-write-wins?) — this must
   reconcile with lazily's single-writer effect authority and CRDT cell plane.
5. **Identity/permissions.** How do holon-side agents/permissions map onto
   `PeerId` + `PeerPermissions`? We must not bypass default-deny.
6. **Transport expectations.** Which channel does Map Holons prefer first
   (WebRTC P2P, WebSocket relay, local IPC)? Drives #akp3 priority.
7. **Reference / spec.** Is there a Map Holons schema doc / repo / wire spec we can
   pin this adapter against (analogous to lazily's `PROTOCOL.md`)?

## Architecture (proposed, pending answers)

```
Map Holons graph  <--adapter-->  lazily Snapshot/Delta  <--#97xn hub-->  peers
   (holon model)   (schema map)     (NodeId/edges)         (WebRTC/WS/IPC)
```

- **`holon-adapter`** (new, likely a feature-gated module or sibling crate):
  - `holon_to_snapshot()` / `holon_change_to_delta()` — inbound.
  - `delta_to_holon_write()` — outbound, gated by `PeerPermissions` write checks.
  - Identity map: holon id ⇄ `NodeId`.
- Attaches to the **#97xn bridge hub** as one more attachment, so holon updates
  fan out to WebRTC/WS/IPC peers via the same re-derive-not-relay path that
  preserves permission filtering.

## Phases

- **Phase 0 — discovery (current).** Resolve the OPEN QUESTIONS with Steve; pin a
  Map Holons spec/repo reference into this doc.
- **Phase 1 — read-only inbound.** Holon snapshot/change → lazily `Snapshot`/`Delta`.
  Deterministic test with a recorded holon fixture; no writes back.
- **Phase 2 — outbound writes.** `Delta` → holon write, gated by `PeerPermissions`;
  reconcile with the chosen conflict model from Q4.
- **Phase 3 — bridge integration.** Wire the adapter into the #97xn hub; prove a
  holon update fanning to a WebRTC peer + a WebSocket peer in one integration test.

## Dependencies

- **#akp3 / #lzwstransport** — WebSocket transport (if Steve wants WS relay first).
- **#97xn / #lzmuxbridge** — multi-channel bridge hub (Phase 3 consumes it).

## Status log

- 2026-06-13: Plan created. Contact with Steve Melville established. Discovery
  phase open — questions above are the immediate blocker before adapter code.
