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
queue_active: false
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-081258.md`*

Compacted content:
- Archived 5 response topic(s): Memo guard utility, IPC corruption logged; Slot invalidation vs recomputation; Updated #fzaf backlog item; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: #rx9y — gpt-5

> **Queue prompt:**
>
> [#rx9y]


Completed `#rx9y`: `ctx.batch(|ctx| { ... })` is implemented in `lazily-rs` and published.

Changed paths:
- `src/lazily-rs/src/context.rs` adds the nested batch guard, queued changed-cell roots, queued explicit slot clears, and one effect flush at the outermost batch boundary.
- `src/lazily-rs/src/slot.rs` defers direct `SlotHandle::clear` effect flushing while a batch is open.
- `src/lazily-rs/SPEC.md`, `src/lazily-rs/README.md`, and `src/lazily-rs/tests/spec_compliance.rs` document and test batch behavior, including nested batches, coalesced effect reruns, deferred slot clears, and deferred `cell.clear_dependents`.
- `src/lazily-rs/AGENTS.md` and `.gitignore` were updated for the current test counts and local tsift metadata hygiene.

Commit/push evidence:
- `lazily-rs` remote `main` contains `ddbb78c` (`feat: add batch updates`). My initial local commit was equivalent and was skipped during rebase because the remote already had the same patch.
- `agent-loop` parent repo commit `bbf6631ef` (`chore: update lazily-rs batch updates`) updates the `src/lazily-rs` submodule pointer and was pushed to `origin/main`.

Verification:
- `cargo fmt --check` passed in `src/lazily-rs`.
- `cargo test` passed in `src/lazily-rs` after the final test set was present.
- `cargo clippy` passed in `src/lazily-rs`.
- `cargo build` passed in `src/lazily-rs`; it only emitted existing workspace patch warnings for unused local patches.
- `cargo test -- --list` reports 13 integration tests and 51 spec compliance tests, 64 total.
- `make check` passed from `/home/brian/work/btakita/agent-loop`.
- Latest `Agent Doc` workflow run `26737409904` was successful, including the `tmux-ci` job. There is no workflow named `CI` in either the `lazily-rs` submodule or the superproject.

Install/escalation notes:
- `cargo install --path .` is not applicable because `lazily` is a library-only crate; Cargo reports `no packages found with binaries or examples`.
- No escalated shell permissions were required.

Preflight warning carried forward: `agent-doc preflight` reported pending-shadow copies for `#rx9y`, `#fzaf`, `#dpzd`, and `#45rh`; `#rx9y` is now completed through the normal `--done rx9y` closeout path.
Completed the `#rx9y` agent-doc cycle.

`lazily-rs` batch updates are implemented and pushed:
- `lazily-rs`: `ddbb78c` (`feat: add batch updates`)
- parent `agent-loop` submodule pointer: `bbf6631ef`
- session closeout commit: `cd1590a1d`

Verification passed:
- `cargo fmt --check`
- `cargo test`
- `cargo clippy`
- `cargo build`
- root `make check`
- latest `Agent Doc` workflow run, including `tmux-ci`, was green

`cargo install --path .` is not applicable because `lazily` is a library-only crate with no binary/example targets.

`agent-doc session-check` passed and reports the next queue head is `#fzaf`. The root branch is synced with `origin/main`; unrelated dirty files remain in the superproject, and `src/lazily-rs` itself is clean.

### Re: #fzaf — gpt-5

> **Queue prompt:**
>
> [#fzaf]


Completed `#fzaf`: the memoization guard is implemented, documented, tested, committed, and pushed.

Changed paths:
- `src/lazily-rs/src/context.rs` adds `ctx.memo()`, dirty-slot validation, equal-value suppression for downstream slot/effect work, and fresh-only `ctx.is_set()` reporting.
- `src/lazily-rs/tests/spec_compliance.rs` covers equal intermediate memo recomputation, effect skip behavior, hard clear behavior, batched set+clear transitive clearing, and the 54-test spec suite.
- `src/lazily-rs/README.md`, `src/lazily-rs/SPEC.md`, and `src/lazily-rs/AGENTS.md` document memo slots, dirty validation, hard clear versus lazy invalidation, fresh-cache status, and the current test counts.

Commit/push evidence:
- `lazily-rs` `4612dfa` (`feat: add memoization guard`) implements the guard.
- `lazily-rs` `b8cc781` (`fix: report only fresh slots as set`) keeps compatibility with older `agent-doc` cache-status expectations while preserving lazy dirty caches internally.
- Parent `agent-loop` `91aa9739d` and `5c8dbeef2` update the `src/lazily-rs` submodule pointer; both were pushed to `origin/main`.

Verification:
- `cargo fmt --check` passed in `src/lazily-rs`.
- `cargo test --test spec_compliance` passed: 54 tests.
- `cargo test` passed: 13 integration tests plus 54 spec tests.
- `cargo clippy` passed in `src/lazily-rs`.
- `cargo build` passed in `src/lazily-rs`.
- Root `make check` passed from `/home/brian/work/btakita/agent-loop`.
- GitHub `Agent Doc` run `26745360067` passed: `tmux-ci` in 2m59s and `check` in 7m1s. There is no workflow named `CI`; `Agent Doc` is the project workflow.

`cargo install --path .` remains not applicable because `lazily` is a library-only crate with no binary/example targets. No escalated shell permissions were required. Confidence is high; `src/lazily-rs` is clean, and unrelated dirty parent-repo files were left untouched.

### Re: #fzaf memo guard — gpt-5

Completed `#fzaf`: `lazily-rs` now has an opt-in memoization guard through `ctx.memo(|ctx| T)` for `T: PartialEq`, while `ctx.slot()` remains available for non-`PartialEq` values.

Changed paths:
- `src/lazily-rs/src/context.rs` adds memoized slot equality, dirty validation, effect rerun skipping for unchanged memo dependencies, and updates `ctx.is_set()` to report only cached fresh slots.
- `src/lazily-rs/tests/spec_compliance.rs` covers equal intermediate recomputation suppressing downstream recompute/effect reruns, dirty freshness reporting, and hard-clear behavior.
- `src/lazily-rs/README.md`, `src/lazily-rs/SPEC.md`, and `src/lazily-rs/AGENTS.md` document the memo guard and current test counts.
- Parent `src/lazily-rs` submodule pointer now references the final lazily-rs commit.

Commit/push evidence:
- `lazily-rs` remote `main`: `4612dfa` (`feat: add memoization guard`) and follow-up `b8cc781` (`fix: report only fresh slots as set`).
- `agent-loop` remote `main`: `91aa9739d` (`chore: update lazily-rs memo guard`) and final pointer update `5c8dbeef2` (`chore: update lazily-rs freshness guard`).
- I removed the temporary local `src/agent-doc` graph workaround; no agent-doc code change was needed after the lazily-rs freshness fix.

Verification:
- `cargo fmt --check` passed in `src/lazily-rs`.
- `tsift rewrite --run 'cargo test'` passed in `src/lazily-rs`.
- `tsift rewrite --run 'cargo build'` passed in `src/lazily-rs` with only existing unused workspace patch warnings.
- `tsift rewrite --run 'cargo clippy --all-targets --all-features -- -D warnings'` passed in `src/lazily-rs`.
- `cargo package --allow-dirty` passed.
- `cargo install --path . --force` is not applicable for this library-only crate; Cargo reported `no packages found with binaries or examples`.
- `tsift rewrite --run 'cargo test -p agent-doc-orchestration graph::tests:: -- --nocapture'` passed after removing the temporary graph workaround.
- Root `make check` passed.
- GitHub has no workflow named `CI`; latest `Agent Doc` run `26745360067` passed both `check` and `tmux-ci`.

Confidence: high. The memo guard behavior is covered in lazily-rs tests and also verified against the downstream agent-doc graph cache expectations.

Escalation: none. Existing unrelated dirty files remain in the superproject worktree; I staged and committed only the lazily-rs paths/submodule pointer.

### Re: #dpzd computed alias — gpt-5

Completed `#dpzd`: `lazily-rs` now exposes `ctx.computed(|ctx| T)` as the ergonomic alias for ordinary derived slots, while keeping `ctx.slot()` as the underlying low-level synonym and `ctx.memo()` for `PartialEq` guarded derived values.

Changed paths:
- `src/lazily-rs/src/context.rs` adds `Context::computed()` delegating to `Context::slot()`.
- `src/lazily-rs/tests/spec_compliance.rs` adds coverage that `ctx.computed()` tracks cell dependencies and invalidates/recomputes like a slot.
- `src/lazily-rs/README.md` and `src/lazily-rs/SPEC.md` document `computed`, clarify the slot/memo choice, and make the quickstart use `ctx.computed()` for derived values.
- `src/lazily-rs/AGENTS.md` updates the test counts to 13 integration tests plus 55 spec tests, 68 total.
- Parent `src/lazily-rs` submodule pointer now references the final lazily-rs commit.

Commit/push evidence:
- `lazily-rs` remote `main`: `780093b` (`feat: add computed alias`).
- `agent-loop` remote `main`: `ffdeb5a96` (`chore: update lazily-rs computed alias`).

Verification:
- `cargo fmt --check` passed in `src/lazily-rs`.
- `tsift rewrite --run 'cargo test'` passed in `src/lazily-rs`.
- `cargo test -- --list` reports 13 integration tests and 55 spec compliance tests.
- `tsift rewrite --run 'cargo build'` passed in `src/lazily-rs` with only existing unused workspace patch warnings.
- `tsift rewrite --run 'cargo clippy --all-targets --all-features -- -D warnings'` passed in `src/lazily-rs`.
- `cargo package --allow-dirty` passed.
- `cargo install --path . --force` is still not applicable for this library-only crate; Cargo reported `no packages found with binaries or examples`.
- Root `make check` passed.
- GitHub has no workflow named `CI`; latest `Agent Doc` run `26746156851` passed both `check` and `tmux-ci`.

Confidence: high. The alias delegates to existing slot behavior, has direct spec coverage, and passed both local and remote verification.

Escalation: none. Existing unrelated dirty files remain in the superproject worktree; I staged and committed only lazily-rs files/submodule pointer for this item.

### Re: #45rh release — gpt-5

> **Queue prompt:**
>
> [#45rh]


Released `lazily` v0.3.0. crates.io now resolves `lazily = "0.3.0"`, and the crate release commit is tagged `v0.3.0`.

Changed paths: `src/lazily-rs/Cargo.toml`, `src/lazily-rs/Cargo.lock`, the `src/lazily-rs` submodule pointer, `src/agent-doc/Cargo.lock`, and the `src/agent-doc` submodule pointer.

Commits: `src/lazily-rs` `acb1633` (`chore: release v0.3.0`), `src/agent-doc` `f9e7e746` (`chore: update lazily lockfile to v0.3.0`), and parent `aefe72545` (`chore: update lazily-rs v0.3.0`). All three were pushed; `v0.3.0` was pushed to `btakita/lazily-rs`.

Verification: `cargo fmt --check`; `tsift rewrite --run 'cargo test'`; `tsift rewrite --run 'cargo build'`; `tsift rewrite --run 'cargo clippy --all-targets --all-features -- -D warnings'`; `cargo package --allow-dirty`; `cargo publish --dry-run`; `cargo publish`; `cargo search lazily --limit 1`; root `make check`; GitHub Actions `Agent Doc` run `26746914026` passed both `check` and `tmux-ci`. The expected library-only `cargo install --path . --force` check still reports `no packages found with binaries or examples`.

Confidence: high. Escalation: none; unrelated dirty edits already present in `src/agent-doc` were preserved and not included.
❯ What are #next-steps for lazily-rs to support parallelism (Threads) and concurrency (Tokio)? See lazily-zig for a reference.

### Re: threads and Tokio next steps — gpt-5

Top backlog item: `#thr-spec`.

Recommended order:

1. `#thr-spec` Spec the thread-safe contract before touching internals. `lazily-rs` is currently deliberately local: `Context` owns `RefCell<HashMap<...>>`, callbacks are `Box<dyn Fn>`, cached values are `Box<dyn Any>`, and effect cleanups are `Box<dyn FnOnce()>`. The useful `lazily-zig` reference pattern is one context-level mutex, a thread-local tracking stack, and explicit unlock boundaries before user callbacks that may re-enter the context.
2. `#thr-core` Build a lock-backed thread-safe context path. Prefer a new `ThreadSafeContext`/feature or internal `ContextInner` split over silently changing the current `Context` semantics. Require `Send + Sync + 'static` for cached values and compute/effect closures on this path, while preserving the existing single-threaded fast path unless the spec decides thread safety should become default.
3. `#thr-tests` Add contention/regression tests before API polish: concurrent first `get`, concurrent `set_cell`, batch flush ordering, clear/dispose races, effect scheduling coalescing, and re-entrant effects. This should mirror `lazily-zig`'s `thread_safe` coverage, but assert Rust-level invariants instead of only proving no deadlock/crash.
4. `#tokio-sync` Add Tokio as an optional integration after sync thread safety is sound. Start with sharing the synchronous context across `tokio::spawn`/`spawn_blocking` and async tests under a `tokio` feature; this proves `Send`/`Sync` bounds and scheduler interaction without introducing async cache state.
5. `#tokio-async` Only then design true async computations/effects. That needs explicit semantics for in-flight future deduplication, cancellation, dependency tracking across `.await`, stale future completion, cleanup ordering, and `Send` vs `LocalSet` futures.

I added these as backlog items in that priority order. My recommendation is to start with `#thr-spec`; it is the smallest step that prevents Tokio decisions from leaking into the lower-level locking model too early.

Changed paths: session document only.
Commands: `agent-doc preflight`; `agent-doc read`; `agent-doc plan`; `tsift status` in root and both submodules; inspected `src/lazily-rs` and `src/lazily-zig` README/SPEC/source/test files.
Verification: no source code changed; this is a planning/backlog update, so I did not run the crate test suite.
Confidence: medium-high. The sequence is based on current local source inspection; exact API naming should be decided in `#thr-spec`.
Escalation: none.

### Re: #thr-spec threading contract — gpt-5

> **Queue prompt:**
>
> [#thr-spec]


Completed `#thr-spec`: the `lazily-rs` thread/concurrency contract is now specified, tested, committed, and pushed.

What changed:
- `SPEC.md` now defines the current local-thread `Context`, the planned explicit `ThreadSafeContext` API shape, `Send + Sync + 'static` bounds, lock/re-entrancy rules, lazily-zig parity points, and the staged Tokio path.
- `README.md` now has a threading roadmap and clarifies that shared-context parallelism is planned as explicit API work rather than a silent change to `Context`.
- `src/lib.rs` adds a compile-fail doctest showing that the current `Context` cannot be moved into `std::thread::spawn`.
- `tests/spec_compliance.rs` adds threading-contract coverage for portable handle traits and independent contexts running on separate OS threads.
- `AGENTS.md` test counts are updated to 13 integration tests, 57 spec tests, and 1 doctest.

Changed paths:
- `src/lazily-rs/AGENTS.md`
- `src/lazily-rs/README.md`
- `src/lazily-rs/SPEC.md`
- `src/lazily-rs/src/lib.rs`
- `src/lazily-rs/tests/spec_compliance.rs`
- parent submodule pointer `src/lazily-rs`
- session document; I also repaired a duplicated queue/backlog scaffold before preflight could parse the document.

Commits/push:
- `src/lazily-rs` `3060680` (`docs: specify threading contract`) pushed to `origin/main`.
- parent `agent-loop` `5b1a49573` (`chore: update lazily-rs threading contract`) pushed to `origin/main`.

Verification:
- `cargo fmt --check` passed in `src/lazily-rs`.
- `cargo test` passed in `src/lazily-rs`.
- `cargo test -- --list` reports 13 integration tests, 57 spec compliance tests, and 1 doctest.
- `cargo build` passed in `src/lazily-rs` with only existing unused workspace patch warnings.
- `cargo clippy --all-targets --all-features -- -D warnings` passed in `src/lazily-rs`.
- `cargo package --allow-dirty` passed.
- `cargo install --path . --force` remains not applicable because `lazily` is library-only; Cargo reports `no packages found with binaries or examples`.
- Root `make check` passed.
- GitHub has no workflow named `CI` in the submodule or root. Latest root `Agent Doc` run `26768336727` passed, including `check` and `tmux-ci`.

Queue closeout: marked `#thr-spec` done. The next queue head should be `#thr-core`.

Confidence: high. The change is spec/test-only for the current item and keeps the current single-threaded implementation unchanged.

Escalation: none.

### Re: #thr-core thread-safe context — gpt-5

Implemented `ThreadSafeContext` for `lazily-rs` and completed the queued `#thr-core` work.

What changed:
- Added `src/thread_safe.rs`, a mutex-backed shared reactive graph with `Send + Sync + 'static` value/callback bounds.
- Exported `ThreadSafeContext` and `ThreadSafeEffectCallbackResult` from `src/lib.rs` while keeping `Context` single-threaded.
- Added cross-thread spec tests for shared reads, cross-thread invalidation, re-entrant compute callbacks, and effects rerun from another thread.
- Updated `SPEC.md`, `README.md`, and `AGENTS.md` to document the implemented thread-safe API and new test counts.
- During review, fixed an effect-flush race where another thread could enqueue work after the queue appeared empty but before `flushing_effects` was cleared.

Verification:
- `cargo fmt --check`
- `cargo check`
- `cargo test` via `tsift rewrite --run 'cargo test'`
- `cargo test -- --list` reports 13 integration tests, 61 spec tests, and 1 doctest
- `cargo build` via `tsift rewrite --run 'cargo build'`
- `cargo clippy --all-targets --all-features -- -D warnings` via `tsift rewrite`
- `cargo package --allow-dirty`
- `cargo install --path . --force` was attempted and failed as expected because this crate has no binaries or examples
- Root `make check` passed after the final code change
- GitHub Actions: no workflow named `CI`; latest completed `Agent Doc` run `26769907756` passed both `check` and `tmux-ci`. Fresh push run `26770438815` had `tmux-ci` passing and `check` still in progress when recorded.

Commits/push:
- `lazily-rs`: `8e778b6` (`feat: add thread-safe context`) pushed to `origin/main`.
- Parent `agent-loop`: the `src/lazily-rs` pointer to `8e778b6` was captured in root commit `93a2e5212` by an overlapping `agent-doc` closeout while the pointer was staged, then pushed to `origin/main` in the `bbb2b65a8..aecd06592` push range.

Queue status: `#thr-core` is complete; next queued item should be `#thr-tests`.

### Re: #thr-tests thread contention coverage — gpt-5

> **Queue prompt:**
>
> [#thr-tests]


Completed `#thr-tests`.

I added broader thread-safe regression coverage in lazily-rs for concurrent first slot access, concurrent `set_cell`, cross-thread batch flush ordering, diamond effect coalescing, reentrant effect writes, clear/dispose races, and a midflight invalidation race where a stale slot callback result must be discarded and retried.

The midflight invalidation test exposed a real publish race, so `ThreadSafeContext` now tracks a per-slot revision. If a slot is invalidated or explicitly cleared while its compute callback is running, the computed stale value is not published as fresh; the getter retries against the latest graph state.

Changed in lazily-rs:
- `src/thread_safe.rs` adds slot revision checks for stale recompute retries.
- `tests/spec_compliance.rs` adds the new contention/race tests.
- `SPEC.md` and `README.md` document the stale-recompute retry guarantee.
- `AGENTS.md` updates the spec test counts to 67 spec tests / 80 integration+spec tests plus 1 doctest.

Commits:
- lazily-rs `fe27baa` `test: cover thread-safe contention races` pushed to `origin/main`.
- lazily-rs `e8fe721` `docs: specify stale thread-safe recompute retry` pushed to `origin/main`.
- parent repo `6883d2a7d` records the lazily-rs submodule pointer update.

Verification passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test threading_contract -- --nocapture`
- `cargo test`
- `cargo test -- --list` reports 13 integration tests, 67 spec tests, and 1 doctest
- `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo package --allow-dirty`
- root `make check`

`cargo install --path . --force` still fails for the expected reason: this crate has no binaries or examples to install.

CI/tmux evidence: there is no GitHub Actions workflow named `CI`; the latest `Agent Doc` run `26770794713` completed successfully, including both `check` and `tmux-ci`.

I left unrelated workspace state out of the lazily-rs commits and parent pointer commit.

### Re: #tokio-sync optional Tokio sync integration — gpt-5

> **Queue prompt:**
>
> [#tokio-sync]


Completed `#tokio-sync`.

I added an optional `tokio` feature that validates the existing synchronous `ThreadSafeContext` on Tokio runtimes without introducing async computations, async effects, future deduplication, or `.await` dependency tracking. That keeps the current boundary clear: Tokio can schedule sync thread-safe graph access now; true async reactive semantics stay in `#tokio-async`.

Changed in lazily-rs:
- `Cargo.toml` adds `features.tokio = ["dep:tokio"]`, an optional Tokio dependency, and a `tokio_sync` example gated by the feature.
- `tests/tokio_sync.rs` adds two feature-gated async tests covering `tokio::spawn` and `tokio::task::spawn_blocking` with `ThreadSafeContext`.
- `examples/tokio_sync.rs` demonstrates using `ThreadSafeContext` from `spawn_blocking`.
- `README.md` and `SPEC.md` document the sync-on-Tokio scope and explicitly keep true async semantics separate.
- `AGENTS.md` records the new feature-gated Tokio test surface.

Commits:
- lazily-rs `0ebb983` `feat: add tokio sync integration` pushed to `origin/main`.
- parent repo `7b30c5069` records the lazily-rs submodule pointer update.

Verification passed:
- `cargo fmt --check`
- `cargo check`
- `cargo check --features tokio`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --all-features`
- `cargo test -- --list` reports 13 integration tests, 68 spec tests, 0 default Tokio tests, and 1 doctest
- `cargo test --features tokio -- --list` reports the same default tests plus 2 Tokio sync tests
- `cargo run --example tokio_sync --features tokio` prints `doubled=42`
- `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo package --allow-dirty`
- root `make check`

`cargo install --path . --force` still fails for the expected library-crate reason: there are no binaries to install.

CI/tmux evidence: there is no GitHub Actions workflow named `CI`. The latest pushed `Agent Doc` run I inspected, `26771332149`, had `tmux-ci` completed successfully while `check` was still in progress; local root `make check` passed before this closeout.

I left unrelated workspace state out of the lazily-rs commit and parent pointer commit.

### Re: #tokio-async async computation design — gpt-5

> **Queue prompt:**
>
> [#tokio-async]


Completed `#tokio-async` as a design/spec item.

I did not implement true async computations or effects in this cycle. The work records the contract that a future implementation must satisfy, and keeps it separate from the already-landed sync-on-Tokio feature.

Changed in lazily-rs:
- `SPEC.md` adds a `Future async computations/effects design` section with an `AsyncContext` API sketch and required semantics for in-flight future deduplication, cancellation, dependency tracking across `.await`, stale future completion, async cleanup ordering, batching interaction, and separate `Send` versus `LocalSet` surfaces.
- `README.md` now points from the Tokio sync feature to the separate `AsyncContext` design and explicitly names the async semantics still deferred.
- `src/lib.rs` adds a compile-fail async contract doctest showing that the current sync graph does not await async slot callbacks.
- `AGENTS.md` updates the doctest count to 2.

Commits:
- lazily-rs `9873900` `docs: design async context semantics` pushed to `origin/main`.
- parent repo `3c90cf805` records the lazily-rs submodule pointer update.

Verification passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo test --all-features`
- `cargo test -- --list` reports 13 integration tests, 68 spec tests, 0 default Tokio tests, and 2 doctests
- `cargo test --features tokio -- --list` reports the same default tests plus 2 Tokio sync tests and 2 doctests
- `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo package --allow-dirty`
- root `make check`

`cargo install --path . --force` still fails for the expected library-crate reason: there are no binaries to install.

CI/tmux evidence: there is no GitHub Actions workflow named `CI`. The latest pushed `Agent Doc` run I inspected, `26771726546`, had `tmux-ci` completed successfully while `check` was still in progress; local root `make check` passed on the final state before this closeout.

Escalation/confidence: implementation is intentionally deferred; the design is now explicit enough to drive future tests and implementation without mixing sync Tokio scheduling with true async reactive semantics.

### Re: lock-free/CAS evaluation plan — gpt-5

> **Queue prompt:**
>
> Would a lock-free mechanism (e.g. CAS) perform better than a lock-backed context state? If yes, create a plan with #mermaid diagrams to evaluate performance and concurrency characteristics. Ensure the parallelism/concurrency is provably safe.


A lock-free mechanism such as CAS could perform better than the current lock-backed context for narrow hot paths, but I would not replace the whole graph state with a lock-free design without benchmarks and model-check proof first. The safe path is to keep `ThreadSafeContext` as the correctness baseline and evaluate targeted CAS only where the operation has a clear linearization point.

I created the plan here: [plan-lazily-rs-lock-free-context.md](/home/brian/work/btakita/agent-loop/tasks/software/plan-lazily-rs-lock-free-context.md:1). It includes Mermaid diagrams for the decision flow, slot state model, concurrent get/set race, and safety test matrix, plus benchmark workloads and proof obligations for memory ordering, ABA avoidance, dependency consistency, effect coalescing, cleanup ordering, and batch boundaries.

No lazily-rs runtime code changed for this prompt. I committed the plan as `dcb22dc09` (`docs: plan lazily-rs lock-free evaluation`). Verification: root `make check` passed. GitHub has no workflow named `CI`; the relevant `Agent Doc` run `26772048032` has `tmux-ci` completed successfully, while its `check` job was still in progress when inspected. The previous `Agent Doc` runs `26771726546` and `26771332149` were successful.
❯ publish + release

### Re: publish + release — gpt-5

Published and released `lazily v0.4.0`.

Changed paths:
- `src/lazily-rs/Cargo.toml` and `src/lazily-rs/Cargo.lock` bumped the crate from `0.3.0` to `0.4.0`.
- Root `src/lazily-rs` submodule pointer advanced to the release commit.

Release artifacts:
- crates.io now reports `lazily = "0.4.0"`.
- `lazily-rs` commit `8478c6b` (`chore: release v0.4.0`) was pushed to `main`.
- `lazily-rs` tag `v0.4.0` was created and pushed.
- Root commit `028f6f7d4` (`chore: release lazily-rs v0.4.0`) records the `src/lazily-rs` pointer update.

Commands and verification:
- `cargo fmt --check`
- `cargo check`
- `cargo check --features tokio`
- `cargo test`
- `cargo test --features tokio`
- `cargo test --all-features`
- `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo package --allow-dirty`
- `cargo publish --dry-run --allow-dirty`
- `cargo publish`
- root `make check`
- `cargo search lazily --limit 1` confirmed `0.4.0`

GitHub Actions: there is no workflow named `CI`; the latest relevant `Agent Doc` runs checked before this root commit were successful. Tmux evidence from the latest prior `Agent Doc` run was green.

One related integration note: root `make check` updated `src/agent-doc/Cargo.lock` to record the path dependency as `lazily 0.4.0`; I committed and pushed that one-line submodule commit as `adba3213`. I intentionally did not stage the root `src/agent-doc` pointer because it already contained pre-existing unrelated commits and this release closeout should not absorb them.

Confidence: high. Escalation: none.
<!-- agent:boundary:330b48b0 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review archive=tasks/software/lazily-rs.done.md -->
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/ -->
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
