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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260531-154043.md`*

Compacted content:
- Archived 3 response topic(s): tsift memory harness support; next-steps for token efficiency + graph/neighborhood performance; queue cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:e3a4efb3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to improve token efficiency along with graph + neighborhood performance & effectiveness.
---
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
preset #spec-test-commit-push
[#terse-graph-serial]
[#graph-default-edge-lookup]
[#neighborhood-per-edge-node]
[#terse-diagnostics]
[#terse-search-hit]
[#graph-counts-full-scan]
[#shortest-path-depth-walk]
[#louvain-scaling]
[#health-reachability]
[#edges-between-chunks]
[#duplicate-callers-fetch]
[#terse-health-scores]
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
