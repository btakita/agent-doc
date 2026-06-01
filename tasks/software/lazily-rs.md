---
agent_doc_session: 9386d273-2288-498c-b9fc-207f224f9fdb
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-040101.md`*

Compacted content:
- Archived 2 response topic(s): Update README, audit docs, release; Docs updated, pushed
❯ What are #next-steps? Are derived signals a priority? Should we focus on effect system first?

### Re: Next steps, derived signals vs effect system — glm-5.1

**Derived signals are already mostly done.** `ctx.slot()` is a derived/computed signal — it auto-tracks dependencies, recomputes lazily, and propagates invalidation. The core reactive graph is in place.

**Phase 3 (Derived signals) should focus on ergonomic enhancements:**
- `ctx.computed()` alias with cleaner ergonomics
- Memoization guard — skip recomputation when dependency values haven't actually changed (currently a slot recomputes every time its cache is invalidated, even if the input resolves to the same value)
- Batch updates — `ctx.batch(|ctx| { ... })` to defer invalidation until the batch completes

**Phase 4 (Effect system) is the higher-value next step.** Effects run side-effect code when dependencies change — this is what makes the library useful for reactive UIs, DOM updates, logging, etc. It builds directly on the existing dependency graph. Key design decisions:
- `ctx.effect(|ctx| { ... })` — registers an effect, auto-tracks deps on first run
- Effect scheduling — when to run effects (synchronously after invalidation? deferred to next tick?)
- Cleanup — effects may return a cleanup function that runs before re-execution
- Disposal — ability to stop an effect

**Recommended priority order:**

1. **Effect system** (Phase 4) — highest impact, enables real reactive usage
2. **Batch updates** — needed to prevent cascading invalidation bugs in effect-heavy code
3. **Memoization guard** — performance optimization for expensive computations
4. **API ergonomics** (`computed()`, better DX)
5. **Release + publish** — after effect system lands

Top of backlog: `#effect-system`
<!-- agent:boundary:a3f8e27f -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
release + publish
release + publish
- [ ] [#exfd] #effect-system Phase 4: Effect system — ctx.effect(|ctx| { ... }) with auto-tracking, scheduling, cleanup, and disposal
- [ ] [#rx9y] #batch-updates ctx.batch(|ctx| { ... }) to defer invalidation until batch completes
- [ ] [#fzaf] #memo-guard Memoization guard — skip recomputation when dependency values haven't changed
- [ ] [#dpzd] #api-ergonomics ctx.computed() alias and DX improvements
- [ ] [#45rh] #release-publish Release + publish after effect system lands
<!-- /agent:backlog -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Review

<!-- agent:review archive=tasks/software/lazily-rs.done.md -->
<!-- /agent:review -->
