---
agent_doc_session: 9386d273-2288-498c-b9fc-207f224f9fdb
resume: 019e8608-2fbe-7410-abb8-7538fa69d044
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260602-182329.md`*

Compacted content:
- Archived 8 response topic(s): contention performance next steps; #lazyperf1; #lazyperf2; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to reduce lock contention? Would Condvar or a lock-free (CAS) be more performant with high parallelization?

### Re: lock contention next steps — gpt-5

Top of backlog: `#lazyperf5`.

For the current `ThreadSafeContext`, a Condvar or parking path is the better fit for same-slot waiter contention; targeted CAS is the better candidate for narrow read/publish fast paths. A full lock-free graph is still not the right next step.

The reason is that the graph update is not a single atomic fact. `set_cell`, dependency repair, dirty propagation, effect scheduling, cleanup, and batch boundaries all mutate related state. Turning that into lock-free CAS would require a much larger linearizability and reclamation proof, and the likely result is more complexity than speed. CAS can still help where there is one clear linearization point: electing the recompute owner, checking a slot revision, publishing a value if the revision is unchanged, or validating an optimistic cached read.

The current code already moved in that direction: per-slot sidecar state uses atomics/RwLock plus a Condvar for recompute coordination, while the graph lock remains the correctness boundary for multi-node state. So the next performance work should not be "Condvar vs CAS" globally. It should be workload-specific:

1. `#lazyperf5` — narrow recompute wakeups first. The current Condvar path avoids graph-mutex spinning, but `notify_all` can still create a wakeup stampede at high parallelism. Compare `notify_one`, explicit waiter counts, or parking against the current per-slot Condvar.
2. `#lazyperf6` — then try a versioned optimistic cached-read path. This is where CAS/atomics are most likely to win for read-mostly and independent-slot workloads, as long as dependency revision visibility and fallback semantics are documented.
3. `#lazyperf7` — separately profile effect-heavy contention before changing queues or batch flush behavior. Effects make lock-free designs much riskier because dropped or duplicated reruns are correctness bugs, not just performance artifacts.
4. `#lazyperf8` — keep a fixed adoption gate for all synchronization prototypes: throughput, p50/p95 latency, lock-site acquisition/wait/hold budgets, and Loom/spec coverage.

My expectation: under high parallelization, Condvar/parking wins for contended same-slot recompute waiters when compute work is non-trivial; targeted CAS/optimistic reads can win for cached reads and independent slots; whole-graph lock-free CAS is unlikely to beat a sharded or sidecar design without unacceptable correctness risk.

Changed paths: session document backlog/exchange only. Commands: `tsift status`, `tsift status --fix`, `tsift index --workspace .`, `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, source/context inspection commands, and this `agent-doc finalize`. Verification: prompt-response cycle only; no source tests run because no code changed. Confidence: high. Escalation: none.

### Re: #lazyperf5 — gpt-5

> **Queue prompt:**
>
> [#lazyperf5]


Completed `#lazyperf5`.

Implemented waiter-counted handoff wakeups for `ThreadSafeContext` recompute waiters in `src/lazily-rs`. The runtime now keeps a per-slot waiter count, has recompute finishers call `notify_one` instead of `notify_all`, and has each awakened waiter notify the next parked waiter after observing the completed sidecar state. That preserves the existing per-slot Condvar correctness envelope while avoiding a completion-wide wakeup stampede.

Updated `SPEC.md` to document the `notify_one` handoff contract, added spec-compliance assertions for the documented behavior, and expanded the Loom sidecar model with `recompute_handoff_notification_drains_multiple_waiters`. The model now covers the case that would deadlock if a pure `notify_one` woke only the first waiter.

Commits and push:

- `btakita/lazily-rs@af2d073` `perf: narrow recompute waiter wakeups` pushed to `origin/main`.
- `agent-loop@975cf9c77` `chore: update lazily-rs submodule` records the submodule pointer update; superproject push follows this document finalize.

Verification:

- `cargo fmt`
- `cargo test --features loom --test thread_safe_loom`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features instrumentation readme_benchmark_results_cover_required_scenarios`
- `cargo test --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --all-targets --all-features`
- `python3 scripts/update-benchmark-results.py --check`
- `cargo package --allow-dirty`
- root `make check`

Install note: `lazily` is a library crate with no binary target or install script, so `cargo package --allow-dirty` was the applicable local install/package validation boundary.

CI evidence: `gh run list --workflow CI --limit 1` reports no workflow named `CI` in both `btakita/lazily-rs` and the superproject. The superproject has an `Agent Doc` workflow; its latest run was `in_progress` when checked.

Changed paths in `lazily-rs`: `SPEC.md`, `src/thread_safe.rs`, `tests/spec_compliance.rs`, `tests/thread_safe_loom.rs`. Changed path in `agent-loop`: `src/lazily-rs`. Confidence: high. Escalation: none.
<!-- agent:boundary:6c8713f7 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#lazyperf5]~
- [#lazyperf6]
- [#lazyperf7]
- [#lazyperf8]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lazyperf6] Prototype a versioned optimistic cached-read path for read-mostly and independent-slot workloads, falling back under dirty/dependency/callback/batch ambiguity and proving visibility semantics before adoption.
- [ ] [#lazyperf7] Add effect-heavy contention profiles before changing synchronization: isolate effect queue coalescing, cleanup execution, and batch flush behavior at 8/16 workers, then decide between sharding and keeping the graph lock.
- [ ] [#lazyperf8] Compare synchronization strategies with a fixed adoption gate: current Condvar, narrower wakeups, parking_lot-style parking, and targeted CAS must report throughput plus p50/p95 latency and lock-site budgets before release.
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
