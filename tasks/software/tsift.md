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
Completed: #gdbrefreshcache, #gdbtargetsnap, #gdbpathprobe, and #gdbpropscan shipped in tsift e8823da and superproject abb56b3af; latest GitHub CI for tsift was green before this push, and no tmux workflow is configured for the tsift repo.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-162142.md`*

Compacted content:
- Archived 1 response topic(s): backend eval gates
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#gdbpropscan]. #spec-test-build-install-commit-push
### Re: gdbpropscan — gpt-5

Completed #gdbpropscan.

I isolated the slow edge-property path to two related SQLite read shapes. First, `paged_edges` applied edge property filters with an `EXISTS` subquery after starting from `graph_edges`; on the real graph DB this meant 588,211 edge rows were considered before the selective property filter that matched only 30 rows. Second, backend-eval's `edge_property_scan` operation still called `all_edges()` just to choose a property-bearing probe edge, so the benchmark included a full-edge preload before the actual paged scan.

I changed SQLite property-filtered edge scans to drive from `graph_edge_properties(key, value, edge_key)` into `graph_edges(edge_key)`, added cardinality diagnostics to the page report, and added a `GraphStore::sample_edge_with_property` probe so backend-eval selects its edge-property test case through the same indexed surface instead of preloading all edges. The spec and conformance tests now lock this behavior.

Proof captured:
- Before: current release backend-eval on `tasks/software/tsift.md --full-projection` measured `real.sqlite.edge_property_scan.duration_micros = 739,883`; the checked-in history fixture had the roughly 3s case at `3,068,727` us.
- Before EXPLAIN: `SCAN e USING INDEX idx_graph_edges_edge_key` plus correlated `SEARCH ep0 ... idx_graph_edge_properties_key_value_edge`.
- After page diagnostics: `SEARCH ep0 USING COVERING INDEX idx_graph_edge_properties_key_value_edge (key=? AND value=?) | SEARCH e USING INDEX idx_graph_edges_edge_key (edge_key=?)`, with `edge property primary filter matched 30 materialized row(s)`.
- After installed-release backend-eval: `real.sqlite.edge_property_scan.duration_micros = 10,394`; full projection `= 12,978`.
- Metric digest versus `fixtures/graph-db-performance-history.json`: real normalized edge-property scan improved by 99.66%; full-projection normalized scan improved by 99.59%.

Verification completed:
- `cargo test edge_properties`
- `cargo test graph_db_scale_caps_pagination_paths_doctor_and_sqlite_plans`
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- final `make check` after the last spec edit
- `gh run list --workflow CI --limit 1` and `gh run view 26363974745 --json status,conclusion,jobs`: latest tsift CI run was successful; jobs were `clippy + test` and `live Convex graph acceptance`. `gh workflow list` shows no tmux-test workflow for this repo.

Local commits are prepared as tsift `e8823da` and superproject `abb56b3af`. Per the session staging rule, push runs after this document closeout.
<!-- agent:boundary:ad0e8b18 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve graph performance?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbpropscan]
- do [#gdbcoldfull]
- do [#gdbprepwm]
- do [#gdbhopguard]
- do [#gdbbackendgate]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
<!-- /agent:review -->
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
