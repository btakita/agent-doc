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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260515-111142.md`*

Compacted content:
- Archived 1 response topic(s): #rrfe
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
Look in the Claude/Codex session + agent-doc logs to find issues. #next-steps
### Re: log review — gpt-5.5

I reviewed the recent Codex, Claude, and agent-doc logs around this document and found four concrete issues worth tracking.

1. Claude hit a pending-only write path that committed a backlog mutation, then returned `Exit code 1` with `Error: empty response — nothing to write`. That means `agent-doc write --commit --pending-add ...` can perform the requested mutation and still report failure, which is a bad closeout contract.
2. The route layer logged four consecutive `route_authoritative_actor_starting_not_ready` failures for the same Codex actor generation before the actor became `ready`. That creates a repeated miss window during clean restarts instead of queueing or retrying exactly once after prompt readiness.
3. On May 14, two cycles show a stale-preflight repair while the same cycle continued receiving `partial_response_checkpoint` writes. That points to a race where repair can classify a cycle stale while the harness is still producing durable response checkpoints.
4. `sidecar_normalization_fallback_repaired_working_tree` fired for this document, which means a fallback silently repaired the working tree. That may be correct recovery, but it deserves a deterministic regression so the primary IPC/snapshot path and fallback boundary are explicit.

I added those as new backlog items: `#writeempty`, `#startroute`, `#staleckpt`, and `#normfallback`.
<!-- agent:boundary:b7037157 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#writeempty] Fix pending-only write/commit ordering: validate empty stdin before committing, or treat valid --pending-add-only writes as successful. Add a regression for Claude's agent-doc write --commit --pending-add empty-response path not exiting 1 after committing.
- [ ] [#startroute] Fix repeated route failures while the authoritative actor is starting: queue or retry the dispatch once the actor reaches prompt-ready, or surface one durable recoverable route error instead of repeated route_authoritative_actor_starting_not_ready failures.
- [ ] [#staleckpt] Audit the stale-preflight repair versus partial checkpoint race. May 14 cycles cycle-1778789175029 and cycle-1778790227635 show repair_preflight_stale_lock while partial_response_checkpoint writes keep arriving for the same cycle; prevent stale repair while a harness is still streaming checkpoints or bind repair to the newer cycle.
- [ ] [#normfallback] Investigate sidecar_normalization_fallback_repaired_working_tree for agent-doc-bugs2.md and add deterministic coverage for why sidecar normalization had to repair the working tree instead of taking the primary IPC/snapshot path.
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
