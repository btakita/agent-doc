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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-191758.md`*

Compacted content:
- Archived 3 response topic(s): #opencode-read; #opencode-grep; #codex-read
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #surreal-warm — warm-start SurrealDB store — glm-5.1

> **Queue prompt:**
>
> [#surreal-warm]


Implemented `SurrealdbGraphStore::open_or_refresh()` with blake3 row-hash diffing. The method opens the file-backed SurrealKV store, hashes incoming `ConvexProjectionRows`, and compares against a stored hash in the SurrealDB `metadata` table. On cache hit (`WarmStartOutcome::CacheHit`), the store reuses existing rows and derived indexes without a transactional bulk replace or index rebuild. On miss (`WarmStartOutcome::Refreshed`), it runs the full `replace_projection_rows()` path and persists the new hash.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: added `row_hash()`, `MetadataRecord`, `WarmStartOutcome` enum, `stored_row_hash()`, `set_stored_row_hash()`, and `open_or_refresh()` public method. Added 2 tests: `surrealdb_store_open_or_refresh_cache_hit_skips_replace` and `surrealdb_store_open_or_refresh_detects_changed_rows`.
- `packages/tsift-surrealdb/Cargo.toml`: added `serde_json` dependency for `to_vec`.
- `packages/tsift-cli/src/lib.rs`: backend-eval SurrealDB path now calls `open_or_refresh` and reports `warm_start: "cache_hit"` in refresh metadata.
- `SPEC.md`: documented warm-start behavior under SurrealDB Backend Spike section.

**Verification:** 7/7 surrealdb tests pass, `make check` (clippy + workspace tests) passes, CLI builds with `backend-surrealdb` feature, `cargo install --debug` succeeds. Commit `bfa0493` pushed to `main`.

### Re: #surreal-delta — delta projection refresh — glm-5.1

> **Queue prompt:**
>
> [#surreal-delta]


Implemented `SurrealdbGraphStore::replace_projection_rows_delta()` with per-row blake3 hash diffing matching SQLite's `replace_projection_with_version()` pattern. Each node and edge is individually hashed; incoming hashes are compared against stored per-row hashes loaded from SurrealDB records. Only changed, new, or tombstoned rows trigger SurrealDB writes: one transactional batch deletes removed + changed rows then inserts changed/new rows. In-memory indexes update incrementally instead of rebuilding from scratch.

`open_or_refresh()` now uses the delta path instead of full `replace_projection_rows()` on refresh, so repeated backend-eval runs against partially-changed data only write the diff.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: added `DeltaRefreshStats`, `replace_projection_rows_delta()`, `delta_surreal_records()`, `delta_memory_indexes()`, `update_row_hashes()`, `all_edge_keys()`, `node_row_hashes`/`edge_row_hashes` fields, `row_hash` field on `SurrealNodeRecord`/`SurrealEdgeRecord`. 3 new tests: skip-unchanged, detect-modified, tombstone-removed.
- `SPEC.md`: documented delta projection refresh under SurrealDB Backend Spike section.

**Verification:** 10/10 surrealdb tests pass, `make check` (clippy + workspace tests) passes, commit `eb7038f` pushed to `main`.

### Re: #surreal-multiproc — multi-process lock conformance test — glm-5.1

> **Queue prompt:**
>
> [#surreal-multiproc]


Added `surrealdb_store_multi_process_reader_writer_lock` conformance test that spawns 3 concurrent reader processes and 1 writer process against the same file-backed SurrealKV store. The test verifies: (1) concurrent reads succeed without contention errors, (2) writes persist across process boundaries, (3) no data corruption occurs. Readers observe a consistent snapshot from open time; stale reads are expected and acceptable for backend-eval's read-only measurement phase.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: added 3 tests (orchestrator, reader, writer) using `std::process::Command` to spawn child processes that open the same SurrealKV path.
- `SPEC.md`: documented multi-process lock behavior findings under promotion boundary.

**Verification:** 13/13 tests pass, `make check` passes, commit `3f661d6` pushed.

### Re: #surreal-rt-pool — shared tokio runtime — glm-5.1

`SurrealdbGraphStore` now stores its tokio runtime as `Arc<Runtime>` and exposes `open_with_runtime(path, rt)` and `in_memory_with_runtime(rt)` constructors that accept a shared runtime. This allows backend-eval or other callers to reuse one runtime across multiple store instances instead of paying the per-store `Runtime::new()` cost. Default `open()` and `in_memory()` continue to create a fresh runtime for backward compatibility.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: changed `rt` field to `Arc<Runtime>`, added `open_with_runtime`, `in_memory_with_runtime`, `create_runtime` helper, and shared-runtime test.
- `SPEC.md`: documented shared runtime constructors.

**Verification:** 14/14 tests pass, `make check` passes, commit `c088055` pushed.
<!-- agent:boundary:de8508a4 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" -->
- ~[#surreal-warm]~
- ~[#surreal-delta]~
- ~[#surreal-multiproc]~
- [#surreal-rt-pool-warm]
- [#surreal-rt-pool]
- [#surrealbulk-close]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
- [ ] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
