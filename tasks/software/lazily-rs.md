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
- [ ] Phase 3: Derived/computed signals
- [ ] Phase 4: Effect system
- [x] Phase 5: Testing & benchmarks
- [ ] Phase 6: Documentation & publish
<!-- /agent:status -->

<!-- agent:exchange -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260602-033259.md`*

Compacted content:
- Archived 5 response topic(s): next steps; #tsc16-matrix; #tsc16-wake-scope; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ What are #next-steps to improve the performance of set_cell_invalidation with parallelization?

### Re: set_cell_invalidation parallelization next steps — gpt-5

Top backlog item: `#pinv`.

The main constraint is in `src/lazily-rs/src/thread_safe.rs`: `ThreadSafeContext::set_cell` holds the single graph mutex while it updates the cell, walks dependents, marks slots dirty, bumps revisions, and schedules effects. Parallelizing the current recursive helpers directly would mostly move contention around and make duplicate revision/effect work harder to reason about.

Best path:

1. Build `#pinv` as a frontier-based invalidation prototype: snapshot changed-cell dependents, coalesce visited slot ids, partition only independent frontier branches, then merge dirty/revision/effect updates deterministically.
2. Keep `#pben` close behind it so each prototype run compares lock wait/hold for same-slot, independent-slot, read-mostly, and batched-write cases.
3. Gate real parallel invalidation behind `#ploom`; stale in-flight recompute, duplicate diamond paths, effect coalescing, and batch flush boundaries are the correctness traps.
4. Defer sharded/versioned graph storage to `#pshrd`; it is a larger architecture move and should wait until the frontier prototype proves the remaining bottleneck is the global mutex.

Changed paths: session document only, via `agent-doc finalize`.
Commands used: `tsift status`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --plan`, and targeted `tsift`/`rg`/`sed` source inspection.
Verification: source and benchmark inspection only; no Rust code changed, so I did not run the Rust suite. Escalation: none.

### Re: #pinv — gpt-5

> **Queue prompt:**
>
> [#pinv]


Completed `#pinv`.

Implemented a frontier-based `ThreadSafeContext` invalidation prototype in `src/thread_safe.rs`: dirty invalidation and hard clears now use queue-based frontier walks, coalesce duplicate slot visits, preserve direct changed-value `force_recompute` upgrades, and apply slot dirty/revision/effect scheduling mutations at the existing graph-mutex boundary.

Updated `SPEC.md` with the frontier contract and added `thread_safe_frontier_invalidation_preserves_direct_force_recompute` in `tests/spec_compliance.rs` for the direct-plus-memo duplicate path that would return stale data if force upgrades were lost.

Commits prepared locally:
- `a537a86` in `src/lazily-rs`: `feat: coalesce thread-safe invalidation frontier`
- `966a7fc59` in `agent-loop`: `chore: update lazily-rs submodule`

Push will run after this binary-owned document commit so the session document is included in the superproject push.

Verification:
- `cargo fmt`
- `cargo test thread_safe_frontier_invalidation_preserves_direct_force_recompute`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features loom --test thread_safe_loom`
- `cargo build --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --no-run`
- `cargo bench --features instrumentation --no-run`
- `cargo install --path . --example instrumentation_profile --features instrumentation --force`

`make check` is unavailable in `src/lazily-rs` because there is no `check` target. GitHub Actions CI is also unavailable for this repo: `gh run list --workflow CI --limit 1` could not find a `CI` workflow, and `gh workflow list` returned no workflows, so there was no tmux CI leg to inspect. Cargo build/bench/install emitted only the existing unused workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router`.

Changed paths: `src/lazily-rs/SPEC.md`, `src/lazily-rs/src/thread_safe.rs`, `src/lazily-rs/tests/spec_compliance.rs`, and the superproject `src/lazily-rs` submodule pointer. Escalation: none.
<!-- agent:boundary:fc4013cb -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#pinv]~
- Are Phase 3, 4, 6 implemented? If not, is now a good time to implement them?
- [#pben]
- [#ploom]
- [#pshrd]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pben] Add Criterion/instrumentation profiles that isolate set_cell_invalidation on high fan-out, same-slot contention, independent-slot contention, and batched write bursts before and after the parallel prototype.
- [ ] [#ploom] Add Loom or Shuttle coverage for parallel invalidation invariants: duplicate paths mark each slot once, effects coalesce once, in-flight recompute stale results are discarded, and batches flush after the outermost update.
- [ ] [#pshrd] Evaluate sharded or versioned graph storage only after #pinv and #pben show the remaining bottleneck is the global mutex rather than recursive traversal or duplicate dirty marking.
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
