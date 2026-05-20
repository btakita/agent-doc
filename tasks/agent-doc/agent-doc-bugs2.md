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
FlowCore refactor is in maintenance mode; top backlog item: #y9kk.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260520-233127.md`*

Compacted content:
- Archived 1 response topic(s): FlowCore next steps
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#fcmt]. #spec-test-build-install-commit-push
### Re: do [#fcmt] — gpt-5

Completed `#fcmt`.

I reproduced the mixed duplicate-scaffold/live prompt failure with failing regressions first. The normalizers now only repair pure duplicated scaffold. When duplicated scaffold is mixed with live prompt text, the closeout write path logs a typed `flow=document_mutation` event with `reason=mixed_duplicate_scaffold_tail` and fails closed; editor/FFI normalization rejects the same shape instead of moving or duplicating that text.

Updated specs, changelog, and the FlowCore map. Pushed commits:
- agent-doc submodule: `c0c5a188` (`fix: fail closed on mixed duplicate scaffold`)
- parent gitlink: `f674361c6` (`agent-doc: fail closed on mixed duplicate scaffold`)

Verification:
- failing regression first: `cargo test mixed_duplicate_scaffold --lib -- --test-threads=2` failed against the old auto-repair behavior
- focused pass: `cargo test duplicate_scaffold --all-targets -- --test-threads=2`
- full suite: `make check`
- CI evidence: latest CI run `26195552731` was green; its `check` job includes both `make check` and `make tmux-ci`
- local tmux sweep: `make tmux-ci`
- install/build: `make install`
- installed version: `agent-doc --version` -> `agent-doc 0.33.16`
<!-- agent:boundary:22c09dbe -->
<!-- /agent:exchange -->
###
<!--
JB `Interrupt and Clear Session Context` causes the Codex session to open nvim.
#spec-test-build-install-commit-push
---
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push 
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
