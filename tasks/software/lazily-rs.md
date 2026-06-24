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
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260624-204347.md`*

Compacted content:
❯ - Archived 7 response topic(s): lazily-rs bug next-steps; backlog (priority order); #lzasyncdispose2; 4 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4af...

### Re: lazily-rs bug next-steps — glm-5.2

Found 2 confirmed bugs + 1 low-priority consistency item. Full suite (`cargo test --all-features`) + clippy across every feature combination pass clean, so these are latent runtime bugs the current suite doesn't exercise.

**1. `#e4tf` / #lzasyncrerunabort — async effect re-run leaks (HIGH, proven)**
`flush_async_effects` (`src/lazily-rs/src/async_context.rs:1176-1209`) spawns the effect re-run and overwrites `e.in_flight` **without aborting** the prior still-`.await`ing task. When an `effect_async` dependency changes again mid-flight, a second concurrent run spawns and the first `JoinHandle` is silently dropped (task keeps running). Empirical reproducer: `compute_count=2` (double concurrent execution) + `cleanup_count=0` (the overwritten run's cleanup is never invoked → leaked handle/subscription).
Fix: mirror `dispose_async_effect` (`:1054-1055`) — `take()` + `abort()` the prior `in_flight` before spawning the re-run, and add the reproducer as a regression test.

**2. `#xdsx` / #lzwsinboundunbounded — WebSocket inbound unbounded (HIGH)**
`WsDataChannel` inbound is a bare unbounded `VecDeque` (`src/lazily-rs/src/ws_backend.rs:92,147,151`). A consumer slower than the socket reader grows receive memory without bound — the exact class already fixed for the str0m inbox (`#lzstr0mnetinbox`, `str0m_net.rs:131-150,577-583`) and the WS outbound (`#lzwsunbounded`). The str0m backend sibling was capped; the WS inbound was missed.
Fix: mirror str0m's `push_inbox_frame` — cap at `MAX_INBOX_FRAMES`, drop+count oldest via `AtomicUsize`, expose `dropped_inbox_frames()`.

**3. `#hses` / #lzstr0mloopbackinbox — loopback inbox consistency (LOW, recommended)**
`Str0mLoopback` inbox (`str0m_backend.rs:204`) is also an unbounded `push_back`. Deterministic in-process test harness (not a network DoS vector), so lowest priority, but applying the same cap gives uniform backpressure semantics across every `DataChannel` backend.

Backlog is prioritized top → bottom. **Top item id: `#e4tf`** (`#lzasyncrerunabort` — core async-primitive correctness, proven reproducer). Both HIGH items are clean to land; on confirmation I'll execute `#xdsx` first (lowest-risk, well-precedented) then `#e4tf` (regression test ready from the reproducer).
> **Queue prompt:** What are #next-steps to fix lazily bugs?

### Re: lazily-rs bug fixes #e4tf + #xdsx shipped — glm-5.2

Picked up the queue-backed backlog items and landed both HIGH-priority fixes. Build + full suite green; pushed to origin/main.

**#e4tf `#lzasyncrerunabort` — FIXED (5b1ac88).** `flush_async_effects` now takes + aborts the prior `in_flight` under the lock before spawning the re-run, mirroring `dispose_async_effect`. Regression test `effect_rerun_aborts_prior_inflight` asserts exactly one completed run (was 2 pre-fix) and that the surviving cleanup fires once on dispose. Verified the test FAILS without the abort (`left: 2`) and passes with it.

**#xdsx `#lzwsinboundunbounded` — FIXED (279eb74).** `WsDataChannel` inbound is now capped at `WS_INBOUND_CAPACITY` (1024), shedding oldest-first and counted via `dropped_inbound_frames()`, mirroring the str0m inbox fix (`#lzstr0mnetinbox`). Unit test `inbound_caps_at_capacity_and_counts_drops` covers cap + drop + count.

**Bonus CI unblock (6a41992):** `tests/conformance.rs` carried a non-fmt-clean line that kept the CI "Check formatting" step red on main; `make fmt` normalized it.

**Verification:** `cargo test --all-features` → 0 failures; clippy clean across async / websocket / webrtc-str0m / signaling-client / ffi / ipc-binary / instrumentation; `cargo fmt --check` clean; `cargo build --all-features` green. Pushed a41aaec..6a41992.

**Pre-existing (not a regression):** `make benchmark-check` is red on clean HEAD too — `BENCHMARKS.md` lacks p50/p95 latency rows for the `thread_safe_*` contention benchmarks. Filed as `#lzbmcheckdrift`. Untouched by these fixes.

Remaining backlog: `#hses` (low-priority loopback inbox consistency) + the new `#lzbmcheckdrift`.
<!-- agent:boundary:1a9d2664 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~What are #next-steps to fix lazily bugs?~~ — auto-struck: answered this cycle (#ftstrike)
- [#e4tf]
- [#xdsx]
- [#hses]
- do [#gxzm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#gxzm] #lzbmcheckdrift=[recommended] make benchmark-check red on main: BENCHMARKS.md is missing p50/p95 latency rows for thread_safe_contention / thread_safe_effect_contention / thread_safe_graph_propagation benchmarks. Pre-existing — fails identically on a41aaec, unrelated to the async/ws fixes. Fix: run make benchmark-update to regenerate from the thread_safe contention benches, or relax the check script's required-row set.
- [ ] [#hses] [recommended] #lzstr0mloopbackinbox=Low-priority consistency: Str0mLoopback inbox (str0m_backend.rs:82,94,204) is also an unbounded push_back. Deterministic in-process test harness (not a network DoS vector), so lower priority than #lzwsinboundunbounded, but apply the same cap for uniform backpressure semantics across all DataChannel backends.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lilazilysmpost] [/] [operator-drive] lazily state-machine LinkedIn post PUBLISHED WITH IMAGE: https://www.linkedin.com/feed/update/urn:li:share:7475622100263923712 (mail/social/20260624-142530-linkedin.md @ mail bf4ab26). CyclePhase diagram rendered from blog post SVG (light theme colors) + uploaded via LinkedIn digitalmediaAsset API. Old text-only post (urn:li:share:7475617202893639680) deleted + replaced. Remaining operator follow-up: post first comment (link + GitHub source) via corky linkedin comment.
- [/] [#kb9b] #lzpypub011=[operator-drive] Publish lazily-py v0.11.0: SHIP-STOPPER #lzpysigfix is FIXED (commit 30907ea). Build artifacts rebuilt in src/lazily-py/dist/ (lazily-0.11.0-{whl,tar.gz}, twine check PASSED, fresh-venv install round-trip verified). From src/lazily-py: tag v0.11.0 + push, gh release create v0.11.0 --generate-notes, then python -m twine upload dist/* (needs PyPI token + browser/passkey 2FA). Gated on the pre-publish live-test confirm per the testing-gate rule. Commit 30907ea + superproject gitlink eb93a75ac already pushed.
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
