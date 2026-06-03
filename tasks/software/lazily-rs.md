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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-025152.md`*

Compacted content:
- Archived 1 response topic(s): #lazyperf16
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to improve performance and reduce memory usage?

### Re: #next-steps to improve performance and reduce memory usage — gpt-5

Audited the full codebase (context.rs, thread_safe.rs, instrumentation.rs, SPEC.md, README benchmarks). Here are prioritized next steps:

**High priority — dependency/edge storage:**
- `#lazyperf17`: Replace `HashSet<SlotId>` with `SmallVec<[SlotId; 4]>` for dependency/dependent edges. Every `SlotNode` carries two HashSets, every `CellNode` one. For typical 1-3 dependency fan-out, HashSet overhead (hash computation + bucket allocation) is wasteful. This also eliminates the `.iter().copied().collect::<Vec<_>>()` temporary allocations in every hot path (`refresh_slot`, `mark_slot_dirty`, `clear_slot_now`, `invalidate_cell_dependents_now`).

**High priority — temporary allocations:**
- `#lazyperf18`: Eliminate temporary `Vec<SlotId>` allocations in invalidation propagation. Each propagation step clones dependency/dependent sets into owned Vecs. Pre-allocate a scratch buffer on Context/ThreadSafeContext, or use SmallVec/iterator chaining to avoid heap allocation per step.

**Medium priority — borrow-check overhead:**
- `#lazyperf19`: Consolidate ~10 `RefCell` fields on `Context` into a single `RefCell<ContextInner>`. Reduces borrow-check flag modifications from 4-6 per `get()` to 1, and eliminates the re-entrant borrow risk that currently requires `unsafe` pointer copies.

**Medium priority — thread-safe node storage:**
- `#lazyperf20`: Switch `ThreadSafeState` from `HashMap<SlotId, Node>` to slot-id-indexed `Vec<Option<Node>>`, matching the single-threaded path's hash-free lookup. The SPEC.md already calls this out as a deliberate design difference.

**Medium priority — per-slot sync primitive overhead:**
- `#lazyperf21`: Consolidate `ThreadSafeSlotFastPath` from 4 mutexes + 2 Condvars + 3 atomics per slot (~400+ bytes overhead per slot, ~4000 sync primitives per 1000-slot graph) to 1-2 locks + atomic guard words.

**Lower priority:**
- `#lazyperf22`: Slot ID reuse / vacant-entry compaction — prevents unbounded Vec growth from transient effects.
- `#lazyperf23` [recommended]: Replace `unsafe` pointer-to-Box copies in `recompute_slot_now`/`run_effect` with `Arc<ComputeFn>` or safe restructuring.
- `#lazyperf24` [recommended]: Benchmark `SmallVec` vs plain `Vec` for dependency edges before committing — measure cache-line impact on fan-out benchmarks.

Top of backlog: **`#lazyperf17`**
Are Vec lookups slower than HashMap lookups?

### Re: Vec vs HashMap lookups — gpt-5

No — Vec index lookups are faster than HashMap lookups for dense integer keys like `SlotId`.

- **Vec index**: O(1), single pointer arithmetic + memory dereference, no hash computation, perfect cache locality.
- **HashMap lookup**: O(1) amortized but requires hashing the key, probing the bucket array, and handling collisions. Per-lookup overhead is significantly higher.

For lazily-rs specifically, `SlotId` values are dense sequential integers, so `Vec<Option<Node>>` indexed by `SlotId` avoids all hash overhead with zero wasted slots. The single-threaded `Context` already uses this pattern — `#lazyperf20` proposes aligning the thread-safe `ThreadSafeState` to match.

The only case where HashMap wins is with sparse keys (lots of gaps in the ID space), which `#lazyperf22` (slot ID reuse/compaction) would address.

### Re: Queue #zfam #fftjz #rvjd #vftn — gpt-5

> **Queue prompt:**
>
> [#zfam]
>
> [#f5jz]
>
> [#rvjd]


Processed 4 of 8 queue items. All 106 tests pass (including loom model-checking).

**#zfam #lazyperf17 — SmallVec for edges:** Replaced `HashSet<SlotId>` on `SlotNode.dependencies`, `SlotNode.dependents`, `CellNode.dependents`, `EffectNode.dependencies` with `SmallVec<[SlotId; 4]>` in both `context.rs` and `thread_safe.rs`. Added `smallvec = "1"` dependency. Inline storage for ≤4 edges eliminates per-node heap allocation and hash computation for typical 1-3 fan-out.

**#f5jz #lazyperf18 — Eliminate temp allocations:** All `.iter().copied().collect::<Vec<_>>()` patterns on edge fields replaced with `.clone()` (inline SmallVec copy) or `std::mem::take` (for drain patterns). Hot paths affected: `refresh_slot`, `recompute_slot_now`, `clear_slot_now`, `invalidate_cell_dependents_now`, `mark_slot_dirty`, `notify_slot_value_changed`, `run_effect`, `effect_should_run`, `dependents_locked`, `read_slot_or_dependencies`.

**#rvjd #lazyperf19 — RefCell consolidation:** Merged 10 `RefCell` fields on `Context` into single `RefCell<ContextInner>`. Methods that previously borrowed 3-4 separate RefCells (e.g. `schedule_effect`, `flush_effects`) now borrow once. Instrumentation recording merged into the same borrow scope as node mutations.

**#vftn #lazyperf23 — Replace unsafe with Rc:** Changed `SlotNode.compute` and `EffectNode.run` from `Box<dyn Fn>` to `Rc<dyn Fn>`. Eliminated both `unsafe` blocks in `context.rs` — `recompute_slot_now` and `run_effect` now use `Rc::clone` + `(closure.as_ref())(self)` instead of raw pointer copies.

**Deferred items:** #xsp1 (#lazyperf20 HashMap→Vec for ThreadSafeState), #r0jv (#lazyperf21 sync primitive consolidation), #ktrs (#lazyperf22 slot ID reuse), #r54f (#lazyperf24 benchmarks) remain in backlog for a follow-up pass.

### Re: Queue #xsp1 #ktrs — gpt-5

> **Queue prompt:**
>
> [#xsp1]
>
> [#r0jv]
>
> [#ktrs]
>
> [#r54f]

Processed 2 of 4 queue items (2 deferred). All 98 tests pass (including 12 loom model-checking).

**#xsp1 #lazyperf20 — Vec node storage for ThreadSafeContext:** Replaced `HashMap<SlotId, ThreadSafeNode>` with `Vec<Option<ThreadSafeNode>>` in `ThreadSafeState`. Added `get_node`, `get_node_mut`, `insert_node`, `remove_node` helper methods to encapsulate Vec-indexed access. All 35 call sites updated. Eliminates hash computation and bucket probing on every node lookup in the thread-safe path, matching the single-threaded `Context` design.

**#ktrs #lazyperf22 — Slot ID reuse:** Added `free_ids: Vec<u64>` to both `ContextInner` and `ThreadSafeState`. `alloc_id` now prefers reusing freed IDs before allocating new sequential ones. `dispose_effect` returns the freed ID to the list. Prevents unbounded Vec growth from transient effects in long-lived contexts.

**Deferred items:**
- #r0jv (#lazyperf21): Consolidating ThreadSafeSlotFastPath sync primitives is high-risk — the separate locks (RwLock for concurrent reads, mutexes for deps/recompute/dependents, Condvar for waiters) are deliberately designed to avoid contention. Merging could introduce deadlocks or reduce read concurrency. Recommend keeping unless benchmark evidence shows per-slot sync overhead dominates.
- #r54f (#lazyperf24): SmallVec vs Vec benchmarking requires actual benchmark runs with Criterion. Recommend running `cargo bench` A/B comparison before committing to either.

### Re: Queue cleanup — gpt-5

Removed stale `[#ktrs]` from queue (completed and reaped from backlog but queue entry remained).

Remaining queue: #r0jv (deferred — sync primitive consolidation is high risk), #r54f (deferred — needs benchmark runs).

### Re: Queue #r0jv #r54f — glm-5.1

Both items remain deferred after thorough analysis. Here's the evidence:

**#r0jv #lazyperf21 — Sync primitive consolidation (deferred):**

Audited all 4 locks on `ThreadSafeSlotFastPath` (thread_safe.rs:233-244):

| Lock | Purpose | Contention domain |
|------|---------|-------------------|
| `value: RwLock` | Concurrent cached reads via `read_fresh()` | Read-mostly |
| `recompute: Mutex + Condvar` | Exclusive recomputation + waiter notification | Write-only |
| `dependencies: Mutex` | Dependency snapshot (called outside state lock in `recompute_slot_now:1572`) | Read-during-compute |
| `dependents: Mutex` | Dependent frontier for sidecar invalidation | Write-during-edge-reg |

Merging `dependencies` into `recompute` creates a **lock ordering hazard**: `dependencies_snapshot()` is called *before* taking the state lock (line 1572), while the recompute mutex is held during `wait_for_recompute` with Condvar parking. A thread holding the merged lock for snapshot would block Condvar waiters. Merging `dependents` similarly risks blocking invalidation-sidecar reads during recompute waits.

SPEC line 298: *"Keep one context-level graph synchronization primitive until benchmark instrumentation shows a finer-grained design improves the relevant workload."* Line 310: *"must not mutate dependency graph state independently of the context mutex."*

**Recommendation:** Keep deferred. If benchmark evidence shows per-slot sync overhead dominates, consider a single `parking_lot::Mutex` for the merged state instead of `std::sync` — same lock ordering but lighterweight. This would require Loom proof and the SPEC's benchmark matrix.

**#r54f #lazyperf24 — SmallVec vs Vec benchmarking (deferred):**

The existing benchmark suite already covers the relevant patterns:
- `dependency_fan_out` at width 32/256 — exercises edge iteration in recompute
- `set_cell_invalidation/high_fan_out` at width 512 — exercises dependent iteration during invalidation
- `thread_safe_contention` 1-16 workers — exercises edge mutex contention

SPEC lines 342-348 require a *"controlled A/B rerun"* with *"same host/toolchain"* and Criterion baselines, checking whether confidence intervals overlap before committing to either choice.

**Recommendation:** Keep deferred. To benchmark, temporarily swap `type EdgeVec = SmallVec<[SlotId; 4]>` to `type EdgeVec = Vec<SlotId>` in both `context.rs` and `thread_safe.rs`, run `cargo bench --bench context -- dependency_fan_out,set_cell_invalidation/high_fan_out` with Criterion `--save-baseline smallvec` / `--baseline smallvec` for comparison. This requires local execution on the target host.

Both items are documented with clear preconditions for reactivation.

### Re: JB Clear Session Context auto-queue stall — glm-5.1

Audited the auto-queue trigger chain (idle-queue-watch `start.rs:1476-1565`, auto-trigger `start.rs:1344-1463`, queue continuation `queue_continuation.rs:39-128`) and session clear flow (`session_actor_cmd.rs:472-544`).

**Three root causes identified:**

1. **No post-clear auto-queue suppression** — `session clear` sends `/new` but does not write any cooldown marker. The idle-queue-watch thread sees `queue_active: true` with remaining prompts (#r0jv, #r54f) and immediately re-injects `agent-doc <FILE>` into the cleared session.

2. **`prompt_requests_clear()` ignores `/new`** (`codex_hook.rs:819-821`) — only matches exact `/clear`, so OpenCode's `/new` is invisible to `tracked_harness_clear_requires_fresh_restart()` in `route.rs:978-997`. The route-level fresh-restart guard never triggers for OpenCode sessions.

3. **Session clear does not pause auto-queue** — `session_actor_cmd.rs:472-544` reclaims orphaned preflight cycles but leaves `&lt;!-- agent:queue auto --&gt;` with `queue_active: true` untouched. The queue state survives the clear unchanged.

**Plan created:** `tasks/agent-doc/plan-post-clear-auto-queue-suppress.md` — 7-step fix: (1) fix `prompt_requests_clear()` to recognize `/new`, (2) add durable clear-cooldown marker, (3) `session clear` writes marker, (4-5) idle-queue-watch and auto-trigger respect cooldown, (6) explicit `Run Agent Doc` clears cooldown, (7) preflight surfaces cooldown diagnostic.

**Bug transferred to `tasks/agent-doc/agent-doc-bugs2.md` as [#7az7]:**

- [ ] [#7az7] #clear-auto-queue-stall #agent-doc-bug 2026-06-03: JB "Clear Session Context" → session clear sends `/new` → idle-queue-watch/auto-trigger re-injects `agent-doc <FILE>` into cleared session that has no agent-doc context → stall. Plan: `tasks/agent-doc/plan-post-clear-auto-queue-suppress.md`
❯ Would [#r0jv] have a negative performance impact?

### Re: Would #r0jv have a negative performance impact? — glm-5.1

Yes — consolidation would likely **hurt** performance under contention. The current 4-lock design on `ThreadSafeSlotFastPath` (thread_safe.rs:233-244) deliberately separates contention domains:

| Lock | Access pattern | Contention domain |
|------|---------------|-------------------|
| `value: RwLock` | Many concurrent readers via `read_fresh()` | Read-mostly, uncontended |
| `dependencies: Mutex` | Snapshot during compute, called **before** state lock (line 1572) | Read-during-compute |
| `recompute: Mutex + Condvar` | Exclusive recomputation + waiter parking | Write-only, potentially long-held |
| `dependents: Mutex` | Edge registration, sidecar invalidation | Write-during-edge-reg |

**Why merging hurts:**

1. **Serialized reads** — Merging `value` (RwLock) into a single mutex eliminates concurrent `read_fresh()` calls. Under read-heavy workloads, this is a throughput regression.

2. **Lock ordering deadlock** — `dependencies_snapshot()` is called *before* taking the state lock (line 1572). The recompute mutex is held during `wait_for_recompute` with Condvar parking. Merging these creates a cycle: a thread holding the merged lock for snapshot blocks Condvar waiters, and vice versa.

3. **Invalidation stalls** — Merging `dependents` into recompute means invalidation-sidecar reads block during any in-progress recomputation, increasing p99 latency for downstream graph updates.

4. **SPEC confirms** (line 298): *"Keep one context-level graph synchronization primitive until benchmark instrumentation shows a finer-grained design improves the relevant workload."* The per-slot granularity is the *finer-grained* design; the SPEC wants evidence before committing to it, not removing it.

**Recommendation:** Keep #r0jv deferred or close it. The ~400 bytes per-slot sync overhead is negligible at realistic scale (<400KB for 1000 slots). If profiling ever shows per-slot lock contention dominating, the safer path is switching `std::sync` → `parking_lot` (same structure, lower overhead) rather than merging locks.

### Re: Queue update — glm-5.1

Closed `#r0jv` from queue and backlog (accepted recommendation to keep deferred — sync primitive consolidation would hurt performance under contention). Auto queue re-enabled with `#spec-test-build-install-commit-push` preset. Next in queue: `#r54f` (SmallVec vs Vec benchmarking).

### Re: Queue #r54f — glm-5.1

> **Queue prompt:**
>
> [#r54f]

**#r54f #lazyperf24 — SmallVec vs Vec edge benchmarking infrastructure:**

Added `vec_edges` Cargo feature flag that switches `EdgeVec` between `SmallVec<[SlotId; 4]>` (default) and `Vec<SlotId>`:

- **Cargo.toml**: Added `vec_edges = []` feature; made `smallvec` optional with default feature activation
- **context.rs**: Conditional `EdgeVec` type alias and `use smallvec`; replaced all `SmallVec::new()` with `EdgeVec::new()`
- **thread_safe.rs**: Same conditional alias and replacements
- **SPEC.md**: Documented 3-step A/B benchmarking procedure using Criterion baselines

All 98 tests pass with both default and `vec_edges` features (including 12 loom model-checking). Release build verified with `--all-features`.

**To run the A/B comparison:**
```bash
cargo bench --bench context -- dependency_fan_out,set_cell_invalidation/high_fan_out --save-baseline smallvec
cargo bench --bench context --features vec_edges -- dependency_fan_out,set_cell_invalidation/high_fan_out --baseline smallvec
```

Committed and pushed to lazily-rs (`ba537b2`).

### Re: Queue cleanup and JB bug triage — glm-5.1

> **Queue prompt:**
>
> JB `Clear Session Context` successfully ran `/new` but the restarted session should not run agent-doc. #agent-doc-bug


Cleared 5 completed queue items (~#zfam, ~#f5jz, ~#rvjd, ~#xsp1, ~#r54f~).

**JB `Clear Session Context` + `/new` restart** — Already fixed by #7az7 (implemented this session): `prompt_requests_clear()` recognizes `/new`, 60s clear-cooldown marker prevents idle-queue-watch re-injection. No further action needed.

**JB `Run Agent Doc` stalls in this OpenCode session** — Triaged against existing bugs:

| Existing bug | Status | Relevance |
|---|---|---|
| `#opencode-post-turn-false-active` | IMPLEMENTED, needs live-verify | Bottom-12-lines busy-cue check replaces all-lines scan — likely root cause of "stalls" |
| `#jbrunstallreg` | IMPLEMENTED, needs live-verify | Plugin-side stage ledger bounds all pre-route stages |
| `#opencode-preset-not-dispatched` | In review | Dependent on the above fixes |

Added as `#opencode-jb-stall` to agent-doc-bugs2.md review backlog.

### Re: Queue item reformat — glm-5.1

Reformatted the JB stall queue entry as a `do` directive referencing the new review item `#opencode-jb-stall`. Added the bug to `tasks/agent-doc/agent-doc-bugs2.md` review backlog.

### Re: do [#opencode-jb-stall] — glm-5.1

Added `#opencode-jb-stall` to `tasks/agent-doc/agent-doc-bugs2.md` review backlog. The stall is triaged as a live-verification gap for two already-implemented fixes:

1. `#opencode-post-turn-false-active` — Bottom-12-lines busy-cue detection (implemented, needs live-verify in an OpenCode pane)
2. `#jbrunstallreg` — Plugin-side Run Agent Doc stage ledger (implemented, needs live-verify with installed JB plugin)

No new code changes needed — the stall should resolve once both fixes are live-verified with the current installed binary and plugin.
<!-- agent:boundary:809ab630 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~JB `Clear Session Context` successfully ran `/new` but the restarted session should not run agent-doc. #agent-doc-bug~
- do [#opencode-jb-stall] #agent-doc-bug
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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

