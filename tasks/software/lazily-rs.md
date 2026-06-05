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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260605-065513.md`*

Compacted content:
- Archived 2 response topic(s): [#vbyc] Phase 2 (#rdstrat2); [#9mhz] Phase 3 (#rdstrat3)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: [#k03k] Fix async_context.rs get_async concurrency race — opus-4-8

> **Queue prompt:**
>
> [#k03k]


Fixed, regression-tested, committed + pushed (`lazily-rs` `e6ae8b3`, superproject `d1a4ec06a`).

**Root cause** — `get_async` asserted two states that are reachable under concurrency:

1. `unreachable!("get() already checked Resolved")` — the slot can transition `Computing → Resolved` between the `get()` fast-path check (which drops the lock) and the re-lock. Now reads the resolved value directly.
2. `panic!("get_async: notifier dropped unexpectedly")` — a superseded (stale-revision early-return) or invalidated compute drops all `watch` senders without a final `Resolved` send. Now treated as "the world changed": re-resolve from authoritative slot state.

**Fix** — `get_async` is now an outer re-resolve loop: each pass re-reads via `get()`, attaches to / spawns a compute, and on a dropped notifier restarts to observe current state (no hot spin — every non-returning pass awaits `recv.changed()` or a fresh spawn). The published value was always correct; this only removes the panics on benign races.

**Verification:**
- New `async_context_concurrent_set_and_get_async_never_panics_k03k` (multi-thread tokio, 8 workers × 250 iters of `set_cell`+`get_async`) — **fails 5/5 on the prior code** (`notifier dropped unexpectedly`), **passes 3/3 with the fix**.
- The original repro — `async_concurrent_contention` benchmark (1/4/16 workers) — now runs clean, no panic.
- fmt, clippy `--all-features -D warnings`, default/async/tokio suites green. Loom suite unaffected (models `thread_safe` only). This **unblocks `make benchmark-update`**'s `--features instrumentation,async,tokio` path.

**Note:** this is a post-`0.10.0` fix; the bug is live in the published `0.10.0` async feature. Say the word and I'll cut a `0.10.1` patch release (crates.io + tag + GitHub release).

Queue drained.
<!-- agent:boundary:1fa22c13 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue 

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ipc1] [recommended] Design lazily-serde feature gate (trait bounds vs type-erased closures)
- [/] [#ipc2] [recommended] Design lazily-ipc snapshot + incremental update protocol
- [/] [#ipc3] [recommended] Investigate CRDT vs Raft trade-offs for lazily-distributed
- [/] [#39c5] #sig1 [recommended] Implement RemoteOp enum + per-peer permission allowlist for lazily-distributed remote access
- [/] [#yxjw] #sig2 [recommended] Build Cloudflare Worker signaling server for internet-scale peer discovery
- [/] [#vd5v] Future: leverage typed cache for lock-free cached reads in ThreadSafeContext (#lazilyperf38)
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily-rs.done.md -->

<!-- completed work archived in tasks/software/lazily-rs.done.md -->
<!-- /agent:done -->
