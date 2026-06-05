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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260605-040841.md`*

Compacted content:
- Archived 4 response topic(s): Switching between low- and high-contention read strategies; (a) as a high/low-concurrency flag, default low; is (c) conditional on the value's size?; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: [#vbyc] Phase 2 (#rdstrat2) — inline small-Copy seqlock cached-read fast path — opus-4-8

> **Queue prompt:**
>
> [#vbyc]


Implemented, loom-gated, committed + pushed (`lazily-rs` `6adae49`, superproject `2600f2b73`).

**What landed** — a third `CachedReadStorage::Inline` variant on the `ThreadSafeContext` slot sidecar. Small `Copy` values are stored inline behind a wait-free single-writer / multi-reader seqlock — no heap `Arc`, no refcount traffic on read or publish — optimal under *both* `ReadStrategy` modes; the runtime mode now only governs the large / non-`Copy` fallback.

**Two deviations from the plan sketch, both forced:**

1. **Opt-in, not automatic.** Stable Rust cannot branch on `T: Copy` inside a generic fn that lacks the bound — method resolution is pre-monomorphization, so a `Copy`-gated impl is never *applicable* where the bound is unprovable (I built the inherent-vs-trait probe the sketch implied; it silently resolved to the non-`Copy` fallback — `dead_code` warnings confirmed it). Automatic detection needs nightly `specialization`. Resolution: opt-in `Copy`-bounded `slot_copy` / `computed_copy` / `memo_copy` constructors that fall back transparently when the value exceeds `INLINE_CAP` (24 bytes) or align 16.
2. **Atomic byte buffer, not memcpy.** The plan's "memcpy `size` bytes under the seqlock" is a *formally-UB data race* (the classic benign-but-UB seqlock) and loom flags it. Buffer is `[AtomicU8; 24]` with relaxed per-byte ops; `seq` Release/Acquire + an Acquire fence (crossbeam-style) give the consistency envelope. Sound and loom-modelable.

**Soundness rests on:** `Inline` chosen only when `T: Copy && size_of ≤ 24 && align ≤ 16`; single-writer invariant (every publish/clear runs under the graph state write lock — verified, only reads are lock-free); the existing `cache_revision`/`dirty` envelope unchanged; bytes interpreted as `T` only after a stable even `seq` proves the snapshot untorn.

**Verification:** fmt ✓, clippy `--all-features -D warnings` ✓, build (all-features + release) ✓, all functional suites ✓ (default/tokio/async/distributed/signaling), **full loom suite 15/15** including three new models — torn-read refusal, full-envelope torn+stale refusal under concurrent publish, post-invalidation stale rejection. New unit tests cover inline selection in both strategies, large-value fallback, multi-field `Copy` struct roundtrip, and a 4-reader concurrent torn-read stress test.

`make check`'s `benchmark-check` reports stale, but that is **pre-existing** leftover `target/criterion/` cache unrelated to this change (a clean HEAD worktree passes; my edits touch no benchmarks/scripts/instrumentation). An inline-vs-strategy A/B benchmark belongs with the 0.10.0 release work (#9mhz).

Next queue item **#9mhz** (release 0.10.0) is user-gated on the irreversible crates.io publish — I'll prepare the bump/VERSIONS/changelog when the loop reaches it and stop for your confirmation before publishing.
<!-- agent:boundary:34eefb57 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~[#vbyc]~
- [#9mhz]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#9mhz] Phase 3 (#rdstrat3): release lazily 0.10.0 (runtime-selectable read strategy + inline seqlock; default flips to LowConcurrency) — gate on user confirm for crates.io publish. Plan: tasks/software/plan-lazily-0.10-read-strategy.md [recommended]
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
