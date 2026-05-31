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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-151545.md`*

Compacted content:
- Archived 6 response topic(s): queued claude-mem migration; #tsiftmemimportall; #tsiftmemsemanticgate; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Does tsift support memory in opencode, codex, and claude code?


### Re: tsift memory harness support — opencode

Yes. tsift memory works across all three harnesses, but the integration layer differs:

| Harness | Integration | Memory access |
|---------|------------|---------------|
| **Claude Code** | `PreToolUse` hooks (`tsift-rewrite.sh`) auto-rewrite high-token commands; `UserPromptSubmit` hook auto-reindexes | Hook contract captures `prompt_target`, `tool_result_artifact`, `response_summary`, `closeout_proof`, `session_check` events via agent-doc closeout pipeline |
| **OpenCode** | `opencode-tsift` npm plugin installs `/tsift-memory-status`, `/tsift-memory-search`, `/tsift-memory-guard` commands | Commands shell out to `tsift memory status`, `tsift graph-db related`, `tsift memory budget-guard` — same `tsift-memory` crate, same `.tsift/memory.db` |
| **Codex** | `tsift init --codex` installs workspace hooks (`packages/tsift-index/src/init.rs`); no `PreToolUse` hooks, so uses `tsift rewrite --run '<command>'` manually | Same memory DB and graph-db retrieval path; `tsift rewrite --run` provides the bounded-output policy that Claude gets automatically via hooks |

The underlying substrate is identical across all three: the `tsift-memory` crate (`packages/tsift-memory/`) with a SQLite-backed `.tsift/memory.db` storing events (prompt targets, tool call artifacts, response summaries, closeout proofs, session checks, imported observations) linked to the graph DB via `memory_graph_links`. The `MemoryHookSpec` contract (`agent_doc_hook_contract()`) defines the capture schema that agent-doc closeout writes regardless of which harness is active.

Key differences:
- Claude Code gets automatic hook-driven capture and rewriting
- OpenCode gets dedicated slash commands via the plugin
- Codex relies on `tsift init --codex` hooks and `tsift rewrite --run` for the same bounded path
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.


### Re: next-steps for token efficiency + graph/neighborhood performance — opencode

Analysis of the tsift codebase identified **12 concrete improvement opportunities** across two dimensions. Below is the prioritized backlog.

#### Token Efficiency (4 items)

| ID | Issue | Location | Impact |
|----|-------|----------|--------|
| `#terse-graph-serial` | GraphNode/GraphEdge include full provenance/freshness in every envelope response | `tsift-core/src/lib.rs:48-102` | ~30-50% of graph output tokens |
| `#terse-search-hit` | SearchHit carries budget/freshness/provenance/coverage per hit | `tsift-search/src/sift.rs:68-82` | ~200 extra tokens per hit |
| `#terse-diagnostics` | SQLite query plan diagnostics are verbose English strings | `tsift-sqlite/src/lib.rs:1525-1540` | 300-500 tokens per paged query |
| `#terse-health-scores` | GraphAnalysisReport includes per-node HealthScore vector | `tsift-cli/src/commands/graph.rs:445-460` | ~120 chars * node_count |

#### Graph + Neighborhood Performance (8 items)

| ID | Issue | Location | Impact |
|----|-------|----------|--------|
| `#graph-default-edge-lookup` | `edge()` loads all edges for single-ID lookup | `tsift-core/src/lib.rs:385-389` | O(N) per edge query on Convex/libsql |
| `#graph-counts-full-scan` | `graph_counts()` deserializes all rows for a COUNT | `tsift-core/src/lib.rs:970-975` | O(N) for a count |
| `#neighborhood-per-edge-node` | `neighborhood()` calls `node()` per traversed edge | `tsift-core/src/lib.rs:499-535` | O(edges * nodes) on Convex |
| `#shortest-path-depth-walk` | `parent_depth()` O(depth) per BFS node | `tsift-core/src/lib.rs:1065-1076` | O(N * max_hops) total |
| `#louvain-scaling` | Community detection O(n*m) per iteration, single-phase | `tsift-graph/src/lib.rs:590-731` | Poor scaling past ~1K nodes |
| `#health-reachability` | Per-node BFS for reachability in health algo | `tsift-algorithms/src/health.rs:59-77` | O(N^2) total |
| `#edges-between-chunks` | `edges_between_nodes` does O(chunks^2) queries | `tsift-sqlite/src/lib.rs:2206-2246` | Many round-trips for large node sets |
| `#duplicate-callers-fetch` | callers/callees fetched twice in JSON mode | `tsift-cli/src/commands/graph.rs:238-291` | 2x edge queries |

#### Priority Order

1. `#terse-graph-serial` — highest token savings, broadest impact across all graph/search/explain output
2. `#graph-default-edge-lookup` — foundational perf fix, unblocks all Convex/libsql backends
3. `#neighborhood-per-edge-node` — neighborhood is the hot path for memory retrieval
4. `#terse-diagnostics` — easy win, self-contained in SQLite module
5. `#terse-search-hit` — second-highest token savings per-query
6. `#graph-counts-full-scan` — quick fix, add COUNT queries to each backend
7. `#shortest-path-depth-walk` — store depth alongside parent in BFS
8. `#louvain-scaling` — incremental ki_in, add phase 2 coarsening
9. `#health-reachability` — SCC condensation or sampled approximation
10. `#edges-between-chunks` — temp table approach
11. `#duplicate-callers-fetch` — reuse edges already fetched
12. `#terse-health-scores` — top-N/bottom-N instead of full vector

Top of backlog: **`#terse-graph-serial`**
<!-- agent:boundary:7769af81 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
- What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#terse-graph-serial] Add terse serialization mode for GraphNode/GraphEdge in envelope output — strip provenance/freshness, keep id/kind/label/properties subset. ~30-50% token savings across search/explain/neighborhood. `tsift-core/src/lib.rs:48-102`
- [ ] [#graph-default-edge-lookup] Override default `edge()` in Convex/libsql backends with indexed single-row lookup instead of loading all edges. `tsift-core/src/lib.rs:385-389`
- [ ] [#neighborhood-per-edge-node] Batch node ID resolution per depth level in default `neighborhood()` — avoid per-edge `node()` calls. `tsift-core/src/lib.rs:499-535`
- [ ] [#terse-diagnostics] Add compact diagnostic codes for SQLite query plan diagnostics in envelope mode. `tsift-sqlite/src/lib.rs:1525-1540`
- [ ] [#terse-search-hit] Strip budget/freshness/provenance/coverage from SearchHit in envelope preview mode. `tsift-search/src/sift.rs:68-82`
- [ ] [#graph-counts-full-scan] Add COUNT queries to each backend instead of deserializing all rows. `tsift-core/src/lib.rs:970-975`
- [ ] [#shortest-path-depth-walk] Store (depth, parent_id) tuples in BFS parent map to eliminate O(depth) walk per node. `tsift-core/src/lib.rs:1065-1076`
- [ ] [#louvain-scaling] Pre-compute ki_in incrementally, add hierarchical coarsening (Louvain phase 2). `tsift-graph/src/lib.rs:590-731`
- [ ] [#health-reachability] Replace per-node BFS with SCC condensation or sampled approximation. `tsift-algorithms/src/health.rs:59-77`
- [ ] [#edges-between-chunks] Use temp table + JOIN for edges_between_nodes instead of chunked IN queries. `tsift-sqlite/src/lib.rs:2206-2246`
- [ ] [#duplicate-callers-fetch] Reuse edges already fetched in graph.rs when --callers + --callees + JSON mode. `tsift-cli/src/commands/graph.rs:238-291`
- [ ] [#terse-health-scores] Emit top-N/bottom-N health scores instead of full vector in envelope mode. `tsift-cli/src/commands/graph.rs:445-460`
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
