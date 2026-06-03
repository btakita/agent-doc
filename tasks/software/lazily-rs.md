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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260603-012347.md`*

Compacted content:
- Archived 3 response topic(s): #lazyperf11; Can we implement lazy-invalidation for more concurrent performance?; #next-steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: lazily-rs queued performance work — gpt-5

> **Queue prompt:**
>
> [#lazyperf13]
>
> [#lazyperf14]
>
> [#lazyperf15]


Completed `#lazyperf13`, `#lazyperf14`, `#lazyperf15`, and the benchmark update request.

Implemented in `src/lazily-rs` commit `2642d81` and pushed to `btakita/lazily-rs` `main`; updated the parent submodule pointer in `agent-loop` commit `9e0fce300` and pushed to `btakita/agent-loop` `main`.

What changed:
- Added optional instrumentation counters for sidecar invalidation frontiers, sidecar dirty marks, sidecar fallbacks, and dirty epoch advances, wired through `ThreadSafeContext`.
- Added high-parallel graph propagation benchmark/profile cases for eager fan-out validation, lazy fan-out/fan-in dirty epochs, and batched fan-in flush.
- Regenerated README benchmark tables and extended `SPEC.md` with the cache-revision dirty epoch / sidecar benchmark gate.
- Added spec and Loom coverage for dynamic effect dependency switching, cleanup, disposal, and stale edge cleanup under races.

Verification:
- `cargo fmt --all --check`
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo build --locked --all-targets --all-features`
- `cargo test --locked`
- `cargo test --locked --features tokio`
- `cargo test --locked --features loom --test thread_safe_loom`
- `python3 scripts/update-benchmark-results.py` and `python3 scripts/update-benchmark-results.py --check`
- `cargo run --example instrumentation_profile --features instrumentation --quiet`
- GitHub Actions `btakita/lazily-rs` CI run `26859281017` passed, including the Loom leg.

`cargo install --path .` is not applicable because `lazily-rs` is a library crate with no binaries. There is no `Makefile` in `src/lazily-rs`; the root `make check` target does not cover this submodule. Root `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
<!-- agent:boundary:9ab5326c -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~[#lazyperf12]~
- ~[#lazyperf13]~
- ~[#lazyperf14]~
- ~[#lazyperf15]~
- Add Makefile
- Update benchmarks
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#lazyperf12] Implement lazy invalidation for ThreadSafeContext propagation under parallel load: mark downstream nodes dirty/versioned during writes, defer derived recompute/effect scheduling until observed or batch flush, and preserve cleanup/disposal ordering.
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
