---
agent_doc_session: tsift-v0.1
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
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-021521.md`*

Compacted content:
- Archived 1 response topic(s): release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: source-read windows vs graph — gpt-5

The `--start` / `--lines` usages I found are generated expansion windows, not evidence that the agent is skipping the semantic graph.

In the `agent-doc-bugs2.md` session, the compacted markdown body does not contain older hand-written `source-read --start --lines` calls. The current `tsift context-pack` for that session does two things side by side:

- `graph_orchestration` reports the semantic graph projection as ready/current and records relationship evidence, for example the session context mentioning symbols such as `project_controller`, `queue_command`, `queue_dispatch`, `codex_hook`, and `state_store`.
- `exploration.source_windows` then emits bounded follow-up commands such as `tsift --envelope source-read "tasks/agent-doc/agent-doc-bugs2.md" --style window --start 1 --lines 96 --budget normal` for the concrete touched files.

So the intended flow is: graph/search/session-review find and rank the relevant nodes, then `source-read --start/--lines` gives a small, line-numbered text window when the agent needs literal source or markdown context. That is still better than `cat`, `sed`, or full-file reads because it is bounded and carries AST/markdown refs when available.

The code path matches that interpretation. `build_context_pack_exploration_packet` builds `relationship_map` first, then adds `source_windows` for changed/touched files. `context_exploration_source_window` currently starts those touched-file windows at line 1 and uses the normal budget line count. The formatter for those expand commands is `source_read_window_command`, which deliberately includes `--style window --start <n> --lines <n>`.

The real rough edge is not that `--start` / `--lines` exists. The rough edge is that for broad markdown/session touched-file entries, context-pack only has a file-level reason, so it emits a line-1 preview instead of a more semantic section/block/symbol target. For code symbols, the better path is still `symbol-read` / graph expansion first; for markdown sessions, tsift needs stronger section/block anchors if we want those expansion commands to stop looking like line slicing.

Changed paths: none.

Commands: `tsift status`, `tsift status --fix`, `agent-doc preflight tasks/software/tsift.md`, `tsift --envelope session-review tasks/agent-doc/agent-doc-bugs2.md --next-context --budget normal`, `tsift --envelope context-pack tasks/agent-doc/agent-doc-bugs2.md --budget normal`, `rg` for `source-read` / `--start` / `--lines`, `tsift --envelope search`, `tsift --envelope symbol-read`, `tsift graph`, `agent-doc plan tasks/software/tsift.md`.

Verification: no code changed. The explanation is verified against the live `context-pack` output and the tsift implementation paths that generate the `source_windows` expansion commands.

Confidence: high.

Escalation: none.
<!-- agent:boundary:70d7d940 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#tsiftinitboost] Rerun plain `tsift init` in `src/boost-client` and `src/boost-client/src/monsterrodholders-dev` after the active monsterrodholders workflow stops rebasing/resetting those worktrees; commit their `AGENTS.md` updates and any resulting pointers.
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
