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

FlowCore refactor is complete enough to leave broad extraction mode; next top backlog item: #fcrg.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-201705.md`*

Compacted content:
- Archived 1 response topic(s): Clear Session Context active-agent guard
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ What are the #next-steps for the flow core refactoring?
### Re: FlowCore refactoring next steps — gpt-5

The FlowCore refactor is at the point where the broad extraction queue can stay closed. The current code has dedicated `flow` modules for routed reopen, document mutation, closeout, session cycle, operator clear, and orchestration batch, and the specs now identify those flows as the owners for the recurring bug classes.

The next step is a maintenance gate, not another large rewrite: for the next hot-path bugfix, verify the touched branch does not add new free-form guard/proof strings outside FlowCore, promote any new branch into the owning enum/event, and add the smallest regression at the right level: pure flow test first, SimWorld when interleavings matter, tmux only for external boundary proof.

I added that as the new top backlog item: #fcrg. The previous lower-agent orchestration sequence should stay immediately behind it, starting with #y9kk.
❯ do [#fcrg]. #spec-test-build-install-commit-push
### Re: FlowCore regression gate — gpt-5

Implemented `#fcrg`.

I tightened the touched routed-reopen branch so route prompt-ready and dispatch-proof failures now use `RoutedReopenGuardReason` instead of passing free-form reason strings from `route.rs` into FlowCore events. I also added the executable regression gate in `tests/test_cli.rs`: it budgets existing hot-path `guard_`, `proof=`, `proof_scope=`, `reason=`, `flow_reason=`, and `accepted_only` tokens across route/write/preflight/session-check/orchestrate/git/repair, so future additions have to be audited and promoted through the owning FlowCore enum/event before the budget changes.

Spec/docs updated:
- `SPEC.md`
- `docs/reference/flow-map.md`
- `VERSIONS.md`

Commits created:
- `src/agent-doc`: `9a331a1c test: add flowcore regression gate`
- parent repo: `273874a91 agent-doc: update flowcore regression gate`

Verification:
- `cargo test flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
- `cargo test route_failure_events_are_owned_by_routed_reopen_flow`
- `cargo test typed_flow_events_cover_route_write_closeout_session_check_and_child_patchback`
- `make check`
- `make install`
- Latest GitHub CI run on `20260422/codex-agent-backend` was green, including the `Run make tmux-ci` step.
<!-- agent:boundary:04599849 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
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
