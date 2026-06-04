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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260604-005829.md`*

Compacted content:
- Archived 4 response topic(s): #lazyperf35 scratch buffer elimination; #lazyperf36 parking_lot vs std::sync A/B benchmark; finish_batch O(n) algorithm; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...
❯ release + publish

### Re: release + publish — glm-5.1

#### BENCHMARKS.md version fix

BENCHMARKS.md referenced version 0.5.1 while Cargo.toml was at 0.6.0. Regenerated via update-benchmark-results.py --no-run to update both the version header and benchmark data from existing criterion results.

#### Verification

make check passes all 83 spec compliance tests, 13 integration tests, 12 loom model tests, 2 tokio sync tests, fmt, clippy, and benchmark-check.

#### Release and publish

- Committed: 6228653 chore: update BENCHMARKS.md for v0.6.0 release
- Pushed to origin/main
- Tag v0.6.0 moved to 6228653 and force-pushed
- v0.6.0 was already published on crates.io
❯ The 0.6.0 release is not on github.

### Re: The 0.6.0 release is not on github. — glm-5.1

Created GitHub release: https://github.com/btakita/lazily-rs/releases/tag/v0.6.0

Release notes summarize #lazyperf25–36 performance work, new handle-centric APIs, feature flags, and CI infrastructure.

### Re: What are #next-steps to implement AsyncContext? — glm-5.1

AsyncContext needs to solve: async compute closures, in-flight slot state, cancellation of stale futures, and non-blocking propagation. Current `ThreadSafeContext` uses `Condvar` for blocking waits — AsyncContext would replace those with async-aware wakers.

#### Architecture sketch

`AsyncSlotNode` extends the slot state machine from `{unset, cached}` to `{unset, pending(Future), cached, error}`. When a slot's async compute is in-flight and a dependency invalidates, the stale future is cancelled and restarted. `AsyncContext` wraps a `ThreadSafeContext`-style graph but uses `tokio::task::spawn` for compute and `tokio::sync::Notify` for awaiting dependents.

#### Priority backlog

Top of backlog: `#asyncctx1`

- `#asyncctx1` Design spec — AsyncContext API surface, slot state machine, cancellation contract
- `#asyncctx2` AsyncSlotNode — pending/resolved/error states, JoinHandle tracking
- `#asyncctx3` AsyncContext core — computed_async, get_async (.await-based read), batch support
- `#asyncctx4` Async dependency tracking — suspend/resume across .await, automatic edge discovery
- `#asyncctx5` Cancellation — stale future abort on dependency invalidation, in-flight dedup
- `#asyncctx6` Async effects — effect_async with cleanup, rerun on dependency change
- `#asyncctx7` Tokio feature gate and integration tests
- `#asyncctx8` Benchmarks — async vs sync contention, cold resolve latency, cancellation throughput

What are #next-steps to implement AsyncContext?
<!-- agent:boundary:eba45d2c -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#sye8] `#asyncctx1` Design spec — AsyncContext API surface, slot state machine, cancellation contract
- [ ] [#kp7w] `#asyncctx2` AsyncSlotNode — pending/resolved/error states, JoinHandle tracking
- [ ] [#8sqb] `#asyncctx3` AsyncContext core — computed_async, get_async (.await-based read), batch support
- [ ] [#6z71] `#asyncctx4` Async dependency tracking — suspend/resume across .await, automatic edge discovery
- [ ] [#cmt9] `#asyncctx5` Cancellation — stale future abort on dependency invalidation, in-flight dedup
- [ ] [#eek4] `#asyncctx6` Async effects — effect_async with cleanup, rerun on dependency change
- [ ] [#r25x] `#asyncctx7` Tokio feature gate and integration tests
- [ ] [#35fc] `#asyncctx8` Benchmarks — async vs sync contention, cold resolve latency, cancellation throughput
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
