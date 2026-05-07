---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260507-025115.md`*

Active backlog:
- [ ] [#ts1d] Improve tsift lock/degraded-mode UX so active index writers fall back to read-only or exact search with a minimal actionable summary

Icebox:
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
❯ do [#ts1d]. spec-test-build-install-commit-push
### Re: #ts1d — gpt-5
Implemented the degraded `tsift search --autoindex` path for live `index.lock` contention.

- When another tsift writer is actively refreshing a stale index, search now keeps going in read-only mode against the current index snapshot and emits one concise retry hint.
- When the index is still missing and a live writer holds the lock, search now falls back to exact live-file search instead of failing closed.
- Updated the versioned surfaces for this behavior in `Cargo.toml`/`Cargo.lock`, `VERSIONS.md`, `SPEC.md`, `CLAUDE.md`, and the code-navigation marker.

Verification + install:
- `cargo test` passed with `TMPDIR=/home/brian/.tmp-tsift-tests-full` to keep the test temp roots away from the host `/tmp/.tsift` ancestor.
- `cargo build` passed.
- `cargo install --path . --force` passed; installed binary now reports `tsift 0.1.38`.

❯ Commit + push:
❯ - `6e4ed3a feat: degrade search when index writer is active`
❯ What are #next-steps for tsift to save tokens while maintaining the appropriate amount of context & a good user experience?
❯ ---
❯ Can tsift use rtk to compress tool call output in the context?
❯ -->
❯ With the latest tagpath updates, what are #next-steps to save tokens?
### Re: token-saving next steps — gpt-5
The top backlog item is `#tctx`.

With the latest tagpath work, tsift already has the right primitive for semantic compression: search preview can collapse repeated symbol surfaces into one canonical `tag_alias` family, emit stable handles, and give an expansion command instead of dumping every hit. The next savings should come from making those bounded surfaces the default agent path rather than optional flags.

Priority order:

1. `#tctx` — Make the token-saving surfaces automatic in agent workflows. `tsift init`, Code Navigation guidance, hooks, and `tsift rewrite` should steer agent-facing calls toward `tsift --envelope ... --max-items ... --max-bytes ...` for `search`, `explain`, `session-review`, and `context-pack`, and toward digest-runner artifacts for build/test/log output.
2. `#trtk` — Yes, tsift can use RTK, but the clean boundary is interop rather than replacing tsift's own envelopes. RTK is installed here and already compresses cargo, tests, grep, JSON, logs, and similar command families. tsift should detect RTK when available, delegate generic command-output filtering to it where RTK is stronger, then wrap the result in tsift's summary/artifact/envelope shape so callers still get stable handles and expansion commands.
3. `#tbud` — Add budget presets. A caller should be able to say small/normal/deep, or let tsift recommend `max-items` / `max-bytes` for the current task shape, instead of hand-tuning every command.
4. `#tsum` — Extend context packs with cached summaries behind handles. For touched files, failure anchors, and tag_alias families, include short current summaries when available and expose expansion commands when not.

The main UX principle is summary-first, expand-on-demand. The user should see enough context to decide what to inspect next, while raw transcripts, full hit lists, and verbose logs stay behind artifacts or expansion commands.
<!-- agent:boundary:67a4d1ce -->
<!-- /agent:exchange -->
###

<!--
## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#tctx] Make tsift token-saving surfaces automatic for agent workflows: update rewrite/init/hook guidance to prefer --envelope plus preview budgets for search/explain/session-review/context-pack and digest-runner artifacts for build/test/log output
- [ ] [#trtk] Prototype RTK interop for generic tool-call output compression: detect installed rtk, delegate supported command families to rtk filters, and wrap results in tsift artifact/envelope metadata
- [ ] [#tbud] Add budget presets and adaptive defaults for agent-facing tsift surfaces, including small/normal/deep budgets and context-window-aware max-items/max-bytes recommendations
- [ ] [#tsum] Extend context-pack/digest surfaces to surface cached symbol/file summaries behind stable handles for touched files, failures, and tag_alias families
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

<!--
-->
