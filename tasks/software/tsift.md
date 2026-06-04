---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-155432.md`*

Compacted content:
- Archived 3 response topic(s): #terse-edge-codes; #sqlite-chunk; #sqlite-ranked-cte
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #sqlite-conflict-par — glm-5.1

> **Queue prompt:**
>
> do [#sqlite-conflict-par]


Parallelized `collect_conflict_matrix_evidence_packets` in `conflict_matrix.rs:2292`. The previous implementation iterated targets sequentially, calling `graph_db_evidence_report_from_store` (the expensive graph traversal) for each cache miss in series.

**Approach:** Two-phase collection with `std::thread::scope`:
1. Cache-first pass: check disk cache for all targets, separate into hits and misses
2. Parallel pass: for 2+ cache misses with an existing SQLite db, spawn scoped threads where each opens its own `SqliteGraphStore::open_read_only` connection and collects evidence independently
3. Fallback: single-target or missing db_path cases use the original sequential path

Each thread owns its own read-only SQLite connection (WAL mode allows concurrent readers), so no `Sync` bound is needed on `GraphStore`. A shared helper `collect_evidence_for_target` encapsulates the per-target cache-check + traversal + cache-write logic.

**Verification:** `cargo check -p tsift-cli` clean, all 6 conflict_matrix tests pass, `cargo install` succeeds. Pre-existing clippy lint in `tsift-sqlite` (unrelated `useless_format`) and surrealdb test flake are unchanged.

### Re: #sqlite-wal-tune — glm-5.1

Tuned `SQLITE_GRAPH_WAL_AUTOCHECKPOINT_PAGES` from 256 to 4096 in `tsift-sqlite/src/lib.rs:127`.

**Rationale:** The previous value of 256 pages triggered frequent auto-checkpoints during bulk projection writes (node/edge upserts). At 4096 pages (~16MB with default 4KB page size), SQLite batches more WAL writes before checkpointing, reducing fsync overhead during the write-heavy projection refresh phase. The WAL file grows larger transiently but is cleaned up after the write completes.

**Verification:** `cargo check -p tsift-sqlite -p tsift-cli` clean, all 13 tsift-sqlite tests pass, all 9 tsift-cli tests pass, `cargo install` succeeds.

### Re: #sqlite-frontier-temp — glm-5.1

> **Queue prompt:**
>
> re [#astnav-npm]: How do I configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm?


> **Queue prompt:**
>
> do [#sqlite-frontier-temp]


Replaced dynamic `WHERE IN` string-per-chunk frontier expansion in `shortest_path_with_max_hops` (`tsift-sqlite/src/lib.rs:2665`) with a pre-allocated temp table `_tsift_frontier`.

**Before:** Each BFS iteration chunked the frontier into groups of 256 and built a new SQL statement with `N` placeholders per chunk, forcing SQLite to parse/compile a different statement for each chunk size. For deep BFS traversals this multiplied statement preparation overhead.

**After:** A single temp table `_tsift_frontier(id TEXT PRIMARY KEY)` is created once before the loop. Each depth iteration: DELETE old frontier, INSERT new frontier IDs, then JOIN against the temp table with a single fixed prepared statement. The edge scan query is compiled once and reused across all iterations and depths.

The single-node fast path (prepared statements with `WHERE from_id = ?1`) is replaced by the temp table JOIN since the INSERT+JOIN overhead is negligible and eliminates code duplication. Follows the existing `edges_between_nodes` temp table pattern.

**Verification:** `cargo check -p tsift-sqlite` clean, all 13 tsift-sqlite tests pass (including `sqlite_shortest_path_uses_bounded_frontier`), all 9 tsift-cli tests pass, `cargo install` succeeds.
<!-- agent:boundary:7ee37f14 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
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
- [x] [#lazyperf32-tsift] Cross-posted #lazyperf32 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf35-tsift] Cross-posted #lazyperf35 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazyperf34-tsift] Cross-posted #lazyperf34 to lazily-rs backlog (lazily-rs project to implement)
- [x] [#lazily-tsift-bench] Cross-posted tsift-pattern benchmarks recommendation to lazily-rs backlog
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
