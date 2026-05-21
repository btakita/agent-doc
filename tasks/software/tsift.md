---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: opus
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
queue_active: false
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-014846.md`*

Compacted content:
- Archived 1 response topic(s): do #802b
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps for graph db features & subagent support?
### Re: graph db features & subagent support — gpt-5

The graph DB base is already fairly complete: SQLite correctness store, Convex projection/snapshot path, conformance coverage, `graph-db evidence`, and `conflict-matrix` are all represented in the completed work. The next useful layer is making those graph outputs directly drive agent-doc/subagent orchestration instead of remaining operator-facing tsift commands.

I added the follow-up backlog in priority order. Top backlog item: `#agbr`.

1. `#agbr` — wire `graph-db evidence` and `conflict-matrix` into agent-doc planning/orchestration, with fail-closed stale/missing-target checks.
2. `#wctx` — promote conflict-matrix worker prompt packets into first-class subagent context: owned files/symbols, read-only context, forbidden files, expected tests, expansion commands, and token budgets.
3. `#mxfx` — add a deterministic multi-worker acceptance fixture proving overlapping targets are blocked and non-overlapping worker prompts are safe to dispatch.
4. `#gobx` — expose graph orchestration observability through `context-pack` / `session-review`, including freshness, evidence ids, dispatch decisions, and next commands.
5. `#semx` — fold semantic graph rows into dispatch ranking so related concept/entity paths improve worker-scope recommendations without replacing hard file/symbol conflict gates.
❯ Any backlog work that should be done on the agent-doc codebase should be transferred to agent-doc-bugs2.md
### Re: transfer agent-doc backlog — gpt-5

Transferred the agent-doc-owned work out of this tsift backlog and into `tasks/agent-doc/agent-doc-bugs2.md` as `#agbr`.

I treated `#agbr` as the agent-doc-codebase item because it changes `agent-doc plan/orchestrate` behavior. The remaining items stay here because they are tsift-owned graph/reporting work: `#wctx`, `#mxfx`, `#gobx`, and `#semx`.

Top tsift backlog item is now `#wctx`. Top transferred agent-doc item id is `#agbr`.
<!-- agent:boundary:b0467a9e -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#atvp]
- do [#g6kf]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#wctx] [recommended] Make worker prompt packets first-class: emit owned files/symbols, read-only context, forbidden files, expected tests, expansion commands, and token budgets from conflict-matrix so subagents can start with bounded graph-backed context.
- [ ] [#mxfx] [recommended] Add a deterministic multi-worker acceptance fixture for subagent support: seed overlapping and non-overlapping backlog targets, run graph-db evidence plus conflict-matrix, and prove the generated worker prompts prevent shared-file/test conflicts.
- [ ] [#gobx] [recommended] Add session-level graph orchestration observability: record graph projection freshness, evidence packet ids, conflict-matrix decisions, worker ownership blocks, and follow-up commands in context-pack/session-review output.
- [ ] [#semx] [recommended] Use semantic graph rows in dispatch ranking: combine semantic_concept/entity edges with code/route/backlog paths so conflict-matrix can recommend related-but-safe follow-up worker scopes.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
