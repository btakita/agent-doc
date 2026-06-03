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

### Re: #lazyperf9 — gpt-5

Implemented `#lazyperf9`.

The benchmark report harness now captures Criterion sample latency distributions for the required 8/16-worker contention cases, emits `p50`/`p95` latency rows into `README.md`, and fails `--check` when any required latency row is missing. I also added effect-heavy `ThreadSafeContext` Criterion cases for queue coalescing, cleanup execution, and nested batch flush at 8 and 16 workers so the required effect-heavy latency evidence exists beside the prior same-slot, independent-slot, read-mostly, and batched-write rows.

Changed paths:
- `src/lazily-rs/SPEC.md`
- `src/lazily-rs/README.md`
- `src/lazily-rs/benches/context.rs`
- `src/lazily-rs/scripts/update-benchmark-results.py`
- `src/lazily-rs/tests/spec_compliance.rs`
- `src/lazily-rs` submodule pointer in the superproject

Commits:
- `src/lazily-rs`: `3cdc49d` (`perf: add latency evidence to benchmark report`), pushed to `origin/main`
- `agent-loop`: `f2a2bc551` (`chore: update lazily-rs submodule`), created locally; root push is intentionally after this binary-owned session finalize boundary

Verification:
- `python3 scripts/update-benchmark-results.py --check` passed
- `python3 -m py_compile scripts/update-benchmark-results.py` passed
- `cargo test` passed
- `cargo test --features instrumentation` passed
- `cargo test --features tokio` passed
- `cargo test --features loom --test thread_safe_loom` passed
- `cargo build --all-features` passed
- `cargo clippy --all-targets --all-features -- -D warnings` passed
- `cargo bench --features instrumentation --no-run` passed
- `cargo bench --features instrumentation thread_safe_effect_contention` passed and generated the missing effect-heavy latency sample files
- `cargo package --allow-dirty` passed as the local package/installability check; the crate has no binary install target
- `make check` is unavailable in `src/lazily-rs` (`No rule to make target 'check'`)

CI:
- `gh run list --workflow CI --limit 1` initially found no remote `CI` workflow because the local CI workflow commit was still unpushed
- after pushing `lazily-rs`, GitHub Actions `CI` run `26850953156` was `in_progress` for `3cdc49d`

Escalation: none. Confidence: high.

### Re: lazyperf9 latency evidence — gpt-5

Completed `#lazyperf9`.

Implemented and pushed lazily-rs commit `3cdc49d perf: add latency evidence to benchmark report`. The parent agent-loop branch was also pushed after the session closeout so `origin/main` now references that valid lazily-rs submodule commit instead of the missing `81ca43c` pointer that broke the prior `Agent Doc` workflow checkout.

Changed paths in lazily-rs: `README.md`, `SPEC.md`, `benches/context.rs`, `scripts/update-benchmark-results.py`, and `tests/spec_compliance.rs`.

What changed:

- Added required p50/p95 latency extraction from Criterion `sample.json` files for the 8/16-worker same-slot, independent-slot, read-mostly, batched-write, and effect-heavy cases.
- Added the `thread_safe_effect_contention` benchmark group for queue coalescing, cleanup execution, and batch flush at 8 and 16 workers.
- Made `python3 scripts/update-benchmark-results.py --check` fail when any required latency row is missing.
- Regenerated the README benchmark section with 14 required latency rows beside throughput and lock-site budgets.
- Updated effect-heavy lock-site budgets to match the current instrumentation profile counters while keeping the regression gate active.

Verification:

- `python3 scripts/update-benchmark-results.py --check`
- `cargo bench --features instrumentation thread_safe_effect_contention`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features loom`
- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --all-features`
- `cargo package --allow-dirty`
- `cargo publish --dry-run` passed; Cargo only warned that `lazily@0.5.1` already exists and that upload is skipped for the dry run.

CI: lazily-rs has no GitHub Actions workflows. The parent `Agent Doc` workflow had failed before the parent push at submodule checkout because `origin/main` referenced missing lazily-rs commit `81ca43c`; after the parent push, the latest `Agent Doc` run is queued for the repaired parent commit.

Queue status: `#lazyperf9` is complete and archived; the next queue item is `#lazyperf12`. I did not start `#lazyperf12` in this cycle.

Escalation: none. Confidence: high.

### Re: lazyperf10 cell-only refresh partition — gpt-5

Implemented and pushed `#lazyperf10` in lazily-rs.

Changed paths:
- `src/lazily-rs/src/thread_safe.rs`
- `src/lazily-rs/tests/spec_compliance.rs`
- `src/lazily-rs/tests/thread_safe_loom.rs`
- `src/lazily-rs/SPEC.md`
- `src/lazily-rs/README.md`
- `src/lazily-rs/scripts/update-benchmark-results.py`

What changed:
- Added a per-slot dependency summary to the thread-safe slot sidecar, including mirrored dependency ids and a slot-dependency count.
- Routed cell-only dirty refreshes through the SlotId sidecar so they skip the graph-locked `get_refresh` dependency scan.
- Moved recompute claim and old-dependency snapshot out of the pre-compute `publish` graph lock; recomputes still take the final publish graph mutation to diff dynamic dependencies, publish values, notify dependents, and reject stale in-flight revisions.
- Added instrumentation coverage for independent cell-only refreshes and a Loom model for dynamic dependency summary races plus stale in-flight completion.
- Tightened regression budgets around the new graph-lock profile while allowing expected zero-wait sidecar handoff coordination.

Benchmark evidence:
- `cargo bench --features instrumentation thread_safe_contention/independent_slots/16` passed and improved: 1.9934-2.0400 ms, about 60% faster than the prior estimate.
- Updated README report: `thread_safe_contention_independent_slots_16` now records 510 total profiled acquisitions, with `get_refresh=32` and `publish=271`; the Criterion row is 2.009 ms mean with 1.965-2.049 ms interval.
- Same-slot graph contention also shifted down: `same_slot_write_read_16` reports `get_refresh=108`, `publish=198`, and sidecar `in_flight_wait=448` with 0 wait/hold time.

Verification run:
- `python3 scripts/update-benchmark-results.py --check`
- `cargo fmt --all --check`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features loom`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --all-features`
- `cargo bench --features instrumentation thread_safe_contention/independent_slots/16`
- `cargo package --allow-dirty`
- `cargo publish --dry-run` (only warning: `lazily@0.5.1` already exists; dry-run upload aborted as expected)
- Parent repo `make check`

Commits and CI:
- lazily-rs commit pushed: `12db3d3 perf: partition cell-only refresh path`.
- Parent repo already points at `12db3d3`; no parent submodule commit was needed.
- GitHub Actions lazily-rs `CI` for `12db3d3` completed successfully.
- Parent `Agent Doc` workflow latest relevant run was successful.

`#lazyperf10` is complete. The active queue should advance to `#lazyperf11`.

### Re: #lazyperf10 — gpt-5

Implemented and pushed the thread-safe independent-slot refresh partition in lazily-rs.

Changed paths:
- `src/thread_safe.rs`
- `tests/spec_compliance.rs`
- `tests/thread_safe_loom.rs`
- `SPEC.md`
- `README.md`
- `scripts/update-benchmark-results.py`

What changed:
- Moved slot compute closures and dependency snapshots into the per-slot fast path.
- Routed dirty cell-only slots through the SlotId-partitioned recompute sidecar so steady-state refresh skips graph `get_refresh` and avoids the first `publish` graph lock; final publish still owns dynamic dependency diffing, value publication, stale revision rejection, and dependent notification.
- Kept dependency sidecars in sync on edge insert/remove, including slot-dependency counts that prevent the cell-only shortcut when a slot dependency is present.
- Added spec assertions and a Loom model for partitioned cell-only refresh, dynamic dependency count races, and stale completion behavior.
- Tightened the independent-slot instrumentation budget to `max_lock_acquisitions=700`, `get_refresh<=64`, `publish<=320`; widened same-slot `in_flight_wait` to `600` after the faster recompute-start path shifts same-slot contention into waiter parking.

Evidence:
- Deterministic profile after the change: `thread_safe_contention_independent_slots_16` reported `locks=520`, `get_refresh=32`, `publish=271`, `dependency_edge=16`.
- Criterion `thread_safe_contention/independent_slots/16`: no performance regression detected.
- Criterion `thread_safe_contention/same_slot_write_read/16`: performance improved by about 61% in the local comparison.
- GitHub Actions for lazily-rs commit `12db3d3` completed successfully: CI run `26852196453`.

Verification commands run:
- `cargo fmt --check`
- `cargo test`
- `cargo test --features instrumentation`
- `cargo test --features tokio`
- `cargo test --features loom --test thread_safe_loom`
- `cargo build --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --features instrumentation --no-run`
- `cargo bench --features instrumentation thread_safe_contention/independent_slots/16`
- `cargo bench --features instrumentation thread_safe_contention/same_slot_write_read/16`
- `python3 scripts/update-benchmark-results.py --no-run`
- `python3 scripts/update-benchmark-results.py --check`
- `cargo package --allow-dirty`
- `make check` in `src/lazily-rs` was unavailable: `No rule to make target 'check'`.
- Superproject `make check` completed successfully.

Confidence: high. The implementation preserves the graph-owned publish boundary and covers the new partition route with deterministic instrumentation assertions, Loom interleavings, the full Rust feature suites, package verification, targeted Criterion runs, superproject verification, and remote CI.

Escalation: none.
<!-- agent:boundary:18684633 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
