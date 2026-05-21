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

Completed: #gdblock graph-db SQLite lock recovery shipped in tsift 5128c46; superproject gitlink commit 66cd04534 created for closeout push.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-064054.md`*

Compacted content:
- Archived 2 response topic(s): graph evidence blocker; LadybugDB backend question
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Would duckdb with graph extensions or duckdb + ladybugdb be a viable + faster replacement for sqlite?
### Re: DuckDB / Ladybug as SQLite replacement — gpt-5

Short answer: not as a direct replacement for the current `.tsift/graph.db` correctness store. DuckDB or Ladybug could be worth an optional analytical backend, but they do not remove the main `#gdblock` failure mode unless tsift also changes the process model.

The current store is a small provider-neutral `GraphStore` surface: node/edge upserts, kind scans, outgoing-edge scans, bounded neighborhoods, shortest path, projection version rows, and transactional full-projection replacement. The local code keeps that in `src/tsift/src/substrate.rs` with indexed `graph_nodes` / `graph_edges` tables and a delete+insert refresh transaction. That is a good fit for SQLite. The lock bug is more specifically that the graph store does not appear to have the same WAL/busy-timeout/snapshot-recovery discipline already described elsewhere in the tsift spec for index and summary DB reads.

DuckDB is attractive for wide analytical scans and bulk reporting, and DuckPGQ gives DuckDB SQL/PGQ graph query syntax. The catch is that DuckPGQ is a community extension still under active development, and DuckDB's native file concurrency model is one read-write process or many read-only processes. That means a Codex + Claude Code + agent-doc workflow with independent processes can still hit writer/reader coordination problems. It may be faster for large `status`, `drift`, graph analytics, or contract-regeneration workloads, but it is not a clean drop-in for `graph-db evidence` unless benchmarks prove those paths are scan-bound rather than point-lookup/traversal-bound.

Ladybug is the stronger graph-shaped candidate. Its docs position it as an embedded graph DB with Cypher, columnar storage, CSR adjacency/join indices, vectorized/factorized query execution, multicore parallelism, serializable ACID transactions, Rust bindings, and DuckDB attach/import support. That could plausibly beat SQLite for large join-heavy graph workloads. But its concurrency docs have the same strategic limitation for this use case: one read-write database object, or multiple read-only database objects; multi-process read/write is expected to go through a single API-server process. So adopting Ladybug directly would either keep the same class of lock/operator issue or force tsift to introduce a graph broker service.

Recommendation: keep SQLite as the default correctness backend and fix `#gdblock` locally first. The lowest-risk path is WAL mode for `.tsift/graph.db`, a short busy timeout, read-only snapshot fallback including sidecars, and clear lock diagnostics/recovery commands. If we want DuckDB/Ladybug, add them behind the existing `GraphStore` contract as experimental read models and benchmark them against the real graph-db workloads: `refresh`, `status`, `evidence`, `conflict-matrix`, `dispatch-trace`, large synthetic traversal caps, and concurrent agent-doc processes. A replacement should require a measured win and no regression in offline portability, deterministic ordering, crash recovery, and binary install friction.

Sources checked: [DuckDB concurrency](https://duckdb.org/docs/current/connect/concurrency), [DuckDB graph queries / DuckPGQ](https://duckdb.org/docs/lts/guides/sql_features/graph_queries.html), [Ladybug overview](https://docs.ladybugdb.com/), [Ladybug concurrency](https://docs.ladybugdb.com/concurrency/), [Ladybug Rust API](https://docs.ladybugdb.com/client-apis/rust/), [Ladybug DuckDB extension](https://docs.ladybugdb.com/extensions/attach/duckdb/).

Closeout: no repo files changed for this response; no tests run because this was an evaluation-only prompt. Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, targeted `rg`/`sed` inspection, and current primary docs lookup. Preflight warning noted: one legacy gated backlog item remains in `agent:backlog` and can be migrated separately with `agent-doc migrate`.
❯ do [#gdblock]. #spec-test-build-install-commit-push
❯ Would the https://github.com/Vela-Engineering/kuzu be a viable alternative to Ladybug since it supports multiple writers?
### Re: #gdblock and Vela Kuzu — gpt-5

I stopped before source edits because `agent-doc plan` failed closed on the tsift graph orchestration contract: `conflict-matrix` is missing evidence packet `gevd-6d1a7e9566` for target `gdblock`. That means `#gdblock` stays open; I did not run the spec/test/build/install path or mark the item done.

On Kuzu: yes, Vela-Engineering/kuzu is more interesting than Ladybug for this exact concurrency problem, but I would treat it as an evaluation candidate rather than a default replacement. As of May 21, 2026, the Vela fork README explicitly says it adds concurrent multi-writer support for multi-agent systems, while the Vela writeup contrasts that with original Kuzu and Ladybug as single-writer options. The repo also shows a March 2026 `0.12.0-vela` release with Python wheels, so this is not just a stale announcement.

The catch is maturity and blast radius. `#gdblock` is currently a local SQLite lock/recovery problem in tsift's correctness store. The low-risk repair path is still WAL/busy-timeout/read-only snapshot fallback/runbook diagnostics. Replacing the graph store with Vela Kuzu would be a larger storage contract migration: schema mapping, Cypher query layer, deterministic ordering checks, crash recovery behavior, packaged binary/wheel availability for the local install path, and concurrent process stress tests. It may be the best non-SQLite backend to benchmark because its advertised multi-writer model directly targets this workflow; it is not yet evidence that tsift should skip the SQLite repair.

Sources checked: [Vela-Engineering/kuzu README](https://github.com/Vela-Engineering/kuzu), [Vela KuzuDB writeup](https://vela.partners/blog/kuzudb-ai-agent-memory-graph-database), [Vela-Engineering/kuzu releases](https://github.com/Vela-Engineering/kuzu/releases).

Closeout evidence: changed paths: none; commands: `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, current upstream source lookup; verification evidence - not run because planning reported a blocker before repo work; escalation: none; non-session repo commit/push: none.
❯ do [#gdblock]. #spec-test-build-install-commit-push
### Re: graph-db lock item — gpt-5

I could not start implementation. `agent-doc plan` returned a fail-closed planner blocker before source work: `tsift graph evidence failed closed: tsift graph orchestration contract invalid: conflict-matrix missing evidence_packet_id gevd-8b99e549b0 for target gdblock`.

The requested graph-db lock work remains open in the backlog. I made no source/spec/test changes and did not run the build/install/commit/push portion of the preset because the planning contract says to stop on blockers.

Commands run: `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`.

Checks: no project tests were run; no non-session repo files changed; no escalation was used.
❯ Please order the backlog by priority
### Re: backlog priority — gpt-5

Priority order applied:

1. `#gdblock` — recurring graph-db lock path blocks graph-backed orchestration and keeps stopping the requested implementation cycle.
2. `#cmgrwres` — conflict-matrix needs to distinguish previously collected worker results from true missing evidence, which is likely adjacent to the current planner blocker.
3. `#tsdag` — dependency DAG extraction is the core new graph capability once the blocking reliability issues are cleared.
4. `#impactdag` — scheduler fields build on the DAG surface and make antichain dispatch usable.
5. `#dagfix` — regeneration fixtures should land after the DAG and scheduler contracts exist.
6. `#dbbench` — backend benchmarking is useful, but it should follow correctness and lock recovery work.
7. `#orchdog` — already gated by prior dogfood runs; keep it behind the active repair and feature backlog.

Changed paths: session document backlog order only. Commands: `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc finalize --pending-reorder ...`. Checks run: no project test suite, because this was a document-only backlog reorder. Escalation: none.
### Re: #gdblock graph-db lock recovery — gpt-5

Implemented the graph-db SQLite lock recovery path and marked `#gdblock` done.

What changed:
- `SqliteGraphStore` now opens writable `graph.db` with WAL mode, a 5s busy timeout, and a bounded WAL autocheckpoint budget.
- Read-only graph-db paths now use resilient read-only opens. If the live SQLite DB reports a rollback-journal or WAL-sidecar lock, tsift copies the DB plus `-wal` / `-shm` sidecars to a temporary snapshot and keeps `status`, `doctor`, `evidence`, query, conflict-matrix, dispatch-trace, context-pack, Convex sync/drift, and semantic-related reads available.
- `graph-db status --json` now surfaces `recovery`, and human/doctor/warning paths report the snapshot fallback so operators can distinguish a wedged writer from stale graph evidence.
- `SPEC.md` now records WAL/busy-timeout/snapshot fallback as part of the graph-db contract.

Commits:
- `src/tsift`: `5128c46` (`fix: recover graph-db reads from sqlite locks`) pushed to `origin/main`.
- Superproject gitlink: `66cd04534` (`chore: bump tsift graph-db lock fix`) created; push follows this document closeout boundary.

Verification:
- `cargo check`
- `cargo test graph_db_status`
- `cargo test graph_db_evidence_uses_snapshot_fallback_when_graph_db_is_locked`
- `make check` (`cargo clippy --all-targets -- -D warnings` plus full non-ignored test suite; 645 unit tests, 108 exit-code tests, 27 graph-db conformance tests passed; 2 ignored live/wide jobs remained ignored)
- `cargo build`
- `cargo install --path .`; installed `tsift 0.1.42`
- Latest `btakita/tsift` CI run `26209859223` was green for `clippy + test` and live Convex acceptance.
- Latest root `Agent Doc` run `26210824562` had empty-step `tmux-ci` and `check` jobs, so I classified it as an external CI-start blocker rather than a tmux/code regression.
### Re: #gdblock push confirmation — gpt-5

Post-closeout push completed: superproject `main` pushed to `origin/main` through `961cecbb1`, including the `src/tsift` gitlink commit `66cd04534` and the agent-doc closeout commits for `#gdblock`.
<!-- agent:boundary:89bbbd46 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#tsdag]
- do [#impactdag]
- do [#dagfix]
- do [#cmgrwres]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#cmgrwres] [recommended] `tsift conflict-matrix` should downgrade per-target `fail_closed` when the target has a non-zero `worker_result` row count in graph-db. Observed 2026-05-21 dogfood against `tsift.md`: `gdbevid`, `cmgr`, `gdbidx` all have completed `worker_result` rows (1 per item, single-hop path from backlog node) but conflict-matrix still reported `fail_closed: 3 target(s)` because the items lacked live `source_handles`/`owned_files`. A completed worker_result IS evidence the item was successfully dispatched; the dogfood path otherwise cannot re-dispatch a completed item. Fix: downgrade `fail_closed` to a soft `info` signal when `worker_result_count > 0`; surface a new `previously_completed: bool` field per target so operators see both signals.
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
