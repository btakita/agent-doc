---
agent_doc_session: 9386d273-2288-498c-b9fc-207f224f9fdb
agent: codex
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
queue_active: false
---



# lazily-rs: Reactive Signals Library

<!-- agent:status -->
- [x] Phase 1: Research & design
- [x] Phase 2: Core signal primitives
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-223728.md`*

Compacted content:
- Archived 14 response topic(s): #rx9y; #fzaf; #fzaf memo guard; 11 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to improve performance of lazily-rs?
### Re: lazily-rs performance next steps — gpt-5

Top backlog item: `#perf-bench`.

I added five performance follow-ups to the backlog in priority order. The right first move is measurement, not an optimization rewrite: add benchmarks for the current single-threaded and thread-safe paths, then add instrumentation so later changes can prove whether they reduce real work or only move overhead around.

Priority order:

1. `#perf-bench` — baseline benchmark suite for cached reads, cold first get, dependency fan-out, memo suppression, effect flushing, batch storms, and threaded contention.
2. `#perf-profile` — lightweight counters for recomputes, duplicate speculative computes, dependency edge churn, effect queue depth, allocations, and lock wait/hold time.
3. `#perf-layout` — single-threaded storage/edge-maintenance optimizations driven by benchmark results.
4. `#perf-thread-dedup` — thread-safe in-flight recompute deduplication if duplicate concurrent first-get work is material.
5. `#perf-lock-eval` — RwLock, sharded-lock, or targeted-CAS evaluation only after contention is proven and modeled with Loom/Shuttle.

No code changed in this cycle; this was a backlog/recommendation update. Verification is `agent-doc finalize` plus follow-up `session-check`.
<!-- agent:boundary:c6e503e7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#perf-bench] #performance Add a Criterion benchmark suite for Context and ThreadSafeContext covering cached reads, cold first get, dependency fan-out, memo equality suppression, effect flushing, batch storms, and threaded contention at 1/2/4/8/16 workers
- [ ] [#perf-profile] #performance Add lightweight benchmark instrumentation for recompute counts, duplicate speculative computes, dependency edge churn, effect queue depth, allocations, and ThreadSafeContext lock wait/hold time
- [ ] [#perf-layout] #performance Use benchmark results to optimize single-threaded Context storage and edge maintenance, starting with node storage locality and avoiding unnecessary HashSet/HashMap churn without changing public semantics
- [ ] [#perf-thread-dedup] #performance Prototype ThreadSafeContext in-flight recompute deduplication so concurrent first get callers share one computation when benchmarks show duplicate work is material
- [ ] [#perf-lock-eval] #performance Evaluate RwLock, sharded locks, or targeted CAS only after baseline data identifies lock contention, and require Loom/Shuttle safety models before adopting concurrency changes
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->
- 2026-06-01 [#n12s] #ipc-corruption-live-repro-lazily-rs Logged as #zpc2 in agent-doc-bugs2.md backlog. Live repro of scaffold duplication during finalize --stream. Document manually repaired.
- 2026-06-01 [#exfd] #effect-system Phase 4: Effect system — ctx.effect(|ctx| { ... }) with auto-tracking, scheduling, cleanup, and disposal
- 2026-06-01 [#rx9y] #batch-updates ctx.batch(|ctx| { ... }) to defer invalidation until the batch completes
- 2026-06-01 [#fzaf] #memo-guard Memoization guard — prevent unnecessary downstream cascade during invalidation when recomputed value equals prior value (slots are already lazy: invalidated only, recomputed on get())
- 2026-06-01 [#dpzd] #api-ergonomics ctx.computed() alias and DX improvements
- 2026-06-01 [#45rh] #release-publish Release + publish after effect system lands
- 2026-06-01 [#thr-spec] #parallelism Specify the thread-safe contract for lazily-rs: API shape, Send/Sync bounds, lock/re-entrancy rules, single-threaded fast path compatibility, and parity/differences with lazily-zig's mutex-first design
- 2026-06-01 [#thr-core] #parallelism Implement the thread-safe core after the spec: lock-backed context state, Send + Sync cached values and compute/effect callbacks, safe dependency tracking, and re-entrant user callback boundaries
- 2026-06-01 [#thr-tests] #parallelism Add thread contention coverage: concurrent first get, concurrent set_cell, batch flush ordering, clear/dispose races, effect coalescing, and deadlock/regression checks modeled after lazily-zig thread_safe tests
- 2026-06-01 [#tokio-sync] #tokio Add optional Tokio integration after thread safety is sound: async tests and examples that share the synchronous context across tokio::spawn/spawn_blocking without introducing async compute semantics
- 2026-06-01 [#tokio-async] #tokio Design true async computations/effects only after #tokio-sync: in-flight future deduplication, cancellation, stale future completion, cleanup ordering, and Send vs LocalSet semantics
<!-- /agent:done -->
