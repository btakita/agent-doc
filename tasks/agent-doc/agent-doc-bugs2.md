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

FlowCore refactor is in maintenance mode; top backlog item: #ipcd.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-022651.md`*

Compacted content:
- Archived 8 response topic(s): JB Interrupt and Clear Session Context; tsift #pwcm backlog accounting; backlog update and duplicate prompt corruption; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#agbr]. #spec-test-build-install-commit-push
### Re: #agbr tsift graph evidence — gpt-5

Implemented `#agbr`. `agent-doc plan` now adds optional `graph_evidence` for queued `do #id` targets when a materialized `.tsift/graph.db` exists, and fails closed on stale graph state or missing evidence targets. `agent-doc orchestrate` now attaches bounded `<tsift_graph_evidence>` JSON packets to fresh child prompts/job packets, and parallel fan-out blocks when the tsift conflict matrix fail-closes.

The change adds `src/tsift_graph.rs`, wires it through `plan.rs` and `orchestrate.rs`, adds regression coverage for target extraction, evidence handles, stale graph failure, and sequential prompt attachment, and documents the behavior in README/SPEC/orchestration docs. `AGENTS.md` was also refreshed by the required `tsift status --fix` session-start step.

Verification passed: `cargo test tsift_graph -- --test-threads=1`, `cargo test sequential_orchestration_attaches_tsift_graph_context_to_agent_prompt -- --test-threads=1`, `cargo test build_plan_includes_finalize_placeholder_for_template_docs -- --test-threads=1`, `cargo check`, `make check`, `make install`, and `agent-doc --version` returned `0.33.16`. CI review: latest GitHub CI run `26201015869` on `20260422/codex-agent-backend` succeeded, including the `Run make tmux-ci` step.

Committed and pushed `src/agent-doc` commit `67838bd8 feat: add tsift graph evidence to orchestration`, then committed and pushed the root gitlink update `db879348b agent-doc: update tsift graph evidence integration`. The nested repo is clean; only unrelated pre-existing root worktree changes remain outside this task.
<!-- agent:boundary:db3e8d5f -->
<!-- /agent:exchange -->
###


<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#r7k8] [recommended] Incorporate tsift #pwcm planner conflict-matrix report into lower-agent orchestration before enabling parallel worker dispatch: use graph/context/diff/impact outputs to rank worker scopes, flag shared file/symbol/test/config conflicts, and emit fail-closed ownership blocks. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
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
