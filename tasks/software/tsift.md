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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-204322.md`*

Compacted content:
- Archived 1 response topic(s): xlog
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Evaluate logs to assess the effectiveness and issues with tsift. #next-steps
### Re: tsift log review — gpt-5

I evaluated `.agent-doc/logs/tsift-v0.1.log` with the tsift commands that should explain this workflow: `log-digest`, `session-digest`, `session-cost`, `session-review`, `session-review --next-context`, `context-pack`, and `status`.

The good news: the earlier `signals:0` gap is fixed in the current code path. `log-digest` now reports `signal_groups: 8` from the 1,710-line log and correctly surfaces agent-doc closeout churn, restart churn, timeout, and nonzero-exit signals. It also extracts `file_ref_groups: 2` and `symbol_ref_groups: 144`, so the log is no longer treated as inert text.

The current log still shows real operational churn: `commit_already_current` dominates with 639 closeout-churn signal occurrences, plus `fresh_restart=28`, `quit_after_eof=17`, `ctrl_d_restart_loop=8`, `auto_trigger_timeout=5`, `claude_exit code=1` three times, and `claude_exit code=143` once. `session-cost` independently flags `restart_loop` and `noop_closeout`, with `commit_already_current` appearing 146 times in this log-level cost view.

The broader `session-review` is useful but also shows why this needs follow-up. Across 12 matched sessions it found 74,275,063 total tokens, 98.03% cached input, a largest turn of 220,368 tokens, and guardrails for prompt budget, cache resend, restart loop, and noop closeout. The latest matched session alone is much smaller at 686,248 tokens, so the aggregate and latest-session split is working and avoids misrepresenting historic spend as the active turn.

Main issue: `session-review --next-context` reports `unresolved_failures: []` even though the review and log digest both have actionable guardrails. That makes the resumable handoff too optimistic. The next context should carry guardrail/action-item signals forward so the following agent sees the restart/noop churn without needing to rediscover it.

Secondary issue: `quit_after_eof` is grouped as restart churn. Some of those may be clean user exits rather than a harness loop, so tsift should separate clean EOF/user quit from actual retry/fresh-restart loops. Otherwise the warning can be directionally right but noisy.

Repository status also needs attention: `tsift status . --json` reports the index is stale with 8 stale files and no summary cache. That does not invalidate the log assessment, but it means search/graph/symbol enrichment is weaker until the index is refreshed.

I added prioritized follow-up backlog items. Top backlog id: `#srgd`.
<!-- agent:boundary:dcfe5929 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#srgd] Promote session-review/log guardrails into next-context unresolved failures or action items; current review/logs report restart/noop churn while next-context says unresolved_failures is empty.
- [ ] [#qtoe] Tune agent-doc log-digest restart-churn classification to separate clean user quits/EOF from actual restart loops/timeouts, so quit_after_eof does not overstate risk.
- [ ] [#idxf] Refresh stale tsift index state and add a lightweight status/context-pack reminder when the tsift repo index is stale; status currently reports 8 stale files and no summaries.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-13 [#xcst] Fix session-cost/log guardrail wording for agent-doc logs so restart_loop warnings are only emitted when restart churn is actually present; ops.log currently reports fresh_restart=0 auto_trigger_timeout=0 ctrl_d_restart_loop=0 while still warning restart churn detected.
- 2026-05-13 [#xlog] Teach tsift log-digest to classify agent-doc runtime/churn/failure events as signals; current tsift-v0.1.log digest reports signals:0 even though session-digest finds claude_exit, auto_trigger_timeout, fresh_restart, ctrl_d_restart_loop, and closeout churn.
<!-- /agent:done -->
