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

### Re: Slot invalidation vs recomputation — glm-5.1

**You're correct — slots are only invalidated, not recomputed.** My previous wording was imprecise. Here's the exact flow from the code:

1. `set_cell()` (`context.rs:249-283`) — changes cell value, then calls `clear_slot()` on each dependent
2. `clear_slot()` (`context.rs:288-305`) — sets `slot.value = None` (pure invalidation), then recursively clears *its* dependents. No recomputation happens here.
3. `get_slot()` (`context.rs:116-208`) — checks `slot.value.is_some()`. If cached, returns immediately. If `None` (invalidated), recomputes. **This is the only place recomputation happens, and only on access.**

So the flow is: cell change → cascade of `value = None` through the dependency graph → nothing else until someone calls `get()`. Fully lazy.

**What the memo guard would actually do:** It's not about skipping recomputation (that's already lazy). It's about preventing *unnecessary downstream cascade* during step 2. Currently `clear_slot` recursively clears all dependents regardless of whether the recomputed value would actually change. A memo guard would:

- After recomputation in `get_slot()`, compare the new value with the prior value
- If unchanged, skip re-clearing dependents (they were already cleared, but the memo guard would restore their cached values or mark them as still-valid)

This matters when: Cell A → Slot B → Slot C. If A changes but B would compute to the same value, C was cleared unnecessarily. The memo guard at B would prevent C from needing recomputation.

IPC corruption bug already tracked as `#zpc2` in `agent-doc-bugs2.md`. Plans: `tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md`, `tasks/agent-doc/plan-prompt-duplicated-while-typing.md`.

### Re: Updated #fzaf backlog item — glm-5.1

Updated `#fzaf` to reflect that slots are already lazy (invalidated only, recomputed on `get()`). The memo guard is about preventing unnecessary downstream cascade — when a slot recomputes to the same value, its dependents were already cleared but didn't need to be.

New text: `#memo-guard Memoization guard — prevent unnecessary downstream cascade during invalidation when recomputed value equals prior value (slots are already lazy: invalidated only, recomputed on get())`

<!-- agent:boundary:79a90e92 -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#n12s] #ipc-corruption-live-repro-lazily-rs Logged as #zpc2 in agent-doc-bugs2.md backlog. Live repro of scaffold duplication during finalize --stream. Document manually repaired.
- [ ] [#exfd] #effect-system Phase 4: Effect system — ctx.effect(|ctx| { ... }) with auto-tracking, scheduling, cleanup, and disposal
- [ ] [#rx9y] #batch-updates ctx.batch(|ctx| { ... }) to defer invalidation until the batch completes
- [ ] [#fzaf] #memo-guard Memoization guard — prevent unnecessary downstream cascade during invalidation when recomputed value equals prior value (slots are already lazy: invalidated only, recomputed on get())
- [ ] [#dpzd] #api-ergonomics ctx.computed() alias and DX improvements
- [ ] [#45rh] #release-publish Release + publish after effect system lands
<!-- /agent:backlog -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Review

<!-- agent:review archive=tasks/software/lazily-rs.done.md -->
<!-- /agent:review -->
