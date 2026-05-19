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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-150125.md`*

Compacted content:
- Archived 2 response topic(s): #wnj2; Clear Session Context idle Codex placeholder
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
The same set of bugs have been worked on more than 1 week using both Codex + Claude Code. Numerous turns have been spent attempting to fix the bugs but none of the set of bugs have been fixed. Can we refactor the architecture of agent-doc to make it easier for agents to work with. The effectiveness of agents working on the codebase is currently compromised due...I believe due to how the complexity is managed.

There is too much emphasis "tactical" "fixes" that simply DO NOT FIX THE BUGS. Not enough emphasis on refactoring and architecural fixes. There seem to be deep architectural issues that the agents are unable to effectively fix. I'm concerned about rampant duplicate logic and logic spread all over the codebase.

How can we refactor the codebase so the use case logic is consolidated and easy for agents to work with.

Review my session logs to see the many attempts to fix the same bug over the past week. What are the #next-steps to refactor the codebase for agents to more effectively work with the codebase? Please assess reoccurring bugs and #next-steps to manage the flows and fix these reoccurring issues.

### Re: recurring agent-doc bugs and refactor path — gpt-5

Yes. I agree this should move from tactical patches to an architectural refactor.

I reviewed the recent `agent-doc-bugs2.md` archives, the completed-work archive, current ops summary, and the hot-path module sizes. The recurring bugs are not one bug. They are repeated failures of ownership boundaries:

- Route/readiness: starting actors, busy panes, accepted-only dispatch, stale supervisor proof, and Clear Session Context racing active panes.
- Closeout/patchback: zero-patch writes, late fallback writers after commit, malformed template patchbacks, duplicate scaffold, stale snapshot drift, and response-not-committed repairs.
- Prompt ownership: stripped or missing `❯ ` prefixes, live-typing duplicate prefixes, sidecar normalization fallback, and IPC ack content becoming the wrong source of truth.
- Queue/backlog/orchestration: pending order, cross-document pending, backlog orphan prompts, queue directive parsing, and child closeout interruptions.

The common root cause is that the use-case lifecycle is spread across `route.rs`, `write.rs`, `git.rs`, `sync.rs`, `preflight.rs`, `session_check.rs`, and orchestration/editor code. Those modules each know part of the truth about prompt ownership, document mutation, route proof, snapshots, cycle state, commit state, and editor-visible state. Agents then patch the local branch they are staring at instead of changing the flow contract.

I added the architecture plan at `tasks/agent-doc/plan-agent-doc-flowcore-refactor.md` and committed it as `b97aa0470` (`agent-doc: add flowcore refactor plan`). The plan introduces a `flow` layer with typed state machines for `session_cycle`, `routed_reopen`, `closeout`, `document_mutation`, `operator_clear`, and `orchestration_batch`.

Recommended next step is not to rewrite `route.rs` first. Start with FlowCore Phase 0/1: inventory the hot-path state ownership, add typed flow events/outcomes in mirror mode, and make ops summary group failures by flow stage while preserving behavior. Then extract routed reopen, then closeout/document mutation, then Clear Session Context and orchestration.

I added the follow-ups to the top of the backlog in that order. Top backlog item: `#flowcore0`.

Verification: docs-only plan change; I ran `git diff --check` for the new plan and committed only that file before this session closeout. I did not run the full Rust/tmux suite because no runtime code, tests, build logic, or instruction surfaces changed.
<!-- agent:boundary:1b32ac42 -->
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
