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

Completed: #gdbrwbatch, #gdbprepfast, #gdbcandpush, #gdbpathbudget, and #gdbperfgate shipped in tsift aaac32a and superproject e25a1c50b; graph refresh now delta-upserts materialized property rows, conflict-matrix reuses a symbol-by-file graph index, experimental backend adapters use indexed outgoing/kind traversal, backend-eval reports adaptive path budgets plus performance gates, and promotion stays held until every measured operation beats SQLite with parity.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-221617.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ - Trailing prompt/context: What are #next-steps to improve graph performance?
### Re: graph performance next steps — gpt-5

Top backlog item: `#gdbpostbase`.

I added the next graph-performance backlog in priority order and left the queued `do` items open for a later implementation cycle. The previous top five are already shipped, and the best current signal is that SQLite improved substantially but still spends most of the real run in refresh and shared preparation work. The next pass starts by replacing the stale baseline with fresh post-aaac32a measurements, then optimizes the remaining refresh construction path, then removes repeated conflict-matrix/dispatch-trace preparation work.

Backend switching stays last. FalkorDB/DuckDB/Ladybug remain behind the promotion gate until SQLite's current baseline is clean and every candidate beats SQLite on parity, real operator reads, synthetic graphs, portability, and lock behavior.

Changed paths: `tasks/software/tsift.md` only.

Commands run: `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `git status --short`, `tsift status --json`, targeted reads/searches over `src/tsift/SPEC.md`, `tasks/software/tsift.done.md`, and the compacted exchange archive, plus repair closeout after an initial malformed patch block was rejected before the response was written.

Verification: no code changed, so I did not run the tsift test/build suite. The tsift workspace index is stale, so I treated `tsift status` as diagnostic and relied on checked-in spec/archive content for the recommendation. No escalated commands were used.

<!-- agent:boundary:d28c0403 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbpostbase]
- do [#gdbrowstream]
- do [#gdbprepstage]
- do [#gdbvacproof]
- do [#gdbenginegate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbpostbase] [recommended] Capture a fresh post-aaac32a graph performance baseline from real backend-eval plus metric-digest, recording current refresh, source_graph_build, projection_rows, sqlite_delta_write, conflict-matrix preparation, status, dispatch-trace, path_max_hops, graph.db size/freelist, and candidate-backend timings so the next tuning pass starts from measured current bottlenecks.
- [ ] [#gdbrowstream] [recommended] Reduce remaining real graph-db refresh cost by streaming projection rows/source handles/worker-context rows into batched temp-table writes, avoiding repeated session markdown scans and large in-memory row churn, then prove improvement against the fresh #gdbpostbase history.
- [ ] [#gdbprepstage] [recommended] Make conflict-matrix and dispatch-trace share a prepared graph-evidence stage across backend-eval operations: reuse target resolution, worker_result/source_handle expansion, semantic rows, and candidate ownership inputs instead of rebuilding them per operation.
- [ ] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
- [ ] [#gdbenginegate] [recommended] Re-run FalkorDB, DuckDB/DuckPGQ, and Ladybug candidate backend evaluation only after SQLite's refreshed baseline/read paths are stable; require parity plus faster real refresh-equivalent load, evidence, conflict-matrix, dispatch-trace, synthetic high-degree/deep-chain runs, portable install, and no worse lock behavior before promotion.
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
