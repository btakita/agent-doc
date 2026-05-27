---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Added prioritized graph/neighborhood next steps to the tsift backlog, tagged for neighborhood effectiveness and performance. Top backlog item: #gneighqual.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-045155.md`*

Compacted content:
- Archived 6 response topic(s): #gneighgate; closeout repair; #gsemrows; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

❯ Please add #next-steps for graph and tag neighborhood effectiveness and performance.
### Re: graph + neighborhood next steps — gpt-5

Added the requested `#next-steps` into the backlog in priority order, tagged across graph, neighborhood effectiveness, and performance.

Top backlog item: `#gneighqual`.

Priority shape:

1. `#gneighqual` — prove ranked-neighborhood effectiveness before changing defaults.
2. `#gcommqual` — improve tagpath/community context that feeds neighborhood ranking.
3. `#gfullhot` — reduce full-projection graph hot-path costs before backend changes.
4. `#gsqlstage` — reduce SQLite refresh staging/write costs.
5. `#ghoppolicy` — keep higher hop caps behind useful-row and latency evidence.
6. `#gbackendproof` — keep FalkorDB/Kuzu promotion behind native-adapter parity and performance proof.

No source code changed this cycle.
<!-- agent:boundary:6bef512c -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gneighqual] [tag: graph] [tag: neighborhood-effectiveness] [tag: performance] Build a ranked-neighborhood effectiveness gate before making ranked output default: seed real backlog/code queries plus synthetic_multi_module duplicate-name fixtures, compare stable node-id output vs ranked_neighbors top-k, and require handle coverage, duplicate precision, semantic/tagpath readiness, top-community stability, and latency metrics to pass through metric-digest.
- [ ] [#gcommqual] [tag: graph] [tag: neighborhood-effectiveness] Improve tagpath/community context feeding neighborhoods: carry file/ref handles into CommunityMember, resolve tagpath matches through community or edge evidence instead of name-only fallback, and add scoped/federated/skipped-directory diagnostics for ambiguous members.
- [ ] [#gfullhot] [tag: graph] [tag: performance] Reduce the full-projection graph hot path before backend changes: stabilize cache-hit source watermarks, skip provider-neutral row rebuilds on unchanged symbol/summary inputs, and prove full_projection.source_graph_build/projection_rows/cache metrics with three backend-eval --full-projection samples.
- [ ] [#gsqlstage] [tag: graph] [tag: performance] Reduce SQLite graph refresh staging/write cost: bulk merge nodes, edges, and materialized property rows through prepared chunks or temp tables, avoid unchanged property rewrites, and gate on sqlite_delta_write, sqlite_edge_staging, post_write_reads, and duration_micros_per_1k_graph_rows.
- [ ] [#ghoppolicy] [tag: graph] [tag: performance] Keep max-hop increases opt-in until useful-row evidence passes: compare 64/128/256/512 tiers on real, full-projection, and synthetic deep-chain workloads, require useful deeper rows and flat latency, then consider adaptive caps rather than raising the default blindly.
- [ ] [#gbackendproof] [tag: graph] [tag: performance] Evaluate FalkorDB/Kuzu only as native optional GraphStore adapters: require provider-neutral projection writes/load, freshness/parity, lock behavior, install portability, and faster real/full-projection evidence/conflict/dispatch/path metrics than SQLite before promotion.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
