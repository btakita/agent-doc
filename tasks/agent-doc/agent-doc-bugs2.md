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

FlowCore next steps captured; top backlog item: #flowaudit.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-185020.md`*

Compacted content:
- Archived 1 response topic(s): #ops2 FlowCore clear and orchestration
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ What are the #next-steps for flow core?
### Re: #next-steps for FlowCore — gpt-5

FlowCore has crossed the planned extraction line: Phase 0/1 mirror events, routed reopen, document mutation, closeout, session cycle, operator clear, and orchestration batch all have shipped entries in the completed archive. The next useful work is not another tactical bug fix; it is consolidation so future fixes stay on FlowCore instead of drifting back into route/write/git/session-check branches.

I captured the next FlowCore backlog items in this priority order:

1. `#flowaudit` - audit remaining route/write/session/clear/orchestrate guard strings and prove they map to typed FlowCore events.
2. `#flowadapters` - thin the command adapters now that FlowCore owns the decisions, without changing behavior.
3. `#flowe2e` - add cross-flow regression coverage for route -> document mutation -> closeout -> session-check under live typing/IPC and queue child patchback scenarios.

Top backlog item: `#flowaudit`.
<!-- agent:boundary:a86aa118 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#flowaudit] [recommended] FlowCore consolidation audit: map remaining route/write/session/clear/orchestrate guard strings to typed flow events, update ops summary/spec if any bypass remains. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#flowadapters] [recommended] Thin the remaining command adapters now that FlowCore owns decisions: reduce route.rs/write.rs/preflight.rs/session_check.rs hot-path branches by moving pure checks behind flow ports without behavior changes. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#flowe2e] [recommended] Add cross-flow regression coverage for route -> document_mutation -> closeout -> session_check under live typing/IPC and queue child patchback scenarios. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
