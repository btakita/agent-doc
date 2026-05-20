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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-211949.md`*

Compacted content:
- Archived 1 response topic(s): full-content IPC guard and FlowCore next step
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#bqn4]. #spec-test-build-install-commit-push
### Re: FlowCore full-content source-buffer proof — gpt-5

Done. I moved the full-content source-buffer proof into `flow::document_mutation` and kept `write.rs` as a caller that serializes that shared contract into IPC payloads. The proof now has typed source hash/byte-length construction, stale-buffer replacement decisions, and a FlowCore event for rejected full-content visible writes.

Spec/tests updated:

- `src/agent-doc/SPEC.md`, `src/agent-doc/editors/SPEC.md`, `src/agent-doc/editors/PLUGIN-SPEC.md`, and `src/agent-doc/VERSIONS.md` now describe the FlowCore-owned full-content visible-replacement contract.
- Added pure FlowCore tests for proof matching/rejection.
- Added a deterministic SimWorld regression covering stale editor buffers for Compact Exchange, full-content repair, and IPC-timeout recovery.

Verification/build/install:

- `cargo test full_content_source_proof`
- `cargo test full_content_ipc_payload_carries_source_buffer_identity`
- `make check`
- `make install`
- GitHub CI run `26191058253` passed on pushed agent-doc commit `c6ff6e18`, including `Run make check` and `Run make tmux-ci`.

Commit/push evidence:

- `src/agent-doc`: `c6ff6e18 fix: move full-content proof into FlowCore`, pushed to `origin/20260422/codex-agent-backend`.
- Parent gitlink: `256953fd2 chore: update agent-doc submodule`, pushed to `origin/main`.

I staged only the intended `src/agent-doc` implementation files inside the submodule and the parent `src/agent-doc` gitlink. The parent worktree still has unrelated local dirty paths and later unrelated local `src/session-share` gitlink commits that I did not push.
<!-- agent:boundary:20df6296 -->
<!-- /agent:exchange -->
###
<!--
As I was writining into the comment below agent:exchange...The duplicate content document corruption bug is still happening. I also saw the duplicate prompt issue occur in corky.md. Do we need more refactoring? Why is this still happening? Do we have adequate log coverage?
#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#qcont] [recommended] Fix auto agent:queue continuation so a direct agent-doc run either drains remaining queue prompts after each successful finalize or explicitly reports single-step resume semantics instead of silently leaving later items unrun. Plan: tasks/agent-doc/plan-auto-queue-continuation-after-finalize.md
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
