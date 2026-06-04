---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: opencode
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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-161239.md`*

Compacted content:
- Archived 5 response topic(s): lazyperf34 lazyperf35; benchmark-skill BENCHMARKS.md freshness + skill/runbook; performance differences; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
### Re: next-steps for single-threaded, Tokio, and multi-threaded performance — glm-5.1

#### BENCHMARKS.md and the Tokio path

**No, BENCHMARKS.md does not include the Tokio/AsyncContext path.** It covers `Context` (single-threaded), `ThreadSafeContext` (multi-threaded via `std::thread`), and instrumentation profiles. The async benchmarks in `benches/async_context.rs` exist but results are absent from BENCHMARKS.md. The `tokio_sync.rs` integration tests exercise `ThreadSafeContext` inside Tokio tasks but have no dedicated Criterion benchmarks.

#### Single-threaded path (Context) next-steps

1. **Eliminate `Rc<dyn Any>` boxing on hot cached reads.** The cached read path (8.2 ns) goes through `Rc<dyn Any>` dispatch. A typed cache layer could drop this below 5 ns.
2. **Reduce `cold_first_get` overhead (90 ns).** Pre-allocating slot storage and using an index-based tracking stack instead of `Vec<SlotId>` on the thread-local would help.
3. **`SmallVec` edge tuning.** For high fan-out, a hybrid approach (small inline + spill to `HashSet` for `contains` checks at scale) would reduce the O(n) scan.
4. **Memo equality fast-path.** A per-node dirty epoch check before recomputation could skip entire subtrees when parity hasn't changed.

#### Tokio path (AsyncContext) next-steps

5. **Add AsyncContext results to BENCHMARKS.md.** Extend `scripts/update-benchmark-results.py` to parse async benchmark output.
6. **Add `ThreadSafeContext`-inside-Tokio benchmarks.** A dedicated `benches/tokio_sync.rs` with cached reads, invalidation, and batch under Tokio multi-thread runtime.
7. **Reduce async overhead vs sync baseline.** A fast-path that returns the cached value synchronously without spawning a task.
8. **Async concurrent contention optimization.** A batch-then-read pattern or read-only cached snapshots for readers would reduce contention.

#### Multi-threaded path (ThreadSafeContext) next-steps

9. **Same-slot write/read scaling (7.2 ms at 16 workers).** Per-slot fine-grained locking would allow independent slots to proceed without contention.
10. **Effect batch flush at 16 workers (6.2 ms).** Batch coalescing (accumulate invalidations, single publish at batch exit) would reduce lock sites.
11. **Independent slot contention — reduce publish lock hold.** A lock-free publish path (atomic epoch bump + deferred edge cleanup).
12. **Graph propagation fan-out eager validation (5.8 ms at 16 workers).** A lazy invalidation frontier would defer cost, matching the `lazy_dirty_epochs` pattern that already shows 3.8 ms vs 5.8 ms.

Top of backlog: **#lazilyperf36**
<!-- agent:boundary:298a8840 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- What are #next-steps to improve the performance for the common single threaded path, Tokio path, and multi-threaded path? Does BENCHMARKS.md include the Tokio path?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#61zg] #lazilyperf36 Add AsyncContext benchmark results to BENCHMARKS.md (run async_context bench, extend update script)
- [ ] [#fk8p] #lazilyperf37 Add ThreadSafeContext-inside-Tokio Criterion benchmarks (benches/tokio_sync.rs)
- [ ] [#v6r8] #lazilyperf38 Typed cache fast-path to eliminate Rc<dyn Any> boxing on Context cached reads
- [ ] [#zspz] #lazilyperf39 Per-slot fine-grained locking for ThreadSafeContext to cut same-slot contention scaling
- [ ] [#8e2q] #lazilyperf40 Lazy invalidation frontier for ThreadSafeContext fan-out eager validation
- [ ] [#mqcw] #lazilyperf41 AsyncContext synchronous cached-read fast-path (skip spawn for resolved slots)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
