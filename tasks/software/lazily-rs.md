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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-155704.md`*

Compacted content:
- Archived 2 response topic(s): #next-steps to improve performance and reduce memory usage; Run benchmarks and update tables in README.md
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: #71yq lazyperf25 — glm-5.1

> **Queue prompt:**
>
> [#71yq]


Converted `ThreadSafeInner::slot_fast_paths` and `cell_fast_paths` from `RwLock<HashMap<SlotId, Arc<…>>>` to `RwLock<Vec<Option<Arc<…>>>>` in `src/thread_safe.rs`.

**Changes:**
- Type declarations (lines 839-840): `HashMap` → `Vec<Option>`
- Init (lines 853-854): `HashMap::new()` → `Vec::new()`
- Read methods `slot_fast_path` / `cell_fast_path`: `.get(&id).cloned()` → `.get(id.0 as usize).and_then(|opt| opt.as_ref().cloned())`
- Write in `slot_with_equals` / `cell`: `.insert(id, fast_path)` → resize-with-None + direct index set

**Verification:** `make check` passes (fmt, clippy, build, 92 default tests, 2 tokio tests, 12 loom concurrency tests, benchmark check). Release build clean. Pushed as `ac0eb4b`.

### Re: #hqmh lazyperf26 — glm-5.1

> **Queue prompt:**
>
> [#hqmh]


Converted `ThreadSafeSlotFastPath.dependents` and `ThreadSafeCellFastPath.dependents` from `Mutex<HashMap<SlotId, ThreadSafeDependentKind>>` to `Mutex<DependentEdgeVec>` (SmallVec for typical 1-5 node fan-out).

**Changes in `src/thread_safe.rs`:**
- Added `DependentEdgeVec` type alias: `SmallVec<[(SlotId, ThreadSafeDependentKind); 4]>` / `Vec<(…)>`
- Added `dependent_edge_insert` / `dependent_edge_remove` helper functions
- Updated both struct fields, constructors, `insert_dependent`, `remove_dependent`, `dependents_snapshot` methods

**Verification:** `make check` passes (fmt, clippy, 92 default tests, 2 tokio tests, 12 loom tests, benchmark check). Pushed as `5f4c76e`.

### Re: #xdq8 lazyperf27 — glm-5.1

> **Queue prompt:**
>
> [#xdq8]


Replaced `ThreadSafeState::scheduled_effects` from `HashSet<SlotId>` to `Vec<bool>` indexed by `SlotId.0 as usize`. Zero-hash O(1) schedule/deschedule/contains.

**Changes:**
- Field type: `HashSet<SlotId>` → `Vec<bool>`
- Added `deschedule_effect()` and `is_effect_scheduled()` helper methods on `ThreadSafeState`
- Updated `schedule_effect_locked`, `dispose_effect`, `flush_effects`, and test helper

**Verification:** `make check` passes (fmt, clippy, 92 default tests, 2 tokio, 12 loom). Pushed as `042246a`.

### Re: #vk55 lazyperf28 — glm-5.1

> **Queue prompt:**
>
> [#vk55]


Replaced `batched_cells`, `batched_cell_clears`, `batched_slots` in `ThreadSafeState` and `ThreadSafeBatchChanges` from `HashSet<SlotId>` to `EdgeVec` (SmallVec with 4-inline capacity).

**Changes:**
- Field types: `HashSet<SlotId>` → `EdgeVec` in both `ThreadSafeState` and `ThreadSafeBatchChanges`
- Insert operations: `.insert(id)` → `edge_insert(&mut vec, id)` (linear dedup)
- Merge in `finish_batch`: `.extend()` → per-element `edge_insert`
- Drain in `flush_batched_invalidations`: `.drain().collect()` → `std::mem::take(..).into_iter().collect()`

**Verification:** `make check` passes (fmt, clippy, 92 tests, 2 tokio, 12 loom). Pushed as `d2282f6`.

### Re: #cffp lazyperf29 — glm-5.1

> **Queue prompt:**
>
> [#cffp]


Replaced `ThreadSafeInvalidationPlan` HashMaps/HashSets with `Vec<(SlotId, bool)>` / `Vec<SlotId>`. Transient plan uses linear scan — faster for typical small node counts.

**Struct changes:** 6 fields → 3 (`slot_marks: Vec<(SlotId, bool)>`, `slot_clears: Vec<SlotId>`, `effect_schedules: Vec<(SlotId, bool)>`)

**Method changes:**
- `from_roots_locked`: local `requested_force` and `simulated_slots` converted from HashMap to Vec with linear find/set helpers
- `apply_locked`: iterates plan Vecs directly
- `add_slot_mark`/`add_effect_schedule`: linear find-or-push
- `add_slot_clear`: linear contains check
- Added `enqueue_root`, `find_force`, `find_simulated`, `set_simulated` helper methods

**Verification:** `make check` passes (fmt, clippy, 92 tests, 2 tokio, 12 loom). Pushed as `9907f6f`.
<!-- agent:boundary:6d4d2f9f -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~[#71yq]~
- ~[#hqmh]~
- ~[#xdq8]~
- ~[#vk55]~
- ~[#cffp]~
- [#vbcq]
- [#vfr1]
- [#94gg]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
