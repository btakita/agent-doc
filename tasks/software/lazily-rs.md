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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-180738.md`*

Compacted content:
- Archived 1 response topic(s): Update BENCHMARKS.md with Tokio results
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Review evaluation — glm-5.1

**#lazilyperf40 (Lazy invalidation frontier): COMPLETE** — Implemented via `ThreadSafeInvalidationPlan` (thread_safe.rs:733-923) with sidecar fast frontier (thread_safe.rs:1924-2021). Benchmarks show 34% improvement (eager 5.832ms vs lazy 3.845ms at 16 workers). Covered by Loom model checking, spec compliance tests, and instrumentation tests. All 101 tests pass.

**#lazilyperf39 (Per-slot fine-grained locking): COMPLETE** — Each `ThreadSafeSlotFastPath` (thread_safe.rs:415-627) has independent RwLock, atomics, condvar, and dependency/dependent mutexes. Per-slot recompute uses waiter-counted condvar handoff (thread_safe.rs:534-596) instead of notify_all stampede. Spec compliance tests verify zero graph-lock acquisitions for cached reads and cell-only refreshes.

**#lazilyperf38 (Typed cache fast-path): NOT IMPLEMENTED** — Both `Context` and `ThreadSafeContext` still use `Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>` with `downcast_ref` on every cached read (context.rs:76, thread_safe.rs:46). No typed cache mechanism exists anywhere. No tests, no spec language. This is the remaining gap.

#### Next steps for #lazilyperf38

1. Design typed cache: replace `Rc<dyn Any>` / `Arc<dyn Any + Send + Sync>` with `TypeId`-keyed storage or generic slot wrapper
2. Add SPEC.md language describing the typed cache fast-path contract
3. Implement for both `Context` (single-threaded) and `ThreadSafeContext`
4. Add benchmarks comparing dyn Any downcast vs typed access
5. Update existing tests to validate typed access paths

### Re: Typed cache fast-path implementation — glm-5.1

Completed all three queue items for #lazilyperf38:

**#93dv (SPEC)** — Added "Typed cache fast-path" section to SPEC.md (lines 397-444). Documents inline TypeId storage, unchecked pointer cast read path, benchmark gate, and future implications for lock-free reads.

**#139c (Implementation)** — Added `type_id: TypeId` to `SlotNode`, `CellNode`, `ThreadSafeSlotFastPath`, and `ThreadSafeCellFastPath`. All hot-path reads (`get`, `get_rc`, `get_cell`, `get_cell_rc`, `set_cell`, `read_fresh`, `read_slot_or_dependencies`) now use inline TypeId comparison + unchecked pointer cast instead of `dyn Any::downcast_ref` vtable call. All 101 tests pass. `make check` clean.

**#ek24 (Benchmarks)** — Added `typed_cache_reads` benchmark group (6 cases: context_slot, context_cell, thread_safe_slot, thread_safe_cell, context_rc_slot, context_rc_cell). Updated BENCHMARKS.md cached_reads results:

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| cached_reads/context | 8.22 ns | 7.92 ns | ~3.6% (p<0.05) |
| cached_reads/thread_safe_context | 64.41 ns | 62.49 ns | ~3.0% (p<0.05) |

**Review item #v6r8** — mark done. The typed cache fast-path is now implemented and benchmarked.
<!-- agent:boundary:b8367b10 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#cttd]~
- [#139c]
- [#93dv]
- [#ek24]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
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
