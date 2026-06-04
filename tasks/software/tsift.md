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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-150615.md`*

Compacted content:
- Archived 4 response topic(s): #source-body-cap; #diff-parse-cap; #evidence-page; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #terse-edge-codes — glm-5.1

> **Queue prompt:**
>
> do [#terse-edge-codes]


Implemented terse edge kind abbreviation in ultra-terse mode.

**Changes:**
- Added `abbreviate_edge_kind()` in `lib.rs` — maps 37 common edge kinds to short codes (calls->c, defines->d, contains->ct, imports->i, mentions->m, etc.)
- Updated `ultra_terse_transform()` to apply edge kind compression on graph edge `k` fields + strip provenance/freshness from edges
- Updated SPEC.md ultra-terse section with edge kind abbreviation docs and updated expected savings to ~30-50%

**Files changed:** `packages/tsift-cli/src/lib.rs`, `SPEC.md`

**Verification:** 9/9 ultra-terse tests pass (including 3 new: `ultra_terse_abbreviates_edge_kinds`, `ultra_terse_strips_provenance_freshness_from_edges`, updated `ultra_terse_strips_properties_from_graph_edges`). Build + install succeeded. Committed + pushed as `c376242`.

### Re: #sqlite-chunk — glm-5.1

> **Queue prompt:**
>
> do [#sqlite-chunk]


Increased `SQLITE_GRAPH_STAGING_CHUNK_ROWS` from 50 to 500 in `packages/tsift-sqlite/src/lib.rs`.

**Rationale:** 500 rows × 9 edge params = 4500 SQLite variables, well under the 32766 limit in SQLite 3.53.1. This reduces INSERT statement count ~10x (from ~19k to ~1.9k for 952k rows), cutting per-statement overhead significantly.

**Verification:** 13/13 tsift-sqlite tests pass. Updated the `sqlite_projection_refresh_handles_bulk_row_replacement` test assertion to match the new chunk size. Build + install succeeded. Committed + pushed as `2cc5177`.

### Re: #sqlite-ranked-cte — glm-5.1 (HEAD)

Implemented recursive CTE-based `ranked_neighborhood` for `SqliteGraphStore`, eliminating the N+1 `outgoing_edges`/`node` query loop.

**Changes:** `packages/tsift-sqlite/src/lib.rs` — added `ranked_neighborhood` override using a single recursive CTE that:
- Walks from center_id up to configurable depth
- Computes scores inline via SQL CASE expressions for all 3 strategies (BreadthFirst, EdgeKindWeighted, DegreeWeighted)
- Orders by score desc, limits to max_nodes
- Collects edges between retained nodes in the same query

**Verification:** 13/13 tsift-sqlite tests pass. 6/6 ranked_neighborhood conformance tests pass (missing center, BFS max_nodes, edge kind weighted, center inclusion, edge kind filter, degree weighted). Committed + pushed as `ebdb441`.
<!-- agent:boundary:f4b700db -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#tokegate-seed]~
- ~do [#source-body-cap]~
- ~do [#diff-parse-cap]~
- ~do [#evidence-page]~
- ~do [#packet-cache-evict]~
- ~do [#terse-edge-codes]~
- ~do [#sqlite-chunk]~
- ~do [#sqlite-ranked-cte]~
- do [#sqlite-conflict-par]
- do [#sqlite-wal-tune]
- do [#sqlite-frontier-temp]
- do [#astnav-npm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#sqlite-conflict-par] Parallelize conflict_matrix evidence packet preparation — at 817ms it is the slowest hot-read; per-target traversals are read-only and independent (medium)
- [ ] [#sqlite-wal-tune] Tune WAL_AUTOCHECKPOINT_PAGES from 256 to 1024-4096 and measure refresh overhead reduction — current 256 triggers frequent checkpoints during bulk projection write (medium)
- [ ] [#sqlite-frontier-temp] Pre-allocate temp table for multi-node frontier expansion instead of dynamic WHERE IN string per chunk — amortizes overhead for deep BFS traversals (low)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
