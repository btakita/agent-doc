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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-172441.md`*

Compacted content:
- Archived 11 response topic(s): #71yq lazyperf25; #hqmh lazyperf26; #xdq8 lazyperf27; 8 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: handle-centric get API — glm-5.1

> **Queue prompt:**
>
> In the README, we have `let val = ctx.get_cell(&counter);`. Please make the canonical usage `counter.get(&ctx)` instead of `ctx.get_cell(&counter)`. Is a Cell a Slot? If so, how can we differentiate from `Slot::get` behavior?


**Cell is NOT a Slot.** They're distinct node types (`CellNode` vs `SlotNode`) sharing only a `SlotId` and `dependents` edge structure. Since `CellHandle<T>` and `SlotHandle<T>` are different Rust types, each can have its own `get(&self, ctx)` method with no collision.

**Changes made** (commit `5817e61`, pushed):

- Added `CellHandle::get(&self, ctx) -> T` — ergonomic alias for `ctx.get_cell(self)` with full dependency tracking
- Added `SlotHandle::get(&self, ctx) -> T` — ergonomic alias for `ctx.get(self)` for API consistency
- Updated README examples to use `counter.get(&ctx)` and `doubled.get(&ctx)` as canonical
- Updated README and SPEC.md API tables with both handle-centric and context-centric entries
- Added 8 new tests in `handle_get_methods` module

**Note:** The handle-centric `get`/`set` methods are on `Context` only. `ThreadSafeContext` continues to use `ctx.get_cell()` / `ctx.get()` — the same split as the existing `cell.set()` which only takes `&Context`.

All 83 spec compliance tests pass, including loom and tokio. `make check` clean.

### Re: extract BENCHMARKS.md — glm-5.1

> **Queue prompt:**
>
> Extract a BENCHMARKS.md from the README.md. All benchmarks, including explanations, should go into BENCHMARKS.md. README.md should link to BENCHMARKS.md.


Extracted all benchmark data from README.md into [BENCHMARKS.md](src/lazily-rs/BENCHMARKS.md).

**Changes** (commit `567b86f`, pushed):

- Created `BENCHMARKS.md` with all benchmark results, regression budgets, lock attribution, instrumentation profiles, and the Multi-Language/Related/License sections
- README.md now has a concise "## Benchmarks" section linking to BENCHMARKS.md
- Updated `scripts/update-benchmark-results.py` to write to `BENCHMARKS.md` via `--benchmarks-file` flag (default: `BENCHMARKS.md`)
- Updated spec compliance tests to read BENCHMARKS.md for benchmark assertions

`make check` clean.
<!-- agent:boundary:5f750485 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~In the README, we have `let val = ctx.get_cell(&counter);`. Please make the canonical usage `counter.get(&ctx)` instead of `ctx.get_cell(&counter)`. Is a Cell a Slot? If so, how can we differentiate from `Slot::get` behavior?~
- ~Extract a BENCHMARKS.md from the README.md. All benchmarks, including explanations, should go into BENCHMARKS.md. README.md should link to BENCHMARKS.md.~
- [#lazyperf33]
- [#lazyperf34]
- [#lazyperf35]
- [#lazyperf36]
- Update BENCHMARKS.md. What are the #next-steps to mitigate the parking_lot regressions?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lazyperf33] [recommended] Hybrid InvalidationPlan: Vec for 16 or fewer nodes, HashMap above threshold — fixes fan_out/256, memo_equality, chain-pattern regressions
- [ ] [#lazyperf34] [recommended] Fix finish_batch O(n2) dedup: replace edge_insert loop with extend+sort-dedup or small HashSet — fixes batched_write_bursts, batch_storms
- [ ] [#lazyperf35] [recommended] Eliminate scratch buffer collect allocation on effect path: pass fill_dependent_scratch result as slice to invalidate_frontier_locked — fixes effect_flushing
- [ ] [#lazyperf36] [recommended] A/B benchmark parking_lot vs std::sync for moderate contention: evaluate fair_mutex or revert main state mutex — fixes same_slot_write_read, independent_slots
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
