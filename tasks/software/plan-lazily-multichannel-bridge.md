# plan: multi-channel reactive bridge hub (#lzmuxbridge)

Status: backlog / design sketch. Answers the session question "Can lazily-rs act
as a reactive bridge between multiple network channels?"

## Why

Today each transport (`InProcess`, socket-style, `WebRtcSink/Source`) attaches a
single reactive graph to one peer set. There is **no layer that fans one graph
across *heterogeneous* transports at once** (e.g. a delta arriving over WebRTC
re-emitted to WebSocket and local-IPC peers). The `IpcSink`/`IpcSource` traits
and the `PeerPermissions` boundary already make this composable; the missing
piece is a hub that owns N attachments and routes through the graph.

## Architecture decision: re-derive, do not relay raw frames

A bridge must NOT forward raw `IpcMessage` frames channel-to-channel. Each
inbound `Delta` is applied to the local graph through the existing graph-apply
write authority; outbound snapshots/deltas are then **recomputed and re-filtered
per peer** via `Snapshot/Delta::filter_readable` + `PeerPermissions`. This keeps
the default-deny / omission-not-redaction invariant and CRDT correctness intact
across a heterogeneous fan-out — a WebRTC peer never leaks state to a WS peer
outside its allowlist.

## Phases

- **Phase 1 — `BridgeHub` skeleton.** A struct owning `Vec<Attachment>` where
  `Attachment = { peer: PeerId, permissions view, sink: Box<dyn IpcSink>, source:
  Box<dyn IpcSource> }`. `poll()` drains every source, applies inbound `Write`
  deltas through the graph-apply authority, and re-emits per-peer filtered deltas
  to every other sink. Deterministic in-memory test with 3 `InMemoryDataChannel`
  attachments (WebRTC-loopback + two plain).
- **Phase 2 — heterogeneous transport mix.** Prove one hub instance serving a
  WebRTC attachment + a socket attachment + an in-process attachment in a single
  loopback integration test. No new transport code — reuse existing impls.
- **Phase 3 — backpressure + liveness.** Drop/close handling when an attachment's
  `is_open()` flips false; bound per-attachment outbound queues.

## Out of scope / dependencies

- WebSocket transport is its own backlog item (#lzwstransport) — the hub is
  transport-agnostic and does not need it to land first.
- Map Holons integration is a schema-adapter problem (holon node/edge model ↔
  `NodeId`/`Snapshot`/`Delta`), not a transport problem; it consumes this hub but
  is tracked separately.
