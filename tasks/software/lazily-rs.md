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

### Re: Phase 3, 4, and 6 status — gpt-5

> **Queue prompt:**
>
> Are Phase 3, 4, 6 implemented? If not, is now a good time to implement them?


Yes: Phases 3, 4, and 6 are implemented now, so I marked them complete in the status block.

Evidence:

- Phase 3, derived/computed signals: `Context::computed`, `Context::memo`, `ThreadSafeContext::computed`, and `ThreadSafeContext::memo` are implemented and covered by spec tests.
- Phase 4, effect system: `ctx.effect`, cleanup, disposal, dependency tracking, dirty-slot validation, coalesced reruns, and thread-safe effects are implemented and covered by spec/Loom-adjacent tests.
- Phase 6, documentation & publish: README/SPEC/rustdoc-facing library docs are present, `cargo publish --dry-run` packages and verifies `lazily v0.4.0`, and registry metadata confirms `lazily 0.4.0` is published at <https://crates.io/crates/lazily/0.4.0> with docs at <https://docs.rs/lazily/0.4.0>.

So this is not the right moment to implement Phases 3, 4, or 6 from scratch; they are done. The useful next work is the existing queue: `#pben`, then `#ploom`, then `#pshrd`, because those are follow-up performance and concurrency-hardening items rather than missing phase implementation.

Commands used: `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `tsift status`, targeted `rg`/`sed` inspection, web lookup for current docs/registry context, `cargo publish --dry-run`, and `cargo info lazily --registry crates-io`.

Verification: publish dry run and crates.io registry metadata check only; no code changed. Changed paths: session document status/exchange only, via `agent-doc finalize`. Escalation: none.

### Re: #pben — gpt-5

> **Queue prompt:**
>
> [#pben]


Completed `#pben`.

Added a dedicated `set_cell_invalidation` Criterion benchmark group that isolates changed-cell invalidation from dependent reads/recompute work:

- `high_fan_out / 512`
- `same_slot_contention / 1/2/4/8/16`
- `independent_slot_contention / 1/2/4/8/16`
- `batched_write_bursts / 1/2/4/8/16`

Added matching instrumentation profile rows in `examples/instrumentation_profile.rs` with assertions that the `set_cell_invalidation` lock-site bucket is populated, extended the benchmark report generator to sort and print lock attribution for those rows, updated `SPEC.md` and spec-compliance checks, and regenerated the README benchmark section with the filtered Criterion estimates plus fresh instrumentation CSV output.

Commits prepared locally:

- `71f2f2a` in `src/lazily-rs`: `bench: isolate set_cell invalidation profiles`
- `95ffd81ba` in `agent-loop`: `chore: update lazily-rs benchmarks`

Push will run after this binary-owned document commit so the session document is included in the superproject push.

Verification:

- `cargo fmt`
- `cargo bench --features instrumentation --no-run`
- `cargo run --example instrumentation_profile --features instrumentation --quiet`
- `cargo bench --features instrumentation set_cell_invalidation`
- `python3 scripts/update-benchmark-results.py --no-run`
- `python3 scripts/update-benchmark-results.py --check`
- `git diff --check`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features loom --test thread_safe_loom`
- `cargo build --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --no-run`
- `cargo install --path . --example instrumentation_profile --features instrumentation --force`
- `cargo publish --dry-run --allow-dirty`

`make check` remains unavailable in `src/lazily-rs` because there is no `check` target. GitHub Actions CI remains unavailable: `gh run list --workflow CI --limit 1` could not find a `CI` workflow, and `gh workflow list` returned no workflows, so there was no tmux CI leg to inspect. Cargo build/bench/install/publish emitted only the existing unused workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router`.

Changed paths: `src/lazily-rs/README.md`, `src/lazily-rs/SPEC.md`, `src/lazily-rs/benches/context.rs`, `src/lazily-rs/examples/instrumentation_profile.rs`, `src/lazily-rs/scripts/update-benchmark-results.py`, `src/lazily-rs/tests/spec_compliance.rs`, and the superproject `src/lazily-rs` submodule pointer. Escalation: none.

### Re: #ploom — gpt-5

> **Queue prompt:**
>
> [#ploom]


Completed `#ploom`.

Added a Loom-backed frontier invalidation model in `tests/thread_safe_loom.rs` covering the parallel-invalidation safety envelope:

- duplicate diamond paths coalesce before the join slot is marked;
- the effect queue is pushed once for the diamond frontier;
- nested batched invalidations do not flush until the outermost batch exits;
- duplicate invalidations inside the batch coalesce into one flush;
- an in-flight join compute that started before the invalidation flush is discarded as stale.

Updated `SPEC.md` to name the frontier invalidation Loom coverage alongside the existing sidecar `Condvar`, stale in-flight completion, effect scheduling/disposal, and re-entrant callback models.

Commits prepared locally:

- `5327cdc` in `src/lazily-rs`: `test: model frontier invalidation invariants`
- `7c4675e5e` in `agent-loop`: `chore: update lazily-rs loom coverage`

Push will run after this binary-owned document commit so the session document is included in the superproject push.

Verification:

- `cargo fmt`
- `cargo test --features loom --test thread_safe_loom`
- `git diff --check`
- `cargo test`
- `cargo test --features tokio`
- `cargo build --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --no-run`
- `cargo bench --features instrumentation --no-run`
- `cargo install --path . --example instrumentation_profile --features instrumentation --force`
- `cargo publish --dry-run --allow-dirty`

`make check` remains unavailable in `src/lazily-rs` because there is no `check` target. GitHub Actions CI remains unavailable: `gh run list --workflow CI --limit 1` could not find a `CI` workflow, and `gh workflow list` returned no workflows, so there was no tmux CI leg to inspect. Cargo build/bench/install/publish emitted only the existing unused workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router`.

Changed paths: `src/lazily-rs/SPEC.md`, `src/lazily-rs/tests/thread_safe_loom.rs`, and the superproject `src/lazily-rs` submodule pointer. Escalation: none.

### Re: #pshrd — gpt-5

> **Queue prompt:**
>
> [#pshrd]


Completed `#pshrd` as an evaluation item, not as a storage rewrite.

I added a SPEC section for the sharded/versioned graph storage decision after the frontier invalidation and isolated `set_cell_invalidation` benchmark work. The conclusion is that the current benchmark evidence still points at the global graph mutex as a measured bottleneck for independent invalidation workloads, but a sharded or versioned replacement should stay gated behind a narrower prototype because it affects shared aggregates, effect queue ordering, batching, disposal, and dynamic edge maintenance. The next storage experiment is scoped to sharding independent `ThreadSafeState` mutation by stable node id with deterministic merge boundaries and Loom/Shuttle coverage before adoption.

Changed paths:

- `src/lazily-rs/SPEC.md`
- `src/lazily-rs/tests/spec_compliance.rs`
- `src/lazily-rs` submodule pointer in the superproject

Verification run:

- `cargo fmt`
- `cargo test sharded_storage_evaluation_is_benchmark_gated`
- `git diff --check`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features loom --test thread_safe_loom`
- `cargo build --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --no-run`
- `cargo bench --features instrumentation --no-run`
- `cargo install --path . --example instrumentation_profile --features instrumentation --force`
- `cargo publish --dry-run --allow-dirty`

Local `make check` is unavailable in `src/lazily-rs` (`No rule to make target 'check'`). GitHub Actions has no `CI` workflow for this repo, and `gh workflow list` returned no workflows, so there was no CI/tmux leg to inspect. The Cargo build/install/publish commands only emitted the existing unused workspace patch warnings for `agent-kit`, `tagpath`, and `tmux-router`; publish dry run also noted that `lazily@0.4.0` already exists, then completed the dry-run verification.

Commits:

- `edd0fb7` in `src/lazily-rs`: `docs: evaluate thread-safe storage strategy`
- `466f103ed` in the superproject: `chore: update lazily-rs storage evaluation`

Escalation: none.
<!-- agent:boundary:aadbff23 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
