---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Added auto-DAG next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-054342.md`*

Compacted content:
- Archived 1 response topic(s): job packet graph gate
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Review session logs to add #next-steps to fix agent-doc bugs.
❯ What are the #next-steps to have agent-doc have an auto-dag feature where task dependencies are setup into a dag schedule and run as a dag. Add pending items to this document and to tsift.md
### Re: auto-DAG next steps — gpt-5

I reviewed the compacted exchange, completed-work archive, tsift backlog, and `tsift session-review --next-context`. The log signal is clear: the graph-backed job packet gate is now done, so the next step is scheduler lifecycle control rather than another context-packet contract. The session logs also show prompt-budget/cache-resend, restart-loop, and no-op closeout guardrails, so the scheduler needs preflight gates before it starts a large multi-agent run.

Top backlog item: `#autodag`.

I added the agent-doc items in priority order: `#autodag`, `#dagdispatch`, `#dagrecover`, `#daglogs`.

I also added the tsift support items: `#tsdag`, `#impactdag`, `#dagfix`.
<!-- agent:boundary:c7b1a78c -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
-->


<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#w25k]
- do [#aggtp]
- do [#jpktscope]
- do [#jpktwsmis]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#daglogs] [recommended] Use session-review guardrails as scheduler input: classify prompt-budget/cache-resend/restart-loop/noop-closeout log families before auto-DAG start and gate large or looping runs into compact/restart/fixture-fix steps.
- [ ] [#dagrecover] [recommended] Add auto-DAG persistence/recovery semantics: write operation docs/job packets with schedule id, node state, attempt count, replay commands, and repair commands so interrupted DAG runs can resume idempotently after compact/finalize/restart.
- [ ] [#dagdispatch] [recommended] Execute auto-DAG batches through agent-doc orchestrate/jobs: run parallel-safe antichain batches, collect worker_result proof, unblock downstream nodes only after required proof/session-check passes, and stop deterministically on failed/gated nodes without launching dependents.
- [ ] [#autodag] [recommended] Add an agent-doc auto-DAG planner for queued do/compound directives: parse explicit depends-on text, backlog ids, graph/conflict evidence, and job packets into a topological schedule; fail closed on cycles, missing ids, stale graph evidence, or ambiguous ownership before dispatch.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
