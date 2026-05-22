---
agent_doc_session: tsift-v0.1
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
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
Completed: #gdbrefreshhot, #gdbmetricreal, #gdbtracehot, #gdbhopcaps, #gdbcompactrun, and #gdbfalkorgate shipped in tsift 638fe4c and superproject 9a5447b88; graph backend evaluation now records real metrics, phase timings, path cap probes, and gated FalkorDB evidence.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-214606.md`*

Compacted content:
- Archived 1 response topic(s): graph backend eval gates
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

Top backlog item: `#gdbrwbatch`.

I added the next graph-performance backlog in priority order. The order follows the current fixture: real SQLite is still the best production baseline at about 24.7s total; its refresh is the largest remaining cost at about 19.6s, with SQLite delta writes at about 12.9s and conflict-matrix preparation at about 5.2s. FalkorDB is not a promotion candidate yet: its refresh-equivalent load is faster, but real conflict-matrix is about 40.4s and total runtime is about 49.5s, roughly 2x SQLite.

The next pass should attack write batching first, then repeated operator preparation, then candidate backend query pushdown. Hop-budget tuning and performance regression gates should follow once those hot paths stop dominating.
<!-- agent:boundary:cb4ce11d -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshhot]
- do [#gdbmetricreal]
- do [#gdbtracehot]
- do [#gdbhopcaps]
- do [#gdbcompactrun]
- do [#gdbfalkorgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbrwbatch] [recommended] Reduce SQLite graph refresh write cost on the real agent-loop projection: profile `real.refresh_phase.sqlite_delta_write.duration_micros` (currently 12,941,973 us of the 19,598,243 us SQLite refresh), batch node/edge/property upserts through temp-table merge or prepared-statement chunks, avoid unchanged property rewrites, and prove the improvement with `graph-db backend-eval` plus `metric-digest` against `fixtures/graph-db-performance-history.json`.
- [ ] [#gdbprepfast] [recommended] Pull conflict-matrix preparation out of repeated graph performance hot paths: cache or reuse evidence target resolution, worker-result/source-handle expansions, and dispatch-trace inputs across backend-eval operations so `real.refresh_phase.conflict_matrix_preparation` (5,158,398 us) and post-refresh operator reads do not redo the same traversal work.
- [ ] [#gdbcandpush] [recommended] Fix candidate backend query pushdown before considering FalkorDB, DuckDB/DuckPGQ, or Ladybug promotion: real candidate refresh-equivalent loads are fast at roughly 2.5s, but conflict-matrix is roughly 40s and total runtime is roughly 49s; implement candidate-side batch traversal/query plans and keep the promotion gate failing closed until every real and synthetic operation beats SQLite with parity.
- [ ] [#gdbpathbudget] [recommended] Decide adaptive max-hop defaults from measured evidence instead of raising caps globally: use the current real `path_max_hops` timing, deep-chain and high-degree probes, truncation warnings, and query-plan assertions to define operation-specific hop budgets.
- [ ] [#gdbperfgate] [recommended] Turn graph backend performance history into a bounded regression gate: keep a short CI-safe synthetic run plus an opt-in real projection benchmark, define allowed deltas for refresh, conflict-matrix, dispatch-trace, path_max_hops, graph.db size/freelist, and require `metric-digest` evidence in graph-performance PRs.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
