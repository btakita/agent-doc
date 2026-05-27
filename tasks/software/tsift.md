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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Completed #gcommqual; verified community member tagpath context is already implemented and pushed in tsift. Local make check, cargo build, and cargo install pass.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-065332.md`*

Compacted content:
- Archived 6 response topic(s): graph + neighborhood next steps; #gneighqual; closeout repair; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:195ed4bb -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gneighqual]~
- ~do [#gcommqual]~
- do [#gfullhot]
- do [#gsqlstage]
- do [#ghoppolicy]
- do [#gbackendproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
