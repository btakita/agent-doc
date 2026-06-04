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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260603-234544.md`*

Compacted content:
- Archived 8 response topic(s): #surreal-warm; #surreal-delta; #surreal-multiproc; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: #surreal-sidecar — glm-5.1

> **Queue prompt:**
>
> [ ] [#surreal-skip-load]


> **Queue prompt:**
>
> [ ] [#surreal-nodes-by-kind]


Shipped `#surreal-sidecar`: persist BTreeMap indexes to sidecar file, skip `load_indexes()` on cache-hit opens.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: Added `SidecarData` struct, `try_load_sidecar()`, `write_sidecar()`, and `sidecar_path()` helper. `open_with_runtime()` now tries the sidecar before falling back to `load_indexes()`. `open_or_refresh()` writes sidecar after refresh. `clear()` removes the sidecar. In-memory stores set `path: None` and skip sidecar I/O entirely.
- `SPEC.md`: Added "Index sidecar" paragraph after "Delta projection refresh" documenting the sidecar validation, write, and fallback contract.

**Sidecar design:**
- File: `<store_path>.index-sidecar` (JSON, adjacent to SurrealKV directory)
- Validation: sidecar's `stored_row_hash` must match current SurrealDB metadata hash
- On mismatch/corruption/missing: transparent fallback to `load_indexes()` from SurrealDB
- Edges stored as `Vec<GraphEdge>` (JSON-safe), indexes rebuilt via `SurrealEdgeIndexes::insert()` on load
- Removed on `clear()`, skipped for in-memory stores

**Tests (4 new, 18 total pass):**
- `surrealdb_store_sidecar_skips_load_on_cache_hit`
- `surrealdb_store_sidecar_invalidated_on_refresh`
- `surrealdb_store_sidecar_corrupted_falls_back_to_load`
- `surrealdb_store_sidecar_no_file_for_in_memory`

**Verification:** `make check` — 687 tests pass (all suites green), `cargo install` succeeds, pushed to `main` in both tsift and agent-loop.

### Re: #surreal-arc — glm-5.1

> **Queue prompt:**
>
> [ ] [#surreal-fixture]


> **Queue prompt:**
>
> [ ] [#surreal-arc]


Shipped `#surreal-arc`: switch indexes to `Arc<GraphEdge>`/`Arc<GraphNode>` to eliminate clone overhead.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: `by_id: BTreeMap<String, Arc<GraphEdge>>`, `nodes: BTreeMap<String, Arc<GraphNode>>`. Internal index lookups clone 8-byte Arc pointers instead of deep-cloning full structs. GraphStore trait boundary clones inner data only when returning owned values. Sidecar wraps loaded data in Arc on deserialization.
- `SPEC.md`: Added "Arc-backed indexes" section.

**Verification:** All 18 surrealdb tests pass, `cargo install` succeeds, pushed to `main`.

### Re: #surreal-fixture — glm-5.1

Gated `#surreal-fixture`: requires running `tsift graph-db --json backend-eval --candidate surrealdb` to capture real benchmark samples, which timed out in this queue cycle. The fixture (`fixtures/graph-db-performance-history.json`) has no SurrealDB samples yet. After a successful backend-eval run, append the captured metrics to the fixture under the `surrealdb` key prefix.

### Re: #surreal-dashmap — glm-5.1

> **Queue prompt:**
>
> [ ] [#surreal-batch-bfs]


> **Queue prompt:**
>
> [ ] [#surreal-dashmap]


Shipped `#surreal-dashmap`: replaced `BTreeMap` with `HashMap` for the primary `by_id` edge index.

**Changes:**
- `packages/tsift-surrealdb/src/lib.rs`: `by_id: HashMap<String, Arc<GraphEdge>>` for O(1) amortized point lookups. Secondary ordered indexes remain `BTreeMap` for range-based traversal.
- `SPEC.md`: Updated Arc-backed indexes section noting HashMap.

**Verification:** All 18 surrealdb tests pass, pushed to `main`.

### Re: #surreal-batch-bfs — glm-5.1

Gated `#surreal-batch-bfs`: batch frontier expansion in `shortest_path` requires modifying the shared `shortest_path_using_outgoing` function in `tsift-core` (`packages/tsift-core/src/store.rs:696`). The current BFS calls `outgoing_edges` per-node; batching would need either a new `batch_outgoing_edges` method on `GraphStore` or a modified closure signature. This cross-crate API change warrants a focused design session. The current per-node BFS is efficient for typical graph sizes; the optimization targets deep-chain benchmarks with high fan-out at each level.

### Re: #surreal-batch-bfs — glm-5.1 (HEAD)

Gated `#surreal-batch-bfs`: batch frontier expansion requires cross-crate API change in `tsift-core`. Deferred to focused design session.
<!-- agent:boundary:d3ac3443 -->
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
- [ ] [#surreal-batch-bfs-gated] Batch frontier expansion in shortest_path - gated, requires cross-crate API design (low)
- [ ] [#astnav-hooks] Ship tsift-rewrite.sh in-tree + expand PreToolUse equivalent for Codex/OpenCode (medium)
- [ ] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [ ] [#astnav-commands] Expand OpenCode command set: /tsift-explain, /tsift-symbol-read, /tsift-graph (low)
- [ ] [#astnav-init-claude] Add tsift init --claude flag for symmetric documentation (low)
- [ ] [#astnav-autoindex-oc] Auto-reindex for OpenCode via plugin freshness check on load (low)
- [ ] [#astnav-harness-docs] Consolidated harness integration guide in README or docs/harness-setup.md (low)
- [ ] [#surreal-nodes-by-kind] Add by_kind index to SurrealNodeIndexes (10-50x on nodes_by_kind) (high)
- [ ] [#surreal-skip-load] Skip load_indexes() when replace_projection_rows() called immediately after (medium)
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
- [x] [#surreal-rt-pool-warm] Accept external tokio Runtime in open_or_refresh so backend-eval can reuse one Runtime across candidate stores
<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
