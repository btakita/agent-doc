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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-175054.md`*

Compacted content:
- Archived 2 response topic(s): handle-centric get API; extract BENCHMARKS.md
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Does get(ctx) clone ctx? — glm-5.1

> **Queue prompt:**
>
> Does `let val = counter.get(ctx);` clone `ctx`?


**No.** `SlotHandle::get` takes `ctx: &Context` — an immutable borrow. The signature is:

```rust
// src/lazily-rs/src/slot.rs:26
pub fn get(&self, ctx: &Context) -> T
```

`Context` doesn't even implement `Clone` (it contains `RefCell<ContextInner>` with `Rc<ComputeFn>` closures). The only clone in `get` is the return value `T`, not the context itself.

When you write `counter.get(ctx)`, Rust auto-refs to `counter.get(&ctx)` — pure borrow, zero cost on the context side.

### Re: test — glm-5.1

Test content.
<!-- agent:boundary:63b908da -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~Does `let val = counter.get(ctx);` clone `ctx`?~
- [#lazyperf33]
- Please create a mermaid of the performance improvements.
- [#lazyperf34]
- [#lazyperf35]
- [#lazyperf36]
- Is there a way to make "Fix finish_batch O(n2) dedup" an O(n) or O(1) algorithm?
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
