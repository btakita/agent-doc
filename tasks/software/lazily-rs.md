---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->
- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [x] Phase 3: Derived/computed signals
- [x] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [x] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260613-075054.md`*

Compacted content:
- Archived 11 response topic(s): next-steps to fix lazily-rs bugs; #ytp5 (#lzasyncloom); #dh3k (#lzspecconf); 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: ongoing plan document for Map Holons integration — opus-4-8

> **Queue prompt:**
>
> I'm in contact with Steve Melville, creator of Map Holons. Please create an ongoing plan document for the Map Holons integration work.


Created `tasks/software/plan-map-holons-integration.md` as a living/ongoing plan, and queued the integration as `#lzholons` (tagged `[operator-drive]` so go-mode defers it — Phase 0 is blocked on Steve, not on me).

The key framing in the plan: **this is a schema-adapter problem, not a transport problem.** lazily-rs already has the transport + reactive-graph plumbing (`IpcSink`/`IpcSource`, the WebRTC transport, `PeerPermissions`, and the queued #akp3 WebSocket + #97xn bridge hub). What's genuinely unknown is the **holon model**, so the plan is honest about that — it carries 7 **OPEN QUESTIONS for Steve Melville** that gate the adapter:

1. Holon identity (shape → must map to `NodeId`).
2. Holon ↔ node/edge mapping.
3. Incremental change feed vs. full-read-and-diff.
4. Write-back semantics + conflict model (must reconcile with lazily's single-writer effect authority / CRDT cell plane).
5. Holon agents/permissions → `PeerId` + `PeerPermissions` (must not bypass default-deny).
6. Preferred first channel (drives #akp3 priority).
7. A Map Holons schema/wire spec to pin the adapter against.

Phases: **0** discovery (current — ask Steve), **1** read-only inbound (holon change → `Delta`, fixture test), **2** outbound writes (gated by `PeerPermissions`), **3** wire the adapter into the #97xn bridge hub and prove a holon update fanning to WebRTC + WebSocket peers.

Since you have a direct line to Steve, the immediate next step is getting those 7 answers — they unblock `#lzholons` Phase 1. Everything lazily-side (transports + bridge) can proceed in parallel via #akp3 / #97xn.

Note: preflight flagged a semantic-completion match suggesting #akp3 (WebSocket transport) may already be done by #webrtcbackend — that's a false positive. #webrtcbackend is the WebRTC str0m DataChannel backend; a WebSocket transport is separate (and simpler). Leaving #akp3 open.

### Re: do [#akp3] — WebSocket DataChannel backend — opus-4-8

> **Queue prompt:**
>
> do [#akp3]


Done. Shipped a concrete WebSocket backend behind an additive `websocket` feature, committed + pushed to `lazily-hub/lazily-rs` (`e1bb1e9`), superproject pointer bumped (`2be46c8b1`).

**What landed (`src/ws_backend.rs`, +302 lines):**
- `WsDataChannel` implements the existing `DataChannel` trait (#webrtc2) over `tokio-tungstenite`. Because WebSocket is already ordered + reliable, it satisfies the frame contract **directly** — no SDP/ICE handshake and no sans-IO pump, unlike the str0m backend.
- The only mismatch is async-vs-sync: a background tokio **driver task** bridges async tungstenite to the sync, non-blocking `DataChannel` — `send_frame` does a non-blocking enqueue onto an unbounded mpsc the driver drains as binary WS frames; inbound WS messages land on a shared queue `try_recv_frame` pops. One whole serialized `IpcMessage` per frame, so the `WebRtcSink`/`WebRtcSource` permission-filter + codec bridge runs over it **unchanged**.
- `from_stream` takes any upgraded `WebSocketStream`, so the same backend serves a real socket and the tests. Driver aborts on last-handle drop (no leaked task on a dead socket).

**Tests (deterministic, no network):** in-process loopback over a **real WS handshake** via `tokio::io::duplex` (`accept_async`/`client_async`) — one test asserts an unreadable node is omitted from a permission-filtered snapshot, another asserts delta send-order is preserved. Both green.

**CI gap also closed:** the whole WebRTC/WebSocket transport family was *not* wired into `make check` or CI. Added `test-webrtc` (`webrtc-str0m`) + `test-websocket` targets and matching CI steps, so the str0m backend (#webrtcbackend) and this backend are now both covered. Local `make check` green; clippy/fmt clean (only the two pre-existing `ipc.rs` dead-code warnings remain).

Next in the queue: `do [#97xn]` (the multi-channel bridge hub) — that one consumes both transports.

### Re: do [#97xn] — multi-channel reactive bridge hub — opus-4-8

> **Queue prompt:**
>
> do [#97xn]


Done. Shipped `BridgeHub` (`src/bridge.rs`, +380), committed + pushed to `lazily-hub/lazily-rs` (`f9e0ffa`), superproject pointer bumped (`e3e4e591c`).

**What it does:** routes an authorized source-cell write arriving on one peer channel out to the *other* channels — one reactive graph bridged across **WebRTC + WebSocket + in-process IPC at once**. Attachments are stored behind object-safe, error-erased `HubSink`/`HubSource` handles, so a single hub mixes transports with different `IpcSink::Error` types (that's what makes "heterogeneous" real, not aspirational).

**Faithful to the plan's core decision — re-derive, never relay raw frames.** The hub is the graph-apply authority and enforces both boundaries itself:
- **Inbound write enforcement:** a peer may only propose `CellSet` writes, and only on nodes it holds `OpKind::Write` for. Every other op kind (slot values, invalidations, node/edge structure) is authority-derived and dropped — a peer can't forge graph structure.
- **Outbound read filtering:** the authorized write is re-emitted to each other peer as a *fresh* `Delta`, re-filtered to that peer's read allowlist via `Delta::filter_readable` and stamped with that peer's own monotonic epoch. So a write never leaks a node outside a peer's allowlist, even crossing from a WebSocket peer to an in-process one.

**Tests (deterministic):** fan-out-to-readers-only, unauthorized-write-dropped, peer-snapshot-rejected, plus a heterogeneous **WebSocket→in-process** bridge test proving the error-erased attachment actually mixes transport types. All green under both `make test-webrtc` (3) and `make test-websocket` (4); full `make check` exit 0; clippy/fmt clean.

That closes the transport trilogy: `#webrtcbackend` (str0m), `#akp3` (WebSocket), and now `#97xn` (the hub that ties them together). The remaining queue item, `#xccx` (Map Holons integration), stays gated on Steve's answers to the 7 open questions in `plan-map-holons-integration.md` — nothing further is agent-drainable there until those land.
<!-- agent:boundary:27ab8b06 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
- [/] [#57by] #lzasyncshuttle=[operator-gated, NO-GO for now per #shuttleeval] Exhaustive Shuttle async-resolve-loop model. Spike result: Shuttle can model async (has an executor, unlike Loom) but is randomized-not-exhaustive, needs ~140 cfg-swaps + a custom notifier (no tokio::watch equiv) in the hot path, and the two #k03k windows are already deterministically covered. Defer until the async graph grows more concurrency-critical paths. See tasks/software/plan-lazily-async-loom.md.
- [/] [#jpf1] #webrtc-data=WebRTC data channel transport for peer-to-peer IPC (beyond signaling)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
