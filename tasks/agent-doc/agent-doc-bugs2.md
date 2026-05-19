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

Added route-focused FlowCore refactor next steps. Top backlog item: #rte2.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260519-155256.md`*

Compacted content:
- Archived 3 response topic(s): FlowCore refactor; refactor next steps; routed reopen FlowCore
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: #873y wedged-cycle recovery — gpt-5

Implemented the immediate wedged-cycle fix from `tasks/agent-doc/plan-wedged-cycle-unrecoverable.md`.

What changed:

- `src/agent-doc/src/write.rs` now only enters strict commit closeout when the write path succeeds. A pre-write guard failure, such as pending-capture rejection, leaves the active cycle open and retryable instead of committing an unanswered cycle.
- `src/agent-doc/tests/finalize_integration.rs` now covers failed strict pre-write closeout followed by a successful retry with backlog capture.
- `src/agent-doc/src/git.rs` now covers historical snapshot repair when a committed assistant response contains an h4 subheading like `#### #next-steps`.
- `src/agent-doc/specs/07-closeout-commands.md` now documents the retryable pre-write failure boundary.
- `tasks/agent-doc/plan-wedged-cycle-unrecoverable.md` now records the implementation result and leaves broader FlowCore / cycle-state extraction under existing `#mut2`.

Verification:

- `cargo fmt`
- `cargo test finalize_prewrite_guard_failure_leaves_cycle_open_for_retry`
- `cargo test commit_repairs_committed_historical_snapshot_drift`
- `cargo test finalize_keeps_queue_head_when_later_strict_pending_gate_fails`
- `make check` after the final spec update
- `make install`, followed by `agent-doc --version` -> `agent-doc 0.33.16`
- GitHub CI run `26109770668` on pushed submodule SHA `c11a120a4`: `make check` and `make tmux-ci` passed in job `check`

Commits pushed:

- `btakita/agent-doc` `20260422/codex-agent-backend`: `c11a120a4` (`fix: keep failed prewrite closeouts retryable`)
- `btakita/agent-loop` `main`: `5bd4af744` (`agent-doc: keep failed prewrite closeouts retryable`)

No new follow-up item is needed for this cycle; the remaining larger architecture work is already tracked by `#mut2` and remains open.
<!-- agent:boundary:08449d84 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue au -->
preset #spec-test-build-install-commit-push
- do [#873y]
- do [#mut2]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#mut2] Continue FlowCore extraction outside routing by promoting document_mutation and closeout guard strings into typed events, using tasks/agent-doc/plan-wedged-cycle-unrecoverable.md as the acceptance target for recoverable finalize/write/repair boundaries.
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
