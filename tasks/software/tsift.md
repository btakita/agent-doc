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
Completed: #gdbret, #gdbbfs, #gdbpush, and #gdbdelta shipped in tsift bd402c5 and superproject ea61810cb; SQLite graph refreshes are delta-based, graph reads push batching/filtering into SQLite, tombstone retention diagnostics are surfaced, and both repos are pushed.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260522-014020.md`*

Compacted content:
- Archived 1 response topic(s): SQLite graph DB optimization batch
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are the #next-steps to improve sqlite graph performance?
### Re: SQLite graph performance next steps — gpt-5

Top backlog item: #gdbperfbase.

I added the next performance work in this priority order:

1. #gdbperfbase: refresh and migrate the real agent-loop `.tsift/graph.db`, then capture the baseline numbers before more tuning. This is first because current `graph-db status` still reports a stale schema-v1 graph missing the new `row_hash` and `source_watermark` columns, so performance data against that file is not clean.
2. #gdbbenchhist: make graph performance measurement repeatable and bounded across the real projection plus synthetic high-degree/deep-chain graphs, with metric history suitable for regression review.
3. #gdbpathbatch: batch evidence shortest-path and worker-context path expansion so evidence/conflict workflows do not regress into per-pair path queries.
4. #gdbpropidx: only promote JSON-property filters into materialized indexed rows or columns after query-plan and benchmark evidence shows JSON extraction scans are the dominant cost.
5. #gdbcompact: add a safe compaction policy for tombstone-heavy graph stores after Convex reconciliation, since current status reports 1,025,144 tombstones versus 929,474 live rows and a 605 MB graph DB.

Evidence gathered this cycle: `tsift graph-db --path /home/brian/work/btakita/agent-loop --json status` completed and showed the stale schema/tombstone numbers above. I also tried `backend-eval --target #orchdog`; it produced no output after roughly 90 seconds, so I stopped it and treated bounded/progress-visible benchmarking as part of the follow-up work. No code changed, so I did not run the project test suite.
### Re: SQLite graph performance batch — gpt-5

Completed the full batch: `#gdbperfbase`, `#gdbbenchhist`, `#gdbpathbatch`, `#gdbpropidx`, and `#gdbcompact`.

Shipped changes in `src/tsift`:
- SQLite graph schema v3 now materializes `graph_node_properties` with `idx_graph_node_properties_key_value_node`, and property-filtered `kind`/`neighborhood` reads use that indexed table instead of JSON extraction scans.
- Full projection refresh is now viable on the real root graph: it uses indexed graph-builder lookups, temp edge join indexes, indexed tombstone pruning, and keeps normal read queries read-only instead of implicitly refreshing.
- Evidence collection now batches reachable node collection by target kind through one SQLite recursive CTE per family group.
- `graph-db backend-eval` now reports real, synthetic high-degree, and synthetic deep-chain datasets, emits numeric metrics, and points at `fixtures/graph-db-performance-history.json` for `metric-digest` review.
- `graph-db compact` adds dry-run/apply policy reporting, WAL checkpoint/VACUUM, and guarded tombstone pruning that requires `--confirmed-convex-reconciled`; status and doctor expose compaction proof without failing closed.

Real agent-loop graph evidence after refresh/backend-eval: current schema/projection, 351,697 nodes, 586,301 edges, 151,622 retained tombstones, 1,136,963,584 byte `graph.db`, and 119,422,976 freelist bytes. The first successful migration refresh pruned 894,637 stale-live tombstones. `doctor` is ok; compaction remains recommended and tombstone pruning still requires Convex reconciliation. A live `kind backlog --property ref_id=gdbperfbase --limit 1` query now returns immediately and reports both `idx_graph_nodes_kind` and `idx_graph_node_properties_key_value_node` in the SQLite plan.

Benchmark baseline from `backend-eval --target gdbperfbase`: real SQLite refresh 125,231,934 us, status 6,202,498 us, evidence 56,775 us, conflict-matrix 3,339,006 us, dispatch-trace 10,934,534 us, total 145,764,747 us. Synthetic high-degree SQLite total was 45,087 us; synthetic deep-chain total was 11,444 us. DuckDB/DuckPGQ and Ladybug remain on hold because real dispatch-trace parity differs from SQLite.

Verification completed: `cargo test graph_db`, `make check`, `cargo install --path .`, `tsift --version`, `target/debug/tsift metric-digest --input fixtures/graph-db-performance-history.json --json`, real `graph-db refresh/status/doctor/evidence/compact/backend-eval`, `conflict-matrix`, and `dispatch-trace`. Latest GitHub Actions `CI` run inspected: `26263293674`, success. Pushed tsift commit `9da2685` and superproject submodule-pointer commit `8fb434e82`.
<!-- agent:boundary:205fcd32 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbperfbase]
- do [#gdbbenchhist]
- do [#gdbpathbatch]
- do [#gdbpropidx]
- do [#gdbcompact]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
