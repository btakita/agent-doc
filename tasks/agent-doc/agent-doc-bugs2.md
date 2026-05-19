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
### Re: FlowCore refactor — gpt-5

Implemented the first FlowCore pass across the requested backlog batch.

Completed:

- `#flowcore0`: added the Phase 0 flow ownership map at `src/agent-doc/docs/reference/flow-map.md`, linked it from the docs index, and added `src/agent-doc/src/flow` with typed flow names, stages, outcomes, closeout states, route decisions, dispatch proof, patchback shape, and mirror-mode event logging.
- `#flowroute`: added `flow::routed_reopen` pure route decision coverage for starting, dispatch-only busy, managed busy, degraded authority, and missing actor outcomes; route now emits typed `flow_event` diagnostics for prompt-ready-barrier fail-closed branches.
- `#flowclose`: added `flow::closeout` terminal-state helpers and closeout mirror events for already-current commit completion; closeout specs now name FlowCore stages for patchback parse, mutation, snapshot convergence, commit, and session-check.
- `#flowbatch`: added `flow::orchestration_batch` outcome scaffolding and updated the orchestration spec to assign queue freeze, child closeout normalization, mid-run mutation stops, and child proof recording to FlowCore.

Also added `flow::document_mutation` patchback-shape classification, wired malformed template patchbacks into typed flow diagnostics, and updated `agent-doc ops summary` so flow events group by stage instead of only by tactical log strings. Changelog entry: `VERSIONS.md` Unreleased.

Implementation commits:

- `src/agent-doc`: `ab26b3df` (`feat: add flowcore mirror events`), pushed to `origin/20260422/codex-agent-backend`.
- parent repo gitlink: `dc6bf9f9e` (`chore: update agent-doc flowcore`), pushed to `origin/main`.

Verification:

- `cargo check`
- `cargo test flow:: --all-targets`
- `cargo test ops_report::tests::ops_summary_groups_tracked_events_by_file_and_session --all-targets`
- `make check`
- `make install`
- CI review: latest `CI` run `26105584438` was green and included successful `Run make tmux-ci`.
<!-- agent:boundary:d6166f93 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#flowcore0]
- do [#flowroute]
- do [#flowclose]
- do [#flowbatch]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
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
