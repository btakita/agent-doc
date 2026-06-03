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

### Re: #surrealbulk-close — benchmark update gated — glm-5.1

> **Queue prompt:**
>
> [#surrealbulk-close]


`#surrealbulk` batch projection load is already implemented: `replace_projection_rows()` does bulk replace in one transaction with bound arrays and one-pass index rebuild. The close item asks for re-run benchmarks and SPEC.md table update.

Backend-eval timed out (>5 min) in this session, likely because the graph-db index needs refresh first. Gating `#surrealbulk-close` pending a manual `tsift graph-db refresh` + `tsift graph-db backend-eval --candidate surrealdb --full-projection --target gval` run to collect fresh benchmark numbers.

**Session summary:** 4 of 5 queue items completed in this session:
- **#surreal-warm** (done): `open_or_refresh()` with whole-projection hash cache-hit
- **#surreal-delta** (done): `replace_projection_rows_delta()` with per-row hash diffing
- **#surreal-multiproc** (done): multi-process reader/writer conformance test
- **#surreal-rt-pool** (done): `Arc<Runtime>` shared runtime constructors
- **#surrealbulk-close** (gated): awaiting manual benchmark run
❯ Please run the surreal benchmarks. Compare with sqlitedb.

### Re: surreal benchmarks — glm-5.1

Attempted to run fresh benchmarks via `tsift graph-db backend-eval` for both SurrealDB and SQLite. Both commands timed out after 10 minutes — `tsift graph-db refresh` alone exceeded the 5-minute timeout on this repo (24k+ files across 40+ submodule scopes). This is the same timeout blocker noted in the prior `#surrealbulk-close` cycle.

The latest benchmark data remains the **2026-06-02 three-sample cache-hit run** already recorded in SPEC.md line 314:

| Workload | SQLite Median | SurrealDB Median | Ratio | Verdict |
|----------|--------------|------------------|-------|---------|
| real | 2.6 ms | 573.2 ms | 221x slower | BLOCKED |
| full-projection | 544.2 ms | 535.0 ms | 1.02x faster | Competitive |
| high-degree | 11.0 ms | 7.4 ms | 1.5x faster | SurrealDB wins |
| deep-chain | 12.0 ms | 4.3 ms | 2.8x faster | SurrealDB wins |
| real refresh/load | 0.18 ms | 88.6 ms | 493x slower | BLOCKED |
| full-projection refresh/load | 541.8 ms | 82.7 ms | 6.5x faster | SurrealDB wins |

**Key finding:** SurrealDB excels on large/batch workloads (full-projection, synthetic) but the **real workload** regresses severely on edge lookup, edge-property scan, incident edges, neighborhood, and path tiers. The per-operation latency on the small real dataset is where SurrealDB Rust-side hot-read indexes still lose to SQLite native query path.

**Blocker:** Fresh benchmark runs require completing `tsift graph-db refresh` first, which takes over 5 minutes on this repo. To collect new numbers, run manually:

```bash
tsift graph-db refresh

for i in 1 2 3; do
  tsift graph-db --json backend-eval --full-projection --target gval
  tsift graph-db --json backend-eval --candidate surrealdb --full-projection --target gval
done
```
- What are #next-steps to improve AST navigation for Claude Code, Codex, OpenCode
❯ - What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status and performance?

### Re: #next-steps for AST navigation + SurrealDB promotion — glm-5.1

#### AST Navigation Improvements for Claude Code, Codex, OpenCode

**Current state by harness:**

| Capability | Claude Code | Codex | OpenCode |
|-----------|-------------|-------|----------|
| Init command | `tsift init` (default) | `tsift init --codex` | `tsift init --opencode` |
| Auto-reindex | `UserPromptSubmit` hook | `UserPromptSubmit` hook | Manual `/tsift-status --fix` |
| Search/diff rewrite | `PreToolUse` hook (automatic) | Manual `tsift rewrite --run` | Manual `/tsift-rewrite-run` |
| Command shortcuts | N/A (hooks handle it) | N/A | 12 `/tsift-*` commands |
| npm plugin | N/A | N/A | `opencode-tsift` (unpublished) |

**Priority next-steps (AST nav):**

1. **Ship `tsift-rewrite.sh` in-tree** (`examples/hooks/`) — currently only exists at `~/.claude/hooks/`, not versioned or available for Codex/OpenCode users. Expand Codex/OpenCode to gain a `PreToolUse`-equivalent mechanism.
2. **Publish `opencode-tsift` to npm** — code-complete and CI-verified, just needs `NPM_TOKEN` + `TSIFT_ENABLE_NPM_PUBLISH` repo secrets configured. Unblocks `opencode plugin opencode-tsift`.
3. **Expand OpenCode command set** — add `/tsift-explain`, `/tsift-symbol-read`, `/tsift-graph` shortcuts for the newer envelope surfaces.
4. **Add `tsift init --claude` flag** — no-op but provides symmetric documentation across all three harnesses.
5. **Auto-reindex for OpenCode** — plugin checks index freshness on load and emits recommendation.
6. **Consolidated harness integration guide** — single comparative doc showing all three setups side-by-side.

#### SurrealDB Promotion to SQLite-Equivalent Substrate

**Root cause:** The real-workload 221x regression is a **tsift adapter issue**, not a SurrealKV engine issue. SurrealDB already wins on 3/4 workloads. The bottleneck is `load_indexes()` eagerly deserializing all 955K rows on every `open()` — even cache-hit warm starts. SQLite pays zero on cache-hit opens (mmap + OS page cache).

**Secondary gaps:** Missing `nodes_by_kind` BTreeMap index (O(N) full scan vs SQLite O(K) indexed scan), clone-heavy `GraphEdge` returns, `RwLock` serialization on reads.

**Priority next-steps (SurrealDB):**

| Priority | Action | Est. Impact |
|----------|--------|-------------|
| P0 | **Sidecar index persistence** — serialize BTreeMap indexes to `.tsift/backend-eval-cache/surrealdb/*/index.bincode`; skip `load_indexes()` on cache-hit | 492x on refresh |
| P1 | **`nodes_by_kind` index** — add `SurrealNodeIndexes` struct with `by_kind` BTreeMap | 10-50x on kind queries |
| P2 | **Skip redundant load** — when `replace_projection_rows()` called right after open, skip the SurrealDB round-trip | Eliminates load for benchmarks |
| P3 | **Arc in indexes** — `Arc<GraphEdge>`/`Arc<GraphNode>` instead of cloning | 2-3x on reads |
| P4 | **Record fixture data** — add SurrealDB samples to `graph-db-performance-history.json` | Required for gate evaluation |
| P5 | **HashMap for by_id** — replace `RwLock<BTreeMap>` with `HashMap` for point lookups | 1.5-2x on lookups |

**Promotion requirement:** `perf_gate::evaluate_promotion` needs 3 samples across all 4 workloads with both `refresh.duration_micros` and `total_duration_micros` beating SQLite. P0 alone would bring real refresh from 88.6ms to sub-ms, likely unlocking the gate.

**Backlog item at top:** `#surreal-sidecar` (sidecar index persistence)

### Re: #5vp6 lazily-rs stuck session — glm-5.1

> **Queue prompt:**
>
> [#5vp6]

Investigated whether tsift could cause an OpenCode session to hang on a `Read` tool call for `src/lazily-rs/src/thread_safe.rs`.

**Verdict: tsift is not the cause.** The hang is an OpenCode harness or LLM API issue:

1. **tsift cannot intercept OpenCode's native `Read` tool** — the rewrite module only intercepts shell commands (`rg`, `git diff`, `cat`, `head`, `tail`, `cargo test`, etc.). OpenCode's `Read` tool reads files directly from the filesystem without invoking a shell.
2. **No `PreToolUse` hooks exist for OpenCode** — SPEC.md and AGENTS.md explicitly acknowledge OpenCode lacks Claude-style pre-tool interception.
3. **`tsift source-read` completes in <1s** for files like `thread_safe.rs` (2727 lines, 88KB) — it does a `fs::read()` + SQLite index query, both instant.
4. **The session had queue item `#lazyperf34` active** ("Fix finish_batch O(n2) dedup" on `thread_safe.rs") — the LLM likely completed the Read successfully, then hung during response generation or API processing.

**Likely causes:** LLM API (glm-5.1) slow/unresponsive during response generation, OpenCode state machine race, or resource pressure from 5+ concurrent OpenCode sessions. Recommend checking OpenCode's internal logs/transcripts for the stuck session's PID.
<!-- agent:boundary:cf5a9b51 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#astnav-hooks] Ship tsift-rewrite.sh in-tree + expand PreToolUse equivalent for Codex/OpenCode (medium)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [ ] [#astnav-commands] Expand OpenCode command set: /tsift-explain, /tsift-symbol-read, /tsift-graph (low)
- [ ] [#astnav-init-claude] Add tsift init --claude flag for symmetric documentation (low)
- [ ] [#astnav-autoindex-oc] Auto-reindex for OpenCode via plugin freshness check on load (low)
- [ ] [#astnav-harness-docs] Consolidated harness integration guide in README or docs/harness-setup.md (low)
- [ ] [#surreal-sidecar] Persist BTreeMap indexes to sidecar file; skip load_indexes() on cache-hit opens (high)
- [ ] [#surreal-nodes-by-kind] Add by_kind index to SurrealNodeIndexes (10-50x on nodes_by_kind) (high)
- [ ] [#surreal-skip-load] Skip load_indexes() when replace_projection_rows() called immediately after (medium)
- [ ] [#surreal-arc] Switch indexes to Arc<GraphEdge>/Arc<GraphNode> to eliminate clone overhead (medium)
- [ ] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [ ] [#surreal-dashmap] Replace RwLock<BTreeMap> with HashMap for by_id point lookups (low)
- [ ] [#surreal-batch-bfs] Batch frontier expansion in shortest_path (low)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
