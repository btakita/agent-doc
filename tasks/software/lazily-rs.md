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
<!-- agent:boundary:67dec853 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#zfam] #lazyperf17 Replace HashSet<SlotId> dependency/dependent edges with SmallVec<[SlotId;4]> — high impact: reduces per-node memory, eliminates hash overhead for typical small fan-out (1-3 deps), avoids HashSet→Vec temporary allocations in every refresh/clear/dirty hot path
- [ ] [#f5jz] #lazyperf18 Pre-allocate or eliminate temporary Vec<SlotId> allocations in invalidation hot paths — refresh_slot, clear_slot_now, mark_slot_dirty, invalidate_cell_dependents_now all .iter().copied().collect() into temp Vecs; use SmallVec or pass iterator slices instead
- [ ] [#rvjd] #lazyperf19 Consolidate Context RefCell fields into single RefCell<ContextInner> — reduces borrow-check overhead from 4-6 flag checks per get() to 1; single borrow scope eliminates re-entrant borrow risk
- [ ] [#xsp1] #lazyperf20 Switch ThreadSafeState node storage from HashMap<SlotId, Node> to slot-id-indexed Vec<Option<Node>> matching single-threaded Context — eliminates hash lookup on every node access in thread-safe path
- [ ] [#r0jv] #lazyperf21 Consolidate ThreadSafeSlotFastPath synchronization primitives — currently 4 mutexes + 2 Condvars + 3 atomics per slot (~400+ bytes overhead); restructure to 1-2 locks + atomics
- [ ] [#ktrs] #lazyperf22 Add slot ID reuse or vacant-entry compaction for long-lived contexts — SlotId is monotonically increasing, disposed effects leave None entries that grow Vec without bound
- [ ] [#vftn] #lazyperf23 [recommended] Replace unsafe pointer-to-Box copies in recompute_slot_now/run_effect with Arc<ComputeFn> or safe restructuring
- [ ] [#r54f] #lazyperf24 [recommended] Benchmark SmallVec vs Vec for dependency edges — measure cache-line impact on fan-out benchmarks before committing to SmallVec
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

