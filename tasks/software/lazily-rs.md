---
agent_doc_session: 3d9ff8e4-bafd-4e1f-b48d-550d3c976904
agent: claude
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
queue: start
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260611-222220.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: next-steps to fix lazily-rs bugs — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix lazily-rs bugs?


**There are no open bugs right now.** I verified the current state rather than assume one:

- `cargo test --all-features` → **277 passed, 0 failed** (14 suites, 536s).
- `make test-loom` (exhaustive `thread_safe_loom` model) → **15 passed, 0 failed** (544s) — the just-landed sidecar-invalidation hardening (`c80ba70`) holds under interleaving.
- Latest CI (run `27367810445`) is **green**.

The two previously-tracked bug classes are already fixed in v0.10.3:
- `#k03k` async race (`get() already checked Resolved` / `notifier dropped` panics) — replaced by the resolve-loop at `src/async_context.rs:758+` that handles both windows; the panic strings are gone.
- thread-safe sidecar invalidation — hardened in `c80ba70` with stress + spec coverage.

So "fixing bugs" is now **bug-prevention**: the one real correctness gap is that the `#k03k` async fix has **no exhaustive interleaving model and is not in CI**. `ThreadSafeContext` has `tests/thread_safe_loom.rs` + a CI loom step (`.github/workflows` line 46-47); `async_context.rs` has only randomized stress (`tests/async_stress.rs`). A concurrency fix without a model is regression-prone.

**Backlog (priority order):**
1. **`#ytp5` `#lzasyncloom`** (top) — Add `tests/async_context_loom.rs` (`--features loom`) modeling the resolve loop's two `#k03k` windows + a CI step alongside `thread_safe_loom`. Plan: `tasks/software/plan-lazily-async-loom.md`.
2. `#dh3k` `#lzspecconf` [recommended] — Run the `lazily-spec` canonical conformance fixtures (`6632cb0`) in CI so cross-language wire-protocol drift fails the build instead of shipping as a runtime bug.
- **CI**: added a **Test async feature** step (async tests were **not run in CI at all** — `async` isn't a default feature) + the resolve-loop window step, beside the loom step.
- **`Makefile`**: `test-async-resolve` target → `check` + `.PHONY`.
- **`SPEC.md`**: documents *why Loom can't model this path* + the deterministic window coverage + the Shuttle follow-up.
- **`src/async_context.rs`**: seam (`Window1Hook`, one-shot hook field, resolved-arm counter, accessors) — entirely `#[cfg(feature = "instrumentation")]`, compiled out of default/release builds.
  - **Window 2** (superseded notifier drop → re-resolve): gated supersede, asserts re-resolve to latest, no panic.
  - **Window 1** (`Resolved` published in the *synchronous* gap between fast-path `get()` and re-lock — no `.await`, so cooperative scheduling can't reach it): forced via a one-shot `instrumentation`-gated seam (`__install_window1_hook`) that resolves the slot inside the gap; asserts the race arm was taken via `__window1_resolved_hits == 1`. Fails if the `#k03k` fix regresses to `unreachable!`.
- **`tests/async_resolve_loop.rs`** (`async` + `instrumentation`):

**Top of backlog: `#ytp5`** (`#lzasyncloom`).
<!-- agent:boundary:1a265bde -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- [#ytp5]
- [#dh3k]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#ytp5] #lzasyncloom=Deterministic #k03k resolve-loop window coverage (Option 1: instrumentation seam). Plan: tasks/software/plan-lazily-async-loom.md
- [ ] [#dh3k] #lzspecconf=[recommended] Run the lazily-spec canonical conformance fixtures (lazily-spec 6632cb0) in lazily-rs CI so cross-language wire-protocol drift fails the build instead of shipping as a runtime bug. tests/conformance.rs already prefers spec fixtures when present (02c9e80); add a CI step that fetches the spec submodule fixtures and runs them.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#57by] #lzasyncshuttle=[recommended, operator-gated] Exhaustive Shuttle interleaving model for the async resolve loop beyond the two known #k03k windows. Requires making src/async_context.rs generic over its concurrency primitives (swap tokio<->shuttle executor + watch + Arc under cfg) the way thread_safe.rs is generic over atomics for Loom — a real architectural change whose main beneficiary is the test harness, verifying an approximation of tokio rather than the real path. BLOCKED on operator approval to take on the tokio-decoupling cost; do only if the async graph grows more concurrency-critical paths. Window 1+2 already deterministically covered by tests/async_resolve_loop.rs.
- [/] [#jpf1] #webrtc-data=WebRTC data channel transport for peer-to-peer IPC (beyond signaling)
- [/] [#w30b] #gh-org=Create GitHub org (lazily-reactive or lazily) and transfer repos
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
