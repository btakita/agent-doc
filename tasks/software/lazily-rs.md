---
agent_doc_session: 9386d273-2288-498c-b9fc-207f224f9fdb
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
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260602-004902.md`*

Compacted content:
- Archived 3 response topic(s): next steps; lazily-rs performance baseline and README benchmark report; README benchmark results and update harness
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to optimize thread_safe_contention_16?

### Re: next steps — gpt-5

For `thread_safe_contention_16`, the next move is attribution, not a lock redesign. The current data is `23.670 ms` Criterion time with `14,504` lock acquisitions, `138.001 ms` cumulative lock wait, `4.020 ms` lock hold, and `0` duplicate speculative recomputes. That points at mutex acquisition churn and waiter scheduling, not wasted recomputation.

I queued the backlog in this priority order:

1. `#tsc16-attrib` — split lock acquisitions/wait by call site and compare 1/2/4/8/16 workers.
2. `#tsc16-coalesce-locks` — reduce adjacent lock/unlock cycles in `get` / `refresh_slot` / recompute using locked helper variants while keeping user callbacks outside the graph lock.
3. `#tsc16-edge-diff` — stop unconditional dependency drain/remove/re-add work when the dependency set is unchanged.
4. `#tsc16-wait-park` — prototype Condvar/parking only if attribution shows the spin-yield in-flight wait loop is material.
5. `#tsc16-safety-model` — require Loom or Shuttle coverage before replacing the mutex-first contract.

Top backlog item: `#tsc16-attrib`.

Proof: read `README.md`, `SPEC.md`, `examples/instrumentation_profile.rs`, and `src/thread_safe.rs`; no code changes or tests were needed for this planning-only response. Confidence: high.

### Re: tsc16-attrib — gpt-5

> **Queue prompt:**
>
> [#tsc16-attrib]


Implemented `#tsc16-attrib` and marked it done.

What changed:
- Added `ThreadSafeLockSite`, `ThreadSafeLockSiteSnapshot`, and `ThreadSafeContext::lock_profile_snapshot()` behind the `instrumentation` feature.
- Attributed profiled graph-lock wait/hold/acquisition time to `get_refresh`, `dependency_edge`, `set_cell_invalidation`, `publish`, `in_flight_wait`, and `other` buckets while preserving the existing aggregate lock counters.
- Updated `examples/instrumentation_profile.rs` and `scripts/update-benchmark-results.py` so the README now includes a per-site attribution table for `thread_safe_contention_1/2/4/8/16`.
- Updated `SPEC.md`, `README.md`, and spec-compliance tests for the new instrumentation surface and report requirements.

Key attribution result from the regenerated local profile: `thread_safe_contention_16` is dominated by `in_flight_wait` (`9634` acquisitions, `86.280 ms` wait, `1.849 ms` hold in the committed README sample). The next queue item remains `#tsc16-coalesce-locks`.

Commits:
- `lazily-rs` submodule: `860031c feat: attribute thread-safe lock contention`, pushed to `origin/main`.
- Superproject gitlink: `6b0dd7216 chore: update lazily-rs submodule`, committed locally; it will be pushed with this document closeout after `finalize` completes.

Verification:
- `cargo fmt`
- `cargo test`
- `cargo test --features instrumentation`
- `cargo test --features tokio`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --all-features` (passed; Cargo reported existing unused workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router`)
- `cargo bench --features instrumentation --no-run` (passed with the same workspace patch warnings)
- `python3 scripts/update-benchmark-results.py --no-run`
- `python3 scripts/update-benchmark-results.py --no-run --check`
- `git diff --check`

Local install/check notes:
- `cargo install --path .` is not applicable because `lazily` is a library crate with no binaries; Cargo returned that explicit error.
- `make check` is not available in `src/lazily-rs`; there is no `Makefile` and `make` returned `No rule to make target 'check'`.

CI/tmux evidence:
- `gh run list --workflow CI --limit 1` reports no workflow named `CI` for `lazily-rs` or the superproject.
- Latest superproject `Agent Doc` run `26791663690` completed successfully on `main`; its `tmux-ci` job and `check` job both concluded `success`.

Escalation: used approved `rm -rf scripts/__pycache__` only to remove a generated Python cache directory created during README generator inspection.

Confidence: high.
<!-- agent:boundary:f146ae54 -->
<!-- /agent:exchange -->7

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#tsc16-attrib]~
- [#tsc16-coalesce-locks]
- [#tsc16-edge-diff]
- [#tsc16-wait-park]
- [#tsc16-safety-model]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsc16-coalesce-locks] #performance #thread-safe Reduce ThreadSafeContext lock churn in get/refresh/recompute by coalescing adjacent state reads/writes and replacing extra helper-level lock acquisitions with locked helper variants where callbacks still run outside the graph lock
- [ ] [#tsc16-edge-diff] #performance #thread-safe Optimize thread-safe dependency edge maintenance by diffing old/new dependency sets and batching edge updates during recompute publish instead of unconditional drain/remove/re-add work
- [ ] [#tsc16-wait-park] #performance #thread-safe Prototype a Condvar or parking wait path for in-flight ThreadSafeContext recompute waiters only after attribution shows the spin-yield wait loop materially contributes to thread_safe_contention_16
- [ ] [#tsc16-safety-model] #performance #thread-safe Add Loom or Shuttle coverage for any ThreadSafeContext synchronization change, covering stale in-flight completion, invalidation during compute, effect scheduling/disposal, and re-entrant callbacks
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
