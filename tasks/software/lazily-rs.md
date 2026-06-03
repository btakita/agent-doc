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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-152451.md`*

Compacted content:
- Archived 13 response topic(s): #next-steps to improve performance and reduce memory usage; Vec vs HashMap lookups; Queue #zfam #fftjz #rvjd #vftn; 10 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to improve performance and reduce memory usage?

### Re: #next-steps to improve performance and reduce memory usage — glm-5.1

Audited the full post-optimization codebase (context.rs, thread_safe.rs, instrumentation.rs). All prior improvements (#lazyperf17–24) are landed and effective. Here are the remaining opportunities:

**High priority — thread-safe fast-path registries:**
- `#lazyperf25`: Convert `ThreadSafeInner.slot_fast_paths` and `cell_fast_paths` from `RwLock<HashMap<SlotId, Arc<...>>>` to `RwLock<Vec<Option<Arc<...>>>>`. Every `get()` / `get_cell()` call does a RwLock read + HashMap lookup. SlotIds are dense integers — the same Vec-indexed approach from #lazyperf20 eliminates hash computation on every thread-safe value access. Two HashMaps → two Vecs, same helper pattern as `ThreadSafeState.get_node`.

**High priority — sidecar dependents as EdgeVec:**
- `#lazyperf26`: Convert `ThreadSafeSlotFastPath.dependents` (thread_safe.rs:247) and `ThreadSafeCellFastPath.dependents` (thread_safe.rs:512) from `Mutex<HashMap<SlotId, ThreadSafeDependentKind>>` to `Mutex<EdgeVec>` with a parallel `Mutex<EdgeVec>` for kinds (or `Mutex<Vec<(SlotId, ThreadSafeDependentKind)>>`). Dependents per node are typically 1-5, so HashMap overhead (hash + bucket) dominates. `dependents_snapshot()` returns `Vec<(SlotId, ThreadSafeDependentKind)>` — the conversion becomes a simple clone instead of HashMap → Vec iteration.

**Medium priority — scheduled_effects as Vec<bool>:**
- `#lazyperf27`: Replace `scheduled_effects: HashSet<SlotId>` in both `ContextInner` and `ThreadSafeState` with `Vec<bool>` indexed by SlotId. `scheduled_effects.insert(id)` → `vec[id] = true`, `.remove()` → `vec[id] = false`, `.contains()` → `vec[id]`. O(1) with zero hash computation. The Vec grows with `nodes` so resize is already bounded. Trade-off: ~1 byte per slot vs ~50 bytes per HashSet entry, but eliminates all hash overhead from the hot `schedule_effect` path.

**Medium priority — batch sets as SmallVec:**
- `#lazyperf28`: Replace `batched_cells: HashSet<SlotId>`, `batched_cell_clears: HashSet<SlotId>`, `batched_slots: HashSet<SlotId>` with `SmallVec<[SlotId; 8]>`. Typical batches touch few nodes. Linear dedup (`push` + `contains` check) is faster than hash for ≤8 items. `drain()` on SmallVec is zero-copy iteration.

**Medium priority — ThreadSafeInvalidationPlan Vecs:**
- `#lazyperf29`: Replace `slots_to_mark: HashMap<SlotId, bool>` and `effects_to_schedule: HashMap<SlotId, bool>` in `ThreadSafeInvalidationPlan` with `Vec<(SlotId, bool)>` + sorted insert. The plan is built in one pass and applied in one pass — HashMap overhead per frontier node is unnecessary for a transient structure.

**Lower priority:**
- `#lazyperf30`: Dependent clone scratch buffer — pre-allocate a `Vec<SlotId>` on `ContextInner` / `ThreadSafeState` for the clone-and-iterate pattern in `clear_slot_now`, `invalidate_cell_dependents_now`, `notify_slot_value_changed`, avoiding SmallVec heap allocation on >4 fan-out.
- `#lazyperf31` [recommended]: `parking_lot` replacement — swap `std::sync::Mutex/RwLock` in thread_safe.rs for `parking_lot` equivalents (smaller lock size, no poisoning overhead, adaptive spinning). Requires benchmark validation per SPEC line 298.
- `#lazyperf32` [recommended]: `Rc<dyn Any>` values in single-threaded `Context` — storing `Rc` instead of `Box` for `SlotNode.value` would make `get()` return an `Rc` clone (pointer increment) instead of a deep clone. This is an API change (`get` returns `Rc<T>` or a new `get_rc` method).

Top of backlog: **`#lazyperf25`** (thread-safe fast-path Vec registries)
<!-- agent:boundary:c509ecec -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#71yq] #lazyperf25 #lazyperf25 Convert ThreadSafeInner slot_fast_paths and cell_fast_paths from RwLock<HashMap> to RwLock<Vec<Option>> — eliminates hash on every thread-safe get/get_cell
- [ ] [#hqmh] #lazyperf26 #lazyperf26 Convert ThreadSafeSlotFastPath.dependents and ThreadSafeCellFastPath.dependents from Mutex<HashMap> to Mutex<EdgeVec> — sidecar dependents are typically 1-5 nodes
- [ ] [#xdq8] #lazyperf27 #lazyperf27 Replace scheduled_effects HashSet with Vec<bool> indexed by SlotId — zero-hash O(1) schedule/contains check
- [ ] [#vk55] #lazyperf28 #lazyperf28 Replace batched_cells/cell_clears/slots HashSet with SmallVec — linear dedup faster for typical small batches
- [ ] [#cffp] #lazyperf29 #lazyperf29 Replace ThreadSafeInvalidationPlan HashMaps with Vec<(SlotId, bool)> — transient plan structure, one-pass build/apply
- [ ] [#vbcq] #lazyperf30 #lazyperf30 Dependent clone scratch buffer on ContextInner/ThreadSafeState — avoid SmallVec heap alloc on >4 fan-out in refresh/clear paths
- [ ] [#vfr1] #lazyperf31 #lazyperf31 [recommended] parking_lot replacement for std::sync Mutex/RwLock — smaller locks, no poisoning, adaptive spinning
- [ ] [#94gg] #lazyperf32 #lazyperf32 [recommended] Rc<dyn Any> values in single-threaded Context — get() returns pointer increment instead of deep clone
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

