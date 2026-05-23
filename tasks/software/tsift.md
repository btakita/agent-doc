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
<!-- agent:boundary:5d395a3b -->
<!-- /agent:exchange -->
###
<!--

#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshcache]
- do [#gdbtargetsnap]
- do [#gdbpathprobe]
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
