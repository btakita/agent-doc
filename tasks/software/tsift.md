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

Blocked: graph-db evidence for #tsdag timed out; queued DAG/lock follow-ups remain open.
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
<!-- agent:boundary:5a728a16 -->
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
- do [#gdblock]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#dbbench] [recommended] Add an experimental GraphStore backend evaluation for DuckDB/DuckPGQ and Ladybug: prototype read-only adapters behind the existing contract, benchmark refresh/status/evidence/conflict-matrix/dispatch-trace on real and synthetic projections, and only promote if it beats SQLite without worsening multi-process lock behavior or install portability.
- [ ] [#tsdag] [recommended] Add graph-level dependency DAG extraction for agent-doc work: derive explicit and implicit edges among backlog ids from file/symbol/test/config overlap, semantic relations, worker_result follow-up ids, and depends-on text; emit nodes, edges, cycle diagnostics, topo batches, replay commands, and repair commands as a versioned JSON contract.
- [ ] [#impactdag] [recommended] Extend conflict-matrix and dispatch-trace with scheduler fields: parallel_safe, blocks, blocked_by, required_context, expected_tests, and stable graph handles so agent-doc can run topological antichains without scraping prose.
- [ ] [#dagfix] [recommended] Add regeneration fixtures for real agent-doc queue DAGs: refresh -> evidence -> conflict-matrix -> dependency-dag -> dispatch-trace, asserting projection freshness, packet ids, worker_result closure, cycle detection, and stale graph fail-closed behavior.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
- [ ] [#cmgrwres] [recommended] `tsift conflict-matrix` should downgrade per-target `fail_closed` when the target has a non-zero `worker_result` row count in graph-db. Observed 2026-05-21 dogfood against `tsift.md`: `gdbevid`, `cmgr`, `gdbidx` all have completed `worker_result` rows (1 per item, single-hop path from backlog node) but conflict-matrix still reported `fail_closed: 3 target(s)` because the items lacked live `source_handles`/`owned_files`. A completed worker_result IS evidence the item was successfully dispatched; the dogfood path otherwise cannot re-dispatch a completed item. Fix: downgrade `fail_closed` to a soft `info` signal when `worker_result_count > 0`; surface a new `previously_completed: bool` field per target so operators see both signals.
- [ ] [#gdblock] [recommended] Graph-db lock recovery path. Observed 2026-05-21 dogfood: `agent-doc jobs create tasks/software/tsift.md --audit` failed with `Error code 5: The database file is locked` because `tsift graph-db --json status` was blocked by a concurrent writer (hot journal + SQLite read-only condition). Operator has no documented way to release the lock short of finding/killing the holder. Same root cause as `agent-doc-bugs2.md` `#w25k`. Fix options: (a) add `tsift graph-db --release-lock` recovery command that inspects pid + hot-journal and rolls back safely; (b) add a documented runbook step ("when locked, check `lsof .tsift/graph.db` and wait for writer to exit"); (c) switch graph-db SQLite to WAL mode so readers don't block on writer journal. Surfaces in any concurrent-agent workflow (codex + claude-code running side-by-side on the same checkout).
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
