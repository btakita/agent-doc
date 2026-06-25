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
- **Minimal latency / efficiency:** MessagePack wins for all-port production use. Cap'n Proto can beat it in a native-only, schema-first benchmark, but not enough to justify the migration cost before we exhaust the cheaper codec swap. For huge values, the real answer remains `SharedBlob`, not a fancier envelope.
- **Developer ergonomics:** MessagePack is the smallest change from the current serde types. Cap'n Proto and gRPC require IDL/codegen and generated-type ownership across every port.
- **Agentic ergonomics:** JSON stays the canonical inspectable form. MessagePack is acceptable if every binary frame can be losslessly rendered to canonical JSON and fixtures remain JSON-first. Cap'n Proto/protobuf are much more opaque to agents unless the schema/toolchain is always loaded.
- **Transport fit:** MessagePack and JSON are byte payloads that fit FFI, IPC, WebSocket, and WebRTC data channels. gRPC is a transport/API stack and should not be nested inside WebRTC or C ABI buffers.
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260625-202238.md`*

Compacted content:
❯ - Archived 17 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); #next-steps; do #gcp4; 14 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: protocol codec comparison — gpt-5

> **Queue prompt:**
>
> do [#lzmsgpack]


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

- parent repo gitlink: `9a2a86c8f` (`lazily-rs: bump MessagePack IPC codec`), pushed to `origin/main`.
- Public APIs should expose one stable codec selection surface, such as `Codec.Json`, `Codec.MessagePack`, and `Codec.Postcard` where supported, instead of forcing users to compose separate package stacks.
- Ecosystems can still hide unstable or heavier pieces behind feature flags, experimental namespaces, or internal modules while the public package stays unified.
- Agent ergonomics are better when the canonical cell kinds live in one package and one docs/fixture set. Splitting CRDT out would force agents and humans to remember which cell kinds are core versus extension-only.
- A separate package would create version-skew risk between core cell traversal/storage and CRDT merge behavior. That is worse than carrying the code in core because CRDT semantics must remain lockstep with the protocol.
- Cell CRDT is a graph cell kind and synchronization semantic, not an optional transport adapter. If a document can contain a CRDT-backed cell, every language runtime needs to understand its schema, permissions, merge semantics, snapshots, deltas, and conformance fixtures.
- Postcard should remain Rust-local or same-schema-local support, not a cross-language package boundary.
- MessagePack should live in the core package as the preferred production cross-language binary codec, but it can be feature/dependency gated where the ecosystem supports that (`ipc-msgpack` in Rust, optional peer/dependency or subpath import in JS if needed).
- Every core package must include JSON because it is the canonical/debug/conformance codec.
- `src/lazily-rs`: `24b6a0b` (`feat: add negotiated MessagePack IPC codec`), pushed to `origin/main`.
- Probed narrow no-default feature sets; they still fail on the existing unconditional `smallvec` imports in `context.rs` / `thread_safe.rs`, which is pre-existing and outside this queue item.
- Passed full `make check` in `src/lazily-rs`.
- Passed default-feature checks for `ipc-msgpack`, `webrtc ipc-msgpack`, and `ffi ipc-msgpack ipc-binary webrtc`.
- Passed focused codec tests with `cargo test --features "ffi ipc-msgpack ipc-binary webrtc" --test ipc --test ffi --test webrtc_transport --test conformance`.
- Added roundtrip, invalid-frame, conformance, FFI cross-codec, and WebRTC loopback coverage for MessagePack.
- Documented the negotiated frame-codec model in `PROTOCOL.md` and `SPEC.md`: JSON stays canonical/debug/default, MessagePack is the cross-language production binary codec, postcard remains the Rust/same-schema binary option.
- Updated WebRTC source/sink construction so JSON remains the default while callers can opt into `IpcCodec::MessagePack`.
- Updated FFI channels to store decoded `IpcMessage`s and re-encode as the requested output codec, then added MessagePack send/recv/validate/kind/clone exports behind `ffi + ipc-msgpack`.
- Added MessagePack encode/decode helpers for `IpcMessage` plus shared codec dispatch for IPC/WebRTC transport paths.
- Added optional `ipc-msgpack` support backed by `rmp-serde`, with a feature-gated `IpcCodec` enum for JSON, MessagePack, and postcard.

**Next step filed:** `#lzmsgpack` is now the top backlog item. Scope: add negotiated `json|msgpack|postcard` codec support, route WebRTC/FFI through that abstraction, add cross-language conformance vectors, and benchmark JSON vs MessagePack vs postcard before changing any default. This was a design/selection cycle only; I did not edit protocol code or run the build suite.

**Top backlog id: `#lzmsgpack`.**

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

> **Queue prompt:**
>
> Should we create separate packages for Cell CRDT or keep it in the core packages?


> **Queue prompt:**
>
> Should we create separate packages for each codec across all languages or keep it in the core packages?


```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=aba09061-676d-4aad-8de9-4b8e97d063e8 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=13625 candidate_hash=82683e259fbbebd8b417b34a8028ba4c564e5e8282fe5fee2ca3c2b7b6583b5b content_ours_len=13821 content_ours_hash=f9484564a6348f05fe4dda0b0104662d9c2158e97a874452c041cd21b7fbf8e0 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782420288917
```

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/lazily-rs.md source=socket_ack_content patch_id=f66024c9-9f07-4680-a390-501054e2fd1d invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=14822 candidate_hash=80dc5e010b40e5a29459104fccbc03071a57ec7e1847c40baf4d8a074bfeae5f content_ours_len=16704 content_ours_hash=b8a2967d9c8f5d578272625625ac96a8839806272764f75c7a3665438b9249b4 doc=lazily-rs session=3d9ff8e4-bafd-4e1f-b48d-550d3c976904 turn=cycle-1782420348170
```
<!-- agent:boundary:81a4c627 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
