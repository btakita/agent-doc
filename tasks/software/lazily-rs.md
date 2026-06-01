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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-224718.md`*

Compacted content:
- Archived 1 response topic(s): lazily-rs performance next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: #perf-bench — gpt-5

> **Queue prompt:**
>
> [#perf-bench]


Completed `#perf-bench`.

Added a Criterion benchmark target for `lazily-rs` covering the requested public API paths: cached reads, cold first `get`, dependency fan-out, memo equality suppression, effect flushing, batch storms, and `ThreadSafeContext` contention at 1/2/4/8/16 workers. The suite lives in `benches/context.rs`; `Cargo.toml` now declares the bench target and a dev-only Criterion dependency.

I also updated `SPEC.md` with the benchmark contract and adjusted the dependency wording in `SPEC.md` / `README.md` to distinguish zero mandatory runtime dependencies from optional Tokio and dev-only Criterion.

During full verification, `cargo test --features tokio` exposed an existing `ThreadSafeContext` race: a getter could refresh a slot, then another thread could clear that slot before the final read, causing a panic on a transient unset state. I fixed `ThreadSafeContext::get_slot` to retry while the slot still exists but has no cached value.

Verification:

- `cargo fmt -- --check`
- `cargo bench --no-run`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --features tokio threading_contract::thread_safe_clear_and_dispose_races_remain_consistent -- --exact`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build`
- CI check: `gh run list --workflow CI --limit 1` found no `CI` workflow, and `gh workflow list` returned no workflows for `lazily-rs`
- `make check` was not run because `src/lazily-rs` has no `Makefile`
- `cargo install --path .` is not applicable because `cargo metadata` shows no package binary target

Commits pushed:

- `src/lazily-rs`: `baae170` (`perf: add benchmark suite`) to `btakita/lazily-rs main`
- `agent-loop`: `e7eaa33bf` (`chore: update lazily-rs benchmark pointer`) to `btakita/agent-loop main`

### Re: #perf-profile — gpt-5

> **Queue prompt:**
>
> [#perf-profile]


Completed `#perf-profile`.

Added a feature-gated benchmark instrumentation surface to `lazily-rs`:

- `instrumentation` Cargo feature
- `InstrumentationSnapshot` export
- `instrumentation_snapshot()` and `reset_instrumentation()` on `Context` and `ThreadSafeContext`
- counters for reactive node allocation events, slot recompute starts, duplicate speculative `ThreadSafeContext` computes, dependency edge add/remove churn, effect queue pushes/max depth, and `ThreadSafeContext` graph-lock acquisition/wait/hold timing
- `benches/profile.rs`, gated by `required-features = ["instrumentation"]`, to exercise and consume the profiling counters in Criterion

The default crate path remains feature-free for this instrumentation; default tests/builds compile without the instrumentation fields or lock wrapper. I updated `SPEC.md` and `README.md` with the profiling contract and added spec coverage for the local graph counters plus a deterministic two-thread speculative compute race.

Verification:

- `cargo fmt -- --check`
- `cargo check --all-targets --all-features`
- `cargo test`
- `cargo test --features instrumentation benchmark_instrumentation -- --nocapture`
- `cargo test --all-features`
- `cargo bench --no-run`
- `cargo bench --features instrumentation --no-run`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build`
- CI check: `gh run list --workflow CI --limit 1` still reports no `CI` workflow, and `gh workflow list` returns no workflows for `lazily-rs`
- `make check` was not run because `src/lazily-rs` has no `Makefile`
- `cargo install --path .` is still not applicable; `cargo metadata` shows no package binary target

Commits pushed:

- `src/lazily-rs`: `0fd3a89` (`perf: add benchmark instrumentation`) to `btakita/lazily-rs main`
- `agent-loop`: `1965d50bb` (`chore: update lazily-rs instrumentation pointer`) to `btakita/agent-loop main`

### Re: #perf-layout — gpt-5

> **Queue prompt:**
>
> [#perf-layout]


Completed `#perf-layout`.

Optimized the single-threaded `Context` storage layout by replacing its `HashMap<SlotId, Node>` with slot-id-indexed `Vec<Option<Node>>` storage. `SlotId` remains monotonic and stable; disposed effects leave vacant entries instead of reusing ids, so existing handles and dependency edges keep their semantics while the hot local path avoids hash-map lookup and insertion churn.

Updated the spec and README to document the contiguous local storage contract, and added a regression test for allocating new slots after effect disposal leaves a storage hole.

Short post-change Criterion sample for the single-threaded path:

- cached read: about `12.3 ns`
- cold first get: about `91 ns`
- dependency fan-out / 256: about `27 us`
- memo equality suppression: about `2.9 us`
- effect flushing: about `94 ns`
- batch storm / 64: about `3.6 us`

Verification:

- `cargo fmt -- --check`
- `cargo check --all-targets --all-features`
- `cargo test context::context_allocates_after_effect_disposal -- --exact`
- `cargo test`
- `cargo test --all-features`
- `cargo bench --no-run`
- `cargo bench --features instrumentation --no-run`
- `cargo bench --bench context -- --warm-up-time 0.1 --measurement-time 0.2`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build`
- CI check: `gh run list --workflow CI --limit 1` still reports no `CI` workflow, and `gh workflow list` returns no workflows for `lazily-rs`
- `make check` was not run because `src/lazily-rs` has no `Makefile`

Commits pushed:

- `src/lazily-rs`: `9f4d7ed` (`perf: index context nodes by slot id`) to `btakita/lazily-rs main`
- `agent-loop`: `bc5940e64` (`chore: update lazily-rs layout pointer`) to `btakita/agent-loop main`
<!-- agent:boundary:1fbdb72a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#perf-bench]~
- ~[#perf-profile]~
- ~[#perf-layout]~
- [#perf-thread-dedup]
- [#perf-lock-eval]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#perf-thread-dedup] #performance Prototype ThreadSafeContext in-flight recompute deduplication so concurrent first get callers share one computation when benchmarks show duplicate work is material
- [ ] [#perf-lock-eval] #performance Evaluate RwLock, sharded locks, or targeted CAS only after baseline data identifies lock contention, and require Loom/Shuttle safety models before adopting concurrency changes
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->
- 2026-06-01 [#n12s] #ipc-corruption-live-repro-lazily-rs Logged as #zpc2 in agent-doc-bugs2.md backlog. Live repro of scaffold duplication during finalize --stream. Document manually repaired.
- 2026-06-01 [#exfd] #effect-system Phase 4: Effect system — ctx.effect(|ctx| { ... }) with auto-tracking, scheduling, cleanup, and disposal
- 2026-06-01 [#rx9y] #batch-updates ctx.batch(|ctx| { ... }) to defer invalidation until the batch completes
- 2026-06-01 [#fzaf] #memo-guard Memoization guard — prevent unnecessary downstream cascade during invalidation when recomputed value equals prior value (slots are already lazy: invalidated only, recomputed on get())
- 2026-06-01 [#dpzd] #api-ergonomics ctx.computed() alias and DX improvements
- 2026-06-01 [#45rh] #release-publish Release + publish after effect system lands
- 2026-06-01 [#thr-spec] #parallelism Specify the thread-safe contract for lazily-rs: API shape, Send/Sync bounds, lock/re-entrancy rules, single-threaded fast path compatibility, and parity/differences with lazily-zig's mutex-first design
- 2026-06-01 [#thr-core] #parallelism Implement the thread-safe core after the spec: lock-backed context state, Send + Sync cached values and compute/effect callbacks, safe dependency tracking, and re-entrant user callback boundaries
- 2026-06-01 [#thr-tests] #parallelism Add thread contention coverage: concurrent first get, concurrent set_cell, batch flush ordering, clear/dispose races, effect coalescing, and deadlock/regression checks modeled after lazily-zig thread_safe tests
- 2026-06-01 [#tokio-sync] #tokio Add optional Tokio integration after thread safety is sound: async tests and examples that share the synchronous context across tokio::spawn/spawn_blocking without introducing async compute semantics
- 2026-06-01 [#tokio-async] #tokio Design true async computations/effects only after #tokio-sync: in-flight future deduplication, cancellation, stale future completion, cleanup ordering, and Send vs LocalSet semantics
<!-- /agent:done -->
