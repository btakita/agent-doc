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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57a5d8366a1fa779d4afde380ce07377133e6daf1d8810384e1383c400-20260601-042343.md`*

Compacted content:
- Archived 1 response topic(s): Next steps, derived signals vs effect system
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/c9ad2e57...

### Re: Memo guard utility, IPC corruption logged — glm-5.1

**Yes, the memo guard is useful** — but it's a performance optimization, not a correctness feature. The current behavior (recompute every time cache is invalidated) is correct. The memo guard adds:

1. **Skip redundant recomputation** — if `ctx.get(&slot)` triggers recomputation but all dependency values resolve to the same result, the slot returns cached value. Matters for expensive computations downstream of frequently-toggled cells.
2. **Reduced invalidation cascade** — a memo guard at intermediate nodes prevents cascading through the whole graph when intermediate values don't change.

Trade-off: requires storing dependency value copies for comparison. Worth doing after the effect system, since effects are where cascading recomputation hurts most.

**IPC corruption logged** as `#zpc2` in `tasks/agent-doc/agent-doc-bugs2.md`. This is a new live repro of `#ipcfullprompt-recur2` / `#prompt-duplicated-while-typing`. The corrupted lazily-rs.md had full scaffold duplication (2x Backlog/Queue/Review + 2x `&lt;!-- /agent:exchange --&gt;` + partial typing "re [#fzaf]: W" / "re [#fzaf]: Would" embedded in wrong positions). Manually repaired — removed duplicate tail sections.
<!-- agent:boundary:18a11b55 -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#n12s] #ipc-corruption-live-repro-lazily-rs Logged as #zpc2 in agent-doc-bugs2.md backlog. Live repro of scaffold duplication during finalize --stream. Document manually repaired.
- [ ] [#exfd] #effect-system Phase 4: Effect system — ctx.effect(|ctx| { ... }) with auto-tracking, scheduling, cleanup, and disposal
- [ ] [#rx9y] #batch-updates ctx.batch(|ctx| { ... }) to defer invalidation until the batch completes
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
