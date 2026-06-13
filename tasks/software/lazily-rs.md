---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
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
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260613-075054.md`*

Compacted content:
- Archived 11 response topic(s): next-steps to fix lazily-rs bugs; #ytp5 (#lzasyncloom); #dh3k (#lzspecconf); 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
<!-- agent:boundary:e50f4a93 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- I'm in contact with Steve Melville, creator of Map Holons. Please create an ongoing plan document for the Map Holons integration work.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#akp3] #lzwstransport=[recommended] WebSocket IpcSink/IpcSource transport. WebSocket is ordered+reliable so it satisfies the DataChannel frame contract directly — no signaling/ICE/sans-IO backend needed (much smaller than #webrtcbackend). Implement a WsDataChannel over a ws stream and reuse WebRtcSink/WebRtcSource, or impl IpcSink/IpcSource directly.
- [ ] [#97xn] #lzmuxbridge=[recommended] Multi-channel reactive bridge hub: fan one reactive graph across heterogeneous IpcSink/IpcSource attachments (WebRTC + WebSocket + in-process IPC) at once. Re-derive per-peer filtered deltas through the graph-apply authority — do NOT relay raw frames — to preserve PeerPermissions default-deny. Plan: tasks/software/plan-lazily-multichannel-bridge.md.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
