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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-232638.md`*

Compacted content:
- Archived 5 response topic(s): #perf-bench; #perf-profile; #perf-layout; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to improve the lazily-rs performance?

### Re: next steps — gpt-5

Top backlog item: `#perf-profile-baseline`.

I added the performance backlog in priority order. The next move should be a full profiling pass using the new Criterion and instrumentation surfaces, because the last contention sample was mixed: 1-2 worker runs improved after in-flight dedup, 4 workers was roughly neutral, and 8-16 workers regressed. That profile should decide whether the next implementation patch targets lock waiting, dependency-edge churn, or effect scheduling.

Priority order:

1. `#perf-profile-baseline` — collect the full benchmark and instrumentation baseline.
2. `#perf-contention-wait` — only pursue Condvar/parking for thread-safe in-flight dedup if the baseline proves waiting dominates, and model it first with Loom or Shuttle.
3. `#perf-edge-diff` — optimize dependency edge maintenance if edge churn is the hotspot.
4. `#perf-effect-queue` — optimize effect coalescing/flush bookkeeping if batch/effect scheduling is the hotspot.
5. `#perf-bench-regression` — add a lightweight benchmark comparison script/workflow so future perf patches carry regression evidence.

Proof: changed paths are limited to the session document via agent-doc backlog/exchange persistence; source code was not changed. Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg`, `tsift --envelope search`, and this `agent-doc finalize`. Verification is the finalize/session-check commit guard for this document-only cycle. Escalation: none.
<!-- agent:boundary:9736db80 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#perf-profile-baseline] #performance Run a full Criterion plus instrumentation baseline across cached reads, cold first get, fan-out, memo equality suppression, effect flushing, batch storms, and 1/2/4/8/16-worker contention; record recomputes, duplicate speculative computes, edge churn, effect queue depth, allocations, and lock wait/hold time
- [ ] [#perf-contention-wait] #performance Use the baseline to decide whether ThreadSafeContext in-flight dedup should replace spin-yield waiting with a modeled Condvar or parking path; include Loom or Shuttle coverage before changing the mutex-first synchronization contract
- [ ] [#perf-edge-diff] #performance If edge churn dominates fan-out or memo workloads, optimize dependency maintenance by diffing dependency sets and avoiding remove/re-add work that does not change graph semantics
- [ ] [#perf-effect-queue] #performance If effect queue depth or scheduling churn dominates batch storms, optimize effect coalescing and flush bookkeeping without changing cleanup, disposal, or rerun ordering
- [ ] [#perf-bench-regression] #performance Add a small benchmark-report workflow or script that compares saved Criterion baselines before and after performance patches so regressions are visible before release
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
