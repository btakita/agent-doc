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

Added prioritized FlowCore refactor next steps. Top backlog item: #rte4.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-224900.md`*

Compacted content:
- Archived 1 response topic(s): FlowCore next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

❯ do [#rte4]
❯ synchronous orchestara
❯ dispatch #spec-test-build-install-commit-push
❯ - do [#rte4]
❯ - do [#mut3]
❯ - do [#clo3]
❯ - do [#cyc2]
### Re: routed-reopen FlowCore slice — gpt-5.5
Completed #rte4.

I extracted the authoritative actor routed-reopen slice into `flow::routed_reopen`: ready-wait facts/logging, retry budgets, delivery-action classification, recovery hints, and dispatch-start proof typing now live in FlowCore. `route.rs` now adapts tmux/supervisor/controller facts into those typed helpers and performs only the selected side effect.

Updated the session-actor spec and Unreleased notes. Verification passed: `cargo check`, `cargo test routed_reopen`, `make check`, `cargo build`, and `cargo install --path .`. Latest GitHub CI run `26130637848` was green, including `Run make tmux-ci`, so no local tmux rerun was required.

Committed and pushed submodule commit `3beb84b0` to `btakita/agent-doc` branch `20260422/codex-agent-backend`, then committed and pushed parent gitlink commit `3737e8cce` to `btakita/agent-loop` `main`.
### Re: FlowCore closeout, mutation, and session-cycle slices — gpt-5.5
Completed #mut3, #clo3, and #cyc2.

I extracted the next FlowCore slices: `flow::document_mutation` now owns template patchback shape parsing/classification and the orchestrate plain-response contract before visible writes; `flow::closeout` now owns the strict terminal transaction for commit, snapshot convergence retry, parent gitlink verification, session-check, and fallback-patch cleanup; `flow::session_cycle` now provides the shared prompt-target and finalize-command contract used by `preflight` and `plan`.

Updated the closeout/core/diff specs and Unreleased notes. Verification passed: `cargo check`, focused FlowCore/write/orchestrate/plan/preflight tests, `make check`, `cargo build`, and `cargo install --path .`. Latest GitHub CI run `26131409609` was green, including `Run make tmux-ci`.

Submodule commit `b6c0fa59` is pushed to `btakita/agent-doc` branch `20260422/codex-agent-backend`; parent gitlink commit `9ea21ba89` is committed and will be pushed with this closeout commit.
<!-- agent:boundary:b8873c8b -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#mut3]
- do [#clo3]
- do [#cyc2]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ops2] [recommended] Move operator_clear and orchestration_batch onto FlowCore typed outcomes after route and closeout extraction; cover active-pane clear refusal, queue freeze/mid-run mutation, and child patchback normalization. Plan: tasks/agent-doc/plan-agent-doc-flowcore-refactor.md
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
