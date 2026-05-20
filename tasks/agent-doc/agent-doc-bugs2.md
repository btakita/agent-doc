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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-190430.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps for FlowCore
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: FlowCore audit/adapters/e2e — gpt-5

Implemented the FlowCore follow-up batch.

What changed:
- Added FlowCore-owned routed-reopen event helpers and removed remaining inline routed-reopen `FlowEvent::new` construction from `route.rs`.
- Expanded `agent-doc ops summary` so typed `flow_event` records get high-signal buckets for dispatch-proof failures, document-mutation pre-write guards, orchestration child closeout completion, and generic flow/stage/outcome fallback buckets.
- Added cross-flow regression coverage for route dispatch proof, live typing/IPC write deferral, closeout `session-check` interruption, and queue child plain-response patchback normalization.
- Updated the FlowCore map, ops-summary spec, and deterministic-simulation spec with the new ownership and coverage boundary.

Verification:
- `cargo test ops_summary`
- `cargo test typed_flow_events_cover`
- `cargo test route_failure_events_are_owned_by_routed_reopen_flow`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- CI/tmux review: latest GitHub CI run `26182778611` was green; job step `Run make tmux-ci` succeeded. Local `make tmux-ci` was not rerun because the latest CI tmux leg was green after runner startup.

Commits:
- `src/agent-doc`: `0b79bb2f` (`agent-doc flowcore audit adapters`)
- parent gitlink: `0bb1f23ce` (`agent-doc flowcore audit adapters`)
<!-- agent:boundary:6a5888d7 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#flowaudit]
- do [#flowadapters]
- do [#flowe2e]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
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
