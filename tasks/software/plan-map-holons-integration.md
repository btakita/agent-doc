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

## 2026-06-17 re-scan (#lzholonsscan)

Public sources reviewed: `evomimic/map-holons` commits through `ab3239f` (2026-06-15), repo `ARCHITECTURE.md`, and the canonical [Layer 0: Holons](https://evomimic.gitbook.io/map-book/map-foundations/layer-0-holons.md) book page. Each question is re-tagged **answered-by-code / sharpened / still-needs-Steve**.

### Architectural context that did not exist at the 2026-06-13 discovery pass

- **Two execution contexts, strict boundary.** `ARCHITECTURE.md` now states the repo "mirrors runtime execution contexts": **hApp (WASM/Holochain)** and **Host (Native Rust + TS)**, each with its own workspace + `Cargo.lock`. `shared_crates/` compile to both but must obey WASM constraints (no threads, no OS, no host-only deps). lazily-rs (tokio + threads) can **only** run in the Host context.
- **Conductora.** The host-side command/integration subsystem — "command dispatch and mapping", "integration bridges (including the Holochain receptor)", "host runtime orchestration". This is the seam where a lazily adapter would attach; it is not in `shared_crates/` or `happ/`.
- **DanceV2.** A new typed command ingress + executor layer (PRs #547/#549/#545 — "DanceV2 ingress, typed dance references, and shared executor"; "Align MAP commands contract with runtime shared types"; "Align DeleteHolon DanceV2"). Commands route through DanceV2 rather than ad-hoc zome calls.
- **Inverse relationship persistence + HasInverse materialization.** Relationships now have inverse auto-materialization during "commit pass 2" — writing a relationship implicitly creates its declared inverse. Relationships are typed and have an `effective_relationship_declaration` helper.
- **CollectionType + typed schema elements.** A new concrete `HolonCollectionType` schema element with `ElementTypeFor` keys; schema JSON files are now generated by a Python script and consumed by both contexts.

### Per-question re-tag

1. **Holon identity — sharpened, still-needs-Steve.** Layer 0 confirms "each holon has a unique identifier" but does not pin the shape. Holochain's substrate implies `ActionHash` (commit-op) + `EntryHash` (content); `NodeId(u64)` will not fit either verbatim, so the adapter needs a holon-id ⇄ `NodeId` registry rather than a direct cast. **Confirm with Steve:** is there a separate semantic id (stable across re-commits), or is `ActionHash` the only handle?

2. **Holon ↔ node/edge mapping — answered-by-code.** Holons have properties + typed relationships + actions; relationships carry `holonic__attraction` (composition/aggregation/association continuum) and now have auto-materialized inverses. Maps cleanly: holon → `NodeSnapshot` (payload = properties), typed relationship → `EdgeSnapshot` (with `holonic__attraction` as an edge weight / type marker). **Still-needs-Steve:** how to represent *containment* (holon-inside-holon) — high-attraction edge vs. a separate structural slot.

3. **Change feed — sharpened, still-needs-Steve.** DanceV2 + commit pass 2 introduce a typed command/result surface, but the docs do not name an *event stream* for external subscribers. Holochain's `get_details` is read-only. **Likely path:** diff-based (record a holon snapshot before/after a commit pass and emit the delta) unless Steve points to a documented event source. **Confirm with Steve:** is there a change/event feed, or is diff-on-snapshot the intended integration pattern?

4. **Write semantics & authority — sharpened, still-needs-Steve.** DanceV2 is the write path (typed commands → executor); Holochain's source-chain + DHT consensus is the underlying authority (each agent owns their chain, DHT validates). Command contract alignment PRO1/PRO2 (PR #538, #544) tightened the wire-domain binding. **Reconciliation question for Steve:** lazily's single-writer-per-effect authority vs. Holochain's per-agent source chains — does the adapter forward writes as DanceV2 commands from a single agent, or fan-out across agents?

5. **Identity/permissions — sharpened, still-needs-Steve.** MAP Security Model is "tied to Layer 0": each holon decides what it is "willing to share" with each requester. Holochain provides agent pubkeys (→ `PeerId`) + capability tokens. **Granularity gap:** MAP's model is per-property/per-relationship, lazily's `PeerPermissions` is per-node (read/write/effect). **Confirm with Steve:** should the adapter model per-property filtering (drop permissions on the floor) or collapse to per-node (over-share)?

6. **Transport expectations — answered-by-code.** `ARCHITECTURE.md` resolves this: lazily-rs runs **only in the Host context** (tokio + threads disqualify it from WASM/zomes). The Holochain receptor in Conductora is the inbound bridge. WebRTC/WS/IPC transport choices apply to **host↔host** peer traffic (UI sync, DAHN visualizers, multi-host coordination), **not** to Holochain DHT traffic (Holochain owns that). → `#akp3` WebSocket priority is correct for host↔UI; WebRTC is for host↔host across NAT.

7. **Reference / spec — sharpened, still-needs-Steve.** Canonical references now exist: [Layer 0: Holons](https://evomimic.gitbook.io/map-book/map-foundations/layer-0-holons.md) (conceptual), repo `ARCHITECTURE.md` (workspace/build model), [MAP Core Dev Docs](https://memetic-activation-platform.github.io/map-dev-docs/core/) (contributor-facing), and the PRO1/PRO2 command-contract PRs come closest to a wire spec. **Still-no single schema doc analogous to lazily's `PROTOCOL.md`.** **Confirm with Steve:** pin the canonical wire-spec reference for adapter-code use.

### Net effect on Phase 0

Three questions are **substantially resolved from public sources** (Q2 mapping, Q6 transport, Q7 references — all sharpened enough to draft adapter scaffolding). Four still need Steve's input (Q1 id shape, Q3 change feed existence, Q4 conflict reconciliation, Q5 permission granularity). The surface Steve has to answer is smaller than the original 7-question set: the adapter can be drafted against the answered three + hypotheses on the other four, then corrected when his answers land.

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
- 2026-06-17 (#lzholonsscan): Public-source re-scan. Three questions
  substantively resolved (Q2 holon↔node/edge mapping, Q6 transport context,
  Q7 canonical references); four sharpened but still-needs-Steve (Q1 id shape,
  Q3 change feed existence, Q4 conflict reconciliation, Q5 permission
  granularity). See the dated re-scan section above. Adapter scaffolding can
  start against the resolved three + hypotheses on the open four.
