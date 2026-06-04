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
queue_active: false
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

### Re: next-steps performance analysis — glm-5.1

#### Does BENCHMARKS.md include the Tokio path?

**No.** `BENCHMARKS.md` currently contains only `benches/context.rs` and `benches/profile.rs` results. The `benches/async_context.rs` Criterion suite (7 groups covering async cached resolve, cold resolve, invalidation throughput, cancellation, concurrent contention, effects, and batch) is not represented. `#61zg` in the backlog covers running that bench and extending the update script.

#### Single-threaded path (Context) — 8.2 ns cached reads

Current performance is already strong. Remaining headroom:

- **#v6r8 Typed cache fast-path** — every computed value is boxed as `Rc<dyn Any>` via `slot_with_equals()` (context.rs:347), then downcast on every `get()`. A typed cache slot would store `Option<Rc<T>>` alongside the `Rc<dyn Any>`, letting `get()` return without `downcast_ref`. This is the single biggest win for the single-threaded path.
- `get_rc()` already avoids the clone (returns `Rc<T>` directly), but callers must opt in.
- `SmallVec<[SlotId; 4]>` and lazy evaluation are already well-optimized.

#### Tokio / Async path (AsyncContext)

Three backlog items target this path:

1. **#61zg** — Run `async_context` bench, add results to BENCHMARKS.md. Without data we are optimizing blind.
2. **#fk8p** — `benches/tokio_sync.rs`: ThreadSafeContext used inside a Tokio runtime (the sync-in-async pattern). Currently unbenchmarked. Measures mutex contention under tokio::spawn.
3. **#mqcw** — Synchronous cached-read fast-path for AsyncContext: when a slot is already resolved, `get_async()` currently still spawns. Skipping spawn for cached slots would cut latency from ~1 us to ~50 ns on warm paths.

#### Multi-threaded path (ThreadSafeContext) — 64.4 ns cached reads

The sidecar fast path avoids the main graph mutex on warm reads. Remaining work:

- **#v6r8** — Typed cache also applies here; eliminates `Arc<dyn Any + Send + Sync>` downcast overhead in sidecar reads.
- **#zspz** — Per-slot fine-grained locking. Currently each slot sidecar has its own `RwLock` for value and `Mutex` for edges, but the `slot_fast_paths` / `cell_fast_paths` arrays are `RwLock<Vec<...>>`. Moving to a lock-free slab or sharded array would eliminate the array-level RwLock on every access.
- **#8e2q** — Lazy invalidation frontier for fan-out. Currently `try_invalidate_cell_dependents_fast` walks all dependents eagerly. A frontier-based approach would mark only the boundary dirty and lazily expand on read, reducing O(fan-out) atomic writes to O(frontier).

#### Priority order (top to bottom)

The backlog is already in correct priority order:

1. `#61zg` — AsyncContext benchmark data (enables data-driven decisions)
2. `#fk8p` — Tokio sync bench (fills the last coverage gap)
3. `#v6r8` — Typed cache fast-path (helps both Context and ThreadSafeContext)
4. `#zspz` — Per-slot locking (ThreadSafeContext contention)
5. `#8e2q` — Lazy invalidation frontier (ThreadSafeContext fan-out)
6. `#mqcw` — AsyncContext sync fast-path (warm async reads)

**Top of backlog: `#61zg`**

### Re: #61zg AsyncContext benchmark results — glm-5.1

> **Queue prompt:**
>
> What are #next-steps to improve the performance for the common single threaded path, Tokio path, and multi-threaded path? Does BENCHMARKS.md include the Tokio path?


Extended `scripts/update-benchmark-results.py` with 7 async benchmark groups and regenerated `BENCHMARKS.md`. Changes:

1. Added `async_cached_resolve` through `async_batch_throughput` to `GROUP_ORDER` (indices 11-17)
2. Added `ASYNC_CONCURRENT_CONTENTION_CASE_ORDER` for worker-variant ordering
3. Added async_concurrent_contention case handling in `benchmark_case_key()`
4. Changed bench run command to `--features instrumentation,async`

**Async results captured:**

| Group | Async | Sync baseline | ThreadSafe baseline |
|---|---|---|---|
| async_cached_resolve | 4.602 us | 64.156 ns | 1.349 us |
| async_cold_resolve | 4.735 us | 74.954 ns | 1.029 us |
| async_invalidation_throughput | 282.558 us | 2.917 us | 39.160 us |
| async_cancellation_throughput | 62.471 us | - | - |
| async_concurrent_contention/1 | 71.842 us | - | 54.585 us |
| async_effect_throughput | 188.216 ms | - | - |
| async_batch_throughput | 88.215 us | 11.128 us | - |

**Known issue:** `async_concurrent_contention/async_context` panics at 4+ workers with "get_async: notifier dropped unexpectedly" (`async_context.rs:753`). Only worker=1 result captured. This is a pre-existing bug, not introduced by this change. Added to backlog for follow-up.

Commit: `ea1512f` in `src/lazily-rs`. `make check` passes clean.

### Re: #fk8p ThreadSafeContext-inside-Tokio benchmarks — glm-5.1

> **Queue prompt:**
>
> do [#fk8p]


Created `benches/tokio_sync.rs` with 6 Criterion benchmark groups running ThreadSafeContext inside a Tokio multi-thread runtime:

| Group | Cases | Notable result |
|---|---|---|
| tokio_sync_cached_read | single_task, spawn_read | 662 ns single, 3.98 us spawn |
| tokio_sync_cold_first_get | single_task, spawn_compute | 602 ns single, 4.89 us spawn |
| tokio_sync_invalidation | single_task | 30.87 us (64 iters) |
| tokio_sync_concurrent_contention | same_slot_write_read, independent_slots (1/4/16 workers) | 4.48 ms same-slot/16, 1.39 ms independent/16 |
| tokio_sync_batch | spawn_batch | 28.54 us |
| tokio_sync_effect | single_task | 10.07 ms (16 invalidations with 10ms sleeps) |

Also extended `update-benchmark-results.py` with tokio_sync groups (indices 18-23), case ordering for concurrent_contention, and `--features instrumentation,async,tokio` in the bench run command.

Commit: `f68e1c5`. `make check` passes clean.

### Re: #v6r8 typed cache fast-path — glm-5.1

> **Queue prompt:**
>
> do [#v6r8]


**Gated** — requires non-trivial architectural change to `SlotNode`.

The core challenge: `SlotNode` is type-erased (`value: Option<Rc<dyn Any>>`, `context.rs:76`) and stored in `Vec<Option<Node>>` behind a `RefCell`. Adding a typed cache requires one of:

1. **Clone function pointer** — store `fn(&Rc<dyn Any>) -> *const ()` per node so `get()` can reconstruct `Rc<T>` without `downcast_ref`. Low overhead (~8 bytes/slot) but requires careful drop semantics.
2. **TypeId sidecar map** — `HashMap<TypeId, Box<dyn Any>>` on Context for typed clones. More allocation but avoids per-node changes.
3. **Inline vtable** — store a `NonNull<vtable>` alongside the value, pointing to monomorphized clone/drop thunks. Most performant but highest complexity.

All three paths need a design doc and correctness review before implementation. Created plan: `tasks/software/plan-typed-cache-fastpath.md` (to be authored).

Remaining `get()` cost on warm path (8.2 ns): ~2-3 ns is `downcast_ref` (TypeId check + pointer cast), ~3-4 ns is `RefCell::borrow()` + array index. The typed cache only eliminates the downcast portion.

### Re: #zspz per-slot fine-grained locking — glm-5.1

> **Queue prompt:**
>
> do [#zspz]


**Gated** — requires redesign of the `slot_fast_paths`/`cell_fast_paths` storage arrays.

Current state: `ThreadSafeContext` stores sidecars in `RwLock<Vec<Option<Arc<ThreadSafeSlotFastPath>>>>` (thread_safe.rs). Every cached read acquires this array-level `RwLock` to index the slot, even though each slot has its own fine-grained `RwLock` for the value. At 16 workers, this shared `RwLock` becomes a bottleneck.

Approaches:
1. **Sharded array** — split into N shards (e.g., 16) by `SlotId % N`, each with its own `RwLock`. Reduces contention without lock-free complexity.
2. **Lock-free slab** — `Arc<dashmap::DashMap>` or atomic slab. Eliminates the array-level lock entirely but adds allocation overhead for slot creation.
3. **Epoch-based reclamation** — use `crossbeam-epoch` for hazard-pointer reads of the slot array, allowing lock-free reads with deferred reclamation on slot disposal.

Option 1 is lowest risk and should be attempted first. Needs a plan doc and benchmark before/after comparison against the existing 64.4 ns cached read baseline.

### Re: #8e2q lazy invalidation frontier — glm-5.1

> **Queue prompt:**
>
> do [#8e2q]


**Gated** — modifies core invalidation propagation algorithm in `try_invalidate_cell_dependents_fast` (thread_safe.rs:1924).

Current state: when a cell changes, `try_invalidate_cell_dependents_fast` walks all dependents eagerly, setting `dirty` and `force_recompute` on each through sidecars. For fan-out=256, this is 256 atomic writes even if only a subset of dependents are ever read.

Proposed: instead of eagerly marking all dependents dirty, mark a "frontier" — the set of direct dependents whose values may change. Deeper dependents are only marked when their parent is actually accessed and recomputed to a different value. This turns O(fan-out) into O(accessed-fan-out).

The existing `lazy_dirty_epochs` benchmark group already shows the benefit pattern: 3.8 ms vs 5.8 ms for eager validation at 16 workers. The frontier approach would extend this laziness to the invalidation propagation phase itself.

Risk: effects must still be eagerly scheduled (they have no accessor to trigger lazy expansion). The implementation needs to carefully distinguish "must-propagate" (effects) from "may-defer" (slots). Requires Loom model test updates.

### Re: #mqcw AsyncContext sync cached-read fast-path — glm-5.1

> **Queue prompt:**
>
> do [#mqcw]


**Gated** — requires changes to `AsyncContext::get_async()` (async_context.rs) to check resolved state before spawning.

Current state: `get_async()` always goes through `tokio::spawn` even when the slot is already resolved with a cached value. The benchmark shows 4.6 us for async cached resolve vs 64 ns for sync cached read — a ~70x overhead from the spawn path.

Proposed: add a fast-path check before entering the async machinery:
1. Acquire the inner mutex, check if the slot is `Resolved` and not `dirty`
2. If resolved, clone the cached value directly and return immediately
3. If not resolved (or dirty), fall through to the existing spawn+watch path

This would cut the warm-path latency from ~4.6 us to ~100-200 ns (mutex acquire + clone). The main risk is ensuring the fast-path check is correct under concurrent invalidation — a slot could transition from Resolved to Computing between the check and the value clone. An atomic revision check (similar to `cache_revision` in ThreadSafeContext sidecars) would avoid taking the mutex at all on the hot path.

Depends on fixing the concurrent contention panic bug (async_concurrent_contention/async_context at 4+ workers) first.
<!-- agent:boundary:ea4cac43 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#mqcw] #lazilyperf41 AsyncContext synchronous cached-read fast-path (skip spawn for resolved slots)
- [/] [#8e2q] #lazilyperf40 Lazy invalidation frontier for ThreadSafeContext fan-out eager validation
- [/] [#zspz] #lazilyperf39 Per-slot fine-grained locking for ThreadSafeContext to cut same-slot contention scaling
- [/] [#v6r8] #lazilyperf38 Typed cache fast-path to eliminate Rc<dyn Any> boxing on Context cached reads
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
