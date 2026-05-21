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
Completed: #impactdag and #dagfix shipped in tsift ed28437 and superproject 189dcac98; graph orchestration now exposes scheduler fields and regenerates dependency-DAG replay fixtures.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-212634.md`*

Compacted content:
- Archived 2 response topic(s): #impactdag and #dagfix; #dagfix queue closeout
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#dbbench]. #spec-test-build-install-commit-push
### Re: #dbbench — gpt-5

Implemented `#dbbench` and marked it done.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/src/main.rs`
- `src/tsift/src/substrate.rs`
- `src/tsift/tests/graph_db_conformance.rs`
- `src/tsift/fixtures/graph-db-operator-examples/agent-orchestration-acceptance-pack.json`

What changed:
- Added `tsift graph-db backend-eval` for experimental DuckDB/DuckPGQ and Ladybug backend evaluation.
- Added a read-only experimental `GraphStore` harness over provider-neutral rows and parity/timing reports against SQLite on real and synthetic datasets.
- Covered refresh/load, status, evidence, conflict-matrix, and dispatch-trace operations with promotion decisions, lock/install notes, cached planner inputs, deterministic dispatch-trace traversal, and conformance coverage.

Verification:
- `cargo check`
- `cargo test graph_db_backend_eval_benchmarks_candidate_stores_against_sqlite`
- `cargo test cli_accepts_global_compact_flag`
- `cargo test agent_orchestration_acceptance_pack_regenerates_from_real_queue_fixture --test graph_db_conformance`
- `make check`
- `cargo build`
- `cargo install --path .`
- `tsift --version` -> `tsift 0.1.42`
- GitHub tsift CI `26255514008`: success for live Convex acceptance and full clippy/test.
- Superproject Agent Doc CI `26255527494`: failed before any job steps or logs for `check` and `tmux-ci`; treated as an external CI-start blocker per runbook, not a tmux regression.

Commits and push:
- `btakita/tsift@e8fced7` (`feat: add graph backend evaluation gate`) pushed to `origin/main`.
- `btakita/agent-loop@db58d6c` (`chore: update tsift backend evaluation`) pushed to `origin/main`.

Note: an installed full-workspace smoke against this agent-loop session hit an existing SQLite `database is locked` state, and `tsift status` reported a stale workspace projection. I left that unrelated workspace index state untouched.

Confidence: high for the CLI surface, parity harness, and regression coverage. The DuckDB/DuckPGQ and Ladybug adapters intentionally remain prototype/read-only and promotion-held until a real engine adapter satisfies the gate.
❯ What does the multi-write story look like? Do we need a broker for both duckdb + ladybug and ladybug?
### Re: multi-write story — gpt-5

Short version: no full broker yet, and I would not introduce one just because DuckDB/DuckPGQ and Ladybug exist as candidates. The current system has one real writer: the projection builder writes `.tsift/graph.db` through SQLite in a single transaction, with WAL for concurrent readers and read-only snapshot recovery when the live file is locked. The DuckDB/DuckPGQ and Ladybug backend-eval paths are read-only prototypes that load the same provider-neutral rows and compare query/report parity against SQLite; they are not participating in write durability.

The multi-write shape should stay projection-oriented:

1. Treat repo/session state as the source of truth, not any graph backend.
2. Build one deterministic `GraphProjection` rowset with `projection_version`, content hash, source watermark, and tombstones.
3. Commit the local SQLite read model synchronously, because it is bundled, transactionally simple, and gives the CLI a dependable offline substrate.
4. Fan out the exact same rowset to optional promoted sinks only through an idempotent projection sink/outbox, then verify with drift/parity reports. Failed secondary writes should mark that sink stale, not corrupt or block the local read model.

So the thing we may need later is a small projection committer/outbox, not a distributed transaction broker. A broker becomes justified only if a promoted backend becomes a primary shared writer with independent user-visible state that cannot be regenerated from the repo/session projection. For tsift's graph rows, regeneration and content-hash drift checks are cheaper and safer than two-phase commit.

For the candidates specifically: DuckDB should probably remain an embedded analytics/read-model sink if it ever wins the gate; it does not need a broker. Ladybug might need a single-writer service or queue if its production adapter is remote or multi-process, but that queue can still consume deterministic projection batches. We should promote at most one non-SQLite write sink at a time behind the `GraphStore`/projection contract, keep SQLite as the fallback baseline until the lock/install story is at least as good, and require backend-eval plus drift/doctor evidence before reads default to anything else.
❯ Do we have real performance wins with Ladybug & DuckDB?
### Re: Ladybug and DuckDB performance — gpt-5

Not yet. I ran the current benchmark with `tsift graph-db --path /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --json backend-eval`, and the real dataset result argues against promotion.

On the real agent-loop projection, the dataset was large enough to matter: 349,095 nodes and 576,953 edges. End-to-end totals were:

- SQLite: 25.49s, parity OK.
- DuckDB/DuckPGQ prototype: 32.38s, parity failed because `dispatch_trace` differed from SQLite.
- Ladybug prototype: 40.70s, parity failed for the same `dispatch_trace` mismatch.

There are isolated load/read-count wins: DuckDB loaded the row adapter in 2.64s versus SQLite refresh at 20.70s, and Ladybug loaded in 3.31s. Both were also faster on the simple status/count read. But those wins do not survive the actual operator workload. On real `evidence`, `conflict_matrix`, and `dispatch_trace`, both prototypes were much slower than SQLite, and the dispatch trace row count differed: SQLite returned 223 rows while both candidates returned 219.

The synthetic dataset was tiny by comparison, 96 nodes and 270 edges. Ladybug narrowly beat SQLite there, 3.08ms versus 3.27ms, while DuckDB was 3.53ms. That is not a promotion signal; it is below the scale where the real workload behavior shows up.

So the current answer is: no real performance win yet. We have a useful gate and some hints that a columnar/graph backend might improve projection load or count-style scans, but the existing DuckDB and Ladybug paths are read-only prototypes, slower on the important graph orchestration queries, and not parity-correct on the real dispatch trace. Promotion should stay on hold until a production adapter beats SQLite end-to-end on real data and preserves exact parity.
<!-- agent:boundary:25977229 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
