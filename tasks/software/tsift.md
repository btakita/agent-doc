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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260604-174501.md`*

Compacted content:
- Archived 1 response topic(s): #temp-table-audit
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve the sqlite graph performance? What are #next-steps improve token efficiency of tsift using the graphs?
### Re: sqlite graph performance next-steps — glm-5.1

#### Hot-read performance (already strong)

SQLite dominates hot reads: `edge_lookup` 15.7ms, `incident_edges` 343us, `path_max_hops` 65us. SurrealDB is 220x slower on real workloads. The main gap is **projection refresh** at ~22s vs candidates at ~3s.

#### #refresh-skip-unchanged-staging — Projection refresh skip-unchanged staging

Current bottleneck: `refresh()` inserts ALL rows into 6 temp tables, then delta-compares via `row_hash`. For a 952k-row projection where <1% changed, ~950k temp INSERTs are discarded. **Fix:** add a pre-staging hash-bloom or row-hash comparison pass that only stages rows whose `row_hash` differs from the incoming hash, skipping the temp INSERT entirely for unchanged rows. Measured target: cut refresh from 22s to under 5s for <1% change rates.

#### #degree-cache — DegreeWeighted scoring cache

`DegreeWeighted` scoring (`lib.rs:2309`) runs a correlated subquery `(SELECT COUNT(*) FROM graph_edges …)` for every node discovered during recursive walk — O(nodes × edges). **Fix:** pre-compute and cache degree counts in a temp table or `graph_operator_stats` before the recursive CTE. For `ranked_neighborhood` with high connectivity, this eliminates thousands of repeated scans.

#### #edges-between-batch-optimization — edges_between_nodes batch optimization

Every `edges_between_nodes` call creates a temp table, inserts IDs in batches of 450, runs EXISTS queries, then drops the table. **Fix:** use a session-scoped temp table or IN-clause expansion for small ID sets (<50 nodes) to avoid temp table creation overhead entirely.

#### #wal-checkpoint-tuning — WAL checkpoint tuning

`wal_autocheckpoint = 4096` pages is conservative. After large refreshes, the WAL may grow significantly, slowing subsequent reads. **Fix:** add an explicit `PRAGMA wal_checkpoint(TRUNCATE)` after successful refresh completes, and tune `wal_autocheckpoint` based on projection size.

### Re: token efficiency next-steps — glm-5.1

#### #terse-uniform — Enforce terse serialization on all agent-facing surfaces

`TerseGraphNode`/`TerseGraphEdge` exist but aren't used uniformly. Some envelope outputs still serialize full `GraphNode` with provenance/freshness. **Fix:** audit all envelope output paths and ensure every agent-facing surface uses terse types. Estimated 15-25% token reduction on affected surfaces.

#### #edge-index-references — Compress edge output with node index references

Large neighborhoods serialize edges with full `from_id` and `to_id` strings. For a 100-node, 300-edge result, that's 600 redundant ID strings. **Fix:** emit a `nodes` array and reference edges by positional index (`[0, 5, "calls"]`) instead of full IDs. Estimated 30-50% edge-output token reduction.

#### #community-summary-default — Community-aware truncation as default

Community detection (`tsift-graph`) already produces `CommunityResult` with terse member lists. When `max_nodes` truncates results, replace the tail with community summaries showing community label + member count + top-3 handles, rather than silently dropping nodes. **Fix:** integrate community detection into neighborhood output as a "truncation summary" section. This gives the agent coarse-grained awareness of what was pruned.

#### #property-sampling — Property sampling for large result sets

Full property maps are serialized for every node/edge in result sets. For `kind`-heavy schemas where most properties are redundant across same-kind nodes, sample properties from the first N nodes of each kind and omit from the rest. **Fix:** add `property_mode: full | sample | omit` to `RankedNeighborhoodOptions` and envelope output options.

#### #ranked-default-promotion — Promote ranked_neighborhood to default

SPEC.md #rankdefault already tracks this. `ranked_neighborhood` achieves 30-60% token reduction vs unranked traversal. **Next step:** run the quality gates from the review item (useful-hit density, duplicate-name precision, handle coverage) on real backlog/code queries and promote if metrics pass.
<!-- agent:boundary:24536aa3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
- ~do [#frontier-unique-temp]~
- do [#temp-table-audit]
- do [#sqlite-doc-sync-safety]
- do [#sqlite-reentrant-guard]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#refresh-skip-unchanged-staging] Pre-staging row-hash comparison to skip unchanged rows before temp INSERT in projection refresh — target <5s for <1% change rates (high)
- [ ] [#terse-uniform] Audit and enforce TerseGraphNode/TerseGraphEdge on all agent-facing envelope output paths — est 15-25% token reduction (high)
- [ ] [#edge-index-references] Compress edge output with positional node index references instead of full ID strings — est 30-50% edge token reduction (medium)
- [ ] [#degree-cache] Pre-compute and cache degree counts for DegreeWeighted ranked_neighborhood scoring (medium)
- [ ] [#community-summary-default] Community-aware truncation summary in neighborhood output when max_nodes prunes results (medium)
- [ ] [#property-sampling] Add property_mode (full|sample|omit) to RankedNeighborhoodOptions for kind-redundant property maps (low)
- [ ] [#edges-between-batch-optimization] Use IN-clause expansion or session-scoped temp table for small edges_between_nodes ID sets (low)
- [ ] [#wal-checkpoint-tuning] Add explicit PRAGMA wal_checkpoint(TRUNCATE) after refresh; tune autocheckpoint by projection size (low)
- [ ] [#sqlite-doc-sync-safety] Add doc comments on SqliteGraphStore/SqliteReadOnlyConnection stating single-active-call-per-connection invariant for temp-table methods (medium)
- [ ] [#sqlite-reentrant-guard] [recommended] Add runtime Cell<bool> guard that panics on re-entrant temp-table calls on same connection (low)
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
