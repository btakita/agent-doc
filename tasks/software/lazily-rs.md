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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260605-030600.md`*

Compacted content:
- Archived 3 response topic(s): do [#xtwf] Rigorous A/B for #vd5v lock-free reads; do [#s0fc] Make the signaling endpoint consumable as a dependency; release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ Would it be possible to switch between the optimal low contention strategy and the optimal high contention strategy?
❯ >   2. The most valuable course-correction this session was #xtwf: the rigorous A/B caught that my #vd5v "perf win" claim
❯        was actually a low-contention regression / high-contention win, and the SPEC gate now reflects reality. Worth keeping
❯        in mind that 0.9.0 ships that tradeoff (a few % slower uncontended reads, big wins at 8+ cores) — documented in SPEC.
### Re: Switching between low- and high-contention read strategies — opus-4-8

> **Queue prompt:**
>
> release + publish


Yes — three ways, in increasing cost/complexity. The honest summary: a **compile-time switch** is cheap and clean; **runtime-adaptive** is possible but usually not worth it; the genuinely-better "best of both" is a **different storage representation** the SPEC already anticipates.

#### (a) Compile-time feature switch — cheap, recommended if you just want the choice
A `lockfree_reads` cargo feature selects the storage at build time: `arc_swap::ArcSwapOption` (high-contention) vs `parking_lot::RwLock` (low-contention). Zero runtime overhead either way; the read path is monomorphized to one strategy. The deployer picks per their workload. This is exactly the "feature-gated" option from the #vd5v decision — easy to add to 0.9.x so low-contention users can opt back to RwLock without losing the 8+ core win for everyone else.

#### (b) Runtime-adaptive switch — possible, rarely worth it
Two hard parts:
- **Migration.** Flipping a slot's storage mode while readers are in flight needs a barrier — you can't lock-free-ly convert RwLock-backed ↔ ArcSwap-backed storage. It *is* feasible to flip a slot's mode on a write (writers already hold the graph mutex), so migration could ride the write path.
- **Per-read cost.** Every read then branches on the current mode (`if lockfree { arcswap.load } else { rwlock.read }`) plus an atomic mode load. That branch lands on the *uncontended* path too — the one we're trying to keep fast — so you partially erode the benefit of *both* strategies. Add contention detection (sampled reader depth / contended-acquire counts) with hysteresis to avoid flapping, and per-slot state, and it's a lot of machinery that seldom recovers the full win of either pure path. Worth it only if contention is sharply bimodal per slot.

#### (c) The real "best of both" — inline `ErasedValue` + seqlock (SPEC's future direction)
The reason arc-swap costs anything uncontended is the `Arc` itself: refcount traffic + the `Arc<Arc<dyn Any>>` double-indirection I had to use. The optimistic-validation envelope (`cache_revision` read-before/read-after) is *already a seqlock*. If small `Copy`/`Clone` values were stored **inline** (the SPEC's anticipated `ErasedValue` replacing `Arc<dyn Any>`), a reader could copy the inline bytes and validate the sequence — **wait-free, no refcount, no per-reader atomic contention, no allocation** — which beats *both* RwLock and arc-swap across the contention spectrum. The catch: it's sound only for small/`Copy` (or cheaply-`Clone`, torn-read-validated) types; large/non-`Copy` values still need the arc-swap fallback. So it's a size-specialized fast path, not a universal swap — a real redesign, good for a 0.10/1.0 milestone.

#### Recommendation
- Want user choice now? I'll add the `lockfree_reads` feature (option a) — small, zero-cost, ships in a 0.9.x patch.
- Want the durable win? Spec out the `ErasedValue`/seqlock inline-small-value path (option c) as the next perf milestone; it subsumes the whole question.
- I'd skip (b) unless you have a concrete bimodal-contention workload that justifies the per-read branch.

Say which (or both a+c) and I'll implement / spec it. No code changed this turn — this is analysis.
<!-- agent:boundary:7be6f76e -->
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
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
