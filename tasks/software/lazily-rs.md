---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260616-230815.md`*

Compacted content:
- Archived 3 response topic(s): release + publish; LinkedIn scheduled post; LinkedIn post repair
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Closeout repair; Lean status; Artifact cleanup; 4 more

### Re: Bug audit + fix — ipc dead-code warnings, SPEC drift — opus-4-6

**Bug found and fixed:** `EncodeError` and `DecodeError` enums in `src/ipc.rs` were defined unconditionally whenever the `ipc` feature was enabled, but their variants are gated on `ffi`/`ipc-binary`. Building with `webrtc-str0m` (which enables `ipc` without `ffi`/`ipc-binary`) produced dead-code warnings. Fixed by gating the enum definitions on `#[cfg(any(feature = "ffi", feature = "ipc-binary"))]` to match their existing impl blocks (`1cee080`).

**SPEC.md drift fixed:** `SlotNode`, `CellNode`, and `EffectNode` struct definitions in SPEC.md used `Box<dyn Any>` where the actual implementation uses `Rc<dyn Any>`, and were missing `type_id: TypeId`. Updated to match the code with proper type aliases (`ComputeFn`, `EqualsFn`, `EffectFn`).

**Tests added:** `json_codec` module in `tests/ipc.rs` covering `encode_json`/`decode_json` round-trip, invalid-input rejection, and `Display` impl verification for both error types.

**Audit notes:** Core reactive logic (dependency tracking, memo guard, effect scheduling, invalidation propagation, batch flush) is correct — all 300+ tests pass, clippy clean across all feature combos. No panics on edge cases found. The `ShmBlobArena` wraparound/generation logic is sound.

### Re: #next-steps — follow-up backlog — opus-4-6

Remaining items added to backlog (in priority order). Top of backlog: **#lz57by-close**.
<!-- agent:boundary:ba7370ec -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#jknk] #lz57by-close=Close review #57by (Shuttle async spike #shuttleeval concluded NO-GO; the spike is done and the decision is final — the review item should be --done not [/])
- [ ] [#tf0k] #lzsid32=Harden SlotId.0 as usize casts in thread_safe.rs to usize::try_from for 32-bit safety parity with single-threaded Context
- [ ] [#j1vw] #lzerrfrom=[recommended] Add From<serde_json::Error>/From<postcard::Error> impls for EncodeError/DecodeError ergonomics
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: build artifacts are ready in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 28f23e4 + superproject gitlink 6339ab2e1 already pushed.
- [/] [#kabc] #lipostlazilyarch=[operator-drive] Main LinkedIn post is published and repaired: Corky edit updated mail/social/20260616-lazily-architecture-linkedin.md / https://www.linkedin.com/feed/update/urn:li:share:7472666756810190848 with the full body after the public post truncated at "The core is small: three primitives". Local metadata is committed in mail commit 07a9ea1 and superproject gitlink e7e0b08c1. Remaining operator follow-up: post the first comment from mail/drafts/linkedin/2026-06-16-lazily-architecture.md via `corky linkedin comment <published-file> <comment-body>` if still desired.
- [/] [#h6qb] [recommended] [operator-drive] #lzwebrtcnet-e2e=End-to-end cross-host validation of the Str0mNet networked backend through the LIVE #yxjw signaling Worker (two real hosts / NAT, not localhost). The localhost two-socket round trip (tests/str0m_net.rs) already proves the real UDP/DTLS/SCTP/timer path in-process; this remaining slice needs two live endpoints + the deployed Worker so it cannot be agent-run headless. Wire SignalingClient offer/answer/ice to Str0mNet::{offer,answer,accept_answer,add_remote_candidate,local_candidate} and confirm a Snapshot crosses between hosts.
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
