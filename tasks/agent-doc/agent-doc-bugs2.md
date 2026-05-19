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

Reviewed recurring bug logs; added FlowCore refactor plan. Top backlog item: #flowcore0.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-151147.md`*

Compacted content:
- Archived 1 response topic(s): recurring agent-doc bugs and refactor path
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:47c61073 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#flowcore0] [recommended] Implement FlowCore Phase 0/1 from tasks/agent-doc/plan-agent-doc-flowcore-refactor.md: inventory hot-path state ownership, add typed flow events/outcomes in mirror mode, and make ops summary group failures by flow stage before behavior changes.
- [ ] [#flowroute] [recommended] Extract routed reopen from route.rs into flow::routed_reopen with pure route decisions, prompt-ready barrier, dispatch-start proof, and one bounded retry/fail-closed policy. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#flowclose] [recommended] Extract closeout/document mutation from write/git/template/session_check into shared flow::closeout and flow::document_mutation so IPC, stream, direct finalize, and repair use one terminal-state guard. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#flowbatch] [recommended] Move Clear Session Context and orchestrate/queue batch behavior onto FlowCore typed outcomes after route/closeout extraction; cover interrupt-clear, active agent-doc pane refusal, and child patchback normalization. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
- [ ] [#873y] Plan filed for the wedged-cycle incident: a failed finalize (blocked at the pre-write pending_capture_guard) left the cycle marked committed; finalize, write --commit, repair, and reset then all failed to recover it; an h4 '#### #next-steps' heading tripped the prompt-target parser; compact became the only (destructive) escape and rewrote the document unrequested. Architectural fix needed — see tasks/agent-doc/plan-wedged-cycle-unrecoverable.md
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
