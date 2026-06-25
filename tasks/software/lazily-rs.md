---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: codex
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
queue: start
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
❯ - **Minimal latency / efficiency:** MessagePack wins for all-port production use. Cap'n Proto can beat it in a native-only, schema-first benchmark, but not enough to justify the migration cost before we exhaust the cheaper codec swap. For huge values, the real answer remains `SharedBlob`, not a fancier envelope.
❯ - **Developer ergonomics:** MessagePack is the smallest change from the current serde types. Cap'n Proto and gRPC require IDL/codegen and generated-type ownership across every port.
❯ - **Agentic ergonomics:** JSON stays the canonical inspectable form. MessagePack is acceptable if every binary frame can be losslessly rendered to canonical JSON and fixtures remain JSON-first. Cap'n Proto/protobuf are much more opaque to agents unless the schema/toolchain is always loaded.
❯ - **Transport fit:** MessagePack and JSON are byte payloads that fit FFI, IPC, WebSocket, and WebRTC data channels. gRPC is a transport/API stack and should not be nested inside WebRTC or C ABI buffers.
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260625-202238.md`*

Compacted content:
❯ - Archived 17 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); #next-steps; do #gcp4; 14 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: protocol codec comparison — gpt-5

> **Queue prompt:**
>
> Compare CapnProto, gRPC, Msgpack, vs our current JSON protocol. Which would be the best to support all our use cases with minimal latency, maximum efficiency, developer ergonomics, agentic ergonomics?

**Recommendation: keep the current `Snapshot`/`Delta` protocol semantics, keep JSON as the canonical/debug codec, and add MessagePack as the negotiated production cross-language codec.** Do not replace the protocol with gRPC or Cap'n Proto. Keep postcard as a Rust/same-schema fast path, but do not make it the cross-language default.

The important distinction is that lazily already has the right protocol shape: `IpcMessage = Snapshot | Delta`, strict epochs, permission filtering, shared-blob refs, and transport-neutral frames. The weak part is the default envelope codec. Today `PROTOCOL.md` says JSON is default and postcard is optional; `src/ipc.rs` already has `encode_json`/`decode_json` and `encode_binary`/`decode_binary`, while `src/webrtc_transport.rs` still hard-codes `serde_json` in the WebRTC sink/source. That means the next useful move is a codec abstraction, not a new RPC architecture.

| Option | Verdict |
|---|---|
| **Current JSON** | Best for canonical fixtures, debugging, browser inspection, and agentic ergonomics. Worst for bytes/CPU, especially because byte payloads become integer arrays unless they are `SharedBlob`s. Keep it as the normative text form and fallback. |
| **MessagePack** | Best all-around production codec. It preserves the existing serde data model, works across Rust/Python/JS/Zig/Kotlin with much less schema ceremony, rides WebSocket/WebRTC/FFI byte frames directly, and can round-trip to the same canonical JSON fixtures for agents and tests. |
| **Cap'n Proto** | Strong native-only latency story, especially for schema-first zero-copy structures, but it forces an IDL/codegen workflow and does not buy much for lazily's current envelope because large values already use shared blobs and payload bytes are opaque. It is too much schema machinery for all six ports right now. |
| **gRPC / protobuf** | Good for service APIs, not for this core frame protocol. It brings HTTP/2/service semantics, browser grpc-web caveats, and a transport model that does not fit WebRTC data channels, FFI byte buffers, or local IPC. It could be a future admin/control API, not the graph-state wire. |

By criterion:

- **Minimal latency / efficiency:** MessagePack wins for all-port production use. Cap'n Proto can beat it in a native-only, schema-first benchmark, but not enough to justify the migration cost before we exhaust the cheaper codec swap. For huge values, the real answer remains `SharedBlob`, not a fancier envelope.
- **Developer ergonomics:** MessagePack is the smallest change from the current serde types. Cap'n Proto and gRPC require IDL/codegen and generated-type ownership across every port.
- **Agentic ergonomics:** JSON stays the canonical inspectable form. MessagePack is acceptable if every binary frame can be losslessly rendered to canonical JSON and fixtures remain JSON-first. Cap'n Proto/protobuf are much more opaque to agents unless the schema/toolchain is always loaded.
- **Transport fit:** MessagePack and JSON are byte payloads that fit FFI, IPC, WebSocket, and WebRTC data channels. gRPC is a transport/API stack and should not be nested inside WebRTC or C ABI buffers.

**Next step filed:** `#lzmsgpack` is now the top backlog item. Scope: add negotiated `json|msgpack|postcard` codec support, route WebRTC/FFI through that abstraction, add cross-language conformance vectors, and benchmark JSON vs MessagePack vs postcard before changing any default. This was a design/selection cycle only; I did not edit protocol code or run the build suite.

**Top backlog id: `#lzmsgpack`.**
<!-- agent:boundary:58a4402d -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- 🚧 do [#lzmsgpack]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#lzmsgpack] [recommended] Implement negotiated MessagePack for lazily-ipc: add a codec enum/capability handshake (json|msgpack|postcard), add rmp-serde encode/decode for IpcMessage, route WebRtcSink/WebRtcSource and FFI helpers through the codec abstraction instead of hard-coded serde_json, add cross-language canonical fixtures/round trips, and benchmark JSON vs MessagePack vs postcard before changing any default. Keep JSON canonical/debug fallback; keep postcard Rust-local.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzcrdtplane] [recommended] Implement the distributed CRDT cell plane (spec §Distributed). Decision: purpose-built HLC + LWW/MV/PN registers (matches lazily's lazy reactive graph + memo-equality invariants); do NOT adopt YRS/YJS document model (wrong replication unit). Keep IPC Snapshot/Delta as the single-producer mirror; CRDT merge lives only at the multi-writer distributed boundary. Optional later: borrow YRS binary anti-entropy sync engine as transport. Plan first: tasks/software/plan-lazily-distributed-crdt-plane.md.
- [/] [#lilazilysmpost] [/] [operator-drive] lazily state-machine LinkedIn post PUBLISHED WITH IMAGE: https://www.linkedin.com/feed/update/urn:li:share:7475622100263923712 (mail/social/20260624-142530-linkedin.md @ mail bf4ab26). CyclePhase diagram rendered from blog post SVG (light theme colors) + uploaded via LinkedIn digitalmediaAsset API. Old text-only post (urn:li:share:7475617202893639680) deleted + replaced. Remaining operator follow-up: post first comment (link + GitHub source) via corky linkedin comment.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet backend through the LIVE #yxjw signaling Worker (two real hosts / NAT). Agent-doable slice (#lzwebrtcwire) is DONE: src/webrtc_signaling.rs wires SignalingClient↔Str0mNet and tests/webrtc_signaling.rs covers the localhost handshake. Remaining: live two-host/NAT run via deployed Worker — operator-only.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#lzsidecarlock] [recommended] agent-doc-repo: close the lost-update window among concurrent cycle_state writers. Deadlock analysis DONE (tasks/agent-doc/plan-cycle-state-flock.md): NO production mutator calls another (only test code) -> non-reentrant flock is safe. Design: bounded try_lock_exclusive (~5s, fs2) + RAII guard on a dedicated .json.lock sibling (state file is atomically renamed each save -> cannot lock it directly); add `let _lock = acquire_state_lock(file)?;` to each of the ~24 public mutators in cycle_state.rs. cycle_phase_rank (:1273) guards only monotonic phase, not the ~30 non-phase fields. Hardening (not live-fire). Split from #lzsidecaratomic (DONE in agent-doc 647af082 + installed).
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
