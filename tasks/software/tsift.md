---
agent_doc_session: tsift-v0.1
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-191524.md`*

Compacted content:
- Archived 1 response topic(s): #ranked-neighborhood
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #terse-health-early-exit — glm-5.1

> **Queue prompt:**
>
> [#terse-health-early-exit]


Rewrote terse_health_report. All 15 tests pass. Committed as 5cf8093, pushed to main.

### Re: #libsql-edges-between-pushdown — glm-5.1

> **Queue prompt:**
>
> [#libsql-edges-between-pushdown]


Overrode `edges_between_nodes` in `LibsqlGraphStore` with temp-table + EXISTS pushdown, matching the SQLite backend pattern (`tsift-sqlite/src/lib.rs:2235`).

**Changes:**
- `tsift-libsql/src/lib.rs` — added `edges_between_nodes` override using `CREATE TEMP TABLE _edges_between_ids`, batch inserts (chunks of 450), and `EXISTS` subqueries against `graph_edges`
- `tsift-libsql/src/lib.rs` — added `libsql_store_edges_between_nodes_pushdown` test verifying scoped filtering, empty set, and single-node edge cases
- `SPEC.md` — updated pushdown queries section to document the new `edges_between_nodes` override

**Verification:** 8/8 tests pass. Built and pushed as `4693e36`.

### Re: #compact-community-serialization — glm-5.1

> **Queue prompt:**
>
> [#compact-community-serialization]


Added `TerseCommunityMember`, `TerseCommunity`, and `TerseCommunityResult` types to `tsift-graph/src/lib.rs` for compact community serialization.

**Types:**
- `TerseCommunityMember` — `name` + `tagpath_handle` only (strips `file`, `line`, `refs`)
- `TerseCommunity` — wraps top-N `TerseCommunityMember` slice + `id` + `modularity_contribution`
- `TerseCommunityResult` — terse result with same aggregate fields as `CommunityResult`
- `CommunityResult::to_terse(top_n)` — converts full result to truncated terse form

**Tests added:** 4 new tests — field stripping, top-N truncation, `to_terse` from `detect_communities`, member count reduction.

**Spec updated:** Community Detection section + tsift-graph crate listing.

**Verification:** 71/71 tests pass. Built and pushed as `26762b0`.

### Re: #louvain-dedup-aggregate — glm-5.1

> **Queue prompt:**
>
> [#louvain-dedup-aggregate]


Added Louvain phase-2 super-node coarsening to `detect_communities` in `tsift-graph/src/lib.rs`.

**Algorithm changes:**
- Extracted `LouvainGraph` struct with `phase1()` (returns community assignment) and `coarsen()` (collapses communities into weighted super-nodes with self-loops)
- Phase 2 loop: coarsen graph → run phase 1 on super-nodes → map back to original nodes → repeat up to 10 levels until no improvement
- Self-loops in coarsened graph carry half the internal edge weight, matching standard Louvain degree accounting

**Spec updated:** Algorithm section now describes the multi-level process (phase 1 local moving, phase 2 coarsening, repeat).

**Test added:** `communities_hierarchical_phase2_improves_modularity` — 4 clusters of 6 nodes each with bridging edges, verifies ≥ 2 communities and positive modularity.

**Verification:** 72/72 tests pass. Built and pushed as `db4d690`.

### Re: #shared-graph-build-pipeline — glm-5.1

> **Queue prompt:**
>
> [#shared-graph-build-pipeline]


Extracted shared `graph_builder` module from duplicated graph construction code in health.rs, dead_code.rs, and scc.rs.

**New file: `tsift-algorithms/src/graph_builder.rs`**
- `Graph` struct — holds `node_vec`, `node_idx`, `out_adj`, `in_adj` with `node_count()`, `edge_count()`, `ensure_nodes()` helpers
- `build_graph(edges)` — shared directed adjacency construction with self-loop exclusion (replaces 30-line duplicate in health.rs and 28-line duplicate in dead_code.rs)
- `build_node_index(edges)` — shared node extraction only (replaces 9-line duplicate in scc.rs)

**Refactored files:**
- `health.rs` — replaced private `build_graph` with `crate::graph_builder::build_graph`; removed ~30 lines
- `dead_code.rs` — replaced inline graph building with `build_graph` + `ensure_nodes` for entry_points; removed ~28 lines
- `scc.rs` — replaced inline node extraction with `build_node_index`; removed ~9 lines
- `lib.rs` — added `pub mod graph_builder` + re-exports for `Graph`, `build_graph`, `build_node_index`

**Tests:** 7 new tests in graph_builder module. All 45/45 tests pass.

**Spec updated:** SPEC.md tsift-algorithms section now lists `graph_builder module`.

**Verification:** Built, installed, committed as `ea0f592`, pushed to main.

### Re: #sift-token-index — glm-5.1

> **Queue prompt:**
>
> [#sift-token-index]


Added inverted `TokenIndex` for search pre-filtering — skips files with no matching query tokens.

**New types in `tsift-search/src/sift.rs`:**
- `TokenIndex` — maps tokens to file sets; serializable to JSON for caching
- `TokenIndex::build(root)` / `load(path)` / `save(path)` / `files_matching_any(tokens)`

**Search changes:**
- `Sift::search()` calls `load_or_build_token_index()` — loads cached `token-index.json` or builds+persists
- Pre-filters candidate files; only reads+scores files with matching tokens
- Empty queries bypass token filter

**Tests:** 8 new tests. All 19/19 pass. Committed as `9047134`, pushed.
<!-- agent:boundary:95dff547 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
