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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Graph orchestration next steps refreshed; top tsift backlog item: #dtrc.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-033842.md`*

Compacted content:
- Archived 1 response topic(s): graph orchestration contracts
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Update #next-steps in agent-doc-bugs2.md with latest tsift features.
❯ Add #next-steps to tsift.md for agent orchestration and graphdb features.
### Re: graph orchestration next steps — gpt-5

I updated the next-step backlog split after the latest tsift graph orchestration features.

Top tsift backlog item: `#dtrc` — add dispatch-trace replay/contract fixture coverage over a real agent-doc queue run.

Top agent-doc backlog item added to `tasks/agent-doc/agent-doc-bugs2.md`: `#dxtr` — consume tsift dispatch-trace outputs in agent-doc plan/orchestrate closeout.

The queued `#wrfb` / `#gdtr` tsift work is already implemented and pushed in the compacted graph orchestration contracts turn (`btakita/tsift` `9728ade`, parent submodule pointer `39117c97b`). This cycle only records the next follow-ups and queue bookkeeping. I did not rerun tests because no code changed in this cycle; the implementation turn already recorded `make check`, `cargo build`, local install, and green tsift CI for those features.
<!-- agent:boundary:74841584 -->
<!-- /agent:exchange -->
###


<!-- agent:queue auto -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#wfcl] [recommended] Add worker-feedback closure ranking controls to conflict-matrix and dispatch-trace: deterministic warnings for repeated blockage, stale expected tests, and follow-up debt that can reorder otherwise safe candidates without weakening file/symbol/test/config hard gates.
- [ ] [#gafe] [recommended] Add an agent-orchestration acceptance pack in tsift fixtures: sample agent-doc job packets, worker_result rows, conflict-matrix output, dispatch-trace export, and context-pack/session-review follow-up commands that agent-doc can consume as golden contracts.
- [ ] [#dtrc] [recommended] Add dispatch-trace replay/contract fixture coverage over a real agent-doc queue run: refresh -> evidence -> conflict-matrix -> dispatch-trace JSON/HTML, then assert trace links, worker_feedback summaries, packet ids, and repair/replay commands stay consistent with graph.db.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
