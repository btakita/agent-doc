---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Completed: #cmgevfix shipped in agent-doc e17bf1f and superproject gitlink 7680e03cf; graph evidence is refreshed before packet validation and graph failures fall back to manual review.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-144258.md`*

Compacted content:
- Archived 1 response topic(s): #cmgrwres
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Any backlog work that should be done on the agent-doc code
❯ do [#tsdag]. #spec-test-build-install-commit-push
### Re: #tsdag blocked — gpt-5

I could not start `#tsdag` because `agent-doc plan` stopped the cycle before implementation with a fail-closed tsift graph evidence error:

`tsift graph orchestration contract invalid: conflict-matrix missing evidence_packet_id gevd-8fb9f57781 for target tsdag`

No repo edits, tests, build, install, manual commit, or push were performed. I am not marking `#tsdag` done or gated because implementation did not begin; it stays open until the graph evidence / conflict-matrix state for this document is repaired or regenerated, then this same `do [#tsdag]. #spec-test-build-install-commit-push` request can be rerun.
### Re: next steps for missing evidence packet — gpt-5

Added `#cmgevfix` as the new top backlog item to resolve the missing `evidence_packet_id` failure that blocked `#tsdag`.

Top of backlog: `#cmgevfix`
❯ do [#cmgevfix]. #spec-test-build-install-commit-push
### Re: #cmgevfix blocked — gpt-5

I could not start `#cmgevfix` because `agent-doc plan` stopped the cycle before implementation with another fail-closed tsift graph evidence error:

`tsift graph orchestration contract invalid: conflict-matrix missing evidence_packet_id gevd-d102ace0f7 for target cmgevfix`

No repo edits, tests, build, install, manual commit, or push were performed. `#cmgevfix` stays open because the planner cannot produce a valid execution contract while the conflict-matrix/evidence graph is inconsistent for the target item itself.
<!-- agent:boundary:d5e3595c -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#tsdag]
- do [#impactdag]
- do [#dagfix]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsdag] [recommended] Add graph-level dependency DAG extraction for agent-doc work: derive explicit and implicit edges among backlog ids from file/symbol/test/config overlap, semantic relations, worker_result follow-up ids, and depends-on text; emit nodes, edges, cycle diagnostics, topo batches, replay commands, and repair commands as a versioned JSON contract.
- [ ] [#impactdag] [recommended] Extend conflict-matrix and dispatch-trace with scheduler fields: parallel_safe, blocks, blocked_by, required_context, expected_tests, and stable graph handles so agent-doc can run topological antichains without scraping prose.
- [ ] [#dagfix] [recommended] Add regeneration fixtures for real agent-doc queue DAGs: refresh -> evidence -> conflict-matrix -> dependency-dag -> dispatch-trace, asserting projection freshness, packet ids, worker_result closure, cycle detection, and stale graph fail-closed behavior.
- [ ] [#dbbench] [recommended] Add an experimental GraphStore backend evaluation for DuckDB/DuckPGQ and Ladybug: prototype read-only adapters behind the existing contract, benchmark refresh/status/evidence/conflict-matrix/dispatch-trace on real and synthetic projections, and only promote if it beats SQLite without worsening multi-process lock behavior or install portability.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
