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
Completed: #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe shipped in tsift c1c7668 and superproject f321eabef; GitHub Agent Doc CI is externally blocked before runner startup by the billing/spending-limit annotation.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-140200.md`*

Compacted content:
- Archived 2 response topic(s): graph DB refresh cache, scoped snapshots, and path probes; benchmark and IPC payload guard
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve sqlite graph performance?
### Re: SQLite graph performance next steps — gpt-5

Top backlog item: `#gdbsqlitehotpath`.

I added `#gdbsqlitehotpath` at the top of the backlog. The next performance work should stay on SQLite first, not FalkorDB/backend promotion yet. Current `backend-eval` says SQLite is still the correctness and portability baseline, and on the real agent-loop graph it is already faster for the read paths that matter most: `path_max_hops` was about 11.7 ms for SQLite while the current read-only candidate adapters were roughly 0.94-1.11 s, and `conflict_matrix` was about 522 ms for SQLite versus roughly 1.28-1.41 s for the candidates.

The remaining SQLite work is the write/refresh and traversal hot path: the same run spent about 25.7 s in SQLite refresh overall, with the largest SQLite-owned phases at `sqlite_delta_write` ~3.8 s, `sqlite_edge_staging` ~2.5 s, `sqlite_node_staging` ~0.96 s, and `sqlite_property_row_staging` ~0.57 s. Synthetic cases still show smaller but clear SQLite gaps for bounded path/conflict traversal, so the item asks for a three-sample baseline, then targeted refresh delta/staging and bounded traversal optimization backed by `metric-digest`.

I would keep `#gdbvacproof` as review/storage proof rather than the top perf item. `graph-db status` reports the projection current with ~946k live rows, ~413k retained tombstone rows, a ~1.09 GB file, no freelist bytes, and compaction `not_needed`; useful evidence, but not the main latency lever today. Preflight also warned that `#orchdog` is still a legacy gated item in `agent:backlog`, so `agent-doc migrate /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` remains housekeeping.

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --from-exchange --plan`, `tsift status --json`, `tsift context-pack`, `tsift graph-db --json --path /home/brian/work/btakita/agent-loop status`, `tsift graph-db --json --path /home/brian/work/btakita/agent-loop evidence next-steps --depth 3 --limit 8`, and `tsift graph-db --json --path /home/brian/work/btakita/agent-loop backend-eval`. Changed paths: session document only. Verification: graph status/evidence/backend-eval all returned successfully; no full source test/build run because this cycle only updated the session response/backlog. Escalation: none.
❯ do [#gdbsqlitehotpath]. #spec-test-build-install-commit-push
### Re: SQLite graph hot path — gpt-5

Completed `#gdbsqlitehotpath`.

Implemented the SQLite graph hot-path pass in `src/tsift`:

- Added `GraphStore::edges_between_nodes` with a batched SQLite implementation, then routed conflict-matrix scoped snapshots through it instead of issuing one outgoing-edge query per scoped node.
- Prefiltered unchanged node, edge, and materialized-property rows before SQLite refresh `ON CONFLICT` work, while retaining the older full-write path when user triggers exist so rollback/failure-injection tests still exercise the fail-closed path.
- Reused prepared statements for single-node SQLite bounded-path frontiers to remove repeated prepare overhead from deep-chain path probes.
- Updated `SPEC.md` and added focused coverage for batched scoped edge collection.

Changed paths: `src/tsift/SPEC.md`, `src/tsift/src/main.rs`, `src/tsift/src/substrate.rs`, and the superproject `src/tsift` submodule pointer.

Metric proof, post-install forced-refresh digest against `fixtures/graph-db-performance-history.json`: `reg:0`, `imp:6`. Current focused values were `real.refresh_phase.sqlite_delta_write.duration_micros=3237200`, `real.refresh_phase.sqlite_property_row_staging.duration_micros=569706`, `real.sqlite.conflict_matrix.duration_micros=34979`, `real.sqlite.path_max_hops.duration_micros=11236`, `synthetic_deep_chain.sqlite.path_max_hops.duration_micros=120`, and `synthetic_deep_chain.sqlite.path_max_hops.duration_micros_per_1k_graph_rows=312.5`. The local pre-change sample was about `3856606`, `571184`, `37504`, `11915`, and `219` respectively, so the targeted local metrics improved or stayed flat within noise; the fixture comparison reported no focused regressions.

Verification run:

- `cargo fmt`
- `cargo check`
- `cargo test sqlite_store_batches_edges_between_node_sets`
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo test graph_db_cli_rolls_back_failed_sqlite_refresh --test graph_db_conformance`
- `cargo test sqlite_shortest_path_uses_bounded_frontier`
- `make check` passed: clippy plus `657` unit tests, `108` exit-code tests, and `31` graph-db conformance tests.
- `cargo build`
- `cargo install --path .`, replacing `/home/brian/.cargo/bin/tsift`
- three forced-refresh `tsift graph-db --json --path /home/brian/work/btakita/agent-loop backend-eval --target gdbsqlitehotpath` samples piped through `tsift metric-digest`

CI/tmux evidence: `btakita/tsift` CI run `26335419296` passed after push (`clippy + test` and `live Convex graph acceptance`). The superproject Agent Doc workflow run `26335423912` failed before runner steps/logs were created; both `check` and `tmux-ci` jobs had empty step lists, so I classified it as an external CI-start blocker rather than a code or tmux regression.

Commit/push: `src/tsift` commit `1e53fb8` (`perf: optimize sqlite graph hot paths`) was pushed to `origin/main`; superproject commit `261e72084` (`tsift: optimize sqlite graph hot paths`) was pushed to `origin/main`.

Escalation: none. Confidence: high.
❯ What are #next-steps to make edges and other graph entities first-class entities in the sqlite graphdb implementation?
❯ Does a sqlite-backed graphdb have the potential to match the performance of duckdb + kuzu?
### Re: First-class SQLite graph entities — gpt-5

Top backlog item: `#gdbedgeentity`.

I added two follow-ups, in priority order: `#gdbedgeentity` first, then `#gdbsqlitepgq`.

Edges are already durable rows in SQLite, but they are not first-class enough yet. Today `graph_edges` has `(from_id, to_id, kind)` as the primary key, JSON properties/provenance/freshness, tombstones, and from/to indexes. That is solid storage, but the rest of the API is still node-centered: `GraphStore` has node lookup/kind scans, outgoing edges, scoped edge scans, neighborhoods, and paths, while there is no stable edge id lookup, edge kind scan, incident-edge query, materialized edge-property table, or edge-property filter parity with `graph_node_properties`.

The best next step is not “replace SQLite”; it is to make SQLite’s relational graph model more explicit:

- Add a stable edge id/row key and preserve `(from_id, to_id, kind)` as a uniqueness constraint.
- Add `graph_edge_properties(edge_id, key, value)` with a covering `(key, value, edge_id)` index, matching the existing node-property pattern.
- Extend `GraphStore` with edge-first reads: `edge(id)`, `edges_by_kind`, `incident_edges`, edge property filters, and paginated edge scans.
- Teach `graph-db schema/query/evidence/doctor/drift/compact` to expose and validate those edge identities instead of treating edges as anonymous adjacency rows.
- Keep conformance tied to SQLite first, then use backend-eval to see whether native DuckDB/DuckPGQ or Kuzu still has a promotion case.

On the performance question: yes, for tsift’s workload SQLite has a realistic chance to match or beat DuckDB/Kuzu, and for the current prototypes it already does on the important graph operations. A current local backend-eval against DuckDB/DuckPGQ and Kuzu reported:

- `path_max_hops`: SQLite `11014us`, DuckDB/DuckPGQ `990345us`, Kuzu `937850us`.
- `conflict_matrix`: SQLite `520346us`, DuckDB/DuckPGQ `1419599us`, Kuzu `1395479us`.
- `dispatch_trace`: SQLite `2483us`, DuckDB/DuckPGQ `1226us`, Kuzu `1005us`.
- `evidence_target_resolution`: SQLite `22225us`, DuckDB/DuckPGQ `856us`, Kuzu `729us`.

So the honest read is mixed but favorable to SQLite for the workload that currently costs real time: bounded traversals and conflict-matrix preparation. DuckDB/Kuzu are faster in a couple of small lookup/report operations, but those are not the dominant hot path right now, and the current adapters are still provider-neutral read-only prototypes rather than production-native storage engines.

SQLite is likely enough if tsift keeps the graph workload bounded, indexed, local, transactional, and projection-cache-friendly. Kuzu or DuckDB/DuckPGQ become more compelling if the product direction shifts toward large analytical graph scans, unbounded/variable-length path workloads, parallel OLAP-style joins, or native graph-query syntax as a user-facing feature. Until a production adapter proves parity, concurrency behavior, install portability, and wins across every backend-eval dataset, SQLite should remain the correctness and local-performance baseline.

Commands/proof: ran `agent-doc preflight`, `agent-doc read`, `agent-doc orchestrate --from-exchange --plan`, `agent-doc plan`, `tsift status --json`, `tsift context-pack`, source/schema inspections with `rg`/`sed`, `tsift graph-db --json --path /home/brian/work/btakita/agent-loop status`, and `tsift graph-db --json --path /home/brian/work/btakita/agent-loop backend-eval --candidate duckdb-duckpgq --candidate kuzu` with focused `jq` extraction. Changed paths: session document/backlog only. Verification: all read/eval commands completed successfully; no source build/test run was needed because this cycle only answered the prompt and added backlog items. Escalation: none. Confidence: high.
### Re: do #gdbrefreshcache — gpt-5

`#gdbrefreshcache` is already complete and archived in `tasks/software/tsift.done.md`. The backing implementation is present in `src/tsift` commit `c1c7668` (`perf: cache scoped graph orchestration`) and superproject commit `f321eabef` (`tsift: cache scoped graph orchestration`).

No source work was rerun for this queue head because the item has already shipped; this closeout is consuming the reactivated queue entry.

Commands/proof: ran `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg` against the session and done archive, `git status --short`, `git log --oneline`, and `git show --stat c1c7668` in `src/tsift`. Changed paths: session document only. Verification: archived done entry and implementation commits confirmed; no source build/test run was needed for this idempotent queue-consumption cycle. Escalation: none. Confidence: high.
### Re: do #gdbtargetsnap — gpt-5

`#gdbtargetsnap` is already complete. The current session status records it as shipped with `#gdbrefreshcache` and `#gdbpathprobe`, and `tasks/software/tsift.done.md` has the archived done entry dated 2026-05-23.

I did not redispatch or rerun the spec/test/build/install/push flow for this queue head. The planning evidence reported `previously_completed=true` for `#gdbtargetsnap`, so this closeout is consuming the reactivated queue entry only.

Commands/proof: ran `agent-doc preflight` and `agent-doc plan`; plan attempted `tsift dispatch-trace` and timed out after 30s, but the partial evidence included `previously_completed=true` and a queue-head drift warning not to redispatch the completed item. Changed paths: session document only. Verification: session status, done archive, and plan evidence confirm this item is already closed; no source build/test run was needed for this idempotent queue-consumption cycle. Escalation: none. Confidence: high.
### Re: do #gdbpathprobe — gpt-5

`#gdbpathprobe` is already complete. The session status records it as shipped with `#gdbrefreshcache` and `#gdbtargetsnap`, and `tasks/software/tsift.done.md` has the archived done entry dated 2026-05-23.

I did not redispatch or rerun source work for this queue head. The planning evidence reported `previously_completed=true`, so this closeout is consuming the final reactivated queue entry and letting the queue drain.

Commands/proof: ran `agent-doc preflight` and `agent-doc plan`; plan attempted `tsift dispatch-trace` and timed out after 30s, but the partial evidence included `previously_completed=true` for `#gdbpathprobe`. Changed paths: session document only. Verification: session status, done archive, and plan evidence confirm this item is already closed; no source build/test run was needed for this idempotent queue-consumption cycle. Escalation: none. Confidence: high.
❯ What are the measured performance improvements?
### Re: measured performance improvements — gpt-5

The big measured win from the last queue run was cached graph reuse on unchanged source rows.

Cold/current superproject baseline on `/home/brian/work/btakita/agent-loop`: real SQLite total `27,515,453us` (`27.5s`), refresh `26,953,139us` (`27.0s`), conflict matrix `520,183us`, dispatch trace `2,380us`, over `946,233` real graph rows.

Three repeated cached samples after the change:
- real SQLite refresh: `317us`, `322us`, `320us` average `319.7us`
- real SQLite total: `519,721us`, `517,194us`, `538,326us` average `525,080us` (`0.525s`)
- dispatch trace: under `1ms` in all three samples

That makes the average total run about `52.4x` faster, a `98.09%` reduction from the cold baseline. Refresh itself went from about `27s` to about `0.00032s`, about `84,300x` faster, a `99.9988%` reduction. `metric-digest` on the latest repeated sample reported `real.sqlite.total_duration_micros` improved `98.16%`, `real.sqlite.refresh.duration_micros` improved `99.9987%`, and `source_graph_build` improved `99.9977%`.

The earlier SQLite hot-path pass had smaller but still useful local wins: `sqlite_delta_write` improved from about `3,856,606us` to `3,237,200us` (`~16%`), real conflict matrix from `37,504us` to `34,979us` (`~6.7%`), real path max-hops from `11,915us` to `11,236us` (`~5.7%`), and the synthetic deep-chain path probe from `219us` to `120us` (`~45%`). The digest for that pass reported `reg:0`, `imp:6`.

Candidate backend promotion stayed on hold: the repeated digest still showed candidate conflict-matrix regressions, so SQLite remains the baseline for this workload.

Commands/proof: ran `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `rg` and `sed` over the session archive/done file, `git show --stat` for `c1c7668` and `1e53fb8`, and a small arithmetic calculation over the recorded benchmark samples. Changed paths: session document only. Verification: answered from the archived benchmark output and recorded metric-digest values; no source build/test run was needed because this was a documentation response only. Escalation: none. Confidence: high.
<!-- agent:boundary:d5aa294a -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbsqlitepgq] [recommended] After first-class edge/entity support lands, run repeated SQLite vs DuckDB/DuckPGQ vs Kuzu backend-eval samples across real, high-degree, and deep-chain projections; update fixture/history and record the promotion/hold decision.
- [ ] [#gdbedgeentity] [recommended] Make graph edges and relationship-like projection rows first-class SQLite graph entities: add stable edge ids, materialized edge-property rows, edge/incident/property scan APIs, graph-db schema/CLI coverage, and conformance/performance tests.
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
