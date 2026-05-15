---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-002915.md`*

Compacted content:
- Archived 1 response topic(s): #ajx2 live typing duplicate insertion
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Look through the logs to identify issues with agent-doc. #next-steps
### Re: log review — gpt-5

I found three issues worth turning into fixes.

1. Opencode routing is often only proving pane-input acceptance, not dispatch start. Evidence includes `.agent-doc/logs/ops.log:103073`, `.agent-doc/logs/ops.log:105032`, and `.agent-doc/logs/ops.log:116379`, all with `proof=accepted proof_scope=accepted_only`. Those are followed by stale locks and unresolved prompt keep-alives for the same opencode/corky flow, for example `.agent-doc/logs/ops.log:105035`, `.agent-doc/logs/ops.log:105044`, and `.agent-doc/logs/ops.log:116382`. That matches the user-facing “button did not actually dispatch” class: text reached the pane, but agent-doc never proved the harness consumed it.

2. The opencode capability proof path appears to be invoking opencode incorrectly. `.agent-doc/logs/b26b9957-8de6-4eae-bb98-5d7fe4d6b781.log:4396`, `:4415`, `:4477`, `:4739`, and `:4762` all record `opencode_capability_proof status=failed`, but the captured child output is opencode’s command/help screen. That looks like a CLI usage/argument construction failure being reported as a child network probe failure. There is also a separate pty write failure in `.agent-doc/logs/be20dd6e-31ce-4f16-93ed-42c9c2a21b98.log:134`.

3. The session lifecycle is repeatedly recovering stale prompt cycles and snapshot drift instead of leaving one clean owner state. Recent evidence includes `.agent-doc/logs/ops.log:105686` for “Left/Right buttons still don't work with agent-doc opencode”, `.agent-doc/logs/ops.log:112752`, `.agent-doc/logs/ops.log:114284`, plus drift warnings like `.agent-doc/logs/ops.log:113760`, `:114287`, and `:116385`. Earlier `snapshot_committed_guard_failed` entries for this same document show the same class at `.agent-doc/logs/ops.log:53655`, `:54300`, `:54857`, `:75002`, and `:89749`.

I added recommended backlog items for those three fixes. I did not change code or run the test suite; this cycle was a log review.
<!-- agent:boundary:d65d6852 -->
<!-- /agent:exchange -->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#has9] [recommended] Fix opencode dispatch-only routing so proof=accepted/proof_scope=accepted_only is treated as not dispatched, and require dispatch-start proof before reporting routed delivery as consumed.
- [ ] [#5pr6] [recommended] Fix opencode capability proof command construction and classify CLI usage/help output separately from real child network probe failures.
- [ ] [#s1mv] [recommended] Add deterministic coverage for session lifecycle drift: dispatch, user follow-up edit, finalize or compact, then preflight with no stale prompt cycle and no snapshot/head guard mismatch.
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
