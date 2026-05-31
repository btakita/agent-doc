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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-154043.md`*

Compacted content:
- Archived 3 response topic(s): tsift memory harness support; next-steps for token efficiency + graph/neighborhood performance; queue cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: queue — terse-graph-serial through terse-health-scores — glm-5.1

All 12 queue items implemented, tested, built, installed, committed, and pushed to tsift main (`2bdec90`).

**Completed items:**

| ID | Change | Package |
|---|---|---|
| `#terse-graph-serial` | `TerseGraphNode`, `TerseGraphEdge`, `TerseGraphSubgraph` — strip provenance/freshness, keep id/kind/label/properties | tsift-core |
| `#terse-diagnostics` | `TerseDiagnostic` struct with `code`/`index` fields for compact SQLite query plan codes | tsift-sqlite |
| `#terse-search-hit` | `TerseSearchHit` — strips budget/freshness/provenance/coverage from SearchHit | tsift-search |
| `#terse-health-scores` | `TerseHealthReport`/`TerseHealthScore` — top-N/bottom-N envelope output | tsift-algorithms |
| `#graph-default-edge-lookup` | Override `edge()` with indexed `edge_key` lookup in libsql backend | tsift-libsql |
| `#graph-counts-full-scan` | Override `graph_counts()` with `COUNT(*)` queries in libsql backend | tsift-libsql |
| `#neighborhood-per-edge-node` | Batch node ID resolution per depth level in default `neighborhood()` | tsift-core |
| `#shortest-path-depth-walk` | Store `(depth, parent_id)` tuples in BFS — eliminates O(depth) parent walk | tsift-core |
| `#louvain-scaling` | Pre-compute `ki_in` per node; update incrementally on community moves | tsift-graph |
| `#health-reachability` | Replace per-node BFS with SCC condensation + DAG BFS | tsift-algorithms |
| `#edges-between-chunks` | Temp table + EXISTS subqueries for `edges_between_nodes` in SQLite | tsift-sqlite |
| `#duplicate-callers-fetch` | Fetch callers/callees once, reuse for both display and combined JSON | tsift-cli |

**Verification:** `cargo test` — all 47 package tests pass (tsift-core, tsift-search, tsift-graph, tsift-algorithms, tsift-sqlite, tsift-libsql, tsift-index, tsift-quality, tsift-cli, and reexports). `cargo build --release` + `cargo install --path .` — installed to `~/.cargo/bin/tsift`. Committed as `2bdec90`, pushed to `origin/main`.

### Re: queue re-activation — all 12 items verified — glm-5.1

Queue re-activated with 12 items. All were already implemented and pushed in commit `2bdec90` (552 insertions across 8 packages). Verified:

- **Code**: `TerseGraphNode`/`TerseGraphEdge`/`TerseGraphSubgraph` (tsift-core), `TerseDiagnostic` (tsift-sqlite), `TerseSearchHit` (tsift-search), `TerseHealthReport`/`TerseHealthScore` (tsift-algorithms), `edge_key` indexed lookup (tsift-libsql), `graph_counts()` with `COUNT(*)` (tsift-libsql), `(depth, parent_id)` BFS tuples (tsift-core), `ki_in` cache (tsift-graph), SCC condensation (tsift-algorithms), temp table + EXISTS (tsift-sqlite), caller/callee reuse (tsift-cli)
- **Tests**: `cargo test` — all pass
- **Commit**: `2bdec90` on `origin/main`

All 12 backlog items already resolved.
Fix the preexisting warnings.

### Re: fix preexisting warnings — glm-5.1

Fixed both warnings:

1. **`post_exchange_comment_prompt_preset`**: Removed orphaned `#next-steps` HTML comment block outside `agent:exchange`. These prompts were already addressed in the prior queue run.

2. **`preset_item_id_collision`**: Removed duplicate icebox entries (`#full-e2e-verify`, `#npm-auth-setup`, `#npm-publish-verify`) that already exist in `agent:review` with the same IDs. Review is the authoritative tracker; icebox duplicates caused ambiguous `do #id` resolution.
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.

### Re: #next-steps — token efficiency + graph/neighborhood performance — glm-5.1

Analyzed the full tsift codebase post-`2bdec90`. 10 follow-up items added to backlog in priority order:

#### Token Efficiency
| # | ID | Impact |
|---|---|---|
| 1 | `#terse-ultra-mode` | 25-40% token savings — strip properties, truncate snippets, compact coverage snapshot |
| 5 | `#terse-health-early-exit` | 30% health compute — stop after top/bottom N, skip sub-metrics |
| 7 | `#compact-community-serialization` | 40-60% community JSON — TerseCommunityMember with name only |

#### Graph Performance
| # | ID | Impact |
|---|---|---|
| 2 | `#libsql-pushdown-edge-incident` | 10-50x edge lookup — indexed queries in libsql (parity with SQLite) |
| 3 | `#health-dedup-scc` | 40% health time — share SCC result between reachability and find_cycles |
| 6 | `#libsql-edges-between-pushdown` | 5-20x neighborhood — temp-table + EXISTS in libsql |
| 9 | `#shared-graph-build-pipeline` | 30% analyze time — single graph build shared across all algorithms |
| 10 | `#sift-token-index` | 5-50x search — inverted index pre-filtering |

#### Neighborhood Effectiveness
| # | ID | Impact |
|---|---|---|
| 4 | `#ranked-neighborhood` | 30-60% token reduction — configurable scoring + max_nodes budget |
| 8 | `#louvain-dedup-aggregate` | 5-15% modularity — phase-2 super-node coarsening |

**Top of backlog: `#terse-ultra-mode`** — highest token savings, directly improves LLM context efficiency across all graph/search/explain envelopes.

Also cleaned up orphaned `#next-steps` post-exchange comment (was causing the `post_exchange_comment_prompt_preset` warning).
<!-- agent:boundary:7996afdc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
- What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
<!-- /agent:queue -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#terse-ultra-mode] Ultra-terse envelope mode: strip properties from TerseGraphNode/TerseGraphEdge (keep id/kind/label only), truncate snippets to 80 chars, compact SearchCoverageSnapshot. ~25-40% token savings. tsift-core/lib.rs:145-208, tsift-search/sift.rs:98-131
- [ ] [#libsql-pushdown-edge-incident] Override edge() and incident_edges() in LibsqlGraphStore with indexed queries (parity with SQLite backend). Currently O(E) per call. tsift-libsql/src/lib.rs
- [ ] [#health-dedup-scc] Eliminate duplicate SCC computation in composite_health_score — compute_reachability and find_cycles both run Tarjan on the same graph. Share SCC result. ~40% health-score time reduction. tsift-algorithms/src/health.rs:106-147,213-280
- [ ] [#ranked-neighborhood] Add ranked neighborhood with configurable scoring (BreadthFirst, EdgeKindWeighted, DegreeWeighted) and max_nodes budget. ~30-60% token reduction for explain/neighborhood. tsift-core/lib.rs:598-668
- [ ] [#terse-health-early-exit] Early-terminated terse health report: stop after top/bottom N scores, skip per-node sub-metrics when only overall is needed. ~30% health compute reduction. tsift-algorithms/src/health.rs:42-71
- [ ] [#libsql-edges-between-pushdown] Override edges_between_nodes in LibsqlGraphStore with temp-table + EXISTS (parity with SQLite backend). 5-20x neighborhood speedup. tsift-libsql/src/lib.rs
- [ ] [#compact-community-serialization] Add TerseCommunityMember/TerseCommunity — name + tagpath_handle only, top-N member slice. ~40-60% community JSON savings. tsift-graph/src/lib.rs:541-572
- [ ] [#louvain-dedup-aggregate] Add Louvain phase-2 aggregation (super-node coarsening). 5-15% modularity improvement on hierarchical codebases. tsift-graph/src/lib.rs:590-743
- [ ] [#shared-graph-build-pipeline] Extract shared build_graph() from health.rs to common module; reuse in scc.rs, coupling.rs, dead_code.rs. ~30% analyze wall-time reduction. tsift-algorithms/src/*.rs
- [ ] [#sift-token-index] Build inverted token index for search pre-filtering — skip files with no matching tokens. 5-50x search speedup on large codebases. tsift-search/src/sift.rs:210-267
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
