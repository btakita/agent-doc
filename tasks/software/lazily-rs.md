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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260614-024149.md`*

Compacted content:
- Archived 4 response topic(s): release + publish; do [#12b1]; #advance-review; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: #advance-review — opus-4-8

> **Queue prompt:**
>
> #advance-review


> **Queue prompt (preset `#advance-review`):** Go through the review items. Add actionable backlog items to complete or advance the review items.

Recovered the session first — a `compact exchange` had left the working tree/snapshot compacted while HEAD was still pre-compact (`unsafe_user_content_drift`). Used `reset --from-current --preserve-session` → `commit` (not plain `commit`, which would have un-compacted) to land the compacted exchange at HEAD, then ran the review triage.

Went through all 5 gated review items against the actual `src/lazily-rs` tree (HEAD `64c7f3f`):

**Resolved — #jpf1 (`#webrtc-data`).** The data-channel transport is already shipped: `webrtc_transport.rs` (tagged `#webrtc2 / #jpf1`) defines the `DataChannel` trait, the permission-filtered `WebRtcSink`/`WebRtcSource` IPC bridge, and `InMemoryDataChannel`; `str0m_backend.rs` gives the concrete str0m loopback backend (`Str0mLoopback`/`Str0mChannel: DataChannel`, with a passing `loopback_datachannel_carries_a_snapshot` test); and `str0m_net.rs` (#lzwebrtcnet) adds the real networked backend (UDP/DTLS/SCTP). "Peer-to-peer IPC beyond signaling" is done — the only remaining slice is live cross-host validation, which is already tracked separately by #h6qb. Archived to `agent:done`.

**Advanced — #h6qb (`#lzwebrtcnet-e2e`, operator-drive).** Confirmed the wiring gap is real: `signaling_client.rs` has **zero** references to `Str0mNet` / `accept_answer` / `add_remote_candidate`. The SignalingClient↔Str0mNet glue plus a localhost handshake test is pure agent work, so I split it out as actionable backlog **#lzwebrtcwire**. Once that lands, #h6qb shrinks to just the live two-host/NAT run through the deployed #yxjw Worker (genuinely operator-only).

**Advanced — #12b1 (`#lzpub011`, operator-drive token rotation).** Found a release-tag hazard while inspecting the tree: HEAD `64c7f3f` (#lzwebrtcnet) sits **one commit ahead** of the unpublished `v0.11.0 @ 03582f5`, so the publish-when-token-rotates instruction in the review item would ship stale code. Captured **#lzpub011prep** to resolve the tag position (move-forward vs 0.12.0 bump) and run a `cargo publish --dry-run` readiness check, so the operator's eventual token rotation is a one-shot. The token rotation itself stays operator-gated.

**Left gated, no agent-actionable advance:**
- **#xccx (`#lzholons`)** — Phase 0 is blocked on Steve Melville answering the 7 open questions (holon identity/mapping/change-feed/conflict/permissions/transport/spec). External; nothing to advance until answers land.
- **#57by (`#lzasyncshuttle`)** — decided NO-GO/defer per #shuttleeval; the trigger to revisit ("async graph grows more concurrency-critical paths") isn't met, so there's no action — left as the gated record of that decision.

Backlog is now ordered **#lzwebrtcwire** (forward progress, unblocked) → **#lzpub011prep** (release prep, ahead of the operator token step). Top of backlog: **#lzwebrtcwire**.
<!-- agent:boundary:89f35c25 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzwebrtcwire] #lzwebrtcwire=Wire SignalingClient offer/answer/ICE <-> Str0mNet::{offer,answer,accept_answer,add_remote_candidate,local_candidate} (signaling_client.rs has ZERO Str0mNet refs today) + add a localhost two-endpoint integration test that drives the full handshake THROUGH SignalingClient over a loopback signaling channel. This is the agent-doable slice of #h6qb; leaves only the live two-host/NAT run via the deployed #yxjw Worker as operator-drive.
- [ ] [#lzpub011prep] #lzpub011prep=Resolve v0.11.0 release-tag drift before the operator publish (#12b1): lazily-rs HEAD 64c7f3f (#lzwebrtcnet networked str0m DataChannel backend) is ONE commit AHEAD of the unpublished tag v0.11.0 @ 03582f5. Decide per the unpublished-tag convention — move v0.11.0 -> HEAD, or bump to 0.12.0 if the networked backend warrants a minor. Then run `cargo publish --dry-run --no-verify` to confirm publish-readiness and pre-stage the gh release notes so the operator token rotation is a one-shot.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet networked backend through the LIVE #yxjw signaling Worker (two real hosts / NAT, not localhost). The localhost two-socket round trip (tests/str0m_net.rs) already proves the real UDP/DTLS/SCTP/timer path in-process; this remaining slice needs two live endpoints + the deployed Worker so it cannot be agent-run headless. Wire SignalingClient offer/answer/ice to Str0mNet::{offer,answer,accept_answer,add_remote_candidate,local_candidate} and confirm a Snapshot crosses between hosts.
- [/] [#12b1] #lzpub011=[operator-drive] Finish lazily-rs v0.11.0 publish — crates.io upload + GitHub Release are BLOCKED on a revoked crates.io token (403 auth failed on both the env CARGO_REGISTRY_TOKEN and passage btak/CARGO_REGISTRY_TOKEN; both are the same expired cio-prefixed token). Operator: rotate the crates.io token (browser/passkey 2FA), store at passage btak/CARGO_REGISTRY_TOKEN, then from src/lazily-rs run: CARGO_REGISTRY_TOKEN=$(passage btak/CARGO_REGISTRY_TOKEN) cargo publish --no-verify (commit 03582f5 + tag v0.11.0 already pushed), then gh release create v0.11.0 --generate-notes.
- [/] [#xccx] #lzholons=[operator-drive] Map Holons ↔ lazily-rs integration. Phase 0 (discovery) is blocked on Steve Melville answering the 7 OPEN QUESTIONS (holon identity, holon↔node/edge mapping, change feed, write/conflict model, permissions mapping, preferred transport, spec reference). Plan: tasks/software/plan-map-holons-integration.md. Unblocks once answers land; then Phase 1 read-only inbound adapter.
- [/] [#57by] #lzasyncshuttle=[operator-gated, NO-GO for now per #shuttleeval] Exhaustive Shuttle async-resolve-loop model. Spike result: Shuttle can model async (has an executor, unlike Loom) but is randomized-not-exhaustive, needs ~140 cfg-swaps + a custom notifier (no tokio::watch equiv) in the hot path, and the two #k03k windows are already deterministically covered. Defer until the async graph grows more concurrency-critical paths. See tasks/software/plan-lazily-async-loom.md.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
- [ ] [#pw26] #lzmapspike=[recommended] Spike: lazily-rs as a host-side reactive derivation + Snapshot/Delta UI-sync cache for MAP holons (evomimic/map-holons, Holochain). Model committed holons as reactive graph nodes; derived holon views/relationships as memoized computeds that recompute only on upstream change; stream lazily-ipc Snapshot/Delta to the TS UI / DAHN graph visualizers. Native host/ layer only (AsyncContext needs tokio; zomes are wasm32/short-lived/single-threaded so in-zome use is limited to single-threaded Context). Do NOT use lazily distributed/signaling — Holochain DHT owns P2P/persistence/validation/consensus. Gate on MAP host-layer actually recomputing derived holon state.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
