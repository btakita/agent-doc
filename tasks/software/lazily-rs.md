---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260602-205356.md`*

Compacted content:
- Archived 2 response topic(s): Add lazily-rs CI; Add a gh action for CI on lazily-rs.
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to optimize concurrency in lazily-rs? Can we reduce the lock contention further? What is actionable from [#lazyperf8]?

### Re: lock contention next steps — gpt-5

Yes, there is still lock contention to reduce, but the actionable path from `#lazyperf8` is evidence-first rather than another broad synchronization rewrite.

`#lazyperf8` made synchronization replacement release-gated on throughput, p50/p95 latency, lock-site budgets, and Loom/Shuttle proof. The current README report has throughput means/CI and deterministic lock-site budgets, but it still does not emit the p50/p95 latency evidence that the gate now requires. That should be the next item, because without it we cannot honestly compare current `std::sync` mutex/Condvar, parking, targeted CAS, or any sharded graph prototype.

The remaining hotspots worth attacking after that are specific:

1. `thread_safe_contention_independent_slots_16` is still graph-lock shaped: the generated report shows `5.907 ms` mean, with `get_refresh` at `542` acquisitions / `6.993 ms` wait and `publish` at `542` acquisitions / `9.334 ms` wait. That is the best candidate for a bounded partitioning or split-publish experiment keyed by `SlotId`.
2. `thread_safe_effect_contention_batch_flush_16` and `queue_coalescing_16` still carry high `other` lock wait (`13.813 ms` and `6.309 ms` respectively in the current report). Effects are riskier than slot reads because cleanup/disposal ordering is correctness-sensitive, so this should be a targeted effect-drain/schedule coalescing item, not a lock-free rewrite.
3. Same-slot write/read can probably be improved at the margins, but it is inherently serialized by same-root mutation and stale-completion semantics. I would not make that the next design axis unless the new latency data shows a clear tail-latency regression from wakeup handoff rather than expected serialization.

I added the backlog in priority order. Top backlog item: `#lazyperf9`.

Changed paths: session document/backlog only. No lazily-rs source changes were made, so I did not run the Rust test suite for this response cycle.
❯ Can we use a "shadow" graph to quickly understand the invalidation structure, then run the invalidations in one batch?

### Re: shadow invalidation graph — gpt-5

Yes, but I would frame it as an explicit invalidation plan, not a second source-of-truth graph.

The current implementation already has the first version of this idea. `invalidate_frontier_locked` builds a frontier work queue, uses `simulated_slots` to understand which nodes would become dirty/forced, coalesces duplicate diamond paths, collects `slots_to_mark` and `effects_to_schedule`, then applies dirty/revision/effect scheduling in one mutation phase. `flush_batched_invalidations` also drains all changed cells at outermost batch exit and runs one coalesced frontier invalidation. Separately, `try_mark_slot_frontier_fast` uses sidecar dependents for slot-only changed-cell invalidation without taking the graph mutex.

The next useful step would be making that implicit frontier computation into a named `InvalidationPlan` or shadow-plan structure:

1. Snapshot only the dependency frontier needed for the roots: slot dependents, effect dependents, current dirty/force flags, and root force upgrades.
2. Compute the plan from that snapshot: ordered slots to dirty, forced slots, effects to schedule, hard clears, and whether the plan must fall back because callbacks/disposal/effects/dynamic dependencies make the snapshot unsafe.
3. Apply the plan in one short graph-lock batch: bump revisions, mirror fast-path dirty state, schedule effects once, then release the lock and flush effects.

That can reduce lock hold time because traversal, duplicate coalescing, and force propagation stop being mixed with mutation. It also gives us a clear unit to benchmark and model. The caution is that the shadow plan cannot be authoritative across user callbacks, effect disposal, or dependency discovery. If a dependency edge can change while planning, the plan must either hold the graph lock for that snapshot or carry a revision/epoch check and fall back to the existing locked path.

So the answer is: yes, it is actionable, and it is probably the right shape before a broader sharded-lock rewrite. I added `#lazyperf12` immediately after `#lazyperf9`, because latency evidence should still come first, then the shadow invalidation plan can target the remaining batch/effect lock-hold hotspots before `#lazyperf10` tries broader partitioning.

Top backlog item remains `#lazyperf9`.

Changed paths: session document/backlog only.
Commands: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `tsift index --workspace .`, targeted source/spec reads.
Verification: `agent-doc finalize` will run commit/session guards; no Rust tests were run because no lazily-rs source changed.
Escalation: none. Confidence: high.
<!-- agent:boundary:6aac4a59 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- [#lazyperf10]
- [#lazyperf11]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [x] [#lazyperf10] Reduce remaining independent-slot ThreadSafeContext publish/get_refresh lock contention: profile thread_safe_contention_independent_slots_16 with the new latency rows, then prototype splitting or partitioning the publish/get_refresh graph-mutation path by SlotId while preserving dependency-edge, effect, batch, and stale-completion semantics under Loom.
- [ ] [#lazyperf11] Reduce effect-heavy ThreadSafeContext lock contention after #lazyperf8: target thread_safe_effect_contention_batch_flush_16 and queue_coalescing_16, especially the high other/dependency_edge waits, by coalescing effect drain/schedule mutations or adding effect-sidecar state without weakening cleanup/disposal ordering.
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
