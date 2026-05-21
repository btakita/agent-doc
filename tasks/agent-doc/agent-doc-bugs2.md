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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-025059.md`*

Compacted content:
- Archived 1 response topic(s): #agbr tsift graph evidence
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#r7k8]. #spec-test-build-install-commit-push
### Re: do #r7k8 — gpt-5

Implemented `#r7k8`.

The agent-doc side now preserves the full `conflict-matrix-v1` planner contract from tsift: context-pack summary, cached diff and impact commands, ranked candidates, shared file/symbol/test/config conflicts, worker prompt packet ownership prompts, token budgets, semantic ranking reasons, replay/repair commands, and warnings. Parallel orchestration now blocks unless tsift explicitly reports `can_parallel=true` and `fail_closed=false`, so high-risk shared-symbol/test cases cannot slip through just because they are not file-level fail-closed conflicts.

Changed paths:
- `src/agent-doc/src/tsift_graph.rs`
- `src/agent-doc/src/orchestrate.rs`
- `src/agent-doc/specs/07-orchestration-commands.md`
- `src/agent-doc/VERSIONS.md`

Verification passed:
- `cargo test tsift_graph`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `agent-doc --version` -> `agent-doc 0.33.16`
- Latest GitHub CI run `26202438433` succeeded, including the `Run make tmux-ci` step.

Commits created locally:
- `src/agent-doc` submodule: `a919134e` (`feat: carry tsift conflict matrix contract`)
- superproject: `318d0ad` (`chore: update agent-doc conflict matrix integration`)

Push follows this document closeout so the session-document commit is included by the normal agent-doc path rather than the manual repo commit.
<!-- agent:boundary:fc836e48 -->
<!-- /agent:exchange -->
###


<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#gxct] [recommended] Validate tsift graph orchestration contracts in agent-doc plan/orchestrate: require contract_version, packet_id, projection_hash, freshness=current, replay_commands, and repair_commands before dispatching lower-agent work.
- [ ] [#wpkm] [recommended] Map tsift worker_prompt_packets into lower-agent job packets: preserve owned_files, read_only_context, forbidden_files, expected_tests, expansion_commands, token budgets, and fail-closed prompt text.
- [ ] [#wrcx] [recommended] Collect lower-agent closeouts into worker_result lines that tsift can project back into graph evidence, including status, touched files, tests, and follow-up ids.
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
